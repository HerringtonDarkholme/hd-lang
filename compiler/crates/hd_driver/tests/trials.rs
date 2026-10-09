//! The instantiation choice through trials (type-checking.md §2.5,
//! trait-solver.md §6.5): a method call whose receiver implements one
//! generic trait at several instantiations tries each candidate, keeps
//! the ones that fit, and commits, reports `ambiguous-method`, or reports
//! `type-mismatch` listing the instantiations. A failed trial leaves no
//! trace in the body.

use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str, render: &[&str]) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: render,
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn errors(out: &Output) -> Vec<(Code, String)> {
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == Severity::Error)
        .map(|i| {
            (
                out.diags.code[i],
                out.diags.get_text(out.diags.message[i]).to_owned(),
            )
        })
        .collect()
}

const ITEM: &str =
    "trait Apply[T]:\n    fn apply(self, run: fn(T) -> T) -> i32\n\ndata Item:\n    value: i32\n\n";
const BOOL_IMPL: &str = "impl Apply[bool] for Item:\n    fn apply(self, run: fn(bool) -> bool) -> i32:\n        if run(true): 1 else: 0\n\n";
const I32_IMPL: &str = "impl Apply[i32] for Item:\n    fn apply(self, run: fn(i32) -> i32) -> i32:\n        run(self.value)\n\n";
// The closure captures `offset`, holds a string constant and a local, and
// fails to check against `fn(bool) -> bool`.
const MAIN: &str = "pub fn main() -> i32:\n    item := Item { value: 40 }\n    offset := +2\n    item.apply(fn(value):\n        label := \"trial\"\n        if label == \"trial\": value + offset else: value\n    )\n";

/// Two candidates, and only the second fits: its trial is the one with
/// no error, and the call is checked for real against it.
#[test]
fn of_two_candidates_the_one_whose_trial_fits_is_chosen() {
    let src = "trait Pick[T]:\n    fn pick(self, value: T) -> i32\n\ndata Item: pass\n\nimpl Pick[bool] for Item:\n    fn pick(self, value: bool) -> i32: 0\n\nimpl Pick[i32] for Item:\n    fn pick(self, value: i32) -> i32: value\n\npub fn main(n: i32) -> i32:\n    Item {}.pick(n)\n";
    let out = analyze(src, &["app/main/main"]);
    assert_eq!(errors(&out), vec![], "{}", out.render());
    let tir = out.tir_text.get("app/main/main").expect("main's TIR");
    assert!(tir.contains("Pick"), "{tir}");
    assert!(
        !tir.contains("bool"),
        "the bool candidate left nothing: {tir}"
    );
}

/// Two candidates that both fit, with no literal to default: one
/// `ambiguous-method` naming both (`trait.resolve.many-apply`).
#[test]
fn two_fitting_candidates_are_ambiguous() {
    let src = "trait Pick[T]:\n    fn pick(self) -> T\n\ndata Money:\n    cents: i32\n\nimpl Pick[i32] for Money:\n    fn pick(self) -> i32: self.cents\n\nimpl Pick[string] for Money:\n    fn pick(self) -> string: \"money\"\n\nfn invalid(price: Money) -> void:\n    _ := price.pick()\n";
    let out = analyze(src, &[]);
    let errs = errors(&out);
    assert_eq!(errs.len(), 1, "{}", out.render());
    assert_eq!(errs[0].0, Code::AmbiguousMethod);
    assert!(errs[0].1.contains("Pick[i32]") && errs[0].1.contains("Pick[string]"));
}

/// No candidate fits: one `type-mismatch` listing the instantiations
/// (`trait.resolve.no-apply`), not one error per trial.
#[test]
fn no_fitting_candidate_is_one_type_mismatch_listing_them() {
    let src = "trait Combine[T]:\n    fn combine(self, other: T) -> i32\n\ndata Money:\n    cents: i32\n\nimpl Combine[i32] for Money:\n    fn combine(self, other: i32) -> i32: self.cents + other\n\nimpl Combine[Money] for Money:\n    fn combine(self, other: Money) -> i32: self.cents + other.cents\n\nfn invalid(price: Money) -> i32:\n    price.combine(\"five\")\n";
    let out = analyze(src, &[]);
    let errs = errors(&out);
    assert_eq!(errs.len(), 1, "{}", out.render());
    assert_eq!(errs[0].0, Code::TypeMismatch);
    assert!(
        errs[0].1.contains("Combine[i32]") && errs[0].1.contains("Combine[Money]"),
        "{}",
        errs[0].1
    );
}

/// A failed trial leaves no trace: with the failing `Apply[bool]`
/// candidate present, `main`'s TIR (instructions, locals, constants,
/// strings, captures) and diagnostics are those of the program without
/// it, where the one candidate is called directly.
#[test]
fn a_failed_trial_leaves_no_trace() {
    let both = format!("{ITEM}{BOOL_IMPL}{I32_IMPL}{MAIN}");
    let one = format!("{ITEM}{I32_IMPL}{MAIN}");
    let a = analyze(&both, &["app/main/main"]);
    let b = analyze(&one, &["app/main/main"]);
    assert_eq!(errors(&a), errors(&b), "{}", a.render());
    let (ta, tb) = (
        a.tir_text.get("app/main/main").expect("TIR"),
        b.tir_text.get("app/main/main").expect("TIR"),
    );
    assert!(ta.contains("captures [$1 Copy]"), "{ta}");
    assert_eq!(ta, tb, "the trial of the bool candidate left a trace");
}
