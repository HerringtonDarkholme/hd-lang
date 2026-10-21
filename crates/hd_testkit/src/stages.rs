//! Stage fixtures (#657, #660): topic files under a crate's
//! `tests/stages/`, each a set of small cases whose expected outputs are
//! views of one compiler stage, filtered to the case's subject.
//!
//! A fixture file (`*.stage`) holds, in order:
//!
//! - optional `#` comment lines describing the topic;
//! - `-- context --` and the hd that every case in the file uses;
//! - for each case, `-- case: NAME --`, the case's hd, then one block per
//!   view, `-- VIEW --` followed by the expected output (`tokens`, `tree`,
//!   `tir`, `tir-flags`, which adds each local's flags, and `wat`, the
//!   normalized Wasm of the subject, `wat.rs`). A header may name a build
//!   after the view, `-- wat (wasmtime, release) --` ([`Build`]).
//!
//! A case's program is the context followed by the case. Its subject is
//! marked by a comment, so the case stays valid hd for `hd fmt` and
//! `hd check`:
//!
//! - a caret line, `#^^^^` under the subject: the columns from the `#` (when
//!   a `^` follows it directly) or from the first `^`, to the last `^`, of
//!   the line above;
//! - a `# subject` line before the subject: the next line and the lines
//!   indented deeper after it;
//! - no marker: the case's last statement or item.
//!
//! Each view keeps only what lies inside the subject's span. `HD_BLESS=1`
//! rewrites every view block in place. A case without view blocks gets
//! the views of the case before it, so adding a case is pasting its code
//! and blessing.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hd_driver::{Executor, Goal, Host, NoClock, PositionPool, build};
use hd_sched::SerialOrder;

mod wat;
pub use wat::{wat, wat_for};

/// One case of a fixture file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub name: String,
    /// The case's hd, as written, markers included.
    pub code: String,
    /// The expected output of each view, in the file's order.
    pub views: Vec<(String, String)>,
}

/// A parsed fixture file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    /// The lines before `-- context --`.
    pub header: String,
    pub context: String,
    pub cases: Vec<Case>,
}

/// Parses a fixture file's text.
///
/// # Errors
///
/// A file without `-- context --`, or a view block before any case.
pub fn parse(text: &str) -> Result<Fixture, String> {
    // Where the next line goes: the context, a case's code, or a view.
    enum At {
        Header,
        Context,
        Code,
        View,
    }
    let mut header = String::new();
    let mut context: Option<String> = None;
    let mut cases: Vec<Case> = Vec::new();
    let mut at = At::Header;
    for line in text.split_inclusive('\n') {
        let t = line.trim_end();
        if t == "-- context --" {
            context = Some(String::new());
            at = At::Context;
            continue;
        }
        if let Some(name) = t
            .strip_prefix("-- case: ")
            .and_then(|r| r.strip_suffix(" --"))
        {
            if context.is_none() {
                return Err(format!("case `{name}` before `-- context --`"));
            }
            cases.push(Case {
                name: name.to_owned(),
                code: String::new(),
                views: Vec::new(),
            });
            at = At::Code;
            continue;
        }
        if let Some(view) = t
            .strip_prefix("-- ")
            .and_then(|r| r.strip_suffix(" --"))
            .filter(|v| {
                // `NAME`, or `NAME (OPTIONS)`.
                let name = v
                    .split_once(" (")
                    .map_or(*v, |(n, o)| if o.ends_with(')') { n } else { "" });
                !name.is_empty() && !name.contains(' ')
            })
        {
            let Some(case) = cases.last_mut() else {
                return Err(format!("view `{view}` before any case"));
            };
            case.views.push((view.to_owned(), String::new()));
            at = At::View;
            continue;
        }
        match at {
            At::Header => header.push_str(line),
            At::Context => context.get_or_insert_default().push_str(line),
            At::Code => {
                if let Some(case) = cases.last_mut() {
                    case.code.push_str(line);
                }
            }
            At::View => {
                if let Some((_, out)) = cases.last_mut().and_then(|c| c.views.last_mut()) {
                    out.push_str(line);
                }
            }
        }
    }
    let Some(context) = context else {
        return Err("no `-- context --` block".to_owned());
    };
    Ok(Fixture {
        header,
        context,
        cases,
    })
}

/// Writes a fixture back: the header, the context and each case as
/// written, then each view's output.
#[must_use]
pub fn print(f: &Fixture) -> String {
    let mut out = f.header.clone();
    out.push_str("-- context --\n");
    out.push_str(&f.context);
    for case in &f.cases {
        let _ = writeln!(out, "-- case: {} --", case.name);
        out.push_str(&case.code);
        for (view, text) in &case.views {
            let _ = writeln!(out, "-- {view} --");
            out.push_str(text);
        }
    }
    out
}

/// A case ready for the views: its program (the context, then the case)
/// and the subject's byte span in it.
#[derive(Clone, Debug)]
pub struct Program {
    pub text: String,
    pub subject: (usize, usize),
}

impl Program {
    /// Whether the span `lo..hi` lies inside the subject.
    #[must_use]
    pub fn inside(&self, lo: usize, hi: usize) -> bool {
        lo >= self.subject.0 && hi <= self.subject.1 && lo < hi
    }

    /// The subject's source text.
    #[must_use]
    pub fn subject_text(&self) -> &str {
        &self.text[self.subject.0..self.subject.1]
    }
}

/// The program of `case` in a file whose context is `context`.
///
/// # Errors
///
/// A caret line with no line above it, or a `# subject` line with nothing
/// after it.
pub fn program(context: &str, case: &Case) -> Result<Program, String> {
    let base = context.len();
    let text = format!("{context}{}", case.code);
    let span = subject(&case.code)?;
    Ok(Program {
        text,
        subject: (base + span.0, base + span.1),
    })
}

/// The subject's byte span in a case's code.
fn subject(code: &str) -> Result<(usize, usize), String> {
    // Each line's byte start and text, newline excluded.
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut at = 0;
    for line in code.split_inclusive('\n') {
        lines.push((at, line.trim_end_matches(['\n', '\r'])));
        at += line.len();
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_code = |l: &str| !l.trim().is_empty() && !l.trim_start().starts_with('#');
    for (k, (_, line)) in lines.iter().enumerate() {
        let t = line.trim_start();
        if let Some(carets) = t.strip_prefix('#')
            && carets.trim_start().starts_with('^')
            && carets.trim().chars().all(|c| c == '^')
        {
            let hash = indent(line);
            let first = if carets.starts_with('^') {
                hash
            } else {
                hash + 1 + (carets.len() - carets.trim_start().len())
            };
            let last = hash + 1 + carets.trim_end().len();
            let (start, above) = lines[..k]
                .iter()
                .rev()
                .find(|(_, l)| is_code(l))
                .copied()
                .ok_or("a caret line with no code above it")?;
            // Carets past the end of the line stop at it.
            return Ok((
                start + first.min(above.len()),
                start + last.min(above.len()),
            ));
        }
        if t == "# subject" {
            let mut rest = lines[k + 1..].iter().filter(|(_, l)| is_code(l));
            let (start, first) = *rest.next().ok_or("`# subject` with no code after it")?;
            let depth = indent(first);
            let mut end = start + first.len();
            for (s, l) in rest {
                // An `else` at the subject's own depth continues it.
                let continues = l.trim_start().starts_with("else");
                if indent(l) < depth || (indent(l) == depth && !continues) {
                    break;
                }
                end = s + l.len();
            }
            return Ok((start + depth, end));
        }
    }
    // The last statement or item: from the last code line at the least
    // indentation to the end.
    let depth = lines
        .iter()
        .filter(|(_, l)| is_code(l))
        .map(|(_, l)| indent(l))
        .min()
        .ok_or("a case with no code")?;
    let (start, first) = *lines
        .iter()
        .rev()
        .find(|(_, l)| is_code(l) && indent(l) == depth)
        .ok_or("a case with no code")?;
    let end = lines
        .iter()
        .rev()
        .find(|(_, l)| is_code(l))
        .map_or(start + first.len(), |(s, l)| s + l.len());
    Ok((start + depth, end))
}

/// The tokens view: each token inside the subject, its kind and text.
#[must_use]
pub fn tokens(p: &Program) -> String {
    let lexed = hd_syntax::lex(p.text.as_bytes());
    let t = &lexed.tokens;
    let mut out = String::new();
    for k in 0..t.kind.len() {
        let (lo, hi) = (t.start[k] as usize, t.end[k] as usize);
        if p.inside(lo, hi) {
            let _ = writeln!(out, "{:?} {}", t.kind[k], &p.text[lo..hi]);
        }
    }
    out
}

/// The tree view: the outline of each largest syntax node inside the
/// subject, one node per line, indented by depth, with its own tokens.
#[must_use]
pub fn tree(p: &Program) -> String {
    fn span(node: hd_syntax::NodeRef<'_>, parsed: &hd_syntax::Parse) -> Option<(usize, usize)> {
        let (lo, hi) = parsed.tree.span_tokens(node.index());
        let (lo, hi) = (lo.get()?, hi.get()?);
        Some((
            parsed.tokens.start[lo.idx()] as usize,
            parsed.tokens.end[hi.idx()] as usize,
        ))
    }
    fn walk(
        node: hd_syntax::NodeRef<'_>,
        depth: usize,
        parsed: &hd_syntax::Parse,
        p: &Program,
        out: &mut String,
    ) {
        let own = span(node, parsed);
        let inside = own.is_some_and(|(lo, hi)| p.inside(lo, hi));
        // A node with one child of its own span, or one outside the
        // subject, shows only what is under it.
        let mut children = node.children();
        let only = match (children.next(), children.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        };
        if !inside
            || only.is_some_and(|c| span(c, parsed) == own && node.direct_tokens().next().is_none())
        {
            for child in node.children() {
                walk(child, depth, parsed, p, out);
            }
            return;
        }
        let own: Vec<&str> = node
            .direct_tokens()
            .map(|t| parsed.tokens.text(t, &p.text))
            .collect();
        let _ = write!(out, "{}{:?}", "  ".repeat(depth), node.kind());
        if !own.is_empty() {
            let _ = write!(out, " {}", own.join(" ").replace('\n', "\\n"));
        }
        out.push('\n');
        for child in node.children() {
            walk(child, depth + 1, parsed, p, out);
        }
    }
    let parsed = hd_syntax::parse(p.text.as_bytes());
    let mut out = String::new();
    if !parsed.tree.is_empty() {
        walk(parsed.tree.root(), 0, &parsed, p, &mut out);
    }
    for d in &parsed.diagnostics {
        let line = parsed.tokens.line_of(d.primary.lo);
        let _ = writeln!(out, "diagnostic line {}: {}", line + 1, d.code.as_str());
    }
    out
}

/// The TIR view: the checked TIR of each body of the program's own
/// package, keeping the instructions whose source lies inside the
/// subject, after the body's path; then the build's diagnostics.
#[must_use]
pub fn tir(p: &Program) -> String {
    tir_with(p, false)
}

/// [`tir`], with each local's flags when `flags` (the `tir-flags` view).
#[must_use]
pub fn tir_with(p: &Program, flags: bool) -> String {
    let mut src = hd_project::MemorySources::default();
    src.insert("main.hd", &p.text);
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &["app/*"],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let built = build(&host, "app", &Goal::Analyze);
    let mut pool = PositionPool::default();
    let positions = built.positions(&mut pool);
    let mut out = String::new();
    for (item, text) in &built.tir_text {
        let mut lines = Vec::new();
        for line in text.lines() {
            let Some(rest) = line.trim_start().strip_prefix('%') else {
                continue;
            };
            let Some(k) = rest.split(' ').next().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            if let Some((_, lo, hi)) = positions.span(item, k)
                && p.inside(lo as usize, hi as usize)
            {
                lines.push(line.trim());
            }
        }
        if !lines.is_empty() {
            let _ = writeln!(out, "{item}");
            // The locals the kept instructions name, as the body declares
            // them, so a reader sees their names and types.
            for local in text.lines().filter_map(|l| l.trim().strip_prefix("local ")) {
                let id = local.split(' ').next().unwrap_or_default();
                if lines
                    .iter()
                    .any(|l| l.split([' ', ']', '[']).any(|w| w == id))
                {
                    let local = match local.rsplit_once(" flags=") {
                        Some((decl, _)) if !flags => decl,
                        _ => local,
                    };
                    let _ = writeln!(out, "  local {local}");
                }
            }
            for l in lines {
                let _ = writeln!(out, "  {l}");
            }
        }
    }
    let rendered = built.render();
    if !rendered.trim().is_empty() {
        let _ = writeln!(out, "{}", rendered.trim_end());
    }
    out
}

/// The output of view `view` for `p`, or `None` for a view this harness
/// does not know. A view header may name the build it shows after the
/// view's name: `wat (wasmtime, release)` (see [`Build`]).
#[must_use]
pub fn view(view: &str, p: &Program) -> Option<String> {
    let (name, build) = match view.split_once(" (") {
        Some((name, opts)) => (name, Build::parse(opts.strip_suffix(')')?)?),
        None => (view, Build::default()),
    };
    Some(match name {
        "tokens" if build == Build::default() => tokens(p),
        "tree" if build == Build::default() => tree(p),
        "tir" if build == Build::default() => tir(p),
        "tir-flags" if build == Build::default() => tir_with(p, true),
        "wat" => wat_for(p, build),
        _ => return None,
    })
}

/// The build a view shows: its engine target (`v8`, the default,
/// `wasmtime` or `browser`) and its level (`dev`, the default, or
/// `release`: the optimized pipeline and the release profile, as
/// `--release` builds).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Build {
    pub target: hd_driver::EngineTarget,
    pub release: bool,
}

impl Build {
    /// The build of a view header's options, comma-separated in any
    /// order; `None` for an option this harness does not know.
    #[must_use]
    pub fn parse(opts: &str) -> Option<Build> {
        let mut build = Build::default();
        for opt in opts.split(',').map(str::trim) {
            match opt {
                "v8" => build.target = hd_driver::EngineTarget::V8,
                "wasmtime" => build.target = hd_driver::EngineTarget::Wasmtime,
                "browser" => build.target = hd_driver::EngineTarget::Browser,
                "dev" => build.release = false,
                "release" => build.release = true,
                _ => return None,
            }
        }
        Some(build)
    }
}

/// A line diff of `expected` against `actual`: `-` for an expected line
/// that is missing, `+` for an actual line that was not expected, two
/// spaces for a kept line.
#[must_use]
pub fn diff(expected: &str, actual: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    // Longest common subsequence, by suffix lengths.
    let mut l = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            l[i][j] = if a[i] == b[j] {
                l[i + 1][j + 1] + 1
            } else {
                l[i + 1][j].max(l[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, String::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            let _ = writeln!(out, "  {}", a[i]);
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || l[i][j + 1] > l[i + 1][j]) {
            let _ = writeln!(out, "+ {}", b[j]);
            j += 1;
        } else {
            let _ = writeln!(out, "- {}", a[i]);
            i += 1;
        }
    }
    // Only the changed lines and two kept lines around each.
    let lines: Vec<&str> = out.lines().collect();
    let changed: Vec<bool> = lines.iter().map(|l| !l.starts_with("  ")).collect();
    let near = |k: usize| {
        (k.saturating_sub(2)..=(k + 2).min(lines.len().saturating_sub(1))).any(|m| changed[m])
    };
    let mut short = String::new();
    let mut gap = false;
    for (k, line) in lines.iter().enumerate() {
        if near(k) {
            if gap {
                short.push_str("  ...\n");
                gap = false;
            }
            short.push_str(line);
            short.push('\n');
        } else {
            gap = true;
        }
    }
    if gap {
        short.push_str("  ...\n");
    }
    short
}

/// Runs every fixture file under `dir` (`*.stage`, found without
/// registration), each case's views against their expected blocks, with
/// `views` computing the views this crate's stages give (`view` for the
/// ones here). With `HD_BLESS=1` set, rewrites the expected blocks in place
/// instead of failing.
///
/// # Panics
///
/// When a view's output differs from its block, with a diff per case; when
/// `dir` holds no fixture, or a fixture does not parse.
pub fn run_dir(dir: &Path, views: &dyn Fn(&str, &Program) -> Option<String>) {
    let bless = std::env::var_os("HD_BLESS").is_some();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "stage"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no `*.stage` fixtures in {}",
        dir.display()
    );
    let mut failures = Vec::new();
    for path in files {
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut fixture = parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut inherited: Vec<String> = Vec::new();
        for case in &mut fixture.cases {
            if case.views.is_empty() {
                case.views = inherited
                    .iter()
                    .map(|v| (v.clone(), String::new()))
                    .collect();
            }
            inherited = case.views.iter().map(|(v, _)| v.clone()).collect();
            let program = match program(&fixture.context, case) {
                Ok(p) => p,
                Err(e) => {
                    failures.push(format!("{} case `{}`: {e}", path.display(), case.name));
                    continue;
                }
            };
            for (name, expected) in &mut case.views {
                let Some(actual) = views(name, &program) else {
                    failures.push(format!(
                        "{} case `{}`: no view `{name}` here",
                        path.display(),
                        case.name
                    ));
                    continue;
                };
                if bless {
                    *expected = actual;
                } else if *expected != actual {
                    failures.push(format!(
                        "{} case `{}`, view `{name}` (subject {:?}) differs; rerun with HD_BLESS=1 to accept:\n{}",
                        path.display(),
                        case.name,
                        program.subject_text(),
                        diff(expected, &actual)
                    ));
                }
            }
        }
        if bless {
            let new = print(&fixture);
            if new != text {
                std::fs::write(&path, new).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[cfg(test)]
mod tests {
    use super::{Case, diff, parse, print, program};

    const FILE: &str = "# A topic.\n-- context --\nstruct P:\n    x: i32\n-- case: caret --\nfn f(p: P) -> i32:\n    p.x + 1\n    #^^\n-- tir --\nold\n-- case: next --\nfn g() -> i32:\n    # subject\n    if true:\n        1\n    else:\n        2\n";

    #[test]
    fn a_fixture_prints_back_as_it_was_written() {
        let f = parse(FILE).expect("parses");
        assert_eq!(f.cases.len(), 2);
        assert_eq!(
            f.cases[0].views,
            vec![("tir".to_owned(), "old\n".to_owned())]
        );
        assert_eq!(print(&f), FILE);
    }

    #[test]
    fn markers_select_the_subject() {
        let f = parse(FILE).expect("parses");
        let caret = program(&f.context, &f.cases[0]).expect("program");
        assert_eq!(caret.subject_text(), "p.x");
        let block = program(&f.context, &f.cases[1]).expect("program");
        assert_eq!(
            block.subject_text(),
            "if true:\n        1\n    else:\n        2"
        );
        let last = Case {
            name: "last".to_owned(),
            code: "x := 1\ny := x + 2\n".to_owned(),
            views: Vec::new(),
        };
        assert_eq!(
            program("", &last).expect("program").subject_text(),
            "y := x + 2"
        );
        let spaced = Case {
            name: "spaced".to_owned(),
            code: "y := x + 2\n#    ^^^^^\n".to_owned(),
            views: Vec::new(),
        };
        assert_eq!(
            program("", &spaced).expect("program").subject_text(),
            "x + 2"
        );
    }

    #[test]
    fn a_diff_marks_missing_and_new_lines() {
        assert_eq!(diff("a\nb\nc\n", "a\nc\nd\n"), "  a\n- b\n  c\n+ d\n");
        assert_eq!(
            diff("1\n2\n3\n4\n5\n6\n7\n", "1\n2\n3\n4\n5\n6\n8\n"),
            "  ...\n  5\n  6\n- 7\n+ 8\n"
        );
    }
}
