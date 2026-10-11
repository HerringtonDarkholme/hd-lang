//! Arity-generic function types (spec/lang/07-functions.md,
//! fn.type.ctor.inputs, fn.type.tuple-vararg-input, fn.vararg.tuple-param.*;
//! spec/lang/05-expressions.md, expr.call.spread.inputs): `Fn[Args, O, $ R]`
//! with `Args < Tuple` takes a function of any arity, and `f(args...)`
//! calls it with the tuple that instantiation fixes. Programs build through
//! the driver and run on V8 (`host/run.mjs`), as the conformance runner
//! does.

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
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("arity-generic-{name}.wasm"));
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

const CALL: &str = "\
use std.function.{Fn, SuspendFn, Tuple}

fn call[Args < Tuple, O, $R](f: Fn[Args, O, $ R], args...: Args) -> O $ R:
    f(args...)

fn through[Args < Tuple, O](f: Fn[Args, O, $()], args: Args) -> O:
    f(args...)

fn retry![Args < Tuple, O](f: SuspendFn[Args, O, $()], args: Args) -> O:
    f!(args...)
";

#[test]
fn args_binds_tuples_of_every_arity() {
    let main = format!(
        "{CALL}
fn zero() -> i32: 7
fn neg(a: i32) -> i32: 0 - a
fn join(a: string, b: string, c: string) -> string: a + b + c
fn tail(tag: string, xs...: List[i32]) -> string: \"${{tag}}${{xs.len()}}\"

pub fn main() -> void $ Console:
    println(call(zero))
    println(call(neg, 5))
    println(call(join, \"a\", \"b\", \"c\"))
    println(call(tail, \"t\", 1, 2, 3))
    println(call(tail, \"u\"))
"
    );
    assert_eq!(output_of("arities", &main), "7\n-5\nabc\nt3\nu0\n");
}

#[test]
fn a_spread_through_a_value_passes_each_element() {
    // Ten `i64`s make a tuple the layout boxes; the bang call suspends.
    let main = format!(
        "{CALL}
fn big(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64, h: i64, i: i64, j: i64) -> i64:
    a + b + c + d + e + f + g + h + i + j
fn twice!(a: i32, b: i32) -> i32: (a + b) * 2
fn pair(a: string, b: i32) -> string: \"${{a}}${{b}}\"

pub fn main!() -> void $ Console:
    println(through(big, (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)))
    println(retry!(twice, (3, 4)))
    t := (\"n\", +4)
    println(through(pair, t))
    println(call(pair, t...))
    println(call(pair, args=(\"m\", 5)))
"
    );
    assert_eq!(output_of("spread", &main), "55\n14\nn4\nn4\nm5\n");
}

#[test]
fn a_generic_caller_forwards_its_arguments() {
    let main = format!(
        "{CALL}
fn twice_through[Args < Tuple](f: Fn[Args, i32, $()], args: Args) -> i32:
    through(f, args) + call(f, args...)

fn add(a: i32, b: i32) -> i32: a + b

pub fn main() -> void $ Console:
    println(twice_through(add, (2, 3)))
"
    );
    assert_eq!(output_of("forward", &main), "10\n");
}

#[test]
fn args_bound_to_several_arities_checks_clean() {
    let main = format!(
        "{CALL}
fn none() -> bool: true
fn one(a: i32) -> i32: a
fn two(a: i32, b: string) -> string: b

fn uses() -> i32:
    _ := call(none)
    _ := call(two, 1, \"b\")
    call(one, 1)
"
    );
    assert_eq!(codes_of(&main), Vec::<Code>::new());
}

#[test]
fn a_tuple_argument_that_does_not_fill_args_is_a_mismatch() {
    let main = format!(
        "{CALL}
fn add(a: i32, b: i32) -> i32: a + b

fn nested() -> i32: call(add, (1, 2))
fn short() -> i32: through(add, (1,))
"
    );
    assert_eq!(
        codes_of(&main),
        vec![Code::TypeMismatch, Code::TypeMismatch]
    );
}

#[test]
fn a_spread_whose_arity_is_not_known_is_rejected() {
    // `Args` is not the type of `add`'s inputs, nor of `B`.
    let main = "\
use std.function.{Fn, Tuple}

fn add(a: i32, b: i32) -> i32: a + b

fn fixed[Args < Tuple](args: Args) -> i32:
    add(args...)

fn other[A < Tuple, B < Tuple](f: Fn[A, i32, $()], b: B) -> i32:
    f(b...)

fn known[A < Tuple](f: Fn[A, i32, $()]) -> i32:
    f((1, 2)...)
";
    assert_eq!(
        codes_of(main),
        vec![Code::TypeMismatch, Code::TypeMismatch, Code::TypeMismatch]
    );
}

#[test]
fn separate_arguments_through_unknown_inputs_are_rejected() {
    let main = "\
use std.function.{Fn, Tuple}

fn separate[Args < Tuple](f: Fn[Args, i32, $()]) -> i32:
    f(1, 2)
";
    assert_eq!(codes_of(main), vec![Code::TypeMismatch]);
}

#[test]
fn inputs_that_are_not_a_tuple_are_a_kind_mismatch() {
    let main = "\
use std.function.{Fn, Tuple}

fn concrete(callback: Fn[i32, i32, $()]) -> void: pass
fn unbounded[Args, O](callback: Fn[Args, O, $()]) -> void: pass
fn bounded[Args < Tuple, O](callback: Fn[Args, O, $()]) -> void: pass
fn later[F < Tuple, Args < Tuple](callback: Fn[Args, F, $()]) -> void: pass
";
    assert_eq!(
        codes_of(main),
        vec![Code::GenericKindMismatch, Code::GenericKindMismatch]
    );
}
