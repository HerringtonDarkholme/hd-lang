//! A function-typed operator operand selects implementations by its own
//! type: row subsumption does not apply (spec 05
//! `expr.op.left-dispatch.exact-function`). An implementation for a
//! function type with a wider row does not fit, and the error suggests a
//! binding typed with the implementation's function type
//! (`expr.op.left-dispatch.exact-function.message`).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn checked(main: &str) -> Output {
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

fn codes_of(main: &str) -> Vec<Code> {
    let out = checked(main);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const PRELUDE: &str = "\
use std.ops.Shr

trait Db

struct Layer:
    depth: i32

impl Shr[Layer] for fn(i32) -> i32 $ Db:
    type Out = i32
    fn shr(self, rhs: Layer) -> i32:
        rhs.depth

fn get(n: i32) -> i32:
    n
";

#[test]
fn wider_row_does_not_fit() {
    let main = format!(
        "{PRELUDE}\nfn wire(layer: Layer) -> i32:\n    get >> layer\n\npub fn main() -> void:\n    pass\n"
    );
    let out = checked(&main);
    let order = out.diags.content_order();
    assert_eq!(
        order.iter().map(|i| out.diags.code[*i]).collect::<Vec<_>>(),
        vec![Code::TypeMismatch]
    );
    let msg = out.diags.get_text(out.diags.message[order[0]]);
    assert!(msg.contains("let handler:"), "{msg}");
    assert!(msg.contains("fn(i32) -> i32 $ Db"), "{msg}");
}

#[test]
fn exact_row_fits() {
    let main = format!(
        "{PRELUDE}\nfn wire_typed(layer: Layer) -> i32:\n    let handler: fn(i32) -> i32 $ Db = get\n    handler >> layer\n\npub fn main() -> void:\n    pass\n"
    );
    assert_eq!(codes_of(&main), Vec::<Code>::new());
}
