//! Member-line names (spec 14 `annot.line.name`, `annot.line.enum`): the
//! name on the left of a member line must be a direct member of the target
//! (a whole variant for enums), or `Self`. Any other name errors with
//! `unknown-annotation-member`.

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

#[test]
fn a_traitless_line_naming_no_member_errors() {
    let out = analyze(
        "use std.structure.Structure\n\nstruct User:\n    name: string\n\nimpl User by Structure:\n    missing = []\n",
    );
    assert_eq!(codes(&out), vec![Code::UnknownAnnotationMember]);
}

#[test]
fn an_enum_line_naming_a_payload_member_errors() {
    let out = analyze(
        "use std.structure.Structure\n\nenum Reply:\n    Sent(id: i64)\n    Busy\n\nimpl Reply by Structure:\n    id = []\n",
    );
    assert_eq!(codes(&out), vec![Code::UnknownAnnotationMember]);
}

#[test]
fn direct_members_self_and_variants_stay_silent() {
    let out = analyze(
        "use std.structure.Structure\n\nstruct User:\n    name: string\n\nimpl User by Structure:\n    Self += []\n    name = []\n\nenum Reply:\n    Sent(id: i64)\n    Busy\n\nimpl Reply by Structure:\n    Busy = []\n",
    );
    assert!(codes(&out).is_empty(), "{}", out.render());
}
