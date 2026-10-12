//! Session linking (repl-plan.md §2.3, job C1): a module `hd_wasm::link`
//! links in `LinkMode::Session` instantiates against the exports of an
//! earlier module in the same store and calls into it.

use hd_base::Hash128;
use hd_wasm::asm::Asm;
use hd_wasm::{
    Externs, GSym, LinkMode, MEMORY_KEY, SESSION_MODULE, Sym, VT, func_key, global_key, link,
};
use wasmtime::{Engine, Linker, Module, Store};

use crate::{EngineConfig, wasmtime_config};

/// Module A defines `a(x: i32) -> i32 = x + g` and the binding `g`;
/// module B defines `b() -> i32`, which sets `g` to 40 and returns `a(2)`.
#[test]
fn a_session_module_calls_an_earlier_module_and_shares_its_state() {
    let (ka, kb) = (Hash128(0xa), Hash128(0xb));
    let g = GSym::Binding(Hash128(0x9), 0, VT::I32);
    let mut a = Asm::new(vec![VT::I32]);
    a.raw_get(0);
    a.global_get(g.clone());
    a.s().i32_add();
    let mut b = Asm::new(vec![]);
    b.i32(40);
    b.global_set(g.clone());
    b.i32(2);
    b.call(Sym::Inst(ka));
    let none = hd_mono::Witnesses::default();
    let first = Externs {
        first: true,
        ..Externs::default()
    };
    let wa = link(
        &[(ka, a.finish(vec![VT::I32]))],
        &["a".into()],
        &[],
        &none,
        false,
        LinkMode::Session(&first),
    )
    .expect("link A");
    let later = Externs {
        first: false,
        funcs: [(Sym::Inst(ka), (vec![VT::I32], vec![VT::I32]))].into(),
        globals: [g.clone()].into(),
    };
    let wb = link(
        &[(kb, b.finish(vec![VT::I32]))],
        &["b".into()],
        &[],
        &none,
        false,
        LinkMode::Session(&later),
    )
    .expect("link B");

    let engine = Engine::new(&wasmtime_config(EngineConfig::default())).expect("engine");
    let mut store = Store::new(&engine, ());
    store.epoch_deadline_trap();
    store.set_epoch_deadline(u64::MAX / 2);
    let module_a = Module::new(&engine, &wa).expect("compile A");
    let ia = Linker::new(&engine)
        .instantiate(&mut store, &module_a)
        .expect("instantiate A");
    let mut linker = Linker::new(&engine);
    linker
        .instance(&mut store, SESSION_MODULE, ia)
        .expect("register A");
    let module_b = Module::new(&engine, &wb).expect("compile B");
    let ib = linker
        .instantiate(&mut store, &module_b)
        .expect("instantiate B against A");

    let key_b = func_key(&Sym::Inst(kb)).expect("key");
    let fb = ib
        .get_typed_func::<(), i32>(&mut store, &key_b)
        .expect("B exports b");
    assert_eq!(fb.call(&mut store, ()).expect("call b"), 42);
    // B wrote the global A owns.
    let ga = ia
        .get_global(&mut store, &global_key(&g))
        .expect("A exports g");
    assert_eq!(ga.get(&mut store).i32(), Some(40));
    // B's memory is A's.
    let ma = ia.get_memory(&mut store, MEMORY_KEY).expect("A memory");
    let mb = ib.get_memory(&mut store, MEMORY_KEY).expect("B memory");
    ma.data_mut(&mut store)[7] = 99;
    assert_eq!(mb.data(&store)[7], 99);
}
