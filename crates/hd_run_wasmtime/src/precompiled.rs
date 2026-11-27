//! The native-image boundary authorized by #2385: hd loads an artifact
//! the user selected or an artifact its build/install command produced.

use std::sync::Arc;

use hd_base::Hash128;
use hd_run::{LoadError, artifact::Native};

use crate::{EngineConfig, Module, RuntimeProfile, WasmtimeEngine};

pub(crate) fn engine(jobs: usize) -> Result<WasmtimeEngine, String> {
    let mut config = EngineConfig::pipeline(hd_base::OptLevel::Optimized);
    config.compilation_jobs = jobs;
    let mut options = crate::engine::wasmtime_config(config);
    options.compiler_inlining(wasmtime::Inlining::Yes);
    let engine = wasmtime::Engine::new(&options).map_err(|e| format!("{e:#}"))?;
    Ok(WasmtimeEngine {
        config,
        engine: Arc::new(engine),
        profile: RuntimeProfile::Default,
    })
}

/// The native compiler's version, target and compilation settings.
pub fn precompile_key() -> Result<Hash128, String> {
    engine(1).map(|engine| crate::code_cache::salt(&engine.engine))
}

/// Compiles a release image, without the dev/test function cache.
pub fn precompile(wasm: &[u8], jobs: usize) -> Result<Vec<u8>, String> {
    let engine = engine(jobs)?;
    let _trace = hd_base::trace::span("phase", || {
        format!("wasmtime precompile Speed Yes jobs={jobs}")
    });
    crate::code_cache::with_compiler_pool(jobs, || engine.engine.precompile_module(wasm))
        .map_err(|e| format!("{e:#}"))
}

/// Loads the same native image for an embedder's ordinary host registration.
///
/// # Safety
/// The image must be produced by a compatible native compiler and remain
/// unchanged while its code is mapped.
pub unsafe fn load(native: &Native) -> Result<(WasmtimeEngine, Module), String> {
    let engine = engine(1)?;
    crate::host::start_clock();
    let package = engine.package(&native.metadata).map_err(|e| match e {
        LoadError::NotHd(message) | LoadError::Engine(message) => message,
    })?;
    let _trace = hd_base::trace::span("phase", || "wasmtime deserialize native image".into());
    // SAFETY: the caller chose this native-code artifact, or hd produced
    // it. The open handle is the same file that supplied its metadata;
    // build/install publish new files instead of rewriting mapped images.
    let module = unsafe { wasmtime::Module::deserialize_open_file(&engine.engine, native.file()?) }
        .map_err(|e| format!("incompatible native module: {e:#}; rebuild with `hd build --release`, or build `--format wasm` for a portable module"))?;
    let module = Module(
        Arc::new(module),
        crate::checked_report::Report::read(&native.metadata),
        package,
    );
    Ok((engine, module))
}
