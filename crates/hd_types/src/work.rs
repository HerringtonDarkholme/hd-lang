//! Opt-in work counters for generated complexity regressions, as
//! `hd_check::work`. Normal runs keep no count. A serial driver run stays
//! on the calling thread, so each test measures independently.

use std::cell::Cell;

thread_local! {
    static TYPE_NODES: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the type nodes that resolving, folding and walking inference
/// variables visit on this thread (`InferTable::resolve`, the occurs
/// check, the solver's folds): one per node walked, not per memo hit.
/// Use the driver's serial executor to measure a whole build.
pub fn count_type_nodes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = TYPE_NODES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = TYPE_NODES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One type node walked.
pub(crate) fn type_node() {
    hd_base::work::add(hd_base::work::Counter::TypeNodes, 1);
    TYPE_NODES.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
