//! The instantiation choice through trials (`crates/hd_check/DESIGN.md` (type checking) §2.5,
//! `crates/hd_check/DESIGN.md` (trait solver) §6.5): a method call whose receiver implements one
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

const ITEM: &str = "trait Apply[T]:\n    fn apply(self, run: fn(T) -> T) -> i32\n\nstruct Item:\n    value: i32\n\n";
const BOOL_IMPL: &str = "impl Apply[bool] for Item:\n    fn apply(self, run: fn(bool) -> bool) -> i32:\n        if run(true): 1 else: 0\n\n";
const I32_IMPL: &str = "impl Apply[i32] for Item:\n    fn apply(self, run: fn(i32) -> i32) -> i32:\n        run(self.value)\n\n";
// The closure captures `offset`, holds a string constant and a local, and
// fails to check against `fn(bool) -> bool`.
const MAIN: &str = "pub fn main() -> i32:\n    item := Item { value: 40 }\n    offset := +2\n    item.apply(fn(value):\n        label := \"trial\"\n        if label == \"trial\": value + offset else: value\n    )\n";

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
