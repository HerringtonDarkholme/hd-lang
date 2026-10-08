//! `hd test [FILE] [--filter PATTERN] [--jobs N]` (spec/cli/command-line.md,
//! "Test Runs"; engines-and-test-runner.md §19): build the package's
//! unit-test program, run each selected case in a fresh instance on Node
//! workers, and print the results through the release cursor in content
//! order, whatever order the workers finish in.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_check::tests::PANIC_CATEGORIES;
use hd_driver::{Executor, Goal, Host, TestCase, build};
use hd_run::tests_model::{
    Case, CaseKey, CaseKind, CaseResult, ProgramKey, ReleaseCursor, TestPlan,
};

use crate::node::{CaseRun, run_cases};
use crate::{Wall, cache_dir, disk};

/// `cli.exit.hd-failure`.
const HD_FAILURE: u8 = 101;

fn fail(msg: &str) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::from(HD_FAILURE)
}

struct Options {
    file: Option<PathBuf>,
    filter: Option<String>,
    jobs: usize,
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        filter: None,
        jobs: crate::default_jobs(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let text = a.to_string_lossy();
        let mut value = |flag: &str| -> Result<String, String> {
            if let Some(v) = text.strip_prefix(&format!("{flag}=")) {
                return Ok(v.to_owned());
            }
            it.next()
                .map(|v| v.to_string_lossy().into_owned())
                .ok_or_else(|| format!("`{flag}` needs a value"))
        };
        if text == "--filter" || text.starts_with("--filter=") {
            o.filter = Some(value("--filter")?);
        } else if text == "--jobs" || text.starts_with("--jobs=") {
            let v = value("--jobs")?;
            o.jobs = v
                .parse::<usize>()
                .ok()
                .filter(|n| *n >= 1)
                .ok_or_else(|| format!("`--jobs {v}`: N is a whole number of at least 1"))?;
        } else if text.starts_with('-') {
            return Err(format!("`hd test` has no option `{text}`"));
        } else if o.file.is_some() {
            return Err("`hd test` takes at most one FILE".into());
        } else {
            o.file = Some(PathBuf::from(a));
        }
    }
    Ok(o)
}

/// The package root and, with a FILE, its module path below the package.
fn target(file: Option<&Path>) -> Result<(PathBuf, Option<String>), String> {
    let Some(f) = file else {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        return disk::package_root(&cwd).map(|r| (r, None)).ok_or_else(|| {
            "outside any package, `hd test` needs a FILE; pass one, or create a package with `hd new`"
                .to_owned()
        });
    };
    if f.is_dir() {
        return Err(format!(
            "`{}` is a directory, not a FILE; select a package member with `-p NAME`",
            f.display()
        ));
    }
    if f.extension().is_none_or(|x| x != "hd") {
        return Err(format!("`{}` is not an .hd file", f.display()));
    }
    let full = std::fs::canonicalize(f).map_err(|e| format!("{}: {e}", f.display()))?;
    let dir = full.parent().unwrap_or(Path::new("."));
    let root = disk::package_root(dir).unwrap_or_else(|| dir.to_path_buf());
    let rel = full
        .strip_prefix(&root)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((root, Some(rel.trim_end_matches(".hd").replace('/', "."))))
}

pub fn command(args: &[OsString]) -> ExitCode {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let (root, file_module) = match target(o.file.as_deref()) {
        Ok(t) => t,
        Err(e) => return fail(&e),
    };
    let (sources, package) = match disk::sources_of(&root) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let store = DiskStore { root: cache_dir() };
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &clock,
        executor: if o.jobs == 1 {
            Executor::Serial(hd_sched::SerialOrder::Priority)
        } else {
            Executor::Pool(o.jobs)
        },
    };
    let goal = Goal::Tests {
        module: file_module.as_ref().map(|m| format!("{package}.{m}")),
        filter: o.filter.clone(),
    };
    let out = build(&host, &package, &goal);
    // Warnings are shown; only errors stop the command.
    eprint!("{}", out.render_located(&sources));
    if out.diags.has_errors() {
        return ExitCode::from(HD_FAILURE);
    }
    if let Some(f) = &o.file
        && out.tests.is_empty()
    {
        // cli.test.file-empty, cli.test.filter.none
        return fail(&match &o.filter {
            Some(p) => format!(
                "no test case of `{}` has a name that contains `{p}`",
                f.display()
            ),
            None => format!("`{}` registers no test case", f.display()),
        });
    }
    run(&out.tests, out.wasm.as_deref(), o.jobs)
}

/// Runs the cases on up to `jobs` Node workers, round robin in content
/// order, and prints the results in content order.
fn run(cases: &[TestCase], wasm: Option<&[u8]>, jobs: usize) -> ExitCode {
    let plan = TestPlan::new(
        cases
            .iter()
            .enumerate()
            .map(|(i, c)| Case {
                key: CaseKey {
                    program: ProgramKey::Unit {
                        package: String::new(),
                    },
                    registration: u32::try_from(i).unwrap_or(u32::MAX),
                    row: 0,
                },
                name: c.name.clone(),
                kind: CaseKind::It,
                module: c.module.clone(),
                export: c.run.map_or(u32::MAX, |r| r.0),
            })
            .collect(),
        None,
    );
    let mut cursor = ReleaseCursor::new(&plan);
    let mut report = Report::default();
    let key = |i: usize| plan.cases[i].key.clone();
    let mut running: Vec<(usize, (u32, u32))> = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let r = if let Some(what) = &c.unsupported {
            CaseResult::Unsupported { what: what.clone() }
        } else if let Some(reason) = &c.ignore {
            CaseResult::Ignored {
                reason: reason.clone(),
            }
        } else if let Some(run) = c.run {
            running.push((i, run));
            continue;
        } else {
            CaseResult::Unsupported {
                what: "a case without a checked body".into(),
            }
        };
        report.release(cases, cursor.finish(key(i), r));
    }
    if !running.is_empty() {
        let Some(wasm) = wasm else {
            return fail("internal: the test program was not built");
        };
        let workers = jobs.min(running.len()).max(1);
        let (tx, rx) = mpsc::channel::<Result<(usize, CaseRun), String>>();
        let by_test: Vec<usize> = {
            let mut v = vec![usize::MAX; running.len()];
            for (i, (_, (t, _))) in running.iter().enumerate() {
                if let Some(x) = v.get_mut(*t as usize) {
                    *x = i;
                }
            }
            v
        };
        let mut error = None;
        std::thread::scope(|s| {
            for w in 0..workers {
                let tx = tx.clone();
                let chunk: Vec<(u32, u32)> = running
                    .iter()
                    .skip(w)
                    .step_by(workers)
                    .map(|(_, r)| *r)
                    .collect();
                let by_test = &by_test;
                let running = &running;
                s.spawn(move || {
                    let r = run_cases(wasm, &chunk, &mut |c: CaseRun| {
                        let at = by_test.get(c.test as usize).copied().unwrap_or(usize::MAX);
                        if let Some((i, _)) = running.get(at) {
                            let _ = tx.send(Ok((*i, c)));
                        }
                    });
                    if let Err(e) = r {
                        let _ = tx.send(Err(e));
                    }
                });
            }
            drop(tx);
            for msg in rx {
                match msg {
                    Ok((i, c)) => {
                        let r = judge(&cases[i], &c);
                        report.release(cases, cursor.finish(key(i), r));
                    }
                    Err(e) => {
                        error.get_or_insert(e);
                    }
                }
            }
        });
        if let Some(e) = error {
            report.flush();
            return fail(&e);
        }
    }
    report.finish()
}

/// A finished case's outcome (`module.testing.pass`, `module.testing.fail`,
/// `module.testing.expect-panic-fail`).
fn judge(case: &TestCase, run: &CaseRun) -> CaseResult {
    if run.trapped {
        let (category, message) = panic_of(&run.stderr);
        if case.expect_panic.as_deref() == Some(category.as_str()) {
            return CaseResult::Passed { us: run.us };
        }
        return CaseResult::Panicked { category, message };
    }
    if let Some(want) = &case.expect_panic {
        return CaseResult::Failed {
            message: format!("the case completed, but it expects the panic `{want}`"),
        };
    }
    if run.status == 0 {
        return CaseResult::Passed { us: run.us };
    }
    let text = run.stderr.trim_end();
    CaseResult::Failed {
        message: if text.is_empty() {
            format!("`report()` returned `ExitCode({})`", run.status)
        } else {
            text.to_owned()
        },
    }
}

/// The category and message of the host's panic report
/// (`panic: <category>: <message>`); a report without a known category is
/// the runner's own failure.
fn panic_of(stderr: &str) -> (String, String) {
    let Some(rest) = stderr.lines().rev().find_map(|l| l.strip_prefix("panic: ")) else {
        return ("internal".into(), stderr.trim().to_owned());
    };
    let (cat, msg) = rest.split_once(": ").unwrap_or((rest, ""));
    match cat {
        c if PANIC_CATEGORIES.contains(&c) => (c.to_owned(), msg.to_owned()),
        "deadlock" => ("suspension-deadlock".into(), msg.to_owned()),
        _ => ("explicit-panic".into(), rest.to_owned()),
    }
}

/// What `hd test` prints: quiet passes, each other case with its location,
/// a failure with its message and a repro command, then a summary
/// (engines-and-test-runner.md §19.5).
#[derive(Default)]
struct Report {
    text: String,
    passed: usize,
    failed: usize,
    ignored: usize,
    unsupported: usize,
}

impl Report {
    fn release(&mut self, cases: &[TestCase], done: Vec<(CaseKey, CaseResult)>) {
        for (k, r) in done {
            let Some(c) = cases.get(k.registration as usize) else {
                continue;
            };
            let at = format!("{}:{}", c.file, c.line);
            let repro = format!("hd test {} --filter \"{}\"", c.file, c.name);
            let t = &mut self.text;
            match r {
                CaseResult::Passed { .. } => self.passed += 1,
                CaseResult::Failed { message } => {
                    self.failed += 1;
                    let _ = writeln!(t, "FAIL {at}: {}", c.name);
                    for l in message.lines() {
                        let _ = writeln!(t, "    {l}");
                    }
                    let _ = writeln!(t, "    repro: {repro}");
                }
                CaseResult::Panicked { category, message } => {
                    self.failed += 1;
                    let _ = writeln!(t, "PANIC {at}: {}", c.name);
                    if message.is_empty() {
                        let _ = writeln!(t, "    panic: {category}");
                    } else {
                        let _ = writeln!(t, "    panic: {category}: {message}");
                    }
                    let _ = writeln!(t, "    repro: {repro}");
                }
                CaseResult::TimedOut => {
                    self.failed += 1;
                    let _ = writeln!(t, "PANIC {at}: {}\n    panic: time-limit", c.name);
                }
                CaseResult::Ignored { reason } => {
                    self.ignored += 1;
                    let _ = writeln!(t, "IGNORED {at}: {} ({reason})", c.name);
                }
                CaseResult::Unsupported { what } => {
                    self.unsupported += 1;
                    let _ = writeln!(t, "UNSUPPORTED {at}: {} ({what} cannot run yet)", c.name);
                }
            }
        }
        self.flush();
    }

    fn flush(&mut self) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(self.text.as_bytes());
        let _ = out.flush();
        self.text.clear();
    }

    fn finish(mut self) -> ExitCode {
        let verdict = if self.failed > 0 { "FAILED" } else { "ok" };
        let _ = writeln!(
            self.text,
            "test result: {verdict}. {} passed; {} failed; {} ignored; {} unsupported",
            self.passed, self.failed, self.ignored, self.unsupported
        );
        self.flush();
        if self.failed > 0 {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        }
    }
}
