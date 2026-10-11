//! `collect` default target (#189, types.generic.default.fill): a method's
//! type parameter the arguments leave open takes its declared default, so
//! `collect` beside a literal list finds `List[T]` instead of a literal
//! class that defaults to the wrong width. The checker test builds through
//! the driver; the run tests execute on V8 (`host/run.mjs`).

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
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("collect-default-{name}.wasm"));
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
fn a_literal_list_beside_collect_takes_the_default_list() {
    let main = "\
fn same[T < Eq](a: T, b: T) -> bool:
    a == b

pub fn main() -> void $ Console:
    let xs: List[i32] = [1, 2, 3]
    println(same(xs.iter().collect(), [1, 2, 3]))
    println(same(xs.iter().skip(1).collect(), [1, 2, 3]))
";
    assert_eq!(output_of("literal", main), "true\nfalse\n");
}

#[test]
fn a_collect_with_no_expected_type_is_a_list() {
    let main = "\
pub fn main() -> void $ Console:
    let xs: List[i32] = [4, 5]
    collected := xs.iter().collect()
    println(collected.len())
";
    assert_eq!(output_of("bare", main), "2\n");
}

#[test]
fn an_expected_mut_list_wins_over_the_default() {
    let main = "\
pub fn main() -> void $ Console:
    let xs: List[i32] = [4, 5]
    let out: mut List[i32] = xs.iter().collect()
    out.push(6)
    println(out.len())
";
    assert_eq!(output_of("mut", main), "3\n");
}

#[test]
fn an_expected_map_wins_over_the_default() {
    let main = "\
pub fn main() -> void $ Console:
    let pairs: List[(string, i32)] = [(\"a\", 1), (\"b\", 2)]
    let m: Map[string, i32] = pairs.iter().collect()
    println(m.len())
";
    assert_eq!(output_of("map", main), "2\n");
}

#[test]
fn a_target_that_is_not_from_iterator_is_still_an_error() {
    let main = "\
pub fn main() -> void $ Console:
    let xs: List[i32] = [4, 5]
    let n: i32 = xs.iter().collect()
    println(n)
";
    let out = program(main);
    assert!(
        out.diags.code.contains(&Code::UnsatisfiedTraitBound),
        "{}",
        out.render()
    );
}
