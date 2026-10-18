//! A range bound must have an integer or a floating-point type
//! (spec/lang/05-expressions.md, expr.range.bound.numeric): `type-mismatch`
//! once, at the bound. A float range is not iterable
//! (spec/lang/06-control-flow.md, flow.for.range.float).

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
fn non_numeric_bounds_are_one_type_mismatch() {
    for stmt in [
        "a := s..",
        "a := ..s",
        "a := s..=s",
        "a := s..s",
        "a := \"a\"..\"b\"",
        "a := b..b",
    ] {
        let src = format!("fn f(s: string, b: bool) -> void:\n    {stmt}\n    _ := a\n");
        assert_eq!(check(&src), [Code::TypeMismatch], "{stmt}");
    }
}

#[test]
fn float_bounds_are_fine() {
    for stmt in [
        "_ := 0.5..x",
        "_ := x..",
        "_ := ..x",
        "_ := ..=x",
        "_ := x..=y",
        "_ := 1.5..",
        "_ := 0.0..=1.0",
        "let _: Range[f32] = n..=0.5",
        "let _: Range[f32] = 0.0..1.0",
    ] {
        let src = format!("use std.ops.Range\nfn f(x: f64, y: f64, n: f32) -> void:\n    {stmt}\n");
        assert_eq!(check(&src), [], "{stmt}");
    }
}

#[test]
fn an_integer_and_a_float_bound_do_not_mix() {
    for stmt in ["_ := 0..x", "_ := n..1.5", "_ := 1..2.5", "_ := x..n"] {
        let src = format!("fn f(x: f64, n: i32) -> void:\n    {stmt}\n");
        assert_eq!(check(&src), [Code::TypeMismatch], "{stmt}");
    }
}

#[test]
fn a_float_range_is_not_iterable() {
    for (sig, range) in [
        ("", "0.0..1.0"),
        ("", "1.5.."),
        ("lo: f64, hi: f64", "lo..=hi"),
        ("lo: f32", "lo..lo"),
    ] {
        let src = format!("fn f({sig}) -> void:\n    for x in {range}:\n        _ := x\n");
        assert_eq!(check(&src), [Code::UnsatisfiedTraitBound], "{range}");
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
fn generic_bounds_need_an_integer_or_float_bound() {
    assert_eq!(
        check("fn f[T](a: T, b: T) -> void:\n    _ := a..b\n"),
        [Code::TypeMismatch]
    );
    assert_eq!(
        check("use std.num.Integer\nfn f[T < Integer](a: T, b: T) -> void:\n    _ := a..b\n"),
        []
    );
    assert_eq!(
        check("use std.num.Float\nfn f[F < Float](a: F, b: F) -> void:\n    _ := a..=b\n"),
        []
    );
    assert_eq!(
        check(
            "use std.num.Float\nfn f[F < Float](a: F, b: F) -> void:\n    for x in a..b:\n        _ := x\n"
        ),
        [Code::UnsatisfiedTraitBound]
    );
}
