//! Misplaced decorators (spec 09 `trait.impl.intrinsic.std-only`, spec
//! 14 `annot.error.marker.outside`): `@intrinsic` outside the standard
//! library names no binding, and `@from` outside an error type is an
//! ordinary decorator that must resolve. Each with its legal twin.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
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

fn errors(main: &str) -> Vec<Code> {
    let out = analyze(main);
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

#[test]
fn intrinsic_outside_std_is_unknown_name() {
    let main = "use std.ops.Add\n\ndata Money:\n    cents: i64\n\nimpl Add for Money:\n    type Out = Money\n    @intrinsic\n    fn add(self, rhs: Money) -> Money\n";
    assert_eq!(errors(main), vec![Code::UnknownName]);
}

#[test]
fn from_outside_error_type_is_unknown_name() {
    let main = "@error\nenum ReadError:\n    @error(\"closed\")\n    Closed\n\nenum Plain:\n    Wrap(@from error: ReadError)\n";
    assert_eq!(errors(main), vec![Code::UnknownName]);
}

#[test]
fn from_with_binding_outside_error_type_stays_clean() {
    let main = "fn from(value: i32) -> i32: value\n\nenum Plain:\n    Wrap(@from error: i32)\n\npub fn main() -> void:\n    pass\n";
    assert_eq!(errors(main), Vec::<Code>::new());
}
