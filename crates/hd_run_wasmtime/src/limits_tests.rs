use super::{HeapBudget, is_heap_exhausted};
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction as I,
    MemorySection, MemoryType, Module, StorageType, TypeSection, ValType,
};
use wasmtime::{Config, Engine, Instance, Memory, ResourceLimiter, Store};

const PAGE: u64 = 1 << 16;

fn engine(custom_pages: bool, gc_initial: u64) -> Engine {
    let mut config = Config::new();
    // The process-wide trap handler must match `wasmtime_config`'s, or
    // whichever engine comes second panics.
    config
        .macos_use_mach_ports(false)
        .collector(wasmtime::Collector::Copying)
        .gc_heap_initial_size(gc_initial)
        .wasm_custom_page_sizes(custom_pages);
    Engine::new(&config).unwrap()
}

fn store(engine: &Engine, cap: u64) -> Store<HeapBudget> {
    let mut store = Store::new(engine, HeapBudget::new(cap));
    store.limiter(|budget| budget);
    store
}

fn memory_type(minimum: u64, maximum: Option<u64>, byte_pages: bool) -> MemoryType {
    MemoryType {
        minimum,
        maximum,
        memory64: false,
        shared: false,
        page_size_log2: byte_pages.then_some(0),
    }
}

fn module(memories: &[MemoryType], gc: bool) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    if gc {
        types.ty().array(&StorageType::I8, true);
    }
    types.ty().function([ValType::I32], []);
    module.section(&types);
    let mut functions = FunctionSection::new();
    functions.function(u32::from(gc));
    module.section(&functions);
    let mut memory = MemorySection::new();
    for ty in memories {
        memory.memory(*ty);
    }
    module.section(&memory);
    let mut exports = ExportSection::new();
    exports.export("allocate", ExportKind::Func, 0);
    for (index, _) in memories.iter().enumerate() {
        exports.export(
            &format!("memory{index}"),
            ExportKind::Memory,
            u32::try_from(index).unwrap(),
        );
    }
    module.section(&exports);
    let mut code = CodeSection::new();
    let mut function = Function::new([]);
    function.instruction(&I::LocalGet(0));
    if gc {
        function.instruction(&I::ArrayNewDefault(0));
    } else {
        function.instruction(&I::MemoryGrow(0));
    }
    function.instruction(&I::Drop);
    function.instruction(&I::End);
    code.function(&function);
    module.section(&code);
    module.finish()
}

fn instantiate(
    engine: &Engine,
    store: &mut Store<HeapBudget>,
    memories: &[MemoryType],
    gc: bool,
) -> wasmtime::Result<Instance> {
    let module = wasmtime::Module::new(engine, module(memories, gc)).unwrap();
    Instance::new(store, &module, &[])
}

#[test]
fn initial_memories_share_one_cap_and_fail_before_overrun() {
    let engine = engine(false, 0);
    let mut store = store(&engine, PAGE);
    let error = instantiate(
        &engine,
        &mut store,
        &[memory_type(1, None, false), memory_type(1, None, false)],
        false,
    )
    .unwrap_err();
    assert!(is_heap_exhausted(&error));
    assert_eq!(store.data().charged, PAGE);
}

#[test]
fn guest_linear_growth_traps_before_overrun() {
    let engine = engine(false, 0);
    let mut store = store(&engine, 2 * PAGE);
    let instance = instantiate(&engine, &mut store, &[memory_type(1, None, false)], false).unwrap();
    let allocate = instance
        .get_typed_func::<i32, ()>(&mut store, "allocate")
        .unwrap();
    allocate.call(&mut store, 1).unwrap();
    let error = allocate.call(&mut store, 1).unwrap_err();
    assert!(is_heap_exhausted(&error));
    assert_eq!(store.data().charged, 2 * PAGE);
    assert_eq!(
        instance
            .get_memory(&mut store, "memory0")
            .unwrap()
            .size(&store),
        2
    );
}

#[test]
fn failed_reserved_growth_rolls_back_only_its_delta() {
    let engine = engine(false, 0);
    let mut store = store(&engine, 3 * PAGE);
    let instance = instantiate(
        &engine,
        &mut store,
        &[memory_type(1, Some(2), false)],
        false,
    )
    .unwrap();
    let memory = instance.get_memory(&mut store, "memory0").unwrap();
    memory.grow(&mut store, 1).unwrap();
    assert!(is_heap_exhausted(&memory.grow(&mut store, 1).unwrap_err()));
    assert_eq!(store.data().charged, 2 * PAGE);
    Memory::new(&mut store, wasmtime::MemoryType::new(1, None)).unwrap();
    assert_eq!(store.data().charged, 3 * PAGE);
}

#[test]
fn pre_callback_failure_does_not_rollback_previous_success() {
    let engine = engine(true, 0);
    let mut store = store(&engine, 4);
    let instance = instantiate(&engine, &mut store, &[memory_type(1, None, true)], false).unwrap();
    let allocate = instance
        .get_typed_func::<i32, ()>(&mut store, "allocate")
        .unwrap();
    let memory = instance.get_memory(&mut store, "memory0").unwrap();
    allocate.call(&mut store, 1).unwrap();
    assert_eq!(store.data().charged, 2);
    // 2 + u32::MAX is clamped to the memory's maximum, then rejected before
    // memory_growing: the custom one-byte-page I32 size cannot represent it.
    assert!(is_heap_exhausted(
        &allocate.call(&mut store, -1).unwrap_err()
    ));
    assert_eq!(store.data().charged, 2);
    assert_eq!(memory.data_size(&store), 2);
    allocate.call(&mut store, 2).unwrap();
    assert_eq!(store.data().charged, 4);
    assert!(is_heap_exhausted(
        &allocate.call(&mut store, 1).unwrap_err()
    ));
    assert_eq!(memory.data_size(&store), 4);
}

#[test]
fn gc_and_linear_backing_count_against_the_same_cap() {
    let engine = engine(false, 0);
    let mut store = store(&engine, 2 * PAGE);
    let instance = instantiate(&engine, &mut store, &[memory_type(1, None, false)], true).unwrap();
    let allocate = instance
        .get_typed_func::<i32, ()>(&mut store, "allocate")
        .unwrap();
    allocate.call(&mut store, 10).unwrap();
    assert_eq!(u64::try_from(store.gc_heap_capacity()).unwrap(), PAGE);
    assert_eq!(store.data().charged, 2 * PAGE);
    assert!(is_heap_exhausted(
        &allocate.call(&mut store, 100_000).unwrap_err()
    ));
    assert_eq!(store.data().charged, 2 * PAGE);
    assert_eq!(u64::try_from(store.gc_heap_capacity()).unwrap(), PAGE);
}

#[test]
fn gc_can_collect_after_denied_growth_without_a_false_panic() {
    let engine = engine(false, PAGE);
    // Verify the same allocation pattern attempts geometric growth when the
    // limiter permits it, rather than merely exercising collect-first policy.
    let mut unrestricted = store(&engine, u64::MAX);
    let unrestricted_instance = instantiate(&engine, &mut unrestricted, &[], true).unwrap();
    let unrestricted_allocate = unrestricted_instance
        .get_typed_func::<i32, ()>(&mut unrestricted, "allocate")
        .unwrap();
    unrestricted_allocate
        .call(&mut unrestricted, 20_000)
        .unwrap();
    unrestricted_allocate
        .call(&mut unrestricted, 20_000)
        .unwrap();
    assert_eq!(
        u64::try_from(unrestricted.gc_heap_capacity()).unwrap(),
        2 * PAGE
    );

    let mut store = store(&engine, PAGE);
    let instance = instantiate(&engine, &mut store, &[], true).unwrap();
    let allocate = instance
        .get_typed_func::<i32, ()>(&mut store, "allocate")
        .unwrap();
    // Each object fits in one semispace; two do not. They are unrooted after
    // each call, so denied growth can be recovered by collecting the first.
    allocate.call(&mut store, 20_000).unwrap();
    allocate.call(&mut store, 20_000).unwrap();
    allocate.call(&mut store, 20_000).unwrap();
    assert_eq!(store.data().charged, PAGE);
    assert_eq!(u64::try_from(store.gc_heap_capacity()).unwrap(), PAGE);
}

#[test]
fn initial_gc_allocation_is_limited_before_allocation() {
    let engine = engine(false, 2 * PAGE);
    let mut store = store(&engine, PAGE);
    // A GC-using module initializes its GC store during instantiation, before
    // calling an export. The cap therefore has to be attached beforehand.
    let error = instantiate(&engine, &mut store, &[], true).unwrap_err();
    assert!(is_heap_exhausted(&error));
    assert_eq!(store.gc_heap_capacity(), 0);
    assert_eq!(store.data().charged, 0);
}

#[test]
fn zero_cap_and_unlimited_budget_have_no_hidden_default() {
    let mut empty = HeapBudget::new(0);
    assert!(empty.memory_growing(0, 0, None).unwrap());
    assert!(is_heap_exhausted(
        &empty.memory_growing(0, 1, None).unwrap_err()
    ));
    let mut unlimited = HeapBudget::new(u64::MAX);
    assert!(unlimited.memory_growing(0, usize::MAX, None).unwrap());
    assert_eq!(unlimited.charged, usize::MAX as u64);
}

#[test]
fn heap_failure_classification_preserves_typed_errors_through_context() {
    let out_of_memory =
        wasmtime::Error::from(wasmtime::OutOfMemory::new(100)).context("initial heap allocation");
    assert!(is_heap_exhausted(&out_of_memory));
    let os_out_of_memory =
        wasmtime::Error::from(std::io::Error::from(std::io::ErrorKind::OutOfMemory))
            .context("initial OS allocation");
    assert!(is_heap_exhausted(&os_out_of_memory));
    assert!(is_heap_exhausted(
        &wasmtime::Trap::AllocationTooLarge.into()
    ));
    assert!(!is_heap_exhausted(
        &wasmtime::Trap::UnreachableCodeReached.into()
    ));
    assert!(!is_heap_exhausted(&wasmtime::Error::msg(
        "unrelated engine error"
    )));
}

#[cfg(unix)]
#[test]
fn initial_unix_mmap_oom_is_a_heap_failure_without_string_matching() {
    let error = wasmtime::Error::from(rustix::io::Errno::NOMEM)
        .context("initial linear or GC backing allocation");
    assert!(is_heap_exhausted(&error));
    let permission_denied =
        wasmtime::Error::from(rustix::io::Errno::PERM).context("a different OS failure");
    assert!(!is_heap_exhausted(&permission_denied));
}
