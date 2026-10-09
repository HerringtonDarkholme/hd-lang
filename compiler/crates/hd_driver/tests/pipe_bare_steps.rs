//! Bare pipe steps (expr.pipe.bare.*): `value |> path` is the call
//! `path(value)`, a method-reference step is the same call, and a step
//! that is not a bare name needs `_`. Programs build through the driver
//! and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("pipe-bare-{name}.wasm"));
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

/// The diagnostic codes of a program that must not build.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn a_bare_function_step_calls_it() {
    let main = "\
fn double(n: i32) -> i32: n * 2
fn inc(n: i32) -> i32: n + 1

pub fn main() -> void $ Console:
    x := +20
    println(x |> double |> inc)
    println(3 |> double)
    f := inc
    println(x |> f)
";
    assert_eq!(output_of("function", main), "41\n6\n21\n");
}

#[test]
fn a_generic_function_step_infers_from_the_value() {
    let main = "\
fn twice[T](value: T) -> (T, T): (value, value)

pub fn main() -> void $ Console:
    pair := \"a\" |> twice
    println(pair._1)
    println(+7 |> twice |> _._0)
";
    assert_eq!(output_of("generic", main), "a\n7\n");
}

#[test]
fn a_method_step_and_a_method_reference_step_call_the_method() {
    let main = "\
data Scale:
    factor: i32

impl Scale:
    fn apply(self, n: i32) -> i32: n * self.factor
    fn make(n: i32) -> Scale: Scale { factor: n }

pub fn main() -> void $ Console:
    s := Scale { factor: 3 }
    println(+5 |> s.apply)
    println(+4 |> Scale::make |> _.factor)
";
    assert_eq!(output_of("method", main), "15\n4\n");
}

#[test]
fn the_piped_value_is_evaluated_once_and_first() {
    let main = "\
fn show(n: i32) -> i32 $ Console:
    println(n)
    n

pub fn main() -> void $ Console:
    x := 1 |> show
    println(x)
";
    assert_eq!(output_of("order", main), "1\n1\n");
}

#[test]
fn a_step_that_is_not_callable_is_not_callable() {
    let main = "\
pub fn main() -> void $ Console:
    n := +1
    m := +2
    println(n |> m)
";
    assert_eq!(codes_of(main), [Code::NotCallable]);
}

#[test]
fn a_call_step_without_a_placeholder_needs_one() {
    let main = "\
fn scale(value: i32, by: i32) -> i32: value * by
fn same[T](value: T) -> T: value

pub fn main() -> void $ Console:
    println(2 |> scale(3))
    println(2 |> same::[i32])
";
    assert_eq!(
        codes_of(main),
        [
            Code::PipeStepNeedsPlaceholder,
            Code::PipeStepNeedsPlaceholder
        ]
    );
}

#[test]
fn a_suspending_bare_step_is_an_error() {
    let main = "\
fn fetch!(id: i32) -> i32: id

fn loaded!(n: i32) -> i32:
    n |> fetch

pub fn main() -> void $ Console:
    println(1)
";
    assert_eq!(codes_of(main), [Code::SuspendingPipeStep]);
}
