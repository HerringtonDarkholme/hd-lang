//! A row parameter inside a type argument takes its solution from the
//! argument's nested function row, exactly (#305; spec 11
//! `req.row.least.solution`, spec 04 `types.variance.function`). The caller
//! then provides the solved row as for any callee, several occurrences
//! agree, and a mismatch between types that differ only in rows prints
//! the rows.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyzed(main: &str) -> Output {
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

const DECLS: &str = "\
pub trait Db

pub trait Clock

fn db_handler() -> void $ Db:
    pass

fn db_clock_handler() -> void $ Db + Clock:
    pass

pub struct Holder[T]:
    item: T

fn run_all[$R](xs: List[fn() -> void $ R]) -> void $ R:
    for x in xs:
        x()

fn run_map[$R](xs: Map[string, fn() -> void $ R]) -> void $ R:
    pass

fn run_option[$R](f: (fn() -> void $ R)?) -> void $ R:
    pass

fn run_pair[$R](p: (i32, fn() -> void $ R)) -> void $ R:
    pass

fn run_holder[$R](h: Holder[fn() -> void $ R]) -> void $ R:
    pass

fn run_two[$R](a: List[fn() -> void $ R], b: List[fn() -> void $ R]) -> void $ R:
    pass

fn run_both[$R](a: List[fn() -> void $ R], b: Map[string, fn() -> void $ R]) -> void $ R:
    pass

fn run_without_clock[$R](xs: List[fn() -> void $ R + Clock]) -> void $ R:
    pass

fn run_deep[$R](xs: List[List[fn() -> void $ R]]) -> void $ R:
    pass
";

fn codes(body: &str) -> Vec<Code> {
    analyzed(&format!("{DECLS}\n{body}")).diags.code.clone()
}

/// A mismatch between types that differ only in rows prints the rows.
#[test]
fn a_row_mismatch_shows_the_rows() {
    let out = analyzed(&format!(
        "{DECLS}\n{}",
        "\
pub fn wire(handlers: List[fn() -> string $ Db]) -> void:
    let wide: List[fn() -> string $ Db + Clock] = handlers
    _ := wide
"
    ));
    assert_eq!(out.diags.code, vec![Code::TypeMismatch], "{}", out.render());
    let text = out.render();
    assert!(
        text.contains(
            "expected List[fn() -> string $ Clock + Db], found List[fn() -> string $ Db]"
        ),
        "{text}"
    );
}

/// Types that already differ read as before, with no rows.
#[test]
fn other_mismatches_keep_their_message() {
    let out = analyzed(&format!(
        "{DECLS}\n{}",
        "\
pub fn wire(handlers: List[fn() -> string $ Db]) -> void:
    let wide: List[i32] = handlers
    _ := wide
"
    ));
    assert_eq!(out.diags.code, vec![Code::TypeMismatch], "{}", out.render());
    assert!(
        out.render()
            .contains("expected List[i32], found List[fn() -> string]"),
        "{}",
        out.render()
    );
    assert!(codes("pub fn ok() -> void:\n    pass\n").is_empty());
}
