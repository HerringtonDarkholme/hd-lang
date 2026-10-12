//! A generic type's declared bounds are never implied inside a bound
//! (`trait.bound.no-implied.positions`, `trait.bound.no-implied.enclosing`):
//! a parameter written in `Map[K, V]` inside a function bound, a data bound,
//! a supertrait list or an impl header gets its bounds from the declaration,
//! one `unsatisfied-trait-bound` per bad argument.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn errors(main: &str) -> Vec<Code> {
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::UnusedLocalBinding)
        .collect()
}

const BOUND: &[Code] = &[Code::UnsatisfiedTraitBound];
const HOLDS: &str = "trait Holds[T]:\n    fn get(self) -> T\n\n";

#[test]
fn an_unbounded_key_in_a_function_bound_fails() {
    let src = format!("{HOLDS}fn use_it[K, V, H < Holds[Map[K, V]]](h: H) -> i32:\n    0\n");
    assert_eq!(errors(&src), BOUND);
}

#[test]
fn a_bounded_key_in_a_function_bound_is_accepted() {
    let src =
        format!("{HOLDS}fn use_it[K < Eq & Hash, V, H < Holds[Map[K, V]]](h: H) -> i32:\n    0\n");
    assert_eq!(errors(&src), Vec::<Code>::new());
}

#[test]
fn an_unbounded_key_in_a_data_bound_fails() {
    let src = format!("{HOLDS}data Box[K, V, H < Holds[Map[K, V]]]:\n    pub h: H\n");
    assert_eq!(errors(&src), BOUND);
}

#[test]
fn a_bounded_key_in_a_data_bound_is_accepted() {
    let src = format!("{HOLDS}data Box[K < Eq & Hash, V, H < Holds[Map[K, V]]]:\n    pub h: H\n");
    assert_eq!(errors(&src), Vec::<Code>::new());
}

#[test]
fn an_unbounded_key_in_a_supertrait_argument_fails() {
    let src = format!("{HOLDS}trait Keyed[K, V] < Holds[Map[K, V]]:\n    fn size(self) -> i32\n");
    assert_eq!(errors(&src), BOUND);
}

#[test]
fn a_bounded_key_in_a_supertrait_argument_is_accepted() {
    let src = format!(
        "{HOLDS}trait Keyed[K < Eq & Hash, V] < Holds[Map[K, V]]:\n    fn size(self) -> i32\n"
    );
    assert_eq!(errors(&src), Vec::<Code>::new());
}

#[test]
fn an_unbounded_key_in_an_impl_bound_fails() {
    let src = format!(
        "{HOLDS}data Wrap[K, V, H]:\n    pub h: H\n\nimpl[K, V, H < Holds[Map[K, V]]] Wrap[K, V, H]:\n    fn id(self) -> i32:\n        0\n"
    );
    assert_eq!(errors(&src), BOUND);
}

#[test]
fn a_bounded_key_in_an_impl_bound_is_accepted() {
    let src = format!(
        "{HOLDS}data Wrap[K, V, H]:\n    pub h: H\n\nimpl[K < Eq & Hash, V, H < Holds[Map[K, V]]] Wrap[K, V, H]:\n    fn id(self) -> i32:\n        0\n"
    );
    assert_eq!(errors(&src), Vec::<Code>::new());
}

#[test]
fn an_unbounded_key_in_an_impl_trait_argument_fails() {
    let src = format!(
        "{HOLDS}data Wrap[K, V]:\n    pub k: K\n    pub v: V\n\nimpl[K, V] Holds[Map[K, V]] for Wrap[K, V]:\n    fn get(self) -> Map[K, V]:\n        {{}}\n"
    );
    assert!(errors(&src).contains(&Code::UnsatisfiedTraitBound));
}
