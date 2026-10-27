//! A concrete `json.encode` instance links its tree fallback only when its
//! selected serializer may make an irregular walk (#921).

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn function_names(source: &str) -> Vec<String> {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
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
        if let wasmparser::Payload::CustomSection(section) = payload.expect("payload")
            && let wasmparser::KnownCustom::Name(reader) = section.as_known()
        {
            for subsection in reader {
                if let wasmparser::Name::Function(map) = subsection.expect("name") {
                    names.extend(
                        map.into_iter()
                            .map(|f| f.expect("function").name.to_owned()),
                    );
                }
            }
        }
    }
    names
}

fn has_tree_fallback(names: &[String]) -> bool {
    names.iter().any(|name| {
        name.contains("JsonWriter")
            || name.contains("encode_fallback")
            || name.contains("encode_pretty_fallback")
    })
}

#[test]
fn a_generated_regular_serializer_does_not_link_the_tree_fallback() {
    let names = function_names(
        "use std.json.{encode, encode_pretty}\nuse std.serde.Serialize\n\n@derive(Serialize)\nstruct Row:\n    id: i64\n    name: string\n\npub fn main() -> void $ Console:\n    value := [Row { id: 1, name: \"one\" }]\n    println(encode(value))\n    println(encode_pretty(value))\n",
    );
    assert!(!has_tree_fallback(&names), "{names:#?}");
}

#[test]
fn a_custom_serializer_keeps_the_tree_fallback() {
    let names = function_names(
        "use std.json.encode\nuse std.serde.{Serialize, Serializer}\n\nstruct Odd:\n    value: i64\n\nimpl Serialize for Odd:\n    fn serialize[W < Serializer](self, out: &mut W) -> Result[void, W::Error]:\n        out.int(self.value)?\n        out.int(self.value + 1)\n\npub fn main() -> void $ Console:\n    println(encode(Odd { value: 1 }))\n",
    );
    assert!(has_tree_fallback(&names), "{names:#?}");
}

#[test]
fn a_derived_serializer_with_rename_facts_keeps_the_tree_fallback() {
    let names = function_names(
        "use std.json.encode\nuse std.serde.{Serialize, rename}\n\n@derive(Serialize)\nstruct Pair:\n    @rename(\"same\")\n    left: i64\n    @rename(\"same\")\n    right: i64\n\npub fn main() -> void $ Console:\n    println(encode(Pair { left: 1, right: 2 }))\n",
    );
    assert!(has_tree_fallback(&names), "{names:#?}");
}
