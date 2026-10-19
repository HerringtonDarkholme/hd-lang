//! Closure code per engine target (`crates/hd_opt/DESIGN.md` (optimization pipelines) §4 and §5, job (d) of
//! #162): `V8` and `Browser` give the closure base a typed code field;
//! `Wasmtime` an abstract `funcref` that a call casts (`crates/hd_opt/DESIGN.md` (known closure calls) §7,
//! E1). Every target's module is valid Wasm GC on every engine, so a module
//! built for one engine runs, with the same output, on the other: the `V8`
//! build on wasmtime (with the `wasmtime` feature) and the `Wasmtime` build
//! on Node.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Closure values called through `call_ref` (a returned closure, a
/// function reference, a stored field, a list of closures), and the
/// `List.iter()` and `Map.iter()` cursors, which are closures too.
const CLOSURES: &str = "\
data Button:
    on_click: fn(i32) -> i32

fn double(x: i32) -> i32: x * 2

fn apply(f: fn(i32) -> i32, x: i32) -> i32:
    f(x)

fn adder(k: i32) -> fn(i32) -> i32:
    fn(x: i32) -> i32: x + k

fn minus_one(x: i32) -> i32: x - 1

pub fn main() -> void $ Console:
    add3 := adder(+3)
    println(apply(add3, +4))
    println(apply(double, +5))
    button := Button { on_click: minus_one }
    println((button.on_click)(+10))
    let fs: List[fn(i32) -> i32] = [add3, double, adder(+100)]
    let total = +0
    for f in fs:
        total = total + f(+1)
    println(total)
    let listed = fs.iter().map(fn(f): f(+2)).fold(0, fn(acc, x): acc + x)
    println(listed)
    let m: mut Map[string, i32] = {\"a\": 1, \"b\": 2}
    let it: &mut Iterator[(string, i32)] = m.iter()
    let sum = +0
    for (_, v) in it:
        sum = sum + v * 10
    println(sum)
";

const OUTPUT: &str = "7\n10\n9\n107\n111\n30\n";

/// The module of `CLOSURES` built for `target` at `level`.
fn wasm_for(target: EngineTarget, level: OptLevel) -> Vec<u8> {
    let mut src = MemorySources::default();
    src.insert("main.hd", CLOSURES);
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
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "app", &packages, &goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("the module validates");
    wasm
}

/// How many struct fields of the module's types are an abstract
/// `funcref`: the `Abstract` closure base and every type extending it.
fn abstract_code_fields(wasm: &[u8]) -> usize {
    let mut n = 0;
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::TypeSection(r) = payload.expect("payload") {
            for group in r {
                for ty in group.expect("rec group").into_types() {
                    if let wasmparser::CompositeInnerType::Struct(s) = &ty.composite_type.inner {
                        n += s
                            .fields
                            .iter()
                            .filter(|f| {
                                f.element_type
                                    == wasmparser::StorageType::Val(wasmparser::ValType::FUNCREF)
                            })
                            .count();
                    }
                }
            }
        }
    }
    n
}

/// `wasm`'s stdout on Node (`host/run.mjs`).
fn run_on_node(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("closure-code-{name}.wasm"),
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

/// `wasm`'s stdout on wasmtime, with the engine options of `level`.
#[cfg(feature = "wasmtime")]
fn run_on_wasmtime(name: &str, wasm: &[u8], level: OptLevel) -> String {
    let setup = hd_run::HostSetup {
        providers: Vec::new(),
        grants: hd_run::Grants::default(),
        limits: hd_run::Limits::default(),
        args: Vec::new(),
        program: String::new(),
    };
    let profile = hd_run_wasmtime::RuntimeProfile::from_name("default").expect("profile");
    let config = hd_run_wasmtime::EngineConfig::pipeline(level);
    let ran = hd_run_wasmtime::run_captured_with_profile(config, profile, wasm, &setup)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(
        ran.status,
        0,
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

/// The `V8` and `Browser` builds keep typed code fields; the `Wasmtime`
/// build stores closure code as an abstract `funcref`, so the modules
/// differ.
#[test]
fn closure_code_is_abstract_for_wasmtime_only() {
    for level in OptLevel::ALL {
        let v8 = wasm_for(EngineTarget::V8, level);
        let wasmtime = wasm_for(EngineTarget::Wasmtime, level);
        assert_eq!(abstract_code_fields(&v8), 0, "V8 {level:?}");
        assert_eq!(
            abstract_code_fields(&wasm_for(EngineTarget::Browser, level)),
            0,
            "Browser {level:?}"
        );
        assert!(abstract_code_fields(&wasmtime) > 0, "Wasmtime {level:?}");
        assert_ne!(v8, wasmtime, "{level:?}");
    }
}

/// Each target's module runs on Node with the same output, the
/// `Wasmtime` build included.
#[test]
fn every_target_build_runs_on_node() {
    for level in OptLevel::ALL {
        for target in EngineTarget::ALL {
            let name = format!("{target:?}-{level:?}");
            let wasm = wasm_for(target, level);
            assert_eq!(run_on_node(&name, &wasm), OUTPUT, "{name}");
        }
    }
}

/// Each target's module runs on wasmtime with the same output, the `V8`
/// build included.
#[cfg(feature = "wasmtime")]
#[test]
fn every_target_build_runs_on_wasmtime() {
    for level in OptLevel::ALL {
        for target in EngineTarget::ALL {
            let name = format!("{target:?}-{level:?}");
            let wasm = wasm_for(target, level);
            assert_eq!(run_on_wasmtime(&name, &wasm, level), OUTPUT, "{name}");
        }
    }
}
