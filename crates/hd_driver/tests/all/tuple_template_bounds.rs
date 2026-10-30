//! Tuple templates hold only for tuples whose members meet the walker's
//! bound (spec 14 `annot.template.tuple.implements`, `.unmet`,
//! `annot.walker.obligation`, `.rest`): a tuple of hashable members is a
//! map key, a tuple with a float, a function or an unhashable data member
//! is not, and the use reports one `unsatisfied-trait-bound`. A program
//! that uses such a tuple stops at checking, never in collection. Valid
//! programs build through the driver and run on V8 (`host/run.ts`), as
//! the conformance runner does.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(files: &[(&str, &str)], goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn program() -> Goal {
    Goal::Program {
        entry: "main".into(),
    }
}

const BOUND: &[Code] = &[Code::UnsatisfiedTraitBound];

#[test]
fn running_a_program_with_an_unmet_tuple_stops_at_checking() {
    let src = "\
pub fn main() -> void $ Console:
    let points: Map[(i32, f64), string] = {}
    println(points.len())
";
    let out = built(&[("main.hd", src)], &program());
    let codes: Vec<Code> = out
        .diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect();
    assert_eq!(codes, BOUND, "{}", out.render());
    assert!(out.wasm.is_none());
    assert!(
        !out.report.stages.contains_key(&hd_base::Stage::Collect),
        "collection never ran: {:?}",
        out.report.reasons
    );
}
