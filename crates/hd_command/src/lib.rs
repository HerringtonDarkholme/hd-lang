//! The `hd` command. It owns everything outside the core (design-overview
//! §2.2): the file system (`DiskSources`, the disk `CacheStore`), the clock,
//! the executor and the engine (`node::NodeEngine`, an `hd_run::Engine`,
//! or wasmtime in-process with `--engine wasmtime`; `engine.rs`).
//!
//! It is a library with one entry, [`main`], because two binaries run it:
//! this crate's own `hd` (`main.rs`), built without Wasmtime by default,
//! and the `hd-lang` crate's, which `cargo install hd-lang` builds with the
//! `dist` feature (`crates/README.md`, "Publishing").

mod affected;
mod cache_cmd;
mod caps;
mod check_cmd;
mod clean_cmd;
mod dep_cmd;
mod disk;
mod doc_cmd;
mod engine;
mod fix;
mod fmt_cmd;
mod help;
mod install_cmd;
mod lint_cmd;
mod new_cmd;
mod node;
#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod jobs_tests {
    /// `cli.jobs.default`: the cores less the load, rounded, in 1..=8.
    #[test]
    fn the_default_pool_takes_the_free_cores() {
        let free = |cores, load| super::free_cores(cores, load);
        assert_eq!(free(14, Some(0.0)), 8);
        assert_eq!(free(14, Some(10.4)), 4);
        assert_eq!(free(14, Some(10.5)), 3);
        assert_eq!(free(14, Some(40.0)), 1);
        assert_eq!(free(4, None), 4);
        let load = super::load_average();
        if cfg!(unix) {
            assert!(load.is_some_and(|l| l >= 0.0));
        } else {
            assert_eq!(load, None);
        }
    }
}
#[cfg(any(unix, windows))]
mod process_job;
mod repl;
mod report;
mod snapshot_update;
mod test_cmd;
mod wat;
mod work;

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
  hd [--cap NAME=VALUE]... FILE.cwasm [-- ARGS]
  hd run [--release] [--format json] [--cap NAME=VALUE]... [NAME]
  hd --repl [--engine NAME] FILE
  hd build [--release] [--format json|wasm] [FILE.hd]
  hd check [FILE.hd] [--tests | --all] [--fix] [--format json]
  hd lint [FILE.hd] [--allow CODE | --warn CODE | --deny CODE] [--fix] [--format json]
  hd doc [--private] [--out DIR] [--open] [--format json]
  hd new [--app | --lib] [--pages] [--vcs none] [PATH]
  hd clean [--cache]
  hd cache gc
  hd add [--dev] [--name NAME] TARGET[@QUERY]...
  hd update [NAME[@QUERY]]
  hd remove NAME
  hd fetch
  hd install [--root DIR] [--engine NAME] [--format wasm] [--deny CAP]... SOURCE
  hd install [--root DIR] --list
  hd uninstall [--root DIR] NAME
  hd fmt [--check] [FILE.hd]
  hd test [FILE.hd] [--filter PATTERN] [--jobs N] [--update] [--format json]

  FILE.hd, FILE.wasm, hd run and hd test accept --time-limit DURATION
  FILE.hd, FILE.wasm and hd run accept --cleanup-limit DURATION
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

/// Runs the command on a thread with a pool worker's stack
/// (`hd_sched::pool::WORKER_STACK`), so `hd fmt` and a one-thread build,
/// which run on this thread, take the chains a worker takes
/// (`grammar.limit.chain`). A main thread gets 8 MiB from the OS, and a
/// chain of 10,000 calls needs more. Untouched stack pages cost no memory.
#[must_use]
pub fn main() -> ExitCode {
    let work = work::start();
    let trace = work::start_trace();
    let status = match std::thread::Builder::new()
        .name("main".to_owned())
        .stack_size(hd_sched::pool::WORKER_STACK)
        // cost: one thread per `hd` process, whatever the input.
        .spawn(command_main)
    {
        Ok(thread) => thread
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        Err(_) => command_main(),
    };
    work::finish(work);
    work::finish_trace(trace);
    status
}

fn command_main() -> ExitCode {
    #[cfg(any(unix, windows))]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--process-job")
    {
        return process_job::run();
    }
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
        ("--repl", rest) => repl_script_command(rest),
        ("new", rest) => new_cmd::command(rest),
        ("clean", rest) => clean_cmd::command(rest),
        ("cache", rest) => cache_cmd::command(rest),
        ("add", rest) => dep_cmd::add(rest),
        ("update", rest) => dep_cmd::update(rest),
        ("remove", rest) => dep_cmd::remove(rest),
        ("fetch", rest) => dep_cmd::fetch(rest),
        ("install", rest) => install_cmd::install(rest),
        ("uninstall", rest) => install_cmd::uninstall(rest),
        ("--installed", rest) => install_cmd::run(rest),
        ("fmt", rest) => fmt_cmd::command(rest),
        ("run", rest) => run_command(rest),
        ("build", rest) => build_command(rest),
        ("check", rest) => check_cmd::command(rest),
        ("lint", rest) => lint_cmd::command(rest),
        ("doc", rest) => doc_cmd::command(rest),
        ("fix", _) => fail("`fix` is no command of hd; `hd help` lists them"),
        _ if arguments
            .iter()
            .take_while(|a| *a != "--")
            .any(|a| has_extension(a, "wasm") || has_extension(a, "cwasm")) =>
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

/// `hd --repl [--engine NAME] FILE` (`cli.repl.script`): a session whose
/// inputs are the lines of FILE, run on the selected engine.
fn repl_script_command(args: &[OsString]) -> ExitCode {
    let mut script: Option<PathBuf> = None;
    let mut engine = None;
    let mut i = 0;
    while i < args.len() {
        if let Some(flag) = engine::engine_flag(args, i) {
            match flag {
                Ok((kind, used)) => {
                    engine = Some(kind);
                    i += used;
                }
                Err(error) => return fail(&error),
            }
        } else if script.is_none() && !args[i].to_string_lossy().starts_with('-') {
            script = Some(PathBuf::from(&args[i]));
            i += 1;
        } else {
            return usage();
        }
    }
    // The REPL's own engine is wasmtime, so it is the default when neither
    // `--engine` nor HD_ENGINE names one (`cli.repl.script.engine`).
    let engine = match engine {
        Some(kind) => kind,
        None if std::env::var_os("HD_ENGINE").is_none_or(|v| v.is_empty()) => {
            engine::EngineKind::Wasmtime
        }
        None => match engine::from_env() {
            Ok(kind) => kind,
            Err(error) => return fail(&error),
        },
    };
    let Some(script) = script else {
        return usage();
    };
    repl::script(&script, engine)
}

/// Follow command dispatch: a source argument to `fmt` is not a program run.
fn compiles(command: &str, arguments: &[OsString]) -> bool {
    match command {
        "run" | "build" | "check" | "lint" | "test" | "doc" => true,
        "install" => !arguments.iter().any(|a| a == "--list"),
        "uninstall" | "--installed" => false,
        "help" | "--help" | "new" | "clean" | "add" | "update" | "remove" | "fetch" | "fmt"
        | "cache" | "--repl" => false,
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
                | "lint"
                | "doc"
                | "new"
                | "add"
                | "update"
                | "remove"
                | "fetch"
                | "clean"
                | "fmt"
                | "cache"
        )
    }) {
        let flag: Vec<OsString> = args.drain(..used).collect();
        args.splice(1..1, flag);
    }
    args
}

fn has_extension(word: &std::ffi::OsStr, ext: &str) -> bool {
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
/// what it wrote; a cache over its cap or of unknown size is collected by
/// a detached `hd cache gc`, so the command never waits for a walk
/// (`cli.cache.obj.evict`, #1932). Beside other runs it claims what it
/// misses, which the command's `DeferredStore` publishes as it puts it. A
/// command that compiles rejects an invalid cap before it gets here.
pub(crate) fn disk_store() -> work::Store {
    work::Store::new(open_disk_store().with_claims())
}

/// The disk store itself, for a consumer that takes a `DiskStore` by value:
/// the wasmtime engine's code cache, which claims nothing. The scale check
/// runs only `check` and `build`, which go through [`disk_store`], so none
/// of its work is counted.
pub(crate) fn open_disk_store() -> DiskStore {
    DiskStore::new(cache_dir())
        .with_cap(cache_cap().unwrap_or(hd_cache::disk::DEFAULT_CAP))
        .on_write_failure(warn_cache_write)
        .on_collection_due(spawn_collection)
}

/// Starts `hd cache gc` in the background, with `HD_CACHE_GC=auto`: it
/// collects only when no other process is collecting and the last
/// collection is an interval old. The command does not wait for it, and
/// a failed start leaves the collection to a later command. It writes
/// no work report or trace: their paths belong to the command (#1969).
fn spawn_collection(_root: &std::path::Path) {
    if let Ok(exe) = std::env::current_exe() {
        // cost: one process per collection interval, not waited for.
        let mut command = std::process::Command::new(exe);
        command
            .args(["cache", "gc"])
            .env("HD_CACHE_GC", "auto")
            .env_remove("HD_WORK")
            .env_remove("HD_TRACE")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // cost: the same one process.
        let _ = command.spawn();
    }
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

/// The `--jobs N` the command line gave (`cli.jobs`); 0 when none did.
static JOBS_FLAG: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `--jobs N` or `--jobs=N` at `args[i]` (`cli.jobs`): N, at least 1, and the
/// words used. Every command that compiles or tests reads it here, so they
/// all give the same error. [`set_jobs`] makes it the thread count.
pub(crate) fn jobs_flag(args: &[OsString], i: usize) -> Option<Result<(usize, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    let (value, used) = if text == "--jobs" {
        match args.get(i + 1) {
            Some(v) => (v.to_string_lossy().into_owned(), 2),
            None => return Some(Err("`--jobs` needs a value".to_owned())),
        }
    } else {
        (text.strip_prefix("--jobs=")?.to_owned(), 1)
    };
    Some(match value.parse::<usize>() {
        Ok(n) if n >= 1 => Ok((n, used)),
        _ => Err(format!(
            "`--jobs {value}`: N is a whole number of at least 1"
        )),
    })
}

/// Makes `n` the thread count of this run, over `HD_JOBS` and the cores.
pub(crate) fn set_jobs(n: usize) {
    JOBS_FLAG.store(n, std::sync::atomic::Ordering::Relaxed);
}

/// The thread count (`cli.jobs`, `cli.jobs.env`, `cli.jobs.default`): the
/// flag, else `HD_JOBS`, else the cores other processes leave free.
pub(crate) fn default_jobs() -> usize {
    let flag = JOBS_FLAG.load(std::sync::atomic::Ordering::Relaxed);
    if flag >= 1 {
        return flag;
    }
    std::env::var("HD_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or_else(|| {
            let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
            free_cores(u32::try_from(cores).unwrap_or(u32::MAX), load_average())
        })
}

/// `cli.jobs.default`: the cores less the 1-minute load average, rounded,
/// at least 1 and at most 8, so N concurrent `hd` processes do not each
/// start a pool on every core (#1802 draft 67). With no load average, as
/// on Windows, every core, at most 8.
fn free_cores(cores: u32, load: Option<f64>) -> usize {
    let busy = load.unwrap_or(0.0);
    (0..cores)
        .filter(|&k| f64::from(k) + 0.5 > busy)
        .count()
        .clamp(1, 8)
}

/// The system's 1-minute load average, as `uptime` shows it.
#[cfg(unix)]
#[allow(unsafe_code)]
fn load_average() -> Option<f64> {
    let mut load = [0.0f64; 1];
    // SAFETY: `getloadavg` writes at most `nelem` samples, here 1, into
    // the buffer, which holds 1, and keeps no pointer to it.
    let got = unsafe { libc::getloadavg(load.as_mut_ptr(), 1) };
    (got == 1 && load[0].is_finite()).then_some(load[0])
}

/// No load average off Unix: Windows has none.
#[cfg(not(unix))]
fn load_average() -> Option<f64> {
    None
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
/// `cli.profile.release`, `cli.profile.release.hd-file`).
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
    /// and optimized with `--release` (`cli.profile.pipeline.one`), with
    /// the code generator.
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
    settings: (BuildProfile, Pipeline),
    doc_scopes: bool,
    rep: &mut Reporter,
) -> Result<Output, ExitCode> {
    // Print, then publish (`crates/hd_cache/DESIGN.md` §5.4): the entries
    // reach the disk when the store drops, after the diagnostics.
    let store = hd_cache::DeferredStore::new(disk_store());
    build_goal_through(program, goal, settings, doc_scopes, rep, &store)
}

/// [`build_goal_in`] through `store`.
fn build_goal_through(
    program: &disk::Program,
    goal: &Goal,
    settings: (BuildProfile, Pipeline),
    doc_scopes: bool,
    rep: &mut Reporter,
    store: &dyn hd_cache::CacheStore,
) -> Result<Output, ExitCode> {
    if rep.diags(&program.problems) {
        return Err(rep.finish(HD_FAILURE));
    }
    let out = build_output(program, goal, settings, doc_scopes, store);
    let mut diags = report::from_output(&out, program.as_written.as_deref());
    diags.extend(caps::required(program, &out));
    report::relocate(&mut diags, |f| program.sources.display(f));
    if rep.diags(&diags) {
        return Err(rep.finish(HD_FAILURE));
    }
    Ok(out)
}

/// The driver's output for `goal` over `program` through `store`,
/// reporting nothing.
fn build_output(
    program: &disk::Program,
    goal: &Goal,
    (profile, pipeline): (BuildProfile, Pipeline),
    doc_scopes: bool,
    store: &dyn hd_cache::CacheStore,
) -> Output {
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store,
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
    out
}

/// [`compile`] for `hd build`: the module with its frames' positions
/// resolved and written in, so it prints a panic's frames without the
/// compiler's cache (`crates/hd_mono/DESIGN.md` "Positions").
fn compile_standalone(
    program: &disk::Program,
    (profile, pipeline): (BuildProfile, Pipeline),
    rep: &mut Reporter,
) -> Result<(Vec<u8>, Option<String>), ExitCode> {
    let store = hd_cache::DeferredStore::new(disk_store());
    compile_standalone_in(program, (profile, pipeline), rep, &store)
}

/// [`compile_standalone`], its artifact stored in and replayed from `store`.
fn compile_standalone_in(
    program: &disk::Program,
    (profile, pipeline): (BuildProfile, Pipeline),
    rep: &mut Reporter,
    store: &dyn hd_cache::CacheStore,
) -> Result<(Vec<u8>, Option<String>), ExitCode> {
    // A warm build with nothing changed writes the artifact a build of the
    // same inputs stored (#1510): one lookup beside the stamp table, no
    // per-module entry. A build that writes its text (`HD_WAT`) builds,
    // for the functions' names.
    let stored = stored_artifact(program, (profile, pipeline), store, "cli-build-artifact-v1")
        .filter(|_| !wat::requested());
    if let Some(wasm) = stored.and_then(|(key, client)| key.read(store, client)) {
        if rep.diags(&program.problems) {
            return Err(rep.finish(HD_FAILURE));
        }
        return Ok((wasm, None));
    }
    let reported = (rep.errors, rep.warnings);
    let (built, text) = build_traced(program, (profile, pipeline), rep, store)?;
    let mut wasm = built.wasm;
    if let Some((frames, sources)) = &built.frames {
        let section = frames.positions_section(&wasm, sources);
        hd_run::frames::cut_inlined(&mut wasm);
        hd_run::frames::cut_names(&mut wasm);
        wasm.extend(section);
    }
    // Only a build that reported nothing: a hit reports nothing either.
    if let Some((key, client)) = stored
        && (rep.errors, rep.warnings) == reported
    {
        key.write(store, client, &wasm);
    }
    Ok((wasm, text))
}

/// The key of `program`'s stored artifact (#1510), `client_tag` naming
/// which: `hd build`'s module or `hd run`'s. It holds the inputs of the
/// program build, and what the CLI reads besides them, a package's
/// manifest text (`caps::required`, which a single-file program skips).
/// The artifact is a function of these: the positions `hd build` writes
/// into it come from the same sources. A single file (`hd FILE`) and
/// standard input take it too: the file has a stamp and the input's is
/// its text's hash.
fn stored_artifact(
    program: &disk::Program,
    (profile, pipeline): (BuildProfile, Pipeline),
    store: &dyn hd_cache::CacheStore,
    client_tag: &str,
) -> Option<(hd_driver::ResultKey, hd_base::Hash128)> {
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store,
        clock: &clock,
        executor: executor(),
    };
    let packages = hd_driver::Packages {
        build_profile: profile,
        target: pipeline.target,
        level: pipeline.level,
        ..program.packages()
    };
    let goal = Goal::Program {
        entry: program.entry.clone(),
    };
    let key = hd_driver::result_key(&host, &program.package, &packages, &goal)?;
    let mut client = hd_base::StableHasher::new(client_tag);
    match manifest_text(program) {
        Some(text) => client.hash(hd_base::hash128(&text)),
        None => client.u8(0),
    }
    Some((key, client.finish()))
}

/// The manifest text a stored result's client key holds: a package's,
/// which `caps::required` reads; `None` for a single-file program, which
/// reads none.
pub(crate) fn manifest_text(program: &disk::Program) -> Option<std::sync::Arc<[u8]>> {
    if program.single_file {
        return None;
    }
    hd_project::SourceSet::read(&program.sources, "hd.toml")
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
    native: Option<hd_run::artifact::Native>,
    frames: Option<(test_cmd::Frames, &'a disk::DiskSources)>,
    /// A stored artifact's program and settings: its frames' positions
    /// come from building it again, only when a panic asks for them.
    stored: Option<(&'a disk::Program, (BuildProfile, Pipeline))>,
}

impl Compiled<'_> {
    fn from_native(native: hd_run::artifact::Native) -> Self {
        Self {
            wasm: native.metadata.clone(),
            native: Some(native),
            frames: None,
            stored: None,
        }
    }

    fn image(&self) -> hd_run::artifact::Image<'_> {
        match &self.native {
            Some(native) => hd_run::artifact::Image::Native(native),
            None => hd_run::artifact::Image::Wasm(&self.wasm),
        }
    }

    /// The lines of a trap's frames ([`test_cmd::Frames::lines`]). A stored
    /// artifact's program is built again through `store` for its positions
    /// (a warm build, each module's entries looked up), and only a build
    /// whose module is the stored one names them: a source edited since the
    /// lookup could move its anchors.
    fn frame_lines(&self, trap: &[(u32, u32)], store: &dyn hd_cache::CacheStore) -> Vec<String> {
        if trap.is_empty() {
            return Vec::new();
        }
        if let Some((frames, sources)) = &self.frames {
            return frames.lines(&self.wasm, trap, sources);
        }
        let Some((program, settings)) = self.stored else {
            return Vec::new();
        };
        let goal = Goal::Program {
            entry: program.entry.clone(),
        };
        let out = build_output(program, &goal, settings, false, store);
        if out.wasm.as_deref() != Some(self.wasm.as_slice()) {
            return Vec::new();
        }
        let positions = out.positions(&mut hd_driver::PositionPool::default());
        test_cmd::Frames::new(positions).lines(&self.wasm, trap, &program.sources)
    }
}

/// [`compile`], keeping what `hd FILE` and `hd run` need to report a
/// panic's source location. A warm run with nothing changed takes the
/// module a run of the same inputs stored (#1510): one lookup beside the
/// stamp table, no per-module entry, and the positions only when a panic
/// asks.
fn compile_traced<'a>(
    program: &'a disk::Program,
    settings: (BuildProfile, Pipeline),
    rep: &mut Reporter,
) -> Result<Compiled<'a>, ExitCode> {
    let store = hd_cache::DeferredStore::new(disk_store());
    compile_traced_in(program, settings, rep, &store)
}

/// [`compile_traced`], its module stored in and replayed from `store`.
fn compile_traced_in<'a>(
    program: &'a disk::Program,
    settings: (BuildProfile, Pipeline),
    rep: &mut Reporter,
    store: &dyn hd_cache::CacheStore,
) -> Result<Compiled<'a>, ExitCode> {
    let stored = stored_artifact(program, settings, store, "cli-run-module-v1");
    if let Some(wasm) = stored.and_then(|(key, client)| key.read(store, client)) {
        if rep.diags(&program.problems) {
            return Err(rep.finish(HD_FAILURE));
        }
        return Ok(Compiled {
            wasm,
            native: None,
            frames: None,
            stored: Some((program, settings)),
        });
    }
    let reported = (rep.errors, rep.warnings);
    let (built, _) = build_traced(program, settings, rep, store)?;
    // Only a build that reported nothing: a hit reports nothing either.
    if let Some((key, client)) = stored
        && (rep.errors, rep.warnings) == reported
    {
        key.write(store, client, &built.wasm);
    }
    Ok(built)
}

/// [`compile_traced`] with no stored module: the build through `store`,
/// with its positions.
fn build_traced<'a>(
    program: &'a disk::Program,
    settings: (BuildProfile, Pipeline),
    rep: &mut Reporter,
    store: &dyn hd_cache::CacheStore,
) -> Result<(Compiled<'a>, Option<String>), ExitCode> {
    let goal = Goal::Program {
        entry: program.entry.clone(),
    };
    let out = build_goal_through(program, &goal, settings, false, rep, store)?;
    let positions = out.positions(&mut hd_driver::PositionPool::default());
    let Some(wasm) = out.wasm else {
        return Err(rep.fail("no Wasm produced"));
    };
    // `HD_WAT`'s text: the module as built, before its names are cut, with
    // every function's name from the build's origins.
    let text = if out.instruction_origins.is_empty() {
        None
    } else {
        let names: Vec<(u32, &str)> = out
            .instruction_origins
            .iter()
            .map(|f| (f.index, f.name.as_str()))
            .collect();
        Some(wat::text(&wasm, &names).map_err(|e| rep.fail(&format!("HD_WAT: {e}")))?)
    };
    let compiled = Compiled {
        wasm,
        native: None,
        frames: Some((test_cmd::Frames::new(positions), &program.sources)),
        stored: None,
    };
    Ok((compiled, text))
}

/// The package host contract of a built module, when it declares one
/// with interfaces: one walk over the module's section headers.
pub(crate) fn host_contract(wasm: &[u8]) -> Result<Option<hd_host_abi::schema::Contract>, String> {
    hd_host_abi::schema::from_wasm(wasm)
        .map(|c| c.filter(|c| !c.interfaces.is_empty()))
        .map_err(|e| format!("the built host contract: {e}"))
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
    provider_program: Option<&disk::Program>,
    args: &[OsString],
    run: (
        engine::Choice,
        OptLevel,
        engine::ProgramLimits,
        Option<&Path>,
    ),
    rep: &mut Reporter,
) -> ExitCode {
    match host_contract(&built.wasm) {
        Ok(contract) => execute_with(
            contract.as_ref(),
            built,
            program,
            (table, table_dir, flags),
            provider_program,
            args,
            run,
            rep,
        ),
        Err(e) => rep.fail(&e),
    }
}

/// [`execute`] of a module whose package host `contract` the caller
/// already decoded. The capability refusal, the engine choice
/// (`cli.host.package.native-cli-startup`) and Node's providers all read
/// this one decoding.
fn execute_with(
    contract: Option<&hd_host_abi::schema::Contract>,
    built: &Compiled<'_>,
    program: &str,
    (table, table_dir, flags): (
        &[(String, hd_project::Grant)],
        Option<&Path>,
        &[caps::CapFlag],
    ),
    provider_program: Option<&disk::Program>,
    args: &[OsString],
    (choice, level, limits, sampling): (
        engine::Choice,
        OptLevel,
        engine::ProgramLimits,
        Option<&Path>,
    ),
    rep: &mut Reporter,
) -> ExitCode {
    let wasm = &built.wasm;
    if let Some(d) = caps::refusal(wasm, contract, table, flags) {
        rep.diag(&d);
        return rep.finish(HD_FAILURE);
    }
    // After the capability refusal (`cli.cap.order.deny`), before the
    // module loads: the run never changes engine afterwards.
    let choice = if built.native.is_some() {
        if choice.kind != engine::EngineKind::Wasmtime {
            return rep.fail(
                "a .cwasm module requires wasmtime; build `--format wasm` for a portable module",
            );
        }
        engine::Choice::named(engine::EngineKind::Wasmtime)
    } else {
        choice
    };
    let engine = match choice.for_module(contract) {
        Ok(engine) => engine,
        Err(e) => {
            rep.diag(&report::Diag::error(
                Some(hd_diag::Code::NativeProviderMissing),
                &e,
            ));
            return rep.finish(HD_FAILURE);
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let host = HostSetup {
        grants: caps::grants(table, table_dir.unwrap_or(&cwd), flags),
        // A run that Ctrl-C may cancel always has a cleanup bound
        // (`cli.cancel.default`).
        limits: engine::ProgramLimits {
            cleanup_ms: Some(limits.cleanup_ms.unwrap_or(engine::DEFAULT_CLEANUP_MS)),
            ..limits
        }
        .into(),
        args: args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        program: program.to_owned(),
    };
    let providers = if engine == engine::EngineKind::Node {
        match node::manifest_providers(contract, provider_program, &host.grants) {
            Ok(providers) => providers,
            Err(refusal) => {
                rep.diag(&refusal.diag());
                return rep.finish(HD_FAILURE);
            }
        }
    } else {
        Vec::new()
    };
    match engine::run(engine, level, built.image(), &host, &providers, sampling) {
        Ok((Outcome::Exit(code), trap)) => {
            // A panic's location follows the host's `panic:` line, as a
            // test's report does (`flow.panic.report`).
            if (built.frames.is_some() || built.stored.is_some()) && !trap.is_empty() {
                let store = hd_cache::DeferredStore::new(disk_store());
                for line in built.frame_lines(&trap, &store) {
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
    /// `--engine NAME`, else `HD_ENGINE`, else this build's default.
    engine: engine::Choice,
    release: bool,
    limits: engine::ProgramLimits,
    sampling: Option<PathBuf>,
}

fn file_words(args: &[OsString]) -> Result<FileWords, String> {
    let mut w = FileWords {
        json: false,
        caps: Vec::new(),
        file: None,
        program_args: Vec::new(),
        engine: engine::Choice::default(),
        release: false,
        limits: engine::ProgramLimits::default(),
        sampling: None,
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
            w.engine = engine::Choice::named(kind);
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
        if let Some(limit) = engine::cleanup_limit_flag(args, i) {
            let (ms, used) = limit?;
            w.limits.cleanup_ms = Some(ms);
            i += used;
            continue;
        }
        if let Some(path) = engine::sampling_flag(&args[i]) {
            w.sampling = Some(path?);
            i += 1;
            continue;
        }
        if let Some(jobs) = jobs_flag(args, i) {
            let (n, used) = jobs?;
            set_jobs(n);
            i += used;
            continue;
        }

        // `cli.profile.release.hd-file`: the release profile, and wasmtime's
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
        w.engine = engine::Choice::from_env()?;
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
    let pipeline = Pipeline::of(w.engine.kind, w.release);
    match compile_traced(&program, (profile_of(w.release), pipeline), &mut rep) {
        Ok(built) => execute(
            &built,
            &file.to_string_lossy(),
            &program.capabilities,
            program.package_dir.as_deref(),
            &w.caps,
            Some(&program),
            &w.program_args,
            (w.engine, pipeline.level, w.limits, w.sampling.as_deref()),
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
    let engine = match engine::Choice::from_env() {
        Ok(engine) => engine,
        Err(error) => return fail(&error),
    };
    let mut rep = Reporter::stderr(false);
    let program = match disk::load_stdin(&text) {
        Ok(p) => p,
        Err(e) => return rep.fail(&e),
    };
    let pipeline = Pipeline::of(engine.kind, false);
    match compile_traced(&program, (BuildProfile::Checked, pipeline), &mut rep) {
        Ok(built) => execute(
            &built,
            "main.hd",
            &program.capabilities,
            program.package_dir.as_deref(),
            &[],
            Some(&program),
            &[],
            (
                engine,
                pipeline.level,
                engine::ProgramLimits::default(),
                None,
            ),
            &mut rep,
        ),
        Err(code) => code,
    }
}

/// `hd [--cap NAME=VALUE]... FILE.wasm [-- ARGS]` (Prebuilt Modules,
/// `cli.wasm.*`): a module that `hd build` wrote. It is checked first: a
/// valid module (`cli.wasm.invalid`), with the entry exports and only
/// imports `hd` provides (`cli.wasm.not-hd.*`). Builtin-only modules use
/// flags alone; package providers discover consent above the artifact.
fn run_wasm_command(args: &[OsString]) -> ExitCode {
    let w = match file_words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stderr(w.json);
    if w.release {
        let extension = if w
            .file
            .as_ref()
            .is_some_and(|file| has_extension(file.as_os_str(), "cwasm"))
        {
            "cwasm"
        } else {
            "wasm"
        };
        return rep.fail(&format!("--release does not apply to FILE.{extension}; its optimization level is recorded in the module"));
    }
    let Some(file) = &w.file else {
        return usage();
    };

    let shown = file.display();
    let native = if has_extension(file.as_os_str(), "cwasm") {
        match hd_run::artifact::Native::open(file) {
            Ok(native) => Some(native),
            Err(error) => return rep.fail(&format!("{shown}: {error}; rebuild with `hd build --release`, or build `--format wasm` for a portable module")),
        }
    } else {
        None
    };
    let wasm = match &native {
        Some(native) => native.metadata.clone(),
        None => match std::fs::read(file) {
            Ok(b) => b,
            Err(e) => return rep.fail(&format!("{shown}: {e}")),
        },
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
    let metadata = match hd_host_abi::schema::module_metadata(&wasm) {
        Ok(metadata) => metadata,
        Err(e) => return rep.fail(&format!("`{shown}` has invalid module metadata: {e}")),
    };
    let level = if native.is_some() {
        OptLevel::Optimized
    } else {
        metadata.opt_level
    };
    let contract = metadata.contract.filter(|c| !c.interfaces.is_empty());
    // One index of the contract's imports, probed once per import.
    let described = contract.as_ref().map(hd_host_abi::schema::Imports::new);
    let ordinary: Vec<_> = imports
        .into_iter()
        .filter(|(m, n)| !described.as_ref().is_some_and(|d| d.describes(m, n)))
        .collect();
    if let Some((m, n)) = hd_run::unknown_import(&ordinary) {
        return rep.fail(&format!(
            "`{shown}` imports `{m}` `{n}`, which `hd` does not provide: {rebuild}"
        ));
    }
    // Package providers need the root manifest's explicit consent even for a
    // prebuilt artifact. Discovery follows the artifact, never the caller cwd.
    let provider_program = if contract.is_some() {
        let absolute = match std::path::absolute(file) {
            Ok(path) => path,
            Err(e) => return rep.fail(&format!("{shown}: {e}")),
        };
        let Some(root) = absolute.parent().and_then(disk::package_root) else {
            return rep.fail("package host capabilities need an app hd.toml above the Wasm file");
        };
        let program = match disk::load_package(&root, "src/main.hd") {
            Ok(program) => program,
            Err(e) => return rep.fail(&e),
        };
        if rep.diags(&program.problems) {
            return rep.finish(HD_FAILURE);
        }
        Some(program)
    } else {
        None
    };
    let table = provider_program
        .as_ref()
        .map_or(&[][..], |p| p.capabilities.as_slice());
    let built = Compiled {
        wasm,
        native,
        frames: None,
        stored: None,
    };
    execute_with(
        contract.as_ref(),
        &built,
        &file.to_string_lossy(),
        (
            table,
            provider_program
                .as_ref()
                .and_then(|p| p.package_dir.as_deref()),
            &w.caps,
        ),
        provider_program.as_ref(),
        &w.program_args,
        (w.engine, level, w.limits, w.sampling.as_deref()),
        &mut rep,
    )
}

fn portable_flag(args: &[OsString], i: usize) -> Option<usize> {
    match args.get(i)?.to_str()? {
        "--format=wasm" => Some(1),
        "--format" if args.get(i + 1).is_some_and(|value| value == "wasm") => Some(2),
        _ => None,
    }
}

/// The words and flags of `hd run` and `hd build`.
struct Words {
    release: bool,
    portable: bool,
    json: bool,
    /// `--cap` flags; only `hd run` takes them (`cli.cap.flag.commands`).
    caps: Vec<caps::CapFlag>,
    positional: Vec<OsString>,
    /// The words after `--`; only `hd run` takes them (`cli.args.separator`).
    program_args: Vec<OsString>,
    /// The members `-p NAME` selects (`cli.workspace.select.anywhere`).
    packages: Vec<String>,
    /// `--engine NAME`, else `HD_ENGINE`, else this build's default: the engine `hd run`
    /// runs on, and the one whose target `hd run` and `hd build` tune for.
    engine: engine::Choice,
    limits: engine::ProgramLimits,
    sampling: Option<PathBuf>,
}

fn words(command: &str, args: &[OsString]) -> Result<Words, String> {
    let mut w = Words {
        release: false,
        portable: false,
        json: false,
        caps: Vec::new(),
        positional: Vec::new(),
        program_args: Vec::new(),
        packages: Vec::new(),
        engine: engine::Choice::default(),
        limits: engine::ProgramLimits::default(),
        sampling: None,
    };
    let mut engine_flag = false;
    let mut i = 0;
    while i < args.len() {
        if command == "run" && args[i] == "--" {
            w.program_args = args[i + 1..].to_vec();
            break;
        }
        if command == "build"
            && let Some(used) = portable_flag(args, i)
        {
            w.portable = true;
            i += used;
            continue;
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
            w.engine = engine::Choice::named(kind);
            engine_flag = true;
            i += used;
            continue;
        }
        if command == "run"
            && let Some(path) = engine::sampling_flag(&args[i])
        {
            w.sampling = Some(path?);
            i += 1;
            continue;
        }
        if let Some(jobs) = jobs_flag(args, i) {
            let (n, used) = jobs?;
            set_jobs(n);
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
        if command == "run"
            && let Some(limit) = engine::cleanup_limit_flag(args, i)
        {
            let (ms, used) = limit?;
            w.limits.cleanup_ms = Some(ms);
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
        w.engine = engine::Choice::from_env()?;
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
    if let Some(d) = disk::manifest_syntax_problem(&root) {
        rep.diag(&d);
        return rep.finish(HD_FAILURE);
    }
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
    let pipeline = Pipeline::of(w.engine.kind, w.release);
    let built = if w.release && w.engine.kind == engine::EngineKind::Wasmtime {
        build_artifact(
            &program,
            &root,
            (true, pipeline, false),
            false,
            &chosen.name,
            Some(w.engine),
            &mut rep,
        )
    } else {
        compile_traced(&program, (profile_of(w.release), pipeline), &mut rep)
    };
    let built = match built {
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
        &program.execution_capabilities(),
        program.package_dir.as_deref(),
        &w.caps,
        Some(&program),
        &w.program_args,
        (w.engine, pipeline.level, w.limits, w.sampling.as_deref()),
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

fn build_artifact(
    program: &disk::Program,
    root: &Path,
    (release, pipeline, portable): (bool, Pipeline, bool),
    files: bool,
    stem: &str,
    run_choice: Option<engine::Choice>,
    rep: &mut Reporter,
) -> Result<Compiled<'static>, ExitCode> {
    let native = release && !portable && pipeline.target == hd_base::EngineTarget::Wasmtime;
    let settings = (profile_of(release), pipeline);
    let dir = root
        .join(if release {
            "build/release"
        } else {
            "build/debug"
        })
        .join(if files { "files" } else { "" });
    let out = dir.join(format!("{stem}.{}", if native { "cwasm" } else { "wasm" }));
    let store = hd_cache::DeferredStore::new(disk_store());
    let tag = if native {
        Some(format!(
            "cli-native-output-v1:{:032x}",
            engine::native_key().map_err(|e| rep.fail(&e))?.0
        ))
    } else {
        None
    };
    let input_key = || {
        tag.as_ref()
            .and_then(|tag| stored_artifact(program, settings, &store, tag))
            .map(|(key, client)| key.entry(client))
            .filter(|key| key.0 != 0)
    };
    let key = input_key();
    if !wat::requested()
        && let Some(key) = key
        && let Ok(image) = hd_run::artifact::Native::open(&out)
        && image.key == key
        && let Ok(contract) = host_contract(&image.metadata)
        && (contract.is_none()
            || (dir.join(format!("{stem}.host.mjs")).is_file()
                && dir.join(format!("{stem}.host.rs")).is_file()))
        && run_choice.is_none_or(|choice| {
            matches!(
                choice.for_module(contract.as_ref()),
                Ok(engine::EngineKind::Wasmtime)
            )
        })
    {
        if rep.diags(&program.problems) {
            return Err(rep.finish(HD_FAILURE));
        }
        return Ok(Compiled::from_native(image));
    }
    let reported = (rep.errors, rep.warnings);
    let (wasm, text) = compile_standalone_in(program, settings, rep, &store)?;
    let built = Compiled {
        wasm,
        native: None,
        frames: None,
        stored: None,
    };
    if native {
        let contract = host_contract(&built.wasm).map_err(|e| rep.fail(&e))?;
        if run_choice.is_some_and(|choice| {
            !matches!(
                choice.for_module(contract.as_ref()),
                Ok(engine::EngineKind::Wasmtime)
            )
        }) {
            // execute retains the existing provider fallback and capability-refusal order.
            return Ok(built);
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| rep.fail(&e.to_string()))?;
    write_companions(&dir, stem, pipeline.target, &built.wasm, text.as_deref())
        .map_err(|e| rep.fail(&e))?;
    if native {
        let key = key
            .or_else(input_key)
            .filter(|_| (rep.errors, rep.warnings) == reported)
            .unwrap_or(hd_base::Hash128(0));
        let bytes = engine::precompile(&built.wasm)
            .and_then(|bytes| hd_run::artifact::encode(bytes, &built.wasm, key))
            .map_err(|e| rep.fail(&e))?;
        publish(&out, &bytes).map_err(|e| rep.fail(&e))?;
        return hd_run::artifact::Native::open(&out)
            .map(Compiled::from_native)
            .map_err(|e| rep.fail(&e));
    }
    std::fs::write(&out, &built.wasm).map_err(|e| rep.fail(&format!("{}: {e}", out.display())))?;
    Ok(built)
}

/// Publish a new file: running native images keep their existing mapping.
fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temp = path.with_extension(format!("pending-{}-{nanos}-{serial}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
}

fn write_companions(
    dir: &Path,
    stem: &str,
    target: hd_base::EngineTarget,
    wasm: &[u8],
    text: Option<&str>,
) -> Result<(), String> {
    let artifacts = hd_driver::host_artifacts(wasm).map_err(str::to_owned)?;
    if let Some(text) = text {
        let wat = dir.join(format!("{stem}.wat"));
        std::fs::write(&wat, text).map_err(|e| format!("{}: {e}", wat.display()))?;
    }
    // The JavaScript glue always, and for a Wasmtime build the Rust
    // companion too (`cli.host.package.native-artifact-companion`), without
    // changing portable Wasm (`native-portable-identity`). A build that needs
    // neither removes a stale one.
    let native = match &artifacts {
        Some(a) if target == hd_base::EngineTarget::Wasmtime => {
            Some(hd_host_abi::native_rust::generate(&a.contract).map_err(str::to_owned)?)
        }
        _ => None,
    };
    companion(
        &dir.join(format!("{stem}.host.mjs")),
        artifacts.map(|a| a.javascript),
    )?;
    companion(&dir.join(format!("{stem}.host.rs")), native)
}

/// Writes `text` to `path`, or removes a stale `path` when there is none.
fn companion(path: &Path, text: Option<String>) -> Result<(), String> {
    match text {
        Some(text) => std::fs::write(path, text),
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
    }
    .map_err(|e| format!("{}: {e}", path.display()))
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
    let pipeline = Pipeline::of(w.engine.kind, w.release);
    let built = match w.positional.as_slice() {
        [] if !w.packages.is_empty() => {
            match std::env::current_dir()
                .map_err(|e| e.to_string())
                .and_then(|cwd| disk::select_members(&cwd, &w.packages))
            {
                Ok(members) => members.iter().try_for_each(|(root, _)| {
                    build_package(root, (w.release, pipeline, w.portable), &mut rep)
                }),
                Err(e) => Err(rep.fail(&e)),
            }
        }
        [] => match std::env::current_dir()
            .ok()
            .and_then(|cwd| disk::workspace_root(&cwd))
        {
            // `cli.workspace.members`: each member in turn.
            Some((_, members)) => members.iter().try_for_each(|root| {
                build_package(root, (w.release, pipeline, w.portable), &mut rep)
            }),
            None => match package_of_cwd("build") {
                Ok(root) => build_package(&root, (w.release, pipeline, w.portable), &mut rep),
                Err(e) => Err(rep.fail(&e)),
            },
        },
        [file] => build_file(Path::new(file), (w.release, pipeline, w.portable), &mut rep),
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
    (release, pipeline, portable): (bool, Pipeline, bool),
    rep: &mut Reporter,
) -> Result<(), ExitCode> {
    if let Some(d) = disk::manifest_syntax_problem(root) {
        rep.diag(&d);
        return Err(rep.finish(HD_FAILURE));
    }
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
        build_artifact(
            &program,
            root,
            (release, pipeline, portable),
            false,
            &exe.name,
            None,
            rep,
        )?;
    }
    Ok(())
}

/// `hd build FILE`: the module of a file of the package, linked with the
/// rest of it. Its package is the one at or above FILE's directory
/// (`cli.mode.start`).
fn build_file(
    file: &Path,
    (release, pipeline, portable): (bool, Pipeline, bool),
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
    let stem = full
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    build_artifact(
        &program,
        &root,
        (release, pipeline, portable),
        true,
        &stem,
        None,
        rep,
    )
    .map(|_| ())
}

#[cfg(all(test, unix))]
mod stored_artifact_tests {
    use super::*;
    use hd_testkit::{CountingStore, Scratch};

    /// `hd build`'s artifact of the package at `root`, listed now, and
    /// whether it was the stored one.
    fn build(root: &Path, store: &CountingStore) -> (Vec<u8>, bool) {
        let mut program = disk::load_package(root, "main").expect("package");
        program.only_program("main");
        let pipeline = Pipeline::of(engine::EngineKind::default(), false);
        store.reset();
        let mut rep = Reporter::stdout(false);
        let (wasm, _) =
            compile_standalone_in(&program, (profile_of(false), pipeline), &mut rep, store)
                .expect("built");
        // A hit: the set's stamp table and the stored artifact.
        (wasm, store.gets() == 2)
    }

    /// Waits until every file written so far has a stamp.
    fn settle() {
        std::thread::sleep(disk::SETTLE + std::time::Duration::from_millis(200));
    }

    /// A package `shop` of `n` modules besides `src/main.hd`, whose `main`
    /// panics in `pick`, which `src/f0/mod.hd` declares.
    fn panicking(name: &str, n: usize) -> Scratch {
        let dir = Scratch::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"),
            name,
        );
        std::fs::create_dir_all(dir.join("src")).expect("src");
        std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("toml");
        std::fs::write(
            dir.join("src/main.hd"),
            include_str!("tests/programs/lib/panicking.hd"),
        )
        .expect("main");
        for i in 0..n {
            let path = dir.join(format!("src/f{i}/mod.hd"));
            std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
            let text = if i == 0 {
                include_str!("tests/programs/lib/panicking_2.hd").to_owned()
            } else {
                format!("pub fn v() -> i32: {i}\n")
            };
            std::fs::write(path, text).expect("module");
        }
        dir
    }

    /// `hd run`'s module of the package at `root`, listed now, and the
    /// lookups that took it.
    fn compiled<'a>(program: &'a disk::Program, store: &CountingStore) -> (Compiled<'a>, usize) {
        let pipeline = Pipeline::of(engine::EngineKind::default(), false);
        store.reset();
        let mut rep = Reporter::stderr(false);
        let Ok(built) = compile_traced_in(program, (profile_of(false), pipeline), &mut rep, store)
        else {
            panic!("built");
        };
        (built, store.gets())
    }

    fn load(root: &Path) -> disk::Program {
        let mut program = disk::load_package(root, "main").expect("package");
        program.only_program("main");
        program
    }

    /// A run of `wasm`'s trap frames.
    fn trap_of(wasm: &[u8]) -> Vec<(u32, u32)> {
        let host = HostSetup {
            grants: hd_run::Grants::default(),
            limits: engine::ProgramLimits::default().into(),
            args: Vec::new(),
            program: "main".to_owned(),
        };
        let kind = engine::EngineKind::default();
        let (outcome, trap) = engine::run(
            kind,
            OptLevel::Dev,
            hd_run::artifact::Image::Wasm(wasm),
            &host,
            &[],
            None,
        )
        .expect("the engine runs");
        assert!(matches!(outcome, Outcome::Exit(3)), "{outcome:?}");
        trap
    }

    /// Cost Model (#1510): a warm `hd run` with nothing changed takes the
    /// stored module with one lookup beside the set's stamp table, at 2
    /// modules as at 40, and builds
    /// nothing until a panic asks for its frames, which then print the
    /// lines the cold run's do. A settled edit builds again.
    #[test]
    fn a_warm_run_takes_the_stored_module_and_its_panic_names_its_lines() {
        for n in [1, 40] {
            let dir = panicking(&format!("run-stored-module-{n}"), n);
            settle();
            let store = CountingStore::default();
            let program = load(&dir);
            let (cold, gets) = compiled(&program, &store);
            assert!(
                cold.stored.is_none() && gets > n,
                "{n}: cold, {gets} lookups"
            );
            let (warm, gets) = compiled(&program, &store);
            assert_eq!(gets, 2, "{n}: warm");
            assert!(warm.frames.is_none() && warm.stored.is_some());
            assert_eq!(warm.wasm, cold.wasm);
            let trap = trap_of(&warm.wasm);
            let lines = warm.frame_lines(&trap, &store);
            assert_eq!(lines, cold.frame_lines(&trap, &store));
            assert_eq!(
                lines,
                [
                    "src/f0/mod.hd:2 shop.f0.pick",
                    "src/main.hd:4 shop.main.main"
                ],
                "{n}"
            );
            // A comment moves `pick` down a line; the stored module is the
            // same, its frames are the edited file's.
            std::fs::write(
                dir.join("src/f0/mod.hd"),
                include_str!("tests/programs/lib/a_warm_run_takes_the_stored_module_and_its_panic_names_its_lines.hd"),
            )
            .expect("edit");
            settle();
            let program = load(&dir);
            let (edited, gets) = compiled(&program, &store);
            assert!(
                edited.stored.is_none() && gets > 1,
                "{n}: an edit builds again"
            );
            let (warm, gets) = compiled(&program, &store);
            assert_eq!(gets, 2, "{n}: warm after the edit");
            assert_eq!(
                warm.frame_lines(&trap_of(&warm.wasm), &store),
                [
                    "src/f0/mod.hd:3 shop.f0.pick",
                    "src/main.hd:4 shop.main.main"
                ],
                "{n}"
            );
        }
    }

    /// Cost Model (#1510): a warm `hd FILE` with nothing changed takes the
    /// stored module with one lookup beside its stamp table, as a
    /// package's run does, though the
    /// file reaches std modules; its panic names the cold run's lines,
    /// under the file's stem. A settled edit builds again.
    #[test]
    fn a_warm_single_file_run_takes_the_stored_module_and_its_panic_names_its_lines() {
        let dir = Scratch::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"),
            "run-stored-single-file",
        );
        let file = dir.join("pick.hd");
        let text = |comment: &str| {
            format!(
                "{comment}fn pick(items: List[i32], at: usize) -> i32:\n    items[at]\n\n\
                 pub fn main() -> void:\n    let _ = pick([1, 2], 5)\n"
            )
        };
        std::fs::write(&file, text("")).expect("file");
        settle();
        let store = CountingStore::default();
        let program = disk::load_file(&file).expect("file");
        let (cold, gets) = compiled(&program, &store);
        assert!(cold.stored.is_none() && gets > 3, "cold, {gets} lookups");
        let (warm, gets) = compiled(&program, &store);
        assert_eq!(gets, 2, "warm");
        assert!(warm.frames.is_none() && warm.stored.is_some());
        assert_eq!(warm.wasm, cold.wasm);
        let trap = trap_of(&warm.wasm);
        let lines = warm.frame_lines(&trap, &store);
        assert_eq!(lines, cold.frame_lines(&trap, &store));
        assert_eq!(lines, ["pick.hd:2 pick.pick", "pick.hd:5 pick.main"]);
        std::fs::write(&file, text("# Picks.\n")).expect("edit");
        settle();
        let program = disk::load_file(&file).expect("file");
        let (edited, gets) = compiled(&program, &store);
        assert!(edited.stored.is_none() && gets > 1, "an edit builds again");
        let (warm, gets) = compiled(&program, &store);
        assert_eq!(gets, 2, "warm after the edit");
        assert_eq!(
            warm.frame_lines(&trap_of(&warm.wasm), &store),
            ["pick.hd:3 pick.pick", "pick.hd:6 pick.main"]
        );
    }

    /// Cost Model (#1510): standard input is keyed by its text, which needs
    /// no settling: the same text again takes the stored module with one
    /// lookup, and another text builds its own.
    #[test]
    fn a_warm_stdin_run_is_keyed_by_its_text() {
        let main = |n: i32| format!("pub fn main() -> void:\n    let _ = [{n}]\n");
        let store = CountingStore::default();
        let one = disk::load_stdin(&main(1)).expect("stdin");
        let (cold, gets) = compiled(&one, &store);
        assert!(cold.stored.is_none() && gets > 3, "cold, {gets} lookups");
        let again = disk::load_stdin(&main(1)).expect("stdin");
        let (warm, gets) = compiled(&again, &store);
        assert_eq!((gets, warm.wasm == cold.wasm), (1, true), "warm");
        let two = disk::load_stdin(&main(2)).expect("stdin");
        let (other, gets) = compiled(&two, &store);
        assert!(other.stored.is_none() && gets > 1, "another text builds");
        assert_ne!(other.wasm, cold.wasm);
    }

    /// Cost Model (#1510): a warm `hd build` with nothing changed writes
    /// the stored artifact, byte for byte the one built; a settled edit
    /// builds again.
    #[test]
    fn a_warm_build_writes_the_stored_artifact_and_an_edit_builds_again() {
        let dir = Scratch::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"),
            "build-stored-artifact",
        );
        let root = dir.join("p");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(
            root.join("hd.toml"),
            "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
        )
        .expect("toml");
        let main =
            |n: i32| format!("pub fn main() -> void $ Console:\n    println(\"{{{n} + 1}}\")\n");
        std::fs::write(root.join("src/main.hd"), main(1)).expect("main");
        std::thread::sleep(disk::SETTLE + std::time::Duration::from_millis(200));
        let store = CountingStore::default();
        let (cold, hit) = build(&root, &store);
        assert!(!hit, "cold");
        let (warm, hit) = build(&root, &store);
        assert!(hit, "warm");
        assert_eq!(warm, cold);
        std::fs::write(root.join("src/main.hd"), main(2)).expect("edit");
        std::thread::sleep(disk::SETTLE + std::time::Duration::from_millis(200));
        let (edited, hit) = build(&root, &store);
        assert!(!hit, "an edit builds again");
        assert_ne!(edited, cold);
        assert_eq!(build(&root, &store), (edited, true));
    }
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
        for command in ["run", "build", "check", "lint", "test"] {
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
        assert_eq!(
            format_after_command(args(&["--format", "json", "lint", "main.hd"])),
            args(&["lint", "--format", "json", "main.hd"])
        );
        let file = args(&["--format", "json", "main.hd"]);
        assert_eq!(format_after_command(file.clone()), file);
    }
}
