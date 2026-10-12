//! A signed and an unsigned integer do not mix (spec/lang/04-type-system.md,
//! types.num.binary.no-mix; spec/lang/05-expressions.md,
//! expr.range.bound.signedness): `mixed-signedness` once, at the operand.
//! Two types of one signedness keep `type-mismatch`.

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
fn a_signed_and_an_unsigned_operand_are_mixed_signedness() {
    for (a, b, op) in [
        ("i32", "u32", "+"),
        ("u64", "i64", "-"),
        ("u32", "i32", "<"),
        ("i64", "u32", "=="),
    ] {
        let src = format!("fn f(a: {a}, b: {b}) -> void:\n    _ := a {op} b\n");
        assert_eq!(check(&src), [Code::MixedSignedness], "{a} {op} {b}");
    }
}

#[test]
fn range_bounds_of_both_signedness_are_mixed_signedness() {
    assert_eq!(
        check("fn f(count: u32, limit: i32) -> void:\n    _ := count..limit\n"),
        [Code::MixedSignedness]
    );
    assert_eq!(
        check("fn f(a: i32, b: u32) -> void:\n    _ := a..=b\n"),
        [Code::MixedSignedness]
    );
}

#[test]
fn a_std_method_result_is_mixed_with_a_signed_operand() {
    assert_eq!(
        check("fn f(a: i32, b: i32, limit: i32) -> bool:\n    a.abs_diff(b) < limit\n"),
        [Code::MixedSignedness]
    );
    assert_eq!(
        check("fn f(flags: u8, limit: i32) -> bool:\n    flags.count_ones() > limit\n"),
        [Code::MixedSignedness]
    );
}

#[test]
fn a_width_mismatch_of_one_signedness_stays_a_type_mismatch() {
    assert_eq!(
        check("fn f(a: i16, b: i64) -> void:\n    _ := a + b\n"),
        [Code::TypeMismatch]
    );
    assert_eq!(
        check("fn f(a: u8, b: u64) -> void:\n    _ := a..b\n"),
        [Code::TypeMismatch]
    );
}

#[test]
fn operands_of_one_type_are_fine() {
    assert_eq!(
        check("fn f(a: u32, b: u32) -> bool:\n    _ := a + b\n    a < b\n"),
        []
    );
}

#[test]
fn a_mixed_result_adds_no_second_report() {
    assert_eq!(
        check("fn f(left: i32, right: u32) -> i64:\n    left + right\n"),
        [Code::MixedSignedness]
    );
}
