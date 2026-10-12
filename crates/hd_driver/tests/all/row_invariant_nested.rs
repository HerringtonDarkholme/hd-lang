//! A function type's row is invariant inside a type argument (#204b; spec 11
//! `req.row.subsume` Note, spec 04 `types.variance.function`,
//! `req.row.union.literal.invariant`). A function value widens its row by
//! subsumption, but a list, map, optional, tuple or data value holding
//! function values converts to no type with another row. A wider
//! collection is built by an explicit copy.

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
trait Db

trait Clock

fn narrow() -> void $ Db:
    pass

fn other() -> void $ Db:
    pass

data Holder[T]:
    item: T

data View[+T]:
    item: T

data Slot:
    handler: fn() -> void $ Db + Clock
";

fn codes(body: &str) -> Vec<Code> {
    let out = analyzed(&format!("{DECLS}\n{body}"));
    out.diags.code.clone()
}

fn assert_clean(body: &str) {
    let out = analyzed(&format!("{DECLS}\n{body}"));
    assert!(out.diags.is_empty(), "{}", out.render());
}

fn assert_mismatch(body: &str) {
    let out = analyzed(&format!("{DECLS}\n{body}"));
    assert_eq!(out.diags.code, vec![Code::TypeMismatch], "{}", out.render());
}

/// A function value widens at an argument, an assignment, a declared
/// binding, a return and a field (`req.row.subsume.sites`).
#[test]
fn a_function_value_still_widens() {
    assert_clean(
        "\
fn take(f: fn() -> void $ Db + Clock) -> void:
    pass

fn give() -> (fn() -> void $ Db + Clock):
    narrow

fn wire() -> void:
    take(narrow)
    let wide: fn() -> void $ Db + Clock = narrow
    let slot: fn() -> void $ Db + Clock = other
    slot = narrow
    _ := Slot { handler: narrow }
    _ := give()
    take(wide)
    take(slot)
",
    );
}

/// The explicit copy builds a wider list from a narrower one.
#[test]
fn a_spread_copy_builds_the_wider_list() {
    assert_clean(
        "\
fn serve_all(handlers: List[fn() -> void $ Db + Clock]) -> void:
    pass

fn wire() -> void:
    narrows := [narrow, other]
    let wide: List[fn() -> void $ Db + Clock] = [narrows...]
    serve_all(wide)
    serve_all([narrows...])
    serve_all([narrow, other])
",
    );
}

/// The same row converts: a list keeps its type through a call.
#[test]
fn a_list_with_the_same_row_passes() {
    assert_clean(
        "\
fn serve_all(handlers: List[fn() -> void $ Db]) -> void:
    pass

fn wire(handlers: List[fn() -> void $ Db]) -> void:
    serve_all(handlers)
    serve_all([narrow])
    let keep: List[fn() -> void $ Db] = handlers
    serve_all(keep)
",
    );
}

/// The fixture: a list inferred with one row does not convert to a wider
/// list at an argument.
#[test]
fn a_list_argument_with_a_narrower_row_is_a_mismatch() {
    assert_mismatch(
        "\
fn serve_all(handlers: List[fn() -> void $ Db + Clock]) -> void:
    pass

fn wire(handlers: List[fn() -> void $ Db]) -> void:
    serve_all(handlers)
",
    );
}

/// The reverse direction is no conversion either: the callee would call a
/// handler with providers it never asked for.
#[test]
fn a_list_argument_with_a_wider_row_is_a_mismatch() {
    assert_mismatch(
        "\
fn serve_all(handlers: List[fn() -> void $ Db]) -> void:
    pass

fn wire(handlers: List[fn() -> void $ Db + Clock]) -> void:
    serve_all(handlers)
",
    );
}

/// An assignment and a declared binding of a list.
#[test]
fn a_list_assignment_and_binding_are_mismatches() {
    assert_mismatch(
        "\
fn serve_all(handlers: List[fn() -> void $ Db + Clock]) -> void:
    pass

fn wire(handlers: List[fn() -> void $ Db]) -> void:
    let wide: List[fn() -> void $ Db + Clock] = handlers
    serve_all(wide)
",
    );
    assert_mismatch(
        "\
fn serve_all(handlers: List[fn() -> void $ Db + Clock]) -> void:
    pass

fn wire(handlers: List[fn() -> void $ Db]) -> void:
    let wide: List[fn() -> void $ Db + Clock] = [narrow]
    wide = handlers
    serve_all(wide)
",
    );
}

/// A returned list.
#[test]
fn a_list_return_is_a_mismatch() {
    assert_mismatch(
        "\
fn wide(handlers: List[fn() -> void $ Db]) -> List[fn() -> void $ Db + Clock]:
    handlers
",
    );
}

/// A field initializer.
#[test]
fn a_list_field_is_a_mismatch() {
    assert_mismatch(
        "\
data Server:
    handlers: List[fn() -> void $ Db + Clock]

fn wire(handlers: List[fn() -> void $ Db]) -> Server:
    Server { handlers: handlers }
",
    );
}

/// Map values.
#[test]
fn map_values_keep_their_row() {
    assert_mismatch(
        "\
fn serve_all(routes: Map[string, fn() -> void $ Db + Clock]) -> void:
    pass

fn wire(routes: Map[string, fn() -> void $ Db]) -> void:
    serve_all(routes)
",
    );
    assert_clean(
        "\
fn serve_all(routes: Map[string, fn() -> void $ Db + Clock]) -> void:
    pass

fn wire() -> void:
    serve_all({\"a\": narrow})
",
    );
}

/// An optional function.
#[test]
fn an_optional_function_keeps_its_row() {
    assert_mismatch(
        "\
fn maybe(f: (fn() -> void $ Db + Clock)?) -> void:
    pass

fn wire(f: (fn() -> void $ Db)?) -> void:
    maybe(f)
",
    );
    assert_clean(
        "\
fn maybe(f: (fn() -> void $ Db + Clock)?) -> void:
    pass

fn wire() -> void:
    maybe(narrow)
",
    );
}

/// A data generic, invariant or declared covariant, and a tuple.
#[test]
fn data_generics_and_tuples_keep_their_row() {
    assert_mismatch(
        "\
fn hold(h: Holder[fn() -> void $ Db + Clock]) -> void:
    pass

fn wire(h: Holder[fn() -> void $ Db]) -> void:
    hold(h)
",
    );
    assert_mismatch(
        "\
fn show(v: View[fn() -> void $ Db + Clock]) -> void:
    pass

fn wire(v: View[fn() -> void $ Db]) -> void:
    show(v)
",
    );
    assert_mismatch(
        "\
fn pair(p: (i32, fn() -> void $ Db + Clock)) -> void:
    pass

fn wire(p: (i32, fn() -> void $ Db)) -> void:
    pair(p)
",
    );
}

/// A function type inside another collection, and a function type in a
/// function's own input or output.
#[test]
fn nested_function_types_keep_their_row() {
    assert_mismatch(
        "\
fn serve_all(groups: List[List[fn() -> void $ Db + Clock]]) -> void:
    pass

fn wire(groups: List[List[fn() -> void $ Db]]) -> void:
    serve_all(groups)
",
    );
    assert_mismatch(
        "\
fn run(maker: fn() -> (fn() -> void $ Db + Clock)) -> void:
    pass

fn wire(maker: fn() -> (fn() -> void $ Db)) -> void:
    run(maker)
",
    );
}

/// One report at the value, with no error after it.
#[test]
fn the_mismatch_is_reported_once() {
    assert_eq!(
        codes(
            "\
fn serve_all(handlers: List[fn() -> void $ Db + Clock]) -> void:
    pass

fn wire(handlers: List[fn() -> void $ Db]) -> void:
    serve_all(handlers)
    serve_all(handlers)
",
        ),
        vec![Code::TypeMismatch, Code::TypeMismatch]
    );
}
