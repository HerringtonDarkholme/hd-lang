//! Opt-in work counters for generated complexity regressions. A normal
//! build leaves these counters disabled; a serial driver run emits on
//! the calling thread, so each test measures on its own.

use std::cell::Cell;

thread_local! {
    static CONTROL: Cell<Option<u64>> = const { Cell::new(None) };
    static USE_FACTS: Cell<Option<u64>> = const { Cell::new(None) };
    static LOAN_QUERIES: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts control continuations, target-index operations and branch-shape
/// census/compaction visits on this thread.
pub fn count_control<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&CONTROL, run)
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
    add(&USE_FACTS, n);
}

pub(crate) fn loan_queries(n: usize) {
    add(&LOAN_QUERIES, n);
}

pub(crate) fn control(n: usize) {
    add(&CONTROL, n);
}

fn add(counter: &'static std::thread::LocalKey<Cell<Option<u64>>>, n: usize) {
    counter.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n as u64));
        }
    });
}
