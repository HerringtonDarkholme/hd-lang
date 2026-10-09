//! `hd check [FILE] [--tests | --all] [--format json]` (`cli.package.whole`, `cli.check.*`,
//! `cli.file.check-test`, `cli.json.*`): the pipeline through checking and
//! coherence, with no collection or emission.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_diag::Code;
use hd_driver::{Goal, Host, Output, build_packages};
use hd_project::{ModuleTable, Role, role_of};

use crate::report::{self, Diag, Summary};
use crate::{HD_FAILURE, Wall, cache_dir, disk, executor, fail, package_of_cwd};

/// What `hd check` was asked for.
struct Options {
    file: Option<OsString>,
    json: bool,
    summary: bool,
    max_errors: Option<usize>,
    /// `--tests`: also the test code (`cli.check.tests`).
    tests: bool,
    /// `--all`: also the test code and the tasks (`cli.check.all`).
    all: bool,
}

fn options(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        json: false,
        summary: false,
        max_errors: None,
        tests: false,
        all: false,
    };
    let mut i = 0;
    while i < args.len() {
        if let Some(format) = report::format_flag(args, i) {
            let (json, used) = format?;
            o.json = json;
            i += used;
            continue;
        }
        let a = &args[i];
        i += 1;
        let text = a.to_string_lossy();
        let switch = match text.as_ref() {
            "--summary" => Some(&mut o.summary),
            "--tests" => Some(&mut o.tests),
            "--all" => Some(&mut o.all),
            _ => None,
        };
        if let Some(on) = switch {
            *on = true;
            continue;
        }
        let max = if text == "--max-errors" {
            let value = args.get(i).ok_or("`--max-errors` needs a value")?;
            i += 1;
            Some(value.to_string_lossy().into_owned())
        } else {
            text.strip_prefix("--max-errors=").map(str::to_owned)
        };
        if let Some(value) = max {
            match value.parse::<usize>() {
                Ok(n) if n >= 1 => o.max_errors = Some(n),
                _ => {
                    return Err(format!(
                        "`--max-errors` takes a whole number of at least 1, not `{value}`"
                    ));
                }
            }
        } else if text.starts_with('-') {
            return Err(format!("`hd check` has no option `{text}`"));
        } else if o.file.replace(a.clone()).is_some() {
            return Err("`hd check` takes at most one FILE".to_owned());
        }
    }
    Ok(o)
}

/// A program to check.
struct Target {
    program: disk::Program,
    /// The package-relative FILE whose module is checked (`cli.package.file`).
    only: Option<String>,
}

fn target(file: Option<&OsString>) -> Result<Target, String> {
    let Some(file) = file else {
        let root = package_of_cwd("check")
            .map_err(|e| format!("{e}; or pass a FILE to check it as a single-file program"))?;
        return Ok(Target {
            program: disk::load_package(&root, "main")?,
            only: None,
        });
    };
    let path = Path::new(file);
    if path.extension().is_none_or(|x| x != "hd") {
        return Err(format!(
            "`{}` is not an .hd file; `hd check` takes a FILE.hd",
            path.display()
        ));
    }
    let full = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = full.parent().unwrap_or(Path::new("."));
    match disk::package_root(dir) {
        Some(root) => {
            let only = full.strip_prefix(&root).ok().map(|rel| {
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            });
            Ok(Target {
                program: disk::load_package(&root, "main")?,
                only,
            })
        }
        None => Ok(Target {
            program: disk::load_file(path)?,
            only: None,
        }),
    }
}

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let o = match options(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let (diags, modules_checked) = match target(o.file.as_ref()) {
        Ok(t) if t.program.problems.iter().any(Diag::is_error) => (t.program.problems, 0),
        Ok(mut t) => {
            let (tests, left_out) = scope(&mut t, &o);
            let goal = if tests {
                Goal::CheckTests
            } else {
                Goal::Analyze
            };
            let store = DiskStore { root: cache_dir() };
            let clock = Wall(Instant::now());
            let host = Host {
                render_tir: &[],
                sources: &t.program.sources,
                store: &store,
                clock: &clock,
                executor: executor(),
            };
            let out = build_packages(&host, &t.program.package, &t.program.packages(), &goal);
            let shown = shown(&out, &t);
            let mut diags = t.program.problems;
            diags.extend(left_out);
            diags.extend(shown);
            (diags, out.counters.modules_checked.len())
        }
        Err(e) => (vec![Diag::error(None, &e)], 0),
    };
    let failed = diags.iter().any(Diag::is_error);
    let status = if failed { HD_FAILURE } else { 0 };
    if o.json {
        let mut text = String::new();
        for d in &diags {
            let _ = writeln!(text, "{}", d.json());
        }
        let summary = Summary {
            status,
            modules_checked: Some(modules_checked),
            ..Summary::of(&diags)
        };
        let _ = writeln!(text, "{}", summary.json());
        print!("{text}");
    } else {
        eprint!("{}", text_report(&diags, &o));
    }
    ExitCode::from(status)
}

/// Narrows a package's sources to what this check covers, and says whether
/// that includes test code. A whole-package check covers the library and
/// the executables (`cli.check.default`); `--tests` adds the test code, its
/// `tests:` blocks, test modules and integration test files
/// (`cli.check.tests`); `--all` adds the tasks too (`cli.check.all`). A
/// FILE of test code or a task brings in its kind. A single-file program is
/// its one file. The layout errors of the files left out, such as
/// `cli.task.beside-dir`, hold whatever the scope, so they are returned.
fn scope(t: &mut Target, o: &Options) -> (bool, Vec<Diag>) {
    let only = t.only.as_deref().map(role_of);
    let tests = o.tests || o.all || only == Some(Role::Test);
    let tasks = o.all || only == Some(Role::Task);
    let keep = |path: &str| match role_of(path) {
        Role::Test => tests,
        Role::Task => tasks,
        Role::Lib | Role::Exe => true,
    };
    if t.program.as_written.is_some() {
        return (tests, Vec::new());
    }
    let layout = ModuleTable::discover(&t.program.package, &t.program.sources);
    let left_out = layout
        .problems
        .iter()
        .map(|(file, code, message)| (&layout.files[file.idx()], code, message))
        .filter(|(path, _, _)| !keep(path))
        .map(|(path, code, message)| Diag::error(Some(*code), message).at(path, Some(1)))
        .collect();
    t.program.sources.retain(keep);
    (tests, left_out)
}

/// The diagnostics this run reports, in content order. A FILE in a package
/// narrows them to its module and the modules it uses, deeply
/// (`cli.package.file`); a diagnostic of no file is kept.
fn shown(out: &Output, t: &Target) -> Vec<Diag> {
    let all = report::from_output(out, &t.program.sources, t.program.as_written.as_deref());
    let Some(only) = &t.only else {
        return all;
    };
    let mut reached = vec![false; out.files.len()];
    let mut stack: Vec<usize> = out
        .files
        .iter()
        .position(|f| f == only)
        .into_iter()
        .collect();
    while let Some(f) = stack.pop() {
        if !std::mem::replace(&mut reached[f], true) {
            stack.extend(out.uses.get(f).into_iter().flatten().copied());
        }
    }
    all.into_iter()
        .filter(|d| {
            d.file.as_ref().is_none_or(|file| {
                out.files
                    .iter()
                    .position(|f| f == file)
                    .is_some_and(|i| reached[i])
            })
        })
        .collect()
}

/// The text report of `cli.check.*`: each diagnostic, or with `--summary` one
/// line per severity, file, and code, then the `check result` line.
fn text_report(diags: &[Diag], o: &Options) -> String {
    let mut text = String::new();
    let (mut errors, mut warnings) = (0, 0);
    let mut stopped = false;
    let mut counts: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for d in diags {
        if d.is_error() {
            errors += 1;
        } else {
            warnings += 1;
        }
        if o.summary {
            let file = d.file.as_deref().unwrap_or("");
            let code = d.code.map_or("", Code::as_str);
            *counts.entry((file, code, d.severity_word())).or_insert(0) += 1;
            continue;
        }
        if stopped {
            continue;
        }
        let _ = writeln!(text, "{}", d.text());
        // Printing stops after the Nth error; the counts keep going.
        stopped = d.is_error() && o.max_errors.is_some_and(|n| errors >= n);
    }
    for ((file, code, severity), n) in &counts {
        let _ = writeln!(text, "{severity}: {file}: {code}: {n}");
    }
    let result = if errors > 0 { "FAILED" } else { "ok" };
    let _ = writeln!(
        text,
        "check result: {result}. errors: {errors}; warnings: {warnings}"
    );
    text
}
