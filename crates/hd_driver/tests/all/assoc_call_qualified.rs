//! Trait-qualified associated calls (spec 09 "Associated Function
//! Calls"): `T::f` resolves through exactly one bound
//! (`trait.assoc-call.parameter`, `trait.assoc-call.parameter.one`), and
//! `Trait::f` whose `Self` inference does not determine reports
//! `cannot-infer-type` (`trait.assoc-call.trait.undetermined`). Each rule
//! has its legal twin.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_main(main: &str) -> Output {
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

const TWO_BOUNDS: &str = "\
trait Factory:
    fn create() -> Self

trait Seeded:
    fn create() -> Self
";

#[test]
fn two_bounds_declaring_is_ambiguous() {
    let src = format!("{TWO_BOUNDS}\nfn fresh[T < Factory & Seeded]() -> T:\n    T::create()\n");
    assert_eq!(errors(&src), vec![Code::AmbiguousMethod]);
}

#[test]
fn one_bound_declaring_resolves() {
    let src = "trait Factory:\n    fn create() -> Self\n\nfn fresh[T < Factory]() -> T:\n    T::create()\n";
    assert_eq!(errors(src), vec![]);
}

const MAKE: &str = "\
trait Make:
    fn make() -> i32

data Widget: pass

impl Make for Widget:
    fn make() -> i32: 1
";

#[test]
fn undetermined_self_is_cannot_infer() {
    let src = format!("{MAKE}\nfn run() -> i32:\n    Make::make()\n");
    assert_eq!(errors(&src), vec![Code::CannotInferType]);
}

#[test]
fn self_determined_by_arguments_resolves() {
    let src = "trait Dup:\n    fn dup(x: Self) -> Self\n\ndata Widget: pass\n\nimpl Dup for Widget:\n    fn dup(x: Self) -> Self: x\n\nfn run(w: Widget) -> Widget:\n    Dup::dup(w)\n";
    assert_eq!(errors(src), vec![]);
}

#[test]
fn self_determined_by_expected_type_resolves() {
    let src = "trait Make:\n    fn make() -> Self\n\ndata Widget:\n    n: i32\n\nimpl Make for Widget:\n    fn make() -> Self: Widget { n: 1 }\n\nfn run() -> Widget:\n    Make::make()\n";
    assert_eq!(errors(src), vec![]);
}
