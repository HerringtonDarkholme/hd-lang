//! The module builder's output validates and has the entry exports.

use hd_base::Hash128;

use crate::module::{Func, Module, Sym};
use crate::rt::Helper;

fn validate(wasm: &[u8]) {
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(wasm).expect("valid module");
}

/// An entry with nothing to run: `hd.init` and `hd.poll` over an empty
/// `main`, the exchange buffer exported as `hd.x`, and the heap as a
/// second memory.
#[test]
fn an_empty_entry_links_two_memories_and_the_entry_exports() {
    let mut m = Module::default();
    let main = Hash128(7);
    m.define(Sym::Inst(main), Func::new("main", vec![], vec![]));
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: vec![] }));
    m.export(
        "hd.poll",
        Sym::Helper(Helper::EntryPoll {
            main,
            results: 0,
            report: None,
            then: vec![],
        }),
    );
    let wasm = m.finish().expect("link");
    validate(&wasm);
    let mut memories = 0;
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::MemorySection(r) => memories += r.count(),
            wasmparser::Payload::ExportSection(r) => {
                for e in r {
                    exports.push(e.expect("export").name.to_owned());
                }
            }
            _ => {}
        }
    }
    assert_eq!(memories, 2);
    assert_eq!(exports, ["hd.init", "hd.poll", "hd.x"]);
}
