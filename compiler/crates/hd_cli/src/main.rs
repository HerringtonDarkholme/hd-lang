#![forbid(unsafe_code)]
//! The `hd` command. It owns everything outside the core (design-overview
//! §2.2): the file system (`DiskSources`, the disk `CacheStore`), the clock,
//! the executor and the engine (`node::NodeEngine`, an `hd_run::Engine`).

mod disk;
mod node;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_driver::{Clock, Executor, Goal, Host, Output, build};
use hd_run::{Grants, HostSetup, Limits, Outcome, run_program};

const USAGE: &str = "usage:
  hd FILE.hd
  hd run FILE.hd|DIR
  hd build FILE.hd|DIR -o OUT.wasm";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(command) = arguments.first().and_then(|c| c.to_str()) else {
        return usage();
    };
    match (command, &arguments[1..]) {
        ("run", [target]) => run_command(Path::new(target)),
        ("build", [target, flag, out]) if flag == "-o" => {
            build_command(Path::new(target), Path::new(out))
        }
        (file, [])
            if Path::new(file)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("hd")) =>
        {
            run_command(Path::new(file))
        }
        _ => usage(),
    }
}

struct Wall(Instant);

impl Clock for Wall {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// The compiled-cache directory (`cli.cache.obj`): `obj` under `HD_CACHE`,
/// or under the platform's user cache directory followed by `hd`.
fn cache_dir() -> PathBuf {
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

fn executor() -> Executor {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    if threads > 1 {
        Executor::Pool(threads)
    } else {
        Executor::Serial(hd_sched::SerialOrder::Priority)
    }
}

/// Compiles a file or a directory to Wasm bytes, printing diagnostics.
fn compile(target: &Path) -> Result<Vec<u8>, ExitCode> {
    let program = disk::load(target).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::from(2)
    })?;
    let store = DiskStore { root: cache_dir() };
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store: &store,
        clock: &clock,
        executor: executor(),
    };
    let out: Output = build(
        &host,
        &program.package,
        &Goal::Program {
            entry: program.entry.clone(),
        },
    );
    if !out.diags.is_empty() {
        for line in out.render().lines() {
            eprintln!("error: {line}");
        }
        return Err(ExitCode::FAILURE);
    }
    out.wasm.ok_or_else(|| {
        eprintln!("error: no Wasm produced");
        ExitCode::FAILURE
    })
}

fn run_command(target: &Path) -> ExitCode {
    let wasm = match compile(target) {
        Ok(wasm) => wasm,
        Err(code) => return code,
    };
    let store = DiskStore { root: cache_dir() };
    let host = HostSetup {
        providers: Vec::new(),
        grants: Grants::default(),
        limits: Limits::default(),
    };
    match run_program(&node::NodeEngine, &wasm, &store, &host) {
        Ok(Outcome::Exit(0)) => ExitCode::SUCCESS,
        Ok(Outcome::Exit(code)) => ExitCode::from(code),
        Ok(Outcome::Panic(p)) => {
            eprintln!("panic: {}: {}", p.category, p.message);
            ExitCode::from(101)
        }
        Ok(Outcome::Internal(e)) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn build_command(target: &Path, out: &Path) -> ExitCode {
    let wasm = match compile(target) {
        Ok(wasm) => wasm,
        Err(code) => return code,
    };
    match std::fs::write(out, &wasm) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", out.display());
            ExitCode::FAILURE
        }
    }
}
