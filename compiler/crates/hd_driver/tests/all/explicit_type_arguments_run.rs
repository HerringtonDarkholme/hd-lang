//! Calls through explicit type arguments (spec/lang/07-functions.md,
//! fn.generic.call-list, fn.generic.explicit.*, fn.generic.methods;
//! spec/lang/02-grammar.md, grammar.expr.bang-suffix): `f!::[T](..)`,
//! `Type::f::[T](..)`, `Type::f!::[T](..)`, `value.f!::[T](..)` and a
//! module's `m.f!::[T](..)` pass the written arguments to the callee the same
//! call without them resolves. Programs build through the driver and run on
//! V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("explicit-type-args-{name}.wasm"));
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

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const ITEMS: &str = "\
data Identity: pass

impl Identity:
    fn echo![Value](value: Value) -> Value:
        value

    fn make[Value](value: Value) -> List[Value]:
        [value]

    fn say![Value](self, value: Value) -> Value:
        value

data Box[T]:
    item: T

impl[T] Box[T]:
    fn wrap[U](item: T, other: U) -> (T, U):
        (item, other)

fn convert![T](value: T) -> T: value

fn ready!() -> i32: 1
";

#[test]
fn a_bang_call_writes_its_type_arguments_after_the_bang() {
    let main = format!(
        "{ITEMS}
pub fn main!() -> void $ Console:
    println(convert!::[i32](+7))
    println(convert!::[string](\"s\"))
"
    );
    assert_eq!(output_of("bang", &main), "7\ns\n");
}

#[test]
fn an_associated_function_takes_type_arguments_through_its_type() {
    let main = format!(
        "{ITEMS}
pub fn main!() -> void $ Console:
    println(Identity::make::[i32](42).len())
    println(Identity::echo!::[i32](42))
    println(Box::[i32]::wrap::[string](1, \"x\"))
"
    );
    assert_eq!(output_of("assoc", &main), "1\n42\n(1, x)\n");
}

#[test]
fn a_method_takes_type_arguments_after_the_bang() {
    let main = format!(
        "{ITEMS}
pub fn main!() -> void $ Console:
    identity := Identity {{}}
    println(identity.say!::[i32](43))
"
    );
    assert_eq!(output_of("method", &main), "43\n");
}

#[test]
fn a_module_function_takes_type_arguments_through_its_module() {
    let main = format!(
        "use std.task

{ITEMS}
pub fn main!() -> void $ Console:
    println(task.race!::[i32](ready(), ready()))
"
    );
    assert_eq!(output_of("module", &main), "1\n");
}

#[test]
fn too_many_type_arguments_are_an_argument_count_error() {
    for call in [
        "convert!::[i32, i32](+7)",
        "Identity::echo!::[i32, i32](42)",
        "Identity::make::[i32, i32](42).len()",
        "Identity {}.say!::[i32, i32](43)",
    ] {
        let main = format!(
            "{ITEMS}
fn use_it!() -> void:
    _ := {call}
"
        );
        assert_eq!(codes_of(&main), vec![Code::ArgumentCount], "{call}");
    }
}
