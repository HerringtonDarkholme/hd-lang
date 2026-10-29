//! The fixing pass shared by `hd check --fix` and `hd lint --fix`: check the
//! package, apply eligible fix-its, and check again, in rounds.
//! Edits are tried in memory, through `DiskSources::set_text`, and only
//! the files they change are written at the end, each through a temporary
//! file. `crates/hd_cli/DESIGN.md` §7.6 has the design.
//!
//! A fix-it is tried when the compiler marks it exact and every edit lies
//! in a package source file, or when a missing-capability fix inserts
//! grants into its `hd.toml`. Dependencies and `std` are never written.
//! It is safe when the check after it has no diagnostic
//! that the check before it lacked, counting diagnostics by code,
//! severity, file and message (`cli.check.fix.safe`); a syntax error is a
//! diagnostic too. A round first tries all its fix-its together; when that
//! adds a diagnostic, it tries them one at a time in output order, each on
//! top of those already kept, so each added diagnostic is charged to the
//! fix-it that brought it.

use hd_base::SliceSort as _;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_project::SourceSet as _;

use crate::fmt_cmd::line_column;
use crate::report::{Diag, Edit};
use crate::{HD_FAILURE, check_cmd, disk, fail, package_of_cwd};

#[derive(Clone, Copy)]
enum Mode {
    Check,
    Lint,
}

impl Mode {
    /// Lint autofixes have the stronger, semantic bar from #805. Today the
    /// only proven behavior-preserving lint edit removes a redundant `mut`.
    fn accepts(self, code: Option<hd_diag::Code>) -> bool {
        match self {
            Self::Check => true,
            Self::Lint => code == Some(hd_diag::Code::RedundantLetMut),
        }
    }
}

/// The most rounds of check and apply that one run makes
/// (`cli.check.fix.rounds`).
const ROUNDS: usize = 3;

/// The texts of the package's source files, by the path their
/// diagnostics name.
type Texts = BTreeMap<String, String>;

/// What tells one diagnostic from another across an edit, which moves
/// positions: its code, whether it is an error, its file and its message.
type Key = (Option<&'static str>, bool, Option<String>, String);

fn key(d: &Diag) -> Key {
    (
        d.code.map(hd_diag::Code::as_str),
        d.is_error(),
        d.file.clone(),
        d.message.clone(),
    )
}

/// One fix-it to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Change {
    /// The file of its first edit, and that edit's line and column in the
    /// text it applies to.
    file: String,
    line: usize,
    column: usize,
    /// Its diagnostic.
    diag: Key,
    /// What the fix-it does.
    message: String,
    edits: Vec<Edit>,
}

impl Change {
    /// The line `--fix` prints for it.
    fn text(&self) -> String {
        let code = self.diag.0.map_or_else(String::new, |c| format!("{c}: "));
        format!(
            "fixed: {}:{}:{}: {code}{}",
            self.file, self.line, self.column, self.message
        )
    }
}

/// What the rounds did: the final texts, each change in order, and the
/// diagnostics of the last check, which match the final texts.
struct Outcome {
    texts: Texts,
    changes: Vec<Change>,
    remaining: Vec<Diag>,
}

pub(crate) fn command(command: &str, lint: Option<hd_lint::Policy>) -> ExitCode {
    let mode = if lint.is_some() {
        Mode::Lint
    } else {
        Mode::Check
    };
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => return fail(&e.to_string()),
    };
    if disk::workspace_root(&cwd).is_some() {
        return fail(&format!(
            "`hd {command} --fix` works on one package, and this is a workspace root; run it in a member's directory"
        ));
    }
    let root = match package_of_cwd(command) {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    let sources = match disk::sources_of(&root) {
        Ok((s, _)) => s,
        Err(e) => return fail(&e),
    };
    // Each source file: its listed path and where it is on disk.
    let mut files: BTreeMap<String, (String, PathBuf)> = BTreeMap::new();
    let mut original = Texts::new();
    for e in sources.list() {
        let shown = sources.display(&e.path);
        let path = sources.disk_path(&e.path);
        if let Ok(text) = std::fs::read_to_string(&path) {
            original.insert(shown.clone(), text);
            files.insert(shown, (e.path, path));
        }
    }
    // Capability fixes may also insert grants into the package manifest.
    // It remains outside the source list, so it is never parsed as HD.
    let manifest = root.join("hd.toml");
    if let Ok(text) = std::fs::read_to_string(&manifest) {
        original.insert("hd.toml".to_owned(), text);
        files.insert("hd.toml".to_owned(), ("hd.toml".to_owned(), manifest));
    }
    let check = |now: &Texts| match disk::load_package(&root, "main") {
        Ok(mut program) => {
            for (shown, text) in now {
                if original.get(shown) != Some(text) {
                    program.sources.set_text(&files[shown].0, text);
                }
            }
            check_cmd::fix_package(program, lint)
        }
        Err(e) => vec![Diag::error(None, &e)],
    };
    let out = fix(original.clone(), mode, check);
    let mut unwritten = BTreeSet::new();
    let mut diags = Vec::new();
    for (shown, text) in &out.texts {
        if original.get(shown) == Some(text) {
            continue;
        }
        if let Err(e) = write_file(&files[shown].1, &original[shown], text) {
            diags.push(Diag::error(None, &format!("{shown}: {e}")));
            unwritten.insert(shown.as_str());
        }
    }
    diags.extend(out.remaining);
    let mut report = String::new();
    let mut fixed = 0;
    for c in &out.changes {
        if c.edits.iter().all(|e| !unwritten.contains(e.file.as_str())) {
            fixed += 1;
            report.push_str(&c.text());
            report.push('\n');
        }
    }
    let errors = diags.iter().filter(|d| d.is_error()).count();
    for d in &diags {
        report.push_str(&d.text());
        report.push('\n');
    }
    let result = if errors > 0 { "FAILED" } else { "ok" };
    let _ = writeln!(
        report,
        "{command} result: {result}. fixed: {fixed}; errors: {errors}; warnings: {}",
        diags.len() - errors
    );
    eprint!("{report}");
    // A fixing check or lint has the command's ordinary status rule.
    if errors > 0 {
        ExitCode::from(HD_FAILURE)
    } else {
        ExitCode::SUCCESS
    }
}

/// Replaces the file at `path` with `text`, through a temporary file
/// beside it, unless it no longer holds `before`: a file edited while
/// fixing ran is left as it is.
pub(crate) fn write_file(path: &Path, before: &str, text: &str) -> Result<(), String> {
    let now = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if now != before {
        return Err("the file changed while fixes ran, so it is left as it is".to_owned());
    }
    let temp = path.with_extension("hd.fix-tmp");
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            e.to_string()
        })
}

/// The rounds of `cli.check.fix.rounds` from `texts`, with `check` as the check
/// of a state of the texts. They stop when a check offers no fix-it that
/// fits and was not found unsafe, or after [`ROUNDS`] rounds. A round
/// that keeps none of its fix-its still counts, and the next one offers
/// what they overlapped.
fn fix(mut texts: Texts, mode: Mode, mut check: impl FnMut(&Texts) -> Vec<Diag>) -> Outcome {
    let mut diags = check(&texts);
    let mut changes = Vec::new();
    // Fix-its found unsafe, by diagnostic and message, which later rounds
    // skip so that a fix-it they overlapped gets its turn.
    let mut rejected = BTreeSet::new();
    for _ in 0..ROUNDS {
        let offered = plan(&diags, &texts, &rejected, mode);
        if offered.is_empty() {
            break;
        }
        let (kept, after) = round(&texts, &diags, offered, &mut rejected, &mut check);
        if kept.is_empty() {
            continue;
        }
        texts = apply(&texts, &kept);
        diags = after;
        changes.extend(kept);
    }
    Outcome {
        texts,
        changes,
        remaining: diags,
    }
}

/// One round: the offered fix-its that are safe, and the check of `texts`
/// with them applied. All of them are tried together first; when that is
/// unsafe, each in turn, on top of the ones kept before it.
fn round(
    texts: &Texts,
    before: &[Diag],
    offered: Vec<Change>,
    rejected: &mut BTreeSet<(Key, String)>,
    check: &mut impl FnMut(&Texts) -> Vec<Diag>,
) -> (Vec<Change>, Vec<Diag>) {
    let all = check(&apply(texts, &offered));
    if adds_none(before, &all) {
        return (offered, all);
    }
    if let [only] = offered.as_slice() {
        rejected.insert((only.diag.clone(), only.message.clone()));
        return (Vec::new(), before.to_vec());
    }
    let mut kept: Vec<Change> = Vec::new();
    let mut current = before.to_vec();
    for c in offered {
        kept.push(c);
        let after = check(&apply(texts, &kept));
        if adds_none(&current, &after) {
            current = after;
        } else if let Some(c) = kept.pop() {
            rejected.insert((c.diag, c.message));
        }
    }
    (kept, current)
}

/// Whether `after` has no diagnostic that `before` lacks: no key occurs
/// more often in it.
fn adds_none(before: &[Diag], after: &[Diag]) -> bool {
    let mut counts: BTreeMap<Key, isize> = BTreeMap::new();
    for d in before {
        *counts.entry(key(d)).or_insert(0) += 1;
    }
    for d in after {
        *counts.entry(key(d)).or_insert(0) -= 1;
    }
    counts.values().all(|n| *n >= 0)
}

/// The fix-its a check offers to apply, in output order: of each
/// diagnostic, its first exact fix-it not rejected before, when every edit
/// lies in an allowed package file and fits its text, and no edit overlaps an edit
/// of a fix-it taken before it (`cli.check.fix.overlap`).
fn plan(
    diags: &[Diag],
    texts: &Texts,
    rejected: &BTreeSet<(Key, String)>,
    mode: Mode,
) -> Vec<Change> {
    let mut taken: Vec<Change> = Vec::new();
    for d in diags {
        if !mode.accepts(d.code) {
            continue;
        }
        let k = key(d);
        let Some(f) = d
            .fixes
            .iter()
            .find(|f| f.exact && !rejected.contains(&(k.clone(), f.message.clone())))
        else {
            continue;
        };
        // Other manifest edits remain suggestions, as before this exception.
        if f.edits.iter().any(|e| e.file == "hd.toml")
            && d.code != Some(hd_diag::Code::MissingCapability)
        {
            continue;
        }
        let fits = |e: &Edit| texts.get(&e.file).is_some_and(|t| fits(t, e));
        let Some(first) = f.edits.first() else {
            continue;
        };
        if !f.edits.iter().all(fits) {
            continue;
        }
        let overlaps = taken
            .iter()
            .flat_map(|c| &c.edits)
            .any(|a| f.edits.iter().any(|b| overlap(a, b)));
        if overlaps {
            continue;
        }
        let (line, column) = line_column(&texts[&first.file], first.start as usize);
        taken.push(Change {
            file: first.file.clone(),
            line,
            column,
            diag: k,
            message: f.message.clone(),
            edits: f.edits.clone(),
        });
    }
    taken
}

/// Whether an edit's byte range lies in `text`, on character boundaries.
fn fits(text: &str, e: &Edit) -> bool {
    let (start, end) = (e.start as usize, e.end as usize);
    start <= end && end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end)
}

/// Whether two edits overlap: they share a byte, or they start at the same
/// byte, so their order would decide the result.
fn overlap(a: &Edit, b: &Edit) -> bool {
    a.file == b.file && ((a.start < b.end && b.start < a.end) || a.start == b.start)
}

/// `texts` with the edits of `changes` applied. The edits overlap nowhere
/// and fit their texts, so they apply from the end of each file backwards.
fn apply(texts: &Texts, changes: &[Change]) -> Texts {
    let mut out = texts.clone();
    let mut edits: Vec<&Edit> = changes.iter().flat_map(|c| &c.edits).collect();
    edits.sort_stable_by(|a, b| (&b.file, b.start, b.end).cmp(&(&a.file, a.start, a.end)));
    for e in edits {
        if let Some(text) = out.get_mut(&e.file) {
            text.replace_range(e.start as usize..e.end as usize, &e.text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Mode, ROUNDS, Texts, fix, plan};
    use crate::report::{Diag, Edit, Fix};
    use hd_diag::Code;
    use std::collections::BTreeSet;

    fn texts(text: &str) -> Texts {
        Texts::from([("f.hd".to_owned(), text.to_owned())])
    }

    /// An error at `f.hd` whose one exact fix-it, when `insert` is given,
    /// writes it at byte `at`.
    fn diag(code: Code, message: &str, insert: Option<(u32, &str)>) -> Diag {
        let mut d = Diag::error(Some(code), message).at("f.hd", Some(1));
        if let Some((at, text)) = insert {
            d.fixes.push(Fix {
                message: format!("insert {text}"),
                edits: vec![Edit {
                    file: "f.hd".to_owned(),
                    start: at,
                    end: at,
                    text: text.to_owned(),
                }],
                exact: true,
            });
        }
        d
    }

    /// `cli.check.fix.overlap`: of two fix-its that insert at the same byte, only
    /// the one of the first diagnostic is taken.
    #[test]
    fn overlapping_fix_its_take_the_first_diagnostic() {
        let diags = [
            diag(Code::UnknownName, "a", Some((0, "a"))),
            diag(Code::UnknownName, "b", Some((0, "b"))),
            diag(Code::UnknownName, "c", Some((1, "c"))),
        ];
        let taken = plan(&diags, &texts("xy\n"), &BTreeSet::new(), Mode::Check);
        let messages: Vec<&str> = taken.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(messages, ["insert a", "insert c"]);
    }

    /// `cli.check.fix.overlap`, `cli.check.fix.rounds`: the fix-it left out for an
    /// overlap is applied by the next round, at its new offset.
    #[test]
    fn an_overlapped_fix_it_waits_for_the_next_round() {
        let out = fix(texts("x\n"), Mode::Check, |t| {
            let text = &t["f.hd"];
            let mut out = Vec::new();
            for c in ["a", "b"] {
                if !text.contains(c) {
                    out.push(diag(Code::UnknownName, c, Some((0, c))));
                }
            }
            out
        });
        assert_eq!(out.texts["f.hd"], "bax\n");
        let messages: Vec<&str> = out.changes.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(messages, ["insert a", "insert b"]);
    }

    /// A suggestion, or an edit outside the source files or past the end
    /// of its file, is not taken.
    #[test]
    fn only_exact_fix_its_that_fit_a_source_file_are_taken() {
        let mut suggestion = diag(Code::UnknownName, "a", Some((0, "a")));
        suggestion.fixes[0].exact = false;
        let mut manifest = diag(Code::UnknownName, "b", Some((0, "b")));
        manifest.fixes[0].edits[0].file = "hd.toml".to_owned();
        let past = diag(Code::UnknownName, "c", Some((9, "c")));
        assert!(
            plan(
                &[suggestion, manifest, past],
                &texts("x\n"),
                &BTreeSet::new(),
                Mode::Check,
            )
            .is_empty()
        );
    }

    /// Lint fixing has a semantic allowlist in addition to exact edits.
    #[test]
    fn lint_fix_takes_only_behavior_preserving_rules() {
        let diags = [
            diag(
                Code::PrivateMain,
                "changes the entry point",
                Some((0, "pub ")),
            ),
            diag(Code::RedundantLetMut, "redundant mut", Some((1, ""))),
        ];
        let taken = plan(&diags, &texts("xy\n"), &BTreeSet::new(), Mode::Lint);
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].diag.0, Some("redundant-let-mut"));
    }

    /// `cli.check.fix.rounds`: a fix-it that only the second check offers, once
    /// the first fix-it is in, is applied in the second round. The
    /// diagnostic it fixes was there before, so it is no new one.
    #[test]
    fn a_fix_it_that_enables_another_is_followed_by_it() {
        let out = fix(texts("x\n"), Mode::Check, |t| {
            let text = &t["f.hd"];
            let mut out = Vec::new();
            if !text.contains('a') {
                out.push(diag(Code::UnknownName, "a", Some((0, "a"))));
            }
            if !text.contains('b') {
                let fix = text.contains('a').then_some((0, "b"));
                out.push(diag(Code::TypeMismatch, "b", fix));
            }
            out
        });
        assert_eq!(out.texts["f.hd"], "bax\n");
        let codes: Vec<_> = out.changes.iter().map(|c| c.diag.0).collect();
        assert_eq!(codes, [Some("unknown-name"), Some("type-mismatch")]);
        assert!(out.remaining.is_empty());
    }

    /// `cli.check.fix.safe`: a fix-it that brings a new diagnostic is reverted,
    /// while the other fix-it of its round stays.
    #[test]
    fn a_fix_it_that_adds_a_diagnostic_is_not_applied() {
        let out = fix(texts("xy\n"), Mode::Check, |t| {
            let text = &t["f.hd"];
            let mut out = Vec::new();
            if !text.contains('a') {
                out.push(diag(Code::UnknownName, "a", Some((0, "a"))));
            }
            if text.contains('z') {
                out.push(diag(Code::TypeMismatch, "brought by z", None));
            } else {
                out.push(diag(Code::UnknownName, "z", Some((1, "z"))));
            }
            out
        });
        assert_eq!(out.texts["f.hd"], "axy\n");
        assert_eq!(out.changes.len(), 1);
        assert_eq!(out.changes[0].message, "insert a");
        assert_eq!(out.remaining.len(), 1);
        assert_eq!(out.remaining[0].message, "z");
    }

    /// A fix-it left out for overlapping an unsafe one is offered by the
    /// next round.
    #[test]
    fn a_fix_it_overlapped_by_an_unsafe_one_gets_its_turn() {
        let out = fix(texts("x\n"), Mode::Check, |t| {
            let text = &t["f.hd"];
            if text.contains('z') {
                return vec![diag(Code::TypeMismatch, "brought by z", None)];
            }
            let mut out = vec![diag(Code::UnknownName, "z", Some((0, "z")))];
            if !text.contains('b') {
                out.push(diag(Code::UnknownName, "b", Some((0, "b"))));
            }
            out
        });
        assert_eq!(out.texts["f.hd"], "bx\n");
        let messages: Vec<&str> = out.changes.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(messages, ["insert b"]);
    }

    /// `cli.check.fix.rounds`: a check that offers a fix-it after every round
    /// stops after the bound.
    #[test]
    fn rounds_stop_at_the_bound() {
        let mut checks = 0;
        let out = fix(texts("x\n"), Mode::Check, |_| {
            checks += 1;
            vec![diag(Code::UnknownName, "again", Some((0, "q")))]
        });
        assert_eq!(out.changes.len(), ROUNDS);
        assert_eq!(out.texts["f.hd"], format!("{}x\n", "q".repeat(ROUNDS)));
        assert_eq!(checks, ROUNDS + 1);
    }

    /// A check that offers no fix-it changes nothing.
    #[test]
    fn no_fix_it_changes_nothing() {
        let out = fix(texts("x\n"), Mode::Check, |_| {
            vec![diag(Code::TypeMismatch, "no fix-it", None)]
        });
        assert_eq!(out.texts, texts("x\n"));
        assert!(out.changes.is_empty());
        assert_eq!(out.remaining.len(), 1);
    }
}
