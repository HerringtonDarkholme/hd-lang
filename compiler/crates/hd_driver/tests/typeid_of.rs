//! `TypeId::of::[T]()` (trait.typeid.of) and the qualified associated
//! calls with explicit type arguments it needs (`Type::f::[T]()`,
//! fn.generic.explicit.*, fn.ref.lookup): the owner resolves first, then the
//! member, then the written type arguments. Checked through the driver, then
//! built and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run_goal(main: &str, goal: &Goal) -> Output {
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

fn errors(src: &str) -> Vec<Code> {
    let out = run_goal(src, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const DECLS: &str = "use std.inspect.{Inspectable, TypeId}

data Box[T]:
    value: T

impl[T] Box[T]:
    fn pick[V](value: V) -> V:
        value

    fn make[V](value: V) -> Box[V]:
        Box { value: value }

impl Box[i32]:
    fn zero() -> Box[i32]:
        Box { value: 0 }

trait Maker:
    fn made[V](value: V) -> V

impl Maker for string:
    fn made[V](value: V) -> V:
        value

data User:
    name: string
";

/// The owner is a type, a generic type at written arguments, a trait value
/// or a parameter; the member's own arguments follow.
#[test]
fn qualified_calls_resolve_owner_then_member_then_type_arguments() {
    let cases = [
        "fn run() -> TypeId:\n    TypeId::of::[User]()\n",
        "fn run() -> TypeId:\n    TypeId::of::[List[mut User]]()\n",
        "fn run[T < Inspectable]() -> TypeId:\n    TypeId::of::[T]()\n",
        "fn run() -> i32:\n    Box::pick::[i32](3)\n",
        "fn run() -> i32:\n    Box::[string]::pick::[i32](3)\n",
        "fn run() -> Box[i64]:\n    Box::make::[i64](3)\n",
        "fn run() -> Box[string]:\n    Box::[i32]::make::[string](\"x\")\n",
        "fn run() -> string:\n    Maker::made::[string](\"x\")\n",
    ];
    for c in cases {
        let src = format!("{DECLS}\n{c}");
        assert_eq!(errors(&src), vec![], "{c}");
    }
}

/// A trailing slot is inferred from the value arguments
/// (fn.generic.explicit.trailing).
#[test]
fn a_qualified_call_infers_what_it_omits() {
    let src = format!("{DECLS}\nfn run() -> Box[i32]:\n    Box::make(3)\n");
    assert_eq!(errors(&src), vec![]);
}

/// The result of a qualified call is an ordinary call: it fits its
/// expected type like any other.
#[test]
fn the_result_is_checked_against_its_use() {
    let src = format!("{DECLS}\nfn run() -> string:\n    TypeId::of::[User]()\n");
    assert_eq!(errors(&src), vec![Code::TypeMismatch]);
}

/// Errors of the owner, the member and the type arguments.
#[test]
fn qualified_call_errors_are_reported() {
    let cases = [
        // The member does not exist on the owner.
        (
            "fn run() -> TypeId:\n    TypeId::missing::[User]()\n",
            Code::UnknownMethod,
        ),
        // fn.generic.explicit.too-long-count: `of` has one parameter.
        (
            "fn run() -> TypeId:\n    TypeId::of::[User, User]()\n",
            Code::ArgumentCount,
        ),
        // A member with no generic parameters takes none.
        (
            "fn run() -> Box[i32]:\n    Box::[i32]::zero::[i32]()\n",
            Code::ArgumentCount,
        ),
        // `of`'s bound: an unbounded parameter is not `Inspectable`.
        (
            "fn run[T]() -> TypeId:\n    TypeId::of::[T]()\n",
            Code::UnsatisfiedTraitBound,
        ),
        // `of`'s bound: a function type is not `Inspectable`.
        (
            "fn run() -> TypeId:\n    TypeId::of::[fn(i32) -> i32]()\n",
            Code::UnsatisfiedTraitBound,
        ),
    ];
    for (c, code) in cases {
        let src = format!("{DECLS}\n{c}");
        assert_eq!(errors(&src), vec![code], "{c}");
    }
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = run_goal(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("typeid-of-{name}.wasm"));
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

/// `TypeId::of` equals itself, differs between types, and equals the
/// `runtime_type` of a value of the type (trait.typeid.of,
/// trait.typeid.equality, trait.inspect.static).
#[test]
fn type_id_of_follows_runtime_identity() {
    let main = "\
use std.inspect.{Inspectable, TypeId}

data User:
    name: string

data Box[T]:
    value: T

fn same[T < Inspectable, U < Inspectable]() -> bool:
    TypeId::of::[T]() == TypeId::of::[U]()

pub fn main() -> void $ Console:
    println(\"${TypeId::of::[i32]() == TypeId::of::[i32]()}\")
    println(\"${TypeId::of::[i32]() == TypeId::of::[i64]()}\")
    println(\"${TypeId::of::[Box[i32]]() == TypeId::of::[Box[string]]()}\")
    println(\"${same::[User, User]()} ${same::[User, Box[User]]()}\")
    let seven: i32 = 7
    println(\"${seven.runtime_type() == TypeId::of::[i32]()}\")
    user := User { name: \"Ada\" }
    println(\"${user.runtime_type() == TypeId::of::[User]()}\")
    println(\"${user.runtime_type() == TypeId::of::[string]()}\")
    println(TypeId::of::[List[User]]().to_string())
";
    assert_eq!(
        output_of("identity", main),
        "true\nfalse\nfalse\ntrue false\ntrue\ntrue\nfalse\nList[app.main.User]\n"
    );
}
