//! Tuple rest elements (types.tuple.rest.*, expr.tuple.rest.*,
//! flow.match.spread.*): a spread pattern binds the rest list, a tuple
//! expression collects its tail into the rest element or spreads a list
//! into it, and a rest tuple spreads into a call. Programs build through
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tuple-rest-{name}.wasm"));
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
fn a_spread_pattern_binds_the_rest_list() {
    let main = "\
fn total(t: (usize, usize, List[i32]...)) -> usize:
    let (a, b, xs...) = t
    a + b + xs.len()

fn describe(t: (usize, List[i32]...)) -> usize:
    match t:
        (0, _...) => 100
        (n, xs...) => n + xs.len()

fn lengths(rows: List[(string, List[i32]...)]) -> List[usize]:
    [for (_, xs...) in rows => xs.len()]

fn nested(t: ((usize, usize), List[usize]...)) -> usize:
    let ((a, b), xs...) = t
    a + b + xs.len()

pub fn main() -> void $ Console:
    println(total((1, 2, 3, 4)))
    println(total((1, 2)))
    println(describe((0, 9)))
    println(describe((4, 5, 6)))
    let first: (string, List[i32]...) = (\"a\", 1, 2)
    let second: (string, List[i32]...) = (\"b\",)
    rows := lengths([first, second])
    println(rows[0])
    println(rows[1])
    println(nested(((1, 2), 3, 4)))
";
    assert_eq!(output_of("pattern", main), "5\n3\n100\n6\n2\n0\n5\n");
}

#[test]
fn a_tuple_expression_collects_or_spreads_into_the_rest() {
    let main = "\
fn total(t: (usize, usize, List[i32]...)) -> usize:
    let (a, b, xs...) = t
    a + b + xs.len()

fn rest_of(t: (usize, List[i32]...)) -> List[i32]:
    let (_, xs...) = t
    xs

fn label(parts...: (string, List[i32]...)) -> string:
    let (s, xs...) = parts
    \"${s}${xs.len()}\"

fn vararg(a: usize, xs...: List[i32]) -> usize: a + xs.len()

pub fn main() -> void $ Console:
    let t: (usize, usize, List[i32]...) = (1, 2, 3, 4, 5)
    println(total(t))
    println(total((1, 2)))
    tail := [+7, 8, 9]
    println(total((1, 2, tail...)))
    println(rest_of((0, 3, 4))[1])
    println(rest_of((0,)).len())
    println(label(\"a\", 1, 2, 3))
    println(label(\"b\"))
    println(label(\"c\", tail...))
    let pair: (usize, List[i32]...) = (1, 5, 6)
    println(vararg(pair...))
    let wide: (usize, usize, usize, usize, usize, usize, usize, usize, usize, List[usize]...) = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11)
    let (a, b, c, d, e, f, g, h, i, xs...) = wide
    println(a + b + c + d + e + f + g + h + i + xs.len())
";
    assert_eq!(
        output_of("expr", main),
        "6\n3\n6\n4\n0\na3\nb0\nc3\n3\n47\n"
    );
}

#[test]
fn a_spread_pattern_needs_the_arity_of_the_fixed_elements() {
    let main = "\
fn short(t: (i32, i32, List[i32]...)) -> i32:
    let (first, _rest...) = t
    first
";
    assert_eq!(codes_of(main), [Code::TypeMismatch]);
}

#[test]
fn a_spread_pattern_needs_a_rest_element() {
    let fixed = "\
fn fixed(pair: (i32, i32)) -> i32:
    let (first, _rest...) = pair
    first
";
    assert_eq!(codes_of(fixed), [Code::TypeMismatch]);
    let other = "\
fn other(n: i32) -> i32:
    let (first, _rest...) = n
    first
";
    assert_eq!(codes_of(other), [Code::TypeMismatch]);
}

#[test]
fn a_rest_tuple_pattern_needs_a_spread_pattern() {
    let main = "\
fn plain(t: (i32, List[i32]...)) -> i32:
    let (first, _rest) = t
    first
";
    assert_eq!(codes_of(main), [Code::TypeMismatch]);
}

#[test]
fn a_spread_pattern_covers_every_list() {
    let covered = "\
fn describe(t: (i32, List[i32]...)) -> i32:
    match t:
        (0, _...) => 0
        (n, _xs...) => n
";
    assert!(codes_of(covered).is_empty());
    let missing = "\
fn describe(t: (i32, List[i32]...)) -> i32:
    match t:
        (0, _...) => 0
";
    assert_eq!(codes_of(missing), [Code::NonexhaustiveMatch]);
    let refutable = "\
fn describe(t: (i32, List[i32]...)) -> i32:
    let (0, _...) = t
    0
";
    assert_eq!(codes_of(refutable), [Code::RefutableLetPattern]);
}

#[test]
fn a_tuple_expression_needs_the_fixed_elements_of_a_rest_type() {
    let short = "\
fn short() -> (i32, i32, List[i32]...):
    (1,)
";
    assert_eq!(codes_of(short), [Code::TypeMismatch]);
    let spread_short = "\
fn short(xs: List[i32]) -> (i32, i32, List[i32]...):
    (1, xs...)
";
    assert_eq!(codes_of(spread_short), [Code::TypeMismatch]);
    let wrong_item = "\
fn wrong() -> (i32, List[i32]...):
    (1, \"a\")
";
    assert_eq!(codes_of(wrong_item), [Code::TypeMismatch]);
}

#[test]
fn a_tuple_spread_takes_a_list_of_the_rest_items() {
    let tuple = "\
fn joined(pair: (i32, i32)) -> (i32, i32, i32):
    (1, pair...)
";
    assert_eq!(codes_of(tuple), [Code::TypeMismatch]);
    let items = "\
fn wrong(xs: List[string]) -> (i32, List[i32]...):
    (1, xs...)
";
    assert_eq!(codes_of(items), [Code::TypeMismatch]);
}

#[test]
fn a_rest_element_is_a_list() {
    let main = "\
fn first(values: (i32, i32...)) -> i32: 0
";
    assert_eq!(codes_of(main), [Code::TypeMismatch]);
}

#[test]
fn a_rest_tuple_converts_only_to_the_same_type() {
    let main = "\
fn plain(values: (i32, List[i32]...)) -> (i32, List[i32]):
    values
";
    assert_eq!(codes_of(main), [Code::TypeMismatch]);
}

#[test]
fn selection_reads_fixed_elements_and_the_rest_list() {
    let main = "\
fn rest_len(t: (usize, string, List[i32]...)) -> usize:
    t._0 + t._2.len()

pub fn main() -> void $ Console:
    let t: (usize, string, List[i32]...) = (1, \"a\", 5, 6, 7)
    println(t._1)
    println(rest_len(t))
    println(t._2[1])
    println(rest_len((2, \"b\")))
    pair := (+3, \"x\")
    println(pair._0)
";
    assert_eq!(output_of("select", main), "a\n4\n6\n2\n3\n");
}

#[test]
fn selection_past_the_tuple_is_unknown() {
    let fixed = "\
fn f(t: (i32, i32)) -> i32: t._2
";
    assert_eq!(codes_of(fixed), [Code::UnknownDataField]);
    let rest = "\
fn f(t: (i32, List[i32]...)) -> i32: t._2
";
    assert_eq!(codes_of(rest), [Code::UnknownDataField]);
    let inside = "\
fn f(t: (i32, i32)) -> i32: t._1
";
    assert!(codes_of(inside).is_empty());
}

#[test]
fn tuple_elements_and_shared_data_are_not_assignable() {
    let tuple = "\
fn f(t: (i32, List[i32]...)):
    t._0 = 1
";
    assert_eq!(codes_of(tuple), [Code::InvalidAssignmentTarget]);
    let rest = "\
fn f(t: (i32, List[i32]...)):
    t._1 = [+1]
";
    assert_eq!(codes_of(rest), [Code::InvalidAssignmentTarget]);
    let shared = "\
enum Code(i32):
    Missing -> Code(404)

fn f(c: mut Code):
    c._0 = 1
";
    assert_eq!(codes_of(shared), [Code::InvalidAssignmentTarget]);
}

#[test]
fn unnamed_shared_enum_parameters_select_as_underscore_members() {
    let main = "\
enum Code(i32, string):
    Missing -> Code(404, \"gone\")
    Moved -> Code(301, \"moved\")

pub fn main() -> void $ Console:
    c := Code.Moved
    println(c._0)
    println(c._1)
    println(Code.Missing._0)
";
    assert_eq!(output_of("shared", main), "301\nmoved\n404\n");
    let out_of_range = "\
enum Code(i32):
    Missing -> Code(404)

fn f(c: Code) -> i32: c._1
";
    assert_eq!(codes_of(out_of_range), [Code::UnknownDataField]);
}
