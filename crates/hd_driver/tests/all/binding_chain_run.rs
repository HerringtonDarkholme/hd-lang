//! Binding chains (spec/lang/02-grammar.md, grammar.stmt.chain; spec/lang/
//! 05-expressions.md, expr.bind.one-name; spec/lang/03-names-and-scopes.md,
//! names.bind.no-redeclare, names.local.short): `a := b := e` is
//! `a := (b := e)`. The initializer runs once, `b` binds to its value, and
//! `a` binds to the value of that binding expression. Programs build
//! through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

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
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("binding-chain-{name}.wasm"),
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

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn a_two_link_chain_binds_both_names_to_one_value() {
    let main = "\
pub fn main() -> void $ Console:
    a := b := +7
    println(a)
    println(b)
    println(a + b)
";
    assert_eq!(output_of("two", main), "7\n7\n14\n");
}

#[test]
fn a_three_link_chain_binds_every_name() {
    let main = "\
pub fn main() -> void $ Console:
    a := b := c := \"hd\"
    println(\"$a $b $c\")
";
    assert_eq!(output_of("three", main), "hd hd hd\n");
}

#[test]
fn a_chain_runs_its_initializer_once() {
    let main = "\
struct Counter:
    n: i32

fn next(c: &mut Counter) -> i32:
    c.n = c.n + 1
    c.n

pub fn main() -> void $ Console:
    let mut counter = Counter { n: 0 }
    a := b := c := next(&mut counter)
    println(\"$a $b $c ${counter.n}\")
";
    assert_eq!(output_of("once", main), "1 1 1 1\n");
}

#[test]
fn a_chain_may_end_in_an_if_suite() {
    let main = "\
pub fn main() -> void $ Console:
    sum := +6
    a := b := if sum > 5: 1 else: 2
    println(\"$a $b\")
";
    assert_eq!(output_of("if", main), "1 1\n");
}

#[test]
fn a_chain_inside_a_suite_binds_in_that_suite() {
    let main = "\
pub fn main() -> void $ Console:
    n := +3
    if n > 0:
        x := y := n * 2
        println(\"$x $y\")
    x := +1
    println(x)
";
    assert_eq!(output_of("suite", main), "6 6\n1\n");
}

#[test]
fn a_parenthesized_chain_is_a_value() {
    let main = "\
pub fn main() -> void $ Console:
    total := (a := b := +4) + 1
    println(\"$a $b $total\")
";
    assert_eq!(output_of("paren", main), "4 4 5\n");
}

#[test]
fn a_top_level_chain_declares_every_name_once() {
    let main = "\
fn seven() -> i32:
    7

a := b := c := seven() + 1

fn sum() -> i32: a + b + c

pub fn main() -> void $ Console:
    println(\"$a $b $c ${sum()}\")
";
    assert_eq!(output_of("top", main), "8 8 8 24\n");
}

#[test]
fn a_chained_name_is_not_reassignable() {
    let inner = "\
pub fn main() -> void $ Console:
    a := b := +1
    b = 2
    println(\"$a $b\")
";
    assert_eq!(codes_of(inner), vec![Code::NonReassignableBinding]);
    let outer = "\
pub fn main() -> void $ Console:
    a := b := +1
    a = 2
    println(\"$a $b\")
";
    assert_eq!(codes_of(outer), vec![Code::NonReassignableBinding]);
}

#[test]
fn a_chain_may_not_redeclare_a_name() {
    let inner = "\
pub fn main() -> void $ Console:
    b := +0
    println(b)
    a := b := +1
    println(\"$a $b\")
";
    assert_eq!(codes_of(inner), vec![Code::DuplicateBinding]);
    let outer = "\
pub fn main() -> void $ Console:
    a := +0
    println(a)
    a := b := +1
    println(\"$a $b\")
";
    assert_eq!(codes_of(outer), vec![Code::DuplicateBinding]);
}
