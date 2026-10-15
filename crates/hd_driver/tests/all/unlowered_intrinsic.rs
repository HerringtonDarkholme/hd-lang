//! An intrinsic the backend does not lower is a build error, never its
//! source-level `panic("intrinsic")` placeholder at run time.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build, std_sources};
use hd_sched::SerialOrder;

#[test]
fn an_unlowered_intrinsic_fails_at_build_time() {
    let mut sources = std_sources();
    sources.insert(
        "bad.hd",
        r#"@intrinsic("task_all_frame")
pub fn unlowered() -> void:
    panic("intrinsic")
"#,
    );
    sources.insert(
        "main.hd",
        r"use std.bad.unlowered

pub fn main() -> void:
    unlowered()
",
    );
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "std",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    let rendered = out.render();
    assert!(out.wasm.is_none(), "the program compiled to Wasm");
    assert!(out.diags.code.contains(&Code::Unsupported), "{rendered}");
    assert!(
        rendered.contains("the intrinsic `task_all_frame`"),
        "{rendered}"
    );
}
