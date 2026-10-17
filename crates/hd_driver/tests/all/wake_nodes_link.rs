//! The `all!` waker-node runtime links only into a program that polls an
//! `all!` (`crates/hd_wasm/DESIGN.md` §14.4): elsewhere a host leaf
//! records no node and a completion walks no records.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The function names and exports of `main`'s program.
fn linked(main: &str) -> (Vec<String>, Vec<String>) {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[],
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    let Some(wasm) = &out.wasm else {
        panic!("{}", out.render())
    };
    let (mut fns, mut exports) = (Vec::new(), Vec::new());
    for p in wasmparser::Parser::new(0).parse_all(wasm) {
        match p.expect("valid module") {
            wasmparser::Payload::ExportSection(s) => {
                exports.extend(s.into_iter().map(|e| e.expect("export").name.to_owned()));
            }
            wasmparser::Payload::CustomSection(c) if c.name() == "name" => {
                let r = wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    c.data(),
                    c.data_offset(),
                ));
                for n in r {
                    if let Ok(wasmparser::Name::Function(m)) = n {
                        fns.extend(m.into_iter().map(|x| x.expect("name").name.to_owned()));
                    }
                }
            }
            _ => {}
        }
    }
    (fns, exports)
}

#[test]
fn a_program_without_all_links_no_waker_nodes() {
    let (fns, exports) = linked("pub fn main() -> void $ Console:\n    println(42)\n");
    // The host leaf still registers and completions still mark.
    assert!(fns.iter().any(|f| f == "rt:wake_register"), "{fns:?}");
    assert!(!fns.iter().any(|f| f == "rt:wake_up"), "{fns:?}");
    assert!(!exports.iter().any(|e| e == "hd.all_polls"), "{exports:?}");

    let (fns, exports) = linked(
        "use std.time.{Clock, ms, sleep}\nuse std.task.all\n\n\
         fn nap!(j: i64) -> i64 $ Clock:\n    sleep!(ms(j))\n    j\n\n\
         pub fn main!() -> void $ Console + Clock:\n    \
         let (a, b) = all!(nap(1), nap(2))\n    println(a + b)\n",
    );
    assert!(fns.iter().any(|f| f == "rt:wake_up"), "{fns:?}");
    assert!(exports.iter().any(|e| e == "hd.all_polls"), "{exports:?}");
}
