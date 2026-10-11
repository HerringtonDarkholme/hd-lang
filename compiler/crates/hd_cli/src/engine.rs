//! Which engine runs a program: V8 through Node (`node::NodeEngine`, the
//! default) or wasmtime in-process (`hd_run_wasmtime`), chosen by
//! `--engine NAME` or else `HD_ENGINE`. wasmtime runs the default
//! profile's `Console`, `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`,
//! `FsWrite`, `Process`, `Random`, `Sys`, `Http`, `Net` and their handle traits; a
//! program that imports another host trait is refused with a message that
//! names it (engines-and-test-runner.md §18, W2/W3).

use std::ffi::OsString;

use hd_cache::DiskStore;
use hd_run::{HostSetup, Outcome, run_program};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EngineKind {
    #[default]
    Node,
    Wasmtime,
}

fn parse(name: &str) -> Result<EngineKind, String> {
    match name {
        "node" => Ok(EngineKind::Node),
        "wasmtime" => Ok(EngineKind::Wasmtime),
        _ => Err(format!(
            "`{name}` is no engine; the engines are `node` and `wasmtime`"
        )),
    }
}

/// `--engine NAME` or `--engine=NAME` at `args[i]`, and the words it used.
pub(crate) fn engine_flag(
    args: &[OsString],
    i: usize,
) -> Option<Result<(EngineKind, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    let (name, used) = if text == "--engine" {
        match args.get(i + 1) {
            Some(v) => (v.to_string_lossy().into_owned(), 2),
            None => return Some(Err("`--engine` needs NAME".to_owned())),
        }
    } else {
        (text.strip_prefix("--engine=")?.to_owned(), 1)
    };
    Some(parse(&name).map(|k| (k, used)))
}

/// The engine of a run with no `--engine` flag: `HD_ENGINE`, else Node.
pub(crate) fn from_env() -> Result<EngineKind, String> {
    match std::env::var("HD_ENGINE") {
        Ok(name) if !name.is_empty() => parse(&name).map_err(|e| format!("HD_ENGINE: {e}")),
        _ => Ok(EngineKind::Node),
    }
}

/// Runs a built program to its end on `kind`, with the process's standard
/// streams. `release` selects wasmtime's optimized pipeline.
pub(crate) fn run(
    kind: EngineKind,
    release: bool,
    wasm: &[u8],
    host: &HostSetup,
) -> Result<Outcome, String> {
    match kind {
        EngineKind::Node => {
            let store = DiskStore {
                root: crate::cache_dir(),
            };
            run_program(&crate::node::NodeEngine, wasm, &store, host).map_err(|e| e.to_string())
        }
        EngineKind::Wasmtime => hd_run_wasmtime::run(
            hd_run_wasmtime::EngineConfig::pipeline(release),
            wasm,
            host,
            hd_run_wasmtime::HostIo::process(),
        ),
    }
}
