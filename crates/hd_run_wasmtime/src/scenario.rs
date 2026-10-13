//! The conformance runtime scenarios a host drives by hand: the same
//! steps the Node worker drives, over the same scenario exports, so both
//! engines judge a scenario fixture alike.

use hd_run::{Engine, HostSetup, Instance, Outcome};

use crate::engine::{WasmtimeEngine, thread_stack};
use crate::{EngineConfig, RuntimeProfile};

/// A runtime scenario the host drives instead of running to completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Poll pending, cancel the root, and read the cleanup: passes when
    /// the cleanup reports true.
    CancellationCleanup,
    /// Poll pending as one driver, then poll as another: passes when the
    /// second poll raises `suspension-competing-driver`.
    CompetingDrivers,
}

/// What a scenario drive captured: the streams, and the status the
/// verdict reads (0 for a cleanup that ran, 3 for a competing poll's
/// trap as a trapped run reports, 1 for any other failure).
pub struct ScenarioRan {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: i32,
}

/// Drives `scenario` over `wasm`'s scenario exports, with the fixture's
/// runtime profile, and returns what the run captured.
pub fn drive_scenario(
    mut config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
    scenario: Scenario,
) -> Result<ScenarioRan, String> {
    if setup.limits.memory_bytes != u64::MAX {
        config.gc_heap_initial = 0;
    }
    let (out, err) = (crate::Shared::default(), crate::Shared::default());
    let status = std::thread::scope(|scope| {
        let (io_out, io_err) = (out.clone(), err.clone());
        std::thread::Builder::new()
            .name("hd-wasmtime-scenario".into())
            .stack_size(thread_stack(&config))
            .spawn_scoped(scope, move || {
                drive(config, profile, wasm, setup, &io_out, &io_err, scenario)
            })
            .map_err(|e| format!("a scenario drive did not start: {e}"))?
            .join()
            .map_err(|_| "a scenario drive panicked".to_owned())?
    })?;
    Ok(ScenarioRan {
        stdout: out.take(),
        stderr: err.take(),
        status,
    })
}

fn drive(
    config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
    out: &crate::Shared,
    err: &crate::Shared,
    scenario: Scenario,
) -> Result<i32, String> {
    use hd_run::{LoadError, StartError};

    let engine = WasmtimeEngine::new_with_profile(config, profile)?;
    let module = engine
        .load(wasm, &hd_cache::MemoryStore::default())
        .map_err(|e| match e {
            LoadError::NotHd(m) | LoadError::Engine(m) => m,
        })?;
    let io = crate::HostIo {
        out: Box::new(out.clone()),
        err: Box::new(err.clone()),
        input: None,
    };
    let mut inst = engine
        .instantiate_with(&module, setup, io)
        .map_err(|e| match e {
            StartError::Refused(m) | StartError::Engine(m) => m,
        })?;
    if !matches!(inst.init(), Outcome::Exit(0)) {
        return Ok(1);
    }
    match scenario {
        Scenario::CancellationCleanup => {
            match inst.poll_with(0) {
                Ok(-1) => {}
                Ok(_) => return Ok(1),
                Err(e) => return Ok(trapped(&mut inst, &e)),
            }
            if let Err(e) = inst.cancel_root() {
                return Ok(trapped(&mut inst, &e));
            }
            match inst.cleanup_call() {
                Ok(1) => Ok(0),
                Ok(_) => Ok(1),
                Err(e) => Ok(trapped(&mut inst, &e)),
            }
        }
        Scenario::CompetingDrivers => {
            match inst.poll_with(0) {
                Ok(-1) => {}
                Ok(_) => return Ok(1),
                Err(e) => return Ok(trapped(&mut inst, &e)),
            }
            match inst.poll_with(1) {
                Ok(_) => Ok(1),
                Err(e) => Ok(trapped(&mut inst, &e)),
            }
        }
    }
}

/// Ends a trapped drive as a trapped run ends: the guest's report is
/// already on standard error, and the status is 3.
fn trapped(inst: &mut crate::WasmtimeInstance, e: &wasmtime::Error) -> i32 {
    match inst.trapped(e) {
        Outcome::Exit(status) => i32::from(status),
        _ => 1,
    }
}
