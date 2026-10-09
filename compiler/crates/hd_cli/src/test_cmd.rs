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
use hd_driver::{Executor, Goal, Host, TestCase, build_packages};
use hd_project::SourceSet as _;
use hd_run::tests_model::{
    Case, CaseKey, CaseKind, CaseResult, ProgramKey, ReleaseCursor, TestPlan,
};

use crate::node::{CaseRun, run_cases};
use crate::report::{self, Reporter};
use crate::{HD_FAILURE, Wall, cache_dir, disk, fail};

struct Options {
    file: Option<PathBuf>,
    filter: Option<String>,
    jobs: usize,
    json: bool,
    /// The members `-p NAME` selects (`cli.workspace.select.anywhere`).
    packages: Vec<String>,
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        filter: None,
        jobs: crate::default_jobs(),
        json: false,
        packages: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        if let Some(format) = report::format_flag(args, i) {
            let (json, used) = format?;
            o.json = json;
            i += used;
            continue;
        }
        if let Some(p) = disk::package_flag(args, i) {
            let (name, used) = p?;
            o.packages.push(name);
            i += used;
            continue;
        }
        // `cli.cap.flag.commands`: `--cap` is checked here. Its test grant
        // (`cli.test.env.grant`, `Grants::for_test`) covers integration
        // test programs and doc tests; a unit test gets no provider, and
        // neither of those runs yet.
        if let Some(cap) = crate::caps::cap_flag(args, i) {
            let (_, used) = cap?;
            i += used;
            continue;
        }
        let a = &args[i];
        i += 1;
        let text = a.to_string_lossy();
        let mut value = |flag: &str| -> Result<String, String> {
            if let Some(v) = text.strip_prefix(&format!("{flag}=")) {
                return Ok(v.to_owned());
            }
            let v = args
                .get(i)
                .map(|v| v.to_string_lossy().into_owned())
                .ok_or_else(|| format!("`{flag}` needs a value"))?;
            i += 1;
            Ok(v)
        };
        if text == "--release" {
            // `cli.profile.test.release`: the test profile stays checked,
            // and the flag selects only the pipeline, of which there is one
            // (`cli.profile.pipeline.one`).
            continue;
        }
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

/// The package root and, with a FILE, its path relative to that root.
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
    Ok((root.clone(), Some(disk::listed_path(&root, &rel))))
}

/// One package `hd test` tests: its root, FILE relative to it, and in
/// workspace mode the member's directory below the workspace root.
type Unit = (PathBuf, Option<String>, Option<String>);

/// What `hd test` tests: FILE's module, the package of the working
/// directory, or in workspace mode every member (`cli.workspace.members`).
fn units(file: Option<&Path>, packages: &[String]) -> Result<Vec<Unit>, String> {
    if file.is_none() {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        if !packages.is_empty() {
            return Ok(disk::select_members(&cwd, packages)?
                .into_iter()
                .map(|(dir, rel)| (dir, None, Some(rel)))
                .collect());
        }
        if let Some((ws, members)) = disk::workspace_root(&cwd) {
            return Ok(members
                .into_iter()
                .map(|dir| {
                    let member = dir
                        .strip_prefix(&ws)
                        .unwrap_or(&dir)
                        .to_string_lossy()
                        .replace('\\', "/");
                    (dir, None, Some(member))
                })
                .collect());
        }
    }
    let (root, rel) = target(file)?;
    Ok(vec![(root, rel, None)])
}

pub fn command(args: &[OsString]) -> ExitCode {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let mut report = Report {
        rep: Reporter::stdout(o.json),
        text: String::new(),
        passed: 0,
        failed: 0,
        ignored: 0,
        unsupported: 0,
    };
    if o.file.is_some() && !o.packages.is_empty() {
        return report
            .rep
            .fail("`hd test` takes a FILE or `-p NAME`, not both");
    }
    let units = match units(o.file.as_deref(), &o.packages) {
        Ok(u) => u,
        Err(e) => return report.rep.fail(&e),
    };
    for (root, file, member) in units {
        if let Err(code) = test_one(&root, file.as_deref(), member.as_deref(), &o, &mut report) {
            return code;
        }
    }
    report.finish()
}

/// One program of a test run (engines-and-test-runner.md §19.1): the
/// unit-test program, an integration test file (`module.test.integration`)
/// or a doc test (`module.test.doc.program`), each built on its own.
struct Built {
    wasm: Option<Vec<u8>>,
    /// An integration test or a doc test runs with the package directory as
    /// its working directory and a closed standard input
    /// (`cli.test.env.cwd`, `cli.test.env.stdin`).
    cwd: Option<PathBuf>,
}

/// A selected case: its program, the case as the driver lists it, and a
/// result known without running it (a compile-fail doc test).
struct Selected {
    program: usize,
    case: TestCase,
    fixed: Option<CaseResult>,
}

/// A doc test's program, added to the sources at `path`.
pub(crate) struct Doc {
    pub(crate) path: String,
    /// The source file it came from, as the package lists it.
    file: String,
    pub(crate) test: hd_project::DocTest,
}

impl Doc {
    /// The source line of a line of the program (`cli.test.doc.location`).
    fn line(&self, program_line: usize) -> usize {
        program_line
            .checked_sub(1)
            .and_then(|i| self.test.lines.get(i))
            .map_or(1, |&l| l as usize)
    }

    /// Moves a diagnostic of the program to the source file and the `##`
    /// line it came from (`cli.test.doc.location`). Its column and its
    /// fixes name the program's text, so they go.
    pub(crate) fn relocate(&self, d: &mut report::Diag) {
        if d.file.as_deref() == Some(self.path.as_str()) {
            d.file = Some(self.file.clone());
            d.line = d.line.map(|l| self.line(l));
            d.column = None;
            d.fixes.clear();
        }
    }
}

/// The doc tests of the package's source files, or of FILE's module
/// (`cli.test.doc.default`), each as a synthetic integration-view module
/// under the test root (checking-and-tir.md §4.13.9).
pub(crate) fn doc_tests(sources: &disk::DiskSources, file: Option<&str>) -> Vec<Doc> {
    let mut out = Vec::new();
    for e in sources.list() {
        if file.is_some_and(|f| f != e.path) || !e.path.starts_with("src/") {
            continue;
        }
        let Some(text) = sources.read(&e.path) else {
            continue;
        };
        for test in hd_project::doc_tests(&e.path, &String::from_utf8_lossy(&text)) {
            out.push(Doc {
                path: format!("tests/$doc{}.hd", out.len()),
                file: e.path.clone(),
                test,
            });
        }
    }
    out
}

/// Tests one package: builds its unit-test program, each integration test
/// file and each doc test as programs of their own, reports their
/// diagnostics, then runs the selected cases into `report`, in the order of
/// their files' paths and lines (`cli.test.report.order`). A member's case
/// files start with its directory.
fn test_one(
    root: &Path,
    file: Option<&str>,
    member: Option<&str>,
    o: &Options,
    report: &mut Report,
) -> Result<(), ExitCode> {
    let rep = &mut report.rep;
    let mut program = disk::load_package(root, "main").map_err(|e| rep.fail(&e))?;
    if rep.diags(&program.problems) {
        return Err(rep.finish(HD_FAILURE));
    }
    // `cli.test.tasks.no-tests`: a task or an executable's entry module
    // without a `tests:` block gets no test build.
    let entries: Vec<String> = disk::executables(root, &program.package)
        .into_iter()
        .map(|e| e.file)
        .collect();
    let untested: Vec<String> = program
        .sources
        .list()
        .into_iter()
        .map(|e| e.path)
        .filter(|p| {
            let task = p
                .strip_prefix("tasks/")
                .is_some_and(|rest| !rest.contains('/'));
            (task || entries.contains(p)) && file != Some(p.as_str())
        })
        .filter(|p| {
            program
                .sources
                .read(p)
                .is_none_or(|t| !hd_driver::has_tests_block(&String::from_utf8_lossy(&t)))
        })
        .collect();
    program.sources.retain(|p| !untested.iter().any(|u| u == p));
    let integration_file = |p: &str| {
        p.strip_prefix("tests/")
            .is_some_and(|rest| !rest.contains('/'))
    };
    let docs = if file.is_some_and(integration_file) {
        Vec::new()
    } else {
        doc_tests(&program.sources, file)
    };
    for d in &docs {
        program.sources.add_synthetic(&d.path, &d.test.program);
    }
    // The programs: the unit-test program unless FILE is an integration
    // test file; each integration test file, or FILE alone; each doc test.
    let mut specs: Vec<(Option<String>, Option<&Doc>)> = Vec::new();
    if !file.is_some_and(integration_file) {
        specs.push((None, None));
    }
    for e in program.sources.list() {
        let doc = docs.iter().find(|d| d.path == e.path);
        let wanted = match file {
            Some(f) if integration_file(f) => e.path == f,
            Some(_) => doc.is_some(),
            None => integration_file(&e.path),
        };
        if wanted {
            specs.push((Some(e.path), doc));
        }
    }
    let store = DiskStore { root: cache_dir() };
    let clock = Wall(Instant::now());
    let executor = if o.jobs == 1 {
        Executor::Serial(hd_sched::SerialOrder::Priority)
    } else {
        Executor::Pool(o.jobs)
    };
    let packages = program.packages();
    let package = &program.package;
    let mut built: Vec<Built> = Vec::new();
    let mut selected: Vec<Selected> = Vec::new();
    let mut diags: Vec<report::Diag> = Vec::new();
    for (path, doc) in &specs {
        let mut sources = program.sources.clone();
        match path {
            // `cli.package.file`: FILE's module, linked with what it uses.
            None => {
                if let Some(f) = file {
                    let keep = hd_driver::use_closure(package, &sources, f);
                    sources.retain(|p| keep.iter().any(|k| k == p));
                } else {
                    sources.retain(|p| !p.starts_with("tests/"));
                }
            }
            Some(p) => {
                let keep = hd_driver::use_closure(package, &sources, p);
                sources.retain(|q| keep.iter().any(|k| k == q));
            }
        }
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &clock,
            executor,
        };
        let module = match path {
            Some(p) => Some(hd_project::module_path(package, p)),
            None => file.map(|f| hd_project::module_path(package, f)),
        };
        let goal = Goal::Tests {
            module,
            filter: o.filter.clone(),
        };
        let out = build_packages(&host, package, &packages, &goal);
        let mut found = report::from_output(&out, &sources, None);
        // `module.test.doc.compile-fail`: it never runs, and passes only
        // when compiling it reports its code.
        if let Some(d) = doc
            && let Some(code) = &d.test.compile_fail
        {
            if o.filter
                .as_ref()
                .is_some_and(|f| !d.test.name.contains(f.as_str()))
            {
                continue;
            }
            let reported = found
                .iter()
                .any(|x| x.code.is_some_and(|c| c.as_str() == code));
            let fixed = if reported {
                CaseResult::Passed { us: 0 }
            } else {
                CaseResult::Failed {
                    message: format!(
                        "the doc test does not compile with `{code}`, as its `# error:` line says"
                    ),
                }
            };
            selected.push(Selected {
                program: built.len(),
                case: TestCase {
                    module: String::new(),
                    file: d.file.clone(),
                    line: u32::try_from(d.line(1)).unwrap_or(u32::MAX),
                    name: d.test.name.clone(),
                    kind: "it".to_owned(),
                    ignore: None,
                    expect_panic: None,
                    unsupported: None,
                    run: None,
                },
                fixed: Some(fixed),
            });
            built.push(Built {
                wasm: None,
                cwd: None,
            });
            continue;
        }
        if let Some(d) = doc {
            for x in &mut found {
                d.relocate(x);
            }
        }
        report::relocate(&mut found, |f| sources.display(f));
        for x in found {
            if !diags.iter().any(|y| y.text() == x.text()) {
                diags.push(x);
            }
        }
        for mut case in out.tests {
            if let Some(d) = doc {
                case.file.clone_from(&d.file);
                case.line = u32::try_from(d.line(case.line as usize)).unwrap_or(u32::MAX);
            }
            selected.push(Selected {
                program: built.len(),
                case,
                fixed: None,
            });
        }
        built.push(Built {
            wasm: out.wasm,
            cwd: path.as_ref().map(|_| root.to_path_buf()),
        });
    }
    if let Some(m) = member {
        for d in &mut diags {
            if let Some(f) = &mut d.file {
                *f = format!("{m}/{f}");
            }
        }
    }
    // Warnings are shown; only errors stop the command.
    if rep.diags(&diags) {
        return Err(rep.finish(HD_FAILURE));
    }
    if let Some(f) = &o.file
        && selected.is_empty()
    {
        // cli.test.file-empty, cli.test.filter.none
        return Err(rep.fail(&match &o.filter {
            Some(p) => format!(
                "no test case of `{}` has a name that contains `{p}`",
                f.display()
            ),
            None => format!("`{}` registers no test case", f.display()),
        }));
    }
    for s in &mut selected {
        s.case.file = program.sources.display(&s.case.file);
        if let Some(m) = member {
            s.case.file = format!("{m}/{}", s.case.file);
        }
    }
    // `cli.test.report.order`: file path, then line, then registration.
    selected.sort_by(|a, b| (&a.case.file, a.case.line).cmp(&(&b.case.file, b.case.line)));
    run(&selected, &built, o.jobs, report)
}

/// Runs the cases program by program on up to `jobs` Node workers, round
/// robin in content order, and prints the results in content order
/// (`cli.test.report.stream`), whatever order the workers finish in.
fn run(
    selected: &[Selected],
    built: &[Built],
    jobs: usize,
    report: &mut Report,
) -> Result<(), ExitCode> {
    let cases: Vec<TestCase> = selected.iter().map(|s| s.case.clone()).collect();
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
    let key = |i: usize| plan.cases[i].key.clone();
    // Per program: (case index, (test export, init export)).
    let mut running: Vec<Vec<(usize, (u32, u32))>> = built.iter().map(|_| Vec::new()).collect();
    for (i, s) in selected.iter().enumerate() {
        let c = &s.case;
        let r = if let Some(r) = &s.fixed {
            r.clone()
        } else if let Some(what) = &c.unsupported {
            CaseResult::Unsupported { what: what.clone() }
        } else if let Some(reason) = &c.ignore {
            CaseResult::Ignored {
                reason: reason.clone(),
            }
        } else if let Some(run) = c.run {
            running[s.program].push((i, run));
            continue;
        } else {
            CaseResult::Unsupported {
                what: "a case without a checked body".into(),
            }
        };
        report.release(&cases, cursor.finish(key(i), r));
    }
    for (prog, running) in built.iter().zip(&running) {
        if running.is_empty() {
            continue;
        }
        let Some(wasm) = prog.wasm.as_deref() else {
            return Err(report.rep.fail("internal: a test program was not built"));
        };
        let workers = jobs.min(running.len()).max(1);
        let (tx, rx) = mpsc::channel::<Result<(usize, CaseRun), String>>();
        let mut error = None;
        std::thread::scope(|s| {
            for w in 0..workers {
                let tx = tx.clone();
                let chunk: Vec<(usize, (u32, u32))> =
                    running.iter().skip(w).step_by(workers).copied().collect();
                let cwd = prog.cwd.as_deref();
                s.spawn(move || {
                    let exports: Vec<(u32, u32)> = chunk.iter().map(|(_, r)| *r).collect();
                    let r = run_cases(wasm, &exports, cwd, &mut |c: CaseRun| {
                        if let Some((i, _)) = chunk.iter().find(|(_, (t, _))| *t == c.test) {
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
                        report.release(&cases, cursor.finish(key(i), r));
                    }
                    Err(e) => {
                        error.get_or_insert(e);
                    }
                }
            }
        });
        if let Some(e) = error {
            report.flush();
            return Err(report.rep.fail(&e));
        }
    }
    Ok(())
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
/// (engines-and-test-runner.md §19.5). With `--format json`, one test
/// object per case instead (`cli.json.test.result`), then the summary.
struct Report {
    rep: Reporter,
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
            let repro = format!("hd test {} --filter {}", c.file, shell_quoted(&c.name));
            let mut t = String::new();
            // The outcome and message of the case's test object.
            let (outcome, message) = match r {
                CaseResult::Passed { .. } => {
                    self.passed += 1;
                    ("passed", String::new())
                }
                CaseResult::Failed { message } => {
                    self.failed += 1;
                    let _ = writeln!(t, "FAIL {at}: {}", c.name);
                    for l in message.lines() {
                        let _ = writeln!(t, "    {l}");
                    }
                    let _ = writeln!(t, "    repro: {repro}");
                    ("failed", message)
                }
                CaseResult::Panicked { category, message } => {
                    self.failed += 1;
                    let panic = if message.is_empty() {
                        format!("panic: {category}")
                    } else {
                        format!("panic: {category}: {message}")
                    };
                    let _ = writeln!(t, "PANIC {at}: {}\n    {panic}", c.name);
                    let _ = writeln!(t, "    repro: {repro}");
                    ("failed", panic)
                }
                CaseResult::TimedOut => {
                    self.failed += 1;
                    let _ = writeln!(t, "PANIC {at}: {}\n    panic: time-limit", c.name);
                    ("failed", "panic: time-limit".to_owned())
                }
                CaseResult::Ignored { reason } => {
                    self.ignored += 1;
                    let _ = writeln!(t, "IGNORED {at}: {} ({reason})", c.name);
                    ("ignored", reason)
                }
                CaseResult::Unsupported { what } => {
                    self.unsupported += 1;
                    let _ = writeln!(t, "UNSUPPORTED {at}: {} ({what} cannot run yet)", c.name);
                    // JSON has no fourth outcome; as in text, it fails nothing.
                    ("ignored", format!("unsupported: {what} cannot run yet"))
                }
            };
            if self.rep.json {
                let _ = writeln!(
                    self.text,
                    "{{\"kind\":\"test\",\"name\":{},\"outcome\":\"{outcome}\",\"message\":{}}}",
                    report::quote(&c.name),
                    report::quote(&message)
                );
            } else {
                self.text.push_str(&t);
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
        let status = u8::from(self.failed > 0);
        if self.rep.json {
            return self.rep.finish_tests(
                status,
                self.passed,
                self.failed,
                self.ignored + self.unsupported,
            );
        }
        // `cli.test.report.summary`: the verdict and the passed, failed and
        // ignored counts; cases that cannot run yet are counted too, when
        // there are any.
        let verdict = if self.failed > 0 { "FAILED" } else { "ok" };
        let _ = write!(
            self.text,
            "test result: {verdict}. {} passed; {} failed; {} ignored",
            self.passed, self.failed, self.ignored
        );
        if self.unsupported > 0 {
            let _ = write!(self.text, "; {} unsupported", self.unsupported);
        }
        self.text.push('\n');
        self.flush();
        ExitCode::from(status)
    }
}

/// A word as a POSIX shell reads it back, in double quotes
/// (`cli.test.report.repro`: "command-line escaped").
fn shell_quoted(word: &str) -> String {
    let mut out = String::from("\"");
    for c in word.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::shell_quoted;

    #[test]
    fn repro_names_survive_the_shell() {
        assert_eq!(shell_quoted("sums prices"), "\"sums prices\"");
        assert_eq!(
            shell_quoted("costs $5 \"now\""),
            "\"costs \\$5 \\\"now\\\"\""
        );
    }
}
