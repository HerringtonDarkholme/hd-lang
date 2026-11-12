//! A runtime init whose bindings no reached code writes does nothing once
//! link folds its constant stores, so `hd.init` leaves it out and the
//! module drops it (#1423). A program that allocates keeps the allocator's
//! init, called from `hd.init`.

use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The debug names of the module's functions, and its global count, for
/// program `main` built in dev (`checked` off) or checked mode.
fn linked(main: &str, checked: bool) -> (Vec<String>, u32) {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        checked,
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let mut names = Vec::new();
    let mut globals = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::GlobalSection(g) => globals = g.count(),
            wasmparser::Payload::CustomSection(c) => {
                if let wasmparser::KnownCustom::Name(r) = c.as_known() {
                    for n in r {
                        if let wasmparser::Name::Function(m) = n.expect("name") {
                            names.extend(m.into_iter().map(|f| f.expect("name").name.to_owned()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (names, globals)
}

const INIT: &str = "std/rt.alloc/init.init#0";

#[test]
fn a_program_that_never_allocates_runs_no_allocator_init() {
    let (names, globals) = linked("fn helper() -> i32:\n    1\n", false);
    assert!(!names.iter().any(|n| n == INIT), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("std/rt.")), "{names:?}");
    // Nor the allocator's bindings, nor the forbidden-context depth its
    // call from `hd.init` was wrapped in.
    assert_eq!(globals, 0, "{names:?}");
}

#[test]
fn a_program_that_allocates_in_a_std_call_runs_the_allocator_init() {
    let (names, globals) = linked(
        "pub fn main() -> void $ Console:\n    println(\"ab\".repeat(3))\n",
        false,
    );
    assert!(names.iter().any(|n| n == INIT), "{names:?}");
    assert!(globals > 0, "{names:?}");
}

#[test]
fn a_checked_program_keeps_the_allocator_init_its_audit_needs() {
    let (names, _) = linked("fn helper() -> i32:\n    1\n", true);
    assert!(names.iter().any(|n| n == INIT), "{names:?}");
}
