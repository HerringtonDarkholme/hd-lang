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

use std::collections::{HashMap, VecDeque};
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

/// Compiled modules for one engine, bound to a small LRU so a process
/// that loads many distinct modules (a future REPL reading one program
/// per input) cannot retain all of them. Lookups hash and compare the
/// source bytes; compilation happens outside the map lock so parallel
/// workers compiling distinct modules never serialize, and a lost race
/// compiles twice but still shares one winner.
///
/// Why an entry count, and why this size: adjacent repeats (the
/// pending-first-poll pairs, compiled twice in a row) hit with any bound
/// past one; 128 entries is about sixteen full rounds of eight workers,
/// so recurring modules (named children reused across cases) usually
/// survive the interleaving; and 128 artifacts bound our retention to
/// tens of megabytes worst case. Eviction drops only our reference:
/// live instances keep their code alive through wasmtime itself, and the
/// engine's own code memory is wasmtime's domain, not ours.
pub(crate) struct ModuleCache {
    modules: Mutex<CacheState>,
}

/// Fits in the suite's noise: the suite compiles about a thousand
/// distinct modules, so almost every load is a miss either way; the bound
/// pays off wherever bytes repeat.
const CAPACITY: usize = 128;

#[derive(Default)]
struct CacheState {
    map: HashMap<Arc<[u8]>, Arc<wasmtime::Module>>,
    /// Least-recently used first; every hit moves its key to the back.
    order: VecDeque<Arc<[u8]>>,
}

impl ModuleCache {
    pub(crate) fn new() -> Self {
        Self {
            modules: Mutex::new(CacheState::default()),
        }
    }

    pub(crate) fn get_or_compile(
        &self,
        engine: &wasmtime::Engine,
        wasm: &[u8],
    ) -> Result<Arc<wasmtime::Module>, wasmtime::Error> {
        {
            let mut state = self.modules.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(hit) = state.map.get(wasm).map(Arc::clone) {
                Self::touch(&mut state.order, wasm);
                return Ok(hit);
            }
        }
        let compiled = wasmtime::Module::new(engine, wasm)?;
        let compiled = Arc::new(compiled);
        let mut state = self.modules.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(hit) = state.map.get(wasm).map(Arc::clone) {
            Self::touch(&mut state.order, wasm);
            return Ok(hit);
        }
        let key: Arc<[u8]> = wasm.into();
        state.map.insert(Arc::clone(&key), Arc::clone(&compiled));
        state.order.push_back(key);
        while state.map.len() > CAPACITY {
            if let Some(oldest) = state.order.pop_front() {
                state.map.remove(&oldest);
            } else {
                break;
            }
        }
        Ok(compiled)
    }

    fn touch(order: &mut VecDeque<Arc<[u8]>>, wasm: &[u8]) {
        if let Some(position) = order.iter().position(|key| &key[..] == wasm) {
            let key = order.remove(position).expect("found position");
            order.push_back(key);
        }
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

    fn numbered_module(n: u32) -> Vec<u8> {
        // Distinct import-free bytes per number.
        let data = n.to_le_bytes();
        let mut module = wasm_encoder::Module::new();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed("m31-probe"),
            data: data.as_slice().into(),
        });
        module.finish()
    }

    #[test]
    fn entries_past_the_bound_evict_oldest_first() {
        const EXTRA: u32 = 8;
        let total = u32::try_from(super::CAPACITY).unwrap() + EXTRA;
        let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let store = MemoryStore::default();
        // Fill past the bound with distinct modules, hitting each once.
        let first = engine.load(&numbered_module(0), &store).unwrap();
        for n in 1..total {
            engine.load(&numbered_module(n), &store).unwrap();
        }
        // The oldest entry evicted: reloading recompiles.
        let recompiled = engine.load(&numbered_module(0), &store).unwrap();
        assert!(!Arc::ptr_eq(&first.0, &recompiled.0));
        // A recent entry still hits the shared compilation, twice in a row.
        let last = total - 1;
        let kept = engine.load(&numbered_module(last), &store).unwrap();
        assert!(Arc::ptr_eq(
            &kept.0,
            &engine.load(&numbered_module(last), &store).unwrap().0
        ));
    }
}
