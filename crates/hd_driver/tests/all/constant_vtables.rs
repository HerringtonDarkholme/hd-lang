//! Vtables as constant globals (#271, `crates/hd_wasm/DESIGN.md`, trait
//! values): a trait value's vtable tree is `ref.func` of its adapters under
//! `struct.new`, all constant, so it is one immutable global, and a
//! coercion reads it instead of allocating. Vtables have no identity and
//! are never written, so every coercion may share one.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `as_shape` coerces a data value, already a reference, to a trait
/// value: the only allocation it could make is the vtable.
const PROGRAM: &str = "trait Shape:
    fn area(self) -> i32

trait Named < Shape:
    fn name(self) -> string

struct Square:
    side: i32

impl Shape for Square:
    fn area(self) -> i32: self.side * self.side

impl Named for Square:
    fn name(self) -> string: \"square\"

fn as_shape(s: &dyn Shape) -> &dyn Shape:
    s

fn as_named(s: &dyn Named) -> &dyn Named:
    s

pub fn main() -> void $ Console:
    a := as_shape(&Square { side: 3 })
    b := as_named(&Square { side: 4 })
    println(a.area())
    println(b.name())
    println(b.area())
";

const OUTPUT: &str = "9\nsquare\n16\n";

/// What a valid module holds: per defined function its `struct.new`
/// count, the function names, and the immutable globals whose
/// initializer is a `struct.new`.
#[derive(Default)]
struct Module {
    func_imports: u32,
    bodies: Vec<usize>,
    names: Vec<(u32, String)>,
    constant_structs: usize,
}

impl Module {
    fn read(wasm: &[u8]) -> Module {
        wasmparser::Validator::new()
            .validate_all(wasm)
            .expect("the module validates");
        let mut m = Module::default();
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            match payload.expect("payload") {
                wasmparser::Payload::ImportSection(r) => {
                    for i in r.into_imports() {
                        if matches!(i.expect("import").ty, wasmparser::TypeRef::Func(_)) {
                            m.func_imports += 1;
                        }
                    }
                }
                wasmparser::Payload::GlobalSection(r) => {
                    for g in r {
                        let g = g.expect("global");
                        let mut ops = g.init_expr.get_operators_reader();
                        let mut new = false;
                        while !ops.eof() {
                            if let wasmparser::Operator::StructNew { .. } =
                                ops.read().expect("operator")
                            {
                                new = true;
                            }
                        }
                        if new && !g.ty.mutable {
                            m.constant_structs += 1;
                        }
                    }
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    let mut new = 0;
                    let mut ops = body.get_operators_reader().expect("operators");
                    while !ops.eof() {
                        if let wasmparser::Operator::StructNew { .. } =
                            ops.read().expect("operator")
                        {
                            new += 1;
                        }
                    }
                    m.bodies.push(new);
                }
                wasmparser::Payload::CustomSection(c) => {
                    if let wasmparser::KnownCustom::Name(r) = c.as_known() {
                        for n in r {
                            if let wasmparser::Name::Function(fs) = n.expect("name") {
                                for f in fs {
                                    let f = f.expect("function name");
                                    m.names.push((f.index, f.name.to_owned()));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        m
    }

    /// The `struct.new` count of the function `name`.
    fn news(&self, name: &str) -> usize {
        let Some((i, _)) = self.names.iter().find(|(_, n)| n == name) else {
            panic!("no function named {name}: {:?}", self.names);
        };
        self.bodies[(i - self.func_imports) as usize]
    }
}

/// The standard output of `wasm` on V8.
fn run(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("constant-vtable-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

#[test]
fn a_trait_value_coercion_reads_a_constant_vtable() {
    for target in [EngineTarget::V8, EngineTarget::Wasmtime] {
        for level in [OptLevel::Dev, OptLevel::Optimized] {
            let what = format!("{target:?} {level:?}");
            let mut src = MemorySources::default();
            src.insert("main.hd", PROGRAM);
            let store = crate::warm_store();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let packages = Packages {
                target,
                level,
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
            assert!(out.diags.is_empty(), "{what}: {}", out.render());
            let wasm = out.wasm.expect("wasm");
            let m = Module::read(&wasm);
            // The coercions allocate nothing: they read the vtable globals.
            assert_eq!(m.news("app/main/as_shape#0"), 0, "{what}");
            assert_eq!(m.news("app/main/as_named#0"), 0, "{what}");
            // One global per vtable tree: `dyn Shape` and `dyn Named` (which
            // holds `Shape`'s table as its parent) at `Square`.
            assert!(m.constant_structs >= 2, "{what}: {}", m.constant_structs);
            assert_eq!(
                run(&format!("{target:?}-{level:?}"), &wasm),
                OUTPUT,
                "{what}"
            );
        }
    }
}
