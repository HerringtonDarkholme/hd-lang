//! Compiler-supplied impls as solver rows (#121; trait-solver.md §3.9):
//! bodies, header checks and codegen's `select` get the sealed traits'
//! answers from the solver. Programs build through the driver and run on
//! V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build, messages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_main(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &MemoryStore::default(),
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

/// Builds and runs a program on V8; returns its standard output.
fn output_of(name: &str, main: &str) -> String {
    let out = build_main(main);
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("sealed-{name}.wasm"));
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

/// Instances under sealed bounds collect and run: `AnyRef` and `AnyVal`
/// (a newtype over a primitive), a user trait with `AnyRef` as its
/// supertrait, `Tuple`, and `Inspectable`, whose coercion to a trait
/// value selects the compiler-supplied impl.
#[test]
fn programs_under_sealed_bounds_run() {
    let main = "\
use std.inspect.Inspectable
use std.function.Tuple

type Mile(i32)

data User:
    name: string

trait Named < AnyRef:
    fn name(self) -> string

impl Named for User:
    fn name(self) -> string: self.name

fn pick[T < AnyRef](a: T, b: T, left: bool) -> T:
    if left:
        return a
    b

fn keep[T < AnyVal](x: T) -> T: x

fn names[T < Named](xs: List[T]) -> string:
    let out = \"\"
    for x in xs:
        out = out + x.name()
    out

fn first[A < Tuple](a: A) -> A: a

fn erase[T < Inspectable](x: T) -> dyn Inspectable: x

pub fn main() -> void $ Console:
    u := User { name: \"ann\" }
    v := User { name: \"bob\" }
    println(pick(u, v, false).name)
    _ := keep(Mile(3))
    println(keep(7))
    println(names([u, v]))
    let (n, s) = first((1, \"x\"))
    println(\"$n$s\")
    _ := erase(u)
    _ := erase([(1, \"a\")])
    println(\"done\")
";
    assert_eq!(output_of("bounds", main), "bob\n7\nannbob\n1x\ndone\n");
}

/// The rows answer use sites: a data type is no `AnyVal`, a newtype over
/// one is no `AnyVal` either, and a function type is not inspectable.
#[test]
fn sealed_rows_reject_what_their_table_excludes() {
    let main = "\
use std.inspect.Inspectable

data User:
    name: string

type Owner(User)

fn keep[T < AnyVal](x: T) -> T: x

fn erase[T < Inspectable](x: T) -> dyn Inspectable: x

pub fn main() -> void:
    u := User { name: \"ann\" }
    _ := keep(u)
    _ := keep(Owner(u))
    _ := erase(fn() -> i32: 1)
";
    let msgs = messages(&build_main(main));
    assert_eq!(
        msgs,
        [
            "User does not implement AnyVal",
            "Owner does not implement AnyVal",
            "fn() -> i32 does not implement Inspectable",
        ],
        "{msgs:#?}"
    );
}

/// Header checks ask the same rows (trait-solver.md §3.7): a written
/// `Box[i32]` misses `Box`'s `AnyRef` bound, and an impl of a trait whose
/// supertrait is `AnyRef` misses it for `i32` but not for a data type.
#[test]
fn header_checks_ask_the_sealed_rows() {
    let defs = "\
pub data Box[T < AnyRef]:
    pub v: T

pub data User:
    pub name: string

pub trait Keyed < AnyRef:
    fn key(self) -> i32

impl Keyed for User:
    fn key(self) -> i32: 1

impl Keyed for i32:
    fn key(self) -> i32: self

pub fn held(b: Box[i32]) -> i32: 0

pub fn kept(b: Box[User]) -> i32: 0
";
    let mut src = MemorySources::default();
    src.insert("a/defs.hd", defs);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &MemoryStore::default(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    let mut msgs = messages(&out);
    msgs.sort();
    assert_eq!(
        msgs,
        [
            "i32 does not implement AnyRef in `held`",
            "i32 implements Keyed but not AnyRef",
        ],
        "{}",
        out.render()
    );
}
