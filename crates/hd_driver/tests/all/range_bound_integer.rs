//! A range bound must have an integer type (spec/lang/05-expressions.md,
//! expr.range.bound.integer): `type-mismatch` once, at the bound.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(items: &str) -> Vec<Code> {
    let main = format!("{items}\npub fn main() -> void: pass\n");
    let mut src = MemorySources::default();
    src.insert("main.hd", &main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out: Output = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags.code.clone()
}

#[test]
fn non_integer_bounds_are_one_type_mismatch() {
    for stmt in [
        "a := 0.5..x",
        "a := x..",
        "a := ..x",
        "a := ..=x",
        "a := x..=x",
        "a := x..y",
        "a := 1.5..",
        "a := s..",
        "a := ..s",
        "a := s..=s",
        "a := \"a\"..\"b\"",
    ] {
        let src = format!("fn f(x: f64, y: f64, s: string) -> void:\n    {stmt}\n    _ := a\n");
        assert_eq!(check(&src), [Code::TypeMismatch], "{stmt}");
    }
}

#[test]
fn integer_bounds_of_each_width_are_fine() {
    for ty in [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize",
    ] {
        let src = format!(
            "fn f(a: {ty}, b: {ty}) -> void:\n    _ := a..b\n    _ := a..\n    _ := ..b\n    _ := a..=b\n    _ := 0..b\n    _ := a..=1\n"
        );
        assert_eq!(check(&src), [], "{ty}");
    }
    assert_eq!(
        check("fn f(n: u32) -> void:\n    _ := 0..n\n    _ := +1..\n    _ := ..\n"),
        []
    );
}

#[test]
fn generic_bounds_need_an_integer_bound() {
    assert_eq!(
        check("fn f[T](a: T, b: T) -> void:\n    _ := a..b\n"),
        [Code::TypeMismatch]
    );
    assert_eq!(
        check("use std.num.Integer\nfn f[T < Integer](a: T, b: T) -> void:\n    _ := a..b\n"),
        []
    );
}
