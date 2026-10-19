//! Raw identifiers end to end (#263b, lex.raw.reserved, lex.raw.name,
//! lex.raw.interpolation): a reserved word written in backticks names a
//! field, method, parameter, named argument, binding or variant, and
//! `` `name` `` is the identifier `name`. Programs build through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The standard output of a `main` program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("raw-ident-{name}.wasm"),
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
fn a_reserved_word_names_a_field() {
    let main = "\
struct Token:
    `type`: string
    `data`: i32

pub fn main() -> void $ Console:
    token := Token { `type`: \"word\", `data`: 7 }
    println(token.`type`)
    println(token.`data`)
";
    assert_eq!(output_of("field", main), "word\n7\n");
}

#[test]
fn a_reserved_word_names_a_method() {
    let main = "\
struct Token:
    text: string

impl Token:
    fn `match`(self, other: string) -> bool: self.text == other

pub fn main() -> void $ Console:
    token := Token { text: \"a\" }
    println(token.`match`(\"a\"))
    println(token.`match`(\"b\"))
";
    assert_eq!(output_of("method", main), "true\nfalse\n");
}

#[test]
fn a_reserved_word_names_a_parameter_and_a_function() {
    let main = "\
fn `use`(`for`: i32) -> i32: `for` + 1

pub fn main() -> void $ Console:
    println(`use`(+7))
";
    assert_eq!(output_of("parameter", main), "8\n");
}

#[test]
fn a_reserved_word_labels_a_named_argument() {
    let main = "\
fn pick(`if`: bool, `else`: i32 = 2) -> i32:
    if `if`:
        `else` * 10
    else:
        `else`

pub fn main() -> void $ Console:
    println(pick(`if` = true))
    println(pick(`if` = false, `else` = +5))
";
    assert_eq!(output_of("label", main), "20\n5\n");
}

#[test]
fn a_reserved_word_names_a_binding_and_a_variant() {
    let main = "\
enum `match`:
    `if`
    `else`(`for`: i32)

pub fn main() -> void $ Console:
    `type` := `match`.`else`(`for` = +9)
    r := match `type`:
        `match`.`if` => 0
        `match`.`else`(`while`) => `while`
    println(r)
";
    assert_eq!(output_of("binding", main), "9\n");
}

#[test]
fn a_raw_interpolation_reads_a_reserved_word() {
    let main = "\
struct Token:
    `type`: string

pub fn main() -> void $ Console:
    token := Token { `type`: \"word\" }
    `in` := +3
    println(\"${token.`type`} ${`in`}\")
";
    assert_eq!(output_of("interpolation", main), "word 3\n");
}

#[test]
fn backticks_around_an_ordinary_name_change_nothing() {
    let main = "\
struct Point:
    x: i32

fn twice(value: i32) -> i32: value * 2

pub fn main() -> void $ Console:
    name := +4
    println(`name`)
    `other` := +5
    println(other)
    println(`twice`(`value` = 3))
    println(twice(value = `other`))
    p := Point { `x`: +1 }
    println(p.x + p.`x`)
";
    assert_eq!(output_of("same", main), "4\n5\n6\n10\n2\n");
}
