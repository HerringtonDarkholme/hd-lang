//! A failed `use` binds the names it lists as poison, so later references
//! add no second error (one error per mistake).

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

fn codes(src: &str) -> Vec<Code> {
    let out = program(src);
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

#[test]
fn unmentioned_name_is_still_unknown() {
    let src = "use pkg.nowhere.{f}

pub fn main() -> void:
    f()
    other()
";
    assert_eq!(codes(src), vec![Code::UnknownModule, Code::UnknownName]);
}
