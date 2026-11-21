//! Opt-in work counters for generated complexity regressions. A normal
//! build leaves these counters disabled; a serial driver run emits on
//! the calling thread, so each test measures on its own.

use std::cell::Cell;

use hd_layout::work::{add, count};

thread_local! {
    static CONTROL: Cell<Option<u64>> = const { Cell::new(None) };
    static USE_FACTS: Cell<Option<u64>> = const { Cell::new(None) };
    static LOAN_QUERIES: Cell<Option<u64>> = const { Cell::new(None) };
    static STATIC_LOOKS: Cell<Option<u64>> = const { Cell::new(None) };
    static LIVENESS: Cell<Option<u64>> = const { Cell::new(None) };
    static ZERO_FACTS: Cell<Option<u64>> = const { Cell::new(None) };
    static INIT_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
    static MOVED_PATHS: Cell<Option<u64>> = const { Cell::new(None) };
    static EXIT_ROWS: Cell<Option<u64>> = const { Cell::new(None) };
    static SHARED_CENSUSES: Cell<Option<u64>> = const { Cell::new(None) };
    static ENTRY_ROOTS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts shared-local censuses, independent of the number of cells emitted.
pub fn count_shared_censuses<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&SHARED_CENSUSES, run)
}

pub(crate) fn shared_census() {
    add(&SHARED_CENSUSES, 1);
}

/// Counts indexed case items and entry-root membership probes.
pub fn count_entry_roots<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&ENTRY_ROOTS, run)
}

/// Records index construction or membership work for entry selection.
pub fn entry_roots(n: usize) {
    add(&ENTRY_ROOTS, n);
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

/// Counts the instructions the static-value proof reads on this thread:
/// a function's body once per look at it ([`crate::opt::static_values`]).
pub fn count_static_looks<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&STATIC_LOOKS, run)
}

pub(crate) fn static_looks(n: usize) {
    add(&STATIC_LOOKS, n);
}

/// Counts the liveness and local coalescing work on this thread: each
/// instruction a rewrite sweep visits, each basic block a liveness pass
/// makes and each node of its persistent sets, and each local, loop and
/// class heap step of the coalescing scan
/// ([`crate::opt::coalesce`]).
pub fn count_liveness<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&LIVENESS, run)
}

pub(crate) fn liveness(n: usize) {
    add(&LIVENESS, n);
}

/// Counts rule 11's zero-fact work on this thread: each node of its
/// persistent sets asked for, made or found, and each node a meet visits.
pub fn count_zero_facts<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&ZERO_FACTS, run)
}

pub(crate) fn zero_facts(n: usize) {
    add(&ZERO_FACTS, n);
}

/// Counts the init keys the constants pass reads on this thread: each
/// key the caller or an entry helper names, put in its set once, and each
/// function's one probe of that set ([`crate::opt::constants`]).
pub fn count_init_probes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&INIT_PROBES, run)
}

pub(crate) fn init_probes(n: usize) {
    add(&INIT_PROBES, n);
}

/// Counts the work on moves out of locals kept by address on this
/// thread: each release plan step and flagged path a drop's level
/// visits ([`crate::emit`]'s `mem_release` and `mem_hooks`).
pub fn count_moved_paths<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&MOVED_PATHS, run)
}

/// Counts the exit-set rows on this thread: each row a body's census
/// indexes, and each row an emitted function's exit release visits
/// ([`crate::emit`]'s `release_all`).
pub fn count_exit_rows<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&EXIT_ROWS, run)
}

pub(crate) fn exit_rows(n: usize) {
    add(&EXIT_ROWS, n);
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
