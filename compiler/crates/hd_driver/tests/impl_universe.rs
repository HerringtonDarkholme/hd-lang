//! Impl universes (trait-solver.md §3.2, §14.2 memo invariance): two
//! modules ask the same open goal, `Receiver: Pick[?]`, with the same
//! traits in scope but different closures. Only `a`'s closure holds
//! `impl Pick[Product] for Receiver`, which its folder owns only through
//! the trait argument, so `a` finds it through the candidate directory and
//! `b` does not, whichever body runs first and on any thread count. The
//! same holds for a goal with known arguments whose proof reads an
//! unowned row (a misplaced impl) that only one closure holds.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const BASE: &str = "\
pub trait Pick[T]:
    fn pick(self) -> T

pub data Receiver:
    pub n: i32
";

const PROD: &str = "\
use pkg.base.defs.{Pick, Receiver}

pub data Product:
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
    let store = MemoryStore::default();
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

const TAG_LIB: &str = "\
pub trait Tag:
    fn tag(self) -> i32

pub fn need[T < Tag](x: T) -> i32:
    1
";

/// A misplaced impl: `prod` owns neither `Tag` nor `i32`.
const TAG_PROD: &str = "\
use pkg.lib.defs.{Tag}

pub data Product:
    pub id: i32

impl Tag for i32:
    fn tag(self) -> i32:
        self
";

const TAG_WITH: &str = "\
use pkg.lib.defs.{need}
use pkg.prod.defs.{Product}

pub fn first(p: Product) -> i32:
    need(p.id)
";

const TAG_WITHOUT: &str = "\
use pkg.lib.defs.{need}

pub fn second(x: i32) -> i32:
    need(x)
";

/// The rendered diagnostics of the misplaced-impl package; `with_first`
/// puts the folder whose closure holds `prod` first in folder order.
fn analyze_misplaced(executor: Executor, with_first: bool) -> String {
    let mut src = MemorySources::default();
    src.insert("lib/defs.hd", TAG_LIB);
    src.insert("prod/defs.hd", TAG_PROD);
    let fw = if with_first {
        "with/main.hd"
    } else {
        "zwith/main.hd"
    };
    src.insert(fw, TAG_WITH);
    src.insert("without/main.hd", TAG_WITHOUT);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor,
    };
    let out = build(&host, "app", &Goal::Analyze);
    out.render().replace("zwith/main.hd", "with/main.hd")
}

/// An `Implements` goal with known arguments (`i32: Tag`) keys without
/// the universe, but its proof reads unowned rows: the misplaced impl in
/// `prod` is visible only where the closure holds `prod`. `without/` must
/// report its unsatisfied bound whichever folder asks first.
#[test]
fn an_answer_that_read_unowned_rows_is_not_shared_across_universes() {
    let reference = analyze_misplaced(Executor::Serial(SerialOrder::Priority), true);
    let count = |file: &str, code: &str| {
        reference
            .lines()
            .filter(|l| l.contains(file) && l.contains(code))
            .count()
    };
    assert_eq!(
        count("without/main.hd", "unsatisfied-trait-bound"),
        1,
        "`without/`'s closure has no `impl Tag for i32`:\n{reference}"
    );
    assert!(
        !reference.contains("with/main.hd"),
        "`with/` sees the misplaced impl and reports nothing:\n{reference}"
    );
    assert!(
        reference.contains("prod/defs.hd")
            && (reference.contains("orphan-impl") || reference.contains("nonlocal-impl")),
        "`prod` reports its misplaced impl:\n{reference}"
    );
    for with_first in [true, false] {
        for executor in EXECUTORS {
            assert_eq!(
                analyze_misplaced(executor, with_first),
                reference,
                "{executor:?}, with first: {with_first}"
            );
        }
    }
}

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
