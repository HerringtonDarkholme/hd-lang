#![forbid(unsafe_code)]
//! The `hd` command. It owns everything outside the core (design-overview
//! §2.2): the file system (`DiskSources`, the disk `CacheStore`), the clock,
//! the executor and the engine (`node::NodeEngine`, an `hd_run::Engine`).

mod caps;
mod check_cmd;
mod clean_cmd;
mod dep_cmd;
mod disk;
mod fmt_cmd;
mod help;
mod new_cmd;
mod node;
mod report;
mod test_cmd;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_driver::{Clock, Executor, Goal, Host, Output, build_packages};
use hd_run::{HostSetup, Limits, Outcome, run_program};

use crate::report::Reporter;

const USAGE: &str = "usage:
  hd [--release] [--format json] [--cap NAME=VALUE]... FILE.hd [-- ARGS]
  hd [--cap NAME=VALUE]... FILE.wasm [-- ARGS]
  hd run [--release] [--format json] [--cap NAME=VALUE]... [NAME]
  hd build [--release] [--format json] [FILE.hd]
  hd check [FILE.hd] [--tests | --all] [--format json]
  hd new [--app | --lib] [--pages] [--vcs none] [PATH]
  hd clean [--cache]
  hd remove NAME
  hd fetch
  hd fmt [--check] [FILE.hd]
  hd test [FILE.hd] [--filter PATTERN] [--jobs N] [--format json]";

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
    // `hd COMMAND --help`, before the program arguments' `--`.
    let asks_help = arguments[1..]
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--help");
    if asks_help && let Some(text) = help::command(command) {
        print!("{text}");
        return ExitCode::SUCCESS;
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
        ("remove", rest) => dep_cmd::remove(rest),
        ("fetch", rest) => dep_cmd::fetch(rest),
        ("fmt", rest) => fmt_cmd::command(rest),
        ("run", rest) => run_command(rest),
        ("build", rest) => build_command(rest),
        ("check", rest) => check_cmd::command(rest),
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

/// Builds `goal` over a program's sources and reports its manifest's and
/// the compiler's diagnostics. Warnings are shown; only errors stop the
/// command, which then ends with status 101.
fn build_goal(
    program: &disk::Program,
    goal: &Goal,
    rep: &mut Reporter,
) -> Result<Output, ExitCode> {
    if rep.diags(&program.problems) {
        return Err(rep.finish(HD_FAILURE));
    }
    let store = DiskStore { root: cache_dir() };
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store: &store,
        clock: &clock,
        executor: executor(),
    };
    let out: Output = build_packages(&host, &program.package, &program.packages(), goal);
    let as_written = program.as_written.as_deref().filter(|_| rep.json);
    if rep.diags(&report::from_output(&out, &program.sources, as_written)) {
        return Err(rep.finish(HD_FAILURE));
    }
    Ok(out)
}

/// Compiles a program to Wasm bytes, reporting diagnostics.
fn compile(program: &disk::Program, rep: &mut Reporter) -> Result<Vec<u8>, ExitCode> {
    let goal = Goal::Program {
        entry: program.entry.clone(),
    };
    match build_goal(program, &goal, rep)?.wasm {
        Some(wasm) => Ok(wasm),
        None => Err(rep.fail("no Wasm produced")),
    }
}

/// Runs compiled Wasm and ends with the program's status
/// (`cli.exit.program`), unless a need is totally denied
/// (`cli.cap.total.refuse`).
fn execute(
    wasm: &[u8],
    table: &[(String, hd_project::Grant)],
    table_dir: Option<&Path>,
    flags: &[caps::CapFlag],
    args: &[OsString],
    rep: &mut Reporter,
) -> ExitCode {
    if let Some(d) = caps::refusal(wasm, table, flags) {
        rep.diag(&d);
        return rep.finish(HD_FAILURE);
    }
    let store = DiskStore { root: cache_dir() };
    let cwd = std::env::current_dir().unwrap_or_default();
    let host = HostSetup {
        providers: Vec::new(),
        grants: caps::grants(table, table_dir.unwrap_or(&cwd), flags),
        limits: Limits::default(),
        args: args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
    };
    match run_program(&node::NodeEngine, wasm, &store, &host) {
        Ok(Outcome::Exit(code)) => rep.finish(code),
        Ok(Outcome::Panic(p)) => {
            eprintln!("panic: {}: {}", p.category, p.message);
            rep.finish(HD_FAILURE)
        }
        Ok(Outcome::Internal(e)) => rep.fail(&e),
        Err(e) => rep.fail(&e.to_string()),
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
}

fn file_words(args: &[OsString]) -> Result<FileWords, String> {
    let mut w = FileWords {
        json: false,
        caps: Vec::new(),
        file: None,
        program_args: Vec::new(),
    };
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
        // `cli.profile.release.file`: accepted; the release profile is not
        // in codegen yet, so the build is the debug one.
        if args[i] == "--release" {
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
    Ok(w)
}

/// `hd [--release] [--format json] [--cap NAME=VALUE]... FILE.hd [-- ARGS]`
/// (`cli.file.run`): FILE as a single-file program.
fn run_file_command(args: &[OsString]) -> ExitCode {
    let Ok(w) = file_words(args) else {
        return usage();
    };
    let mut rep = Reporter::stderr(w.json);
    let Some(file) = &w.file else {
        return usage();
    };
    let program = match disk::load_file(file) {
        Ok(p) => p,
        Err(e) => return rep.fail(&e),
    };
    match compile(&program, &mut rep) {
        Ok(wasm) => execute(
            &wasm,
            &program.capabilities,
            program.package_dir.as_deref(),
            &w.caps,
            &w.program_args,
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
    execute(&wasm, &[], None, &w.caps, &w.program_args, &mut rep)
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
}

fn words(command: &str, args: &[OsString]) -> Result<Words, String> {
    let mut w = Words {
        release: false,
        json: false,
        caps: Vec::new(),
        positional: Vec::new(),
        program_args: Vec::new(),
    };
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
        if command == "run"
            && let Some(cap) = caps::cap_flag(args, i)
        {
            let (flag, used) = cap?;
            w.caps.push(flag);
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
    let root = match workspace_member(name.as_deref()) {
        Some(Ok(r)) => r,
        Some(Err(e)) => return rep.fail(&e),
        None => match package_of_cwd("run") {
            Ok(r) => r,
            Err(e) => return rep.fail(&e),
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
    let wasm = match compile(&program, &mut rep) {
        Ok(wasm) => wasm,
        Err(code) => return code,
    };
    if chosen.is_task
        && let Err(e) = std::env::set_current_dir(&root)
    {
        return rep.fail(&format!("{}: {e}", root.display()));
    }
    execute(
        &wasm,
        &program.capabilities,
        program.package_dir.as_deref(),
        &w.caps,
        &w.program_args,
        &mut rep,
    )
}

/// In workspace mode, the member whose executable or task `hd run NAME`
/// runs (`cli.workspace.run-name`, `.run-ambiguous`, `.run-missing`,
/// `.run-bare`); `None` outside workspace mode.
fn workspace_member(name: Option<&str>) -> Option<Result<PathBuf, String>> {
    let cwd = std::env::current_dir().ok()?;
    let (ws, members) = disk::workspace_root(&cwd)?;
    let rel = |dir: &Path| {
        dir.strip_prefix(&ws)
            .unwrap_or(dir)
            .to_string_lossy()
            .replace('\\', "/")
    };
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

/// `hd build [FILE]` (`cli.build.*`).
fn build_command(args: &[OsString]) -> ExitCode {
    let w = match words("build", args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(w.json);
    let built = match w.positional.as_slice() {
        [] => match std::env::current_dir()
            .ok()
            .and_then(|cwd| disk::workspace_root(&cwd))
        {
            // `cli.workspace.members`: each member in turn.
            Some((_, members)) => members
                .iter()
                .try_for_each(|root| build_package(root, w.release, &mut rep)),
            None => match package_of_cwd("build") {
                Ok(root) => build_package(&root, w.release, &mut rep),
                Err(e) => Err(rep.fail(&e)),
            },
        },
        [file] => build_file(Path::new(file), w.release, &mut rep),
        _ => Err(rep.fail("`hd build` takes at most one FILE")),
    };
    match built {
        Ok(()) => rep.finish(0),
        Err(code) => code,
    }
}

/// A whole-package `hd build`: each executable to its module; a package with
/// no executable is checked and writes no `.wasm` file.
fn build_package(root: &Path, release: bool, rep: &mut Reporter) -> Result<(), ExitCode> {
    let package = disk::package_name(root).map_err(|e| rep.fail(&e))?;
    let executables = disk::executables(root, &package);
    if executables.is_empty() {
        let mut program = disk::load_package(root, "main").map_err(|e| rep.fail(&e))?;
        program.only_program("");
        return build_goal(&program, &Goal::Analyze, rep).map(|_| ());
    }
    for exe in &executables {
        let mut program = disk::load_package(root, &exe.file).map_err(|e| rep.fail(&e))?;
        program.only_program(&exe.file);
        let wasm = compile(&program, rep)?;
        write_module(root, release, false, &exe.name, &wasm).map_err(|e| rep.fail(&e))?;
    }
    Ok(())
}

/// `hd build FILE`: the module of a file of the package, linked with the
/// rest of it. Its package is the one at or above FILE's directory
/// (`cli.mode.start`).
fn build_file(file: &Path, release: bool, rep: &mut Reporter) -> Result<(), ExitCode> {
    if file.extension().is_none_or(|x| x != "hd") {
        return Err(rep.fail(&format!(
            "`{}` is not an .hd file; `hd build` takes a FILE.hd",
            file.display()
        )));
    }
    let full =
        std::fs::canonicalize(file).map_err(|e| rep.fail(&format!("{}: {e}", file.display())))?;
    let dir = full.parent().unwrap_or(Path::new("."));
    let Some(root) = disk::package_root(dir) else {
        return Err(rep.fail(&no_package("build")));
    };
    let Ok(rel) = full.strip_prefix(&root) else {
        return Err(rep.fail("FILE is outside its package"));
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    let mut program = disk::load_package(&root, &rel).map_err(|e| rep.fail(&e))?;
    program.only_program(&rel);
    let wasm = compile(&program, rep)?;
    let stem = full
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    write_module(&root, release, true, &stem, &wasm).map_err(|e| rep.fail(&e))
}
