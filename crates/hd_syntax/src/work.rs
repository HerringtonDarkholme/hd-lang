//! Opt-in work counters for generated complexity regressions, as
//! `hd_check::work`. Normal runs keep no count.

use std::cell::Cell;

thread_local! {
    static SKIM_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the steps of the skim's per-range passes on this thread
/// ([`crate::skim`]): each `use` line and each `tests:` block of the
/// merge that places the lines, and each function body and each `by`
/// implementation of the merge that places the bodies.
pub fn count_skim_steps<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = SKIM_STEPS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = SKIM_STEPS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One skim step.
pub(crate) fn skim_step() {
    SKIM_STEPS.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
