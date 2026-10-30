//! Const generic arguments at call sites (spec/lang/04-type-system.md,
//! "Const Generic Arguments"): a const parameter is solved from argument
//! types or written (`types.generic.const.infer-argument-type`,
//! `types.generic.const.infer-unsolved`), an explicit argument is
//! evaluated, values select implementations exactly
//! (`types.generic.const.impl-exact`), and a build stops before
//! instances until they carry const arguments.

use hd_base::Stage;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

use crate::type_default_written_run::codes_of;

const DECLS: &str = "struct Fixed[T, N < const usize]:
    value: T

fn keep[T, N < const usize](value: Fixed[T, N]) -> Fixed[T, N]:
    value

fn pick[N < const usize]() -> Fixed[i32, N]:
    Fixed { value: 1 }

trait Mark[N < const usize]:
    fn marked(self) -> i32

impl[T, N < const usize] Mark[N] for Fixed[T, N]:
    fn marked(self) -> i32:
        1

fn needs_four[M < Mark[4]](m: M) -> i32:
    m.marked()
";

fn build_main(body: &str, goal: &Goal) -> Output {
    let main = format!("{DECLS}\nfn uses() -> void:\n{body}\npub fn main() -> void: pass\n");
    let mut src = MemorySources::default();
    src.insert("main.hd", &main);
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

fn errors(body: &str) -> Vec<Code> {
    let out = build_main(body, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn argument_types_and_explicit_arguments_solve_const_parameters() {
    let body = "    let a: Fixed[i32, 4] = Fixed { value: 1 }
    let b: Fixed[i32, 2 + 2] = keep(a)
    let c: Fixed[i32, 4] = pick::[4]()
    _ := needs_four(b)
    _ := needs_four(c)
";
    assert_eq!(errors(body), []);
}

#[test]
fn the_expected_result_does_not_solve_a_const_parameter() {
    assert_eq!(
        errors("    let a: Fixed[i32, 4] = pick()\n"),
        [Code::CannotInferType]
    );
}

#[test]
fn explicit_and_inferred_values_keep_their_identity() {
    assert_eq!(
        errors("    let a: Fixed[i32, 5] = pick::[4]()\n"),
        [Code::TypeMismatch]
    );
    assert_eq!(
        errors(
            "    let a: Fixed[i32, 4] = Fixed { value: 1 }\n    let b: Fixed[i32, 5] = keep(a)\n"
        ),
        [Code::TypeMismatch]
    );
    assert_eq!(
        errors("    let a: Fixed[i32, 5] = Fixed { value: 1 }\n    _ := needs_four(a)\n"),
        [Code::UnsatisfiedTraitBound]
    );
}

#[test]
fn a_const_slot_takes_no_type_and_names_only_values_in_scope() {
    assert_eq!(
        errors("    let a: Fixed[i32, 4] = pick::[i32]()\n"),
        [Code::GenericKindMismatch]
    );
    assert_eq!(
        errors("    let a: Fixed[i32, MISSING] = pick::[4]()\n"),
        [Code::UnknownName]
    );
}

#[test]
fn a_build_stops_before_instances() {
    let out = build_main(
        "    let a: Fixed[i32, 4] = pick::[4]()\n",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    let collect = out.report.tally(Stage::Collect);
    assert_eq!(collect.not_implemented, 1);
    assert_eq!(
        collect.first_reason.as_deref(),
        Some("const generic parameters")
    );
}

#[test]
fn a_failed_use_in_a_type_argument_does_not_reach_tir_verification() {
    let head = "use std.text.{Nope}\n\nstruct Fixed[T, N < const usize]:\n    value: T\n\nstruct Plain[T]:\n    value: T\n\n";
    for (ty, value) in [
        ("Fixed[Nope, 4]", "Fixed { value: 1 }"),
        ("Plain[Nope]", "Plain { value: 1 }"),
    ] {
        let main = format!("{head}fn broken() -> void:\n    let value: {ty} = {value}\n");
        assert_eq!(codes_of(&main), [Code::UnknownImport], "{ty}");
    }
}
