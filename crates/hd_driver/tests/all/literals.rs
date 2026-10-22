//! Literal typing and conversions: negative literals into unsigned types,
//! numeric literal suffixes (`250ticks`), and string prefixes (`sql"..."`).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str) -> Output {
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

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

const MAIN: &str = "\npub fn main() -> void:\n    pass\n";

#[test]
fn fractional_duration_hints_use_the_written_value() {
    for (literal, hint) in [
        ("1.5s", Some("1500ms")),
        ("2.25min", Some("135000ms")),
        ("0.125h", Some("450000ms")),
        ("1.000s", Some("1000ms")),
        ("0.1ms", None),
        ("1e3ms", None),
    ] {
        let src = format!("fn delay():\n    _ := {literal}\n{MAIN}");
        let out = program(&src);
        assert!(
            codes(&out).contains(&Code::TypeMismatch),
            "{}",
            out.render()
        );
        let message = out.render();
        assert!(message.contains(literal), "{message}");
        if let Some(hint) = hint {
            assert!(message.contains(&format!("write `{hint}`")), "{message}");
        } else {
            assert!(!message.contains("; write"), "{message}");
        }
    }
}

// `expr.literal-fn.marker` and `.not-marked`: a marker is recognized by its
// resolved declaration, including aliases and qualified paths, never by text.
