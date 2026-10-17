use hd_base::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use hd_cache::MemoryStore;
use wasmtime::{InstanceAllocationStrategy, InstancePre, Linker, PoolingAllocationConfig};

use crate::host::{Host, define_test};
use crate::{EngineConfig, Module, RuntimeProfile, WasmtimeEngine};
use hd_run::{Engine, Grants};

/// Test-only providers are explicit, and never accepted by ordinary engines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestHostKind {
    Unit,
    Integration,
    Doc(RuntimeProfile),
}

impl TestHostKind {
    pub(crate) fn profile(self) -> RuntimeProfile {
        match self {
            Self::Doc(profile) => profile,
            Self::Unit | Self::Integration => RuntimeProfile::Default,
        }
    }
}

/// Slots bound simultaneous stores, not the amount of committed memory.
#[derive(Clone, Copy, Debug)]
pub struct TestPoolConfig {
    pub workers: u32,
    pub shrink_spares: u32,
}

pub(crate) struct Inner {
    pub(crate) engine: Arc<wasmtime::Engine>,
    cache: crate::cache::ModuleCache,
}

#[derive(Clone)]
pub struct TestEngine(Arc<Inner>);

pub(crate) struct Compiled {
    pub(crate) module: Arc<wasmtime::Module>,
    pub(crate) engine: Arc<Inner>,
    pub(crate) prepared: Mutex<HashMap<TestHostKind, InstancePre<Host>>>,
}

#[derive(Clone)]
pub struct TestModule(pub(crate) Arc<Compiled>);

#[derive(Clone)]
pub struct PreparedTest {
    pub(crate) engine: Arc<Inner>,
    pub(crate) pre: InstancePre<Host>,
}

impl TestEngine {
    pub fn new(mut config: EngineConfig, pool: TestPoolConfig) -> Result<Self, String> {
        if pool.workers == 0 {
            return Err("the test pool needs at least one worker".into());
        }
        let slots = pool
            .workers
            .checked_add(pool.shrink_spares)
            .ok_or_else(|| "the test pool slot count overflows".to_owned())?;
        let memories = slots
            .checked_mul(2)
            .ok_or_else(|| "the test memory slot count overflows".to_owned())?;
        // Test heaps grow lazily, so a pooled store reserves no fixed
        // initial heap before a case allocates anything.
        config.gc_heap_initial = 0;
        // The engine stays per `TestEngine`: pool slots bound simultaneous
        // stores, so sharing the pool would merge independent capacity
        // contracts. Only compiled modules are shared, per engine below.
        let mut base = crate::wasmtime_config(config);
        let mut allocator = PoolingAllocationConfig::default();
        allocator
            .total_core_instances(slots)
            // One exchange linear memory and one GC backing memory per store.
            // Wasmtime 49 allocates both from the same memory-slot pool.
            .total_memories(memories)
            .total_tables(slots)
            .total_gc_heaps(slots);
        // Pool GC and linear reservations must agree in pinned Wasmtime 49.
        // These are virtual reservations, not eagerly committed 4 GiB heaps.
        base.memory_reservation(1 << 32)
            .gc_heap_reservation(1 << 32)
            .memory_reservation_for_growth(0)
            .gc_heap_reservation_for_growth(0)
            .allocation_strategy(InstanceAllocationStrategy::Pooling(allocator));
        let code = crate::code_cache::configure(&mut base);
        let engine = Arc::new(wasmtime::Engine::new(&base).map_err(|error| error.to_string())?);
        crate::runtime::epochs::register(&engine)?;
        Ok(Self(Arc::new(Inner {
            cache: crate::cache::ModuleCache::new(&engine, code),
            engine,
        })))
    }

    /// Compiles once per engine; kind-specific imports are checked while
    /// preparing. Identical bytes return the shared compilation.
    pub fn compile(&self, wasm: &[u8]) -> Result<TestModule, String> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err("not a Wasm module".into());
        }
        let module = self
            .0
            .cache
            .get_or_compile(&self.0.engine, wasm)
            .map_err(|error| format!("{error:#}"))?;
        Ok(TestModule(Arc::new(Compiled {
            module,
            engine: Arc::clone(&self.0),
            prepared: Mutex::default(),
        })))
    }

    /// Resolves one import table per program/kind, shared by every worker.
    pub fn prepare(&self, module: &TestModule, kind: TestHostKind) -> Result<PreparedTest, String> {
        if !Arc::ptr_eq(&self.0, &module.0.engine) {
            return Err("the test module belongs to a different engine".into());
        }
        let mut prepared = module
            .0
            .prepared
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let pre = if let Some(pre) = prepared.get(&kind) {
            pre.clone()
        } else {
            let mut linker = Linker::new(&self.0.engine);
            define_test(&mut linker, &module.0.module, kind)?;
            let pre = linker
                .instantiate_pre(&module.0.module)
                .map_err(|error| format!("{error:#}"))?;
            prepared.insert(kind, pre.clone());
            pre
        };
        Ok(PreparedTest {
            engine: Arc::clone(&self.0),
            pre,
        })
    }
}

/// A package executable compiled once, on an ordinary on-demand engine.
/// Running it cannot exhaust a parent's test-pool slot while that case waits.
#[derive(Clone)]
pub struct NamedProgram {
    pub(crate) name: String,
    pub(crate) engine: Arc<WasmtimeEngine>,
    pub(crate) module: Arc<Module>,
    pub(crate) grants: Grants,
}

impl NamedProgram {
    pub fn compile(
        mut config: EngineConfig,
        name: String,
        wasm: &[u8],
        grants: Grants,
    ) -> Result<Self, String> {
        config.gc_heap_initial = 0;
        let engine = Arc::new(WasmtimeEngine::new(config)?);
        let module = Arc::new(
            engine
                .load(wasm, &MemoryStore::default())
                .map_err(|error| format!("{error:?}"))?,
        );
        Ok(Self {
            name,
            engine,
            module,
            grants,
        })
    }
}

/// Per-host cwd avoids process-global chdir, including concurrent packages.
#[derive(Clone)]
pub struct TestEnvironment {
    pub cwd: PathBuf,
    pub program: String,
    pub children: Vec<NamedProgram>,
}
