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

/// Builds the script module `entry` over `files` through the driver, as
/// one more module of `session`; returns the module and what it adds.
pub(crate) fn session_module(
    files: &[(&str, &str)],
    entry: &str,
    session: &hd_driver::Session,
) -> (Vec<u8>, hd_driver::Session) {
    let mut src = hd_project::MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = hd_cache::MemoryStore::default();
    let host = hd_driver::Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &hd_driver::NoClock,
        executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let packages = hd_driver::Packages {
        session: Some(session),
        ..hd_driver::Packages::default()
    };
    let out = hd_driver::build_packages(
        &host,
        "app",
        &packages,
        &hd_driver::Goal::Program {
            entry: entry.into(),
        },
    );
    assert!(out.diags.is_empty(), "{entry}: {}", out.render());
    (
        out.wasm.expect("a module"),
        out.session_added.expect("what the module adds"),
    )
}

/// The one instance a module adds that is not an init.
fn only_function(added: &hd_driver::Session) -> String {
    let fs: Vec<&Sym> = added
        .externs
        .funcs
        .keys()
        .filter(|s| !matches!(s, Sym::Inst(k) if added.inits_run.contains(k)))
        .collect();
    assert_eq!(fs.len(), 1, "{fs:?}");
    func_key(fs[0]).expect("an instance")
}

/// The init instance a module adds.
fn only_init(added: &hd_driver::Session) -> String {
    let is: Vec<&Hash128> = added.inits_run.iter().collect();
    assert_eq!(is.len(), 1, "{is:?}");
    func_key(&Sym::Inst(*is[0])).expect("an instance")
}

/// Job C2 on wasmtime: input 1 defines `bump`, which reads its binding
/// `start`; input 2's `twice` calls `bump` twice and its init stores
/// `twice(1)` in `last`. Input 2's module holds no `bump`: it calls input
/// 1's, which reads the `start` input 1's init set.
#[test]
fn a_session_input_calls_a_function_an_earlier_input_defined() {
    let in1 = "start := +20\n\npub fn bump(x: i32) -> i32:\n    x + start\n\nseed := bump(+1)\n";
    let in2 = "use pkg.in1.{bump}\n\npub fn twice(x: i32) -> i32:\n    bump(bump(x))\n\nlast := twice(+1)\n";
    let mut session = hd_driver::Session::start();
    let (wa, added) = session_module(&[("in1.hd", in1)], "in1", &session);
    let init_a = only_init(&added);
    session.add(added);
    let (wb, added) = session_module(&[("in1.hd", in1), ("in2.hd", in2)], "in2", &session);
    let (twice, init_b) = (only_function(&added), only_init(&added));
    let last: Vec<&GSym> = added.externs.globals.iter().collect();
    assert_eq!(last.len(), 1, "{last:?}");
    let last = global_key(last[0]);

    let engine = Engine::new(&wasmtime_config(EngineConfig::default())).expect("engine");
    let mut store = Store::new(&engine, ());
    store.epoch_deadline_trap();
    store.set_epoch_deadline(u64::MAX / 2);
    // Neither module reaches a host function here.
    let module_a = Module::new(&engine, &wa).expect("compile A");
    let mut linker = Linker::new(&engine);
    linker
        .define_unknown_imports_as_traps(&module_a)
        .expect("host imports");
    let ia = linker
        .instantiate(&mut store, &module_a)
        .expect("instantiate A");
    ia.get_typed_func::<(), ()>(&mut store, &init_a)
        .expect("A exports its init")
        .call(&mut store, ())
        .expect("run A's init");
    let mut linker = Linker::new(&engine);
    linker
        .instance(&mut store, SESSION_MODULE, ia)
        .expect("register A");
    let module_b = Module::new(&engine, &wb).expect("compile B");
    linker
        .define_unknown_imports_as_traps(&module_b)
        .expect("host imports");
    let ib = linker
        .instantiate(&mut store, &module_b)
        .expect("instantiate B against A");
    let f = ib
        .get_typed_func::<i32, i32>(&mut store, &twice)
        .expect("B exports twice");
    assert_eq!(f.call(&mut store, 1).expect("call twice"), 41);
    ib.get_typed_func::<(), ()>(&mut store, &init_b)
        .expect("B exports its init")
        .call(&mut store, ())
        .expect("run B's init");
    let g = ib.get_global(&mut store, &last).expect("B exports last");
    assert_eq!(g.get(&mut store).i32(), Some(41));
}
