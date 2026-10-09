//! The specification conformance suite through the new compiler.
//!
//! Unsupported language surface is progress data, not a failing test.
//! Crashes always fail. Cases recorded as passing in `CONFORMANCE.md`
//! may not regress. Set `HD_UPDATE_CONFORMANCE=1` to replace the report
//! and its embedded pass list with the current result. The CLI tier runs in
//! `hd_cli/tests/cli_conformance.rs`, which owns the last section of the
//! report; this test keeps that section when it rewrites the file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;

use hd_base::Stage;
use hd_cache::MemoryStore;
use hd_diag::Severity;
use hd_driver::{
    Dependency, Executor, Goal, Host, NoClock, Output, Packages, PipelineReport, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_syntax::parse;

const START_COMMIT: &str = "103dd1e3";
const PASS_START: &str = "<!-- pass-list-start -->";
const PASS_END: &str = "<!-- pass-list-end -->";
const CLI_HEADING: &str = "\n## CLI Conformance\n";

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
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn suite_root() -> PathBuf {
    root().join("spec/conformance")
}

fn report_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../CONFORMANCE.md")
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
    })
}

fn first_unsupported(report: &PipelineReport, back_half: bool) -> Option<String> {
    if report.body_failed > 0 {
        return Some("Body".to_owned());
    }
    // `Goal::Analyze` checks a package without a program root, so it never
    // collects: the back half is judged by the `Goal::Program` build.
    Stage::ALL
        .into_iter()
        .take_while(|stage| back_half || *stage != Stage::Collect)
        .find_map(|stage| {
            (report.tally(stage).not_implemented > 0).then(|| stage.name().to_owned())
        })
}

fn build_fixture(fixture: &Fixture, store: &MemoryStore, goal: &Goal) -> Output {
    let host = Host {
        render_tir: &[],
        sources: &fixture.sources,
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
                sources,
                requires: Vec::new(),
            })
            .collect(),
        ..Packages::default()
    };
    build_packages(&host, "fixture", &packages, goal)
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

fn diagnostic_verdict(case: &Case, fixture: &Fixture, output: &Output) -> Option<Verdict> {
    let errors: Vec<usize> = output
        .diags
        .content_order()
        .into_iter()
        .filter(|index| output.diags.severity[*index] == Severity::Error)
        .collect();
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

fn type_case(case: &Case, fixture: &Fixture, store: &MemoryStore) -> Verdict {
    let output = build_fixture(fixture, store, &Goal::Analyze);
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

fn has_runtime_harness(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with("tests:") || line.starts_with("## "))
        || directive(text, "fixture-runtime-profile").is_some()
        || directive(text, "fixture-runtime-scenario").is_some()
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

fn run_wasm(wasm: &[u8], serial: usize) -> std::io::Result<std::process::Output> {
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("conformance-{serial}.wasm"));
    std::fs::write(&path, wasm)?;
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output();
    let _ = std::fs::remove_file(path);
    result
}

/// A fixture whose only harness is a single-file `tests:` block.
fn plain_tests(text: &str) -> bool {
    text.lines().any(|line| line.starts_with("tests:"))
        && !text.lines().any(|line| line.starts_with("## "))
        && [
            "fixture-runtime-profile",
            "fixture-runtime-scenario",
            "fixture-runtime-pending-function",
            "fixture-test-layout",
            "fixture-package-tree",
        ]
        .iter()
        .all(|name| directive(text, name).is_none())
}

/// Whether a test host result line reports panic category `code`.
fn reports_panic(line: &str, code: &str) -> bool {
    [":", "\\n", "\""]
        .iter()
        .any(|end| line.contains(&format!("panic: {code}{end}")))
}

/// `test FILE`: each case of the fixture's `tests:` block in its own
/// instance (spec/conformance/README.md, Runtime Execution steps 2 and 3).
fn tests_case(case: &Case, fixture: &Fixture, store: &MemoryStore, serial: usize) -> Verdict {
    let built = build_fixture(
        fixture,
        store,
        &Goal::Tests {
            module: None,
            filter: None,
        },
    );
    if built.diags.has_errors() {
        let index = first_error(&built);
        if built.diags.code[index].as_str() == "unsupported" {
            return Verdict::Unsupported(
                first_unsupported(&built.report, true).unwrap_or_else(|| "Build".to_owned()),
            );
        }
        return Verdict::Fail(built.diags.code[index].as_str().to_owned());
    }
    if built.tests.is_empty() {
        return Verdict::Unsupported("TestPlan".to_owned());
    }
    if built.tests.iter().any(|test| test.unsupported.is_some()) {
        return Verdict::Unsupported("TestCase".to_owned());
    }
    let runs: Vec<_> = built
        .tests
        .iter()
        .filter(|test| test.ignore.is_none())
        .filter_map(|test| test.run.map(|run| (test, run)))
        .collect();
    let Some(wasm) = built.wasm else {
        return Verdict::Unsupported("Link".to_owned());
    };
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("conformance-{serial}.wasm"));
    if let Err(error) = std::fs::write(&path, &wasm) {
        return Verdict::Crash(error.to_string());
    }
    let list: Vec<String> = runs
        .iter()
        .map(|(_, (test, init))| format!("{test}:{init}"))
        .collect();
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/test.mjs"))
        .arg(&path)
        .arg(list.join(","))
        .output();
    let _ = std::fs::remove_file(path);
    let run = match result {
        Ok(run) => run,
        Err(error) => return Verdict::Crash(error.to_string()),
    };
    let stdout = String::from_utf8_lossy(&run.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != runs.len() {
        return Verdict::Fail("runtime-exit".to_owned());
    }
    let mut panicked = Vec::new();
    let mut failed = false;
    for ((test, _), line) in runs.iter().zip(&lines) {
        if line.contains("\"trapped\":true") {
            match &test.expect_panic {
                Some(code) if reports_panic(line, code) => {}
                _ => panicked.push(*line),
            }
        } else if test.expect_panic.is_some() || !line.contains("\"status\":0,") {
            failed = true;
        }
    }
    if let Some(code) = case.expectation.strip_prefix("panic:") {
        if panicked.iter().any(|line| reports_panic(line, code)) {
            Verdict::Pass
        } else {
            Verdict::Fail("runtime-exit".to_owned())
        }
    } else if failed || !panicked.is_empty() {
        Verdict::Fail("runtime-exit".to_owned())
    } else {
        Verdict::Pass
    }
}

fn runtime_case(case: &Case, fixture: &Fixture, store: &MemoryStore, serial: usize) -> Verdict {
    let checked = build_fixture(fixture, store, &Goal::Analyze);
    if checked.diags.has_errors() {
        let index = first_error(&checked);
        return Verdict::Fail(checked.diags.code[index].as_str().to_owned());
    }
    if let Some(stage) = first_unsupported(&checked.report, false) {
        return Verdict::Unsupported(stage);
    }
    if has_runtime_harness(&fixture.text) {
        if !plain_tests(&fixture.text) {
            return Verdict::Unsupported("RunCase".to_owned());
        }
        let verdict = tests_case(case, fixture, store, serial);
        if verdict != Verdict::Pass || expected_stdout(&fixture.text).is_none() {
            return verdict;
        }
    }
    let built = build_fixture(
        fixture,
        store,
        &Goal::Program {
            entry: "main".to_owned(),
        },
    );
    if built.diags.has_errors() {
        let index = first_error(&built);
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
    let Some(wasm) = built.wasm else {
        return Verdict::Unsupported("Link".to_owned());
    };
    let run = match run_wasm(&wasm, serial) {
        Ok(run) => run,
        Err(error) => return Verdict::Crash(error.to_string()),
    };
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    if let Some(expected) = case.expectation.strip_prefix("panic:") {
        if !run.status.success() && stderr.contains(&format!("panic: {expected}:")) {
            Verdict::Pass
        } else {
            Verdict::Fail("runtime-exit".to_owned())
        }
    } else if !run.status.success() {
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
        Phase::Runtime => runtime_case(case, &fixture, store, serial),
    }
}

fn chapter(case: &Case) -> String {
    case.specification
        .split('#')
        .next()
        .unwrap_or(&case.specification)
        .to_owned()
}

fn directory(case: &Case) -> String {
    Path::new(&case.path)
        .parent()
        .map_or_else(String::new, |path| path.to_string_lossy().into_owned())
}

fn add_count(counts: &mut Counts, verdict: &Verdict) {
    match verdict {
        Verdict::Pass => counts.pass += 1,
        Verdict::Fail(_) | Verdict::Crash(_) => counts.fail += 1,
        Verdict::Unsupported(_) => counts.unsupported += 1,
    }
}

fn table(out: &mut String, values: &BTreeMap<String, Counts>) {
    out.push_str("| Group | Pass | Fail | Unsupported | Total |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: |\n");
    for (name, counts) in values {
        let total = counts.pass + counts.fail + counts.unsupported;
        writeln!(
            out,
            "| `{name}` | {} | {} | {} | {total} |",
            counts.pass, counts.fail, counts.unsupported
        )
        .expect("write report");
    }
}

fn render_report(cases: &[Case], results: &[Verdict]) -> String {
    let mut total = Counts::default();
    let mut chapters: BTreeMap<String, Counts> = BTreeMap::new();
    let mut directories: BTreeMap<String, Counts> = BTreeMap::new();
    let mut buckets: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut passed = Vec::new();
    for (case, verdict) in cases.iter().zip(results) {
        add_count(&mut total, verdict);
        add_count(chapters.entry(chapter(case)).or_default(), verdict);
        add_count(directories.entry(directory(case)).or_default(), verdict);
        match verdict {
            Verdict::Pass => passed.push(case.path.as_str()),
            Verdict::Fail(bucket) => buckets
                .entry(format!("fail:{bucket}"))
                .or_default()
                .push(&case.path),
            Verdict::Unsupported(stage) => buckets
                .entry(format!("unsupported:{stage}"))
                .or_default()
                .push(&case.path),
            Verdict::Crash(message) => buckets
                .entry(format!("crash:{message}"))
                .or_default()
                .push(&case.path),
        }
    }
    passed.sort_unstable();
    let mut out = format!(
        "# New Compiler Conformance\n\nStatus: measured 2026-10-07 against `{START_COMMIT}`. This is implementation\ncoverage, not accepted language behavior. The Cargo test runs every indexed\nfixture; unsupported surface records progress without failing.\n\n## Summary\n\n| Pass | Fail | Unsupported | Total |\n| ---: | ---: | ---: | ---: |\n| {} | {} | {} | {} |\n\n## By Chapter\n\n",
        total.pass,
        total.fail,
        total.unsupported,
        total.pass + total.fail + total.unsupported
    );
    table(&mut out, &chapters);
    out.push_str("\n## By Directory\n\n");
    table(&mut out, &directories);
    out.push_str("\n## Failure Buckets\n\n");
    out.push_str("A failure bucket is the first diagnostic code. An unsupported bucket is the\ncompiler stage that first declined the case.\n\n");
    out.push_str("| Bucket | Cases |\n| --- | ---: |\n");
    for (bucket, paths) in &buckets {
        writeln!(out, "| `{bucket}` | {} |", paths.len()).expect("write report");
    }
    for (bucket, paths) in buckets {
        writeln!(
            out,
            "\n<details><summary><code>{bucket}</code> ({})</summary>\n",
            paths.len()
        )
        .expect("write report");
        for path in paths {
            writeln!(out, "- `{path}`").expect("write report");
        }
        out.push_str("\n</details>\n");
    }
    out.push_str("\n## Checked-In Pass List\n\n");
    out.push_str("`HD_UPDATE_CONFORMANCE=1` replaces this list with every case that passes.\n\n");
    out.push_str(PASS_START);
    out.push('\n');
    out.push_str("```text\n");
    for path in passed {
        writeln!(out, "{path}").expect("write report");
    }
    out.push_str("```\n");
    out.push_str(PASS_END);
    out.push('\n');
    out
}

fn pass_list(report: &str) -> BTreeSet<String> {
    let Some((_, after)) = report.split_once(PASS_START) else {
        return BTreeSet::new();
    };
    let Some((body, _)) = after.split_once(PASS_END) else {
        return BTreeSet::new();
    };
    body.lines()
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .map(str::to_owned)
        .collect()
}

/// `HD_CONFORMANCE_ONLY=a,b`: run only the cases whose path or rule
/// contains one of the comma-separated parts, print each verdict with the
/// first diagnostic or unsupported reason, and skip the pass-list check.
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
    let mut output = build_fixture(&fixture, store, &Goal::Analyze);
    if case.phase == Phase::Runtime && !output.diags.has_errors() {
        let goal = if plain_tests(&fixture.text) {
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

#[test]
fn specification_conformance_does_not_regress() {
    let all = cases();
    let only = filtered(all.clone());
    let cases = only.clone().unwrap_or(all);
    let store = MemoryStore::default();
    let mut results = Vec::with_capacity(cases.len());
    for (serial, case) in cases.iter().enumerate() {
        let verdict = catch_unwind(AssertUnwindSafe(|| run_case(case, &store, serial)))
            .unwrap_or_else(|panic| {
                let message = panic
                    .downcast_ref::<&str>()
                    .map_or("panic", |message| *message);
                Verdict::Crash(message.to_owned())
            });
        results.push(verdict);
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

    if only.is_some() {
        print_filtered(&cases, &results, &store);
        return;
    }
    if std::env::var_os("HD_UPDATE_CONFORMANCE").is_some() {
        let mut report = render_report(&cases, &results);
        if let Ok(old) = std::fs::read_to_string(report_path())
            && let Some((_, cli)) = old.split_once(CLI_HEADING)
        {
            report.push_str(CLI_HEADING);
            report.push_str(cli);
        }
        std::fs::write(report_path(), report).expect("write report");
        return;
    }
    let report = std::fs::read_to_string(report_path())
        .expect("compiler/CONFORMANCE.md; run with HD_UPDATE_CONFORMANCE=1");
    let baseline = pass_list(&report);
    assert!(!baseline.is_empty(), "the checked-in pass list is empty");
    let current: BTreeSet<&str> = cases
        .iter()
        .zip(&results)
        .filter(|(_, verdict)| **verdict == Verdict::Pass)
        .map(|(case, _)| case.path.as_str())
        .collect();
    let regressed: Vec<_> = baseline
        .iter()
        .filter(|path| !current.contains(path.as_str()))
        .cloned()
        .collect();
    assert!(
        regressed.is_empty(),
        "previously passing conformance cases regressed:\n{}",
        regressed.join("\n")
    );
}
