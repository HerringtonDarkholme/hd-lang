//! An unsuffixed literal index key chooses its `Index` or `IndexSet`
//! implementation as an operator operand does (#330,
//! `expr.index.trait.choice`, `trait.resolve.literal-arg`). Programs
//! build through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

/// The standard output of a `main` program that must build and succeed.
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
        &format!("index-literal-key-{name}.wasm"),
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

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// A table read and written by `usize` and by `string` keys. A read of
/// `usize` key `k` is `size + k`; a string key stores `value + 1000`.
const TABLE: &str = "\
use std.ops.Index
use std.ops.IndexSet

data Table:
    size: i32

impl Index[usize] for Table:
    type Out = i32
    fn index(self, key: usize) -> i32:
        self.size + i32(key)

impl Index[string] for Table:
    type Out = string
    fn index(self, key: string) -> string:
        key

impl IndexSet[usize, i32] for Table:
    fn index_set(mut self, key: usize, value: i32) -> void:
        self.size = value

impl IndexSet[string, i32] for Table:
    fn index_set(mut self, key: string, value: i32) -> void:
        self.size = value + 1000
";

/// With `Index[usize]` and `Index[string]`, the literal `0` takes the
/// default type `usize`, so `table[0]` is the `Index[usize]` read; the
/// string key and a typed binding choose by their own types.
#[test]
fn literal_key_reads_by_its_default_type() {
    let main = format!(
        "{TABLE}
pub fn main() -> void $ Console:
    let table = Table {{ size: 10 }}
    let k: usize = 2
    println(table[0])
    println(table[\"a\"])
    println(table[k])
"
    );
    assert_eq!(output_of("read", &main), "10\na\n12\n");
}

/// `table[0] = v` and `table[0] += v` choose the `IndexSet[usize, i32]`
/// store by the literal's default type; a string key stores through
/// `IndexSet[string, i32]`.
#[test]
fn literal_key_stores_by_its_default_type() {
    let main = format!(
        "{TABLE}
pub fn main() -> void $ Console:
    let table: mut Table = Table {{ size: 1 }}
    table[0] = 5
    println(table[0])
    table[0] += 3
    println(table[0])
    table[\"a\"] = 7
    println(table[0])
"
    );
    assert_eq!(output_of("store", &main), "5\n8\n1007\n");
}

/// `+0` is an `i32` literal and a `u8` binding is a `u8`: no
/// implementation takes either key, so each is an error
/// (`expr.index.trait.no-read`).
#[test]
fn a_typed_key_without_an_implementation_is_an_error() {
    let signed = format!(
        "{TABLE}
fn read(table: Table) -> i32:
    table[+0]
"
    );
    assert!(codes_of(&signed).contains(&Code::TypeMismatch));
    let narrow = format!(
        "{TABLE}
fn read(table: Table, k: u8) -> i32:
    table[k]
"
    );
    assert_eq!(codes_of(&narrow), vec![Code::TypeMismatch]);
}

/// As `price.add(5)` is an error when no implementation takes `usize`
/// (`trait.resolve.literal-arg.no-apply`), so is a literal key when
/// two integer widths fit and the default fits neither.
#[test]
fn a_default_type_without_an_implementation_is_an_error() {
    let main = "\
use std.ops.Index

data Table:
    size: i32

impl Index[u8] for Table:
    type Out = i32
    fn index(self, key: u8) -> i32:
        self.size

impl Index[i64] for Table:
    type Out = i32
    fn index(self, key: i64) -> i32:
        self.size

fn read(table: Table) -> i32:
    table[5]
";
    assert_eq!(codes_of(main), vec![Code::TypeMismatch]);
}
