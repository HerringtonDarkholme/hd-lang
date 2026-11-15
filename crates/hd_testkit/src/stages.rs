//! Plain hd stage cases under `tests/cases/<topic>/`, with separate
//! generated outputs under `tests/baselines/<topic>/` (#675).
//! Header comments select dumps/builds and optional virtual modules.
//! Subject comments narrow a dump; otherwise output is attributed to source
//! lines and interleaved below them. Blessing never rewrites source cases.

mod counts;
mod diagnostics;
mod inspection;
mod layout;
pub mod node_host;
mod own;
mod wat;
pub use wat::{wat, wat_for};

use hd_base::SliceSort as _;
use hd_driver::{Executor, Goal, Host, NoClock, PositionPool};
use hd_sched::SerialOrder;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

/// One plain source case, with header-selected views and builds.
#[derive(Clone, Debug)]
pub struct Case {
    pub program: Program,
    pub dumps: Vec<String>,
    /// The builds each view shows, as `@build` writes them, `|` between
    /// two (`# @build: dev | release checked`); one `dev` build by default.
    pub builds: Vec<(String, Build)>,
}

/// A program's main module and optional additional virtual modules.
#[derive(Clone, Debug)]
pub struct Program {
    pub text: String,
    pub subject: (usize, usize),
    pub files: Vec<(String, String)>,
    /// The host capability traits of the `# @profile:` runtime profile.
    pub profile: Option<Vec<String>>,
    /// Functions the `wat` view prints whole: short runtime names without
    /// `rt:` and `#k`, or exact ordinary item paths
    /// (`# @helpers: enc std/hash/hash_integer`).
    pub helpers: Vec<String>,
    /// The `@host` traits the case's package declares in its manifest's
    /// `[host]` (`# @host: main.Surface`), each package-relative.
    pub hosts: Vec<String>,
    /// For a REPL session case (`# @repl: N`): the number of the input the
    /// case's own text is. Its earlier inputs are the `@input` modules.
    pub repl: Option<u32>,
    /// The fuel of each body and header goal of the case's package
    /// (`# @fuel: N`), smaller than `Fuel::BODY_DEFAULT`, so a small case
    /// reaches `item-too-complex` (`fn.limit.complexity.error`) at a chosen
    /// step. The `tir`, `diagnostics`, `spans`, `fixed`, `stdout` and
    /// `tests` views build with it (`Packages::body_fuel`).
    pub fuel: Option<u64>,
    /// The file that holds the case's own text when it is not `main.hd`
    /// (`# @main: src/main.hd`), for a case about package layout.
    pub main: Option<String>,
    /// The package's executable entry modules, by path below `src/`
    /// (`# @entry: tool`), as a manifest's `[[bin]]` names them.
    pub entries: Vec<String>,
    /// The `tests` view's name filter (`# @filter: TEXT`): only the cases
    /// whose names contain it, as `hd test --filter` selects
    /// (`cli.test.filter`).
    pub filter: Option<String>,
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
            profile: None,
            helpers: Vec::new(),
            hosts: Vec::new(),
            repl: None,
            fuel: None,
            main: None,
            entries: Vec::new(),
            filter: None,
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

    /// The file that holds the case's own text: `main.hd`, or `repl#N`
    /// for REPL input `N`.
    fn main_file(&self) -> String {
        match (&self.repl, &self.main) {
            (Some(n), _) => hd_project::repl_file(*n),
            (None, Some(main)) => main.clone(),
            (None, None) => "main.hd".to_owned(),
        }
    }

    fn sources(&self) -> hd_project::MemorySources {
        let mut sources = hd_project::MemorySources::default();
        sources.insert(&self.main_file(), &self.text);
        for (name, text) in &self.files {
            sources.insert(name, text);
        }
        sources
    }
}

/// Parses directives. `@dump`, `@build`, `@profile`, `@host`, `@fuel` and
/// `@helpers` belong in the header comments before the first line of code.
/// `@profile` names a conformance runtime profile
/// ([`crate::profile_traits`]) whose traits the case's main module
/// declares; `@host` lists the package host traits its manifest declares
/// (`module.host.package.marker`); `@helpers` selects functions the `wat`
/// view prints whole, by short runtime name or exact ordinary item path,
/// without following their calls, and `@helper` selects one item path
/// that holds spaces, as a method's does. `@file` begins a virtual module;
/// subsequent lines belong to it until the next `@file`. Main defaults to
/// `main.hd`. `@repl: N` makes the case REPL input `N` of a session whose
/// earlier inputs each begin at `@input: K`, numbered 1 up, as `@file`
/// begins a module. The `tir` view checks the joined session; `wat`
/// builds the inputs in order and shows the final extension module.
/// `@fuel: N` gives the package `N` steps of fuel per item in the `tir`,
/// `diagnostics`, `spans`, `fixed`, `stdout` and `tests` views.
/// `@main: PATH` names the file of the case's own text, `main.hd` by
/// default, for a case about package layout (`src/main.hd`); `@entry:
/// NAME` names an executable entry module (`src/NAME.hd`), as a manifest
/// does. `@filter: TEXT` keeps only the test cases whose names contain
/// `TEXT` in the `tests` view.
///
/// # Errors
/// Unknown directives/builds/views, `@dump` or `@build` after code,
/// duplicate modules, unsafe module paths, or missing dump directives.
pub fn parse(text: &str) -> Result<Case, String> {
    let mut dumps = Vec::new();
    let mut builds = Vec::new();
    let mut profile = None;
    let mut repl = None;
    let mut fuel = None;
    let mut main_path = None;
    let mut entries = Vec::new();
    let mut filter = None;
    let mut inputs = 0u32;
    let mut helpers = Vec::new();
    let mut hosts = Vec::new();
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
            if !header
                && matches!(
                    name,
                    "dump"
                        | "build"
                        | "profile"
                        | "host"
                        | "helpers"
                        | "helper"
                        | "repl"
                        | "fuel"
                        | "main"
                        | "entry"
                        | "filter"
                )
            {
                return Err(format!("`@{name}` after the header comments"));
            }
            match name {
                "dump" => {
                    for view in value.split_whitespace() {
                        if !matches!(
                            view,
                            "tokens"
                                | "tree"
                                | "tir"
                                | "own"
                                | "wat"
                                | "counts"
                                | "layout"
                                | "diagnostics"
                                | "fixed"
                                | "spans"
                                | "stdout"
                                | "tests"
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
                    if !builds.is_empty() {
                        return Err("a second `@build` line; write `|` between builds".to_owned());
                    }
                    for spec in value.split('|').map(str::trim) {
                        let build =
                            Build::parse(spec).ok_or_else(|| format!("unknown build `{spec}`"))?;
                        if builds.iter().any(|(_, b)| *b == build) {
                            return Err(format!("duplicate build `{spec}`"));
                        }
                        builds.push((spec.to_owned(), build));
                    }
                }
                "profile" => {
                    let module = hd_project::module_path("app", "main.hd");
                    profile = Some(
                        crate::profile_traits(value, &module)
                            .ok_or_else(|| format!("unknown profile `{value}`"))?,
                    );
                }
                "helpers" => helpers.extend(value.split_whitespace().map(str::to_owned)),
                // One item path that holds spaces, as a method's does.
                "helper" => helpers.push(value.to_owned()),
                "repl" => {
                    repl = Some(
                        value
                            .parse::<u32>()
                            .ok()
                            .filter(|n| *n > 1)
                            .ok_or_else(|| format!("invalid REPL input `{value}`"))?,
                    );
                }
                "fuel" => {
                    fuel = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| format!("invalid fuel `{value}`"))?,
                    );
                }
                "input" => {
                    let next = inputs + 1;
                    if repl.is_none_or(|n| next >= n) || value != next.to_string() {
                        return Err(format!(
                            "`@input: {value}` needs `@repl` after it and inputs numbered 1 up"
                        ));
                    }
                    inputs = next;
                    current = hd_project::repl_file(next);
                }
                "host" => hosts.extend(value.split_whitespace().map(str::to_owned)),
                "main" => {
                    if !module_path_ok(value) {
                        return Err(format!("invalid module path `{value}`"));
                    }
                    main_path = Some(value.to_owned());
                }
                "entry" => entries.extend(value.split_whitespace().map(str::to_owned)),
                "filter" => filter = Some(value.to_owned()),
                "file" => {
                    if !module_path_ok(value) {
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
    if repl.is_some()
        && dumps
            .iter()
            .any(|dump| !matches!(dump.as_str(), "tir" | "wat"))
    {
        return Err("a REPL session case shows only `tir` and `wat` views".to_owned());
    }
    let main = modules.remove("main.hd").ok_or("no main.hd module")?;
    let mut program = Program::new(&main)?;
    program.files = modules.into_iter().collect();
    program.profile = profile;
    program.helpers = helpers;
    program.hosts = hosts;
    program.repl = repl;
    program.fuel = fuel;
    if (main_path.is_some() || !entries.is_empty())
        && (repl.is_some()
            || program.profile.is_some()
            || dumps.iter().any(|d| {
                !matches!(
                    d.as_str(),
                    "diagnostics" | "fixed" | "spans" | "stdout" | "tests"
                )
            }))
    {
        return Err(
            "an `@main` or `@entry` case shows only the `diagnostics`, `fixed`, `spans`, `stdout` and `tests` views, with no profile or REPL"
                .to_owned(),
        );
    }
    if main_path
        .as_ref()
        .is_some_and(|main| program.files.iter().any(|(name, _)| name == main))
    {
        return Err("`@main` names a module the case also declares with `@file`".to_owned());
    }
    program.main = main_path;
    program.entries = entries;
    if filter.is_some() && dumps.iter().any(|d| d != "tests") {
        return Err("a `@filter` case shows only the `tests` view".to_owned());
    }
    program.filter = filter;
    if fuel.is_some()
        && (repl.is_some()
            || dumps.iter().any(|d| {
                !matches!(
                    d.as_str(),
                    "tir" | "diagnostics" | "fixed" | "spans" | "stdout" | "tests"
                )
            }))
    {
        return Err(
            "a `@fuel` case shows only the `tir`, `diagnostics`, `fixed`, `spans`, `stdout` and `tests` views of a package"
                .to_owned(),
        );
    }
    if builds.is_empty() {
        builds.push(("dev".to_owned(), Build::default()));
    }
    Ok(Case {
        program,
        dumps,
        builds,
    })
}

/// Whether `value` is a relative `.hd` path inside the case's package.
fn module_path_ok(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !value.contains('\\')
        && path.extension().is_some_and(|e| e == "hd")
        && path.components().all(|c| matches!(c, Component::Normal(_)))
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

/// The store every view of a test run shares, so that std is prepared once per
/// build setting instead of once per view.
fn shared_store() -> &'static hd_cache::MemoryStore {
    static STORE: std::sync::OnceLock<hd_cache::MemoryStore> = std::sync::OnceLock::new();
    STORE.get_or_init(hd_cache::MemoryStore::default)
}

/// The TIR view: the checked TIR of each body of the program's own
/// package, keeping the instructions whose source lies inside the
/// subject, after the body's path; then the build's diagnostics.
#[must_use]
pub fn tir(p: &Program) -> String {
    let src = p.sources();
    let host = Host {
        render_tir: &["app/*"],
        sources: &src,
        store: shared_store(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let built = match p.repl {
        // The session through input `n`, built as one run, as a host
        // rebuilds a session (`Packages::inputs_from`).
        Some(n) => {
            let session = hd_driver::Session::linked();
            let packages = hd_driver::Packages {
                session: Some(&session),
                inputs_from: Some(1),
                ..hd_driver::Packages::default()
            };
            hd_driver::build_packages(&host, "app", &packages, &Goal::ReplInput { input: n })
        }
        None => hd_driver::build_packages(
            &host,
            "app",
            &hd_driver::Packages {
                body_fuel: p.fuel,
                ..hd_driver::Packages::default()
            },
            &Goal::Analyze,
        ),
    };
    let main_file = p.main_file();
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
                && file == main_file
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

/// The packages of a `diagnostics`, `spans`, `fixed`, `stdout` or `tests`
/// build of `p` as `build`: the case's `@fuel`, `@profile` and `@entry`,
/// and `@build`'s level (`release` runs the optimized pipeline, and a
/// program gets the release profile), engine target and checked runtime.
/// Every build verifies the ownership passes' output
/// (`Packages::verify_own`), as a debug-built compiler always does, so a
/// test run of a release-built harness (CI's) checks the same. It costs
/// only the case package's whole-result reuse, which a case never repeats.
fn packages(p: &Program, build: Build) -> hd_driver::Packages<'static> {
    let mut packages = hd_driver::Packages {
        body_fuel: p.fuel,
        profile: p.profile.clone(),
        entries: p.entries.clone(),
        checked: build.checked,
        verify_own: true,
        target: build.target,
        ..hd_driver::Packages::default()
    };
    if build.release {
        packages.level = hd_driver::OptLevel::Optimized;
        packages.build_profile = hd_driver::BuildProfile::Release;
    }
    packages
}

/// The diagnostics view: every diagnostic of the case's `Analyze` build,
/// as `severity: file:line:column: code: message`, in source order, each
/// followed by its fix-its' edits ([`diagnostics::render`]). With
/// `@build: tests` it checks the `tests:` blocks too (`Goal::CheckTests`,
/// as `hd check --tests` does).
#[must_use]
pub fn diagnostics(p: &Program, build: Build) -> String {
    diagnostics::render(&check(p, build, &p.sources()))
}

/// The `diagnostics` view's build of `p` as `build`, from `src`: the
/// case's own sources, or the `fixed` view's edited ones.
fn check(p: &Program, build: Build, src: &hd_project::MemorySources) -> hd_driver::Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store: shared_store(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = packages(p, build);
    let goal = if build.tests {
        Goal::CheckTests
    } else {
        Goal::Analyze
    };
    hd_driver::build_packages(&host, "app", &packages, &goal)
}

/// The tests view: the `tests:` blocks built as `hd test` builds them
/// (`Goal::Tests`, with the case's `@filter`), as the build's diagnostics, then the test plan, one
/// case per line (`kind "name" file:line`, with its `ignore`, expected
/// panic or reason it cannot run), then, when the build has no errors,
/// each result record of the runnable cases from the shared Node host
/// ([`node_host::run_tests`]), after its case's name.
#[must_use]
pub fn tests(p: &Program, at: Build) -> String {
    let src = p.sources();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: shared_store(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = packages(p, at);
    let goal = Goal::Tests {
        module: None,
        filter: p.filter.clone(),
    };
    let built = hd_driver::build_packages(&host, "app", &packages, &goal);
    let mut out = diagnostics::render(&built);
    let _ = writeln!(out, "-- plan");
    for case in &built.tests {
        let _ = write!(
            out,
            "{} {:?} {}:{}",
            case.kind, case.name, case.file, case.line
        );
        for (label, value) in [
            ("ignore", &case.ignore),
            ("expect panic", &case.expect_panic),
            ("cannot run", &case.unsupported),
        ] {
            if let Some(value) = value {
                let _ = write!(out, "; {label}: {value}");
            }
        }
        out.push('\n');
    }
    let runs: Vec<_> = built
        .tests
        .iter()
        .filter(|case| case.ignore.is_none())
        .filter_map(|case| case.run.map(|run| (case, run)))
        .collect();
    if built.diags.has_errors() || runs.is_empty() {
        return out;
    }
    let Some(wasm) = &built.wasm else {
        out.push_str("-- no program\n");
        return out;
    };
    let list: Vec<String> = runs
        .iter()
        .map(|(case, (test, init))| format!("{test}:{init}:{}", case.kind))
        .collect();
    let _ = writeln!(out, "-- results");
    match node_host::run_tests(wasm, &list.join(","), RUN_LIMIT) {
        Ok(ran) if ran.exit == 0 => {
            for line in String::from_utf8_lossy(&ran.stdout).lines() {
                // `{"test":N,...}`: the case's export index, then the rest.
                let parsed = line.strip_prefix("{\"test\":").and_then(|rest| {
                    let (index, rest) = rest.split_once(',')?;
                    Some((index.parse::<u32>().ok()?, rest))
                });
                match parsed.and_then(|(index, rest)| {
                    runs.iter()
                        .find(|(_, (test, _))| *test == index)
                        .map(|(case, _)| (case, rest))
                }) {
                    Some((case, rest)) => {
                        let _ = writeln!(out, "{}: {{{rest}", case.name);
                    }
                    None => {
                        let _ = writeln!(out, "{line}");
                    }
                }
            }
        }
        Ok(ran) => out.push_str(&shown_run(&ran.exit.to_string(), &ran.stdout, &ran.stderr)),
        Err(error) => {
            let _ = writeln!(out, "host failed: {error}");
        }
    }
    out
}

/// The stdout view: the program built from `main` on the dev pipeline (or
/// as `@build` says) and run on the default host, as its exit status, standard output and
/// standard error; or its diagnostics when it does not build. Every run of
/// the test process goes to one long-lived Node host ([`node_host`]),
/// which reports what a lone `node host/run.ts` run of the program would.
#[must_use]
pub fn stdout(p: &Program, build: Build) -> String {
    let wasm = match program_wasm(p, build) {
        Ok(wasm) => wasm,
        Err(failed) => return failed,
    };
    match node_host::run(&wasm, &[], RUN_LIMIT) {
        Ok(ran) => shown_run(&ran.exit.to_string(), &ran.stdout, &ran.stderr),
        Err(error) => format!("host failed: {error}\n"),
    }
}

/// How long one `stdout` case's program may run on the shared host. A
/// stage program runs in milliseconds; one that overruns fails alone.
const RUN_LIMIT: std::time::Duration = std::time::Duration::from_mins(1);

/// The program of `p` built from `main` as `build` says (the dev pipeline
/// by default), or the `stdout` view's text for a build that fails.
///
/// # Errors
/// The build reports diagnostics or writes no Wasm.
pub fn program_wasm(p: &Program, at: Build) -> Result<Vec<u8>, String> {
    let src = p.sources();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: shared_store(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let built = hd_driver::build_packages(
        &host,
        "app",
        &packages(p, at),
        &Goal::Program {
            entry: "main".into(),
        },
    );
    match built.wasm {
        Some(wasm) if built.diags.is_empty() => Ok(wasm),
        _ => Err(format!("build failed\n{}", diagnostics::render(&built))),
    }
}

/// The `stdout` view's text of a run: its exit status, then each
/// non-empty stream.
#[must_use]
pub fn shown_run(exit: &str, stdout: &[u8], stderr: &[u8]) -> String {
    let mut out = format!("exit: {exit}\n");
    for (name, bytes) in [("stdout", stdout), ("stderr", stderr)] {
        let text = String::from_utf8_lossy(bytes);
        if !text.is_empty() {
            let _ = writeln!(out, "-- {name}");
            out.push_str(&text);
            if !text.ends_with('\n') {
                out.push_str("\n<no newline at end>\n");
            }
        }
    }
    out
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
        "diagnostics" => diagnostics(p, build),
        "fixed" => diagnostics::fixed(p, build),
        "spans" => diagnostics::spans(p, build),
        "stdout" => stdout(p, build),
        "tests" => tests(p, build),
        _ => return None,
    })
}

/// The build a view shows: its engine target (`v8`, the default,
/// `wasmtime` or `browser`) and its level (`dev`, the default, or
/// `release`: the optimized pipeline and the release profile, as
/// `--release` builds). `std` builds the case as module `std.rt_tests.stage_case`
/// of the standard library, which may use the runtime modules
/// (`module.rt.std-only`); the semantic and Wasm views read it.
/// `checked` links the checked runtime (`crates/hd_wasm/DESIGN.md`
/// §23.24), whose walks mark buffers, as checked conformance runs do.
/// `program` links from `pub fn main`, including the runtime helpers a
/// fixture build leaves out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Build {
    pub target: hd_driver::EngineTarget,
    pub release: bool,
    pub std: bool,
    pub checked: bool,
    /// Link a complete program, runtime helpers included.
    pub program: bool,
    /// The `diagnostics`, `spans` and `fixed` views check the `tests:`
    /// blocks too.
    pub tests: bool,
}

impl Build {
    fn goal(self) -> Goal {
        let module = if self.std {
            "rt_tests.stage_case"
        } else {
            "main"
        }
        .to_owned();
        if self.program {
            Goal::Program { entry: module }
        } else {
            Goal::Fixture { module }
        }
    }

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
                "std" => build.std = true,
                "program" => build.program = true,
                "checked" => build.checked = true,
                "tests" => build.tests = true,
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

/// What a crate's stage tests dump for a dump name, a build and a program.
pub type Views<'a> = dyn Fn(&str, Build, &Program) -> Option<String> + Sync + 'a;

/// Runs every `.hd` case recursively under `dir`, comparing each dump
/// with its mirrored baseline. `HD_BLESS=1` writes only baselines.
///
/// # Panics
/// Invalid cases, missing baselines, view differences, or an empty directory.
pub fn run_dir(dir: &Path, views: &Views<'_>) {
    let bless = std::env::var_os("HD_BLESS").as_deref() == Some(std::ffi::OsStr::new("1"));
    run_cases(dir, views, bless);
}

fn run_cases(dir: &Path, views: &Views<'_>, bless: bool) {
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
    files.sort_stable();
    assert!(!files.is_empty(), "no `.hd` cases in {}", dir.display());
    let baseline_root = dir.parent().expect("tests directory").join("baselines");
    // Cases run on a pool of threads; results merge in file order, so the
    // failures read as a sequential run's.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<CaseResult>>> =
        files.iter().map(|_| std::sync::Mutex::new(None)).collect();
    // `HD_TEST_JOBS` caps the pool, as it does for conformance: the machine is
    // shared, and `RUST_TEST_THREADS` limits only libtest's own threads.
    let workers = std::env::var("HD_TEST_JOBS")
        .ok()
        .and_then(|jobs| jobs.parse::<usize>().ok())
        .filter(|&jobs| jobs >= 1)
        .or_else(|| {
            std::thread::available_parallelism()
                .ok()
                .map(std::num::NonZero::get)
        })
        .unwrap_or(4)
        .min(files.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let result = run_case(dir, &baseline_root, path, views, bless);
                    *slots[i].lock().expect("case slot") = Some(result);
                }
            });
        }
    });
    let mut expected_baselines = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for slot in slots {
        let result = slot.into_inner().expect("case slot").expect("case ran");
        expected_baselines.extend(result.baselines);
        failures.extend(result.failures);
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

/// One case's baselines, and the failures its dumps report.
struct CaseResult {
    baselines: Vec<PathBuf>,
    failures: Vec<String>,
}

fn run_case(
    dir: &Path,
    baseline_root: &Path,
    path: &Path,
    views: &Views<'_>,
    bless: bool,
) -> CaseResult {
    let mut baselines = Vec::new();
    let mut failures = Vec::new();
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let case = match parse(&text) {
        Ok(case) => case,
        Err(error) => {
            failures.push(format!("{}: {error}", path.display()));
            return CaseResult {
                baselines,
                failures,
            };
        }
    };
    let relative = path.strip_prefix(dir).expect("case under root");
    for dump in &case.dumps {
        // One build shows its view alone; several each show theirs under
        // a header that names the build.
        let mut output = Some(String::new());
        for (spec, build) in &case.builds {
            let (Some(out), Some(shown)) = (&mut output, views(dump, *build, &case.program)) else {
                output = None;
                break;
            };
            if case.builds.len() > 1 {
                let _ = writeln!(out, "== @build: {spec}");
            }
            out.push_str(&shown);
        }
        let Some(output) = output else {
            failures.push(format!("{}: unknown dump `{dump}`", path.display()));
            continue;
        };
        let prefix = if dump == "wat" { ";;" } else { "#" };
        let actual = format!(
            "{prefix} Generated by HD_BLESS=1 from {}; do not edit.\n{output}",
            relative.to_string_lossy().replace('\\', "/")
        );
        let baseline = baseline_root.join(relative).with_extension(dump);
        baselines.push(baseline.clone());
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
    inspection::forget();
    CaseResult {
        baselines,
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::{Build, Program, diff, parse, tokens};

    #[test]
    fn headers_select_dumps_builds_and_modules() {
        let case = parse("# @dump: tir wat\n# @build: wasmtime release\n# @file: main.hd\nfn one() -> i32: +1\n# @file: other.hd\npub fn two() -> i32: +2\n").expect("case");
        assert_eq!(case.dumps, ["tir", "wat"]);
        assert_eq!(
            case.builds,
            [(
                "wasmtime release".to_owned(),
                Build::parse("wasmtime release").unwrap()
            )]
        );
        assert!(case.program.text.contains("fn one"));
        assert!(!case.program.text.contains("fn two"));
        assert_eq!(case.program.files[0].0, "other.hd");
        let case = parse("# @dump: stdout\n# @build: dev | release checked\n").expect("case");
        assert_eq!(
            case.builds,
            [
                ("dev".to_owned(), Build::default()),
                (
                    "release checked".to_owned(),
                    Build::parse("release,checked").unwrap()
                )
            ]
        );
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
            "# @dump: tir\n# @build: dev | fast\n",
            "# @dump: tir\n# @build: dev | v8 dev\n",
            "# @dump: tir\n# @build: dev\n# @build: release\n",
            "# @dump: tests\nx := 1\n# @filter: name\n",
            "# @dump: tests diagnostics\n# @filter: name\n",
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
        let (_, dev) = case.builds[0];
        let built = super::inspection::build(&case.program, dev);
        assert!(!built.diags.has_errors(), "{}", built.render());
        for view in ["tir", "own", "wat", "counts", "layout"] {
            let out = super::view(view, dev, &case.program).unwrap();
            assert!(out.contains("fn result()"), "{view}: {out}");
            assert!(!out.contains("pub fn answer()"), "{view}: {out}");
        }
    }
}
