//! An unread binding of a must-use value is `discarded-must-use-value`, not
//! the unused-binding warning (spec/lang/06-control-flow.md,
//! flow.unused.must-use, flow.unused.discard-forms).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(body: &str) -> Vec<Code> {
    let main = format!(
        "fn work!() -> i32: 42\nfn res() -> Result[i32, string]: .Ok(1)\n\
         fn opt() -> i32?: 1\npub fn main() -> void:\n{body}\n"
    );
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
    let out: Output = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags.code.clone()
}

#[test]
fn unread_bindings_of_must_use_values_are_errors() {
    for line in [
        "    p := work()",
        "    let p = work()",
        "    _pending := work()",
    ] {
        assert_eq!(check(line), [Code::DiscardedMustUseValue], "{line}");
    }
}

#[test]
fn read_or_discarded_bindings_are_fine() {
    for body in [
        "    r := res()\n    _ := r",
        "    _ := work()",
        "    let _ = work()",
        "    _ := res()",
        "    o := opt()\n    f := fn() -> bool: o == none\n    _ := f()",
    ] {
        assert!(
            !check(body).contains(&Code::DiscardedMustUseValue),
            "{body}"
        );
    }
}

#[test]
fn plain_unused_binding_stays_a_warning() {
    assert_eq!(check("    x := +1"), [Code::UnusedLocalBinding]);
}

#[test]
fn unread_result_and_optional_bindings_stay_warnings() {
    for line in [
        "    r := res()",
        "    let r = res()",
        "    o := opt()",
        "    let o = opt()",
    ] {
        assert_eq!(check(line), [Code::UnusedLocalBinding], "{line}");
    }
    for line in ["    _r := res()", "    _o := opt()"] {
        assert!(check(line).is_empty(), "{line}");
    }
}
