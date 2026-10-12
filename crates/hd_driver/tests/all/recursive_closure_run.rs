//! Recursive local closures (spec 03 `names.scope.recursive-closure`;
//! spec 07 `fn.recursion.closure.*`, `fn.closure.recursive-result`): a
//! closure directly initializing a `:=` or `let` binding calls the name
//! being bound from its own body, through the binding. Programs build
//! through the driver and run on V8 (`host/run.mjs`), as the conformance
//! runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("recursive-closure-{name}.wasm"),
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
fn a_short_binding_computes_a_factorial() {
    let main = "\
fn factorial_of(limit: i32) -> i32:
    factorial := fn(n: i32) -> i32:
        if n <= 1:
            1
        else:
            n * factorial(n - 1)
    factorial(limit)

pub fn main() -> void $ Console:
    println(factorial_of(5))
    println(factorial_of(1))
";
    assert_eq!(output_of("factorial", main), "120\n1\n");
}

#[test]
fn a_let_binding_with_an_expected_type_recurses() {
    let main = "\
fn total(limit: i32) -> i32:
    let sum: fn(i32) -> i32 = fn(n) -> i32:
        if n == 0:
            0
        else:
            n + sum(n - 1)
    sum(limit)

pub fn main() -> void $ Console:
    println(total(9))
";
    assert_eq!(output_of("let-expected", main), "45\n");
}

#[test]
fn a_recursive_closure_captures_another_local() {
    let main = "\
fn countdown(start: i32) -> string:
    separator := \", \"
    from := fn(n: i32) -> string:
        if n == 0:
            \"liftoff\"
        else:
            \"${n}${separator}${from(n - 1)}\"
    from(start)

pub fn main() -> void $ Console:
    println(countdown(3))
";
    assert_eq!(output_of("capture", main), "3, 2, 1, liftoff\n");
}

#[test]
fn a_let_binding_calls_the_closure_it_is_reassigned_to() {
    let main = "\
fn run() -> i32:
    let step = fn(n: i32) -> i32:
        if n == 0:
            0
        else:
            1 + step(n - 1)
    first := step(4)
    step = fn(n: i32) -> i32: 100
    first + step(4)

pub fn main() -> void $ Console:
    println(run())
";
    assert_eq!(output_of("let-reassigned", main), "104\n");
}

#[test]
fn a_nonrecursive_closure_binding_still_works() {
    let main = "\
fn double(value: i32) -> i32:
    twice := fn(n: i32): n * 2
    twice(value)

pub fn main() -> void $ Console:
    println(double(21))
";
    assert_eq!(output_of("plain", main), "42\n");
}

#[test]
fn a_recursive_closure_without_a_result_type_is_rejected() {
    let main = "\
fn sum_to(limit: i32) -> i32:
    sum := fn(n: i32):
        if n == 0: 0
        else: n + sum(n - 1)
    sum(limit)

pub fn main() -> void $ Console:
    println(sum_to(3))
";
    let out = built(main);
    assert_eq!(
        out.diags.code,
        vec![Code::RecursiveFunctionNeedsResultType],
        "{}",
        out.render()
    );
}

#[test]
fn an_expected_type_does_not_replace_the_written_result_type() {
    let main = "\
fn sum_to(limit: i32) -> i32:
    let sum: fn(i32) -> i32 = fn(n):
        if n == 0: 0
        else: n + sum(n - 1)
    sum(limit)

pub fn main() -> void $ Console:
    println(sum_to(3))
";
    let out = built(main);
    assert_eq!(
        out.diags.code,
        vec![Code::RecursiveFunctionNeedsResultType],
        "{}",
        out.render()
    );
}

/// A chain member is not the statement form `fn.recursion.closure.binding`
/// names, so its closure body cannot see the name: `names.scope.not-in-initializer`.
fn assert_unknown_name(name: &str, main: &str) {
    let out = built(main);
    assert_eq!(
        out.diags.code,
        vec![Code::UnknownName],
        "{name}: {}",
        out.render()
    );
}

#[test]
fn a_two_link_chain_closure_cannot_see_its_own_name() {
    let main = "\
pub fn main() -> void $ Console:
    a := b := fn(n: i32) -> i32:
        if n == 0: 0
        else: n + b(n - 1)
    println(a(3) + b(3))
";
    assert_unknown_name("two-link", main);
}

#[test]
fn a_three_link_chain_hides_every_chain_name_in_the_closure() {
    for own in ["a", "b", "c"] {
        let main = format!(
            "\
pub fn main() -> void $ Console:
    a := b := c := fn(n: i32) -> i32:
        if n == 0: 0
        else: n + {own}(n - 1)
    println(a(3) + b(3) + c(3))
"
        );
        assert_unknown_name(own, &main);
    }
}

#[test]
fn a_chain_closure_may_call_an_outer_function_of_the_same_name() {
    let main = "\
fn b(n: i32) -> i32:
    n * 10

pub fn main() -> void $ Console:
    a := b := fn(n: i32) -> i32:
        b(n) + 1
    println(a(3) + b(3))
";
    // The outer `b` is shadowed only after the initializer: 31 + 31.
    assert_eq!(output_of("chain-outer-name", main), "62\n");
}
