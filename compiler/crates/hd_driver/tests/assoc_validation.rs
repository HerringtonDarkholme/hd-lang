//! Associated-type validation (spec 09 "Associated Type Bindings",
//! "Supertrait Bindings", "Bound Associated Types", "Implementation
//! Declarations"; spec 11 "Bound Requirement Keys"): one test per rule,
//! each with the reported code and, where the rule has a legal twin, the
//! twin that reports nothing.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_main(main: &str) -> Output {
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
    build(&host, "app", &Goal::Analyze)
}

fn errors(src: &str) -> Vec<Code> {
    let out = build_main(src);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const SUPPLIER: &str = "trait Supplier:\n    type Item\n    fn get(self) -> Self::Item\n\n";

// trait.dyn.binding.complete

#[test]
fn a_dyn_type_must_bind_every_associated_type() {
    let src = format!("{SUPPLIER}fn unbound(source: dyn Supplier) -> void:\n    pass\n");
    assert_eq!(errors(&src), vec![Code::TraitNotDynamicallySafe]);
    let bound =
        format!("{SUPPLIER}fn bound(source: dyn Supplier[Item = i32]) -> void:\n    pass\n");
    assert_eq!(errors(&bound), vec![]);
}

#[test]
fn a_dyn_type_in_a_body_must_bind_every_associated_type() {
    let src = format!(
        "{SUPPLIER}fn run() -> void:\n    let f = fn(source: dyn Supplier) -> void: pass\n    f\n"
    );
    assert!(
        errors(&src).contains(&Code::TraitNotDynamicallySafe),
        "{:?}",
        errors(&src)
    );
}

#[test]
fn a_dyn_type_counts_the_bindings_its_supertrait_list_fixes() {
    let src = format!(
        "{SUPPLIER}trait Prices < Supplier[Item = i32]\n\nfn ok(source: dyn Prices) -> void:\n    pass\n"
    );
    assert_eq!(errors(&src), vec![]);
}

#[test]
fn a_dyn_type_binds_the_associated_types_of_its_supertraits_too() {
    let src = format!(
        "{SUPPLIER}trait Named < Supplier:\n    fn name(self) -> string\n\nfn bad(source: dyn Named) -> void:\n    pass\n\nfn good(source: dyn Named[Item = i32]) -> void:\n    pass\n"
    );
    assert_eq!(errors(&src), vec![Code::TraitNotDynamicallySafe]);
}

// req.key.binding.complete

#[test]
fn a_requirement_key_must_bind_every_associated_type() {
    let store = "trait Store:\n    type Item\n    fn load(self, id: string) -> Self::Item\n\n";
    let src = format!("{store}fn count(id: string) -> i32 $ Store:\n    0\n");
    assert_eq!(errors(&src), vec![Code::TraitNotDynamicallySafe]);
    let bound = format!("{store}fn count(id: string) -> i32 $ Store[Item = i32]:\n    0\n");
    assert_eq!(errors(&bound), vec![]);
}

// trait.dyn.bound.available

const FACTORY: &str = "trait Factory:\n    type Item\n    fn count() -> i32\n\nfn total[F < Factory[Item = i32]]() -> i32:\n    F::count()\n\n";

#[test]
fn a_dyn_type_does_not_satisfy_a_bound_on_a_trait_with_an_associated_function() {
    let src = format!(
        "{FACTORY}fn with_function(factory: dyn Factory[Item = i32]) -> i32:\n    total::[dyn Factory[Item = i32]]()\n"
    );
    assert_eq!(errors(&src), vec![Code::UnsatisfiedTraitBound]);
}

#[test]
fn a_dyn_type_does_not_satisfy_a_bound_on_a_supertrait_with_an_associated_function() {
    let src = format!(
        "{FACTORY}trait Named < Factory:\n    fn name(self) -> string\n\nfn with_named(source: dyn Named[Item = i32]) -> i32:\n    total::[dyn Named[Item = i32]]()\n"
    );
    assert_eq!(errors(&src), vec![Code::UnsatisfiedTraitBound]);
}

#[test]
fn a_dyn_type_satisfies_a_bound_on_a_trait_of_methods() {
    let src = "trait Sized:\n    type Item\n    fn size(self) -> i32\n\nfn measure[S < Sized[Item = i32]](value: S) -> i32:\n    value.size()\n\nfn with_value(value: dyn Sized[Item = i32]) -> i32:\n    measure(value)\n";
    assert_eq!(errors(src), vec![]);
}

// trait.binding.super.mismatch

const SUMMABLE: &str =
    "use std.ops.Add\n\ntrait Summable < Add[Out = Self]\n\ndata Money:\n    cents: i64\n\n";

#[test]
fn an_implementation_must_bind_its_supertrait_bindings() {
    let bad = format!(
        "{SUMMABLE}impl Add for Money:\n    type Out = i64\n    fn add(self, rhs: Money) -> i64:\n        self.cents + rhs.cents\n\nimpl Summable for Money\n"
    );
    assert_eq!(errors(&bad), vec![Code::MissingSupertraitImplementation]);
    let good = format!(
        "{SUMMABLE}impl Add for Money:\n    type Out = Money\n    fn add(self, rhs: Money) -> Money:\n        self\n\nimpl Summable for Money\n"
    );
    assert_eq!(errors(&good), vec![]);
}

// trait.binding.super.conflict

const SOURCE: &str = "trait Source:\n    type Item\n    fn get(self) -> Self::Item\n\ntrait Numbers < Source[Item = i32]\n\n";

#[test]
fn two_supertrait_paths_must_not_bind_one_type_to_different_types() {
    let bad =
        format!("{SOURCE}trait Words < Source[Item = string]\n\ntrait Both < Numbers & Words\n");
    assert_eq!(errors(&bad), vec![Code::DuplicateAssociatedBinding]);
}

#[test]
fn a_conflict_through_a_longer_path_is_reported_once_at_the_declaring_trait() {
    let bad = format!(
        "{SOURCE}trait Wide < Numbers\n\ntrait Words < Source[Item = string]\n\ntrait Both < Wide & Words\n"
    );
    assert_eq!(errors(&bad), vec![Code::DuplicateAssociatedBinding]);
}

#[test]
fn two_supertrait_paths_that_bind_the_same_type_merge() {
    let good =
        format!("{SOURCE}trait Counts < Source[Item = i32]\n\ntrait Both < Numbers & Counts\n");
    assert_eq!(errors(&good), vec![]);
}
