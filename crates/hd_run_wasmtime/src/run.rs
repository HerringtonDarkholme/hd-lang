use std::path::Path;
use std::time::Duration;

use super::engine::{EngineConfig, WasmtimeEngine, thread_stack};
use crate::cancel::HostInterrupt;
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
pub struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

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
    #[must_use]
    pub fn take(&self) -> Vec<u8> {
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
    run_profile(config, profile, wasm, setup, io, None).map(|(o, _, _)| o)
}

/// [`run_captured_with_profile`] on the default profile, and the value of
/// the module's exported `i32` global `global` when the run ends, such as
/// the `join!` child poll count `hd.all_polls` (`crates/hd_wasm/DESIGN.md`
/// §14.5).
pub fn run_captured_reading(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    global: &str,
) -> Result<(Captured, Option<i32>), String> {
    let (out, err) = (Shared::default(), Shared::default());
    let io = HostIo {
        out: Box::new(out.clone()),
        err: Box::new(err.clone()),
        input: None,
    };
    let (outcome, _, value) = run_profile(
        config,
        RuntimeProfile::Default,
        wasm,
        setup,
        io,
        Some(global),
    )?;
    let status = match outcome {
        Outcome::Exit(code) => code,
        Outcome::Panic(p) => return Err(format!("panic: {}: {}", p.category, p.message)),
        Outcome::Internal(e) => return Err(e),
    };
    let captured = Captured {
        stdout: out.take(),
        stderr: err.take(),
        status,
    };
    Ok((captured, value))
}

/// [`run`], and with the Wasm frames of the trap the run ended in,
/// innermost first as function index and module byte offset (empty when
/// it ended any other way), which the caller symbolizes through the
/// module's line table (`flow.panic.report`).
pub fn run_traced(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    run_profile(config, RuntimeProfile::Default, wasm, setup, io, None)
        .map(|(o, frames, _)| (o, frames))
}

/// [`run_traced`] for a run that `interrupt` may cancel, as a Ctrl-C does
/// (`cli.cancel.unhandled-ctrl-c`): a request while the program waits on the host
/// cancels its entry, and the next one, or `bound` after that cancel,
/// abandons it; a request while Wasm runs stops it (`cancel.rs`).
pub fn run_cancellable(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
    interrupt: &HostInterrupt,
    bound: Duration,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    run_image(
        config,
        RuntimeProfile::Default,
        hd_run::artifact::Image::Wasm(wasm),
        setup,
        io,
        RunOptions {
            cancel: Some((interrupt, bound)),
            ..RunOptions::default()
        },
    )
    .map(|(o, frames, _)| (o, frames))
}

/// How a run ended, the trap's frames, and the global it was asked to read.
type Ran = (Outcome, Vec<(u32, u32)>, Option<i32>);

fn run_profile(
    config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
    read: Option<&str>,
) -> Result<Ran, String> {
    run_image(
        config,
        profile,
        hd_run::artifact::Image::Wasm(wasm),
        setup,
        io,
        RunOptions {
            read,
            ..RunOptions::default()
        },
    )
}

/// Runs an explicitly selected native image through the ordinary host,
/// cancellation and frame-reporting path, without Wasm translation.
///
/// # Safety
/// The image must be produced by a compatible native compiler and must
/// remain unchanged while its code is mapped.
#[allow(unsafe_code)]
pub unsafe fn run_native(
    native: &hd_run::artifact::Native,
    setup: &HostSetup,
    io: HostIo,
    cancel: Option<(&HostInterrupt, Duration)>,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    run_image(
        EngineConfig::pipeline(hd_base::OptLevel::Optimized),
        RuntimeProfile::Default,
        hd_run::artifact::Image::Native(native),
        setup,
        io,
        RunOptions {
            cancel,
            ..RunOptions::default()
        },
    )
    .map(|(outcome, frames, _)| (outcome, frames))
}

/// Records Firefox guest samples while running portable Wasm.
pub fn run_sampled(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
    output: &Path,
    cancel: Option<(&HostInterrupt, Duration)>,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    run_image(
        config,
        RuntimeProfile::Default,
        hd_run::artifact::Image::Wasm(wasm),
        setup,
        io,
        RunOptions {
            sample: Some(output),
            cancel,
            ..RunOptions::default()
        },
    )
    .map(|(outcome, frames, _)| (outcome, frames))
}

/// Records Firefox guest samples from an explicitly selected native image.
///
/// # Safety
/// The image must meet the same compatibility and immutability conditions
/// as [`run_native`]. Profiling changes no native-image trust boundary.
#[allow(unsafe_code)]
pub unsafe fn run_native_sampled(
    native: &hd_run::artifact::Native,
    setup: &HostSetup,
    io: HostIo,
    output: &Path,
    cancel: Option<(&HostInterrupt, Duration)>,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    run_image(
        EngineConfig::pipeline(hd_base::OptLevel::Optimized),
        RuntimeProfile::Default,
        hd_run::artifact::Image::Native(native),
        setup,
        io,
        RunOptions {
            sample: Some(output),
            cancel,
            ..RunOptions::default()
        },
    )
    .map(|(outcome, frames, _)| (outcome, frames))
}

#[derive(Clone, Copy, Default)]
struct RunOptions<'a> {
    read: Option<&'a str>,
    cancel: Option<(&'a HostInterrupt, Duration)>,
    sample: Option<&'a Path>,
    /// The libraries of the module's C-bound root traits.
    c: &'a [crate::native::c::Library],
}

/// Runs `image` as the entry points above do, with `c`, the libraries the
/// invoker selected for the module's C-bound root traits
/// (`cli.host.c.file`): before the guest starts, each is opened, its entry
/// symbol checked and its provider created, and the instance's close
/// destroys it (`crate::native::c`).
///
/// # Safety
/// A native image must meet [`run_native`]'s conditions. Each library runs
/// with the process's authority, which the app's root consent granted.
#[allow(unsafe_code)]
pub unsafe fn run_with_c_providers(
    config: EngineConfig,
    image: hd_run::artifact::Image<'_>,
    setup: &HostSetup,
    io: HostIo,
    c: &[crate::native::c::Library],
    cancel: Option<(&HostInterrupt, Duration)>,
    sample: Option<&Path>,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    run_image(
        config,
        RuntimeProfile::Default,
        image,
        setup,
        io,
        RunOptions {
            cancel,
            sample,
            c,
            ..RunOptions::default()
        },
    )
    .map(|(outcome, frames, _)| (outcome, frames))
}

#[allow(unsafe_code)]
fn run_image(
    config: EngineConfig,
    profile: RuntimeProfile,
    image: hd_run::artifact::Image<'_>,
    setup: &HostSetup,
    io: HostIo,
    options: RunOptions<'_>,
) -> Result<Ran, String> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("hd-wasmtime".into())
            .stack_size(thread_stack(&config))
            .spawn_scoped(scope, move || {
                let (engine, module) = match image {
                    hd_run::artifact::Image::Wasm(wasm) => {
                        let engine = WasmtimeEngine::new_with_profile(config, profile)?;
                        let module = engine
                            .load(wasm, &hd_cache::MemoryStore::default())
                            .map_err(|e| match e {
                                LoadError::NotHd(m) | LoadError::Engine(m) => m,
                            })?;
                        (engine, module)
                    }
                    hd_run::artifact::Image::Native(native) => {
                        // SAFETY: Native reaches this private function only
                        // through the native entry points, whose callers guarantee the image.
                        unsafe { crate::precompiled::load(native)? }
                    }
                };
                let sampling = options
                    .sample
                    .map(|path| {
                        crate::sampling::Sampling::start(
                            &engine,
                            &module,
                            &setup.program,
                            path,
                            match image {
                                hd_run::artifact::Image::Wasm(wasm) => wasm,
                                hd_run::artifact::Image::Native(native) => &native.metadata,
                            },
                        )
                    })
                    .transpose()?;
                let started = engine.instantiate_run(
                    &module,
                    setup,
                    io,
                    options.cancel,
                    sampling
                        .as_ref()
                        .map(|sampling| std::sync::Arc::clone(&sampling.profiler)),
                    options.c,
                );
                let mut inst = started.map_err(|e| match e {
                    StartError::Refused(m) | StartError::Engine(m) => m,
                })?;
                let outcome = match inst.init() {
                    Outcome::Exit(0) => hd_run::drive(&mut inst, usize::MAX),
                    other => other,
                };
                if let Some(sampling) = sampling {
                    sampling.finish()?;
                }
                let value = options.read.and_then(|name| inst.global_i32(name));
                Ok((outcome, std::mem::take(&mut inst.last_frames), value))
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the wasmtime thread panicked".to_owned())?
    })
}
