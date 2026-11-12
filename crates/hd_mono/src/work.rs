//! An opt-in work counter for generated complexity regressions. A normal
//! build reads an empty thread-local once per type it hashes or walks; a
//! serial driver run collects on the calling thread, so each test
//! measures on its own.

use std::cell::Cell;

thread_local! {
    static TYPE_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts the type nodes collection walks while `run` runs on this
/// thread: one per digest [`crate::layout::canon`] computes, and one per
/// type `items_in` visits. Both are linear in the distinct types they
/// reach, so a type that doubles per level costs a step per level, not
/// per leaf.
pub fn count_type_steps<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = TYPE_STEPS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = TYPE_STEPS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One type node walked.
pub(crate) fn type_step() {
    TYPE_STEPS.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
