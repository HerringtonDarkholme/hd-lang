//! Sealed traits (spec 09 "Sealed Traits"): user code implements none of
//! the traits of the table, whatever the target, with one
//! `sealed-trait-implementation` on the `impl` or on the member and
//! nothing after it. A bound, a supertrait and a value type may still name
//! one.

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

fn codes(main: &str) -> Vec<Code> {
    let out = analyze(main);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn the_error_starts_at_the_impl() {
    let main = "use std.num.Num

type Count(i64)

impl Num for Count
";
    let out = analyze(main);
    let rendered = out.render();
    let start = main.find("impl Num").unwrap_or(0);
    assert!(
        rendered.contains(&format!("main.hd:{start}..")),
        "{rendered}"
    );
    assert_eq!(codes(main), vec![Code::SealedTraitImplementation]);
}

/// `Suspend[T]` is a built-in type, so an `impl` of it names no trait
/// (`req.protocol.builtin`).
#[test]
fn an_impl_of_suspend_is_an_unknown_trait() {
    let main = "struct Handle: pass\n\nimpl Suspend[i32] for Handle:\n    fn poll(&mut self, context: PollContext) -> Poll[i32]: Poll.Ready(1)\n    fn cancel(&mut self) -> void: pass\n";
    let out = analyze(main);
    assert_eq!(codes(main), vec![Code::UnknownTrait], "{}", out.render());
    assert!(out.render().contains("built-in type"), "{}", out.render());
}
