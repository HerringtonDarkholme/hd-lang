//! Explicit type arguments for row parameters (spec 07
//! `fn.generic.explicit.row-dollar`): the slot takes a row type argument
//! written after `$`, so a bare key there is one `generic-kind-mismatch`,
//! and the `$` forms stay clean.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const PRELUDE: &str = "\
trait Log

fn provide_log[$R](callback: fn() -> void $ R + Log) -> void $ R:
    pass

fn quiet() -> void: pass
";

#[test]
fn bare_key_in_row_slot_is_one_error() {
    let main = format!(
        "{PRELUDE}\nfn run() -> void $ Log:\n    provide_log::[dyn Log](quiet)\n\npub fn main() -> void:\n    pass\n"
    );
    assert_eq!(codes(&main), vec![Code::GenericKindMismatch]);
}

#[test]
fn dollar_forms_in_row_slot_stay_clean() {
    for call in ["provide_log::[$ Log](quiet)", "provide_log::[$()](quiet)"] {
        let main = format!(
            "{PRELUDE}\nfn run() -> void $ Log:\n    {call}\n\npub fn main() -> void:\n    pass\n"
        );
        assert_eq!(codes(&main), Vec::<Code>::new(), "{main}");
    }
}
