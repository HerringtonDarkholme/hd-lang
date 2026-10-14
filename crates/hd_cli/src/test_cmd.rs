//! `hd test [FILE] [--filter PATTERN] [--jobs N]` (spec/cli/command-line.md,
//! "Test Runs"; engines-and-test-runner.md §19): build the package's
//! unit-test program, run each selected case in a fresh instance on the selected
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
use hd_driver::{Executor, Goal, Host, TestCase};

use crate::memory_cap::build_packages;
use hd_project::SourceSet as _;
use hd_run::tests_model::{
    Case, CaseKey, CaseKind, CaseResult, ProgramKey, ReleaseCursor, TestPlan,
};

use crate::caps::CapFlag;
use crate::engine::{CaseRun, HostCase, Snapshot, TestEnv, TestPrograms, TestSession};
use crate::report::{self, Reporter};
use crate::{HD_FAILURE, Wall, cache_dir, disk, fail};

struct Options {
    file: Option<PathBuf>,
    filter: Option<String>,
    jobs: usize,
    json: bool,
    /// The members `-p NAME` selects (`cli.workspace.select.anywhere`).
    packages: Vec<String>,
    /// The `--cap` flags, which the test grant reads
    /// (`cli.test.env.grant`).
    caps: Vec<CapFlag>,
    /// `--seed N`: every property test case's base seed
    /// (`cli.test.seed.flag`).
    seed: Option<i64>,
    /// `--update`: an update run, which records missing or differing
    /// snapshot files (`std-testing.snapshot-file.missing`).
    update: bool,
    engine: crate::engine::EngineKind,
    release: bool,
    limits: crate::engine::ProgramLimits,
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    parse_with_engine(args, crate::engine::from_env()?)
}

fn parse_with_engine(
    args: &[OsString],
    engine: crate::engine::EngineKind,
) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        filter: None,
        jobs: crate::default_jobs(),
        json: false,
        packages: Vec::new(),
        caps: Vec::new(),
        seed: None,
        update: false,
        engine,
        release: false,
        limits: crate::engine::ProgramLimits::default(),
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
        if let Some(engine) = crate::engine::engine_flag(args, i) {
            let (kind, used) = engine?;
            o.engine = kind;
            i += used;
            continue;
        }
        if let Some(limit) = crate::engine::limit_flag(args, i) {
            let (flag, used) = limit?;
            match flag {
                crate::engine::LimitFlag::Heap(bytes) => o.limits.heap_bytes = Some(bytes),
                crate::engine::LimitFlag::Time(ms) => o.limits.wall_ms = Some(ms),
                crate::engine::LimitFlag::CompilerMemory(_) => {
                    return Err("--max-memory is a global compiler option".into());
                }
            }
            i += used;
            continue;
        }
        // `cli.cap.flag.commands`: the flags join the test grant of each
        // integration test program and doc test (`cli.test.env.grant`); a
        // unit test gets no provider.
        if let Some(cap) = crate::caps::cap_flag(args, i) {
            let (flag, used) = cap?;
            o.caps.push(flag);
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
            // and the flag selects only the pipeline
            // (`cli.profile.pipeline.one`).
            o.release = true;
            continue;
        }
        if text == "--update" {
            o.update = true;
        } else if text == "--filter" || text.starts_with("--filter=") {
            o.filter = Some(value("--filter")?);
        } else if text == "--seed" || text.starts_with("--seed=") {
            // `cli.test.seed.flag`: N is a whole number.
            let v = value("--seed")?;
            o.seed = Some(
                v.parse::<i64>()
                    .ok()
                    .filter(|n| *n >= 0)
                    .ok_or_else(|| format!("`--seed {v}`: N is a whole number"))?,
            );
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
        inputs: std::collections::HashMap::new(),
        text: String::new(),
        passed: 0,
        failed: 0,
        ignored: 0,
        unsupported: 0,
    };
    if o.engine == crate::engine::EngineKind::Node {
        for (flag, present) in [
            ("--max-heap", o.limits.heap_bytes.is_some()),
            ("--time-limit", o.limits.wall_ms.is_some()),
        ] {
            if present {
                report.rep.diag(&report::Diag::new(
                    None,
                    hd_diag::Severity::Warning,
                    &format!(
                        "{flag} is ignored on Node until W5; use --engine wasmtime for enforcement"
                    ),
                ));
            }
        }
    }
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
    /// (`cli.test.env.cwd`, `cli.test.env.stdin`); a unit test has none.
    cwd: Option<PathBuf>,
    /// What its `Args.program` returns: the test case's file relative to
    /// the package directory (`cli.test.env.args.program`).
    program: String,
    /// What names its panics' frames, when it was built.
    frames: Option<Frames>,
}

/// What names one program's panic frames (`cli.test.report.frame`): its
/// sources' positions, and its line table, read on the program's first
/// trap and shared by every case after it (wasm-layout.md §15.9, Q2).
struct Frames {
    positions: hd_driver::Positions,
    table: std::sync::OnceLock<Option<hd_run::frames::LineTable>>,
    /// A doc test's program: its path, its source file, and the source
    /// line of each of its lines (`cli.test.doc.location`).
    doc: Option<(String, String, Vec<u32>)>,
    /// A workspace member's directory, which its files start with.
    member: Option<String>,
}

impl Frames {
    /// A trap's frames as `FILE:LINE FUNCTION` lines, panic site first,
    /// each function named as hd names it. A frame outside compiled hd
    /// code, or in a body the run did not check, is left out, and repeated
    /// frames are collapsed ([`collapse`]).
    fn lines(&self, wasm: &[u8], raw: &[(u32, u32)], src: &disk::DiskSources) -> Vec<String> {
        if raw.is_empty() {
            return Vec::new();
        }
        let Some(table) = self
            .table
            .get_or_init(|| hd_run::frames::LineTable::parse(wasm).ok())
        else {
            return Vec::new();
        };
        let lines: Vec<String> = table
            .anchor(raw)
            .into_iter()
            .filter_map(|a| {
                let (file, line) = self.positions.line(&a.item, a.inst)?;
                let (file, line) = match &self.doc {
                    Some((path, source, lines)) if path == file => (
                        source.clone(),
                        line.checked_sub(1)
                            .and_then(|i| lines.get(i as usize))
                            .copied()
                            .unwrap_or(1),
                    ),
                    _ => (src.display(file), line),
                };
                let file = match &self.member {
                    Some(m) => format!("{m}/{file}"),
                    None => file,
                };
                Some(format!("{file}:{line} {}", self.positions.name(&a.item)))
            })
            .collect();
        collapse(&lines)
    }
}

/// The longest cycle of frames [`collapse`] looks for.
const FRAME_CYCLE: usize = 8;

/// Frames with each run of repeats printed once: a frame repeated right
/// after itself, then `... frame repeated N more times`; a cycle of up to
/// [`FRAME_CYCLE`] frames repeated right after itself (A B A B), then
/// `... last K frames repeated N more times`. So a recursion that never
/// ends reports a few lines. At each frame the cycle that covers the most
/// frames wins, the shorter on a tie.
fn collapse(frames: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < frames.len() {
        // The best cycle at `i`: its length, its repeats after the first.
        let mut best = (1, 0);
        for k in 1..=FRAME_CYCLE.min((frames.len() - i) / 2) {
            let block = &frames[i..i + k];
            let repeats = frames[i + k..]
                .chunks_exact(k)
                .take_while(|c| *c == block)
                .count();
            if repeats > 0 && k * (repeats + 1) > best.0 * (best.1 + 1) {
                best = (k, repeats);
            }
        }
        let (k, repeats) = best;
        out.extend_from_slice(&frames[i..i + k]);
        if repeats > 0 {
            let what = if k == 1 {
                "frame".to_owned()
            } else {
                format!("last {k} frames")
            };
            let times = if repeats == 1 { "time" } else { "times" };
            out.push(format!("... {what} repeated {repeats} more {times}"));
        }
        i += k * (repeats + 1);
    }
    out
}

/// A selected case: its program, the case as the driver lists it, and a
/// result known without running it (a compile-fail doc test).
struct Selected {
    program: usize,
    case: TestCase,
    fixed: Option<CaseResult>,
    /// The `<module>` of its snapshot files' folder
    /// (`std-testing.snapshot-file.module`).
    snapshots: String,
}

/// The `<module>` part of a snapshot file's path for a case in `file`
/// (`std-testing.snapshot-file.module`, `.module.root`): its module path
/// below the package, `tests.<name>` under the test root, and `pkg` for
/// `src/lib.hd`.
fn snapshot_module(file: &str) -> String {
    let below = hd_project::module_below(file);
    if below.is_empty() {
        "pkg".to_owned()
    } else {
        below
    }
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
    // An earlier member's tests disarmed the compiler memory cap
    // (`cli.memory.cap`); this member's builds are limited again.
    crate::memory_cap::rearm();
    let rep = &mut report.rep;
    let mut program = disk::load_package(root, "main").map_err(|e| rep.fail(&e))?;
    // `cli.file.check-test`: outside any package, FILE is tested as a
    // single-file program, whose declarations print under the file's stem
    // (`trait.typeid.name.single-file`), in its frames too.
    program.single_file = disk::package_root(root).is_none();
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
    let integration_file = hd_driver::integration_test_file;
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
    let specs: Vec<(hd_driver::TestProgram, Option<&Doc>)> =
        hd_driver::test_programs(&program.package, &program.sources, file, &|p| {
            docs.iter().any(|d| d.path == p)
        })
        .into_iter()
        .map(|t| {
            let doc = docs.iter().find(|d| Some(&d.path) == t.file.as_ref());
            (t, doc)
        })
        .collect();
    let store = DiskStore { root: cache_dir() };
    let clock = Wall(Instant::now());
    let executor = if o.jobs == 1 {
        Executor::Serial(hd_sched::SerialOrder::Priority)
    } else {
        Executor::Pool(o.jobs)
    };
    // `cli.profile.pipeline.one`: `--release` selects the optimized
    // pipeline; the profile stays checked. The programs are tuned for the
    // engine the tests run on.
    let pipeline = crate::Pipeline::of(o.engine, o.release);
    let packages = hd_driver::Packages {
        target: pipeline.target,
        level: pipeline.level,
        ..program.packages()
    };
    let package = &program.package;
    let mut built: Vec<Built> = Vec::new();
    let mut selected: Vec<Selected> = Vec::new();
    // The programs' modules, each kept once for their panic frames.
    let mut positions = hd_driver::PositionPool::default();
    let mut diags: Vec<report::Diag> = Vec::new();
    for (spec, doc) in &specs {
        let path = &spec.file;
        let mut sources = program.sources.clone();
        sources.retain(|p| spec.keep.iter().any(|k| k == p));
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &clock,
            executor,
        };
        let goal = Goal::Tests {
            module: spec.module.clone(),
            filter: o.filter.clone(),
        };
        let mut out = build_packages(&host, package, &packages, &goal);
        crate::note_built(&out);
        let mut found = report::from_output(&out, None);
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
                snapshots: snapshot_module(&d.file),
            });
            built.push(Built {
                wasm: None,
                cwd: None,
                program: String::new(),
                frames: None,
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
        for mut case in std::mem::take(&mut out.tests) {
            if let Some(d) = doc {
                case.file.clone_from(&d.file);
                case.line = u32::try_from(d.line(case.line as usize)).unwrap_or(u32::MAX);
            }
            selected.push(Selected {
                program: built.len(),
                snapshots: snapshot_module(&case.file),
                case,
                fixed: None,
            });
        }
        // A doc test's file is the source file of its block.
        let shown = match (path, doc) {
            (Some(_), Some(d)) => d.file.clone(),
            (Some(p), None) => p.clone(),
            (None, _) => String::new(),
        };
        let frames = Frames {
            positions: out.positions(&mut positions),
            table: std::sync::OnceLock::new(),
            doc: doc.map(|d| (d.path.clone(), d.file.clone(), d.test.lines.clone())),
            member: member.map(str::to_owned),
        };
        built.push(Built {
            wasm: out.wasm,
            cwd: path.as_ref().map(|_| root.to_path_buf()),
            program: program.sources.display(&shown),
            frames: Some(frames),
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
    let env = RunEnv {
        root,
        table: disk::test_capabilities(root).map_err(|e| report.rep.fail(&e))?,
        flags: &o.caps,
        seed: o.seed,
        update: o.update,
        filter: o.filter.as_deref(),
        sources: &program.sources,
    };
    // `cli.test.builds-executables`: when an integration test or a doc
    // test can start a program, the package's executables and tasks are
    // built first, for the runner's `Process` provider.
    let starts = built.iter().any(|b| {
        b.cwd.is_some()
            && b.wasm.as_deref().is_some_and(|w| {
                hd_run::module_imports(w).is_ok_and(|i| i.iter().any(|(m, _)| m == "hd:Process"))
            })
    });
    let programs = if starts {
        Some(build_programs(
            root,
            &program.package,
            (o.engine, pipeline),
            &mut report.rep,
        )?)
    } else {
        None
    };
    run(&selected, &built, &env, programs.as_ref(), o, report)
}

/// The package's executables and tasks, built for the test runner's
/// `Process` provider (`cli.test.process`, `.process.tasks`). Each gets
/// the package's `[capabilities]` table and no flags, as `hd run NAME`
/// would (`cli.cap.source.hd-run`). They run on the tests' `engine`, so
/// they are built with the tests' `pipeline`.
fn build_programs(
    root: &Path,
    package: &str,
    (engine, pipeline): (crate::engine::EngineKind, crate::Pipeline),
    rep: &mut report::Reporter,
) -> Result<TestPrograms, ExitCode> {
    let mut list = Vec::new();
    for r in disk::executables(root, package)
        .into_iter()
        .chain(disk::tasks(root))
    {
        let mut p = disk::load_package(root, &r.file).map_err(|e| rep.fail(&e))?;
        p.only_program(&r.file);
        // `cli.profile.test`: the executables a test run builds are checked.
        let wasm = crate::compile(&p, (hd_driver::BuildProfile::Checked, pipeline), rep)?;
        let grants = crate::caps::grants(
            &p.capabilities,
            p.package_dir.as_deref().unwrap_or(root),
            &[],
        );
        list.push((r.name, wasm, grants));
    }
    TestPrograms::new(engine, list, root).map_err(|e| rep.fail(&e))
}

/// What a test case's environment comes from (Test Environments): the
/// package directory, its `[test.capabilities]` table and the `--cap`
/// flags (`cli.test.env.grant`), and `--seed` (`cli.test.seed.flag`).
struct RunEnv<'a> {
    root: &'a Path,
    table: Vec<(String, hd_project::Grant)>,
    flags: &'a [CapFlag],
    seed: Option<i64>,
    /// `--update`: snapshot files are recorded.
    update: bool,
    /// `--filter PATTERN` (`cli.test.filter`), which also selects the rows
    /// of an `it_each` case by their names `name[i]`.
    filter: Option<&'a str>,
    /// The package's files, which name a panic frame's file as a case's
    /// file is named.
    sources: &'a disk::DiskSources,
}

/// Whether row `row` of the `it_each` case `name` is selected: its name
/// `name[row]` contains the filter (`cli.test.filter`,
/// `std-testing.it-each.name`).
fn row_selected(name: &str, row: u32, filter: Option<&str>) -> bool {
    filter.is_none_or(|f| format!("{name}[{row}]").contains(f))
}

/// A test case name's slug (`std-testing.snapshot-file.slug`): lowercased,
/// each run of characters other than ASCII letters and digits turned into
/// `-`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if gap {
                out.push('-');
                gap = false;
            }
            out.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    if gap {
        out.push('-');
    }
    out
}

/// The temporary directories of one test program's run
/// (`cli.test.env.temp-dir`): a fresh root of this process below the
/// system's temporary directory, and in it one directory per case, which
/// the host makes on the case's first `temp_dir` call
/// (`std-testing.runner.temp-dir`). No other case, in this run or another,
/// shares one. A case's directory is removed once the case ends, and the
/// root with whatever is left when the run ends
/// (`cli.test.env.temp-dir.removed`).
pub(crate) struct TempDirs(PathBuf);

impl TempDirs {
    pub(crate) fn new() -> Result<TempDirs, String> {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let base = std::env::temp_dir();
        loop {
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = base.join(format!("hd-test-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => {
                    let real = std::fs::canonicalize(&dir).unwrap_or(dir);
                    return Ok(TempDirs(real));
                }
                // A leftover of an earlier process with the same id.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("{}: {e}", dir.display())),
            }
        }
    }

    /// The directory of the case at `index`.
    pub(crate) fn case(&self, index: usize) -> PathBuf {
        self.0.join(index.to_string())
    }

    /// Removes the directory of a case that ended, passed or failed.
    pub(crate) fn end(&self, index: usize) {
        let _ = std::fs::remove_dir_all(self.case(index));
    }
}

impl Drop for TempDirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the cases program by program on up to `jobs` workers, round
/// robin in content order, and prints the results in content order
/// (`cli.test.report.stream`), whatever order the workers finish in. An
/// integration test program or a doc test with a totally denied need stops
/// the command before any case runs (`cli.cap.total.test`).
fn run(
    selected: &[Selected],
    built: &[Built],
    env: &RunEnv<'_>,
    programs: Option<&TestPrograms>,
    options: &Options,
    report: &mut Report,
) -> Result<(), ExitCode> {
    let cases: Vec<TestCase> = selected.iter().map(|s| s.case.clone()).collect();
    let seeds: Vec<Option<i64>> = cases.iter().map(|c| c.base_seed(env.seed)).collect();
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
    // A case's result when it is known without running it.
    let known = |s: &Selected| -> Option<CaseResult> {
        let c = &s.case;
        if let Some(r) = &s.fixed {
            Some(r.clone())
        } else if let Some(what) = &c.unsupported {
            Some(CaseResult::Unsupported { what: what.clone() })
        } else if let Some(reason) = &c.ignore {
            Some(CaseResult::Ignored {
                reason: reason.clone(),
            })
        } else if c.run.is_some() {
            None
        } else {
            Some(CaseResult::Unsupported {
                what: "a case without a checked body".into(),
            })
        }
    };
    // Per program: (case index, (test export, init export)).
    let mut running: Vec<Vec<(usize, (u32, u32))>> = built.iter().map(|_| Vec::new()).collect();
    for (i, s) in selected.iter().enumerate() {
        if let (None, Some(run)) = (known(s), s.case.run) {
            running[s.program].push((i, run));
        }
    }
    for (prog, running) in built.iter().zip(&running) {
        if running.is_empty() || prog.cwd.is_none() {
            continue;
        }
        if let Some(wasm) = &prog.wasm
            && let Some(d) = crate::caps::test_refusal(wasm, &env.table, env.flags, &prog.program)
        {
            report.rep.diag(&d);
            return Err(report.rep.finish(HD_FAILURE));
        }
    }
    for (i, s) in selected.iter().enumerate() {
        if let Some(r) = known(s) {
            report.release(&cases, &seeds, cursor.finish(key(i), r));
        }
    }
    if running.iter().all(Vec::is_empty) {
        return Ok(());
    }
    let (session, prepared) = {
        let _preparing = crate::memory_cap::stage("Precompile");
        let session = TestSession::new(
            options.engine,
            crate::level_of(options.release),
            options.jobs,
            options.limits,
            programs,
        )
        .map_err(|error| report.rep.fail(&error))?;
        let mut prepared = Vec::with_capacity(built.len());
        for (program, running) in built.iter().zip(&running) {
            if running.is_empty() {
                prepared.push(None);
                continue;
            }
            let wasm = program
                .wasm
                .as_deref()
                .ok_or_else(|| report.rep.fail("internal: a test program was not built"))?;
            prepared.push(Some(
                session
                    .prepare(wasm, program.cwd.is_some())
                    .map_err(|error| report.rep.fail(&error))?,
            ));
        }
        (session, prepared)
    };
    let _guest = crate::memory_cap::enter_guest();
    for ((prog, running), prepared) in built.iter().zip(&running).zip(&prepared) {
        if running.is_empty() {
            continue;
        }
        let Some(prepared) = prepared else {
            return Err(report.rep.fail("internal: a test program was not prepared"));
        };
        let temp = TempDirs::new().map_err(|e| report.rep.fail(&e))?;
        let test_env = TestEnv {
            cwd: prog.cwd.as_deref(),
            program: &prog.program,
            programs: programs.filter(|_| prog.cwd.is_some()),
        };
        // An integration test or a doc test gets the test grant; a unit test
        // gets no host provider, so no grant limits it.
        let host_case = |i: usize, (test, init): (u32, u32)| HostCase {
            test,
            init,
            kind: cases[i].kind.clone(),
            temp_dir: temp.case(i),
            seed: seeds[i],
            snapshot: Snapshot {
                folder: format!("__snapshots__/{}", selected[i].snapshots),
                regressions: format!("__regressions__/{}", selected[i].snapshots),
                root: env.root.to_path_buf(),
                slug: slug(&cases[i].name),
                update: env.update,
            },
            grants: if prog.cwd.is_some() {
                crate::caps::test_grants(&env.table, env.root, env.flags, &temp.case(i))
            } else {
                hd_run::Grants::default()
            },
        };
        let workers = options.jobs.min(running.len()).max(1);
        let (tx, rx) = mpsc::channel::<Result<(usize, CaseRun), String>>();
        let mut error = None;
        std::thread::scope(|s| {
            for w in 0..workers {
                let tx = tx.clone();
                let chunk: Vec<(usize, (u32, u32))> =
                    running.iter().skip(w).step_by(workers).copied().collect();
                let host_cases: Vec<HostCase> =
                    chunk.iter().map(|&(i, run)| host_case(i, run)).collect();
                let test_env = &test_env;
                let session = &session;
                let mut builder = std::thread::Builder::new();
                if let Some(size) = options
                    .engine
                    .worker_stack(crate::level_of(options.release))
                {
                    builder = builder.stack_size(size);
                }
                let worker = builder.spawn_scoped(s, move || {
                    let r = session.run(
                        prepared,
                        test_env,
                        &host_cases,
                        options.limits,
                        &mut |c: CaseRun| {
                            if let Some((i, _)) = chunk.iter().find(|(_, (t, _))| *t == c.test) {
                                let _ = tx.send(Ok((*i, c)));
                            }
                        },
                    );
                    if let Err(e) = r {
                        let _ = tx.send(Err(e));
                    }
                });
                if let Err(spawn_error) = worker {
                    error.get_or_insert_with(|| {
                        format!("cannot start a test worker: {spawn_error}")
                    });
                }
            }
            drop(tx);
            for msg in rx {
                match msg {
                    Ok((i, c)) => {
                        if c.last() {
                            temp.end(i);
                        }
                        let mut k = key(i);
                        // Each row of an `it_each` case is a case of its
                        // own, which its first result lists
                        // (`std-testing.it-each`).
                        if let Some((row, rows)) = c.row {
                            let name = &cases[i].name;
                            if row == 0 {
                                let kept: Vec<u32> = (0..rows)
                                    .filter(|r| row_selected(name, *r, env.filter))
                                    .collect();
                                report.release(&cases, &seeds, cursor.rows(&k, &kept));
                            }
                            if rows == 0 || !row_selected(name, row, env.filter) {
                                continue;
                            }
                            k.row = row;
                        }
                        if let Some(input) = &c.input {
                            report.inputs.insert(k.clone(), input.clone());
                        }
                        let frames = match (&prog.frames, &prog.wasm) {
                            (Some(f), Some(wasm)) if c.trapped => {
                                f.lines(wasm, &c.frames, env.sources)
                            }
                            _ => Vec::new(),
                        };
                        let r = judge(&cases[i], &c, frames);
                        report.release(&cases, &seeds, cursor.finish(k, r));
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
fn judge(case: &TestCase, run: &CaseRun, frames: Vec<String>) -> CaseResult {
    if run.trapped {
        let (category, message) = panic_of(&run.stderr);
        if case.expect_panic.as_deref() == Some(category.as_str()) {
            return CaseResult::Passed { us: run.us };
        }
        return CaseResult::Panicked {
            category,
            message,
            debug: debug_lines(&run.stderr),
            frames,
        };
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

/// The `dbg` lines of a trapped case's standard error: everything but the
/// panic report (`cli.dbg.test`).
fn debug_lines(stderr: &str) -> String {
    let mut lines: Vec<&str> = stderr.lines().collect();
    if let Some(report) = lines.iter().rposition(|l| l.starts_with("panic: ")) {
        lines.remove(report);
    }
    lines.join("\n")
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
    /// A failed property case's shrunk input (`std-testing.prop.report`).
    inputs: std::collections::HashMap<CaseKey, String>,
    text: String,
    passed: usize,
    failed: usize,
    ignored: usize,
    unsupported: usize,
}

impl Report {
    fn release(
        &mut self,
        cases: &[TestCase],
        seeds: &[Option<i64>],
        done: Vec<(CaseKey, CaseResult)>,
    ) {
        for (k, r) in done {
            let i = k.registration as usize;
            let Some(c) = cases.get(i) else {
                continue;
            };
            let at = format!("{}:{}", c.file, c.line);
            // An `it_each` row is named `name[i]` (`std-testing.it-each.name`).
            let name = if c.kind == "it_each" {
                format!("{}[{}]", c.name, k.row)
            } else {
                c.name.clone()
            };
            let repro = repro(c, &name, seeds.get(i).copied().flatten());
            let input = self.inputs.remove(&k);
            let mut t = String::new();
            // The outcome and message of the case's test object.
            let (outcome, message) = match r {
                CaseResult::Passed { .. } => {
                    self.passed += 1;
                    ("passed", String::new())
                }
                CaseResult::Failed { message } => {
                    self.failed += 1;
                    let _ = writeln!(t, "FAIL {at}: {name}");
                    for l in message.lines() {
                        let _ = writeln!(t, "    {l}");
                    }
                    if let Some(input) = &input {
                        let _ = writeln!(t, "    input: {input}");
                    }
                    let _ = writeln!(t, "    repro: {repro}");
                    ("failed", message)
                }
                CaseResult::Panicked {
                    category,
                    message,
                    debug,
                    frames,
                } => {
                    self.failed += 1;
                    let panic = if message.is_empty() {
                        format!("panic: {category}")
                    } else {
                        format!("panic: {category}: {message}")
                    };
                    let _ = writeln!(t, "PANIC {at}: {name}\n    {panic}");
                    // `cli.test.report.frame`: the frames follow the panic.
                    for f in &frames {
                        let _ = writeln!(t, "    {f}");
                    }
                    for l in debug.lines() {
                        let _ = writeln!(t, "    {l}");
                    }
                    if let Some(input) = &input {
                        let _ = writeln!(t, "    input: {input}");
                    }
                    let _ = writeln!(t, "    repro: {repro}");
                    ("failed", panic)
                }
                CaseResult::TimedOut => {
                    self.failed += 1;
                    let _ = writeln!(t, "PANIC {at}: {name}\n    panic: time-limit");
                    ("failed", "panic: time-limit".to_owned())
                }
                CaseResult::Ignored { reason } => {
                    self.ignored += 1;
                    let _ = writeln!(t, "IGNORED {at}: {name} ({reason})");
                    ("ignored", reason)
                }
                CaseResult::Unsupported { what } => {
                    self.unsupported += 1;
                    let _ = writeln!(t, "UNSUPPORTED {at}: {name} ({what} cannot run yet)");
                    // JSON has no fourth outcome; as in text, it fails nothing.
                    ("ignored", format!("unsupported: {what} cannot run yet"))
                }
            };
            if self.rep.json {
                let _ = writeln!(
                    self.text,
                    "{{\"kind\":\"test\",\"name\":{},\"outcome\":\"{outcome}\",\"message\":{}}}",
                    report::quote(&name),
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

/// The command that reruns a failed case `name`, a row's `name[i]` for an
/// `it_each` row (`cli.test.report.repro`). A property test case's names
/// its base seed, so it draws the same cases (`cli.test.seed.report`).
fn repro(case: &TestCase, name: &str, seed: Option<i64>) -> String {
    let mut out = format!("hd test {} --filter {}", case.file, shell_quoted(name));
    if let Some(seed) = seed {
        let _ = write!(out, " --seed {seed}");
    }
    out
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
    use super::{TempDirs, TestCase, collapse, parse_with_engine, repro, shell_quoted};

    #[test]
    fn repeated_frames_collapse_into_a_count() {
        let frames = |s: &str| s.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(collapse(&frames("a b c")), frames("a b c"));
        assert_eq!(
            collapse(&frames("p a a a a t")),
            ["p", "a", "... frame repeated 3 more times", "t"]
        );
        assert_eq!(
            collapse(&frames("a a b")),
            ["a", "... frame repeated 1 more time", "b"]
        );
        assert_eq!(
            collapse(&frames("x a b a b a b a t")),
            [
                "x",
                "a",
                "b",
                "... last 2 frames repeated 2 more times",
                "a",
                "t"
            ]
        );
        // The cycle covers more frames than the repeat inside it.
        assert_eq!(
            collapse(&frames("a a b a a b a a b")),
            ["a", "a", "b", "... last 3 frames repeated 2 more times"]
        );
        let deep = vec!["f".to_owned(); 20_000];
        assert_eq!(
            collapse(&deep),
            ["f", "... frame repeated 19999 more times"]
        );
    }

    #[test]
    fn test_engine_pipeline_and_limits_have_independent_selection() {
        use crate::engine::{EngineKind, ProgramLimits};
        let words = |words: &[&str]| {
            words
                .iter()
                .map(|word| std::ffi::OsString::from(*word))
                .collect::<Vec<_>>()
        };
        let default = parse_with_engine(&[], EngineKind::Node).unwrap();
        assert_eq!(default.engine, EngineKind::Node);
        assert!(!default.release);
        assert_eq!(default.limits, ProgramLimits::default());
        #[cfg(feature = "wasmtime")]
        {
            let native = parse_with_engine(
                &words(&[
                    "tests/check.hd",
                    "--engine",
                    "wasmtime",
                    "--release",
                    "--max-heap=2M",
                    "--time-limit",
                    "0ms",
                ]),
                EngineKind::Node,
            )
            .unwrap();
            assert_eq!(native.engine, EngineKind::Wasmtime);
            assert!(native.release);
            assert_eq!(
                native.limits,
                ProgramLimits {
                    heap_bytes: Some(2 * 1024 * 1024),
                    wall_ms: Some(0)
                }
            );
            let explicit = parse_with_engine(
                &words(&["--engine=wasmtime", "--engine=node"]),
                EngineKind::Wasmtime,
            )
            .unwrap();
            assert_eq!(explicit.engine, EngineKind::Node);
        }
        for words in [
            words(&["--engine=unknown"]),
            words(&["--engine"]),
            words(&["--max-heap=bad"]),
            words(&["--time-limit", "5"]),
        ] {
            assert!(parse_with_engine(&words, EngineKind::Node).is_err());
        }
    }

    fn case(name: &str, kind: &str) -> TestCase {
        TestCase {
            module: String::new(),
            file: "tests/props.hd".into(),
            line: 3,
            name: name.into(),
            kind: kind.into(),
            ignore: None,
            expect_panic: None,
            unsupported: None,
            run: None,
        }
    }

    /// `cli.test.seed`, `.seed.flag`, `.seed.report`.
    #[test]
    fn a_property_case_has_a_base_seed_from_its_name() {
        let a = case("sorts any list", "it_prop");
        let seed = a.base_seed(None).expect("a property case");
        assert!(seed >= 0);
        assert_eq!(
            a.base_seed(None),
            Some(seed),
            "the same name, the same seed"
        );
        assert_ne!(
            case("sorts any vec", "it_prop_with").base_seed(None),
            Some(seed)
        );
        assert_eq!(a.base_seed(Some(7)), Some(7));
        assert_eq!(case("adds", "it").base_seed(Some(7)), None);
        assert_eq!(
            repro(&a, &a.name, Some(7)),
            "hd test tests/props.hd --filter \"sorts any list\" --seed 7"
        );
        assert_eq!(
            repro(&case("adds", "it"), "adds", None),
            "hd test tests/props.hd --filter \"adds\""
        );
    }

    /// `cli.test.env.temp-dir`, `.temp-dir.removed`.
    #[test]
    fn each_case_has_its_own_directory_removed_when_it_ends() {
        let dirs = TempDirs::new().expect("a root");
        let other = TempDirs::new().expect("another root");
        assert_ne!(dirs.case(0), dirs.case(1));
        assert_ne!(dirs.case(0), other.case(0));
        std::fs::create_dir_all(dirs.case(0).join("deep")).expect("made");
        std::fs::write(dirs.case(0).join("deep/f"), "x").expect("written");
        std::fs::create_dir_all(dirs.case(1)).expect("made");
        dirs.end(0);
        assert!(!dirs.case(0).exists());
        assert!(dirs.case(1).exists());
        let root = dirs.0.clone();
        drop(dirs);
        assert!(!root.exists());
    }

    #[test]
    fn repro_names_survive_the_shell() {
        assert_eq!(shell_quoted("sums prices"), "\"sums prices\"");
        assert_eq!(
            shell_quoted("costs $5 \"now\""),
            "\"costs \\$5 \\\"now\\\"\""
        );
    }
}
