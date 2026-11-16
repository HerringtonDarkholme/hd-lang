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
    static STATIC_LOOKS: Cell<Option<u64>> = const { Cell::new(None) };
    static LIVENESS: Cell<Option<u64>> = const { Cell::new(None) };
    static ZERO_FACTS: Cell<Option<u64>> = const { Cell::new(None) };
    static LAYOUT_PARTS: Cell<Option<u64>> = const { Cell::new(None) };
    static INIT_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
    static MOVED_PATHS: Cell<Option<u64>> = const { Cell::new(None) };
    static EXIT_ROWS: Cell<Option<u64>> = const { Cell::new(None) };
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
/// out, its probes, and the enum variants whose payload types are materialized
/// ([`crate::layout::members`] and [`crate::layout::variants`]), the inline
/// parts the walk for a release's `Drop` calls enters, and the types whose
/// `Drop` question is answered from their parts.
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

/// Counts the types the layout queries compute from their parts on this
/// thread: one per type a [`crate::layout`] walk (`by_parts`) answers,
/// which a cache in place answers once per scope.
pub fn count_layout_parts<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&LAYOUT_PARTS, run)
}

pub(crate) fn layout_parts(n: usize) {
    add(&LAYOUT_PARTS, n);
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

/// Counts the work on moves out of parameters kept by address on this
/// thread: each `Read move` the scan notes and each one emitted, one
/// probe each, each release plan step and moved path a drop's level
/// visits, and each move flag a frame local's store clears
/// ([`crate::emit`]'s `mem_moves`, `mem_read`, `mem_release` and
/// `mem_arrive`).
pub fn count_moved_paths<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&MOVED_PATHS, run)
}

pub(crate) fn moved_paths(n: usize) {
    add(&MOVED_PATHS, n);
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
