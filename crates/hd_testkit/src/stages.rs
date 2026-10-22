//! Plain hd stage cases under `tests/cases/<topic>/`, with separate
//! generated outputs under `tests/baselines/<topic>/` (#675).
//! Header comments select dumps/builds and optional virtual modules.
//! Subject comments narrow a dump; otherwise output is attributed to source
//! lines and interleaved below them. Blessing never rewrites source cases.

mod counts;
mod inspection;
mod layout;
mod own;
mod wat;
pub use wat::{wat, wat_for};

use hd_driver::{Executor, Goal, Host, NoClock, PositionPool, build};
use hd_sched::SerialOrder;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

/// One plain source case, with header-selected views and build.
#[derive(Clone, Debug)]
pub struct Case {
    pub program: Program,
    pub dumps: Vec<String>,
    pub build: Build,
}

/// A program's main module and optional additional virtual modules.
#[derive(Clone, Debug)]
pub struct Program {
    pub text: String,
    pub subject: (usize, usize),
    pub files: Vec<(String, String)>,
    marked: bool,
}

impl Program {
    /// Reads valid hd with optional subject comments. Without a marker,
    /// the subject is the whole file.
    ///
    /// # Errors
    /// An invalid or empty subject marker.
    pub fn new(text: &str) -> Result<Self, String> {
        let span = subject(text)?;
        Ok(Self {
            text: text.to_owned(),
            subject: span.unwrap_or((0, text.len())),
            files: Vec::new(),
            marked: span.is_some(),
        })
    }

    #[must_use]
    pub fn inside(&self, lo: usize, hi: usize) -> bool {
        lo >= self.subject.0 && hi <= self.subject.1 && lo < hi
    }

    #[must_use]
    pub fn subject_text(&self) -> &str {
        &self.text[self.subject.0..self.subject.1]
    }

    fn sources(&self) -> hd_project::MemorySources {
        let mut sources = hd_project::MemorySources::default();
        sources.insert("main.hd", &self.text);
        for (name, text) in &self.files {
            sources.insert(name, text);
        }
        sources
    }
}

/// Parses directives. `@dump` and `@build` belong in the header comments
/// before the first line of code. `@file` begins a virtual module;
/// subsequent lines belong to it until the next `@file`. Main defaults to
/// `main.hd`.
///
/// # Errors
/// Unknown directives/builds/views, `@dump` or `@build` after code,
/// duplicate modules, unsafe module paths, or missing dump directives.
pub fn parse(text: &str) -> Result<Case, String> {
    let mut dumps = Vec::new();
    let mut build = Build::default();
    let mut modules = BTreeMap::<String, String>::new();
    let mut current = "main.hd".to_owned();
    let mut declared = Vec::new();
    let mut header = true;
    for line in text.split_inclusive('\n') {
        let code = !line.trim().is_empty() && !line.trim_start().starts_with('#');
        header &= !code;
        if let Some(directive) = line.trim_start().strip_prefix("# @") {
            let (name, value) = directive
                .trim()
                .split_once(':')
                .ok_or("a directive needs `:`")?;
            let value = value.trim();
            if !header && matches!(name, "dump" | "build") {
                return Err(format!("`@{name}` after the header comments"));
            }
            match name {
                "dump" => {
                    for view in value.split_whitespace() {
                        if !matches!(
                            view,
                            "tokens" | "tree" | "tir" | "own" | "wat" | "counts" | "layout"
                        ) {
                            return Err(format!("unknown dump `{view}`"));
                        }
                        if dumps.iter().any(|v| v == view) {
                            return Err(format!("duplicate dump `{view}`"));
                        }
                        dumps.push(view.to_owned());
                    }
                }
                "build" => {
                    build =
                        Build::parse(value).ok_or_else(|| format!("unknown build `{value}`"))?;
                }
                "file" => {
                    let path = Path::new(value);
                    if value.is_empty()
                        || value.contains('\\')
                        || path.extension().is_none_or(|e| e != "hd")
                        || path
                            .components()
                            .any(|c| !matches!(c, Component::Normal(_)))
                    {
                        return Err(format!("invalid module path `{value}`"));
                    }
                    if declared.iter().any(|n| n == value)
                        || modules.get(value).is_some_and(|t| {
                            t.lines()
                                .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                        })
                    {
                        return Err(format!("duplicate module `{value}`"));
                    }
                    declared.push(value.to_owned());
                    value.clone_into(&mut current);
                }
                _ => return Err(format!("unknown directive `@{name}`")),
            }
            // Keep line numbers in the implicit main source; directives
            // remain ordinary comments, never compiler input flags.
            if current == "main.hd" {
                modules.entry(current.clone()).or_default().push_str(line);
            }
        } else {
            modules.entry(current.clone()).or_default().push_str(line);
        }
    }
    if dumps.is_empty() {
        return Err("no `# @dump:` directive".to_owned());
    }
    let main = modules.remove("main.hd").ok_or("no main.hd module")?;
    let mut program = Program::new(&main)?;
    program.files = modules.into_iter().collect();
    Ok(Case {
        program,
        dumps,
        build,
    })
}

/// A view's output, either a flat marked excerpt or rows attributed to
/// source starts. The same rendering walk serves both modes; no per-line
/// recompilation and no synthetic source origins.
struct Dump<'a> {
    program: &'a Program,
    at: usize,
    flat: String,
    rows: BTreeMap<usize, String>,
}

impl<'a> Dump<'a> {
    fn new(program: &'a Program) -> Self {
        Self {
            program,
            at: 0,
            flat: String::new(),
            rows: BTreeMap::new(),
        }
    }

    fn at(&mut self, offset: usize) {
        let offset = offset.min(self.program.text.len());
        self.at = self.program.text.as_bytes()[..offset]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
    }

    fn push_str(&mut self, text: &str) {
        let _ = self.write_str(text);
    }

    fn finish(self) -> String {
        if self.program.marked {
            return self.flat;
        }
        let mut out = String::new();
        let mut at = 0;
        let mut rows = self.rows.into_iter().peekable();
        for line in self.program.text.split_inclusive('\n') {
            out.push_str(line.trim_end_matches('\n'));
            out.push('\n');
            let end = at + line.len();
            while rows.peek().is_some_and(|(pos, _)| *pos < end) {
                let (_, text) = rows.next().expect("peeked row");
                for l in text.lines() {
                    let _ = writeln!(out, "> {l}");
                }
            }
            at = end;
        }
        for (_, text) in rows {
            for line in text.lines() {
                let _ = writeln!(out, "> {line}");
            }
        }
        out
    }
}

impl std::fmt::Write for Dump<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.flat.push_str(text);
        self.rows.entry(self.at).or_default().push_str(text);
        Ok(())
    }
}

/// The subject's byte span in a case's code, `None` when it has no marker.
fn subject(code: &str) -> Result<Option<(usize, usize)>, String> {
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
            return Ok(Some((
                start + first.min(above.len()),
                start + last.min(above.len()),
            )));
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
            return Ok(Some((start + depth, end)));
        }
    }
    Ok(None)
}

/// The tokens view: each token inside the subject, its kind and text.
#[must_use]
pub fn tokens(p: &Program) -> String {
    let lexed = hd_syntax::lex(p.text.as_bytes());
    let t = &lexed.tokens;
    let mut out = Dump::new(p);
    for k in 0..t.kind.len() {
        let (lo, hi) = (t.start[k] as usize, t.end[k] as usize);
        if p.inside(lo, hi) {
            out.at(lo);
            let _ = writeln!(out, "{:?} {}", t.kind[k], &p.text[lo..hi]);
        }
    }
    out.finish()
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
        out: &mut Dump<'_>,
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
        out.at(own.expect("inside span").0);
        let own: Vec<&str> = node
            .direct_tokens()
            .map(|t| parsed.tokens.text(t, &p.text))
            .collect();
        let _ = write!(out, "{}{:?}", "  ".repeat(depth), node.kind());
        if !own.is_empty() {
            let _ = write!(out, " {}", own.join(" ").replace('\n', "\\n"));
        }
        out.push_str("\n");
        for child in node.children() {
            walk(child, depth + 1, parsed, p, out);
        }
    }
    let parsed = hd_syntax::parse(p.text.as_bytes());
    let mut out = Dump::new(p);
    if !parsed.tree.is_empty() {
        walk(parsed.tree.root(), 0, &parsed, p, &mut out);
    }
    for d in &parsed.diagnostics {
        let line = parsed.tokens.line_of(d.primary.lo);
        out.at(d.primary.lo as usize);
        let _ = writeln!(out, "diagnostic line {}: {}", line + 1, d.code.as_str());
    }
    out.finish()
}

/// The TIR view: the checked TIR of each body of the program's own
/// package, keeping the instructions whose source lies inside the
/// subject, after the body's path; then the build's diagnostics.
#[must_use]
pub fn tir(p: &Program) -> String {
    let src = p.sources();
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
    let mut out = Dump::new(p);
    for (item, text) in &built.tir_text {
        let mut lines = Vec::new();
        for line in text.lines() {
            let Some(rest) = line.trim_start().strip_prefix('%') else {
                continue;
            };
            let Some(k) = rest.split(' ').next().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            if let Some((file, lo, hi)) = positions.span(item, k)
                && file == "main.hd"
                && p.inside(lo as usize, hi as usize)
            {
                lines.push((lo as usize, line.trim()));
            }
        }
        if !lines.is_empty() {
            out.at(lines
                .iter()
                .map(|(lo, _)| *lo)
                .min()
                .expect("kept instructions"));
            let _ = writeln!(out, "{item}");
            // The locals the kept instructions name, as the body declares
            // them, so a reader sees their names and types.
            for local in text.lines().filter_map(|l| l.trim().strip_prefix("local ")) {
                let id = local.split(' ').next().unwrap_or_default();
                if lines
                    .iter()
                    .any(|(_, l)| l.split([' ', ']', '[']).any(|w| w == id))
                {
                    let _ = writeln!(out, "  local {local}");
                }
            }
            for (lo, l) in lines {
                out.at(lo);
                let _ = writeln!(out, "  {l}");
            }
        }
    }
    let rendered = built.render();
    if !rendered.trim().is_empty() {
        out.at(p.text.len());
        let _ = writeln!(out, "{}", rendered.trim_end());
    }
    out.finish()
}

/// The output of view `name` for `p` built as `build`, or `None` for a
/// view this harness does not know.
#[must_use]
pub fn view(name: &str, build: Build, p: &Program) -> Option<String> {
    Some(match name {
        "layout" => layout::view(p, build),
        "counts" => counts::view(p, build),
        "tokens" => tokens(p),
        "tree" => tree(p),
        "tir" => tir(p),
        "own" => own::view(p, build),
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
        for opt in opts.split([',', ' ']).filter(|s| !s.is_empty()) {
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

/// Runs every `.hd` case recursively under `dir`, comparing each dump
/// with its mirrored baseline. `HD_BLESS=1` writes only baselines.
///
/// # Panics
/// Invalid cases, missing baselines, view differences, or an empty directory.
pub fn run_dir(dir: &Path, views: &dyn Fn(&str, Build, &Program) -> Option<String>) {
    let bless = std::env::var_os("HD_BLESS").as_deref() == Some(std::ffi::OsStr::new("1"));
    run_cases(dir, views, bless);
}

fn run_cases(dir: &Path, views: &dyn Fn(&str, Build, &Program) -> Option<String>, bless: bool) {
    fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                collect(&path, files);
            } else if path.extension().is_some_and(|e| e == "hd") {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    collect(dir, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no `.hd` cases in {}", dir.display());
    let baseline_root = dir.parent().expect("tests directory").join("baselines");
    let mut expected_baselines = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for path in files {
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let case = match parse(&text) {
            Ok(case) => case,
            Err(error) => {
                failures.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let relative = path.strip_prefix(dir).expect("case under root");
        for dump in &case.dumps {
            let Some(output) = views(dump, case.build, &case.program) else {
                failures.push(format!("{}: unknown dump `{dump}`", path.display()));
                continue;
            };
            let prefix = if dump == "wat" { ";;" } else { "#" };
            let actual = format!(
                "{prefix} Generated by HD_BLESS=1 from {}; do not edit.\n{output}",
                relative.to_string_lossy().replace('\\', "/")
            );
            let baseline = baseline_root.join(relative).with_extension(dump);
            expected_baselines.insert(baseline.clone());
            if bless {
                std::fs::create_dir_all(baseline.parent().expect("baseline directory"))
                    .expect("create baseline directory");
                std::fs::write(&baseline, actual)
                    .unwrap_or_else(|e| panic!("{}: {e}", baseline.display()));
            } else {
                match std::fs::read_to_string(&baseline) {
                    Ok(expected) if expected == actual => {}
                    Ok(expected) => failures.push(format!(
                        "{} differs; rerun with HD_BLESS=1 to accept:\n{}",
                        baseline.display(),
                        diff(&expected, &actual)
                    )),
                    Err(error) => failures.push(format!(
                        "{}: {error}; rerun with HD_BLESS=1 to create this baseline",
                        baseline.display()
                    )),
                }
            }
        }
    }
    // A baseline whose case or dump is gone: blessing deletes it.
    let mut stack = vec![baseline_root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if !expected_baselines.contains(&path) {
                if bless {
                    std::fs::remove_file(&path)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                } else {
                    failures.push(format!(
                        "{} has no case or dump; rerun with HD_BLESS=1 to delete it",
                        path.display()
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[cfg(test)]
mod tests {
    use super::{Build, Program, diff, parse, tokens};

    #[test]
    fn headers_select_dumps_builds_and_modules() {
        let case = parse("# @dump: tir wat\n# @build: wasmtime release\n# @file: main.hd\nfn one() -> i32: +1\n# @file: other.hd\npub fn two() -> i32: +2\n").expect("case");
        assert_eq!(case.dumps, ["tir", "wat"]);
        assert_eq!(case.build, Build::parse("wasmtime release").unwrap());
        assert!(case.program.text.contains("fn one"));
        assert!(!case.program.text.contains("fn two"));
        assert_eq!(case.program.files[0].0, "other.hd");
        for bad in [
            "# @dump: typo\n",
            "# @dump: tir tir\n",
            "# @dump: tir\n# @build: fast\n",
            "# @dump: tir\n# @file: ../escape.hd\n",
            "# @dump: tir\n# @file: /absolute.hd\n",
            "# @dump: tir\n# @file: other.hd\nx := 1\n# @file: other.hd\n",
            "# @dunp: tir\n",
            "# @dump: tir\nx := 1\n# @dump: wat\n",
            "# @dump: tir\nx := 1\n# @build: release\n",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn markers_select_subjects_and_no_marker_selects_the_whole_file() {
        let p = Program::new("fn f(p: P) -> i32:\n    p.x + 1\n    #^^\n").unwrap();
        assert_eq!(p.subject_text(), "p.x");
        let p = Program::new(
            "fn f() -> i32:\n    # subject\n    if true:\n        1\n    else:\n        2\n",
        )
        .unwrap();
        assert_eq!(
            p.subject_text(),
            "if true:\n        1\n    else:\n        2"
        );
        let p = Program::new("x := 1\ny := x + 2\n").unwrap();
        assert_eq!(p.subject_text(), p.text);
        let p = Program::new("y := x + 2\n#    ^^^^^\n").unwrap();
        assert_eq!(p.subject_text(), "x + 2");
    }

    #[test]
    fn whole_file_output_interleaves_both_source_lines() {
        let p = Program::new("x := 1\ny := 2\n").unwrap();
        let text = tokens(&p);
        assert!(text.starts_with("x := 1\n> Ident x\n"), "{text}");
        assert!(text.contains("y := 2\n> Ident y\n"), "{text}");
        let p = Program::new("x := 1\n#^^^^^\n").unwrap();
        assert!(!tokens(&p).contains("> "));
    }

    #[test]
    fn a_diff_marks_missing_and_new_lines() {
        assert_eq!(diff("a\nb\nc\n", "a\nc\nd\n"), "  a\n- b\n  c\n+ d\n");
    }
    #[test]
    fn blessing_changes_only_baselines_and_normal_runs_require_them() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = std::env::temp_dir().join(format!("hd-stage-bless-{}", std::process::id()));
        let _cleanup = Cleanup(root.clone());
        let cases = root.join("cases/topic");
        std::fs::create_dir_all(&cases).unwrap();
        let input = "# @dump: tokens tree wat\nx := 1\n";
        let source = cases.join("one.hd");
        std::fs::write(&source, input).unwrap();
        let dir = root.join("cases");
        let fake = |name: &str, _: Build, _: &Program| Some(format!("{name} output\n"));
        assert!(std::panic::catch_unwind(|| super::run_cases(&dir, &fake, false)).is_err());
        super::run_cases(&dir, &fake, true);
        assert!(
            std::fs::read_to_string(root.join("baselines/topic/one.wat"))
                .unwrap()
                .starts_with(";; Generated by HD_BLESS=1")
        );
        let baseline = root.join("baselines/topic/one.tokens");
        let first = std::fs::read(&baseline).unwrap();
        assert!(first.starts_with(b"# Generated by HD_BLESS=1"));
        super::run_cases(&dir, &fake, false);
        super::run_cases(&dir, &fake, true);
        assert_eq!(std::fs::read(&baseline).unwrap(), first);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), input);
        std::fs::write(&baseline, "stale\n").unwrap();
        assert!(std::panic::catch_unwind(|| super::run_cases(&dir, &fake, false)).is_err());
        super::run_cases(&dir, &fake, true);
        let orphan = root.join("baselines/topic/gone.tokens");
        std::fs::write(&orphan, "orphan\n").unwrap();
        assert!(std::panic::catch_unwind(|| super::run_cases(&dir, &fake, false)).is_err());
        super::run_cases(&dir, &fake, true);
        assert!(!orphan.exists());
    }
    #[test]
    fn virtual_modules_reach_every_compiler_view_without_leaking_their_spans() {
        let case = parse("# @dump: tir own wat counts layout\n# @file: main.hd\nuse pkg.other.answer\n\nfn result() -> i32: answer()\n\n# @file: other.hd\npub fn answer() -> i32: +42\n").unwrap();
        let built = super::inspection::build(&case.program, true, case.build);
        assert!(!built.diags.has_errors(), "{}", built.render());
        for view in ["tir", "own", "wat", "counts", "layout"] {
            let out = super::view(view, case.build, &case.program).unwrap();
            assert!(out.contains("fn result()"), "{view}: {out}");
            assert!(!out.contains("pub fn answer()"), "{view}: {out}");
        }
    }
}
