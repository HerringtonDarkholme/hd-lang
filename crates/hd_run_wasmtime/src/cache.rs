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
//! These dev/test caches hold modules in process only. Their machine code
//! persists function by function when `code_cache.rs` is installed. Native
//! release artifacts use the separate `precompiled.rs` boundary and bypass
//! both caches.

use hd_base::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::{EngineConfig, OptLevel};

/// Everything that reaches `wasmtime::Config` on the on-demand path and
/// can change generated code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct OnDemandKey {
    opt: OptLevel,
    max_stack: usize,
    compilation_jobs: usize,
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
    /// The engine's salt when the code cache is on for it.
    code: Option<hd_base::Hash128>,
    compilation_jobs: usize,
}

/// Fits in the suite's noise: the suite compiles about a thousand
/// distinct modules, so almost every load is a miss either way; the bound
/// pays off wherever bytes repeat.
pub const CAPACITY: usize = 128;

#[derive(Default)]
struct CacheState {
    map: HashMap<Arc<[u8]>, Arc<wasmtime::Module>>,
    /// Least-recently used first; every hit moves its key to the back.
    order: VecDeque<Arc<[u8]>>,
}

impl ModuleCache {
    /// The cache of `engine`, which `code` says was made with the code
    /// cache on (`code_cache::configure`).
    pub(crate) fn new(engine: &wasmtime::Engine, code: bool, compilation_jobs: usize) -> Self {
        Self {
            modules: Mutex::new(CacheState::default()),
            code: code.then(|| crate::code_cache::salt(engine)),
            compilation_jobs,
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
        let compiled = match self.code {
            Some(salt) => crate::code_cache::compile(engine, salt, wasm, self.compilation_jobs)?,
            None => crate::code_cache::compile_module(engine, wasm, self.compilation_jobs)?,
        };
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
    ON_DEMAND.get_or_init(|| Mutex::new(HashMap::default()))
}

/// The shared on-demand engine for `config`, creating it on first use.
pub(crate) fn ondemand(config: &EngineConfig) -> Result<Arc<Shared>, String> {
    let key = OnDemandKey {
        opt: config.cranelift_opt,
        max_stack: config.max_wasm_stack,
        compilation_jobs: config.compilation_jobs.max(1),
    };
    if let Some(shared) = on_demand_engines()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return Ok(Arc::clone(shared));
    }
    let mut wasmtime_config = crate::wasmtime_config(*config);
    let code = crate::code_cache::configure(&mut wasmtime_config);
    let engine = Arc::new(wasmtime::Engine::new(&wasmtime_config).map_err(|e| e.to_string())?);
    let mut engines = on_demand_engines()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(shared) = engines.get(&key) {
        return Ok(Arc::clone(shared));
    }
    let shared = Arc::new(Shared {
        cache: ModuleCache::new(&engine, code, config.compilation_jobs),
        engine,
    });
    engines.insert(key, Arc::clone(&shared));
    Ok(shared)
}
