//! A runtime profile's host method may return a value of another trait
//! the profile declares itself: a handle the profile made
//! (`module.resource.closed`, spec/conformance/README.md "Runtime Profiles").
//! It crosses as its number, and its methods are imported from
//! `hd:<Trait>` by name with the number first. A handle trait that one
//! of its own called methods returns does not cross. A method no code
//! calls is imported by no one.

use hd_base::SliceSort as _;
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
    fn open!(self) -> *mut dyn FileHandle

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
    out.sort_stable();
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
        "{FILES}    fn again(&mut self) -> *mut dyn FileHandle\n{MAIN}    let _ = handle.again()\n"
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
    let text = format!("{FILES}    fn again(self) -> *mut dyn FileHandle\n{MAIN}");
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

/// Identical result schemas need different codecs when one method discards its
/// strings and another retains them past a later exchange-buffer overwrite.
#[test]
fn discarded_host_error_payloads_do_not_change_an_observing_methods_decoder() {
    let out = build(
        r#"
pub trait Vault:
    fn ignored(self) -> Result[void, ConsoleError]
    fn observed(self) -> Result[void, ConsoleError]

pub fn main() -> void $ Vault:
    _ := $.use(Vault).ignored()
    let saved = $.use(Vault).observed()
    _ := $.use(Vault).ignored()
    match saved:
        .Err(.Other(message)) =>
            if message != "kept λ 🦀": panic("observed host error changed")
        _ => panic("observed host error variant changed")
"#,
        &["Vault"],
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.as_deref().expect("the program links");
    let mut imports = Vec::new();
    let mut names = std::collections::BTreeMap::new();
    let mut calls = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import.expect("import");
                    if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                        imports.push((import.module, import.name));
                    }
                }
            }
            wasmparser::Payload::CustomSection(section) if section.name() == "name" => {
                let section = wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    section.data(),
                    section.data_offset(),
                ));
                for subsection in section {
                    if let wasmparser::Name::Function(map) = subsection.expect("name subsection") {
                        for entry in map {
                            let entry = entry.expect("function name");
                            names.insert(entry.index, entry.name);
                        }
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                calls.push(
                    body.get_operators_reader()
                        .expect("operators")
                        .into_iter()
                        .filter_map(|op| match op.expect("operator") {
                            wasmparser::Operator::Call { function_index } => Some(function_index),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                );
            }
            _ => {}
        }
    }
    let decoder_allocates = |method: &str| {
        let import = u32::try_from(
            imports
                .iter()
                .position(|(module, name)| *module == "hd:Vault" && *name == method)
                .expect("method import"),
        )
        .expect("function index");
        let slot = calls
            .iter()
            .find(|body| body.contains(&import))
            .expect("host slot");
        let mut pending = slot.clone();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            if names
                .get(&index)
                .is_some_and(|name| name.starts_with("rt:str"))
            {
                return true;
            }
            if let Some(body) = index
                .checked_sub(u32::try_from(imports.len()).expect("function imports"))
                .and_then(|index| calls.get(index as usize))
            {
                pending.extend(body);
            }
        }
        false
    };
    assert!(
        !decoder_allocates("ignored"),
        "discarded decoder reaches rt:str"
    );
    assert!(
        decoder_allocates("observed"),
        "observed decoder lost owned strings"
    );

    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../host/core.mjs")
        .canonicalize()
        .expect("core host");
    let script = format!(
        r"
import {{ pathToFileURL }} from 'node:url';
const {{ createCoreHost }} = await import(pathToFileURL({core:?}).href);
const host = createCoreHost({{out(){{}},err(text){{throw Error(text)}}}}, {{providers: ({{put,Enc}}) => ({{
  'hd:Vault': {{
    ignored: () => put(new Enc().leb(1).leb(2).str('discarded and overwritten').done()),
    observed: () => put(new Enc().leb(1).leb(2).str('kept λ 🦀').done()),
  }}
}})}});
const instance = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({wasm:?})),host.imports);
const result = await host.run(instance,'hd.init','hd.poll');
if (result.trapped || result.status !== 0) throw Error(JSON.stringify(result));
"
    );
    let ran = std::process::Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .output()
        .expect("Node available");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
}
