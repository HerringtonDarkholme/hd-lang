//! Capture-free closures as constant globals (#49, `docs/compiler/lowering-catalog.md`
//! "Numbering"): a closure literal that captures nothing, or a function
//! reference used as a value, is one immutable global per closure code,
//! initialized with `struct.new` of its environment over `ref.func`. Each
//! evaluation reads the global instead of allocating. Function values have
//! no identity (`expr.is.function-none`), so every evaluation may share
//! it. A capturing closure still allocates. Both closure code lowerings
//! (typed for `V8`, abstract for `Wasmtime`) and both levels do this.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{
    EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Output, Packages, Session,
    build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `twice` makes a capture-free literal and a function reference; `adder`
/// makes a capturing closure.
const PROGRAM: &str = "\
fn double(x: i32) -> i32: x * 2

fn apply(f: fn(i32) -> i32, x: i32) -> i32:
    f(x)

fn twice(x: i32) -> i32:
    apply(fn(y: i32) -> i32: y + 1, x) + apply(double, x) + apply(fn(y: i32) -> i32: y + 1, x)

fn adder(k: i32) -> fn(i32) -> i32:
    fn(x: i32) -> i32: x + k

pub fn main() -> void $ Console:
    println(twice(+3))
    println(twice(+4))
    println(apply(adder(+5), +1))
";

const OUTPUT: &str = "14\n18\n6\n";

/// The build of the files `files` with entry `entry` for `target` at
/// `level`, as one more module of `session` when it is given.
fn build(
    files: &[(&str, &str)],
    entry: &str,
    target: EngineTarget,
    level: OptLevel,
    session: Option<&Session>,
) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
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
        session,
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: entry.into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

/// What a module holds, after it validates.
#[derive(Debug, Default)]
struct Module {
    /// Function imports.
    func_imports: u32,
    /// Each defined global: whether it is mutable, and the function its
    /// initializer takes a `ref.func` of, if any.
    globals: Vec<(bool, Option<u32>)>,
    /// The export names.
    exports: Vec<String>,
    /// Per defined function, in order: its `struct.new` and `ref.func`
    /// counts.
    bodies: Vec<(usize, usize)>,
    /// Each function's index and debug name.
    names: Vec<(u32, String)>,
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
                        let mut func = None;
                        let mut ops = g.init_expr.get_operators_reader();
                        while !ops.eof() {
                            if let wasmparser::Operator::RefFunc { function_index } =
                                ops.read().expect("operator")
                            {
                                func = Some(function_index);
                            }
                        }
                        m.globals.push((g.ty.mutable, func));
                    }
                }
                wasmparser::Payload::ExportSection(r) => {
                    m.exports
                        .extend(r.into_iter().map(|e| e.expect("export").name.to_owned()));
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    let (mut new, mut func) = (0, 0);
                    let mut ops = body.get_operators_reader().expect("operators");
                    while !ops.eof() {
                        match ops.read().expect("operator") {
                            wasmparser::Operator::StructNew { .. } => new += 1,
                            wasmparser::Operator::RefFunc { .. } => func += 1,
                            _ => {}
                        }
                    }
                    m.bodies.push((new, func));
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

    /// The index of the function whose debug name is `name`.
    fn index(&self, name: &str) -> u32 {
        let Some((i, _)) = self.names.iter().find(|(_, n)| n == name) else {
            panic!("no function named {name}: {:?}", self.names);
        };
        *i
    }

    /// The `struct.new` and `ref.func` counts of the function `name`.
    fn body(&self, name: &str) -> (usize, usize) {
        self.bodies[(self.index(name) - self.func_imports) as usize]
    }

    /// The functions that constant globals hold closures of, but the
    /// runtime's own (`rt:`), such as the `runtime_type` slot of the host
    /// `Console` provider's reference (req.ref.host).
    fn constant_closures(&self) -> Vec<u32> {
        self.globals
            .iter()
            .filter(|(mutable, _)| !mutable)
            .filter_map(|(_, f)| *f)
            .filter(|f| {
                !self
                    .names
                    .iter()
                    .any(|(i, n)| i == f && n.starts_with("rt:"))
            })
            .collect()
    }
}

/// The standard output of `wasm` on V8.
fn run(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("constant-closure-{name}.wasm"),
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
fn a_capture_free_closure_is_a_constant_global_on_every_target_and_level() {
    for target in [EngineTarget::V8, EngineTarget::Wasmtime] {
        for level in [OptLevel::Dev, OptLevel::Optimized] {
            let what = format!("{target:?} {level:?}");
            let wasm = build(&[("main.hd", PROGRAM)], "main", target, level, None)
                .wasm
                .expect("wasm");
            let m = Module::read(&wasm);
            // `twice` allocates no closure: it reads the globals.
            assert_eq!(m.body("app/main/twice#0"), (0, 0), "{what}");
            // The capturing closure still allocates its environment.
            assert_eq!(m.body("app/main/adder#0"), (1, 1), "{what}");
            // One global per closure code: the literal written twice is
            // two literals with two codes, and `double` has one adapter.
            let mut held = m.constant_closures();
            held.sort_unstable();
            let mut want = [
                "app/main/twice#1",
                "app/main/twice#2",
                "app/main/double#adapter",
            ]
            .map(|n| m.index(n));
            want.sort_unstable();
            assert_eq!(held, want, "{what}: {:?}", m.names);
            assert_eq!(
                run(&format!("{target:?}-{level:?}"), &wasm),
                OUTPUT,
                "{what}"
            );
        }
    }
}

const IN1: &str = "\
pub fn bump(x: i32) -> i32:
    x + 1

pub fn apply(f: fn(i32) -> i32, x: i32) -> i32:
    f(x)

seed := apply(bump, +1)
";

const IN2: &str = "\
use pkg.in1.{apply, bump}

println(apply(bump, +2))
";

/// A session module keeps its closure globals to itself: it defines its
/// own over the code an earlier module exports, and never exports one.
#[test]
fn a_session_module_defines_its_own_constant_closures() {
    let mut session = Session::start();
    let first = build(
        &[("in1.hd", IN1)],
        "in1",
        EngineTarget::Wasmtime,
        OptLevel::Dev,
        Some(&session),
    );
    let m1 = Module::read(first.wasm.as_deref().expect("input 1's module"));
    assert_eq!(m1.constant_closures().len(), 1, "{:?}", m1.globals);
    session.add(first.session_added.clone().expect("what input 1 adds"));

    let both = [("in1.hd", IN1), ("in2.hd", IN2)];
    let second = build(
        &both,
        "in2",
        EngineTarget::Wasmtime,
        OptLevel::Dev,
        Some(&session),
    );
    let m2 = Module::read(second.wasm.as_deref().expect("input 2's module"));
    // Its global holds `bump`'s adapter, which input 1 defines and input 2
    // imports.
    let held = m2.constant_closures();
    assert_eq!(held.len(), 1, "{:?}", m2.globals);
    assert!(held[0] < m2.func_imports, "{held:?} {:?}", m2.names);
    for m in [&m1, &m2] {
        assert!(
            !m.exports.iter().any(|e| e.starts_with("clo.")),
            "{:?}",
            m.exports
        );
    }
}
