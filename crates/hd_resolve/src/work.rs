//! Opt-in work counters for generated complexity regressions, as
//! `hd_check::work`. A serial driver run stays on the calling thread, so
//! each test measures independently.

use std::cell::Cell;

thread_local! {
    static SCOPE_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the probes header lowering makes into a generic scope on this
/// thread ([`crate::generic_scope::Scoped`]): one per name or type looked
/// up, however many parameters are in scope. Use the driver's serial
/// executor to measure a whole build.
pub fn count_scope_probes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = SCOPE_PROBES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = SCOPE_PROBES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One probe into a generic scope.
pub(crate) fn scope_probe() {
    SCOPE_PROBES.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
