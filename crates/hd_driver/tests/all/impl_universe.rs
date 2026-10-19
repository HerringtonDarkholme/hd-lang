//! Impl universes (`crates/hd_check/DESIGN.md` (trait solver) §3.2, §14.2 memo invariance): two
//! modules ask the same open goal, `Receiver: Pick[?]`, with the same
//! traits in scope but different closures. Only `a`'s closure holds
//! `impl Pick[Product] for Receiver`, which its folder owns only through
//! the trait argument, so `a` finds it through the candidate directory and
//! `b` does not, whichever body runs first and on any thread count.
//! A misplaced impl is left out of the program, so no closure holds an
//! unowned row; the solver's own tests cover that mechanism.

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const BASE: &str = "\
pub trait Pick[T]:
    fn pick(self) -> T

pub struct Receiver:
    pub n: i32
";

const PROD: &str = "\
use pkg.base.defs.{Pick, Receiver}

pub struct Product:
    pub id: i32

impl Pick[Product] for Receiver:
    fn pick(self) -> Product:
        Product { id: self.n }
";

const A: &str = "\
use pkg.base.defs.{Pick, Receiver}
use pkg.prod.defs.{Product}

pub fn first(r: Receiver) -> i32:
    let p: Product = r.pick()
    p.id
";

const B: &str = "\
use pkg.base.defs.{Pick, Receiver}

pub fn second(r: Receiver) -> i32:
    r.pick()
";

/// The rendered diagnostics of one analysis.
fn analyze(executor: Executor, a_first: bool) -> String {
    let mut src = MemorySources::default();
    src.insert("base/defs.hd", BASE);
    src.insert("prod/defs.hd", PROD);
    // File order decides module and folder order; swapping the folder
    // names swaps which body is checked first.
    let (fa, fb) = if a_first {
        ("a/main.hd", "b/main.hd")
    } else {
        ("z/main.hd", "b/main.hd")
    };
    src.insert(fa, A);
    src.insert(fb, B);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor,
    };
    let out = build(&host, "app", &Goal::Analyze);
    out.render().replace("z/main.hd", "a/main.hd")
}

const EXECUTORS: [Executor; 6] = [
    Executor::Serial(SerialOrder::Fifo),
    Executor::Serial(SerialOrder::Shuffled(7)),
    Executor::Serial(SerialOrder::Shuffled(11)),
    Executor::Pool(2),
    Executor::Pool(4),
    Executor::Pool(8),
];

#[test]
fn each_closure_gets_its_own_answer_in_any_order_and_thread_count() {
    let reference = analyze(Executor::Serial(SerialOrder::Priority), true);
    assert!(
        !reference.contains("a/main.hd"),
        "`a` sees the argument-owned impl:\n{reference}"
    );
    assert!(
        reference.contains("b/main.hd") && reference.contains("unknown-method"),
        "`b`'s closure has no `impl Pick[Product] for Receiver`:\n{reference}"
    );
    for a_first in [true, false] {
        for executor in EXECUTORS {
            assert_eq!(
                analyze(executor, a_first),
                reference,
                "{executor:?}, a first: {a_first}"
            );
        }
    }
}
