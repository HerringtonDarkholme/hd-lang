//! Positional spreads in calls (expr.call.spread.*, fn.vararg.*): a
//! spread at a `List` vararg passes the list itself, a tuple spread fills
//! the remaining parameters, and a vararg may be passed by name. Programs
//! build through the driver and run on V8 (`host/run.mjs`), as the
//! conformance runner does.

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("spread-args-{name}.wasm"));
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
fn a_spread_fills_a_list_vararg() {
    let main = "\
fn sum(values...: List[i32]) -> i32:
    let total = +0
    for v in values:
        total = total + v
    total

pub fn main() -> void $ Console:
    xs := [+1, 2, 3]
    println(sum(xs...))
    println(sum(4, 5))
    println(sum())
";
    assert_eq!(output_of("list", main), "6\n9\n0\n");
}

#[test]
fn a_spread_follows_fixed_arguments() {
    let main = "\
fn tagged(tag: i32, values...: List[i32]) -> i32: tag * 100 + i32(values.len()) * 10 + values[0]

pub fn main() -> void $ Console:
    xs := [+7, 8]
    println(tagged(1, xs...))
    println(tagged([2]..., tag=3))
";
    assert_eq!(output_of("fixed", main), "127\n312\n");
}

#[test]
fn a_tuple_spread_fills_several_parameters() {
    let main = "\
fn add3(a: i32, b: i32, c: i32) -> i32: a * 100 + b * 10 + c
fn greet(name: string, times: i32) -> string: \"${name}${times}\"

pub fn main() -> void $ Console:
    pair := (+2, +3)
    println(add3(1, pair...))
    who := (\"ada\", +2)
    println(greet(who...))
    f := add3
    println(f(4, pair...))
";
    assert_eq!(output_of("tuple", main), "123\nada2\n423\n");
}

#[test]
fn a_spread_fills_a_tuple_bounded_vararg() {
    let main = "\
use std.function.Tuple

fn pack[Args < Tuple](args...: Args) -> Args: args
fn label(parts...: (i32, string)) -> string: \"${parts._0}${parts._1}\"

pub fn main() -> void $ Console:
    pair := (+3, +4)
    packed := pack(pair...)
    println(packed._1)
    println(pack(1, \"a\")._1)
    one := (+2, \"b\")
    println(label(one...))
    println(label(1, \"a\"))
";
    assert_eq!(output_of("tuple-vararg", main), "4\na\n2b\n1a\n");
}

#[test]
fn a_vararg_is_passed_by_name() {
    let main = "\
fn sum(values...: List[i32]) -> i32:
    let total = +0
    for v in values:
        total = total + v
    total
fn label(parts...: (i32, string)) -> string: \"${parts._0}${parts._1}\"

pub fn main() -> void $ Console:
    println(sum(values=[1, 2]))
    println(label(parts=(3, \"c\")))
";
    assert_eq!(output_of("named", main), "3\n3c\n");
}

#[test]
fn a_list_spread_is_passed_through_without_a_copy() {
    // The callee pushes to the caller's own list (Q-R4.2); separate
    // arguments still collect a fresh list.
    let main = "\
fn grow(values...: mut List[i32]) -> void:
    values.push(9)

pub fn main() -> void $ Console:
    let mut mine = [+1]
    grow(mine...)
    println(mine.len())
    grow(5)
    println(mine.len())
";
    assert_eq!(output_of("pass-through", main), "2\n2\n");
}

#[test]
fn spread_errors() {
    let fixed = "\
fn plain(value: i32) -> i32: value
fn bad(xs: List[i32]) -> i32: plain(xs...)
";
    assert_eq!(codes_of(fixed), [Code::PositionalSpreadNeedsVararg]);
    let twice = "\
fn collect(values...: List[i32]) -> List[i32]: values
fn bad(xs: List[i32]) -> List[i32]: collect(xs..., 4)
";
    assert_eq!(codes_of(twice), [Code::DuplicateArgument]);
    let arity = "\
fn add(a: i32, b: i32) -> i32: a + b
fn bad(t: (i32, i32, i32)) -> i32: add(t...)
";
    assert_eq!(codes_of(arity), [Code::TypeMismatch]);
    let by_name = "\
fn collect(values...: List[i32]) -> List[i32]: values
fn bad(xs: List[i32]) -> List[i32]: collect(xs..., values=xs)
";
    assert_eq!(codes_of(by_name), [Code::DuplicateArgument]);
}
