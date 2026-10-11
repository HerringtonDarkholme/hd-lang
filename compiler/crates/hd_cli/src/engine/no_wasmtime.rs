//! The wasmtime engine's place in a build without the `wasmtime` feature.
//! `--engine wasmtime` and `HD_ENGINE=wasmtime` are refused when the engine
//! name is read (`UNAVAILABLE`), so nothing below runs. Each item still
//! says the same thing, in case a caller gets past the check.

use hd_run::{HostSetup, Outcome};

use super::{CaseRun, HostCase, ProgramLimits, TestEnv, TestPrograms};

const MISSING: &str = "this build of hd has no wasmtime engine; rebuild with `--features wasmtime`";

/// Why this build lacks the engine.
pub(crate) const UNAVAILABLE: Option<&str> = Some(MISSING);

fn missing<T>() -> Result<T, String> {
    Err(MISSING.to_owned())
}

/// No test module is ever prepared, so none can be held.
pub(crate) enum Prepared {}

/// No child programs are ever held.
pub(crate) enum Programs {}

impl TryFrom<Vec<(String, Vec<u8>, hd_run::Grants)>> for Programs {
    type Error = String;

    fn try_from(_list: Vec<(String, Vec<u8>, hd_run::Grants)>) -> Result<Self, String> {
        missing()
    }
}

/// No session is ever started.
pub(crate) enum Session {}

impl Session {
    pub(crate) fn new(
        _release: bool,
        _jobs: usize,
        _limits: ProgramLimits,
        _programs: Option<&TestPrograms>,
    ) -> Result<Self, String> {
        missing()
    }

    pub(crate) fn prepare(&self, _wasm: &[u8], _integration: bool) -> Result<Prepared, String> {
        match *self {}
    }

    pub(crate) fn run(
        &self,
        _prepared: &Prepared,
        _env: &TestEnv<'_>,
        _cases: &[HostCase],
        _limits: ProgramLimits,
        _done: &mut dyn FnMut(CaseRun),
    ) -> Result<(), String> {
        match *self {}
    }
}

pub(crate) fn run_program(
    _release: bool,
    _wasm: &[u8],
    _host: &HostSetup,
) -> Result<Outcome, String> {
    missing()
}

/// No guest runs, so a worker thread keeps the default stack.
pub(crate) fn worker_stack(_release: bool) -> usize {
    0
}
