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

fn assert_clean(body: &str) {
    let out = analyzed(&format!("{DECLS}\n{body}"));
    assert!(out.diags.is_empty(), "{}", out.render());
}

fn assert_codes(body: &str, want: &[Code]) {
    let out = analyzed(&format!("{DECLS}\n{body}"));
    assert_eq!(out.diags.code, want, "{}", out.render());
}

/// A caller without `Db` cannot call `run_all` with `Db` handlers: the
/// nested row solves `R` as `$ Db`, which the caller must provide.
#[test]
fn a_caller_without_the_solved_row_is_missing_it() {
    assert_codes(
        "\
pub fn wire(handlers: List[fn() -> void $ Db]) -> void:
    run_all(handlers)
",
        &[Code::MissingRequirement],
    );
}

/// With `Db` in its row, or with a wider row, the call is valid.
#[test]
fn a_caller_with_the_solved_row_is_clean() {
    assert_clean(
        "\
pub fn wire(handlers: List[fn() -> void $ Db]) -> void $ Db:
    run_all(handlers)

pub fn wider(handlers: List[fn() -> void $ Db]) -> void $ Db + Clock:
    run_all(handlers)
",
    );
}

/// A caller whose own row parameter is the nested row forwards it.
#[test]
fn a_row_parameter_forwards() {
    assert_clean(
        "\
pub fn forward[$Q](xs: List[fn() -> void $ Q]) -> void $ Q:
    run_all(xs)
",
    );
    assert_codes(
        "\
pub fn forward[$Q](xs: List[fn() -> void $ Q]) -> void:
    run_all(xs)
",
        &[Code::MissingRequirement],
    );
}

/// An empty handler row needs nothing; a list literal still takes the
/// least solution of its elements.
#[test]
fn empty_and_literal_arguments_keep_working() {
    assert_clean(
        "\
fn quiet() -> void:
    pass

pub fn wire(handlers: List[fn() -> void]) -> void:
    run_all(handlers)
    run_all([quiet])
    run_all([])
",
    );
    assert_codes(
        "\
pub fn wire() -> void:
    run_all([db_handler])
",
        &[Code::MissingRequirement],
    );
}

/// The same solution inside a map, an optional, a tuple, a data generic
/// and a list of lists.
#[test]
fn nested_in_map_option_tuple_data_and_lists() {
    for (decl, call) in [
        ("routes: Map[string, fn() -> void $ Db]", "run_map(routes)"),
        ("f: (fn() -> void $ Db)?", "run_option(f)"),
        ("p: (i32, fn() -> void $ Db)", "run_pair(p)"),
        ("h: Holder[fn() -> void $ Db]", "run_holder(h)"),
        ("xs: List[List[fn() -> void $ Db]]", "run_deep(xs)"),
    ] {
        assert_codes(
            &format!("pub fn wire({decl}) -> void:\n    {call}\n"),
            &[Code::MissingRequirement],
        );
        assert_clean(&format!("pub fn wire({decl}) -> void $ Db:\n    {call}\n"));
    }
}

/// Two occurrences of one row parameter agree: one solution, one missing
/// requirement.
#[test]
fn two_occurrences_that_agree_solve_once() {
    assert_clean(
        "\
pub fn wire(a: List[fn() -> void $ Db], b: List[fn() -> void $ Db]) -> void $ Db:
    run_two(a, b)
",
    );
    assert_codes(
        "\
pub fn wire(a: List[fn() -> void $ Db], b: Map[string, fn() -> void $ Db]) -> void:
    run_both(a, b)
",
        &[Code::MissingRequirement],
    );
}

/// Two occurrences that disagree are one `type-mismatch` at the second
/// argument, in either order, and no requirement error follows.
#[test]
fn two_occurrences_that_disagree_are_one_mismatch() {
    assert_codes(
        "\
pub fn wire(a: List[fn() -> void $ Db], b: List[fn() -> void $ Clock]) -> void $ Db + Clock:
    run_two(a, b)
",
        &[Code::TypeMismatch],
    );
    assert_codes(
        "\
pub fn wire(a: List[fn() -> void $ Db], b: Map[string, fn() -> void $ Db + Clock]) -> void $ Db + Clock:
    run_both(a, b)
",
        &[Code::TypeMismatch],
    );
}

/// A pattern beside a concrete key takes exactly what the nested row lists
/// beyond that key.
#[test]
fn a_pattern_with_a_key_solves_the_rest() {
    assert_clean(
        "\
pub fn wire(xs: List[fn() -> void $ Db + Clock]) -> void $ Db:
    run_without_clock(xs)
",
    );
    assert_codes(
        "\
pub fn wire(xs: List[fn() -> void $ Db + Clock]) -> void:
    run_without_clock(xs)
",
        &[Code::MissingRequirement],
    );
    assert_codes(
        "\
pub fn wire(xs: List[fn() -> void $ Db]) -> void $ Db:
    run_without_clock(xs)
",
        &[Code::TypeMismatch],
    );
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
