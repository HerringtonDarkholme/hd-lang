//! Payloadless variants of a subtype-boxed enum as constant globals
//! (#260, `docs/compiler/lowering-catalog.md` "Enums Over The Bound"): a
//! variant with no payload is one immutable global per enum type and tag,
//! initialized with `struct.new` of the base over the tag. Constructing it
//! reads the global instead of allocating. Enum values have no identity
//! (`expr.is.value-operand`), so every construction may share it. A
//! variant with a payload still allocates.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `Click` alone needs four slots, the value bound, so `Ev` is boxed; its
/// seven payload fields, past the flat box's four, make it a subtype box.
const PROGRAM: &str = "\
enum Ev:
    Click(x: i32, y: i32, z: i32, w: i32)
    Key(code: i32, alt: bool, shift: bool)
    Idle
    Quit

fn idle() -> Ev: Ev.Idle

fn quit() -> Ev: Ev.Quit

fn click(x: i32) -> Ev: Ev.Click(x, 2, 3, 4)

fn name(e: Ev) -> string:
    match e:
        .Click(x, _, _, _) => \"click ${x}\"
        .Key(code, _, _) => \"key ${code}\"
        .Idle => \"idle\"
        .Quit => \"quit\"

pub fn main() -> void $ Console:
    for e in [idle(), click(+1), quit(), Ev.Key(7, false, true), idle()]:
        println(name(e))
";

const OUTPUT: &str = "idle\nclick 1\nquit\nkey 7\nidle\n";

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
        &format!("payloadless-variant-{name}.wasm"),
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
fn a_payloadless_variant_is_a_constant_global_on_every_target_and_level() {
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
                backend: hd_driver::Backend::WasmGc,
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
            // `idle` and `quit` read their globals; `click` allocates.
            assert_eq!(m.news("app/main/idle#0"), 0, "{what}");
            assert_eq!(m.news("app/main/quit#0"), 0, "{what}");
            assert_eq!(m.news("app/main/click#0"), 1, "{what}");
            // One global per payloadless variant, however often it is made,
            // and one that the host `Console` provider's reference brings
            // (req.ref.host.reference-values).
            assert_eq!(m.constant_structs, 3, "{what}");
            assert_eq!(
                run(&format!("{target:?}-{level:?}"), &wasm),
                OUTPUT,
                "{what}"
            );
        }
    }
}
