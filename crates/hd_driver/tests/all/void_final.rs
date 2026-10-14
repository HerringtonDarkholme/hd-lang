//! A `void` function rejects a non-`void` final expression (spec 07
//! `fn.body.void-final`): a literal or call tail is `type-mismatch`, while
//! `pass`, `return`, a diverging tail, and an inferred result stay clean.

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

#[test]
fn non_void_tail_is_one_error() {
    for tail in ["42", "g()"] {
        let main = format!(
            "fn g() -> i32: 1\n\nfn f() -> void:\n    {tail}\n\npub fn main() -> void:\n    pass\n"
        );
        assert_eq!(codes(&main), vec![Code::TypeMismatch], "{tail}");
    }
}

#[test]
fn void_and_diverging_tails_stay_clean() {
    for tail in ["pass", "return", "panic(\"x\")"] {
        let main = format!("fn f() -> void:\n    {tail}\n\npub fn main() -> void:\n    pass\n");
        assert_eq!(codes(&main), Vec::<Code>::new(), "{tail}");
    }
}

#[test]
fn inferred_result_stays_clean() {
    let main = "fn f():\n    42\n\npub fn main() -> void:\n    pass\n";
    assert_eq!(codes(main), Vec::<Code>::new());
}

#[test]
fn must_use_and_debug_tails_keep_their_own_diagnostics() {
    let main = "fn maybe() -> i32?: .None\n\nfn f() -> void:\n    maybe()\n\npub fn main() -> void:\n    pass\n";
    assert_eq!(codes(main), vec![Code::DiscardedMustUseValue]);
}
