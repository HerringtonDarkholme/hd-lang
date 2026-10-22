//! Implementation targets and ownership (spec 09 "Implementation
//! Targets", "Implementation Ownership", "Implementation Modules",
//! "Inherent Methods On `dyn` Types", "Overlap"; spec 14 `annot.no-trait.unique`
//! and `annot.template.tuple.overlap`): a rejected implementation gets
//! one error on its header and is then left out, so nothing in its body
//! is reported and no overlap follows. The accepted forms keep working.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
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

/// The files report exactly `code`, once.

// trait.target.bare-parameter, trait.from.reflexive

// trait.own.orphan, trait.own.inherent

#[test]
fn an_orphan_impl_is_left_out_of_the_overlap_check() {
    // The same pair twice would overlap, but each is one orphan error.
    let out = analyze(&[(
        "main.hd",
        "impl Display for i32:\n    fn to_string(self) -> string:\n        \"a\"\n\nimpl Display for i32:\n    fn to_string(self) -> string:\n        \"b\"\n",
    )]);
    assert_eq!(
        codes(&out),
        vec![Code::OrphanImpl, Code::OrphanImpl],
        "{}",
        out.render()
    );
}

// trait.own.module.*, trait.own.dyn-inherent.*

// trait.overlap, annot.no-trait.unique, annot.template.tuple.overlap

const STRUCTURE: &str = "use std.structure.Structure\n\nstruct Tag:\n    text: string\n\nfn tag(text: string) -> Tag:\n    Tag { text: text }\n\nstruct User:\n    name: string\n    email: string\n\nstruct Group:\n    title: string\n\n";

#[test]
fn a_third_block_is_a_second_error() {
    let out = analyze(&[(
        "main.hd",
        &format!(
            "{STRUCTURE}impl User by Structure:\n    name += [tag(\"n\")]\n\nimpl User by Structure:\n    email += [tag(\"e\")]\n\nimpl User by Structure\n"
        ),
    )]);
    assert_eq!(
        codes(&out),
        vec![Code::OverlappingImpl, Code::OverlappingImpl],
        "{}",
        out.render()
    );
}
