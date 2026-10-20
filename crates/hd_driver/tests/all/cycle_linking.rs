//! A program none of whose objects can be on a reference cycle links no
//! cycle collector (`crates/hd_wasm_linear/DESIGN.md` §23.28, #649); one
//! that builds a ring of references does.

use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The debug names of the functions the program `main` defines.
fn names_of(main: &str) -> Vec<String> {
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
    let out = build_packages(
        &host,
        "app",
        &Packages::default(),
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let mut names = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        if let wasmparser::Payload::CustomSection(c) = payload.expect("payload")
            && let wasmparser::KnownCustom::Name(r) = c.as_known()
        {
            for n in r {
                if let wasmparser::Name::Function(m) = n.expect("name") {
                    names.extend(m.into_iter().map(|f| f.expect("name").name.to_owned()));
                }
            }
        }
    }
    names
}

fn collector(names: &[String]) -> Vec<&String> {
    names
        .iter()
        .filter(|n| {
            n.starts_with("std/rt.cycle/") || n.starts_with("rt:cycle_") || *n == "rt:collect"
        })
        .collect()
}

#[test]
fn a_program_of_acyclic_values_links_no_collector() {
    let names = names_of(
        "pub fn main() -> void $ Console:\n    xs := [\"a\", \"b\"]\n    add := fn(k: i32) -> i32: k + 1\n    println(\"${xs.len()} ${add(41)}\")\n",
    );
    assert!(collector(&names).is_empty(), "{names:?}");
    assert!(
        names
            .iter()
            .any(|n| n.starts_with("std/rt.alloc/release_acyclic")),
        "{names:?}"
    );
}

#[test]
fn a_program_with_a_reference_ring_links_the_collector() {
    let names = names_of(
        "struct Stop:\n    next: &mut Stop?\n\npub fn main() -> void $ Console:\n    first := &mut Stop { next: .None }\n    second := &mut Stop { next: .Some(first) }\n    first.next = .Some(second)\n    println(\"ring\")\n",
    );
    assert!(
        names.iter().any(|n| n.starts_with("std/rt.cycle/collect#")),
        "{names:?}"
    );
}
