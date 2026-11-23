//! Function references to targets the compiler lowers at each call
//! (`crates/hd_mono/DESIGN.md` §13.11): an intrinsic, a built-in impl's method, and a
//! trait method at a trait-value `Self`. The adapter makes the call a
//! direct call at the same types makes, so each reference gives the
//! answer the call gives. Built and run on V8 (`host/run.ts`).

use std::path::Path;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_program(store: &MemoryStore, main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

/// Builds the program twice and checks the second build re-emits
/// nothing, then runs it on V8 and returns its standard output.
fn run(name: &str, main: &str) -> String {
    let store = MemoryStore::default();
    let out = build_program(&store, main);
    let again = build_program(&store, main);
    assert_eq!(again.counters.emitted, 0, "{name}: re-emitted");
    assert_eq!(again.wasm, out.wasm, "{name}: rebuilt differently");
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("fnref-lowered-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = hd_testkit::node::command()
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

/// A reference to an intrinsic method (`i32`'s `Eq.eq`, `Ord.cmp`, `Add.add`,
/// `Neg.neg`) and to a body-less std function (`TypeId::of`): called,
/// stored, and passed to `map` and to a function parameter.
#[test]
fn references_to_intrinsics_call_through_their_adapters() {
    let main = include_str!("programs/function_reference_lowered/intrinsics.hd");
    assert_eq!(
        run("intrinsics", main),
        "true\nfalse\ntrue\n7\n-3\nfalse\ntrue\ntrue\n"
    );
}

/// A reference to a built-in impl's method (`Display` of a primitive):
/// stored and called, and passed to `map`.
#[test]
fn references_to_builtin_impls_call_through_their_adapters() {
    let main = include_str!("programs/function_reference_lowered/builtins.hd");
    assert_eq!(run("builtins", main), "-4\n3\n3\n1.5\ntrue\nx\n");
}

/// A member reference at a `dyn` receiver calls the method in the
/// receiver's vtable (`trait.dyn.bound.reference-dispatch`): a plain method, a `mut
/// self` method, a default method, a method of a supertrait, and a bound
/// reference.
#[test]
fn references_at_a_dyn_receiver_dispatch_through_the_vtable() {
    let main = include_str!("programs/function_reference_lowered/dyn.hd");
    assert_eq!(
        run("dyn", main),
        "item1\nitem1\nhi bob from item2\nt\nitem3\nitem9\nitem9\n"
    );
}
