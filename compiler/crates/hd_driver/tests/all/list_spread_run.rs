//! List literal spreads (expr.list.spread.*): a spread inserts a list's
//! elements at its position, each operand is evaluated once in element
//! order, and the operand must be a list of assignable elements. Programs
//! build through the driver and run on V8 (`host/run.mjs`), as the
//! conformance runner does.

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("list-spread-{name}.wasm"));
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

/// The diagnostic codes of a program that must not check.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn spreads_insert_at_their_position() {
    let main = "\
pub fn main() -> void $ Console:
    xs := [+1, +2]
    ys := [+7, +8, +9]
    a := [+0, xs..., +3]
    b := [xs..., ys...]
    c := [xs..., +5, ys..., +6, xs...]
    d := [xs...]
    println(a.len())
    println(a[0] + a[1] * 10 + a[2] * 100 + a[3] * 1000)
    println(b.len())
    println(b[4])
    println(c.len())
    println(c[2] + c[6] + c[7])
    println(d.len())
";
    assert_eq!(output_of("positions", main), "4\n3210\n5\n9\n9\n12\n2\n");
}

#[test]
fn empty_spreads_add_nothing() {
    let main = "\
pub fn main() -> void $ Console:
    let none: List[i32] = []
    a := [none..., +1, none..., none...]
    let b: List[i32] = [none..., none...]
    println(a.len())
    println(a[0])
    println(b.len())
";
    assert_eq!(output_of("empty", main), "1\n1\n0\n");
}

#[test]
fn operands_are_evaluated_once_in_element_order() {
    let main = "\
fn note(label: string, value: i32, log: mut List[string]) -> i32:
    log.push(label)
    value

fn many(label: string, log: mut List[string]) -> List[i32]:
    log.push(label)
    [+1, +2]

pub fn main() -> void $ Console:
    let log: mut List[string] = []
    xs := [note(\"a\", +0, log), many(\"b\", log)..., note(\"c\", +3, log), many(\"d\", log)...]
    println(xs.len())
    println(log.len())
    println(log[0] + log[1] + log[2] + log[3])
";
    assert_eq!(output_of("order", main), "6\n4\nabcd\n");
}

#[test]
fn the_result_is_a_fresh_list() {
    let main = "\
pub fn main() -> void $ Console:
    let mut xs = [+1, +2]
    let mut ys = [xs..., +3]
    ys[0] = +9
    xs.push(+4)
    println(xs[0])
    println(ys[0])
    println(ys.len())
";
    assert_eq!(output_of("fresh", main), "1\n9\n3\n");
}

#[test]
fn spreads_of_other_element_layouts() {
    let main = "\
pub fn main() -> void $ Console:
    names := [\"b\", \"c\"]
    all := [\"a\", names..., \"d\"]
    pairs := [(1, \"x\")]
    more := [(0, \"w\"), pairs..., (2, \"y\")]
    println(all[0] + all[1] + all[2] + all[3])
    println(more.len())
    println(more[1]._1)
";
    assert_eq!(output_of("layouts", main), "abcd\n3\nx\n");
}

#[test]
fn a_spread_operand_must_be_a_list() {
    let tuple = "\
fn f() -> List[i32]:
    t := (+1, +2)
    [t...]
";
    assert_eq!(codes_of(tuple), [Code::TypeMismatch]);
    let scalar = "\
fn f() -> List[i32]:
    [+1, 2...]
";
    assert_eq!(codes_of(scalar), [Code::TypeMismatch]);
}

#[test]
fn spread_elements_must_fit_the_list() {
    let expected = "\
fn f(xs: List[string]) -> List[i32]:
    [+1, xs...]
";
    assert_eq!(codes_of(expected), [Code::TypeMismatch]);
    let joined = "\
fn f(xs: List[string]) -> usize:
    ys := [+1, xs...]
    ys.len()
";
    assert_eq!(codes_of(joined), [Code::TypeMismatch]);
}
