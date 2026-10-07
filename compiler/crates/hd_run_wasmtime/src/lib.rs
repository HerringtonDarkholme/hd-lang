#![forbid(unsafe_code)]
//! `hd_run_wasmtime`: the wasmtime engine, the default profile's
//! providers, the reactor, epochs, limits and the `cwasm` cache
//! (codegen.md §11.4; engines-and-test-runner.md §18).
//!
//! wasmtime is not a dependency yet (owner approval pending, SK-10), so
//! the engine reports itself as not linked. `hd run` runs on V8 through
//! Node meanwhile (build-order.md §22, "Wasm first").

use hd_cache::CacheStore;
use hd_run::{Engine, HostSetup, Instance, LoadError, StartError};

/// The wasmtime configuration of §18.1, as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub gc: bool,
    pub pooling: bool,
    pub epoch_interruption: bool,
    pub cranelift_opt: OptLevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptLevel {
    None,
    Speed,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { gc: true, pooling: true, epoch_interruption: true, cranelift_opt: OptLevel::None }
    }
}

pub struct WasmtimeEngine {
    pub config: EngineConfig,
}

/// A loaded module: the precompiled bytes once wasmtime is linked.
pub struct Module;

impl Engine for WasmtimeEngine {
    type Module = Module;
    fn load(&self, _: &[u8], _: &dyn CacheStore) -> Result<Module, LoadError> {
        Err(LoadError::Engine("wasmtime is not linked in this build".into()))
    }
    fn instantiate(&self, _: &Module, _: &HostSetup) -> Result<Box<dyn Instance>, StartError> {
        Err(StartError::Engine("wasmtime is not linked in this build".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::{EngineConfig, WasmtimeEngine};
    use hd_cache::MemoryStore;
    use hd_run::Engine;

    #[test]
    fn reports_not_linked() {
        let e = WasmtimeEngine { config: EngineConfig::default() };
        assert!(e.load(b"\0asm", &MemoryStore::default()).is_err());
    }
}
