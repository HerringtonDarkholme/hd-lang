//! `by Structure` must name `std.structure.Structure` (spec 14
//! `annot.block.structure-use`): without the import it is `unknown-trait`,
//! and with it the block stays clean.

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

fn errors(main: &str) -> Vec<Code> {
    let out = analyze(main);
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const PRELUDE: &str = "\
trait Show:
    fn show(self) -> string

struct Point:
    x: i64
";

#[test]
fn by_structure_without_import_is_unknown_trait() {
    let main = format!("{PRELUDE}\nimpl Show for Point by Structure\n");
    assert_eq!(errors(&main), vec![Code::UnknownTrait]);
}

#[test]
fn by_structure_with_import_stays_clean() {
    let main =
        format!("use std.structure.Structure\n\n{PRELUDE}\nimpl Debug for Point by Structure\n");
    assert_eq!(errors(&main), Vec::<Code>::new());
}
