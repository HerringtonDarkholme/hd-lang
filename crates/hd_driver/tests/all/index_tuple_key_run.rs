//! A tuple index key with unsuffixed literal elements has its type
//! before the chosen implementation's `Out` is read (#342,
//! `expr.index.trait.read-other`, `expr.index.trait.choice`,
//! `trait.resolve.literal-arg`). Element by element, a single fitting
//! implementation gives the literals their types; typed elements narrow
//! the candidates; plain literals take their default type only to break
//! a remaining tie. Programs build through the driver and run on V8
//! (`host/run.mjs`).

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
        &format!("index-tuple-key-{name}.wasm"),
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

/// A two-column generic matrix read and written by `(usize, usize)`.
const MATRIX: &str = "\
use std.ops.Index
use std.ops.IndexSet

struct Matrix[T]:
    cells: List[T]

impl[T] Index[(usize, usize)] for Matrix[T]:
    type Out = T
    fn index(self, at: (usize, usize)) -> T:
        self.cells[at._0 * 2 + at._1]

impl[T] IndexSet[(usize, usize), T] for Matrix[T]:
    fn index_set(&mut self, at: (usize, usize), value: T) -> void:
        self.cells[at._0 * 2 + at._1] = value
";

/// The read is `Out`, so it binds as `f64` on a `Matrix[f64]` and as
/// `T` inside a generic function.
#[test]
fn a_tuple_key_reads_as_out() {
    let main = format!(
        "{MATRIX}
fn corner[T](m: Matrix[T]) -> T:
    m[(1, 1)]

pub fn main() -> void $ Console:
    let m = Matrix {{ cells: [1.5, 2.5, 3.5, 4.5] }}
    let x: f64 = m[(0, 1)]
    println(x)
    println(m[(1, 0)] + 1.0)
    println(corner(m))
"
    );
    assert_eq!(output_of("read", &main), "2.5\n4.5\n4.5\n");
}

/// `m[(r, c)] = v` and `m[(r, c)] += v` store through
/// `IndexSet[(usize, usize), T]`, in `main` and in a generic function.
#[test]
fn a_tuple_key_stores_through_index_set() {
    let main = format!(
        "{MATRIX}
fn fill[T](m: &mut Matrix[T], v: T) -> void:
    m[(0, 1)] = v

pub fn main() -> void $ Console:
    let mut m = Matrix {{ cells: [1, 2, 3, 4] }}
    fill(&mut m, 7)
    m[(1, 0)] = 8
    m[(1, 1)] += 10
    println(m[(0, 1)])
    println(m[(1, 0)])
    println(m[(1, 1)])
"
    );
    assert_eq!(output_of("store", &main), "7\n8\n14\n");
}

/// With `Index[(usize, usize)]` and `Index[(i32, i32)]`, `(0, 1)` ties
/// and takes the default `usize`; the signed `+1` and an `i32` binding
/// narrow to the `i32` implementation.
#[test]
fn a_tie_falls_back_to_the_default_type() {
    let main = "\
use std.ops.Index

struct Grid:
    size: i32

impl Index[(usize, usize)] for Grid:
    type Out = string
    fn index(self, at: (usize, usize)) -> string:
        \"usize ${at._0} ${at._1}\"

impl Index[(i32, i32)] for Grid:
    type Out = i32
    fn index(self, at: (i32, i32)) -> i32:
        at._0 * 10 + at._1

pub fn main() -> void $ Console:
    let g = Grid { size: 0 }
    let i: i32 = 4
    let a: string = g[(0, 1)]
    let b: i32 = g[(0, +1)]
    let c: i32 = g[(2, i)]
    println(a)
    println(b)
    println(c)
";
    assert_eq!(output_of("tie", main), "usize 0 1\n1\n24\n");
}

/// A single `Index[(i32, i32)]` implementation gives `(0, 1)` its
/// element types by context.
#[test]
fn a_single_implementation_types_the_literals() {
    let main = "\
use std.ops.Index

struct Board:
    cells: List[f64]

impl Index[(i32, i32)] for Board:
    type Out = f64
    fn index(self, at: (i32, i32)) -> f64:
        self.cells[usize(at._0 * 2 + at._1)]

pub fn main() -> void $ Console:
    let b = Board { cells: [1.0, 2.0, 3.0, 4.0] }
    let x: f64 = b[(0, 1)]
    println(x)
    println(b[(1, 1)])
";
    assert_eq!(output_of("single", main), "2.0\n4.0\n");
}

/// With `Index[(u8, u8)]` and `Index[(u16, u16)]`, `(0, 1)` ties and its
/// default `(usize, usize)` fits neither, an error as for a lone literal
/// (`trait.resolve.literal-arg.no-apply`).
#[test]
fn a_default_without_an_implementation_is_an_error() {
    let main = "\
use std.ops.Index

struct Small:
    size: i32

impl Index[(u8, u8)] for Small:
    type Out = i32
    fn index(self, at: (u8, u8)) -> i32:
        self.size

impl Index[(u16, u16)] for Small:
    type Out = i32
    fn index(self, at: (u16, u16)) -> i32:
        self.size

fn read(s: Small) -> i32:
    s[(0, 1)]
";
    assert_eq!(codes_of(main), vec![Code::TypeMismatch]);
}
