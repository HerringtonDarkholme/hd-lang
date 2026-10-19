//! A use of a derived implementation whose type argument lacks a bound
//! the derivation added is `missing-derived-bound` at the use (spec 09
//! `trait.derive.bound-unmet`); other failures keep their own errors.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn errors(main: &str) -> Vec<Code> {
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

fn with_derive(derive: &str, body: &str) -> String {
    format!(
        "@derive({derive})
struct Box[T]:
    value: T

struct Socket:
    port: i32

{body}
"
    )
}

#[test]
fn eq_on_an_argument_without_eq_is_reported() {
    let src = with_derive(
        "Eq",
        "fn same(a: Box[Socket], b: Box[Socket]) -> bool:
    a == b",
    );
    assert_eq!(errors(&src), vec![Code::MissingDerivedBound]);
}

#[test]
fn eq_on_an_argument_with_eq_is_clean() {
    let src = with_derive(
        "Eq",
        "fn same(a: Box[i32], b: Box[i32]) -> bool:
    a == b",
    );
    assert_eq!(errors(&src), Vec::<Code>::new());
}

#[test]
fn a_type_without_a_derive_keeps_its_own_error() {
    let src = with_derive(
        "Eq",
        "fn same(a: Socket, b: Socket) -> bool:
    a == b",
    );
    assert_eq!(errors(&src), vec![Code::TypeMismatch]);
}

#[test]
fn ord_on_an_argument_without_ord_is_reported() {
    let src = with_derive(
        "Eq, PartialOrd",
        "fn less(a: Box[Socket], b: Box[Socket]) -> bool:
    a < b",
    );
    assert_eq!(errors(&src), vec![Code::MissingDerivedBound]);
}
