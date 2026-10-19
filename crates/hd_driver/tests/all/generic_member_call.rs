//! Generic walker calls (spec 14 `annot.walker.generic-member-call`):
//! `member` through a generic walker type is for generated code only, so
//! source that calls it errors with `generic-member-call`. A call on a
//! concrete walker applies that type's own bounds and stays silent.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &MemoryStore::default(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

const WALKER: &str = "use std.structure.{Field, Variant, Walker}\n\nstruct Prober:\n    out: string\n\nimpl[S] Walker[S] for Prober:\n    type Error = never\n\n    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:\n        .Ok(())\n\n    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:\n        .Ok(())\n\n";

#[test]
fn member_through_a_generic_walker_errors() {
    let out = analyze(&format!(
        "{WALKER}fn forward[S](w: &mut Prober, h: Field[S, i64], value: i64) -> void:\n    _ := w.member(h, value)\n"
    ));
    assert!(codes(&out).is_empty(), "sanity: {}", out.render());
    let out = analyze(
        "use std.structure.{Field, Walker}\n\nfn forward[S, W < Walker[S]](w: &mut W, h: Field[S, i64], value: i64) -> void:\n    _ := w.member(h, value)\n",
    );
    assert_eq!(codes(&out), vec![Code::GenericMemberCall]);
}

#[test]
fn member_on_a_concrete_walker_stays_silent() {
    let out = analyze(&format!(
        "{WALKER}fn run(h: Field[i64, i64]) -> void:\n    let mut w = Prober {{ out: \"\" }}\n    _ := w.member(h, 1)\n"
    ));
    assert!(codes(&out).is_empty(), "{}", out.render());
}
