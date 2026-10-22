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

struct Holder[T]:
    item: T

struct View[+T]:
    item: T

struct Slot:
    handler: fn() -> void $ Db + Clock
";

fn codes(body: &str) -> Vec<Code> {
    let out = analyzed(&format!("{DECLS}\n{body}"));
    out.diags.code.clone()
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
