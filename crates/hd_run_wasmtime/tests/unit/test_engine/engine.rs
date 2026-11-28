mod wide_memory_tests {
    use hd_run_wasmtime::EngineConfig;
    use hd_run_wasmtime::internals::test_engine::engine::{TestEngine, TestPoolConfig};
    use hd_run_wasmtime::internals::{TestEngineExt, TestModuleExt};

    #[test]
    fn pool_holds_three_memories_for_every_worker() {
        let engine = TestEngine::new(
            EngineConfig::default(),
            TestPoolConfig {
                workers: 2,
                shrink_spares: 0,
            },
        )
        .expect("create pool");
        let mut memories = wasm_encoder::MemorySection::new();
        for _ in 0..3 {
            memories.memory(wasm_encoder::MemoryType {
                minimum: 1,
                maximum: None,
                memory64: false,
                shared: false,
                page_size_log2: None,
            });
        }
        let mut module = wasm_encoder::Module::new();
        module.section(&memories);
        let module = engine
            .compile(&module.finish())
            .expect("compile three memories");
        let mut stores = Vec::new();
        for _ in 0..2 {
            let mut store = wasmtime::Store::new(engine.wasmtime(), ());
            let _instance = wasmtime::Instance::new(&mut store, module.code(), &[])
                .expect("each worker has three pool memory slots");
            stores.push(store);
        }
        assert_eq!(stores.len(), 2);
    }
}
