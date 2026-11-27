//! A trait bound to a C provider records its whole C table beside the
//! package host contract (#2388 step 2, `docs/future-work/FFI.md`, "Wasm"):
//! every method in source order, called or not, with its C rows, and the
//! hash a library's table must carry. Its imports take C arguments.

use hd_driver::{Executor, Goal, Host, NoClock, Packages};
use hd_host_abi::c::{Method, Num, Param, Ret, Table};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use wasmparser::ValType::{F32, I32};

const SOURCE: &str = "\
@host
pub trait Disk:
    fn write(&mut self, path: string, data: &List[u8], at: &mut u64, scale: f32) -> i32
    fn wait!(&mut self, ms: u32) -> Result[List[u8], i32]
    fn unused(&self, on: bool) -> void

pub fn main!() -> void $ Disk:
    disk := $.use(Disk)
    let mut at: u64 = 0
    let data: List[u8] = [1, 2]
    _ := disk.write(\"a.txt\", &data, &mut at, 1.5)
    _ := disk.wait!(10)
";

/// The dev build and the release build, whose inliner may copy a call
/// site, each keep one C import per called method.
#[test]
fn a_c_bound_trait_records_its_whole_table_and_takes_c_arguments() {
    check(hd_driver::OptLevel::Dev, hd_driver::BuildProfile::default());
    check(
        hd_driver::OptLevel::Optimized,
        hd_driver::BuildProfile::Release,
    );
}

fn check(level: hd_driver::OptLevel, build_profile: hd_driver::BuildProfile) {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", SOURCE);
    let store = crate::warm_store();
    let out = hd_driver::build_packages(
        &Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        },
        "app",
        &Packages {
            host_packages: vec![hd_driver::HostPackage {
                package: 0,
                identity: "c-tables".into(),
                versioned_source: None,
                manifest: "hd.toml".into(),
                declarations: hd_project::parse_manifest(
                    "[host]\n\"main.Disk\" = { c = \"hd_disk\" }\n",
                )
                .expect("manifest")
                .host,
            }],
            level,
            build_profile,
            ..Packages::default()
        },
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{level:?}: {}", out.render());
    let wasm = out.wasm.expect("the program links");
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&wasm)
        .expect("valid module");
    let metadata = hd_host_abi::schema::module_metadata(&wasm).expect("metadata");
    let contract = metadata.contract.expect("a package host contract");
    let [interface] = &contract.interfaces[..] else {
        panic!("one root interface: {contract:?}");
    };
    // The contract lists the called methods; the C table every method.
    assert_eq!(
        interface
            .methods
            .iter()
            .map(|m| m.member.as_str())
            .collect::<Vec<_>>(),
        ["write", "wait"]
    );
    let expected = Table {
        module: interface.module.clone(),
        trait_name: "Disk".into(),
        methods: vec![
            Method {
                name: "write".into(),
                params: vec![
                    ("path".into(), Param::Str),
                    (
                        "data".into(),
                        Param::List {
                            num: Num::U8,
                            mutable: false,
                        },
                    ),
                    (
                        "at".into(),
                        Param::Ref {
                            num: Num::U64,
                            mutable: true,
                        },
                    ),
                    ("scale".into(), Param::Num(Num::F32)),
                ],
                result: Ret::Num(Num::I32),
                suspends: false,
            },
            Method {
                name: "wait".into(),
                params: vec![("ms".into(), Param::Num(Num::U32))],
                result: Ret::Result(Box::new(Ret::Bytes)),
                suspends: true,
            },
            Method {
                name: "unused".into(),
                params: vec![("on".into(), Param::Bool)],
                result: Ret::Void,
                suspends: false,
            },
        ],
    };
    assert_eq!(metadata.c_tables, std::slice::from_ref(&expected));
    assert_eq!(metadata.c_tables[0].hash(), expected.hash());
    assert_eq!(
        ["write", "wait", "unused"].map(|m| expected.slot(m)),
        [Some(0), Some(1), Some(3)],
        "a suspending method's abort slot follows it"
    );
    // Each argument is an import parameter: a string and a list two each.
    let mut imports = Vec::new();
    let mut types = Vec::new();
    let mut heap_exported = false;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::TypeSection(reader) => {
                for group in reader {
                    for ty in group.expect("type").into_types() {
                        types.push(ty.unwrap_func().clone());
                    }
                }
            }
            wasmparser::Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import.expect("import");
                    if import.module == interface.module
                        && let wasmparser::TypeRef::Func(ty) = import.ty
                    {
                        imports.push((import.name.to_owned(), ty));
                    }
                }
            }
            wasmparser::Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.expect("export");
                    heap_exported |=
                        export.name == "hd.heap" && export.kind == wasmparser::ExternalKind::Memory;
                }
            }
            _ => {}
        }
    }
    // A host reads pointer arguments in the guest heap.
    assert!(heap_exported, "a module with a C table exports its heap");
    let signature = |name: &str| {
        let ty = imports.iter().find(|(n, _)| n == name).expect(name).1;
        let f = &types[ty as usize];
        (f.params().to_vec(), f.results().to_vec())
    };
    assert_eq!(
        signature("write"),
        (vec![I32, I32, I32, I32, I32, F32], vec![I32])
    );
    assert_eq!(signature("wait.start"), (vec![I32], vec![I32, I32]));
    assert!(imports.iter().all(|(n, _)| !n.starts_with("unused")));
    // One import per called method, or per part of a suspending one.
    let mut names: Vec<&str> = imports.iter().map(|(n, _)| n.as_str()).collect();
    hd_base::SliceSort::sort_stable(&mut names[..]);
    assert_eq!(
        names,
        ["wait.finish", "wait.start", "write"],
        "{level:?}: one import each"
    );
}
