//! A runtime profile's host method may return a value of another trait
//! the profile declares itself: a handle the profile made
//! (`module.resource.closed`, spec/conformance/README.md "Runtime Profiles").
//! It crosses as its number, and its methods are imported from
//! `hd:<Trait>` by name with the number first. A handle trait that one
//! of its own called methods returns does not cross. A method no code
//! calls is imported by no one.

use hd_cache::{CacheStore, MemoryStore};
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const FILES: &str = "\
pub enum IoError:
    Failed(message: string)
    Closed

pub trait FileHandle:
    fn read!(&mut self) -> Result[string, IoError]
    fn close(&mut self) -> Result[void, IoError]
";

const MAIN: &str = "
pub trait Files:
    fn open!(self) -> &mut dyn FileHandle

pub fn main!() -> void $ Files:
    let handle: &mut dyn FileHandle = $.use(Files).open!()
    let _ = handle.close()
    let _ = handle.read!()
";

/// Builds `main.hd` with `text` under a profile of the traits `traits`
/// that it declares, or under the default profile when there are none.
fn build(text: &str, traits: &[&str]) -> Output {
    build_in(&crate::warm_store(), text, traits)
}

/// [`build`] on `store`.
fn build_in(store: &dyn CacheStore, text: &str, traits: &[&str]) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let module = hd_project::module_path("app", "main.hd");
    let packages = Packages {
        profile: (!traits.is_empty())
            .then(|| traits.iter().map(|t| format!("{module}.{t}")).collect()),
        ..Packages::default()
    };
    build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The module's imports outside the runtime's own, as `module name`.
fn profile_imports(wasm: &[u8]) -> Vec<String> {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ImportSection(r) = payload.expect("payload") {
            for i in r.into_imports() {
                let i = i.expect("import");
                if !matches!(i.module, "hd:rt" | "hd:prim") {
                    out.push(format!("{} {}", i.module, i.name));
                }
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_returned_profile_trait_value_is_a_handle_whose_methods_take_its_number() {
    let out = build(&format!("{FILES}{MAIN}"), &["Files", "FileHandle"]);
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.as_deref().expect("the program links");
    assert_eq!(
        profile_imports(wasm),
        [
            "hd:FileHandle close",
            "hd:FileHandle read.finish",
            "hd:FileHandle read.start",
            "hd:Files open.finish",
            "hd:Files open.start",
        ]
    );
}

#[test]
fn a_handle_trait_that_its_own_method_returns_stops_at_link() {
    let text = format!(
        "{FILES}    fn again(&mut self) -> &mut dyn FileHandle\n{MAIN}    let _ = handle.again()\n"
    );
    let out = build(&text, &["Files", "FileHandle"]);
    assert!(out.wasm.is_none());
    assert!(
        out.render().contains("which a method of its own returns"),
        "{}",
        out.render()
    );
}

/// The trait method's edit leaves every body's TIR as it was. No code
/// calls the new method, so its slot is a trap that crosses nothing.
#[test]
fn an_uncalled_self_returning_handle_method_links_after_the_handle_free_build() {
    let store = crate::warm_store();
    let traits = ["Files", "FileHandle"];
    let free = build_in(&store, &format!("{FILES}{MAIN}"), &traits);
    let free = free.wasm.as_deref().expect("the handle-free program links");
    let text = format!("{FILES}    fn again(self) -> &mut dyn FileHandle\n{MAIN}");
    let out = build_in(&store, &text, &traits);
    let wasm = out.wasm.as_deref().expect("the program links");
    assert_eq!(profile_imports(wasm), profile_imports(free));
}

/// A handle trait's new method relinks, though no body changed; no code
/// calls it, so it imports nothing.
#[test]
fn a_handle_trait_edit_relinks() {
    let store = MemoryStore::default();
    let traits = ["Files", "FileHandle"];
    let cold = build_in(&store, &format!("{FILES}{MAIN}"), &traits);
    assert!(cold.wasm.is_some(), "{}", cold.render());
    let text = format!("{FILES}    fn name(self) -> string\n{MAIN}");
    let edited = build_in(&store, &text, &traits);
    let wasm = edited.wasm.as_deref().expect("the program links");
    assert_eq!(edited.counters.hit("link"), 0);
    assert_eq!(edited.counters.miss("link"), 1);
    let imports = profile_imports(wasm);
    assert!(
        !imports.contains(&"hd:FileHandle name".to_owned()),
        "{imports:?}"
    );
}

/// A host trait of the ABI table links only the methods the program
/// calls: `println` alone imports no `write_error`.
#[test]
fn an_uncalled_host_method_is_not_imported() {
    let out = build(
        "pub fn main() -> void $ Console:\n    println(\"hello\")\n",
        &[],
    );
    let Some(wasm) = out.wasm.as_deref() else {
        panic!("{}", out.render());
    };
    assert_eq!(
        profile_imports(wasm),
        ["hd:Console write.finish", "hd:Console write.start"]
    );
    let out = build(
        "use std.console.eprintln\n\npub fn main() -> void $ Console:\n    eprintln(\"oops\")\n",
        &[],
    );
    let Some(wasm) = out.wasm.as_deref() else {
        panic!("{}", out.render());
    };
    assert_eq!(
        profile_imports(wasm),
        [
            "hd:Console write_error.finish",
            "hd:Console write_error.start"
        ]
    );
}
