//! #2031: a struct nested by value, built by `mk{i}` and read by
//! `get{i}` down a chain of K structs, costs each function code that does
//! not grow with the value's size: a parameter over 16 values passes as
//! one address, a result over 16 values is written into its caller's
//! block, and a literal headed there writes each field in place. So the
//! module grows linearly in K. Before, it grew 11x when K grew 4x.

use std::fmt::Write as _;

use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The issue's program in one file: `T{i}` holds `T{i-1}` and an `i32`.
fn program(k: usize) -> String {
    let mut text = String::from(
        "struct T0:\n    v: i32\n\nfn mk0() -> T0: T0 { v: +1 }\nfn get0(t: T0) -> i32: t.v\n",
    );
    for i in 1..k {
        let p = i - 1;
        let _ = write!(
            text,
            "\nstruct T{i}:\n    inner: T{p}\n    v: i32\n\n\
             fn mk{i}() -> T{i}: T{i} {{ inner: mk{p}(), v: +{i} }}\n\
             fn get{i}(t: T{i}) -> i32: get{p}(t.inner) * 3 + 1\n"
        );
    }
    let last = k - 1;
    let _ = write!(
        text,
        "\npub fn main() -> void $ Console:\n    println(\"${{get{last}(mk{last}())}}\")\n"
    );
    text
}

/// The counted variant (step 4): `T{i}` also holds a string. Each
/// `get{i}` takes its parameter by address, moves `inner` on and drops
/// the rest through `T{i}`'s release witness, which calls `T{i-1}`'s;
/// each `mk{i}` writes its literal, string included, into its caller's
/// block, `inner` in place.
fn counted_program(k: usize) -> String {
    let mut text = String::from(
        "struct T0:\n    v: i32\n    s: string\n\n\
         fn mk0() -> T0: T0 { v: +1, s: \"s\" }\nfn get0(t: T0) -> i32: t.v\n",
    );
    for i in 1..k {
        let p = i - 1;
        let _ = write!(
            text,
            "\nstruct T{i}:\n    inner: T{p}\n    v: i32\n    s: string\n\n\
             fn mk{i}() -> T{i}: T{i} {{ inner: mk{p}(), v: +{i}, s: \"s\" }}\n\
             fn get{i}(t: T{i}) -> i32: get{p}(t.inner) % 1000 + t.v\n"
        );
    }
    let last = k - 1;
    let _ = write!(
        text,
        "\npub fn main() -> void $ Console:\n    println(\"${{get{last}(mk{last}())}}\")\n"
    );
    text
}

fn module_bytes(k: usize) -> usize {
    build_bytes(&program(k))
}

fn build_bytes(source: &str) -> usize {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target: EngineTarget::V8,
        ..Packages::default()
    };
    let output = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!output.diags.has_errors(), "{}", output.render());
    output.wasm.as_deref().map_or(0, <[u8]>::len)
}

#[test]
fn struct_chain_module_grows_linearly() {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let (small, large) = (module_bytes(50), module_bytes(200));
            eprintln!("struct chain module bytes: K=50 {small}, K=200 {large}");
            assert!(small > 0);
            assert!(large * 2 <= small * 9, "K=50 {small}, K=200 {large}");
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}

#[test]
fn counted_struct_chain_module_grows_linearly() {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let (small, large) = (
                build_bytes(&counted_program(50)),
                build_bytes(&counted_program(200)),
            );
            eprintln!("counted struct chain module bytes: K=50 {small}, K=200 {large}");
            assert!(small > 0);
            assert!(large * 2 <= small * 9, "K=50 {small}, K=200 {large}");
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}

/// `take` owns a struct of `f` strings by address and moves each field
/// on to `keep`, which owns it, so its drop skips each under the
/// field's flag. With `local`, the struct is a local of `take` kept in a
/// frame slot instead of its parameter.
fn moved_fields_program(f: usize, local: bool) -> String {
    let mut text = String::from("struct W:\n");
    for k in 0..f {
        let _ = writeln!(text, "    s{k}: string");
    }
    text.push_str("\nfn mk() -> W: W { ");
    for k in 0..f {
        let _ = write!(text, "{}s{k}: \"v{k}\"", if k == 0 { "" } else { ", " });
    }
    text.push_str(" }\n\nfn keep(s: string) -> string: s\n\n");
    text.push_str(if local {
        "fn take() -> i32:\n    let w = mk()\n    let n = 0\n"
    } else {
        "fn take(w: W) -> i32:\n    let n = 0\n"
    });
    for k in 0..f {
        let _ = writeln!(text, "    n += i32(keep(w.s{k}).len())");
    }
    let call = if local { "take()" } else { "take(mk())" };
    let _ = write!(
        text,
        "    n\n\npub fn main() -> void $ Console:\n    println(\"${{{call}}}\")\n"
    );
    text
}

/// Moving F fields out of one parameter kept by address costs O(F): each
/// move probes its flag by (local, path), and the drop groups the moved
/// paths by field once per level. Before, each move scanned the moved
/// list and each release step filtered it, O(F^2).
#[test]
fn moved_fields_cost_linear_work() {
    moved_fields_linear(false);
}

/// The same for a local kept in a frame slot, whose store also clears
/// each moved field's flag once.
#[test]
fn moved_local_fields_cost_linear_work() {
    moved_fields_linear(true);
}

fn moved_fields_linear(local: bool) {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(move || {
            let ((), small) = hd_wasm::work::count_moved_paths(|| {
                build_bytes(&moved_fields_program(40, local));
            });
            let ((), large) = hd_wasm::work::count_moved_paths(|| {
                build_bytes(&moved_fields_program(400, local));
            });
            eprintln!("moved field work (local {local}): F=40 {small}, F=400 {large}");
            assert!(small >= 40, "F=40 {small}");
            assert!(large <= small * 12, "F=40 {small}, F=400 {large}");
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}
