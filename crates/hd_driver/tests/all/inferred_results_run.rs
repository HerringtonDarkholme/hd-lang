//! Omitted result types (#80; spec 07 `fn.decl.result-omitted-private`,
//! `fn.decl.result-inferred`, `fn.decl.result-inferred.common`,
//! `fn.decl.result-inferred.void`, `fn.decl.omitted-cycle`;
//! `crates/hd_tir/DESIGN.md` §4.13.1 "Omitted result types (M1)"). A private
//! function or inherent method without `-> T` takes the least common type
//! of its final value and its `return` operands, and every caller sees
//! that result. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// A cycle of omitted results is one error, at the member first in source
/// order even when M1 entered another member first
/// (`fn.decl.omitted-cycle.report`): `entry` reaches `late` before `odd`.
#[test]
fn a_cycle_of_omitted_results_is_one_error_at_its_first_member() {
    let main = "\
fn entry(): late(4)

fn odd(n: i32):
    if n == 0: false
    else: late(n - 1)

fn late(n: i32):
    if n == 0: true
    else: odd(n - 1)

pub fn main() -> void $ Console:
    println(1)
";
    let out = built(main);
    assert_eq!(
        out.diags.code,
        vec![Code::RecursiveFunctionNeedsResultType],
        "{}",
        out.render()
    );
    let at = main.find("odd(n: i32)").expect("odd");
    assert_eq!(out.diags.primary[0].lo as usize, at, "{}", out.render());
}
