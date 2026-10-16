//! An omitted argument whose default is a scalar constant, such as
//! `times: i32 = 3`, is inlined at the call: the program has no function
//! for that default body (`crates/hd_mono/DESIGN.md` §13.13). A default
//! that computes keeps its instance.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const MAIN: &str = "\
fn retry(times: i32 = 3, wait: f64 = 1.5, loud: bool = true) -> i32:
    if loud:
        return times
    0

fn computed(n: i32 = 2 + 3) -> i32:
    n

pub fn main() -> void $ Console:
    println(retry())
    println(retry(times=7))
    println(computed())
";

/// The debug names of the functions the program defines.
fn names() -> Vec<String> {
    let mut src = MemorySources::default();
    src.insert("main.hd", MAIN);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "app", &Packages::default(), &goal);
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

#[test]
fn a_constant_default_has_no_function() {
    let names = names();
    for constant in ["retry.times", "retry.wait", "retry.loud"] {
        assert!(
            !names.iter().any(|n| n.contains(constant)),
            "{constant}: {names:?}"
        );
    }
    assert!(
        names.iter().any(|n| n.contains("computed.n")),
        "a computed default keeps its instance: {names:?}"
    );
}
