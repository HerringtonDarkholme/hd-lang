//! Slicing (spec/lang/05-expressions.md, expr.index.slice.*): a list or
//! string indexed by a range is the call of `std.ops.Index`, with `usize`
//! as the expected type of the range's bounds. Programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::{Command, Output as Process};

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

/// Runs a program that must build.
fn run(name: &str, main: &str) -> Process {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("slicing-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let ran = run(name, main);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

/// The standard error of a program that must build and panic.
fn panic_of(name: &str, main: &str) -> String {
    let ran = run(name, main);
    assert!(!ran.status.success(), "{name}: the program did not panic");
    String::from_utf8(ran.stderr).expect("UTF-8")
}

/// The diagnostic codes of a program that must not build.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn every_range_form_slices_a_string() {
    let main = "\
pub fn main() -> void $ Console:
    text := \"hello\"
    println(text[1..3])
    println(text[1..=3])
    println(text[2..])
    println(text[..2])
    println(text[..=2])
    println(text[..])
    println(text[2..2].len())
";
    assert_eq!(
        output_of("string-forms", main),
        "el\nell\nllo\nhe\nhel\nhello\n0\n"
    );
}

#[test]
fn every_range_form_slices_a_list() {
    let main = "\
fn show(items: List[i32]) -> void $ Console:
    let line = \"\"
    for item in items:
        line = \"${line}${item} \"
    println(line)

pub fn main() -> void $ Console:
    let items: List[i32] = [10, 20, 30, 40]
    show(items[1..3])
    show(items[1..=3])
    show(items[2..])
    show(items[..2])
    show(items[..=2])
    show(items[..])
";
    assert_eq!(
        output_of("list-forms", main),
        "20 30 \n20 30 40 \n30 40 \n10 20 \n10 20 30 \n10 20 30 40 \n"
    );
}

#[test]
fn a_slice_is_a_value_to_use() {
    let main = "\
pub fn main() -> void $ Console:
    items := [10, 20, 30, 40]
    println(items[1..3].len())
    println(\"hello\"[1..4].len())
    let mut part = items[1..]
    part.push(50)
    println(part.len() + items.len())
    println(\"héllo\"[0..3])
";
    assert_eq!(output_of("use", main), "2\n3\n8\nhé\n");
}

#[test]
fn a_slice_bound_of_another_unsigned_type_selects_its_implementation() {
    let main = "\
pub fn main() -> void $ Console:
    let n: u8 = 3
    let wide: u64 = 2
    println(\"hello\"[1..n])
    println([1, 2, 3, 4][wide..][0])
    let start: usize = 1
    println(\"hello\"[start..=3])
";
    assert_eq!(output_of("bound-types", main), "el\n3\nell\n");
}

#[test]
fn a_range_value_is_a_slice_key() {
    let main = "\
use std.ops.{Range, RangeFrom, RangeFull}

pub fn main() -> void $ Console:
    let r: Range[usize] = Range { start: 1, end: 3, inclusive: false }
    let from: RangeFrom[usize] = RangeFrom { start: 3 }
    println(\"hello\"[r])
    println([1, 2, 3, 4][from][0])
    println(\"abc\"[RangeFull {}])
";
    assert_eq!(output_of("range-value", main), "el\n4\nabc\n");
}

#[test]
fn a_slice_type_is_the_out_type_of_its_implementation() {
    let main = "\
use std.ops.{Index, Range}

fn head[C < Index[Range[usize]]](items: C) -> C::Out:
    items[0..1]

fn texts(text: string) -> string: head(text)
fn lists(items: List[i32]) -> List[i32]: head(items)
fn grown(items: List[i32]) -> List[i32]:
    let mut part = items[0..2]
    part.push(9)
    part

pub fn main() -> void $ Console:
    println(texts(\"abc\"))
    println(lists([4, 5]).len())
    println(grown([1, 2, 3])[2])
";
    assert_eq!(output_of("out-type", main), "a\n1\n9\n");
}

#[test]
fn a_slice_has_the_type_of_the_receiver() {
    let ok = "\
fn text(s: string) -> string: s[1..2]
fn items(l: List[i32]) -> List[i32]: l[..=1]
";
    assert_eq!(codes_of(ok), []);
    let wrong = "\
fn text(s: string) -> List[i32]: s[1..2]
fn items(l: List[i32]) -> string: l[..]
";
    assert_eq!(codes_of(wrong), [Code::TypeMismatch, Code::TypeMismatch]);
}

#[test]
fn a_signed_or_negated_bound_is_a_type_mismatch() {
    let main = "\
fn invalid(items: List[i32], start: i32) -> void:
    a := items[-1..]
    b := items[start..]
    c := \"text\"[..-1]
";
    let codes = codes_of(main);
    assert!(
        codes.iter().filter(|c| **c == Code::TypeMismatch).count() == 3,
        "{codes:?}"
    );
}

#[test]
fn a_slice_is_not_a_place() {
    let main = "\
use std.ops.Range

fn invalid(items: mut List[i32], text: string) -> void:
    items[0..2] = [7, 8]
    items[..] = [1]
    text[0..1] = \"a\"
    let r: Range[usize] = Range { start: 0, end: 1, inclusive: false }
    items[r] = [3]
";
    assert_eq!(
        codes_of(main),
        [
            Code::InvalidAssignmentTarget,
            Code::InvalidAssignmentTarget,
            Code::InvalidAssignmentTarget,
            Code::InvalidAssignmentTarget
        ]
    );
}

#[test]
fn an_end_past_the_length_panics() {
    let main = "\
pub fn main() -> void $ Console:
    items := [1, 2, 3]
    println(items[0..9].len())
";
    assert!(panic_of("list-end", main).contains("panic: index-out-of-bounds"));
    let main = "\
pub fn main() -> void $ Console:
    println(\"abc\"[1..4])
";
    assert!(panic_of("string-end", main).contains("panic: index-out-of-bounds"));
}

#[test]
fn an_inclusive_end_at_the_length_panics() {
    let main = "\
pub fn main() -> void $ Console:
    println([1, 2, 3][..=3].len())
";
    assert!(panic_of("inclusive", main).contains("panic: index-out-of-bounds"));
}

#[test]
fn a_start_after_the_end_panics() {
    let main = "\
pub fn main() -> void $ Console:
    items := [1, 2, 3]
    println(items[2..1].len())
";
    assert!(panic_of("list-reversed", main).contains("panic: index-out-of-bounds"));
    let main = "\
pub fn main() -> void $ Console:
    println(\"abc\"[3..1])
";
    assert!(panic_of("string-reversed", main).contains("panic: index-out-of-bounds"));
}

#[test]
fn a_string_slice_inside_a_scalar_panics() {
    let main = "\
pub fn main() -> void $ Console:
    println(\"héllo\"[0..2])
";
    assert!(panic_of("boundary-end", main).contains("panic: index-out-of-bounds"));
    let main = "\
pub fn main() -> void $ Console:
    println(\"héllo\"[2..])
";
    assert!(panic_of("boundary-start", main).contains("panic: index-out-of-bounds"));
}
