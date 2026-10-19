//! Named-argument identity errors (spec/lang/07-functions.md, fn.arg.*;
//! spec/lang/08-data-and-enums.md, data.enum.construct.call-codes): a
//! duplicate or unknown named argument reports its own code once, and the
//! slot it was meant to fill is not also an `argument-count` error.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const PRELUDE: &str = "\
enum Pair:
    Value(first: i32, second: i32)

struct Size:
    width: i32

impl Size:
    fn resize(self, width: i32, height: i32) -> i32: width * height

fn resize(width: i32, height: i32) -> i32: width * height

";

fn codes(body: &str) -> Vec<Code> {
    let mut src = MemorySources::default();
    src.insert("main.hd", &format!("{PRELUDE}{body}"));
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn duplicate_on_function() {
    let c = codes("fn f() -> i32: resize(640, width=480)\n");
    assert_eq!(c, vec![Code::DuplicateArgument]);
}

#[test]
fn duplicate_on_method() {
    let c = codes("fn f(s: Size) -> i32: s.resize(640, width=480)\n");
    assert_eq!(c, vec![Code::DuplicateArgument]);
}

#[test]
fn duplicate_on_variant() {
    let c = codes("fn f() -> Pair: Pair.Value(1, first=2)\n");
    assert_eq!(c, vec![Code::DuplicateArgument]);
}

#[test]
fn unknown_named_argument() {
    let c = codes("fn f() -> i32: resize(640, depth=480)\n");
    assert_eq!(c, vec![Code::UnknownNamedArgument]);
}

#[test]
fn plain_arity_error() {
    let c = codes("fn f() -> i32: resize(640)\n");
    assert_eq!(c, vec![Code::ArgumentCount]);
}
