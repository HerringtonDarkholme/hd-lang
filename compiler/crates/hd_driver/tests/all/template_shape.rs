//! By-Structure template shape checks (spec/lang/14-annotations.md,
//! annot.structure.named-positions, annot.template.body;
//! spec/lang/10-modules.md, module.package.template-private).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(items: &str) -> Vec<Code> {
    let main = format!("{items}\npub fn main() -> void: pass\n");
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
fn a_structure_bound_outside_a_template_is_reported_once() {
    let src = "use std.structure.Structure\n\nfn fields[X < Structure]() -> void:\n    pass\n";
    assert_eq!(check(src), [Code::StructureOutsideTemplate]);
}

#[test]
fn a_structure_bound_on_a_type_is_reported() {
    let src = "use std.structure.Structure\n\ndata Box[X < Structure]:\n    value: X\n";
    assert_eq!(check(src), [Code::StructureOutsideTemplate]);
}

#[test]
fn an_ordinary_bound_and_the_import_stay_clean() {
    let src = "use std.structure.Structure\n\ntrait Named:\n    fn title() -> string\n\nfn show[X < Named]() -> void:\n    pass\n";
    assert_eq!(check(src), []);
}
