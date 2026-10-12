//! `PollContext` is constructed only by the host runtime and `std.task`
//! (req.poll-context.construction). It carries a private field, so a data
//! literal or a copy-update in user code is the ordinary `private-member`
//! error, once, and nothing else.

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

fn codes(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn a_data_literal_in_user_code_is_one_private_member_error() {
    let main =
        "pub fn main() -> void $ Console:\n    let _c = PollContext {}\n    println(\"x\")\n";
    let out = analyze(main);
    assert_eq!(codes(&out), vec![Code::PrivateMember], "{}", out.render());
}

#[test]
fn a_literal_naming_the_field_is_one_private_member_error() {
    let main = "pub fn main() -> void $ Console:\n    let _c = PollContext { waker: () }\n    println(\"x\")\n";
    let out = analyze(main);
    assert_eq!(codes(&out), vec![Code::PrivateMember], "{}", out.render());
}

#[test]
fn a_poll_context_parameter_is_still_allowed() {
    let main = "pub fn main() -> void $ Console:\n    println(\"x\")\n\nfn take(_c: PollContext) -> void: pass\n";
    let out = analyze(main);
    assert_eq!(codes(&out), vec![], "{}", out.render());
}
