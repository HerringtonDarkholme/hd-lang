//! Opt-in work counters of the driver's own steps, as `hd_types::work`.
//! Normal runs keep no count. A serial driver run stays on the calling
//! thread, so each test measures independently.

use std::cell::Cell;

thread_local! {
    static ROOT_LOOKUPS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts the collect step's root lookups on this thread: each extra root
/// added or found, each init's instance looked up, and each host-ref key
/// probed. Each is one hash probe, so the count is the step's work.
pub fn count_root_lookups<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = ROOT_LOOKUPS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = ROOT_LOOKUPS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One root lookup of the collect step.
pub(crate) fn root_lookup() {
    hd_base::work::add(hd_base::work::Counter::RootLookups, 1);
    ROOT_LOOKUPS.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
