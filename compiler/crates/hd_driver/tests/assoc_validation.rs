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
