//! The `diagnostics` view's rows with their fix-its, the
//! `spans` view of what each diagnostic points at, and the `fixed` view:
//! the case after its fix-its, checked again.
//!
//! Each reads the build's diagnostic columns once: a row's fix-its and
//! edits are ranges into pooled columns, so rendering is linear in the
//! diagnostics and their parts. The `fixed` view applies one fix-it
//! alternative at a time, with the edits of every file sorted once and
//! applied in one pass per file, then checks the edited package once per
//! alternative.

use super::{Build, Program};
use hd_base::SliceSort as _;
use hd_base::Span;
use hd_diag::FixSafety;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A span as `file:line:column lo..hi`: where it starts, for the eye, and
/// its exact byte range, as an edit holds it.
fn span(out: &hd_driver::Output, s: Span) -> String {
    let (file, line, column) = out.locate(s);
    format!("{file}:{line}:{column} {}..{}", s.lo, s.hi)
}

/// Every diagnostic of `out` as `severity: file:line:column: code:
/// message`, in content order, each followed by its fix-its (`fix
/// (exact|suggestion): title`, then each edit as `edit SPAN:
/// "replacement"`). Labels are not shown: no stage of the compiler adds
/// one today, so a case could never reach them. Each message and fix
/// title is the user's text (`Output::user_text`), as `hd check` prints
/// it: a package by its name, never its internal `money$2` root, and test
/// code by its file. Edits stay as written.
pub(super) fn render(out: &hd_driver::Output) -> String {
    let d = &out.diags;
    let at = |s: Span| {
        let (file, line, column) = out.locate(s);
        format!("{file}:{line}:{column}")
    };
    let mut text = String::new();
    let mut row = String::new();
    for i in d.content_order() {
        row.clear();
        d.render_row(i, &at, &mut row);
        text.push_str(&out.user_text(&row));
        for f in d.fixes[i].range() {
            let safety = match d.fix_safety[f] {
                FixSafety::Exact => "exact",
                FixSafety::Suggestion => "suggestion",
            };
            let title = out.user_text(d.get_text(d.fix_title[f]));
            let _ = writeln!(text, "  fix ({safety}): {title}");
            for e in &d.edits[d.fix_edits[f].range()] {
                let _ = writeln!(
                    text,
                    "    edit {}: {:?}",
                    span(out, e.span),
                    d.get_text(e.text)
                );
            }
        }
    }
    text
}

/// The `spans` view: each diagnostic of the `diagnostics` view's build,
/// in content order, as its code, its primary span and the text that span
/// covers, which the `diagnostics` view's `file:line:column` start alone
/// does not show.
pub(super) fn spans(p: &Program, build: Build) -> String {
    let out = super::check(p, build, &p.sources());
    let d = &out.diags;
    let mut text = String::new();
    for i in d.content_order() {
        let s = d.primary[i];
        let covered = &out.texts[s.file.idx()][s.lo as usize..s.hi as usize];
        let code = d.code[i].as_str();
        let _ = writeln!(text, "{code} {}: {covered:?}", span(&out, s));
    }
    text
}

/// One edit to apply: its file's index in the build, its byte range and
/// its replacement.
struct Edit<'a> {
    file: usize,
    lo: usize,
    hi: usize,
    text: &'a str,
}

/// The `fixed` view: for each fix-it alternative `k` (1 up, to the most
/// any diagnostic offers), the case with the `k`th fix-it of every
/// diagnostic that has one applied, as the lines the edits wrote, then
/// the diagnostics of checking it again as the `diagnostics` view does.
/// As `hd check --fix` plans (`cli.check.fix.overlap`), a fix-it whose
/// edit overlaps an edit of one taken before it, in content order, is
/// left out and named.
///
/// Cost per alternative: one check of the case, plus O(e log e) for its
/// e edits (each overlap test is two ordered-map probes) and one pass over
/// each edited file.
pub(super) fn fixed(p: &Program, build: Build) -> String {
    let texts: BTreeMap<String, String> = std::iter::once((p.main_file(), p.text.clone()))
        .chain(p.files.iter().cloned())
        .collect();
    let first = super::check(p, build, &sources(&texts));
    let d = &first.diags;
    let most = d.fixes.iter().map(|f| f.len).max().unwrap_or(0);
    if most == 0 {
        return "no fix-it\n".to_owned();
    }
    let order = d.content_order();
    let mut out = String::new();
    for k in 0..most {
        let _ = writeln!(out, "-- fix {}", k + 1);
        // The taken edits' ranges, by file and start.
        let mut taken = BTreeMap::<(usize, usize), usize>::new();
        let mut edits: Vec<Edit<'_>> = Vec::new();
        for &i in &order {
            let fixes = d.fixes[i];
            if fixes.len <= k {
                continue;
            }
            let f = (fixes.start + k) as usize;
            let own = &d.edits[d.fix_edits[f].range()];
            if own.iter().any(|e| overlaps(&taken, e.span)) {
                let _ = writeln!(
                    out,
                    "fix-it left out: {}: {}",
                    span(&first, d.primary[i]),
                    d.get_text(d.fix_title[f])
                );
                continue;
            }
            for e in own {
                let (file, lo, hi) = (e.span.file.idx(), e.span.lo as usize, e.span.hi as usize);
                taken.insert((file, lo), hi);
                edits.push(Edit {
                    file,
                    lo,
                    hi,
                    text: d.get_text(e.text),
                });
            }
        }
        edits.sort_stable_by_key(|e| (e.file, e.lo, e.hi));
        let mut edited = texts.clone();
        for run in edits.chunk_by(|a, b| a.file == b.file) {
            let name = &first.files[run[0].file];
            let (after, written) = apply(&texts[name], run);
            show_lines(name, &after, &written, &mut out);
            edited.insert(name.clone(), after);
        }
        let again = super::check(p, build, &sources(&edited));
        let _ = writeln!(out, "-- check");
        let rendered = render(&again);
        out.push_str(if rendered.is_empty() {
            "no diagnostics\n"
        } else {
            &rendered
        });
    }
    out
}

/// Whether an edit at `s` overlaps a taken one: they share a byte, or
/// start at the same byte, so their order would decide the result. Taken
/// ranges overlap nowhere, so only the one starting last before `s` can
/// reach into it.
fn overlaps(taken: &BTreeMap<(usize, usize), usize>, s: Span) -> bool {
    let (file, lo, hi) = (s.file.idx(), s.lo as usize, s.hi as usize);
    taken
        .range((file, lo)..(file, hi.max(lo + 1)))
        .next()
        .is_some()
        || taken
            .range(..(file, lo))
            .next_back()
            .is_some_and(|(&(f, _), &end)| f == file && end > lo)
}

fn sources(texts: &BTreeMap<String, String>) -> hd_project::MemorySources {
    let mut sources = hd_project::MemorySources::default();
    for (name, text) in texts {
        sources.insert(name, text);
    }
    sources
}

/// `before` with `edits` (one file's, sorted by range, overlapping
/// nowhere) applied in one pass, and the byte range each edit's text
/// takes in the result.
fn apply(before: &str, edits: &[Edit<'_>]) -> (String, Vec<(usize, usize)>) {
    let mut after = String::with_capacity(before.len());
    let mut written = Vec::with_capacity(edits.len());
    let mut copied = 0;
    for e in edits {
        after.push_str(&before[copied..e.lo]);
        let start = after.len();
        after.push_str(e.text);
        written.push((start, after.len()));
        copied = e.hi;
    }
    after.push_str(&before[copied..]);
    (after, written)
}

/// Each line of `text` that a range of `written` touches, once, in order,
/// as `file:line: text`. Line starts are found in one pass and each range
/// located by binary search.
fn show_lines(name: &str, text: &str, written: &[(usize, usize)], out: &mut String) {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |offset: usize| starts.partition_point(|&s| s <= offset) - 1;
    let mut lines = std::collections::BTreeSet::new();
    for &(lo, hi) in written {
        lines.extend(line_of(lo)..=line_of(hi.saturating_sub(1).max(lo)));
    }
    for line in lines {
        let end = starts.get(line + 1).map_or(text.len(), |&next| next);
        let shown = text[starts[line]..end].trim_end_matches(['\n', '\r']);
        let _ = writeln!(out, "{name}:{}: {shown}", line + 1);
    }
}
