mod tests {
    use hd_base::Hash128;
    use hd_cache::{CacheStore, EntryKind, MemoryStore};

    use hd_run_wasmtime::internals::code_cache::{
        Function, MAX_IDLE, compile_with, configure_with, decode, encode, salt,
    };

    /// A fresh engine with the code cache on, so the in-process module
    /// cache cannot answer for it, and its salt.
    fn engine(config: hd_run_wasmtime::EngineConfig) -> (wasmtime::Engine, Hash128) {
        let mut c = hd_run_wasmtime::internals::wasmtime_config(config);
        configure_with(&mut c).expect("incremental cache");
        let engine = wasmtime::Engine::new(&c).expect("engine");
        let salt = salt(&engine);
        (engine, salt)
    }

    /// `(func (export "f{i}") (result i32) i32.const {7i} i32.const 1 i32.add)`
    /// for each `i`.
    fn module(count: u32) -> Vec<u8> {
        use wasm_encoder::{
            CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction, Module,
            TypeSection, ValType,
        };
        let mut types = TypeSection::new();
        types.ty().function([], [ValType::I32]);
        let mut functions = FunctionSection::new();
        let mut exports = ExportSection::new();
        let mut code = CodeSection::new();
        for i in 0..count {
            functions.function(0);
            exports.export(&format!("f{i}"), ExportKind::Func, i);
            let mut body = Function::new([]);
            body.instruction(&Instruction::I32Const(i32::try_from(i).expect("small") * 7))
                .instruction(&Instruction::I32Const(1))
                .instruction(&Instruction::I32Add)
                .instruction(&Instruction::End);
            code.function(&body);
        }
        let mut m = Module::new();
        m.section(&types)
            .section(&functions)
            .section(&exports)
            .section(&code);
        m.finish()
    }

    /// An hd program's module: GC structs, strings, closures, trait calls
    /// and the runtime, as `hd run` and `hd test` compile them.
    fn program() -> Vec<u8> {
        let source = include_str!("programs/code_cache.hd");
        let mut sources = hd_project::MemorySources::default();
        sources.insert("main.hd", source);
        let store = MemoryStore::default();
        let host = hd_driver::Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &hd_driver::NoClock,
            executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
        };
        let built = hd_driver::build(
            &host,
            "shop",
            &hd_driver::Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(built.diags.is_empty(), "{}", built.render());
        built.wasm.expect("the program builds")
    }

    fn call(engine: &wasmtime::Engine, module: &wasmtime::Module, name: &str) -> i32 {
        let mut store = wasmtime::Store::new(engine, ());
        store.set_epoch_deadline(u64::MAX / 2);
        let instance = wasmtime::Instance::new(&mut store, module, &[]).expect("instance");
        instance
            .get_typed_func::<(), i32>(&mut store, name)
            .expect("export")
            .call(&mut store, ())
            .expect("call")
    }

    #[test]
    fn a_warm_compile_takes_every_function_from_the_pack() {
        let packs = MemoryStore::default();
        let bytes = module(5);
        let (cold_engine, cold_salt) = engine(hd_run_wasmtime::EngineConfig::default());
        let (cold, stats) = compile_with(&packs, &cold_engine, cold_salt, &bytes);
        cold.expect("cold compile");
        assert!(stats.compiled >= 5, "{stats:?}");
        assert_eq!(packs.drain_new().len(), 1, "one pack per module");

        let (warm_engine, warm_salt) = engine(hd_run_wasmtime::EngineConfig::default());
        assert_eq!(warm_salt, cold_salt, "the salt is stable");
        let (reused, warm_stats) = compile_with(&packs, &warm_engine, warm_salt, &bytes);
        let reused = reused.expect("warm compile");
        assert_eq!(warm_stats.compiled, 0, "{warm_stats:?}");
        assert!(warm_stats.hits >= 5, "{warm_stats:?}");
        assert!(
            packs.drain_new().is_empty(),
            "a warm compile writes nothing"
        );
        // The reused code runs.
        assert_eq!(call(&warm_engine, &reused, "f3"), 22);
    }

    #[test]
    fn a_warm_compile_of_an_hd_program_compiles_nothing() {
        // Without the inlining pipeline, about one function in twenty
        // misses here.
        let packs = MemoryStore::default();
        let bytes = program();
        let (cold_engine, salt) = engine(hd_run_wasmtime::EngineConfig::default());
        let (cold, stats) = compile_with(&packs, &cold_engine, salt, &bytes);
        cold.expect("cold compile");
        assert!(stats.compiled > 0);
        for _ in 0..2 {
            let (warm_engine, _) = engine(hd_run_wasmtime::EngineConfig::default());
            let (reused, stats) = compile_with(&packs, &warm_engine, salt, &bytes);
            reused.expect("warm compile");
            assert_eq!(stats.compiled, 0, "{stats:?}");
        }
    }

    #[test]
    fn a_damaged_or_foreign_pack_is_a_miss() {
        let packs = MemoryStore::default();
        let wasm = module(2);
        let (e, salt) = engine(hd_run_wasmtime::EngineConfig::default());
        let (_, cold) = compile_with(&packs, &e, salt, &wasm);
        let (_, stored) = packs.drain_new().pop().expect("pack");
        let mut bytes = packs
            .get(EntryKind::Clpack, &stored)
            .expect("pack")
            .to_vec();
        *bytes.last_mut().expect("byte") ^= 1;
        packs.put(EntryKind::Clpack, &stored, &bytes);
        let (again, stats) = compile_with(&packs, &e, salt, &wasm);
        assert_eq!(call(&e, &again.expect("compile"), "f1"), 8);
        assert_eq!(stats.hits, cold.hits);
        assert_eq!(stats.compiled, cold.compiled);
        // Another module's pack under this key is a miss too.
        let other = module(3);
        compile_with(&packs, &e, salt, &other).0.expect("compile");
        let (_, other_key) = packs.drain_new().pop().expect("pack");
        let foreign = packs.get(EntryKind::Clpack, &other_key).expect("pack");
        packs.put(EntryKind::Clpack, &stored, &foreign);
        let (_, stats) = compile_with(&packs, &e, salt, &wasm);
        assert_eq!(stats.compiled, cold.compiled);
    }

    #[test]
    fn a_function_no_compile_uses_ages_out_of_its_pack() {
        let function = |used, idle| Function {
            code: Box::from(&b"code"[..]),
            used,
            idle,
        };
        let mut functions = hd_base::HashMap::default();
        functions.insert(Box::from(&b"used"[..]), function(true, 3));
        functions.insert(Box::from(&b"young"[..]), function(false, MAX_IDLE - 1));
        functions.insert(Box::from(&b"old"[..]), function(false, MAX_IDLE));
        let key = Hash128(7);
        let read = decode(&encode(key, Hash128(1), &functions), key).expect("decodes");
        let idle = |k: &[u8]| read.get(k).map(|f| f.idle);
        assert_eq!(idle(b"used"), Some(0));
        assert_eq!(idle(b"young"), Some(MAX_IDLE));
        assert_eq!(idle(b"old"), None);
        assert!(decode(&encode(key, Hash128(1), &functions), Hash128(8)).is_none());
    }

    #[test]
    fn another_engine_configuration_keeps_its_own_pack() {
        let packs = MemoryStore::default();
        let wasm = module(2);
        let (dev, dev_salt) = engine(hd_run_wasmtime::EngineConfig::default());
        let (release, release_salt) = engine(hd_run_wasmtime::EngineConfig::pipeline(
            hd_base::OptLevel::Optimized,
        ));
        assert_ne!(dev_salt, release_salt);
        compile_with(&packs, &dev, dev_salt, &wasm).0.expect("dev");
        compile_with(&packs, &release, release_salt, &wasm)
            .0
            .expect("release");
        assert_eq!(packs.len(), 2);
        let (_, stats) = compile_with(&packs, &dev, dev_salt, &wasm);
        assert_eq!(
            stats.compiled, 0,
            "the release pack left the dev pack alone"
        );
    }
}
