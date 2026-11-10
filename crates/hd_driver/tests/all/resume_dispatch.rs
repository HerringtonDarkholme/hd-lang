//! A suspending body resumes into a block list through one `br_table` on
//! its state (`crates/hd_wasm/DESIGN.md` §14.2), not a guard per item.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A loop of three waits, so its body list holds three states.
const MAIN: &str = "use std.time.{Clock, Duration, sleep}\n\n\
pub fn main!() -> void $ Console + Clock:\n    \
    let i: i32 = 0\n    \
    while i < 3:\n        \
        sleep!(Duration::milliseconds(1))\n        \
        println(\"a\")\n        \
        sleep!(Duration::milliseconds(1))\n        \
        println(\"b\")\n        \
        sleep!(Duration::milliseconds(1))\n        \
        i += 1\n";

#[test]
fn a_loop_of_waits_resumes_through_one_br_table() {
    let mut src = MemorySources::default();
    src.insert("main.hd", MAIN);
    let store = MemoryStore::default();
    let host = Host {
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[],
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build(&host, "app", &goal);
    let Some(wasm) = &out.wasm else {
        panic!("{}", out.render())
    };
    let mut names = std::collections::BTreeMap::new();
    let mut tables = Vec::new();
    for p in wasmparser::Parser::new(0).parse_all(wasm) {
        match p.expect("valid module") {
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut ops = body.get_operators_reader().expect("operators");
                let mut n = 0;
                while !ops.eof() {
                    if let wasmparser::Operator::BrTable { .. } = ops.read().expect("op") {
                        n += 1;
                    }
                }
                tables.push(n);
            }
            wasmparser::Payload::CustomSection(c) if c.name() == "name" => {
                let r = wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    c.data(),
                    c.data_offset(),
                ));
                for n in r {
                    if let Ok(wasmparser::Name::Function(m)) = n {
                        for x in m {
                            let x = x.expect("name");
                            names.insert(x.index as usize, x.name.to_owned());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // The dev `name` section names every function, imports first.
    let imports = names.len() - tables.len();
    let in_main: usize = names
        .iter()
        .filter(|(_, n)| n.starts_with("app/main/main"))
        .map(|(i, _)| tables[i - imports])
        .sum();
    assert_eq!(in_main, 1, "{names:?}");
}
