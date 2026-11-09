//! A program whose function table is past the descriptor word's glue field
//! (`crates/hd_wasm_linear/DESIGN.md` §23.4) gets `program-too-large`, a
//! diagnostic that names the limit, never a descriptor that wraps into the
//! borrow state (#1536). Each program is generated: a binary tree of `n`
//! functions, each named as a value by its parent, so each takes a slot.

use std::fmt::Write as _;
use std::time::Instant;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;

/// `n` functions that each take a function-table slot; with `weak`, `main`
/// also makes a weak reference, which links the weak glue field.
fn program(n: usize, weak: bool) -> String {
    let mut text = String::new();
    if weak {
        text.push_str("use std.weak.{weak}\n\nstruct Node:\n    value: i32\n\n");
    }
    text.push_str("fn apply(f: fn(i32) -> i32, x: i32) -> i32: f(x)\n\n");
    for i in 0..n {
        let (a, b) = (2 * i + 1, 2 * i + 2);
        if b < n {
            let _ = writeln!(
                text,
                "fn g{i}(x: i32) -> i32:\n    if x < 2: apply(g{a}, x + 2) else: apply(g{b}, x - 1) + {i}"
            );
        } else {
            let _ = writeln!(text, "fn g{i}(x: i32) -> i32: x + {i}");
        }
    }
    text.push_str("\npub fn main() -> void $ Console:\n    println(apply(g0, 3))\n");
    if weak {
        text.push_str("    node := box(Node { value: 1 })\n    println(weak(node).is_some())\n");
    }
    text
}

/// The one error of building `program(n, weak)`, and its rendered report.
fn build_error(n: usize, weak: bool) -> (Code, String) {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", &program(n, weak));
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(8),
    };
    let started = Instant::now();
    let out = build_packages(
        &host,
        "app",
        &Packages::default(),
        &Goal::Program {
            entry: "main".into(),
        },
    );
    println!(
        "{n} functions, weak {weak}: built in {:?}",
        started.elapsed()
    );
    assert!(out.wasm.is_none(), "{}", out.render());
    assert_eq!(out.diags.code.len(), 1, "{}", out.render());
    (out.diags.code[0], out.render())
}

#[test]
fn a_function_table_past_the_glue_field_is_program_too_large() {
    let (code, message) = build_error(65_536, false);
    assert_eq!(code, Code::ProgramTooLarge, "{message}");
    assert!(message.contains("may have at most 65535"), "{message}");
    assert!(message.contains("16 bits"), "{message}");
}

#[test]
fn a_weak_linked_table_past_its_glue_field_is_program_too_large() {
    let (code, message) = build_error(32_768, true);
    assert_eq!(code, Code::ProgramTooLarge, "{message}");
    assert!(
        message.contains("that may hold weak references may have at most 32767"),
        "{message}"
    );
    assert!(message.contains("15 bits"), "{message}");
}
