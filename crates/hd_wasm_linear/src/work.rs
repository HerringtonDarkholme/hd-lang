//! Opt-in work counters for generated complexity regressions. A normal
//! build only reads an empty thread-local per body; a serial driver run
//! emits on the calling thread, so each test measures on its own.

use std::cell::Cell;

thread_local! {
    static USE_FACTS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts the work the emitter spends on its per-value use facts while
/// `run` runs on this thread: one per instruction a body's fact pass
/// visits, and one per query of the facts ([`crate::emit`]'s
/// `lazy_borrow`).
pub fn count_use_facts<R>(run: impl FnOnce() -> R) -> (R, u64) {
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            USE_FACTS.set(self.0);
        }
    }
    let restore = Restore(USE_FACTS.replace(Some(0)));
    let result = run();
    let work = USE_FACTS.get().unwrap_or(0);
    drop(restore);
    (result, work)
}

/// Adds `n` to the count, when one is being taken.
pub(crate) fn use_facts(n: usize) {
    USE_FACTS.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n as u64));
        }
    });
}
