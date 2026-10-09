//! Variant constructors as function values (spec/lang/08-data-and-enums.md,
//! data.enum.fn-value.*): a one-payload constructor written without an
//! argument clause is the function that builds the variant. Programs build
//! through the driver and run on V8 (`host/run.mjs`), as the conformance
//! runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("variant-value-{name}.wasm"));
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn a_constructor_passes_to_a_map() {
    let main = "\
enum Shape:
    Circle(radius: i32)
    Square(side: i32)

fn area(shape: Shape) -> i32:
    match shape:
        .Circle(r) => r * 3
        .Square(s) => s * s

fn build(sizes: List[i32], make: fn(i32) -> Shape) -> List[Shape]:
    [for size in sizes => make(size)]

pub fn main() -> void $ Console:
    shapes := build([+1, 2], Shape.Circle)
    println(area(shapes[0]) + area(shapes[1]))
    g := Shape.Square
    println(area(g(4)))
";
    assert_eq!(output_of("map", main), "9\n16\n");
}

#[test]
fn a_generic_constructor_takes_its_arguments_from_the_call() {
    let main = "\
enum Wrapped[E]:
    Failed(error: E)
    Gone

fn wrap[T](value: T, make: fn(T) -> Wrapped[T]) -> Wrapped[T]:
    make(value)

pub fn main() -> void $ Console:
    match wrap(+7, Wrapped.Failed):
        .Failed(n) => println(n)
        .Gone => println(0)
    match wrap_some(Option.Some, 5):
        .Some(n) => println(n)
        .None => println(0)

fn wrap_some(make: fn(i32) -> Option[i32], value: i32) -> Option[i32]:
    make(value)
";
    assert_eq!(output_of("generic", main), "7\n5\n");
}

#[test]
fn constructors_that_are_not_function_values_are_errors() {
    let two = "\
enum Pair:
    Both(left: i32, right: i32)

fn apply(make: fn(i32, i32) -> Pair) -> Pair:
    make(1, 2)

fn run() -> Pair:
    apply(Pair.Both)
";
    assert_eq!(codes_of(two), vec![Code::UnsaturatedEnumConstructor]);
    let unsolved = "\
enum Either[L, R]:
    Left(value: L)
    Right(value: R)

fn run() -> void:
    _ := Either.Left
";
    assert_eq!(codes_of(unsolved), vec![Code::CannotInferType]);
}

#[test]
fn the_contextual_shorthand_is_never_a_function_value() {
    let main = "\
enum Shape:
    Circle(radius: i32)

fn apply(make: fn(i32) -> Shape) -> Shape:
    make(1)

fn run() -> Shape:
    apply(.Circle)
";
    assert_eq!(codes_of(main), vec![Code::MissingContextualEnumType]);
}
