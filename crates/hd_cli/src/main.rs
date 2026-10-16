//! The `hd` command. It owns everything outside the core (design-overview
//! §2.2): the file system (`DiskSources`, the disk `CacheStore`), the clock,
//! the executor and the engine (`node::NodeEngine`, an `hd_run::Engine`,
//! or wasmtime in-process with `--engine wasmtime`; `engine.rs`).

mod affected;
mod cache_cmd;
mod caps;
mod check_cmd;
mod clean_cmd;
mod dep_cmd;
mod disk;
mod doc_cmd;
mod engine;
mod fix_cmd;
mod fmt_cmd;
mod help;
mod new_cmd;
mod node;
#[cfg(test)]
mod pipeline_tests;
#[cfg(feature = "wasmtime")]
mod repl;
mod report;
mod test_cmd;

use std::ffi::OsString;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_driver::{BuildProfile, Clock, EngineTarget, Executor, Goal, Host, OptLevel, Output};
use hd_run::{HostSetup, Outcome};

use crate::report::Reporter;

const USAGE: &str = "usage:
  hd [--release] [--format json] [--cap NAME=VALUE]... FILE.hd [-- ARGS]
  hd [--cap NAME=VALUE]... FILE.wasm [-- ARGS]
  hd run [--release] [--format json] [--cap NAME=VALUE]... [NAME]
  hd build [--release] [--format json] [FILE.hd]
  hd check [FILE.hd] [--tests | --all] [--format json]
  hd doc [--private] [--out DIR] [--open] [--format json]
  hd new [--app | --lib] [--pages] [--vcs none] [PATH]
  hd clean [--cache]
  hd cache gc
  hd add [--dev] [--name NAME] TARGET[@QUERY]...
  hd update [NAME[@QUERY]]
  hd remove NAME
  hd fetch
  hd fmt [--check] [FILE.hd]
  hd fix [--jobs N]
  hd test [FILE.hd] [--filter PATTERN] [--jobs N] [--update] [--format json]

  FILE.hd, FILE.wasm, hd run and hd test accept --time-limit DURATION
  DURATION: whole ms/s/min/h";

/// `cli.exit.hd-failure`: `hd` itself failed, or rejected its command line.
pub(crate) const HD_FAILURE: u8 = 101;

pub(crate) fn fail(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(HD_FAILURE)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(HD_FAILURE)
}

fn main() -> ExitCode {
    let arguments = format_after_command(std::env::args_os().skip(1).collect());
    let Some(command) = arguments.first().and_then(|c| c.to_str()) else {
        // `cli.repl.open.terminal`: `hd` with no arguments on a terminal
        // opens the REPL. `cli.stdin.program`: without a terminal, it runs
        // standard input as a single-file program.
        if std::io::stdin().is_terminal() {
            return repl_command();
        }
        return run_stdin_command();
    };
    let asks_help = arguments[1..]
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--help");
    if asks_help && let Some(text) = help::command(command) {
        print!("{text}");
        return ExitCode::SUCCESS;
    }
    if compiles(command, &arguments)
        && let Err(error) = cache_cap()
    {
        return fail(&error);
    }
    match (command, &arguments[1..]) {
        ("help" | "--help", []) => {
            print!("{}", help::list());
            ExitCode::SUCCESS
        }
        ("help", [name]) => match help::command(&name.to_string_lossy()) {
            Some(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            None => fail(&format!(
                "`{}` is no command of hd; `hd help` lists them",
                name.to_string_lossy()
            )),
        },
        ("test", rest) => test_cmd::command(rest),
        ("new", rest) => new_cmd::command(rest),
        ("clean", rest) => clean_cmd::command(rest),
        ("cache", rest) => cache_cmd::command(rest),
        ("add", rest) => dep_cmd::add(rest),
        ("update", rest) => dep_cmd::update(rest),
        ("remove", rest) => dep_cmd::remove(rest),
        ("fetch", rest) => dep_cmd::fetch(rest),
        ("fmt", rest) => fmt_cmd::command(rest),
        ("fix", rest) => fix_cmd::command(rest),
        ("run", rest) => run_command(rest),
        ("build", rest) => build_command(rest),
        ("check", rest) => check_cmd::command(rest),
        ("doc", rest) => doc_cmd::command(rest),
        _ if arguments
            .iter()
            .take_while(|a| *a != "--")
            .any(|a| has_extension(a, "wasm")) =>
        {
            run_wasm_command(&arguments)
        }
        _ if arguments
            .iter()
            .take_while(|a| *a != "--")
            .any(|a| has_extension(a, "hd")) =>
        {
            run_file_command(&arguments)
        }
        _ => usage(),
    }
}

/// `hd` on a terminal (`cli.repl.open.terminal`): the REPL, which runs on
/// wasmtime only.
#[cfg(feature = "wasmtime")]
fn repl_command() -> ExitCode {
    repl::command()
}

/// `hd` on a terminal, in a build without wasmtime, which the REPL needs.
#[cfg(not(feature = "wasmtime"))]
fn repl_command() -> ExitCode {
    fail(
        "the REPL runs on wasmtime, and this build of hd has none; rebuild with `--features wasmtime`",
    )
}

/// Follow command dispatch: a source argument to `fmt` is not a program run.
fn compiles(command: &str, arguments: &[OsString]) -> bool {
    match command {
        "run" | "build" | "check" | "test" | "doc" | "fix" => true,
        "help" | "--help" | "new" | "clean" | "add" | "update" | "remove" | "fetch" | "fmt"
        | "cache" => false,
        _ => arguments
            .iter()
            .take_while(|a| *a != "--")
            .any(|a| has_extension(a, "hd") || has_extension(a, "wasm")),
    }
}

/// `--format` is a global flag (`hd help`): written before a command's
/// name, as in `hd --format json check FILE`, it moves after the name,
/// where the command reads it. Before a FILE it stays, since `hd FILE`
/// reads it there.
fn format_after_command(mut args: Vec<OsString>) -> Vec<OsString> {
    let Some(Ok((_, used))) = report::format_flag(&args, 0) else {
        return args;
    };
    let command = args.get(used).and_then(|c| c.to_str());
    if command.is_some_and(|c| {
        matches!(
            c,
            "build"
                | "run"
                | "test"
                | "check"
                | "doc"
                | "new"
                | "add"
                | "update"
                | "remove"
                | "fetch"
                | "clean"
                | "fmt"
                | "fix"
                | "cache"
        )
    }) {
        let flag: Vec<OsString> = args.drain(..used).collect();
        args.splice(1..1, flag);
    }
    args
}

fn has_extension(word: &OsString, ext: &str) -> bool {
    Path::new(word)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

pub(crate) struct Wall(pub(crate) Instant);

impl Clock for Wall {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// The cache directory (`cli.cache.directory`): `HD_CACHE`, or the
/// platform's user cache directory followed by `hd`.
pub(crate) fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("HD_CACHE") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Caches")
    } else {
        std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from)
    };
    base.join("hd")
}

/// The compiled-cache directory (`cli.cache.obj`): `obj` in the cache
/// directory.
pub(crate) fn cache_dir() -> PathBuf {
    cache_root().join("obj")
}

/// The compiled entries' size cap (`cli.cache.obj.cap`): `HD_CACHE_MAX_SIZE`
/// as a SIZE, else 10 GiB.
pub(crate) fn cache_cap() -> Result<u64, String> {
    match std::env::var_os("HD_CACHE_MAX_SIZE") {
        None => Ok(hd_cache::disk::DEFAULT_CAP),
        Some(value) => match value.to_str().map(engine::parse_size) {
            Some(Ok(bytes)) => Ok(bytes),
            Some(Err(error)) => Err(format!("HD_CACHE_MAX_SIZE: {error}")),
            None => Err("HD_CACHE_MAX_SIZE is not a valid SIZE".to_owned()),
        },
    }
}

/// The store of the compiled entries under their cap. Dropping it records
/// what it wrote and runs the bounded eviction (`cli.cache.obj.evict`).
/// A command that compiles rejects an invalid cap before it gets here.
pub(crate) fn disk_store() -> DiskStore {
    DiskStore::new(cache_dir())
        .with_cap(cache_cap().unwrap_or(hd_cache::disk::DEFAULT_CAP))
        .on_write_failure(warn_cache_write)
}

/// Warns once per command that the compiled cache cannot be written; the
/// command continues uncached (`crates/hd_cache/DESIGN.md` §5.4).
fn warn_cache_write(root: &Path, error: &std::io::Error) {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        eprintln!(
            "warning: cannot write the compiled cache in {}: {error}; continuing without it",
            root.display()
        );
    });
}

/// The thread count (`cli.jobs.env`, `cli.jobs.default`): `HD_JOBS`, else
/// the number of cores, at most 8.
pub(crate) fn default_jobs() -> usize {
    std::env::var("HD_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(1, std::num::NonZero::get)
                .min(8)
        })
}

pub(crate) fn executor() -> Executor {
    executor_of(default_jobs())
}

/// The executor of `threads` threads: a pool, or the serial one for 1.
pub(crate) fn executor_of(threads: usize) -> Executor {
    if threads > 1 {
        Executor::Pool(threads)
    } else {
        Executor::Serial(hd_sched::SerialOrder::Priority)
    }
}

/// The build profile `--release` selects (`cli.profile.default`,
/// `cli.profile.release`, `cli.profile.release.file`).
fn profile_of(release: bool) -> BuildProfile {
    if release {
        BuildProfile::Release
    } else {
        BuildProfile::Checked
    }
}

/// Builds `goal` in `profile` over a program's sources and reports its
/// manifest's and the compiler's diagnostics. Warnings are shown; only
/// errors stop the command, which then ends with status 101.
pub(crate) fn build_goal(
    program: &disk::Program,
    goal: &Goal,
    profile: BuildProfile,
    rep: &mut Reporter,
) -> Result<Output, ExitCode> {
    build_goal_with(program, goal, profile, false, rep)
}

/// [`build_goal`], with `doc_scopes` asking for each module's scope in the
/// output (`hd doc` resolves its links in them). Such a build emits no
/// module that runs, so it keeps the program's default pipeline.
pub(crate) fn build_goal_with(
    program: &disk::Program,
    goal: &Goal,
    profile: BuildProfile,
    doc_scopes: bool,
    rep: &mut Reporter,
) -> Result<Output, ExitCode> {
    let packages = program.packages();
    let pipeline = Pipeline {
        target: packages.target,
        level: packages.level,
    };
    build_goal_in(program, goal, (profile, pipeline), doc_scopes, rep)
}

/// The optimization level `--release` selects (`cli.profile.pipeline.one`).
pub(crate) const fn level_of(release: bool) -> OptLevel {
    if release {
        OptLevel::Optimized
    } else {
        OptLevel::Dev
    }
}

/// The pipeline a build runs (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5): the engine target it is
/// tuned for and the optimization level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pipeline {
    pub(crate) target: EngineTarget,
    pub(crate) level: OptLevel,
}

impl Pipeline {
    /// The pipeline of a build that `engine` runs: tuned for its target,
    /// and optimized with `--release` (`cli.profile.pipeline.one`).
    pub(crate) const fn of(engine: engine::EngineKind, release: bool) -> Self {
        Self {
            target: engine.target(),
            level: level_of(release),
        }
    }
}

#[cfg(test)]
thread_local! {
    /// The pipeline of each build this thread ran, for the tests that
    /// check what each command builds with (`pipeline_tests.rs`).
    static BUILT: std::cell::RefCell<Vec<Pipeline>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Notes the pipeline that built `out`, as the driver reports it.
#[cfg(test)]
pub(crate) fn note_built(out: &Output) {
    BUILT.with_borrow_mut(|built| {
        built.push(Pipeline {
            target: out.target,
            level: out.level,
        });
    });
}

#[cfg(not(test))]
pub(crate) const fn note_built(_: &Output) {}

/// The pipelines of the builds this thread ran since the last call.
#[cfg(test)]
pub(crate) fn take_built() -> Vec<Pipeline> {
    BUILT.take()
}

/// [`build_goal_with`], with the pipeline chosen apart from the profile:
/// `hd test --release` builds checked and runs the optimized pipeline
/// (`cli.profile.pipeline.one`), and every build that runs is tuned for
/// the engine that runs it.
pub(crate) fn build_goal_in(
    program: &disk::Program,
    goal: &Goal,
    (profile, pipeline): (BuildProfile, Pipeline),
    doc_scopes: bool,
    rep: &mut Reporter,
) -> Result<Output, ExitCode> {
    if rep.diags(&program.problems) {
        return Err(rep.finish(HD_FAILURE));
    }
    // Print, then publish (`crates/hd_cache/DESIGN.md` §5.4): the entries
    // reach the disk when the store drops, after the diagnostics.
    let store = hd_cache::DeferredStore::new(disk_store());
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store: &store,
        clock: &clock,
        executor: executor(),
    };
    let packages = hd_driver::Packages {
        build_profile: profile,
        target: pipeline.target,
        level: pipeline.level,
        doc_scopes,
        ..program.packages()
    };
    let out: Output = hd_driver::build_packages(&host, &program.package, &packages, goal);
    note_built(&out);
    let as_written = program.as_written.as_deref().filter(|_| rep.json);
    let mut diags = report::from_output(&out, as_written);
    report::relocate(&mut diags, |f| program.sources.display(f));
    if rep.diags(&diags) {
        return Err(rep.finish(HD_FAILURE));
    }
    Ok(out)
}

/// [`compile`] for `hd build`: the module with its frames' positions
/// resolved and written in, so it prints a panic's frames without the
/// compiler's cache (`crates/hd_mono/DESIGN.md` "Positions").
fn compile_standalone(
    program: &disk::Program,
    (profile, pipeline): (BuildProfile, Pipeline),
    rep: &mut Reporter,
) -> Result<Vec<u8>, ExitCode> {
    let built = compile_traced(program, (profile, pipeline), rep)?;
    let mut wasm = built.wasm;
    if let Some((frames, sources)) = &built.frames {
        let section = frames.positions_section(&wasm, sources);
        wasm.extend(section);
    }
    Ok(wasm)
}

/// Compiles a program to Wasm bytes in `profile` through `pipeline`,
/// reporting diagnostics.
fn compile(
    program: &disk::Program,
    (profile, pipeline): (BuildProfile, Pipeline),
    rep: &mut Reporter,
) -> Result<Vec<u8>, ExitCode> {
    compile_traced(program, (profile, pipeline), rep).map(|c| c.wasm)
}

/// A compiled program and what names its panic's frames
/// (`flow.panic.report`): none for a module that was built earlier.
struct Compiled<'a> {
    wasm: Vec<u8>,
    frames: Option<(test_cmd::Frames, &'a disk::DiskSources)>,
}

/// [`compile`], keeping what `hd FILE` and `hd run` need to report a
/// panic's source location.
fn compile_traced<'a>(
    program: &'a disk::Program,
    (profile, pipeline): (BuildProfile, Pipeline),
    rep: &mut Reporter,
) -> Result<Compiled<'a>, ExitCode> {
    let goal = Goal::Program {
        entry: program.entry.clone(),
    };
    let out = build_goal_in(program, &goal, (profile, pipeline), false, rep)?;
    let positions = out.positions(&mut hd_driver::PositionPool::default());
    match out.wasm {
        Some(wasm) => Ok(Compiled {
            wasm,
            frames: Some((test_cmd::Frames::new(positions), &program.sources)),
        }),
        None => Err(rep.fail("no Wasm produced")),
    }
}

/// Runs compiled Wasm and ends with the program's status
/// (`cli.exit.program`), unless a need is totally denied
/// (`cli.cap.total.refuse`). `program` is what its `Args.program` returns;
/// `engine` runs it, with the engine options of `level`.
fn execute(
    built: &Compiled<'_>,
    program: &str,
    table: &[(String, hd_project::Grant)],
    table_dir: Option<&Path>,
    flags: &[caps::CapFlag],
    args: &[OsString],
    (engine, level, limits): (engine::EngineKind, OptLevel, engine::ProgramLimits),
    rep: &mut Reporter,
) -> ExitCode {
    let wasm = &built.wasm;
    if let Some(d) = caps::refusal(wasm, table, flags) {
        rep.diag(&d);
        return rep.finish(HD_FAILURE);
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let host = HostSetup {
        providers: Vec::new(),
        grants: caps::grants(table, table_dir.unwrap_or(&cwd), flags),
        limits: limits.into(),
        args: args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        program: program.to_owned(),
    };
    match engine::run(engine, level, wasm, &host) {
        Ok((Outcome::Exit(code), trap)) => {
            // A panic's location follows the host's `panic:` line, as a
            // test's report does (`flow.panic.report`).
            if let Some((frames, sources)) = &built.frames {
                for line in frames.lines(wasm, &trap, sources) {
                    eprintln!("    {line}");
                }
            } else if !trap.is_empty()
                && let Ok(table) = hd_run::frames::LineTable::parse(wasm)
            {
                // A module `hd build` wrote carries its positions.
                for line in test_cmd::collapse(&table.resolved_lines(&trap)) {
                    eprintln!("    {line}");
                }
            }
            rep.finish(code)
        }
        Ok((Outcome::Panic(p), _)) => {
            eprintln!("panic: {}: {}", p.category, p.message);
            rep.finish(HD_FAILURE)
        }
        Ok((Outcome::Internal(e), _)) => rep.fail(&e),
        Err(e) => rep.fail(&e),
    }
}

/// The words of `hd FILE` and `hd FILE.wasm`: flags before `--`, one
/// FILE, and the program arguments after `--` (`cli.args.separator`).
struct FileWords {
    json: bool,
    caps: Vec<caps::CapFlag>,
    file: Option<PathBuf>,
    /// The program's arguments (`cli.args.pass`).
    program_args: Vec<OsString>,
    /// `--engine NAME`, else `HD_ENGINE`, else Node.
    engine: engine::EngineKind,
    release: bool,
    limits: engine::ProgramLimits,
}

fn file_words(args: &[OsString]) -> Result<FileWords, String> {
    let mut w = FileWords {
        json: false,
        caps: Vec::new(),
        file: None,
        program_args: Vec::new(),
        engine: engine::EngineKind::default(),
        release: false,
        limits: engine::ProgramLimits::default(),
    };
    let mut engine_flag = false;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            w.program_args = args[i + 1..].to_vec();
            break;
        }
        if let Some(format) = report::format_flag(args, i) {
            let (json, used) = format?;
            w.json = json;
            i += used;
            continue;
        }
        if let Some(cap) = caps::cap_flag(args, i) {
            let (flag, used) = cap?;
            w.caps.push(flag);
            i += used;
            continue;
        }
        if let Some(e) = engine::engine_flag(args, i) {
            let (kind, used) = e?;
            w.engine = kind;
            engine_flag = true;
            i += used;
            continue;
        }
        if let Some(limit) = engine::time_limit_flag(args, i) {
            let (ms, used) = limit?;
            w.limits.wall_ms = Some(ms);
            i += used;
            continue;
        }
        // `cli.profile.release.file`: the release profile, and wasmtime's
        // optimized pipeline (`cli.profile.pipeline.release`).
        if args[i] == "--release" {
            w.release = true;
            i += 1;
            continue;
        }
        if w.file.replace(PathBuf::from(&args[i])).is_some() {
            return Err(
                "`hd FILE` takes one FILE; put the program's arguments after `--`".to_owned(),
            );
        }
        i += 1;
    }
    // The flag wins over `HD_ENGINE`: read the variable only when no
    // flag is given (`cli.engine.select`).
    if !engine_flag {
        w.engine = engine::from_env()?;
    }
    Ok(w)
}

/// `hd [--release] [--format json] [--cap NAME=VALUE]... FILE.hd [-- ARGS]`
/// (`cli.file.run`): FILE as a single-file program.
fn run_file_command(args: &[OsString]) -> ExitCode {
    let w = match file_words(args) {
        Ok(w) => w,
        Err(error) => return fail(&error),
    };
    let mut rep = Reporter::stderr(w.json);
    let Some(file) = &w.file else {
        return usage();
    };
    let program = match disk::load_file(file) {
        Ok(p) => p,
        Err(e) => return rep.fail(&e),
    };
    let pipeline = Pipeline::of(w.engine, w.release);
    match compile_traced(&program, (profile_of(w.release), pipeline), &mut rep) {
        Ok(built) => execute(
            &built,
            &file.to_string_lossy(),
            &program.capabilities,
            program.package_dir.as_deref(),
            &w.caps,
            &w.program_args,
            (w.engine, pipeline.level, w.limits),
            &mut rep,
        ),
        Err(code) => code,
    }
}

/// `hd` with no arguments and piped standard input (`cli.stdin.program`):
/// standard input as a single-file program, as `hd FILE` runs FILE. The
/// engine comes from `HD_ENGINE`, as without `--engine`; no other flags
/// exist to give.
fn run_stdin_command() -> ExitCode {
    let mut text = String::new();
    if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
        return fail(&format!("standard input: {e}"));
    }
    let engine = match engine::from_env() {
        Ok(engine) => engine,
        Err(error) => return fail(&error),
    };
    let mut rep = Reporter::stderr(false);
    let program = match disk::load_stdin(&text) {
        Ok(p) => p,
        Err(e) => return rep.fail(&e),
    };
    let pipeline = Pipeline::of(engine, false);
    match compile_traced(&program, (BuildProfile::Checked, pipeline), &mut rep) {
        Ok(built) => execute(
            &built,
            "main.hd",
            &program.capabilities,
            program.package_dir.as_deref(),
            &[],
            &[],
            (engine, pipeline.level, engine::ProgramLimits::default()),
            &mut rep,
        ),
        Err(code) => code,
    }
}

/// `hd [--cap NAME=VALUE]... FILE.wasm [-- ARGS]` (Prebuilt Modules,
/// `cli.wasm.*`): a module that `hd build` wrote. It is checked first: a
/// valid module (`cli.wasm.invalid`), with the entry exports and only
/// imports `hd` provides (`cli.wasm.not-hd.*`). Its grant comes from the
/// flags alone (`cli.wasm.grant`).
fn run_wasm_command(args: &[OsString]) -> ExitCode {
    let w = match file_words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stderr(w.json);
    let Some(file) = &w.file else {
        return usage();
    };
    let shown = file.display();
    let wasm = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => return rep.fail(&format!("{shown}: {e}")),
    };
    let checked = hd_run::module_imports(&wasm).and_then(|imports| {
        let exports = hd_run::module_exports(&wasm)?;
        Ok((imports, exports))
    });
    let (imports, exports) = match checked {
        Ok(ie) => ie,
        Err(e) => return rep.fail(&format!("`{shown}` is not a valid Wasm module: {e}")),
    };
    let rebuild = "it was not written by this `hd build`; rebuild it with `hd build`";
    if let Some(missing) = ["hd.init", "hd.poll"]
        .into_iter()
        .find(|e| !exports.iter().any(|x| x == e))
    {
        return rep.fail(&format!(
            "`{shown}` has no entry export `{missing}`: {rebuild}"
        ));
    }
    if let Some((m, n)) = hd_run::unknown_import(&imports) {
        return rep.fail(&format!(
            "`{shown}` imports `{m}` `{n}`, which `hd` does not provide: {rebuild}"
        ));
    }
    let built = Compiled { wasm, frames: None };
    execute(
        &built,
        &file.to_string_lossy(),
        &[],
        None,
        &w.caps,
        &w.program_args,
        (w.engine, level_of(w.release), w.limits),
        &mut rep,
    )
}

/// The words and flags of `hd run` and `hd build`.
struct Words {
    release: bool,
    json: bool,
    /// `--cap` flags; only `hd run` takes them (`cli.cap.flag.commands`).
    caps: Vec<caps::CapFlag>,
    positional: Vec<OsString>,
    /// The words after `--`; only `hd run` takes them (`cli.args.separator`).
    program_args: Vec<OsString>,
    /// The members `-p NAME` selects (`cli.workspace.select.anywhere`).
    packages: Vec<String>,
    /// `--engine NAME`, else `HD_ENGINE`, else Node: the engine `hd run`
    /// runs on, and the one whose target `hd run` and `hd build` tune for.
    engine: engine::EngineKind,
    limits: engine::ProgramLimits,
}

fn words(command: &str, args: &[OsString]) -> Result<Words, String> {
    let mut w = Words {
        release: false,
        json: false,
        caps: Vec::new(),
        positional: Vec::new(),
        program_args: Vec::new(),
        packages: Vec::new(),
        engine: engine::EngineKind::default(),
        limits: engine::ProgramLimits::default(),
    };
    let mut engine_flag = false;
    let mut i = 0;
    while i < args.len() {
        if command == "run" && args[i] == "--" {
            w.program_args = args[i + 1..].to_vec();
            break;
        }
        if let Some(format) = report::format_flag(args, i) {
            let (json, used) = format?;
            w.json = json;
            i += used;
            continue;
        }
        if let Some(p) = disk::package_flag(args, i) {
            let (name, used) = p?;
            w.packages.push(name);
            i += used;
            continue;
        }
        if let Some(e) = engine::engine_flag(args, i) {
            let (kind, used) = e?;
            w.engine = kind;
            engine_flag = true;
            i += used;
            continue;
        }
        if command == "run"
            && let Some(cap) = caps::cap_flag(args, i)
        {
            let (flag, used) = cap?;
            w.caps.push(flag);
            i += used;
            continue;
        }
        if command == "run"
            && let Some(limit) = engine::time_limit_flag(args, i)
        {
            let (ms, used) = limit?;
            w.limits.wall_ms = Some(ms);
            i += used;
            continue;
        }
        let text = args[i].to_string_lossy();
        if text == "--release" {
            w.release = true;
        } else if text.starts_with('-') {
            return Err(format!("`hd {command}` has no option `{text}`"));
        } else {
            w.positional.push(args[i].clone());
        }
        i += 1;
    }
    // The flag wins over `HD_ENGINE`: read the variable only when no
    // flag is given (`cli.engine.select`).
    if !engine_flag {
        w.engine = engine::from_env()?;
    }
    Ok(w)
}

/// The message of every command that needs a package and finds none
/// (`cli.run.package-only`).
fn no_package(command: &str) -> String {
    format!(
        "`hd {command}` works on a package, and no `hd.toml` is at or above here; create a package with `hd new`"
    )
}

/// The package directory of the working directory.
pub(crate) fn package_of_cwd(command: &str) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    disk::package_root(&cwd).ok_or_else(|| no_package(command))
}

/// Whether a word names a file or a directory, not a NAME.
fn is_path(word: &str) -> bool {
    Path::new(word)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("hd") || e.eq_ignore_ascii_case("wasm"))
        || word.contains('/')
        || word.contains(std::path::MAIN_SEPARATOR)
}

/// `hd run [NAME]` (`cli.run.*`).
fn run_command(args: &[OsString]) -> ExitCode {
    match std::env::current_dir() {
        Ok(cwd) => run_in(&cwd, args),
        Err(e) => fail(&e.to_string()),
    }
}

/// [`run_command`] from the working directory `cwd`.
fn run_in(cwd: &Path, args: &[OsString]) -> ExitCode {
    let w = match words("run", args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stderr(w.json);
    let name = match w.positional.as_slice() {
        [] => None,
        [one] => Some(one.to_string_lossy().into_owned()),
        _ => return rep.fail("`hd run` takes at most one NAME"),
    };
    if let Some(n) = &name
        && is_path(n)
    {
        return rep.fail(&if Path::new(n).is_dir() {
            format!(
                "`{n}` is a directory, neither a NAME nor a FILE; select a package member with `-p NAME`"
            )
        } else {
            format!(
                "`hd run` takes no FILE (`{n}`); use `hd run` for the package's executable or `hd run NAME` for a named executable or task, and `hd FILE` to run one file"
            )
        });
    }
    let root = match workspace_member(cwd, name.as_deref(), &w.packages) {
        Some(Ok(r)) => r,
        Some(Err(e)) => return rep.fail(&e),
        None => match disk::package_root(cwd) {
            Some(r) => r,
            None => return rep.fail(&no_package("run")),
        },
    };
    let package = match disk::package_name(&root) {
        Ok(p) => p,
        Err(e) => return rep.fail(&e),
    };
    let executables = disk::executables(&root, &package);
    let tasks = disk::tasks(&root);
    if let Some(t) = tasks
        .iter()
        .find(|t| executables.iter().any(|e| e.name == t.name))
    {
        return rep.fail(&format!(
            "the task `{}` and an executable have the same name",
            t.name
        ));
    }
    let chosen = match name {
        None => match executables.as_slice() {
            [one] => one,
            [] => {
                return rep.fail(&format!(
                    "package `{package}` has no executable to run; `hd run NAME` runs a task"
                ));
            }
            _ => {
                return rep.fail(&format!(
                    "package `{package}` has several executables; run one with `hd run NAME`"
                ));
            }
        },
        Some(n) => match executables.iter().chain(&tasks).find(|r| r.name == n) {
            Some(r) => r,
            None => {
                return rep.fail(&format!(
                    "package `{package}` has no executable or task named `{n}`"
                ));
            }
        },
    };
    let mut program = match disk::load_package(&root, &chosen.file) {
        Ok(p) => p,
        Err(e) => return rep.fail(&e),
    };
    program.only_program(&chosen.file);
    let pipeline = Pipeline::of(w.engine, w.release);
    let built = match compile_traced(&program, (profile_of(w.release), pipeline), &mut rep) {
        Ok(built) => built,
        Err(code) => return code,
    };
    if chosen.is_task
        && let Err(e) = std::env::set_current_dir(&root)
    {
        return rep.fail(&format!("{}: {e}", root.display()));
    }
    execute(
        &built,
        &chosen.name,
        &program.capabilities,
        program.package_dir.as_deref(),
        &w.caps,
        &w.program_args,
        (w.engine, pipeline.level, w.limits),
        &mut rep,
    )
}

/// In workspace mode, or with `-p NAME` flags, the member whose executable
/// or task `hd run NAME` runs (`cli.workspace.run-name`, `.run-ambiguous`,
/// `.run-missing`, `.run-bare`, `cli.workspace.select.*`): among the
/// selected members when flags select some. One selected member runs as a
/// package does. `None` in plain package mode. `cwd` is the working
/// directory.
fn workspace_member(
    cwd: &Path,
    name: Option<&str>,
    packages: &[String],
) -> Option<Result<PathBuf, String>> {
    let members: Vec<(PathBuf, String)> = if packages.is_empty() {
        let (ws, members) = disk::workspace_root(cwd)?;
        members
            .into_iter()
            .map(|dir| {
                let rel = dir
                    .strip_prefix(&ws)
                    .unwrap_or(&dir)
                    .to_string_lossy()
                    .replace('\\', "/");
                (dir, rel)
            })
            .collect()
    } else {
        match disk::select_members(cwd, packages) {
            Ok(m) => m,
            Err(e) => return Some(Err(e)),
        }
    };
    if let [(one, _)] = members.as_slice()
        && !packages.is_empty()
    {
        return Some(Ok(one.clone()));
    }
    let rel = |dir: &Path| {
        members
            .iter()
            .find(|(d, _)| d == dir)
            .map_or_else(String::new, |(_, r)| r.clone())
    };
    let members: Vec<PathBuf> = members.iter().map(|(d, _)| d.clone()).collect();
    let runnables = |dir: &Path| -> Vec<String> {
        let package = disk::package_name(dir).unwrap_or_default();
        disk::executables(dir, &package)
            .into_iter()
            .chain(disk::tasks(dir))
            .map(|r| r.name)
            .collect()
    };
    let Some(name) = name else {
        let lists: Vec<String> = members
            .iter()
            .map(|dir| format!("{}: {}", rel(dir), runnables(dir).join(", ")))
            .collect();
        return Some(Err(format!(
            "this is a workspace; run one member's executable or task with `hd run NAME`: {}",
            lists.join("; ")
        )));
    };
    let having: Vec<&PathBuf> = members
        .iter()
        .filter(|dir| runnables(dir).iter().any(|n| n == name))
        .collect();
    Some(match having.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!(
            "no member of this workspace has an executable or task named `{name}`"
        )),
        several => Err(format!(
            "several members have an executable or task named `{name}`: {}; run it from inside one",
            several
                .iter()
                .map(|d| rel(d))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    })
}

/// Writes a built module to `build/PROFILE/[files/]NAME.wasm`
/// (`cli.build.output`, `cli.build.output.file`).
fn write_module(
    root: &Path,
    release: bool,
    files: bool,
    stem: &str,
    wasm: &[u8],
) -> Result<(), String> {
    let mut dir = root
        .join("build")
        .join(if release { "release" } else { "debug" });
    if files {
        dir.push("files");
    }
    let out = dir.join(format!("{stem}.wasm"));
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&out, wasm))
        .map_err(|e| format!("{}: {e}", out.display()))
}

/// `hd build [FILE]` (`cli.build.*`). Every module is tuned for the engine
/// `hd run` would choose (`--engine`, else `HD_ENGINE`, else Node), so
/// `hd FILE.wasm` in the same environment runs it on that engine.
fn build_command(args: &[OsString]) -> ExitCode {
    let w = match words("build", args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(w.json);
    let pipeline = Pipeline::of(w.engine, w.release);
    let built = match w.positional.as_slice() {
        [] if !w.packages.is_empty() => {
            match std::env::current_dir()
                .map_err(|e| e.to_string())
                .and_then(|cwd| disk::select_members(&cwd, &w.packages))
            {
                Ok(members) => members
                    .iter()
                    .try_for_each(|(root, _)| build_package(root, (w.release, pipeline), &mut rep)),
                Err(e) => Err(rep.fail(&e)),
            }
        }
        [] => match std::env::current_dir()
            .ok()
            .and_then(|cwd| disk::workspace_root(&cwd))
        {
            // `cli.workspace.members`: each member in turn.
            Some((_, members)) => members
                .iter()
                .try_for_each(|root| build_package(root, (w.release, pipeline), &mut rep)),
            None => match package_of_cwd("build") {
                Ok(root) => build_package(&root, (w.release, pipeline), &mut rep),
                Err(e) => Err(rep.fail(&e)),
            },
        },
        [file] => build_file(Path::new(file), (w.release, pipeline), &mut rep),
        _ => Err(rep.fail("`hd build` takes at most one FILE")),
    };
    match built {
        Ok(()) => rep.finish(0),
        Err(code) => code,
    }
}

/// A whole-package `hd build`: each executable to its module; a package with
/// no executable is checked and writes no `.wasm` file.
fn build_package(
    root: &Path,
    (release, pipeline): (bool, Pipeline),
    rep: &mut Reporter,
) -> Result<(), ExitCode> {
    let package = disk::package_name(root).map_err(|e| rep.fail(&e))?;
    let executables = disk::executables(root, &package);
    if executables.is_empty() {
        let mut program = disk::load_package(root, "main").map_err(|e| rep.fail(&e))?;
        program.only_program("");
        return build_goal(&program, &Goal::Analyze, BuildProfile::Checked, rep).map(|_| ());
    }
    for exe in &executables {
        let mut program = disk::load_package(root, &exe.file).map_err(|e| rep.fail(&e))?;
        program.only_program(&exe.file);
        let wasm = compile_standalone(&program, (profile_of(release), pipeline), rep)?;
        write_module(root, release, false, &exe.name, &wasm).map_err(|e| rep.fail(&e))?;
    }
    Ok(())
}

/// `hd build FILE`: the module of a file of the package, linked with the
/// rest of it. Its package is the one at or above FILE's directory
/// (`cli.mode.start`).
fn build_file(
    file: &Path,
    (release, pipeline): (bool, Pipeline),
    rep: &mut Reporter,
) -> Result<(), ExitCode> {
    if file.extension().is_none_or(|x| x != "hd") {
        return Err(rep.fail(&format!(
            "`{}` is not an .hd file; `hd build` takes a FILE.hd",
            file.display()
        )));
    }
    let full =
        hd_run::canonicalize(file).map_err(|e| rep.fail(&format!("{}: {e}", file.display())))?;
    let dir = full.parent().unwrap_or(Path::new("."));
    let Some(root) = disk::package_root(dir) else {
        return Err(rep.fail(&no_package("build")));
    };
    let Ok(rel) = full.strip_prefix(&root) else {
        return Err(rep.fail("FILE is outside its package"));
    };
    let rel = disk::listed_path(&root, &rel.to_string_lossy().replace('\\', "/"));
    let mut program = disk::load_package(&root, &rel).map_err(|e| rep.fail(&e))?;
    program.only_program(&rel);
    let wasm = compile_standalone(&program, (profile_of(release), pipeline), rep)?;
    let stem = full
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    write_module(&root, release, true, &stem, &wasm).map_err(|e| rep.fail(&e))
}

#[cfg(test)]
mod limit_tests {
    use super::{compiles, format_after_command};
    use std::ffi::OsString;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn compiling_follows_dispatch_not_incidental_source_arguments() {
        for command in ["run", "build", "check", "test", "fix"] {
            assert!(compiles(command, &args(&[command])));
        }
        for command in [
            "help", "--help", "new", "clean", "add", "update", "remove", "fetch", "fmt",
        ] {
            assert!(!compiles(command, &args(&[command, "main.hd"])));
        }
        assert!(compiles("--release", &args(&["--release", "main.hd"])));
        assert!(compiles("main.wasm", &args(&["main.wasm"])));
        assert!(!compiles("unknown", &args(&["unknown", "--", "main.hd"])));
    }

    #[test]
    fn a_leading_format_flag_moves_after_the_command() {
        assert_eq!(
            format_after_command(args(&["--format", "json", "check", "main.hd"])),
            args(&["check", "--format", "json", "main.hd"])
        );
        assert_eq!(
            format_after_command(args(&["--format=json", "test"])),
            args(&["test", "--format=json"])
        );
        let file = args(&["--format", "json", "main.hd"]);
        assert_eq!(format_after_command(file.clone()), file);
    }
}
