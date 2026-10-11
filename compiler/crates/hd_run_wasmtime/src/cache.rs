//! Compiled-module caches for both engine paths.
//!
//! `Engine::load` used to call `wasmtime::Module::new` on every call and
//! `TestEngine::compile` likewise, so identical bytes compiled again and
//! again (pending-first-poll fixtures twice; test-harness cases twice, in
//! two forms). These caches compile identical bytes once.
//!
//! A `wasmtime::Module` may only run on the engine that compiled it
//! (`Engine::same` is pointer equality), so sharing is engine-anchored:
//! a cache belongs to one engine and is only consulted with that engine.
//! Hash collisions cannot alias modules: the map key is the source bytes
//! themselves, compared on every lookup.
//!
//! Scope differs per path by design. The on-demand path has no capacity
//! bounds, so its engines are shared process-wide per configuration and
//! every on-demand user shares one module cache per configuration. The
//! pooled path bounds simultaneous stores per engine, so each
//! `TestEngine` keeps its own cache: sharing the pool would silently
//! merge independent capacity contracts. (The suite runs one process-wide
//! `TestEngine`, so its repeats still compile once.)
//!
//! The caches are intentionally in-process only. wasmtime's serialized
//! modules are `unsafe` to deserialize, so no cross-run artifact cache.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::{EngineConfig, OptLevel};

/// Everything that reaches `wasmtime::Config` on the on-demand path and
/// can change generated code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct OnDemandKey {
    opt: OptLevel,
    max_stack: usize,
    gc_init: u64,
}

/// Compiled modules for one engine. Lookups hash and compare the source
/// bytes; compilation happens outside the map lock so parallel workers
/// compiling distinct modules never serialize, and a lost race compiles
/// twice but still shares one winner.
pub(crate) struct ModuleCache {
    modules: Mutex<HashMap<Arc<[u8]>, Arc<wasmtime::Module>>>,
}

impl ModuleCache {
    pub(crate) fn new() -> Self {
        Self {
            modules: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn get_or_compile(
        &self,
        engine: &wasmtime::Engine,
        wasm: &[u8],
    ) -> Result<Arc<wasmtime::Module>, wasmtime::Error> {
        if let Some(hit) = self
            .modules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(wasm)
        {
            return Ok(Arc::clone(hit));
        }
        let compiled = wasmtime::Module::new(engine, wasm)?;
        let compiled = Arc::new(compiled);
        let mut modules = self.modules.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(hit) = modules.get(wasm) {
            return Ok(Arc::clone(hit));
        }
        modules.insert(wasm.into(), Arc::clone(&compiled));
        Ok(compiled)
    }
}

/// One shared on-demand engine and its module cache.
pub(crate) struct Shared {
    pub(crate) engine: Arc<wasmtime::Engine>,
    pub(crate) cache: ModuleCache,
}

static ON_DEMAND: OnceLock<Mutex<HashMap<OnDemandKey, Arc<Shared>>>> = OnceLock::new();

fn on_demand_engines() -> &'static Mutex<HashMap<OnDemandKey, Arc<Shared>>> {
    ON_DEMAND.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The shared on-demand engine for `config`, creating it on first use.
pub(crate) fn ondemand(config: &EngineConfig) -> Result<Arc<Shared>, String> {
    let key = OnDemandKey {
        opt: config.cranelift_opt,
        max_stack: config.max_wasm_stack,
        gc_init: config.gc_heap_initial,
    };
    if let Some(shared) = on_demand_engines()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return Ok(Arc::clone(shared));
    }
    let engine = Arc::new(
        wasmtime::Engine::new(&crate::wasmtime_config(*config)).map_err(|e| e.to_string())?,
    );
    let mut engines = on_demand_engines()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(shared) = engines.get(&key) {
        return Ok(Arc::clone(shared));
    }
    let shared = Arc::new(Shared {
        engine,
        cache: ModuleCache::new(),
    });
    engines.insert(key, Arc::clone(&shared));
    Ok(shared)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{EngineConfig, WasmtimeEngine};
    use hd_cache::MemoryStore;
    use hd_run::Engine as _;

    fn empty_module() -> Vec<u8> {
        wasm_encoder::Module::new().finish()
    }

    fn other_module() -> Vec<u8> {
        // Same shape, different bytes: import-free, so `load` accepts it.
        let mut module = wasm_encoder::Module::new();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed("m29-probe"),
            data: std::borrow::Cow::Borrowed(b"different".as_slice()),
        });
        module.finish()
    }

    #[test]
    fn identical_bytes_compile_once_across_wrappers() {
        let bytes = empty_module();
        let store = MemoryStore::default();
        // Two separate wrappers, one shared engine: one compilation.
        let first = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let second = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let a = first.load(&bytes, &store).unwrap();
        let b = second.load(&bytes, &store).unwrap();
        assert!(Arc::ptr_eq(&a.0, &b.0));
        // Different bytes still compile separately.
        let other = first.load(&other_module(), &store).unwrap();
        assert!(!Arc::ptr_eq(&a.0, &other.0));
    }

    #[test]
    fn distinct_configs_compile_separately() {
        // A different engine config must not alias the default's artifact.
        let bytes = empty_module();
        let store = MemoryStore::default();
        let default_engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let release_engine = WasmtimeEngine::new(EngineConfig::pipeline(true)).unwrap();
        let a = default_engine.load(&bytes, &store).unwrap();
        let b = release_engine.load(&bytes, &store).unwrap();
        assert!(!Arc::ptr_eq(&a.0, &b.0));
    }
}
