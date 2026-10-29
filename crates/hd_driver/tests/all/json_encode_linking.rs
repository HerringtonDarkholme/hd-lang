//! A concrete `json.encode` instance links its tree fallback only when its
//! selected serializer may make an irregular walk (#921).

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
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
    let packages = Packages {
        build_profile: BuildProfile::Release,
        level: OptLevel::Optimized,
        instruction_origins: true,
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

fn has_enum_tag_dispatch(names: &[String]) -> bool {
    names
        .iter()
        .any(|name| name.contains("enum_tagging") || name.contains("EnumTagging"))
}

fn has_skip_fact_search(names: &[String]) -> bool {
    names.iter().any(|name| {
        name.contains("FieldRecord.fact")
            || name.contains("Facts.find[std.serde.Skip")
            || name.contains("std.serde.SkipNone")
    })
}

#[test]
fn a_generated_regular_serializer_does_not_link_the_tree_fallback() {
    let names = function_names(
        "use std.json.{decode, encode, encode_pretty}\nuse std.serde.{Deserialize, Serialize, RenameCase, rename_all}\n\n@derive(Serialize, Deserialize)\n@rename_all(.CamelCase)\nstruct Row:\n    id: i64\n    name: string\n\npub fn main() -> void $ Console:\n    value := [Row { id: 1, name: \"one\" }]\n    text := encode(value)\n    println(text)\n    println(encode_pretty(value))\n    println(decode::[List[Row]](text).is_ok())\n",
    );
    assert!(!has_tree_fallback(&names), "{names:#?}");
    assert!(!has_enum_tag_dispatch(&names), "{names:#?}");
    assert!(!has_skip_fact_search(&names), "{names:#?}");
}

#[test]
fn empty_member_and_variant_handles_do_not_link_skip_searches() {
    let names = function_names(
        "use std.serde.Skip\nuse std.structure.{Field, Structure, Variant, Walker}\n\ntrait HasSkip:\n    fn has_skip(self) -> bool\n\nstruct Finder:\n    found: bool\n\nimpl[S] Walker[S] for Finder:\n    type Error = never\n    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:\n        self.found = self.found || v.info.facts.find::[Skip]().is_some()\n        .Ok(())\n    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:\n        self.found = self.found || h.fact::[Skip]().is_some()\n        .Ok(())\n\nimpl[T] HasSkip for T by Structure:\n    fn has_skip(self) -> bool:\n        let mut finder = Finder { found: false }\n        _ := Structure::walk(self, &mut finder)\n        finder.found\n\n@derive(HasSkip)\nstruct Row:\n    value: i64\n\npub fn main() -> void $ Console:\n    println(Row { value: 1 }.has_skip())\n",
    );
    assert!(!has_skip_fact_search(&names), "{names:#?}");
}

#[test]
fn empty_handle_facts_remove_the_present_branch() {
    let names = function_names(
        r"use std.annotation.inline
use std.serde.Skip
use std.structure.{Field, Structure, Variant, Walker}

@inline(.Never)
fn fact_was_present() -> bool:
    true

trait HasSkip:
    fn has_skip(self) -> bool

struct Finder:
    found: bool

impl[S] Walker[S] for Finder:
    type Error = never
    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        found := match v.info.facts.find::[Skip]():
            .Some(_) => fact_was_present()
            .None => false
        self.found = self.found || found
        .Ok(())
    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        found := match h.fact::[Skip]():
            .Some(_) => fact_was_present()
            .None => false
        self.found = self.found || found
        .Ok(())

impl[T] HasSkip for T by Structure:
    fn has_skip(self) -> bool:
        let mut finder = Finder { found: false }
        _ := Structure::walk(self, &mut finder)
        finder.found

@derive(HasSkip)
struct Row:
    value: i64

@derive(HasSkip)
enum Choice:
    Item(i64)

pub fn main() -> void $ Console:
    println(Row { value: 1 }.has_skip())
    println(Choice.Item(2).has_skip())
",
    );
    assert!(
        !names.iter().any(|name| name.contains("fact_was_present")),
        "{names:#?}"
    );
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
