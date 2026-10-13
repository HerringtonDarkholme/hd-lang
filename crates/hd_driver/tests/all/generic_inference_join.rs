//! First inferred argument sets the join (spec 04, group Q):
//! conflicting later arguments for one bare type parameter report one
//! `type-mismatch`, while single inferred arguments and literals keep the
//! behavior the other conformance fixtures require.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &MemoryStore::default(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

const PICK: &str = "fn pick[T](left: T, right: T) -> T:\n    left\n";

#[test]
fn variance_conflict_is_single_mismatch() {
    let out = analyze(&format!(
        "{PICK}\nfn first(owned: List[mut User], shared: List[User]) -> List[User]:\n    pick(owned, shared)\n\ndata User:\n    name: string\n"
    ));
    assert_eq!(codes(&out), vec![Code::TypeMismatch], "{}", out.render());
}

#[test]
fn readonly_first_variance_points_at_use() {
    let out = analyze(&format!(
        "{PICK}\nfn first(shared: List[User], owned: List[mut User]) -> List[User]:\n    pick(shared, owned)\n\ndata User:\n    name: string\n"
    ));
    assert_eq!(codes(&out), vec![Code::TypeMismatch], "{}", out.render());
    assert!(
        out.render()
            .contains("expected List[User], found List[mut User]"),
        "{}",
        out.render()
    );
}

#[test]
fn optional_conflict_is_single_mismatch() {
    let out = analyze(&format!(
        "{PICK}\nfn first(value: i32, maybe: i32?) -> i32?:\n    pick(value, maybe)\n"
    ));
    assert_eq!(codes(&out), vec![Code::TypeMismatch], "{}", out.render());
}

#[test]
fn numeric_conflict_is_single_mismatch() {
    let out = analyze(&format!(
        "{PICK}\nfn widest(small: i32, large: i64) -> i64:\n    pick(small, large)\n"
    ));
    assert_eq!(codes(&out), vec![Code::TypeMismatch], "{}", out.render());
}

#[test]
fn single_inferred_argument_stays_clean() {
    let out = analyze(
        "fn keep[T](value: T) -> void:\n    pass\n\nfn run[A](item: List[A]) -> void:\n    keep(item)\n",
    );
    assert!(codes(&out).is_empty(), "{}", out.render());
}

#[test]
fn optional_against_string_literal_stays_clean() {
    let out = analyze(
        "fn eq2[T](a: T, b: T) -> void:\n    pass\n\nfn run(s: string?) -> void:\n    eq2(s, \"x\")\n",
    );
    assert!(codes(&out).is_empty(), "{}", out.render());
}

#[test]
fn bool_literal_against_integer_still_errors() {
    let out = analyze(&format!("{PICK}\nfn first() -> i32:\n    pick(1, true)\n"));
    assert_eq!(codes(&out), vec![Code::TypeMismatch], "{}", out.render());
}
