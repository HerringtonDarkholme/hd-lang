//! The specification conformance suite through the new compiler.
//!
//! Every indexed case must pass, except a case listed in the root
//! `KNOWN_FAILURES.tsv` (`hd_testkit::known_failures`) for the judging
//! engine; a listed case that passes fails the test too, so the list stays
//! exact. Unsupported surface counts as not passing, and crashes always
//! fail. A full run prints the pass, known, fail and unsupported counts and
//! the failure buckets. The CLI tier runs in
//! `hd_cli/tests/cli_conformance.rs` under the same rule.
//!
//! `HD_CONFORMANCE_ENGINE=wasmtime` compares runtime programs, including
//! fixture profiles, on both engines and judges by wasmtime.
//! `wasmtime-only` never launches Node for entries or test programs.
//! Each engine runs a build tuned for its own target (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5,
//! Q3): Node runs the `V8` build and wasmtime the `Wasmtime` build, so
//! compare mode builds each runnable program once per target. The
//! default, Node, builds for `V8` only.
//! Test outcomes are compared without their nondeterministic elapsed time.
//! Neither mode falls back to Node after a wasmtime error.
//! Both need the `wasmtime` feature (`cargo test --features wasmtime`).
//! `HD_CONFORMANCE_PIPELINE=optimized` builds every case through the
//! optimized pipeline (`hd_opt::passes::OPTIMIZED`) instead of the dev one, with
//! the checked build profile, under the same rule. `dev` is the default.
//! `HD_CONFORMANCE_WASM_DIGEST=record|check` records the hash of every
//! Wasm module a runtime case runs (its entry under each profile, its test
//! programs and its scenario drive), in
//! `target/conformance-wasm-digests-<target>-<pipeline>.tsv` for each
//! target the engine mode builds, or compares the run with those saved
//! files and fails on any difference. It proves a refactor leaves the
//! emitted code byte-identical; it needs the full runtime suite.
//! `HD_CONFORMANCE_TIER=lang|std` selects that tier's runtime cases for
//! timing, without applying the rule.
//!
//! The runtime cases of the root `LINEAR_PASSES.tsv` (`path<TAB>note`)
//! also run once more through the linear-memory backend
//! (`hd_driver::Backend::Linear`, `crates/hd_wasm/DESIGN.md` §23), on
//! the same engines, and each must pass: the list is a ratchet that grows
//! as the backend's jobs land. `HD_CONFORMANCE_BACKEND=linear` runs every
//! runtime case through the linear backend as well and prints the passing
//! cases the list does not name yet, without failing on them. A linear
//! run's digests and runtime statistics are noted under `linear:PATH`.

//! `HD_TEST_TIMINGS=1` logs per-case or per-command wall times for profiling.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use hd_base::Stage;
use hd_cache::MemoryStore;
use hd_diag::Severity;
use hd_driver::{
    Backend, Dependency, EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Output, Packages,
    PipelineReport, ScenarioHold, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_syntax::parse;
use hd_testkit::Scratch;
use hd_testkit::known_failures::{self, Outcome};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Parse,
    Type,
    Runtime,
}

#[derive(Clone, Debug)]
struct Case {
    path: String,
    phase: Phase,
    expectation: String,
    specification: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Fail(String),
    Unsupported(String),
    Crash(String),
}

#[derive(Default)]
struct Counts {
    pass: usize,
    fail: usize,
    unsupported: usize,
}

struct Fixture {
    sources: MemorySources,
    primary: String,
    text: String,
    tree: bool,
    /// The dependencies of a package-role fixture: each package under
    /// `packages/`, by name (README "Package Roles").
    deps: Vec<(String, MemorySources)>,
    /// The backend its builds link with.
    backend: Backend,
    /// The package its sources are: `fixture`, or `std` for the hd tests
    /// of std's runtime modules ([`STD_RT_TESTS`]).
    package: &'static str,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn suite_root() -> PathBuf {
    root().join("spec/conformance")
}

fn cases() -> Vec<Case> {
    let text = std::fs::read_to_string(suite_root().join("cases.tsv")).expect("cases.tsv");
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 4, "bad cases.tsv row: {line}");
        let phase = match fields[1] {
            "parse" => Phase::Parse,
            "type" => Phase::Type,
            "runtime" => Phase::Runtime,
            other => panic!("unknown conformance phase {other}"),
        };
        out.push(Case {
            path: fields[0].to_owned(),
            phase,
            expectation: fields[2].to_owned(),
            specification: fields[3].to_owned(),
        });
    }
    out
}

fn directive<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("# {name}: ");
    text.lines().find_map(|line| line.strip_prefix(&prefix))
}

fn load_tree(root: &Path, dir: &Path, sources: &mut MemorySources) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("fixture tree directory")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            load_tree(root, &path, sources);
        } else if path.extension().is_some_and(|extension| extension == "hd") {
            let relative = path
                .strip_prefix(root)
                .expect("tree root")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&path).expect("fixture tree file");
            sources.insert(&relative, &text);
        }
    }
}

/// The packages of the multi-package environment: each directory
/// `packages/NAME/`, in ascending order of NAME, as a package whose source
/// root holds the directory's files.
fn role_packages() -> Vec<(String, MemorySources)> {
    let dir = suite_root().join("packages");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("packages directory")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|path| {
            let mut flat = MemorySources::default();
            load_tree(&path, &path, &mut flat);
            let mut sources = MemorySources::default();
            for entry in hd_project::SourceSet::list(&flat) {
                let text = hd_project::SourceSet::read(&flat, &entry.path).expect("package file");
                sources.insert(
                    &format!("src/{}", entry.path),
                    &String::from_utf8_lossy(&text),
                );
            }
            let name = path
                .file_name()
                .expect("package name")
                .to_string_lossy()
                .into_owned();
            (name, sources)
        })
        .collect()
}

fn fixture(case: &Case) -> Result<Fixture, String> {
    let file = suite_root().join(&case.path);
    let text = std::fs::read_to_string(&file).map_err(|error| error.to_string())?;
    let mut sources = MemorySources::default();
    let mut tree = false;
    let mut deps = Vec::new();
    let primary = if let Some(role) = directive(&text, "fixture-package-role") {
        // The primary file is the root module of a package in the role,
        // which depends on every package under `packages/`, each a
        // library whose `lib.hd` is its root module.
        deps = role_packages();
        match role {
            "library" => "src/lib.hd".to_owned(),
            "root-application" => "src/main.hd".to_owned(),
            _ => return Err("Discover".to_owned()),
        }
    } else if let Some(value) = directive(&text, "fixture-package-tree") {
        let Some((name, path)) = value.split_once('/') else {
            return Err("Discover".to_owned());
        };
        let tree_root = suite_root().join("trees").join(name);
        load_tree(&tree_root, &tree_root, &mut sources);
        tree = true;
        path.to_owned()
    } else if let Some(layout) = directive(&text, "fixture-test-layout") {
        let stem = file.file_stem().expect("fixture stem").to_string_lossy();
        match layout {
            "test-module" => format!("src/{stem}_test.hd"),
            "integration" => format!("tests/{stem}.hd"),
            _ => return Err("Discover".to_owned()),
        }
    } else {
        "main.hd".to_owned()
    };
    sources.insert(&primary, &text);
    Ok(Fixture {
        sources,
        primary,
        text,
        tree,
        deps,
        backend: Backend::WasmGc,
        package: "fixture",
    })
}

fn first_unsupported(report: &PipelineReport, back_half: bool) -> Option<String> {
    if report.body_failed > 0 {
        return Some("Body".to_owned());
    }
    // `Goal::CheckTests` checks a package without a program root, so it
    // never collects: the back half is judged by the `Goal::Program` build.
    Stage::ALL
        .into_iter()
        .take_while(|stage| back_half || *stage != Stage::Collect)
        .find_map(|stage| {
            let tally = report.tally(stage);
            // The linear backend's reason names what it does not lower
            // yet, which buckets its known failures.
            let linear = tally
                .first_reason
                .as_deref()
                .and_then(|r| r.strip_suffix(" in the linear backend"));
            (tally.not_implemented > 0).then(|| match linear {
                Some(what) => format!("{}: {what}", stage.name()),
                None => stage.name().to_owned(),
            })
        })
}

fn build_fixture(fixture: &Fixture, store: &MemoryStore, goal: &Goal) -> Output {
    build_sources(fixture, &fixture.sources, store, goal)
}

/// Builds `sources` as the fixture's package, with the fixture's
/// dependencies, for the target of the engine that judges the case.
fn build_sources(
    fixture: &Fixture,
    sources: &MemorySources,
    store: &MemoryStore,
    goal: &Goal,
) -> Output {
    build_for(fixture, sources, store, goal, engine_mode().judged_target())
}

/// A runnable goal's builds: the judged one, for the judging engine's
/// target, and in compare mode Node's own build for `V8`.
struct Runnable {
    out: Output,
    node: Option<Output>,
}

impl Runnable {
    /// The modules the engines run, once the judged build made one.
    fn modules<'a>(&'a self, judged: &'a [u8]) -> Modules<'a> {
        Modules {
            judged,
            node: match &self.node {
                None => NodeModule::Judged,
                Some(Output {
                    wasm: Some(wasm), ..
                }) => NodeModule::Own(wasm),
                Some(_) => NodeModule::Missing,
            },
        }
    }
}

/// [`build_sources`] for a goal that makes a module to run, built once
/// more for `V8` in compare mode.
fn build_runnable(
    fixture: &Fixture,
    sources: &MemorySources,
    store: &MemoryStore,
    goal: &Goal,
) -> Runnable {
    let out = build_sources(fixture, sources, store, goal);
    let node = (engine_mode() == EngineMode::Compare)
        .then(|| build_for(fixture, sources, store, goal, EngineTarget::V8));
    Runnable { out, node }
}

/// A program as each engine of the mode runs it: the judged module, and
/// the module Node runs.
#[derive(Clone, Copy)]
struct Modules<'a> {
    judged: &'a [u8],
    node: NodeModule<'a>,
}

/// The module Node runs: the judged one in Node mode; in compare mode the
/// module of its own `V8` build, or none when that build made none.
#[derive(Clone, Copy)]
enum NodeModule<'a> {
    Judged,
    Own(&'a [u8]),
    Missing,
}

impl<'a> Modules<'a> {
    /// The module Node runs.
    fn node(self) -> std::io::Result<&'a [u8]> {
        match self.node {
            NodeModule::Judged => Ok(self.judged),
            NodeModule::Own(wasm) => Ok(wasm),
            NodeModule::Missing => Err(std::io::Error::other(
                "the V8 build made no module where the Wasmtime build did",
            )),
        }
    }
}

/// [`build_sources`] for `target`.
fn build_for(
    fixture: &Fixture,
    sources: &MemorySources,
    store: &MemoryStore,
    goal: &Goal,
    target: EngineTarget,
) -> Output {
    let host = Host {
        render_tir: &[],
        sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: fixture
            .deps
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.clone(), u16::try_from(i + 1).expect("packages")))
            .collect(),
        deps: fixture
            .deps
            .iter()
            .map(|(name, sources)| Dependency {
                name: name.clone(),
                identity: name.clone(),
                sources,
                requires: Vec::new(),
                fetched: false,
            })
            .collect(),
        profile: profile(fixture),
        target,
        level: level(),
        backend: fixture.backend,
        ..Packages::default()
    };
    build_packages(&host, fixture.package, &packages, goal)
}

/// The host capability traits of the fixture's runtime profile, as the
/// runner passes it to `check` and `test` (README "Runtime Profiles"): the
/// fixture's own traits it names, in the primary module, or the prelude
/// `Console`. `console`, the profile of a fixture that names none, selects
/// no profile.
fn profile(fixture: &Fixture) -> Option<Vec<String>> {
    let module = hd_project::module_path("fixture", &fixture.primary);
    let own = |traits: &[&str]| Some(traits.iter().map(|t| format!("{module}.{t}")).collect());
    match directive(&fixture.text, "fixture-runtime-profile")? {
        "serde-vault" => own(&["Vault"]),
        "disposed-file" => own(&["Files", "FileHandle"]),
        "pending-gate" => own(&["Gate"]),
        "misbehaving-host" => own(&["Gauge"]),
        "special-float-host" => own(&["Sensor"]),
        "pending-write" => Some(vec!["std.console.Console".to_owned()]),
        _ => None,
    }
}

fn marker_line(text: &str, kind: &str) -> Option<usize> {
    let marker = format!("# {kind}: ");
    text.lines()
        .position(|line| line.contains(&marker))
        .map(|line| line + 1)
}

#[allow(clippy::naive_bytecount)]
fn byte_line(text: &str, byte: u32) -> usize {
    let at = usize::try_from(byte).unwrap_or(text.len()).min(text.len());
    text.as_bytes()[..at]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn parse_case(case: &Case, text: &str) -> Verdict {
    let parsed = parse(text.as_bytes());
    match case.expectation.as_str() {
        "accept" if parsed.diagnostics.is_empty() => Verdict::Pass,
        "accept" => Verdict::Fail(parsed.diagnostics[0].code.as_str().to_owned()),
        expectation if expectation.starts_with("reject:") => {
            let expected = expectation.trim_start_matches("reject:");
            let errors: Vec<_> = parsed
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == Severity::Error)
                .collect();
            let line = marker_line(text, "diagnostic");
            if errors.len() == 1
                && errors[0].code.as_str() == expected
                && line.is_none_or(|line| byte_line(text, errors[0].primary.lo) == line)
            {
                Verdict::Pass
            } else {
                Verdict::Fail(errors.first().map_or_else(
                    || "no-diagnostic".to_owned(),
                    |d| d.code.as_str().to_owned(),
                ))
            }
        }
        expectation if expectation.starts_with("warn:") => {
            let expected = expectation.trim_start_matches("warn:");
            let no_errors = parsed
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != Severity::Error);
            let line = marker_line(text, "warning");
            let found = parsed.diagnostics.iter().any(|diagnostic| {
                diagnostic.severity == Severity::Warning
                    && diagnostic.code.as_str() == expected
                    && line.is_none_or(|line| byte_line(text, diagnostic.primary.lo) == line)
            });
            if no_errors && found {
                Verdict::Pass
            } else {
                Verdict::Fail(parsed.diagnostics.first().map_or_else(
                    || "no-diagnostic".to_owned(),
                    |d| d.code.as_str().to_owned(),
                ))
            }
        }
        _ => Verdict::Fail("unsupported-expectation".to_owned()),
    }
}

/// The first error in content order; a warning that sorts before it is
/// not the reason a build failed.
fn first_error(output: &Output) -> usize {
    output
        .diags
        .content_order()
        .into_iter()
        .find(|index| output.diags.severity[*index] == Severity::Error)
        .unwrap_or(0)
}

/// The errors of a check in content order, but for `unsupported`: a stage
/// that declines is progress data, which the report's stage tallies carry
/// (`first_unsupported`), not a diagnostic the fixture is judged on.
fn check_errors(output: &Output) -> Vec<usize> {
    output
        .diags
        .content_order()
        .into_iter()
        .filter(|index| {
            output.diags.severity[*index] == Severity::Error
                && output.diags.code[*index].as_str() != "unsupported"
        })
        .collect()
}

fn diagnostic_verdict(case: &Case, fixture: &Fixture, output: &Output) -> Option<Verdict> {
    let errors = check_errors(output);
    let warnings: Vec<usize> = output
        .diags
        .content_order()
        .into_iter()
        .filter(|index| output.diags.severity[*index] == Severity::Warning)
        .collect();
    if case.expectation == "accept" {
        return errors
            .first()
            .map(|index| Verdict::Fail(output.diags.code[*index].as_str().to_owned()));
    }
    let (kind, expected) = case
        .expectation
        .split_once(':')
        .unwrap_or((case.expectation.as_str(), ""));
    let line_kind = if kind == "warn" {
        "warning"
    } else {
        "diagnostic"
    };
    let on_marker = |index: usize| {
        fixture.tree
            || marker_line(&fixture.text, line_kind).is_none_or(|line| {
                let span = output.diags.primary[index];
                output.files.get(span.file.idx()) == Some(&fixture.primary)
                    && byte_line(&fixture.text, span.lo) == line
            })
    };
    if kind == "warn" {
        // `warn:CODE`: "exit 0, and a located warning `CODE` on the marker
        // line"; warnings never fail a case, so other warnings are allowed.
        let found = warnings
            .iter()
            .any(|index| output.diags.code[*index].as_str() == expected && on_marker(*index));
        if errors.is_empty() && found {
            return Some(Verdict::Pass);
        }
    } else {
        let only_one = errors.len() == 1;
        let code_ok = errors
            .first()
            .is_some_and(|index| output.diags.code[*index].as_str() == expected);
        if only_one && code_ok && errors.first().is_some_and(|index| on_marker(*index)) {
            return Some(Verdict::Pass);
        }
    }
    errors
        .first()
        .or_else(|| warnings.first())
        .map(|index| Verdict::Fail(output.diags.code[*index].as_str().to_owned()))
}

/// `check FILE`, which the runner always gives `--tests`, so it checks
/// the fixture's test code too (README "Command Contract").
///
/// A package-tree fixture also checks the doc tests of its primary file,
/// each as an integration-view module at `tests/$docN.hd`, but for the
/// compile-fail ones, as `hd check --tests` does (README "Package Trees").
fn check_fixture(fixture: &Fixture, store: &MemoryStore) -> Output {
    if !fixture.tree {
        return build_fixture(fixture, store, &Goal::CheckTests);
    }
    let mut sources = fixture.sources.clone();
    for (index, doc) in hd_project::doc_tests(&fixture.primary, &fixture.text)
        .into_iter()
        .enumerate()
    {
        if doc.compile_fail.is_none() {
            sources.insert(&format!("tests/$doc{index}.hd"), &doc.program);
        }
    }
    build_sources(fixture, &sources, store, &Goal::CheckTests)
}

fn type_case(case: &Case, fixture: &Fixture, store: &MemoryStore) -> Verdict {
    let output = check_fixture(fixture, store);
    if let Some(verdict) = diagnostic_verdict(case, fixture, &output) {
        return verdict;
    }
    if let Some(stage) = first_unsupported(&output.report, false) {
        return Verdict::Unsupported(stage);
    }
    if case.expectation == "accept" {
        return Verdict::Pass;
    }
    Verdict::Fail("no-diagnostic".to_owned())
}

/// The scenario that runs Runtime Execution steps 1 and 2 with its hook
/// instead of replacing them (README "Runtime Scenarios").
const PENDING_FIRST_POLL: &str = "pending-first-poll";

/// Whether the fixture's runtime scenario replaces Runtime Execution steps
/// 1 and 2 with its own procedure: every scenario but `pending-first-poll`
/// (README "Runtime Execution").
fn replacing_scenario(text: &str) -> bool {
    directive(text, "fixture-runtime-scenario").is_some_and(|name| name != PENDING_FIRST_POLL)
}

fn pending_first_poll(text: &str) -> bool {
    directive(text, "fixture-runtime-scenario") == Some(PENDING_FIRST_POLL)
}

fn has_runtime_harness(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with("tests:") || line.starts_with("## "))
        || directive(text, "fixture-runtime-profile").is_some()
        || replacing_scenario(text)
        || directive(text, "fixture-runtime-pending-function").is_some()
        || directive(text, "fixture-test-layout").is_some()
        || directive(text, "fixture-package-tree").is_some()
}

/// The README's Standard Output: each `# expect-stdout: TEXT` line, or
/// `# expect-stdout:` for an empty one, decoded and followed by a newline.
/// An invalid escape makes the fixture invalid: `Err`.
fn expected_stdout(text: &str) -> Option<Result<String, String>> {
    if directive(text, "expect-empty-stdout") == Some("true") {
        return Some(Ok(String::new()));
    }
    let lines: Vec<&str> = text
        .lines()
        .filter_map(|line| {
            line.strip_prefix("# expect-stdout: ")
                .or_else(|| (line == "# expect-stdout:").then_some(""))
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut out = String::new();
    for line in lines {
        match decode_stdout_line(line) {
            Ok(decoded) => out.push_str(&decoded),
            Err(e) => return Some(Err(e)),
        }
        out.push('\n');
    }
    Some(Ok(out))
}

/// `TEXT` taken literally, except `\\` (a backslash), `\t` (U+0009) and
/// `\u{H}` (the scalar value of 1 to 6 hexadecimal digits).
fn decode_stdout_line(text: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('u') => {
                if chars.next() != Some('{') {
                    return Err(format!("bad escape in `{text}`"));
                }
                let mut digits = String::new();
                loop {
                    match chars.next() {
                        Some('}') if !digits.is_empty() => break,
                        Some(d) if d.is_ascii_hexdigit() && digits.len() < 6 => digits.push(d),
                        _ => return Err(format!("bad \\u escape in `{text}`")),
                    }
                }
                let scalar = u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| format!("bad \\u scalar in `{text}`"))?;
                out.push(scalar);
            }
            _ => return Err(format!("bad escape in `{text}`")),
        }
    }
    Ok(out)
}

/// A program run's standard output and error, and its status.
struct Ran {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: Option<i32>,
}

impl Ran {
    fn success(&self) -> bool {
        self.status == Some(0)
    }
}

// The wasmtime half of the engine modes. Without the feature the modes are
// refused (`EngineMode::from_value`), so `no_wasmtime.rs` never runs.
#[cfg_attr(feature = "wasmtime", path = "conformance/wasmtime.rs")]
#[cfg_attr(not(feature = "wasmtime"), path = "conformance/no_wasmtime.rs")]
mod wasmtime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineMode {
    Node,
    Compare,
    WasmtimeOnly,
}

impl EngineMode {
    fn from_value(value: Option<&str>) -> Self {
        let mode = match value {
            None | Some("" | "node") => Self::Node,
            Some("wasmtime") => Self::Compare,
            Some("wasmtime-only") => Self::WasmtimeOnly,
            Some(other) => panic!("unknown HD_CONFORMANCE_ENGINE: {other}"),
        };
        assert!(
            mode == Self::Node || cfg!(feature = "wasmtime"),
            "HD_CONFORMANCE_ENGINE={} needs the wasmtime engine; rerun with --features wasmtime",
            value.unwrap_or_default()
        );
        mode
    }
}

impl EngineMode {
    /// The engine that judges this mode's runs, as the summaries name it.
    const fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Compare | Self::WasmtimeOnly => "wasmtime",
        }
    }

    /// The target of the builds this mode judges: wasmtime judges both
    /// wasmtime modes, so they build for `Wasmtime`; Node builds for `V8`.
    const fn judged_target(self) -> EngineTarget {
        match self {
            Self::Node => EngineTarget::V8,
            Self::Compare | Self::WasmtimeOnly => EngineTarget::Wasmtime,
        }
    }
}

fn engine_mode() -> EngineMode {
    EngineMode::from_value(selection_env("HD_CONFORMANCE_ENGINE").as_deref())
}

fn selection_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => panic!("{name} must contain valid UTF-8"),
    }
}

/// Whether `HD_CONFORMANCE_PIPELINE=optimized` selects the optimized
/// pipeline for every build.
fn optimized_pipeline() -> bool {
    match selection_env("HD_CONFORMANCE_PIPELINE").as_deref() {
        None | Some("" | "dev") => false,
        Some("optimized") => true,
        Some(other) => panic!("unknown HD_CONFORMANCE_PIPELINE: {other}"),
    }
}

/// The selected pipeline's name, for the summary lines.
fn pipeline_name() -> &'static str {
    if optimized_pipeline() {
        "optimized"
    } else {
        "dev"
    }
}

/// The optimization level of every build, and of wasmtime's engine.
fn level() -> OptLevel {
    if optimized_pipeline() {
        OptLevel::Optimized
    } else {
        OptLevel::Dev
    }
}

#[test]
fn engine_selection_is_explicit() {
    for value in [None, Some(""), Some("node")] {
        assert_eq!(EngineMode::from_value(value), EngineMode::Node);
    }
    #[cfg(feature = "wasmtime")]
    {
        assert_eq!(
            EngineMode::from_value(Some("wasmtime")),
            EngineMode::Compare
        );
        assert_eq!(
            EngineMode::from_value(Some("wasmtime-only")),
            EngineMode::WasmtimeOnly
        );
    }
}

/// Each engine runs a build tuned for its own target (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5,
/// Q3): the judged build is for the judging engine's target, and compare
/// mode builds once more for `V8`, the module Node runs. Both at the
/// selected level.
#[test]
fn each_engine_runs_a_build_for_its_own_target() {
    assert_eq!(EngineMode::Node.judged_target(), EngineTarget::V8);
    assert_eq!(EngineMode::Compare.judged_target(), EngineTarget::Wasmtime);
    assert_eq!(
        EngineMode::WasmtimeOnly.judged_target(),
        EngineTarget::Wasmtime
    );
    let fixture = unit_fixture("pub fn main() -> void $ Console:\n    println(\"open\")\n");
    let built = build_runnable(
        &fixture,
        &fixture.sources,
        &MemoryStore::default(),
        &Goal::Program {
            entry: "main".to_owned(),
        },
    );
    let mode = engine_mode();
    assert_eq!(
        (built.out.target, built.out.level),
        (mode.judged_target(), level())
    );
    let node = built.node.as_ref().map(|n| (n.target, n.level));
    if mode == EngineMode::Compare {
        assert_eq!(node, Some((EngineTarget::V8, level())));
    } else {
        assert_eq!(node, None);
    }
    let wasm = built.out.wasm.as_deref().expect("the entry links");
    assert!(built.modules(wasm).node().is_ok());
}

#[cfg(not(feature = "wasmtime"))]
#[test]
#[should_panic(expected = "rerun with --features wasmtime")]
fn wasmtime_modes_are_refused_without_the_feature() {
    EngineMode::from_value(Some("wasmtime"));
}

#[test]
#[should_panic(expected = "unknown HD_CONFORMANCE_ENGINE: typo")]
fn unknown_engine_is_not_a_node_fallback() {
    EngineMode::from_value(Some("typo"));
}

fn tier(case: &Case) -> &'static str {
    if case.specification.starts_with("std/") {
        "std"
    } else {
        assert!(case.specification.starts_with("lang/"));
        "lang"
    }
}

#[derive(Default)]
struct RuntimeStats {
    node_programs: BTreeSet<String>,
    wasmtime_programs: BTreeSet<String>,
    node_harness: BTreeMap<String, usize>,
    wasmtime_harness: BTreeMap<String, usize>,
    test_matches: usize,
    test_comparisons: usize,
    node_elapsed: Duration,
    wasmtime_elapsed: Duration,
    matches: usize,
}

static RUNTIME_STATS: Mutex<BTreeMap<&str, RuntimeStats>> = Mutex::new(BTreeMap::new());
static WASMTIME_DIFFS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Per target, each noted module's digest by case and run.
static WASM_DIGESTS: Mutex<BTreeMap<&str, BTreeMap<String, String>>> = Mutex::new(BTreeMap::new());

/// The name of a target in a digest file's name.
const fn target_name(target: EngineTarget) -> &'static str {
    match target {
        EngineTarget::V8 => "v8",
        EngineTarget::Wasmtime => "wasmtime",
        EngineTarget::Browser => "browser",
    }
}

/// Notes the digest of the module built for `target` that `case` runs as
/// `run`: an entry's runtime profile (`-` for none), `tests`, a doc test
/// `docN`, or `scenario`. For `HD_CONFORMANCE_WASM_DIGEST`.
fn note_wasm_digest(target: EngineTarget, case: &Case, wasm: &[u8], run: &str) {
    if selection_env("HD_CONFORMANCE_WASM_DIGEST").is_none() {
        return;
    }
    let digest = format!("{:032x}", hd_base::hash128(wasm).0);
    let key = format!("{}\t{run}", case.path);
    let mut digests = WASM_DIGESTS.lock().expect("digest lock");
    let digests = digests.entry(target_name(target)).or_default();
    if let Some(old) = digests.insert(key.clone(), digest.clone()) {
        assert_eq!(old, digest, "{key}: one case built two different modules");
    }
}

/// Notes the modules each engine of the mode runs, each under its target.
fn note_modules(case: &Case, wasm: Modules<'_>, run: &str) -> std::io::Result<()> {
    let mode = engine_mode();
    if mode != EngineMode::WasmtimeOnly {
        note_wasm_digest(EngineTarget::V8, case, wasm.node()?, run);
    }
    if mode != EngineMode::Node {
        note_wasm_digest(EngineTarget::Wasmtime, case, wasm.judged, run);
    }
    Ok(())
}

/// Records or checks the digests noted by this run
/// (`HD_CONFORMANCE_WASM_DIGEST=record|check`).
fn finish_wasm_digests() {
    let Some(mode) = selection_env("HD_CONFORMANCE_WASM_DIGEST") else {
        return;
    };
    let pipeline = pipeline_name();
    for (target, digests) in WASM_DIGESTS.lock().expect("digest lock").iter() {
        let file = root().join(format!(
            "target/conformance-wasm-digests-{target}-{pipeline}.tsv"
        ));
        let mut text = String::new();
        for (key, digest) in digests {
            writeln!(text, "{key}\t{digest}").expect("write to a string");
        }
        match mode.as_str() {
            "record" => std::fs::write(&file, text).expect("write the digest file"),
            "check" => {
                let saved = std::fs::read_to_string(&file)
                    .unwrap_or_else(|_| panic!("record {} first", file.display()));
                let old: BTreeSet<&str> = saved.lines().collect();
                let new: BTreeSet<&str> = text.lines().collect();
                let changed: Vec<String> = old
                    .symmetric_difference(&new)
                    .map(|line| (*line).to_owned())
                    .collect();
                assert!(
                    changed.is_empty(),
                    "Wasm digests differ from {}:\n{}",
                    file.display(),
                    changed.join("\n")
                );
            }
            other => panic!("unknown HD_CONFORMANCE_WASM_DIGEST: {other}"),
        }
    }
}

/// Runs only the selected engines. A failed wasmtime setup/run is an
/// error, never an implicit Node fallback.
fn run_program(
    case: &Case,
    wasm: Modules<'_>,
    serial: usize,
    profile: Option<&str>,
) -> std::io::Result<Ran> {
    note_modules(case, wasm, profile.unwrap_or("-"))?;
    let mode = engine_mode();
    let node = if mode == EngineMode::WasmtimeOnly {
        None
    } else {
        let started = Instant::now();
        let ran = run_wasm(wasm.node()?, serial, profile);
        let mut stats = RUNTIME_STATS.lock().expect("runtime stats lock");
        let stats = stats.entry(tier(case)).or_default();
        stats.node_elapsed += started.elapsed();
        stats.node_programs.insert(case.path.clone());
        let ran = ran?;
        Some(ran)
    };
    if mode == EngineMode::Node {
        return Ok(node.expect("Node mode ran Node"));
    }
    wasmtime::run_program(case, wasm.judged, profile, node)
}

/// Execution durations are sums across workers, not elapsed suite wall
/// time. The selected-suite duration includes compilation and all phases.
fn report_runtime(cases: &[Case], results: &[Verdict], elapsed: Duration) {
    let diffs = WASMTIME_DIFFS.lock().expect("diffs lock");
    for d in diffs.iter() {
        println!("wasmtime differs: {d}");
    }
    println!(
        "conformance {:?}, {} pipeline: selected suite wall {:.3}s (including compilation; {} workers), {} output/status differences",
        engine_mode(),
        pipeline_name(),
        elapsed.as_secs_f64(),
        workers(),
        diffs.len()
    );
    if engine_mode() != EngineMode::Node {
        println!("{}", wasmtime::code_cache_line());
    }
    let stats = RUNTIME_STATS.lock().expect("runtime stats lock");
    for name in ["lang", "std"] {
        let selected: Vec<_> = cases
            .iter()
            .zip(results)
            .filter(|(case, _)| case.phase == Phase::Runtime && tier(case) == name)
            .collect();
        if selected.is_empty() {
            continue;
        }
        let mut counts = Counts::default();
        for (_, verdict) in &selected {
            add_count(&mut counts, verdict);
        }
        let empty = RuntimeStats::default();
        let stats = stats.get(name).unwrap_or(&empty);
        println!(
            "{name}: {} runtime cases; pass {} fail {} unsupported {}; {} wasmtime entries, {} Node entries, {} exact entry matches; {} wasmtime test fixtures ({} invocations), {} Node test fixtures ({} invocations); {} / {} test outcome matches (timing excluded); aggregate execution wall Node {:.3}s, wasmtime {:.3}s",
            selected.len(),
            counts.pass,
            counts.fail,
            counts.unsupported,
            stats.wasmtime_programs.len(),
            stats.node_programs.len(),
            stats.matches,
            stats.wasmtime_harness.len(),
            stats.wasmtime_harness.values().sum::<usize>(),
            stats.node_harness.len(),
            stats.node_harness.values().sum::<usize>(),
            stats.test_matches,
            stats.test_comparisons,
            stats.node_elapsed.as_secs_f64(),
            stats.wasmtime_elapsed.as_secs_f64()
        );
    }
}

/// Runs a program's entry on the run host, under the fixture's runtime
/// `profile` when it names one: the run configuration names it, and the
/// host's `profiles.mjs` supplies its providers (README "Runtime
/// Profiles").
///
/// Entry runs go through one long-lived Node worker per conformance
/// worker thread (`host/conformance-worker.mjs`), started once.
/// Each case gets a fresh host; the worker never reuses one. A case that
/// kills its worker restarts it and reports a plain runtime failure, the
/// same verdict a fresh process's non-zero exit gives. The test harness
/// below still spawns a fresh process per program.
fn run_wasm(wasm: &[u8], serial: usize, profile: Option<&str>) -> std::io::Result<Ran> {
    ENTRY_WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        ensure_worker(&mut slot)?;
        if let Ok(ran) = worker_run(
            slot.as_mut().expect("entry worker"),
            wasm,
            serial,
            profile,
            None,
        ) {
            Ok(ran)
        } else {
            *slot = None;
            if ensure_worker(&mut slot).is_err() {
                return Err(std::io::Error::other("node entry worker failed to start"));
            }
            if let Ok(ran) = worker_run(
                slot.as_mut().expect("entry worker"),
                wasm,
                serial,
                profile,
                None,
            ) {
                Ok(ran)
            } else {
                *slot = None;
                // The case killed two workers: report what a fresh
                // process crash reports, a runtime failure.
                Ok(Ran {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    status: Some(1),
                })
            }
        }
    })
}

/// How many cases a worker handles before it is recycled as cheap
/// insurance against V8 heap growth (`f1c55020:future-work/compiler/wasmtime-perf.md`, M38).
const WORKER_USES: usize = 500;

struct EntryWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    uses: usize,
}

impl Drop for EntryWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

thread_local! {
    static ENTRY_WORKER: RefCell<Option<EntryWorker>> = const { RefCell::new(None) };
}

fn worker_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/conformance-worker.mjs")
}

fn ensure_worker(slot: &mut Option<EntryWorker>) -> std::io::Result<()> {
    let stale = slot
        .as_mut()
        .is_some_and(|w| w.uses >= WORKER_USES || w.child.try_wait().ok().flatten().is_some());
    if stale {
        *slot = None;
    }
    if slot.is_none() {
        let mut child = Command::new("node")
            .arg(worker_script())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("node entry worker has no pipes"));
        };
        *slot = Some(EntryWorker {
            child,
            stdin,
            stdout,
            uses: 0,
        });
    }
    Ok(())
}

/// A JSON string literal of `text`.
fn json_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = std::fmt::write(out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn request_header(
    serial: usize,
    wasm_len: usize,
    profile: Option<&str>,
    scenario: Option<&str>,
) -> Vec<u8> {
    let mut header = String::from("{\"id\":");
    header.push_str(&serial.to_string());
    header.push_str(",\"wasmLen\":");
    header.push_str(&wasm_len.to_string());
    header.push_str(",\"profile\":");
    match profile {
        Some(name) => json_string(name, &mut header),
        None => header.push_str("null"),
    }
    header.push_str(",\"scenario\":");
    match scenario {
        Some(name) => json_string(name, &mut header),
        None => header.push_str("null"),
    }
    header.push_str(",\"args\":[],\"stdinLen\":0}");
    header.into_bytes()
}

/// The number after `"key":` in a response header of numbers only.
fn header_number(header: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\":");
    let (_, rest) = header.split_once(&needle)?;
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn read_response(worker: &mut EntryWorker) -> std::io::Result<(usize, Ran)> {
    let mut len = [0; 4];
    worker.stdout.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len);
    let len = usize::try_from(len).map_err(std::io::Error::other)?;
    let mut header = vec![0; len];
    worker.stdout.read_exact(&mut header)?;
    let header = String::from_utf8(header).map_err(|e| std::io::Error::other(e.to_string()))?;
    let id = header_number(&header, "id")
        .ok_or_else(|| std::io::Error::other(format!("node entry worker answered {header:?}")))?;
    let status = header_number(&header, "status")
        .ok_or_else(|| std::io::Error::other(format!("node entry worker answered {header:?}")))?;
    let stdout_len = header_number(&header, "stdoutLen")
        .ok_or_else(|| std::io::Error::other(format!("node entry worker answered {header:?}")))?;
    let stderr_len = header_number(&header, "stderrLen")
        .ok_or_else(|| std::io::Error::other(format!("node entry worker answered {header:?}")))?;
    let stdout_len =
        usize::try_from(stdout_len).map_err(|_| std::io::Error::other("negative stdoutLen"))?;
    let stderr_len =
        usize::try_from(stderr_len).map_err(|_| std::io::Error::other("negative stderrLen"))?;
    let mut stdout = vec![0; stdout_len];
    worker.stdout.read_exact(&mut stdout)?;
    let mut stderr = vec![0; stderr_len];
    worker.stdout.read_exact(&mut stderr)?;
    let status = i32::try_from(status).map_err(|_| std::io::Error::other("status out of range"))?;
    let id = usize::try_from(id).map_err(|_| std::io::Error::other("id out of range"))?;
    worker.uses += 1;
    Ok((
        id,
        Ran {
            stdout,
            stderr,
            status: Some(status),
        },
    ))
}

fn worker_run(
    worker: &mut EntryWorker,
    wasm: &[u8],
    serial: usize,
    profile: Option<&str>,
    scenario: Option<&str>,
) -> std::io::Result<Ran> {
    let header = request_header(serial, wasm.len(), profile, scenario);
    let expected = serial;
    worker
        .stdin
        .write_all(&(u32::try_from(header.len()).map_err(std::io::Error::other)?).to_be_bytes())?;
    worker.stdin.write_all(&header)?;
    worker.stdin.write_all(wasm)?;
    worker.stdin.flush()?;
    let (id, ran) = read_response(worker)?;
    // The worker answers in order; a mismatched id means the stream lost
    // sync, so the worker goes.
    if id != expected {
        return Err(std::io::Error::other(
            "node entry worker answered out of order",
        ));
    }
    Ok(ran)
}

/// A fixture whose only harness is its runtime profile: its entry runs
/// under the profile (README "Runtime Profiles"), with no test case, no
/// doc test and no scenario.
fn profile_entry(text: &str) -> bool {
    directive(text, "fixture-runtime-profile").is_some()
        && !text
            .lines()
            .any(|line| line.starts_with("tests:") || line.starts_with("## "))
        && [
            "fixture-runtime-scenario",
            "fixture-runtime-pending-function",
            "fixture-test-layout",
            "fixture-package-tree",
        ]
        .iter()
        .all(|name| directive(text, name).is_none())
}

/// A fixture whose only harness is a single-file `tests:` block, or a
/// single test module or integration test file whose top-level
/// registrations are its test cases.
fn plain_tests(text: &str) -> bool {
    (text.lines().any(|line| line.starts_with("tests:"))
        || directive(text, "fixture-test-layout").is_some())
        && !text.lines().any(|line| line.starts_with("## "))
        && [
            "fixture-runtime-profile",
            "fixture-runtime-scenario",
            "fixture-runtime-pending-function",
            "fixture-package-tree",
        ]
        .iter()
        .all(|name| directive(text, name).is_none())
}

/// Whether a test host result line reports panic category `code`: its
/// `category` field, which the host recorded, never the report text. In
/// the JSON line every quote inside a string is escaped, so the key's
/// unescaped `"category":"` cannot come from program text.
fn reports_panic(line: &str, code: &str) -> bool {
    line.contains(&format!("\"category\":\"{code}\""))
}

/// A package-tree fixture whose only harness is its tree and its primary
/// file: the `tests:` block and the doc comments of the file, or its
/// top-level registrations when it is a test module or an integration test
/// file (README "Package Trees": its test cases are the primary module's,
/// the primary file's doc tests included).
fn tree_tests_harness(fixture: &Fixture) -> bool {
    fixture.tree
        && (fixture
            .text
            .lines()
            .any(|line| line.starts_with("## ") || line.starts_with("tests:"))
            || hd_project::role_of(&fixture.primary) == hd_project::Role::Test)
        && [
            "fixture-runtime-profile",
            "fixture-runtime-scenario",
            "fixture-runtime-pending-function",
        ]
        .iter()
        .all(|name| directive(&fixture.text, name).is_none())
}

/// What the test programs of one case did: the result lines of cases that
/// panicked against their expectation, and whether any other case failed.
#[derive(Default)]
struct TestRun {
    panicked: Vec<String>,
    failed: bool,
}

/// The verdict of a build that reports an error: unsupported when the
/// first error is `unsupported`, else a failure with its code.
fn build_error(built: &Output) -> Option<Verdict> {
    if !built.diags.has_errors() {
        return None;
    }
    let index = first_error(built);
    if built.diags.code[index].as_str() == "unsupported" {
        return Some(Verdict::Unsupported(
            first_unsupported(&built.report, true).unwrap_or_else(|| "Build".to_owned()),
        ));
    }
    Some(Verdict::Fail(built.diags.code[index].as_str().to_owned()))
}

/// Runs each case of a `Goal::Tests` build in its own instance and adds
/// what they did to `run`; `Err` is the case's verdict when the build
/// cannot run. `program` names the build among the case's test programs
/// (`tests`, or a doc test's `docN`).
fn run_tests(
    case: &Case,
    runnable: &Runnable,
    program: &str,
    serial: usize,
    run: &mut TestRun,
) -> Result<(), Verdict> {
    let built = &runnable.out;
    if let Some(verdict) = build_error(built) {
        return Err(verdict);
    }
    if built.tests.is_empty() {
        return Err(Verdict::Unsupported("TestPlan".to_owned()));
    }
    if built.tests.iter().any(|test| test.unsupported.is_some()) {
        return Err(Verdict::Unsupported("TestCase".to_owned()));
    }
    let runs: Vec<_> = built
        .tests
        .iter()
        .filter(|test| test.ignore.is_none())
        .filter_map(|test| test.run.map(|run| (test, run)))
        .collect();
    // Every case is ignored: nothing runs, so there is no program.
    if runs.is_empty() {
        return Ok(());
    }
    let Some(wasm) = &built.wasm else {
        return Err(Verdict::Unsupported("Link".to_owned()));
    };
    let modules = runnable.modules(wasm);
    note_modules(case, modules, program).map_err(|e| Verdict::Crash(e.to_string()))?;
    let mode = engine_mode();
    let node = if mode == EngineMode::WasmtimeOnly {
        None
    } else {
        let path = Scratch::unmade(
            env!("CARGO_TARGET_TMPDIR"),
            &format!("conformance-{serial}.wasm"),
        );
        let node_wasm = modules.node().map_err(|e| Verdict::Crash(e.to_string()))?;
        std::fs::write(&path, node_wasm).map_err(|e| Verdict::Crash(e.to_string()))?;
        let list: Vec<String> = runs
            .iter()
            .map(|(case, (test, init))| format!("{test}:{init}:{}", case.kind))
            .collect();
        let started = Instant::now();
        let result = Command::new("node")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/test.mjs"))
            .arg(&path)
            .arg(list.join(","))
            .output();
        {
            let mut stats = RUNTIME_STATS.lock().expect("runtime stats lock");
            let stats = stats.entry(tier(case)).or_default();
            stats.node_elapsed += started.elapsed();
            *stats.node_harness.entry(case.path.clone()).or_default() += 1;
        }
        drop(path);
        let output = result.map_err(|e| Verdict::Crash(e.to_string()))?;
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let stdout = if mode == EngineMode::Node {
        node.expect("Node mode ran Node")
    } else {
        wasmtime::test_stdout(case, wasm, &runs, node.as_deref())?
    };
    // One line per case, or per row of an `it_each` case, whose `test`
    // field names its case's export; a table with no rows has one line
    // with `"rows":0` and no outcome.
    let index = |line: &str| -> Option<u32> {
        let rest = line.strip_prefix("{\"test\":")?;
        rest[..rest.find(',')?].parse().ok()
    };
    let mut lines: Vec<(&hd_driver::TestCase, &str)> = Vec::new();
    for line in stdout.lines() {
        let Some(case) = index(line).and_then(|i| runs.iter().find(|(_, (t, _))| *t == i)) else {
            return Err(Verdict::Fail("runtime-exit".to_owned()));
        };
        if !line.contains("\"rows\":0}") {
            lines.push((case.0, line));
        }
    }
    let reported = |t: u32| stdout.lines().any(|line| index(line) == Some(t));
    if runs.iter().any(|(_, (t, _))| !reported(*t)) {
        return Err(Verdict::Fail("runtime-exit".to_owned()));
    }
    for (test, line) in lines {
        if line.contains("\"trapped\":true") {
            match &test.expect_panic {
                Some(code) if reports_panic(line, code) => {}
                _ => run.panicked.push(line.to_owned()),
            }
        } else if test.expect_panic.is_some() || !line.contains("\"status\":0,") {
            run.failed = true;
        }
    }
    Ok(())
}

/// The case's verdict from what its test programs did: `panic:CODE` needs
/// a case that panicked with `CODE`; any other expectation needs every
/// case to pass.
fn judge_tests(case: &Case, run: &TestRun) -> Verdict {
    if let Some(code) = case.expectation.strip_prefix("panic:") {
        if run.panicked.iter().any(|line| reports_panic(line, code)) {
            Verdict::Pass
        } else {
            Verdict::Fail("runtime-exit".to_owned())
        }
    } else if run.failed || !run.panicked.is_empty() {
        Verdict::Fail("runtime-exit".to_owned())
    } else {
        Verdict::Pass
    }
}

/// `test FILE`: each case of the fixture's `tests:` block in its own
/// instance (spec/conformance/README.md, Runtime Execution steps 2 and 3).
fn tests_case(case: &Case, fixture: &Fixture, store: &MemoryStore, serial: usize) -> Verdict {
    let built = build_runnable(
        fixture,
        &fixture.sources,
        store,
        &Goal::Tests {
            module: None,
            filter: None,
        },
    );
    let mut run = TestRun::default();
    match run_tests(case, &built, "tests", serial, &mut run) {
        Ok(()) => judge_tests(case, &run),
        Err(verdict) => verdict,
    }
}

/// `test FILE` for a package-tree fixture: the primary module's test cases
/// (its `tests:` block, or the registrations of a test module or an
/// integration test file), then each doc test of the primary file as a
/// program of its own, an integration-view module at `tests/$docN.hd`
/// (README "Package Trees"; `module.test.doc.program`). A compile-fail doc
/// test never runs: it passes when its build reports its code
/// (`module.test.doc.compile-fail`).
fn tree_tests_case(case: &Case, fixture: &Fixture, store: &MemoryStore, serial: usize) -> Verdict {
    let mut run = TestRun::default();
    if fixture.text.lines().any(|line| line.starts_with("tests:"))
        || hd_project::role_of(&fixture.primary) == hd_project::Role::Test
    {
        let built = build_runnable(
            fixture,
            &fixture.sources,
            store,
            &Goal::Tests {
                module: Some(hd_project::module_path("fixture", &fixture.primary)),
                filter: None,
            },
        );
        if let Err(verdict) = run_tests(case, &built, "tests", serial, &mut run) {
            return verdict;
        }
    }
    for (index, doc) in hd_project::doc_tests(&fixture.primary, &fixture.text)
        .into_iter()
        .enumerate()
    {
        let path = format!("tests/$doc{index}.hd");
        let mut sources = fixture.sources.clone();
        sources.insert(&path, &doc.program);
        let goal = Goal::Tests {
            module: Some(hd_project::module_path("fixture", &path)),
            filter: None,
        };
        if let Some(code) = &doc.compile_fail {
            let built = build_sources(fixture, &sources, store, &goal);
            if !built.diags.code.iter().any(|found| found.as_str() == code) {
                return build_error(&built)
                    .unwrap_or_else(|| Verdict::Fail("no-diagnostic".to_owned()));
            }
            continue;
        }
        let built = build_runnable(fixture, &sources, store, &goal);
        if let Err(verdict) = run_tests(case, &built, &format!("doc{index}"), serial, &mut run) {
            return verdict;
        }
    }
    judge_tests(case, &run)
}

fn runtime_case(case: &Case, fixture: &Fixture, store: &MemoryStore, serial: usize) -> Verdict {
    let checked = check_fixture(fixture, store);
    if let Some(index) = check_errors(&checked).first() {
        return Verdict::Fail(checked.diags.code[*index].as_str().to_owned());
    }
    if let Some(stage) = first_unsupported(&checked.report, false) {
        return Verdict::Unsupported(stage);
    }
    // `pending-first-poll` without test cases runs the ordinary entry
    // below, twice; every other replacing scenario drives its own steps.
    let ordinary_pending = pending_first_poll(&fixture.text)
        && !fixture
            .text
            .lines()
            .any(|line| line.starts_with("tests:") || line.starts_with("## "));
    if has_runtime_harness(&fixture.text) && !profile_entry(&fixture.text) && !ordinary_pending {
        let verdict = if tree_tests_harness(fixture) {
            tree_tests_case(case, fixture, store, serial)
        } else if plain_tests(&fixture.text) {
            tests_case(case, fixture, store, serial)
        } else {
            return scenario_case(case, fixture, store, serial);
        };
        if verdict != Verdict::Pass || expected_stdout(&fixture.text).is_none() {
            return verdict;
        }
    }
    let runnable = build_runnable(
        fixture,
        &fixture.sources,
        store,
        &Goal::Program {
            entry: "main".to_owned(),
        },
    );
    let built = &runnable.out;
    if built.diags.has_errors() {
        let index = first_error(built);
        if built.diags.code[index].as_str() == "unsupported" {
            return Verdict::Unsupported(
                first_unsupported(&built.report, true).unwrap_or_else(|| "Build".to_owned()),
            );
        }
        // Runtime Execution step 1 runs an entry point only when the module
        // declares one (module.entry.private-main): no entry, nothing to run.
        if built.diags.code[index].as_str() == "missing-entry-point"
            && !case.expectation.starts_with("panic:")
            && expected_stdout(&fixture.text).is_none()
        {
            return Verdict::Pass;
        }
        return Verdict::Fail(built.diags.code[index].as_str().to_owned());
    }
    let Some(wasm) = &built.wasm else {
        return Verdict::Unsupported("Link".to_owned());
    };
    let modules = runnable.modules(wasm);
    let profile = directive(&fixture.text, "fixture-runtime-profile");
    let verdict = entry_verdict(case, fixture, modules, serial, profile);
    if verdict == Verdict::Pass && pending_first_poll(&fixture.text) {
        // README "Runtime Scenarios", `pending-first-poll`: after the
        // ordinary run, the entry again under the `console` profile with
        // every `write_line!` pending on its first poll, which is the
        // `pending-write` profile; both runs must succeed.
        return entry_verdict(case, fixture, modules, serial, Some("pending-write"));
    }
    verdict
}

/// Runs the program's entry under `profile` and judges the run by the
/// case's expectation and the fixture's expected standard output.
fn entry_verdict(
    case: &Case,
    fixture: &Fixture,
    wasm: Modules<'_>,
    serial: usize,
    profile: Option<&str>,
) -> Verdict {
    let run = match run_program(case, wasm, serial, profile) {
        Ok(run) => run,
        Err(error) => return Verdict::Crash(error.to_string()),
    };
    judge_run(case, fixture, &run)
}

/// A runtime scenario that replaces the execution steps: builds the
/// entry with `Goal::Scenario` and drives the scenario's own steps on
/// both engines, then judges the drive like an entry run.
fn scenario_case(case: &Case, fixture: &Fixture, store: &MemoryStore, serial: usize) -> Verdict {
    let scenario = directive(&fixture.text, "fixture-runtime-scenario");
    // `pending-first-poll` with test cases runs under the test hosts,
    // which take no runtime profile.
    if scenario == Some("pending-first-poll") {
        return Verdict::Unsupported("RunCase".to_owned());
    }
    let hold = match scenario {
        // An owned-but-unfinished root is the whole point: holding the
        // root itself keeps it pending with no call running beneath it.
        // `reentrant-poll` leaves that frame active, so the next
        // same-driver poll reports re-entry.
        Some("competing-drivers") => Some(ScenarioHold {
            func: "main".to_owned(),
            active: false,
        }),
        Some("reentrant-poll") => Some(ScenarioHold {
            func: "main".to_owned(),
            active: true,
        }),
        _ => {
            directive(&fixture.text, "fixture-runtime-pending-function").map(|name| ScenarioHold {
                func: name.to_owned(),
                active: false,
            })
        }
    };
    let runnable = build_runnable(
        fixture,
        &fixture.sources,
        store,
        &Goal::Scenario {
            entry: "main".to_owned(),
            hold,
        },
    );
    let built = &runnable.out;
    if built.diags.has_errors() {
        let index = first_error(built);
        if built.diags.code[index].as_str() == "unsupported" {
            return Verdict::Unsupported(
                first_unsupported(&built.report, true).unwrap_or_else(|| "Build".to_owned()),
            );
        }
        return Verdict::Fail(built.diags.code[index].as_str().to_owned());
    }
    let Some(wasm) = &built.wasm else {
        return Verdict::Unsupported("Link".to_owned());
    };
    let profile = directive(&fixture.text, "fixture-runtime-profile");
    let scenario = scenario.unwrap_or("cancellation-cleanup");
    let run = match scenario_run(case, runnable.modules(wasm), serial, profile, scenario) {
        Ok(run) => run,
        Err(error) => return Verdict::Crash(error.to_string()),
    };
    judge_run(case, fixture, &run)
}

/// Drives a scenario's own steps on both engines and compares them, as
/// `run_program` runs and compares an entry.
fn scenario_run(
    case: &Case,
    wasm: Modules<'_>,
    serial: usize,
    profile: Option<&str>,
    scenario: &str,
) -> std::io::Result<Ran> {
    note_modules(case, wasm, "scenario")?;
    let mode = engine_mode();
    let node = if mode == EngineMode::WasmtimeOnly {
        None
    } else {
        let started = Instant::now();
        let node_wasm = wasm.node()?;
        let ran = ENTRY_WORKER.with(|slot| {
            let mut slot = slot.borrow_mut();
            ensure_worker(&mut slot)?;
            worker_run(
                slot.as_mut().expect("entry worker"),
                node_wasm,
                serial,
                profile,
                Some(scenario),
            )
        });
        let mut stats = RUNTIME_STATS.lock().expect("runtime stats lock");
        let stats = stats.entry(tier(case)).or_default();
        stats.node_elapsed += started.elapsed();
        stats.node_programs.insert(case.path.clone());
        let ran = ran?;
        Some(ran)
    };
    if mode == EngineMode::Node {
        return Ok(node.expect("Node mode ran Node"));
    }
    wasmtime::drive_scenario(case, wasm.judged, profile, scenario, node)
}

fn judge_run(case: &Case, fixture: &Fixture, run: &Ran) -> Verdict {
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    if let Some(expected) = case.expectation.strip_prefix("panic:") {
        if !run.success() && stderr.contains(&format!("panic: {expected}:")) {
            Verdict::Pass
        } else {
            Verdict::Fail("runtime-exit".to_owned())
        }
    } else if !run.success() {
        Verdict::Fail("runtime-exit".to_owned())
    } else if let Some(expected) = expected_stdout(&fixture.text) {
        match expected {
            Ok(expected) if stdout == expected => Verdict::Pass,
            Ok(_) => Verdict::Fail("stdout".to_owned()),
            Err(_) => Verdict::Fail("invalid-expect-stdout".to_owned()),
        }
    } else {
        Verdict::Pass
    }
}

fn run_case(case: &Case, store: &MemoryStore, serial: usize) -> Verdict {
    let fixture = match fixture(case) {
        Ok(fixture) => fixture,
        Err(stage) => return Verdict::Unsupported(stage),
    };
    match case.phase {
        Phase::Parse => parse_case(case, &fixture.text),
        Phase::Type => type_case(case, &fixture, store),
        Phase::Runtime => {
            let verdict = runtime_case(case, &fixture, store, serial);
            if linear_sweep() || linear_passes().contains_key(&case.path) {
                linear_case(case, fixture, store, serial);
            }
            verdict
        }
    }
}

/// The root list of runtime cases that pass on the linear backend.
const LINEAR_PASSES: &str = "LINEAR_PASSES.tsv";

/// [`LINEAR_PASSES`]' rows, path to note.
fn linear_passes() -> &'static BTreeMap<String, String> {
    static ROWS: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    ROWS.get_or_init(|| {
        let text = std::fs::read_to_string(root().join(LINEAR_PASSES))
            .unwrap_or_else(|e| panic!("{LINEAR_PASSES}: {e}"));
        parse_linear_passes(&text).unwrap_or_else(|error| panic!("{error}"))
    })
}

/// Parses [`LINEAR_PASSES`]: the header `path<TAB>note`, then one row per
/// case, whose note cites the issue `#N` of the job that made it pass.
fn parse_linear_passes(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut lines = text.lines();
    if lines.next() != Some("path\tnote") {
        return Err(format!(
            "{LINEAR_PASSES} must start with the header `path<TAB>note`"
        ));
    }
    let mut rows = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let [path, note] = line.split('\t').collect::<Vec<_>>()[..] else {
            return Err(format!("{LINEAR_PASSES} row needs two columns: {line}"));
        };
        let cites = note.split('#').skip(1).any(|after| {
            after
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_digit())
        });
        if path.is_empty() || !cites {
            return Err(format!(
                "{LINEAR_PASSES} row needs a path and a note citing its issue #N: {line}"
            ));
        }
        if rows.insert(path.to_owned(), note.to_owned()).is_some() {
            return Err(format!("{LINEAR_PASSES} repeats a row: {line}"));
        }
    }
    Ok(rows)
}

/// Whether `HD_CONFORMANCE_BACKEND=linear` runs every runtime case through
/// the linear backend too.
fn linear_sweep() -> bool {
    match selection_env("HD_CONFORMANCE_BACKEND").as_deref() {
        None | Some("") => false,
        Some("linear") => true,
        Some(other) => {
            panic!("unknown HD_CONFORMANCE_BACKEND: {other}; the backend to sweep is linear")
        }
    }
}

#[test]
fn linear_passes_rows_cite_an_issue() {
    let rows = parse_linear_passes("path\tnote\nruntime/valid/a.hd\t#539: no entry point\n")
        .expect("a valid list");
    assert_eq!(rows["runtime/valid/a.hd"], "#539: no entry point");
    for bad in [
        "path\treason\n",
        "path\tnote\nruntime/valid/a.hd\tno issue\n",
        "path\tnote\nruntime/valid/a.hd\n",
        "path\tnote\nruntime/valid/a.hd\t#539\nruntime/valid/a.hd\t#540\n",
    ] {
        assert!(parse_linear_passes(bad).is_err(), "{bad:?} parses");
    }
}

/// The linear-run verdict of each runtime case that ran one, by path.
static LINEAR_VERDICTS: Mutex<BTreeMap<String, Verdict>> = Mutex::new(BTreeMap::new());

/// Runs a runtime case once more, built by the linear backend, and notes
/// its verdict in [`LINEAR_VERDICTS`]; a crash there is the linear run's
/// own. Its runs are noted under `linear:PATH`.
fn linear_case(case: &Case, fixture: Fixture, store: &MemoryStore, serial: usize) {
    let noted = Case {
        path: format!("linear:{}", case.path),
        ..case.clone()
    };
    let fixture = Fixture {
        backend: Backend::Linear,
        ..fixture
    };
    let verdict = catch_unwind(AssertUnwindSafe(|| {
        runtime_case(&noted, &fixture, store, serial)
    }))
    .unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<&str>()
            .map_or("panic", |message| *message);
        Verdict::Crash(message.to_owned())
    });
    LINEAR_VERDICTS
        .lock()
        .expect("linear verdicts lock")
        .insert(case.path.clone(), verdict);
}

/// The std package's own hd tests of its runtime modules: only std may use
/// a runtime module (`module.rt.std-only`), so no fixture can. Each file
/// under `tests/std_rt/` joins std's sources at the path given here.
const STD_RT_TESTS: &[(&str, &str)] = &[
    ("rt/memory_tests.hd", include_str!("std_rt/memory_tests.hd")),
    ("rt/alloc_tests.hd", include_str!("std_rt/alloc_tests.hd")),
    ("rt/count_tests.hd", include_str!("std_rt/count_tests.hd")),
];

/// Every case of each [`STD_RT_TESTS`] file passes on the linear backend,
/// on the engines of the mode, as a `tests` case of the suite would.
#[test]
fn std_runtime_modules_pass_their_hd_tests_on_the_linear_backend() {
    let store = MemoryStore::default();
    for (serial, (path, text)) in STD_RT_TESTS.iter().enumerate() {
        let mut sources = hd_driver::std_sources();
        sources.insert(path, text);
        let fixture = Fixture {
            sources,
            primary: (*path).to_owned(),
            text: (*text).to_owned(),
            tree: false,
            deps: Vec::new(),
            backend: Backend::Linear,
            package: "std",
        };
        let case = Case {
            path: format!("std/{path}"),
            phase: Phase::Runtime,
            expectation: "accept".to_owned(),
            specification: "lang/10-modules.md#memory-intrinsics".to_owned(),
        };
        let goal = Goal::Tests {
            module: Some(hd_project::module_path("std", path)),
            filter: None,
        };
        let built = build_runnable(&fixture, &fixture.sources, &store, &goal);
        let mut run = TestRun::default();
        let verdict = run_tests(&case, &built, "tests", (1 << 41) + serial, &mut run)
            .map_or_else(|verdict| verdict, |()| judge_tests(&case, &run));
        assert_eq!(
            verdict,
            Verdict::Pass,
            "{path}: {:#?}\n{}",
            run.panicked,
            built.out.render()
        );
        println!(
            "std runtime hd tests (linear backend) on {}: {path}, {} cases pass",
            engine_mode().name(),
            built.out.tests.len()
        );
    }
}

/// A single-file fixture with source `text`, for the runner's own tests.
fn unit_fixture(text: &str) -> Fixture {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", text);
    Fixture {
        sources,
        primary: "main.hd".to_owned(),
        text: text.to_owned(),
        tree: false,
        deps: Vec::new(),
        backend: Backend::WasmGc,
        package: "fixture",
    }
}

/// A runtime case for the runner's own tests.
fn unit_case(expectation: &str, serial: usize) -> Case {
    Case {
        path: format!("runner-unit/{serial}.hd"),
        phase: Phase::Runtime,
        expectation: expectation.to_owned(),
        specification: "lang/11-requirements-and-suspension.md".to_owned(),
    }
}

/// The verdict of the runtime case of `text`. Its serial stays clear of
/// the suite's case indices, so its program files are its own.
fn unit_runtime(text: &str, expectation: &str, serial: usize) -> Verdict {
    let case = unit_case(expectation, serial);
    let store = MemoryStore::default();
    runtime_case(&case, &unit_fixture(text), &store, (1 << 40) + serial)
}

const PENDING_WRITES: &str = "\
# fixture-runtime-scenario: pending-first-poll
use std.testing.assert_equal

let calls = +0

fn note!(text: string) -> void $ Console:
    let mut console = $.use(Console)
    _ := console.write_line!(\"${text} ${calls}\")
    calls = calls + 1

pub fn main!() -> void $ Console:
    note!(\"first\")
    note!(\"second\")
    assert_equal(calls, EXPECTED, reason=\"each call resumed after its pending write\")
";

#[test]
fn pending_first_poll_runs_the_entry_again_with_pending_writes() {
    let passing = PENDING_WRITES.replace("EXPECTED", "2");
    assert_eq!(unit_runtime(&passing, "accept", 0), Verdict::Pass);
    // The scenario run is the entry under `pending-write`: each write is
    // pending once, then written in call order, and the run succeeds.
    let fixture = unit_fixture(&passing);
    let built = build_runnable(
        &fixture,
        &fixture.sources,
        &MemoryStore::default(),
        &Goal::Program {
            entry: "main".to_owned(),
        },
    );
    let wasm = built.out.wasm.as_deref().expect("the entry links");
    let serial = (1 << 40) + 1;
    let ran = run_program(
        &unit_case("accept", 1),
        built.modules(wasm),
        serial,
        Some("pending-write"),
    )
    .expect("runs");
    assert!(ran.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "first 0\nsecond 1\n");
    // A program whose result differs fails the case.
    let failing = PENDING_WRITES.replace("EXPECTED", "3");
    assert_eq!(
        unit_runtime(&failing, "accept", 2),
        Verdict::Fail("runtime-exit".to_owned())
    );
}

#[test]
fn scenarios_that_replace_execution_drive_or_stay_unsupported() {
    assert!(!replacing_scenario(PENDING_WRITES));
    assert!(!replacing_scenario("pub fn main!() -> void: pass\n"));
    // The cancellation program never pends, so its drive fails the
    // first step; the competing and reentrant drives trap, which the
    // `accept` expectation fails.
    // Each program has its own serial, so its digest row is its own.
    for (serial, name) in [
        (3, "cancellation-cleanup"),
        (6, "competing-drivers"),
        (7, "reentrant-poll"),
    ] {
        let text = format!(
            "# fixture-runtime-scenario: {name}\nfn ready!() -> i32: 42\n\npub fn main!() -> void:\n    _ := ready!()\n\npub fn cleanup_ran() -> bool: true\n"
        );
        assert!(replacing_scenario(&text));
        assert_eq!(
            unit_runtime(&text, "accept", serial),
            Verdict::Fail("runtime-exit".to_owned())
        );
    }
    // The test hosts take no runtime profile, so the scenario's run of
    // test cases cannot hold a write pending.
    let with_tests = format!(
        "{}\ntests:\n    it(\"runs\"):\n        pass\n",
        PENDING_WRITES.replace("EXPECTED", "2")
    );
    assert_eq!(
        unit_runtime(&with_tests, "accept", 4),
        Verdict::Unsupported("RunCase".to_owned())
    );
}

/// A case of the suite's own fixture `path`, for the runner's own tests.
fn suite_case(path: &str, serial: usize) -> Case {
    Case {
        path: path.to_owned(),
        ..unit_case("accept", serial)
    }
}

#[test]
fn a_package_tree_fixture_takes_its_place_in_the_tree() {
    // README "Package Trees": the primary file takes the package path of
    // the header, next to the tree's files, which are not cases.
    let case = suite_case("runtime/valid/integration-shared-use.hd", 5);
    let fixture = fixture(&case).expect("a package-tree fixture");
    assert!(fixture.tree);
    assert!(fixture.deps.is_empty());
    assert_eq!(fixture.primary, "tests/checkout.hd");
    let files: Vec<String> = hd_project::SourceSet::list(&fixture.sources)
        .into_iter()
        .map(|entry| entry.path)
        .collect();
    assert_eq!(
        files,
        ["tests/checkout.hd", "tests/common/mod.hd", "tests/smoke.hd"]
    );
    let primary = hd_project::SourceSet::read(&fixture.sources, "tests/checkout.hd")
        .expect("the primary file");
    assert_eq!(String::from_utf8_lossy(&primary), fixture.text);
    // Its harness is the tree: the primary file is an integration test.
    assert!(tree_tests_harness(&fixture));
    let store = MemoryStore::default();
    assert_eq!(
        runtime_case(&case, &fixture, &store, (1 << 40) + 5),
        Verdict::Pass
    );
}

#[test]
fn a_package_tree_fixture_that_is_not_test_code_is_no_test_run() {
    // A library file in a tree with no `tests:` block or doc comment has
    // no test cases to run.
    let mut fixture = unit_fixture("pub fn answer() -> i32: 42\n");
    fixture.tree = true;
    fixture.primary = "src/answer.hd".to_owned();
    assert!(!tree_tests_harness(&fixture));
    fixture.primary = "src/answer_test.hd".to_owned();
    assert!(tree_tests_harness(&fixture));
    fixture.primary = "tests/answer.hd".to_owned();
    assert!(tree_tests_harness(&fixture));
}

fn chapter(case: &Case) -> String {
    case.specification
        .split('#')
        .next()
        .unwrap_or(&case.specification)
        .to_owned()
}

fn add_count(counts: &mut Counts, verdict: &Verdict) {
    match verdict {
        Verdict::Pass => counts.pass += 1,
        Verdict::Fail(_) | Verdict::Crash(_) => counts.fail += 1,
        Verdict::Unsupported(_) => counts.unsupported += 1,
    }
}

/// `HD_CONFORMANCE_ONLY=a,b`: run only the cases whose path or rule
/// contains one of the comma-separated parts, print each verdict with the
/// first diagnostic or unsupported reason, and skip the known-failure rule.
fn filtered(cases: Vec<Case>) -> Option<Vec<Case>> {
    let only = std::env::var("HD_CONFORMANCE_ONLY").ok()?;
    let parts: Vec<&str> = only.split(',').filter(|part| !part.is_empty()).collect();
    Some(
        cases
            .into_iter()
            .filter(|case| {
                parts
                    .iter()
                    .any(|part| case.path.contains(part) || case.specification.contains(part))
            })
            .collect(),
    )
}

fn detail(case: &Case, store: &MemoryStore) -> String {
    let Ok(fixture) = fixture(case) else {
        return String::new();
    };
    if case.phase == Phase::Parse {
        return String::new();
    }
    let mut output = check_fixture(&fixture, store);
    if case.phase == Phase::Runtime && check_errors(&output).is_empty() {
        let goal = if plain_tests(&fixture.text) || tree_tests_harness(&fixture) {
            Goal::Tests {
                module: None,
                filter: None,
            }
        } else {
            Goal::Program {
                entry: "main".to_owned(),
            }
        };
        output = build_fixture(&fixture, store, &goal);
        if let Some(test) = output.tests.iter().find(|test| test.unsupported.is_some()) {
            return format!(
                "test case: {}",
                test.unsupported.clone().unwrap_or_default()
            );
        }
    }
    let mut out = String::new();
    if let Some(line) = output.render().lines().next() {
        out.push_str(line);
    }
    let mut reasons: Vec<_> = output.report.body_failures.iter().take(1).collect();
    if reasons.is_empty() {
        reasons = output
            .report
            .reasons
            .keys()
            .filter(|reason| !reason.starts_with("Collect"))
            .take(1)
            .collect();
    }
    for reason in reasons {
        out.push_str(" | ");
        out.push_str(reason);
    }
    out
}

fn print_filtered(cases: &[Case], results: &[Verdict], store: &MemoryStore) {
    let mut chapters: BTreeMap<String, Counts> = BTreeMap::new();
    let mut buckets: BTreeMap<String, usize> = BTreeMap::new();
    for (case, verdict) in cases.iter().zip(results) {
        add_count(chapters.entry(chapter(case)).or_default(), verdict);
        let bucket = match verdict {
            Verdict::Pass => continue,
            Verdict::Fail(code) => format!("fail:{code}"),
            Verdict::Unsupported(stage) => format!("unsupported:{stage}"),
            Verdict::Crash(message) => format!("crash:{message}"),
        };
        *buckets.entry(bucket.clone()).or_default() += 1;
        println!(
            "{bucket}\t{}\t{}\t{}",
            case.path,
            case.expectation,
            detail(case, store)
        );
    }
    // The linear backend's runs, one line per case that does not pass.
    let linear = LINEAR_VERDICTS.lock().expect("linear verdicts lock");
    for case in cases {
        match linear.get(&case.path) {
            None | Some(Verdict::Pass) => {}
            Some(verdict) => println!("linear\t{verdict:?}\t{}", case.path),
        }
    }
    drop(linear);
    let mut sorted: Vec<_> = buckets.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (bucket, count) in sorted {
        println!("{count:>5} {bucket}");
    }
    for (name, counts) in &chapters {
        println!(
            "{name}: pass {} fail {} unsupported {}",
            counts.pass, counts.fail, counts.unsupported
        );
    }
}

/// Worker count, like the TS runner (`spec/tools/run-conformance.ts`):
/// `HD_TEST_JOBS` if set to a positive integer, else
/// `min(8, available_parallelism())`. The shared `MemoryStore` stays
/// correct under concurrent use: both of its maps sit behind a `Mutex`
/// (`CacheStore` is `Sync`), so workers share one store.
fn workers() -> usize {
    if let Ok(value) = std::env::var("HD_TEST_JOBS")
        && let Ok(n) = value.trim().parse::<usize>()
        && n >= 1
    {
        return n;
    }
    std::thread::available_parallelism().map_or(8, |n| n.get().min(8))
}

/// Runs every case on a pool of worker threads. Results stay in case
/// order, so the verdicts match the sequential run. `serial` is
/// the case index, so each runtime case still gets unique
/// `conformance-{serial}.wasm` and `.json` names under
/// `CARGO_TARGET_TMPDIR` and workers never share a file.
fn run_all(cases: &[Case], store: &MemoryStore) -> Vec<Verdict> {
    let count = workers().min(cases.len().max(1));
    let timings = std::env::var_os("HD_TEST_TIMINGS").is_some();
    let next = AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<Verdict>>> =
        cases.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..count {
            let builder = std::thread::Builder::new();
            let builder = if engine_mode() == EngineMode::Node {
                builder
            } else {
                builder.stack_size(wasmtime::worker_stack())
            };
            builder
                .spawn_scoped(scope, || {
                    loop {
                        let serial = next.fetch_add(1, Ordering::Relaxed);
                        let Some(case) = cases.get(serial) else { break };
                        let started = timings.then(Instant::now);
                        let verdict =
                            catch_unwind(AssertUnwindSafe(|| run_case(case, store, serial)))
                                .unwrap_or_else(|panic| {
                                    let message = panic
                                        .downcast_ref::<&str>()
                                        .map_or("panic", |message| *message);
                                    Verdict::Crash(message.to_owned())
                                });
                        if let Some(started) = started {
                            eprintln!(
                                "conformance-case\t{}\t{:.6}",
                                case.path,
                                started.elapsed().as_secs_f64()
                            );
                        }
                        *slots[serial].lock().expect("results lock") = Some(verdict);
                    }
                })
                .expect("spawn conformance worker");
        }
    });
    slots
        .into_iter()
        .map(|slot| slot.into_inner().expect("results lock").expect("verdict"))
        .collect()
}

#[test]
fn specification_conformance_passes_except_known_failures() {
    let mode = engine_mode();
    let all = cases();
    let only = filtered(all.clone());
    let mut cases = only.clone().unwrap_or(all);
    let selected_tier = selection_env("HD_CONFORMANCE_TIER");
    if let Some(name) = &selected_tier {
        assert!(
            matches!(name.as_str(), "lang" | "std"),
            "HD_CONFORMANCE_TIER must be lang or std"
        );
        cases.retain(|case| case.phase == Phase::Runtime && tier(case) == name);
    }
    let store = MemoryStore::default();
    let started = Instant::now();
    let results = {
        // Installed for the run; its drop publishes the cache's sizes.
        let _code = (mode != EngineMode::Node)
            .then(wasmtime::code_cache)
            .flatten();
        run_all(&cases, &store)
    };
    let elapsed = started.elapsed();
    finish_wasm_digests();
    if mode != EngineMode::Node || selected_tier.is_some() {
        report_runtime(&cases, &results, elapsed);
    }
    let crashes: Vec<_> = cases
        .iter()
        .zip(&results)
        .filter(|(_, verdict)| matches!(verdict, Verdict::Crash(_)))
        .map(|(case, verdict)| format!("{}: {verdict:?}", case.path))
        .collect();
    assert!(
        crashes.is_empty(),
        "compiler crashes:\n{}",
        crashes.join("\n")
    );

    if only.is_some() || selected_tier.is_some() {
        print_filtered(&cases, &results, &store);
        return;
    }
    let engine = mode.name();
    let outcomes: Vec<Outcome<'_>> = cases
        .iter()
        .zip(&results)
        .map(|(case, verdict)| Outcome {
            path: &case.path,
            bucket: match verdict {
                Verdict::Pass => None,
                Verdict::Fail(code) => Some(format!("fail:{code}")),
                Verdict::Unsupported(stage) => Some(format!("unsupported:{stage}")),
                Verdict::Crash(message) => Some(format!("crash:{message}")),
            },
        })
        .collect();
    let known = known_failures::load(&root());
    let (summary, errors) = known_failures::judge(
        "driver",
        engine,
        Some(pipeline_name()),
        &known,
        |path| !path.starts_with("cli/"),
        &outcomes,
    );
    // Straight to stdout, past the test harness's capture, so a passing run
    // shows the counts too.
    std::io::stdout()
        .write_all(summary.as_bytes())
        .expect("print the summary");
    let linear = judge_linear(&cases, engine);
    assert!(
        errors.is_empty(),
        "conformance cases break the KNOWN_FAILURES.tsv rule:\n{}",
        errors.join("\n")
    );
    assert!(
        linear.is_empty(),
        "linear-backend runs break the LINEAR_PASSES.tsv rule:\n{}",
        linear.join("\n")
    );
}

/// Judges the linear backend's runs ([`LINEAR_VERDICTS`]) and prints their
/// summary: every case [`LINEAR_PASSES`] lists must be a runtime case that
/// passed, and no linear run may crash. A sweep also prints how its other
/// cases fared and each passing one the list does not name yet.
fn judge_linear(cases: &[Case], engine: &str) -> Vec<String> {
    let verdicts = LINEAR_VERDICTS.lock().expect("linear verdicts lock");
    let listed = linear_passes();
    let mut errors = Vec::new();
    let mut out = String::new();
    let runtime: BTreeSet<&str> = cases
        .iter()
        .filter(|case| case.phase == Phase::Runtime)
        .map(|case| case.path.as_str())
        .collect();
    for path in listed.keys() {
        if !runtime.contains(path.as_str()) {
            errors.push(format!(
                "{path}: no such runtime case: remove its row from {LINEAR_PASSES}"
            ));
        } else if !verdicts.contains_key(path) {
            errors.push(format!(
                "{path}: its fixture did not load, so it has no linear run"
            ));
        }
    }
    let (mut pass, mut fail, mut unlisted) = (0, 0, Vec::new());
    let mut buckets: BTreeMap<String, usize> = BTreeMap::new();
    for (path, verdict) in verdicts.iter() {
        let bucket = match verdict {
            Verdict::Pass => {
                pass += 1;
                if !listed.contains_key(path) {
                    unlisted.push(path.as_str());
                }
                continue;
            }
            Verdict::Fail(code) => format!("fail:{code}"),
            Verdict::Unsupported(stage) => format!("unsupported:{stage}"),
            Verdict::Crash(message) => {
                errors.push(format!("{path}: linear-backend crash: {message}"));
                format!("crash:{message}")
            }
        };
        fail += 1;
        if listed.contains_key(path) {
            errors.push(format!(
                "{path}: {bucket} on the linear backend, but {LINEAR_PASSES} lists it as passing"
            ));
        }
        *buckets.entry(bucket).or_default() += 1;
    }
    if linear_sweep() {
        let mut sorted: Vec<_> = buckets.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (bucket, count) in sorted {
            let _ = writeln!(out, "{count:>5} linear {bucket}");
        }
        for path in &unlisted {
            let _ = writeln!(out, "linear pass, not in {LINEAR_PASSES}: {path}");
        }
    }
    let _ = writeln!(
        out,
        "driver (linear backend) conformance on {engine}, {} pipeline: listed {}, pass {pass}, fail {fail}, unlisted passes {}, total {}",
        pipeline_name(),
        listed.len(),
        unlisted.len(),
        verdicts.len()
    );
    std::io::stdout()
        .write_all(out.as_bytes())
        .expect("print the summary");
    errors
}

/// Every `typing/invalid` fixture, checked: none reaches an internal error
/// (`hd_diag::DiagBuf::internal_error`), such as a body that fails the TIR
/// verifier. Error recovery must leave a body the verifier accepts, so a
/// rejected program reports its diagnostic and never a compiler bug. The
/// suite's judging leaves stopped stages out of a case's errors, so a case
/// can pass with an internal error beside its diagnostic; this test does
/// not let it.
#[test]
fn no_typing_invalid_fixture_reaches_an_internal_error() {
    let cases: Vec<Case> = cases()
        .into_iter()
        .filter(|case| case.phase == Phase::Type && case.path.starts_with("typing/invalid/"))
        .collect();
    assert!(!cases.is_empty(), "no typing/invalid cases");
    let store = MemoryStore::default();
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers().min(cases.len()) {
            scope.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let fixture = fixture(case).expect("fixture");
                    let output = check_fixture(&fixture, &store);
                    for index in 0..output.diags.len() {
                        if output.diags.is_internal(index) {
                            let message = output.diags.get_text(output.diags.message[index]);
                            let line = format!("{}: {message}", case.path);
                            found.lock().expect("found lock").push(line);
                        }
                    }
                }
            });
        }
    });
    let mut found = found.into_inner().expect("found lock");
    found.sort();
    assert!(
        found.is_empty(),
        "typing/invalid fixtures reach an internal error:\n{}",
        found.join("\n")
    );
}
