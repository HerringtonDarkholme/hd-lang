//! Opt-in work counters for generated complexity regressions, as
//! `hd_check::work`. Normal runs keep no count. A serial driver run stays
//! on the calling thread, so each test measures independently.

use std::cell::Cell;

thread_local! {
    static TYPE_NODES: Cell<Option<u64>> = const { Cell::new(None) };
    static RESOLVE_NODES: Cell<Option<u64>> = const { Cell::new(None) };
    static ENV_LINKS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the clauses [`crate::solver::ParamEnv::renew_key`] links on
/// this thread: the work of keeping environment keys current.
pub fn count_env_links<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = ENV_LINKS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = ENV_LINKS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// `n` clauses linked into environment keys.
pub(crate) fn env_links(n: usize) {
    ENV_LINKS.with(|c| {
        if let Some(k) = c.get() {
            c.set(Some(k + n as u64));
        }
    });
}

/// Count the type nodes `InferTable::resolve` walks on this thread: the
/// share of [`count_type_nodes`] that resolving inference variables
/// takes, one per node not answered by a memo.
pub fn count_resolve_nodes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = RESOLVE_NODES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = RESOLVE_NODES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One type node `InferTable::resolve` walked.
pub(crate) fn resolve_node() {
    RESOLVE_NODES.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
    type_node();
}

/// Count the type nodes that resolving, folding and walking inference
/// variables visit on this thread (`InferTable::resolve`, the occurs
/// check, the solver's folds), substituting parameters (`Types::subst`),
/// the `Copy` row's parts, and the checker's `normalize_deep` and
/// instantiation-cycle walks: one per node walked, not per memo hit.
/// Use the driver's serial executor to measure a whole build.
pub fn count_type_nodes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = TYPE_NODES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = TYPE_NODES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One type node walked; public so the checker's own walks count.
pub fn type_node() {
    hd_base::work::add(hd_base::work::Counter::TypeNodes, 1);
    TYPE_NODES.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}

thread_local! {
    static VIEW_SLOTS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the folder slots of impl views that `Impls::tables_at` reads on
/// this thread. Use the driver's serial executor to measure a whole build.
pub fn count_view_slots<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = VIEW_SLOTS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = VIEW_SLOTS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// `n` folder slots of an impl view read.
pub fn view_slots(n: usize) {
    VIEW_SLOTS.with(|c| {
        if let Some(m) = c.get() {
            c.set(Some(m + n as u64));
        }
    });
}
