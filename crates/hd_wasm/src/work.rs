//! Opt-in work counters for generated complexity regressions. A normal
//! build leaves these counters disabled; a serial driver run emits on
//! the calling thread, so each test measures on its own.

use std::cell::Cell;

thread_local! {
    static CONTROL: Cell<Option<u64>> = const { Cell::new(None) };
    static WALK_PLANS: Cell<Option<u64>> = const { Cell::new(None) };
    static USE_FACTS: Cell<Option<u64>> = const { Cell::new(None) };
    static LOAN_QUERIES: Cell<Option<u64>> = const { Cell::new(None) };
    static MEMBER_LAYOUTS: Cell<Option<u64>> = const { Cell::new(None) };
    static LOOP_SCANS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts control continuations, target-index operations and branch-shape
/// census/compaction visits on this thread.
pub fn count_control<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&CONTROL, run)
}

/// Counts recipe nodes/edges, mapping composition and view/import visits.
pub fn count_walk_plans<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&WALK_PLANS, run)
}

pub(crate) fn walk_plans(n: usize) {
    hd_base::work::add(hd_base::work::Counter::WalkPlans, n as u64);
    add(&WALK_PLANS, n);
}

/// Counts the work the emitter spends on its per-value use facts while
/// `run` runs on this thread: one per instruction a body's fact pass
/// visits, and one per query of the facts ([`crate::emit`]'s
/// `lazy_borrow`).
pub fn count_use_facts<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&USE_FACTS, run)
}

/// Counts immutable-body loan census visits, actual local uses, and
/// call argument query/index work on this thread.
pub fn count_loan_queries<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&LOAN_QUERIES, run)
}

/// Counts the members the layout's struct and tuple member memo lays
/// out, and its probes, on this thread ([`crate::layout::members`]), and
/// the inline parts the walk for a release's `Drop` calls enters.
pub fn count_member_layouts<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&MEMBER_LAYOUTS, run)
}

pub(crate) fn member_layouts(n: usize) {
    add(&MEMBER_LAYOUTS, n);
}

/// Counts the instructions the emitter's loop and statement questions
/// visit on this thread: each body's block tree once, and each list of
/// a loop's or a run of statements' instructions it hands out
/// ([`crate::emit`]'s `Tree`).
pub fn count_loop_scans<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&LOOP_SCANS, run)
}

pub(crate) fn loop_scans(n: usize) {
    hd_base::work::add(hd_base::work::Counter::LoopScans, n as u64);
    add(&LOOP_SCANS, n);
}

fn count<R>(
    counter: &'static std::thread::LocalKey<Cell<Option<u64>>>,
    run: impl FnOnce() -> R,
) -> (R, u64) {
    struct Restore(
        &'static std::thread::LocalKey<Cell<Option<u64>>>,
        Option<u64>,
    );
    impl Drop for Restore {
        fn drop(&mut self) {
            self.0.set(self.1);
        }
    }
    let restore = Restore(counter, counter.replace(Some(0)));
    let result = run();
    let work = counter.get().unwrap_or(0);
    drop(restore);
    (result, work)
}

/// Adds `n` to the count, when one is being taken.
pub(crate) fn use_facts(n: usize) {
    hd_base::work::add(hd_base::work::Counter::UseFacts, n as u64);
    add(&USE_FACTS, n);
}

pub(crate) fn loan_queries(n: usize) {
    hd_base::work::add(hd_base::work::Counter::LoanQueries, n as u64);
    add(&LOAN_QUERIES, n);
}

pub(crate) fn control(n: usize) {
    hd_base::work::add(hd_base::work::Counter::Control, n as u64);
    add(&CONTROL, n);
}

fn add(counter: &'static std::thread::LocalKey<Cell<Option<u64>>>, n: usize) {
    counter.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n as u64));
        }
    });
}
