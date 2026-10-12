use super::engine::{EngineConfig, WasmtimeEngine, thread_stack};
use crate::host::HostIo;
use crate::profiles::RuntimeProfile;
use hd_run::{Engine, HostSetup, Instance, LoadError, Outcome, StartError};

/// The streams and status of a run whose output a caller keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: u8,
}

/// A shared buffer that a run writes one stream to.
#[derive(Clone, Default)]
pub(crate) struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("poisoned"))?
            .extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shared {
    pub(crate) fn take(&self) -> Vec<u8> {
        self.0
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

/// Runs `wasm` to its end on a thread of its own, with its standard
/// output and error kept and its standard input closed: what `hd run`
/// prints and its status, for tests and the conformance runner. `Err` is
/// a module wasmtime does not run (an import not on wasmtime yet, or a
/// module it rejects).
pub fn run_captured(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
) -> Result<Captured, String> {
    run_captured_with_profile(config, RuntimeProfile::Default, wasm, setup)
}

/// Runs a module with explicitly selected conformance fixture providers.
/// Standard streams are captured and standard input is closed.
pub fn run_captured_with_profile(
    config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
) -> Result<Captured, String> {
    let (out, err) = (Shared::default(), Shared::default());
    let io = HostIo {
        out: Box::new(out.clone()),
        err: Box::new(err.clone()),
        input: None,
    };
    let outcome = run_with_profile(config, profile, wasm, setup, io)?;
    let status = match outcome {
        Outcome::Exit(code) => code,
        Outcome::Panic(p) => {
            return Err(format!("panic: {}: {}", p.category, p.message));
        }
        Outcome::Internal(e) => return Err(e),
    };
    Ok(Captured {
        stdout: out.take(),
        stderr: err.take(),
        status,
    })
}

/// Runs `wasm` to its end on a thread with the stack it needs, its
/// standard streams on `io`.
pub fn run(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
) -> Result<Outcome, String> {
    run_with_profile(config, RuntimeProfile::Default, wasm, setup, io)
}

/// Runs a module with explicitly selected conformance fixture providers and
/// caller-supplied standard streams, on a thread with the required stack.
pub fn run_with_profile(
    config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
) -> Result<Outcome, String> {
    run_profile_with_guards(config, profile, wasm, setup, io, || (), || ())
}

/// Runs with caller-owned guards around native compilation and guest execution.
/// The guest guard remains alive until the instance and its host are dropped.
pub fn run_with_guards<P, G>(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
    preparing: impl FnOnce() -> P + Send,
    guest: impl FnOnce() -> G + Send,
) -> Result<Outcome, String> {
    run_profile_with_guards(
        config,
        RuntimeProfile::Default,
        wasm,
        setup,
        io,
        preparing,
        guest,
    )
}

fn run_profile_with_guards<P, G>(
    mut config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
    preparing: impl FnOnce() -> P + Send,
    guest: impl FnOnce() -> G + Send,
) -> Result<Outcome, String> {
    // A bounded store grows lazily rather than reserving the profile's
    // initial 64 MiB before the guest has allocated anything.
    if setup.limits.memory_bytes != u64::MAX {
        config.gc_heap_initial = 0;
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("hd-wasmtime".into())
            .stack_size(thread_stack(&config))
            .spawn_scoped(scope, move || {
                let preparing = preparing();
                let engine = WasmtimeEngine::new_with_profile(config, profile)?;
                let module = engine
                    .load(wasm, &hd_cache::MemoryStore::default())
                    .map_err(|e| match e {
                        LoadError::NotHd(m) | LoadError::Engine(m) => m,
                    })?;
                drop(preparing);
                let _guest = guest();
                let mut inst =
                    engine
                        .instantiate_with(&module, setup, io)
                        .map_err(|e| match e {
                            StartError::Refused(m) | StartError::Engine(m) => m,
                        })?;
                Ok(match inst.init() {
                    Outcome::Exit(0) => hd_run::drive(&mut inst, usize::MAX),
                    other => other,
                })
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the wasmtime thread panicked".to_owned())?
    })
}
