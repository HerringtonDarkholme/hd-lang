//! The `?` operand hint (#234, expr.try.expected): an expected type `T` on
//! `x?` gives the operand `Result[T, E]` or `T?`, by the nearest function's
//! result kind, so a generic operand call finds its type arguments. The
//! run tests execute on V8 (`host/run.mjs`).

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str) -> Output {
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

/// The standard output of a `main` program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = program(main);
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("try-hint-{name}.wasm"));
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
fn a_result_hint_picks_the_collect_target() {
    let main = "\
fn parse(text: string) -> Result[i32, string]:
    if text == \"x\":
        return .Err(\"bad\")
    .Ok(+7)

fn all(items: List[string]) -> Result[List[i32], string]:
    let xs: List[i32] = items.iter().map(parse).collect()?
    .Ok(xs)

pub fn main() -> void $ Console:
    println(all([\"a\", \"b\"]) == .Ok([+7, +7]))
    println(all([\"a\", \"x\"]) == .Err(\"bad\"))
";
    assert_eq!(output_of("result", main), "true\ntrue\n");
}

#[test]
fn an_optional_hint_picks_the_collect_target() {
    let main = "\
fn find(text: string) -> i32?:
    if text == \"x\":
        return .None
    .Some(+3)

fn all(items: List[string]) -> List[i32]?:
    let xs: List[i32] = items.iter().map(find).collect()?
    .Some(xs)

pub fn main() -> void $ Console:
    println(all([\"a\", \"b\"]) == .Some([+3, +3]))
    println(all([\"x\"]) == .None)
";
    assert_eq!(output_of("option", main), "true\ntrue\n");
}

#[test]
fn a_hint_that_does_not_fit_is_not_a_coercion() {
    let main = "\
fn parse(text: string) -> Result[usize, string]:
    .Ok(text.len())

fn run(text: string) -> Result[i32, string]:
    let n: i32 = parse(text)?
    .Ok(n)

pub fn main() -> void $ Console:
    println(run(\"a\") == .Ok(+1))
";
    let out = program(main);
    let codes: Vec<Code> = out
        .diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect();
    assert!(!codes.is_empty(), "the mismatch must be reported");
    assert!(
        !codes.contains(&Code::InvalidResultPropagation),
        "{}",
        out.render()
    );
}
