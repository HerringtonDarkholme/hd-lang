//! Known closure calls (closure-calls.md, option A1, #53): in the optimized
//! pipeline, a call of a closure value made in the same body, directly or
//! through a local bound once, is a direct `call` of the closure's code. The
//! dev pipeline keeps `call_ref`. Both run the same on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The Wasm of the program `main`, through the optimized pipeline or the
/// dev one.
fn wasm_of(main: &str, optimized: bool) -> Vec<u8> {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        optimized,
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
    out.wasm.expect("wasm")
}

/// The `call` and `call_ref` counts in the body of the function whose
/// debug name is `name`, in a module that validates.
fn calls_in(wasm: &[u8], name: &str) -> (usize, usize) {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut imports = 0u32;
    let mut index = None;
    let mut names = Vec::new();
    let mut bodies = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    if matches!(i.expect("import").ty, wasmparser::TypeRef::Func(_)) {
                        imports += 1;
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => bodies.push(body),
            wasmparser::Payload::CustomSection(c) => {
                if let wasmparser::KnownCustom::Name(r) = c.as_known() {
                    for n in r {
                        if let wasmparser::Name::Function(m) = n.expect("name") {
                            for f in m {
                                let f = f.expect("function name");
                                if f.name == name {
                                    index = Some(f.index);
                                }
                                names.push(f.name.to_owned());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let Some(index) = index else {
        panic!("no function named {name}: {names:?}");
    };
    let body = &bodies[(index - imports) as usize];
    let (mut call, mut call_ref) = (0, 0);
    let mut ops = body.get_operators_reader().expect("operators");
    while !ops.eof() {
        match ops.read().expect("operator") {
            wasmparser::Operator::Call { .. } => call += 1,
            wasmparser::Operator::CallRef { .. } => call_ref += 1,
            _ => {}
        }
    }
    (call, call_ref)
}

/// The standard output of `wasm` on V8.
fn run(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("known-closure-{name}.wasm"),
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

/// `add_one` holds only the closure call, so its `call` count is the
/// direct calls A1 made.
const BOUND_ONCE: &str = "\
fn add_one() -> i32:
    let f = fn(x: i32) -> i32: x + 1
    f(2)

pub fn main() -> void $ Console:
    println(add_one())
";

#[test]
fn a_closure_bound_once_is_called_directly_in_the_optimized_pipeline() {
    let dev = wasm_of(BOUND_ONCE, false);
    let opt = wasm_of(BOUND_ONCE, true);
    assert_eq!(calls_in(&dev, "app/main/add_one#0"), (0, 1), "dev");
    assert_eq!(calls_in(&opt, "app/main/add_one#0"), (1, 0), "optimized");
    assert_eq!(run("bound-dev", &dev), "3\n");
    assert_eq!(run("bound-opt", &opt), "3\n");
}

/// A capturing closure, a function reference, a literal called in place,
/// and a reassigned local, which stays indirect.
const FORMS: &str = "\
fn double(x: i32) -> i32:
    x * 2

fn forms(k: i32) -> i32:
    let add = fn(x: i32) -> i32: x + k
    let twice = double
    let total = add(1) + twice(2) + (fn(x: i32) -> i32: x * 7)(3)
    let swap = fn(x: i32) -> i32: x
    swap = fn(x: i32) -> i32: x - 1
    total + swap(1)

pub fn main() -> void $ Console:
    println(forms(+10))
";

#[test]
fn captures_function_references_and_literals_are_direct_but_reassigned_locals_are_not() {
    let dev = wasm_of(FORMS, false);
    let opt = wasm_of(FORMS, true);
    assert_eq!(calls_in(&dev, "app/main/forms#0").1, 4, "dev");
    assert_eq!(calls_in(&opt, "app/main/forms#0").1, 1, "optimized");
    assert_eq!(run("forms-dev", &dev), "36\n");
    assert_eq!(run("forms-opt", &opt), "36\n");
}

/// A suspending closure keeps its call protocol: the cold call is direct,
/// its suspension is polled as before.
const SUSPENDING: &str = "\
fn delayed!(value: i32) -> i32:
    value

fn drive!(base: i32) -> i32:
    loader := fn!(offset: i32) -> i32:
        delayed!(base + offset)
    loader!(2)

pub fn main!() -> void $ Console:
    println(drive!(+40))
";

#[test]
fn a_suspending_closure_is_constructed_directly_and_polled_as_before() {
    let dev = wasm_of(SUSPENDING, false);
    let opt = wasm_of(SUSPENDING, true);
    let (dev_calls, dev_refs) = calls_in(&dev, "app/main/drive#0$Body");
    let (opt_calls, opt_refs) = calls_in(&opt, "app/main/drive#0$Body");
    assert_eq!(opt_refs + 1, dev_refs, "one call_ref fewer");
    assert_eq!(opt_calls, dev_calls + 1, "one direct call more");
    assert_eq!(run("susp-dev", &dev), "42\n");
    assert_eq!(run("susp-opt", &opt), "42\n");
}
