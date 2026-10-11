//! A qualified variant of the optional (`Option.Some(v)`, `Option.None`) is
//! a value like `Result.Ok(v)`: `Option` names an alias of the built-in
//! optional, and a qualified variant looks through an alias to the enum it
//! expands to (#178).

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

#[test]
fn a_qualified_some_is_a_value() {
    let src = "fn present(value: i32) -> i32?:
    Option.Some(value)

fn absent() -> Option[i32]:
    Option.None

fn bound() -> i32:
    let x: Option[i32] = Option.Some(3)
    match x:
        .Some(n) => n
        .None => 0

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), Vec::<Code>::new());
}

#[test]
fn a_qualified_some_as_a_subject_takes_the_arm_literal_width() {
    let src = "fn take(value: u8) -> u8:
    value

fn run() -> u8:
    match Option.Some(3):
        .Some(n) => take(n)
        .None => 0

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), Vec::<Code>::new());
}

#[test]
fn an_alias_of_a_non_enum_has_no_variants() {
    let src = "pub fn main() -> void:
    x := i32.Some(3)
";
    assert_ne!(codes(src), Vec::<Code>::new());
}
