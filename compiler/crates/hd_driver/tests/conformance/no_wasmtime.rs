//! The wasmtime half of the conformance engine modes in a build without
//! the `wasmtime` feature. `EngineMode::from_value` refuses those modes, so
//! nothing here runs; each item says why, in case it is reached.

use super::{Case, Ran, Verdict};

const MISSING: &str = "this build has no wasmtime engine; rerun with --features wasmtime";

pub fn run_program(
    _case: &Case,
    _wasm: &[u8],
    _profile: Option<&str>,
    _node: Option<Ran>,
) -> std::io::Result<Ran> {
    Err(std::io::Error::other(MISSING))
}

pub fn test_stdout(
    _case: &Case,
    _wasm: &[u8],
    _runs: &[(&hd_driver::TestCase, (u32, u32))],
    _node: Option<&str>,
) -> Result<String, Verdict> {
    Err(Verdict::Crash(MISSING.to_owned()))
}

/// No guest runs, so a worker thread keeps the default stack.
pub fn worker_stack() -> usize {
    0
}
