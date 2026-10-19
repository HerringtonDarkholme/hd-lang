//! A declaration of a used module reached through the module name, where a
//! value or a call base is expected (spec/lang/05-expressions.md,
//! expr.name.qualified, expr.name.qualified.private,
//! expr.name.qualified.missing): `module.Enum.Variant`,
//! `module.Type::function(..)`, `module.Type::function` as a function value
//! and `module.Data` for a data type without fields resolve like the same
//! name imported directly. A private declaration through a module is
//! `private-import`. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SHAPES: &str = "\
@derive(Eq)
pub enum Color:
    Red
    Grey(i32)
    Rgb(i32, i32, i32)

pub struct Counter:
    pub count: i32

impl Counter:
    pub fn new() -> Counter:
        Counter { count: 0 }

    pub fn starting(count: i32) -> Counter:
        Counter { count: count }

    pub fn show(self) -> string:
        \"count ${self.count}\"

pub struct Marker:
    pass

pub trait Describe:
    fn describe(self) -> string

pub type Tally = Counter

enum Hidden:
    Only

struct Secret:
    pub count: i32

impl Secret:
    pub fn new() -> Secret:
        Secret { count: 0 }
";

fn built(main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("shapes.hd", SHAPES);
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
        &format!("module-member-{name}.wasm"),
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

/// The error codes of a program checked without running.
fn errors(main: &str) -> Vec<&'static str> {
    let out = built(main, &Goal::Analyze);
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| out.diags.code[i].as_str())
        .collect()
}

#[test]
fn a_variant_through_a_module_is_a_value() {
    let main = "use pkg.shapes

fn label(color: shapes.Color) -> string:
    if color == shapes.Color.Red:
        return \"red\"
    \"other\"

pub fn main() -> void $ Console:
    println(label(shapes.Color.Red))
    println(label(shapes.Color.Grey(3)))
";
    assert_eq!(output_of("variant", main), "red\nother\n");
}

#[test]
fn a_variant_with_a_payload_through_a_module_is_built_by_a_call() {
    let main = "use pkg.shapes

pub fn main() -> void $ Console:
    color := shapes.Color.Rgb(1, 2, 3)
    match color:
        .Rgb(r, g, b) => println(r + g + b)
        _ => println(0)
";
    assert_eq!(output_of("payload", main), "6\n");
}

#[test]
fn a_variant_through_a_module_is_a_function_value() {
    let main = "use pkg.shapes

pub fn main() -> void $ Console:
    greys := [1, 2].map(shapes.Color.Grey)
    println(greys.len())
";
    assert_eq!(output_of("variant-fn", main), "2\n");
}

#[test]
fn an_associated_function_through_a_module_type_is_called() {
    let main = "use pkg.shapes

pub fn main() -> void $ Console:
    println(shapes.Counter::new().show())
    println(shapes.Counter::starting(7).show())
";
    assert_eq!(output_of("assoc-call", main), "count 0\ncount 7\n");
}

#[test]
fn an_associated_function_through_a_module_type_is_a_function_value() {
    let main = "use pkg.shapes

pub fn main() -> void $ Console:
    counters := [4, 5].map(shapes.Counter::starting)
    println(counters[1].show())
";
    assert_eq!(output_of("assoc-value", main), "count 5\n");
}

#[test]
fn a_data_type_without_fields_through_a_module_is_a_value() {
    let main = "use pkg.shapes

fn mark(marker: shapes.Marker) -> string:
    \"marked\"

pub fn main() -> void $ Console:
    println(mark(shapes.Marker))
";
    assert_eq!(output_of("marker", main), "marked\n");
}

#[test]
fn a_type_with_fields_through_a_module_is_not_a_value() {
    let main = "use pkg.shapes

pub fn main() -> void:
    counter := shapes.Counter
    pass
";
    assert_eq!(errors(main), ["type-used-as-value"]);
}

#[test]
fn a_trait_or_alias_through_a_module_is_not_a_value() {
    let tr = "use pkg.shapes

pub fn main() -> void:
    d := shapes.Describe
    pass
";
    assert_eq!(errors(tr), ["type-used-as-value"]);
    let alias = "use pkg.shapes

pub fn main() -> void:
    t := shapes.Tally
    pass
";
    assert_eq!(errors(alias), ["type-used-as-value"]);
}

#[test]
fn a_bare_trait_or_alias_is_not_a_value() {
    let tr = "use pkg.shapes.{Describe}

pub fn main() -> void:
    d := Describe
    pass
";
    assert_eq!(errors(tr), ["type-used-as-value"]);
    let alias = "use pkg.shapes.{Tally}

pub fn main() -> void:
    t := Tally
    pass
";
    assert_eq!(errors(alias), ["type-used-as-value"]);
}

#[test]
fn a_private_declaration_through_a_module_is_private_import() {
    let variant = "use pkg.shapes

pub fn main() -> void:
    only := shapes.Hidden.Only
    pass
";
    assert_eq!(errors(variant), ["private-import"]);
    let assoc = "use pkg.shapes

pub fn main() -> void:
    secret := shapes.Secret::new()
    pass
";
    assert_eq!(errors(assoc), ["private-import"]);
}

#[test]
fn a_missing_declaration_through_a_module_is_unknown_import() {
    let variant = "use pkg.shapes

pub fn main() -> void:
    only := shapes.Missing.Only
    pass
";
    assert_eq!(errors(variant), ["unknown-import"]);
    let assoc = "use pkg.shapes

pub fn main() -> void:
    other := shapes.Missing::new()
    pass
";
    assert_eq!(errors(assoc), ["unknown-import"]);
}
