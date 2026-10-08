#![forbid(unsafe_code)]
//! The `hd` command. It owns everything outside the core (design-overview
//! §2.2): the file system (`DiskSources`, the disk `CacheStore`), the clock,
//! the executor and the engine (`node::NodeEngine`, an `hd_run::Engine`).

mod check_cmd;
mod disk;
mod node;
mod test_cmd;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_driver::{Clock, Executor, Goal, Host, Output, build};
use hd_run::{Grants, HostSetup, Limits, Outcome, run_program};

const USAGE: &str = "usage:
  hd FILE.hd
  hd run [--release] [NAME]
  hd build [--release] [FILE.hd]
  hd check [FILE.hd] [--format json]
  hd test [FILE.hd] [--filter PATTERN] [--jobs N]";

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
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(command) = arguments.first().and_then(|c| c.to_str()) else {
        return usage();
    };
    match (command, &arguments[1..]) {
        ("test", rest) => test_cmd::command(rest),
        ("run", rest) => run_command(rest),
        ("build", rest) => build_command(rest),
        ("check", rest) => check_cmd::command(rest),
        (file, [])
            if Path::new(file)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("hd")) =>
        {
            run_file_command(Path::new(file))
        }
        _ => usage(),
    }
}

pub(crate) struct Wall(pub(crate) Instant);

impl Clock for Wall {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// The compiled-cache directory (`cli.cache.obj`): `obj` under `HD_CACHE`,
/// or under the platform's user cache directory followed by `hd`.
pub(crate) fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HD_CACHE") {
        return PathBuf::from(dir).join("obj");
    }
    let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Caches")
    } else {
        std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from)
    };
    base.join("hd").join("obj")
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
    let threads = default_jobs();
    if threads > 1 {
        Executor::Pool(threads)
    } else {
        Executor::Serial(hd_sched::SerialOrder::Priority)
    }
}

/// Builds `goal` over a program's sources and prints the diagnostics.
/// Warnings are shown; only errors stop the command.
fn build_goal(program: &disk::Program, goal: &Goal) -> Result<Output, ExitCode> {
    let store = DiskStore { root: cache_dir() };
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store: &store,
        clock: &clock,
        executor: executor(),
    };
    let out: Output = build(&host, &program.package, goal);
    eprint!("{}", out.render_located(&program.sources));
    if out.diags.has_errors() {
        return Err(ExitCode::from(HD_FAILURE));
    }
    Ok(out)
}

/// Compiles a program to Wasm bytes, printing diagnostics.
fn compile(program: &disk::Program) -> Result<Vec<u8>, ExitCode> {
    let goal = Goal::Program {
        entry: program.entry.clone(),
    };
    build_goal(program, &goal)?
        .wasm
        .ok_or_else(|| fail("no Wasm produced"))
}

/// Runs compiled Wasm and exits with the program's status
/// (`cli.exit.program`).
fn execute(wasm: &[u8]) -> ExitCode {
    let store = DiskStore { root: cache_dir() };
    let host = HostSetup {
        providers: Vec::new(),
        grants: Grants::default(),
        limits: Limits::default(),
    };
    match run_program(&node::NodeEngine, wasm, &store, &host) {
        Ok(Outcome::Exit(0)) => ExitCode::SUCCESS,
        Ok(Outcome::Exit(code)) => ExitCode::from(code),
        Ok(Outcome::Panic(p)) => {
            eprintln!("panic: {}: {}", p.category, p.message);
            ExitCode::from(101)
        }
        Ok(Outcome::Internal(e)) => fail(&e),
        Err(e) => fail(&e.to_string()),
    }
}

/// `hd FILE.hd` (`cli.file.run`): FILE as a single-file program.
fn run_file_command(file: &Path) -> ExitCode {
    let program = match disk::load_file(file) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    match compile(&program) {
        Ok(wasm) => execute(&wasm),
        Err(code) => code,
    }
}

/// The words and flags of `hd run` and `hd build`.
struct Words {
    release: bool,
    positional: Vec<OsString>,
}

fn words(command: &str, args: &[OsString]) -> Result<Words, String> {
    let mut w = Words {
        release: false,
        positional: Vec::new(),
    };
    for a in args {
        let text = a.to_string_lossy();
        if text == "--release" {
            w.release = true;
        } else if text.starts_with('-') {
            return Err(format!("`hd {command}` has no option `{text}`"));
        } else {
            w.positional.push(a.clone());
        }
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
    let w = match words("run", args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let name = match w.positional.as_slice() {
        [] => None,
        [one] => Some(one.to_string_lossy().into_owned()),
        _ => return fail("`hd run` takes at most one NAME"),
    };
    if let Some(n) = &name
        && is_path(n)
    {
        return if Path::new(n).is_dir() {
            fail(&format!(
                "`{n}` is a directory, neither a NAME nor a FILE; select a package member with `-p NAME`"
            ))
        } else {
            fail(&format!(
                "`hd run` takes no FILE (`{n}`); use `hd run` for the package's executable or `hd run NAME` for a named executable or task, and `hd FILE` to run one file"
            ))
        };
    }
    let root = match package_of_cwd("run") {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    let package = match disk::package_name(&root) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let executables = disk::executables(&root, &package);
    let tasks = disk::tasks(&root);
    if let Some(t) = tasks
        .iter()
        .find(|t| executables.iter().any(|e| e.name == t.name))
    {
        return fail(&format!(
            "the task `{}` and an executable have the same name",
            t.name
        ));
    }
    let chosen = match name {
        None => match executables.as_slice() {
            [one] => one,
            [] => {
                return fail(&format!(
                    "package `{package}` has no executable to run; `hd run NAME` runs a task"
                ));
            }
            _ => {
                return fail(&format!(
                    "package `{package}` has several executables; run one with `hd run NAME`"
                ));
            }
        },
        Some(n) => match executables.iter().chain(&tasks).find(|r| r.name == n) {
            Some(r) => r,
            None => {
                return fail(&format!(
                    "package `{package}` has no executable or task named `{n}`"
                ));
            }
        },
    };
    let program = match disk::load_package(&root, &chosen.file) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let wasm = match compile(&program) {
        Ok(wasm) => wasm,
        Err(code) => return code,
    };
    if chosen.is_task
        && let Err(e) = std::env::set_current_dir(&root)
    {
        return fail(&format!("{}: {e}", root.display()));
    }
    execute(&wasm)
}

/// Writes a built module to `build/PROFILE/[files/]NAME.wasm`
/// (`cli.build.output`, `cli.build.output.file`).
fn write_module(root: &Path, release: bool, files: bool, stem: &str, wasm: &[u8]) -> ExitCode {
    let mut dir = root
        .join("build")
        .join(if release { "release" } else { "debug" });
    if files {
        dir.push("files");
    }
    let out = dir.join(format!("{stem}.wasm"));
    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&out, wasm)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&format!("{}: {e}", out.display())),
    }
}

/// `hd build [FILE]` (`cli.build.*`).
fn build_command(args: &[OsString]) -> ExitCode {
    let w = match words("build", args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    match w.positional.as_slice() {
        [] => build_package(w.release),
        [file] => build_file(Path::new(file), w.release),
        _ => fail("`hd build` takes at most one FILE"),
    }
}

/// A whole-package `hd build`: each executable to its module; a package with
/// no executable is checked and writes no `.wasm` file.
fn build_package(release: bool) -> ExitCode {
    let root = match package_of_cwd("build") {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    let package = match disk::package_name(&root) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let executables = disk::executables(&root, &package);
    if executables.is_empty() {
        let program = match disk::load_package(&root, "main") {
            Ok(p) => p,
            Err(e) => return fail(&e),
        };
        return match build_goal(&program, &Goal::Analyze) {
            Ok(_) => ExitCode::SUCCESS,
            Err(code) => code,
        };
    }
    for exe in &executables {
        let program = match disk::load_package(&root, &exe.file) {
            Ok(p) => p,
            Err(e) => return fail(&e),
        };
        let wasm = match compile(&program) {
            Ok(wasm) => wasm,
            Err(code) => return code,
        };
        let code = write_module(&root, release, false, &exe.name, &wasm);
        if code != ExitCode::SUCCESS {
            return code;
        }
    }
    ExitCode::SUCCESS
}

/// `hd build FILE`: the module of a file of the package, linked with the
/// rest of it. Its package is the one at or above FILE's directory
/// (`cli.mode.start`).
fn build_file(file: &Path, release: bool) -> ExitCode {
    if file.extension().is_none_or(|x| x != "hd") {
        return fail(&format!(
            "`{}` is not an .hd file; `hd build` takes a FILE.hd",
            file.display()
        ));
    }
    let full = match std::fs::canonicalize(file) {
        Ok(f) => f,
        Err(e) => return fail(&format!("{}: {e}", file.display())),
    };
    let dir = full.parent().unwrap_or(Path::new("."));
    let Some(root) = disk::package_root(dir) else {
        return fail(&no_package("build"));
    };
    let Ok(rel) = full.strip_prefix(&root) else {
        return fail("FILE is outside its package");
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    let program = match disk::load_package(&root, &rel) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let wasm = match compile(&program) {
        Ok(wasm) => wasm,
        Err(code) => return code,
    };
    let stem = full
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    write_module(&root, release, true, &stem, &wasm)
}
