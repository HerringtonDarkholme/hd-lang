mod tests {
    use std::sync::Arc;

    use hd_cache::MemoryStore;
    use hd_run::Engine as _;
    use hd_run_wasmtime::internals::ModuleExt;
    use hd_run_wasmtime::{EngineConfig, WasmtimeEngine};

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
        assert!(Arc::ptr_eq(a.code(), b.code()));
        // Different bytes still compile separately.
        let other = first.load(&other_module(), &store).unwrap();
        assert!(!Arc::ptr_eq(a.code(), other.code()));
    }

    #[test]
    fn distinct_configs_compile_separately() {
        // A different engine config must not alias the default's artifact.
        let bytes = empty_module();
        let store = MemoryStore::default();
        let default_engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let release_engine =
            WasmtimeEngine::new(EngineConfig::pipeline(hd_base::OptLevel::Optimized)).unwrap();
        let a = default_engine.load(&bytes, &store).unwrap();
        let b = release_engine.load(&bytes, &store).unwrap();
        assert!(!Arc::ptr_eq(a.code(), b.code()));
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
        let total = u32::try_from(hd_run_wasmtime::internals::cache::CAPACITY).unwrap() + EXTRA;
        let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let store = MemoryStore::default();
        // Fill past the bound with distinct modules, hitting each once.
        let first = engine.load(&numbered_module(0), &store).unwrap();
        for n in 1..total {
            engine.load(&numbered_module(n), &store).unwrap();
        }
        // The oldest entry evicted: reloading recompiles.
        let recompiled = engine.load(&numbered_module(0), &store).unwrap();
        assert!(!Arc::ptr_eq(first.code(), recompiled.code()));
        // A recent entry still hits the shared compilation, twice in a row.
        let last = total - 1;
        let kept = engine.load(&numbered_module(last), &store).unwrap();
        assert!(Arc::ptr_eq(
            kept.code(),
            engine.load(&numbered_module(last), &store).unwrap().code()
        ));
    }
}
