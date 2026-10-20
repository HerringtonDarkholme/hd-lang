//! A `TypeId` is built once per type and read from its global after
//! (`crates/hd_wasm/DESIGN.md` §15.4): the code that asks for one calls a
//! helper and allocates nothing itself.

use crate::build_wasm_gc as build;
use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const MAIN: &str = "use std.inspect.TypeId\n\n\
fn same_ids(n: i32) -> i32:\n    \
    let i: i32 = 0\n    \
    let same: i32 = 0\n    \
    while i < n:\n        \
        if TypeId::of::[i32]() == TypeId::of::[i32]():\n            \
            same += 1\n        \
        i += 1\n    \
    same\n\n\
pub fn main() -> void $ Console:\n    \
    println(same_ids(1000000))\n    \
    println(TypeId::of::[List[string]]())\n";

#[test]
fn a_type_id_is_built_once_by_its_helper() {
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
    // Function names by index, then each body's `struct.new` count.
    let mut names = std::collections::BTreeMap::new();
    let mut news = Vec::new();
    for p in wasmparser::Parser::new(0).parse_all(wasm) {
        match p.expect("valid module") {
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut ops = body.get_operators_reader().expect("operators");
                let mut n = 0;
                while !ops.eof() {
                    if let wasmparser::Operator::StructNew { .. } = ops.read().expect("op") {
                        n += 1;
                    }
                }
                news.push(n);
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
    let imports = names.len() - news.len();
    let news_in = |name: &str| {
        let (i, _) = names
            .iter()
            .find(|(_, n)| n.contains(name))
            .unwrap_or_else(|| panic!("no `{name}` in {names:?}"));
        news[i - imports]
    };
    assert_eq!(
        news_in("app/main/same_ids"),
        0,
        "the loop allocates no TypeId"
    );
    assert_eq!(news_in("type_id(i32)"), 1, "the helper builds it once");
}
