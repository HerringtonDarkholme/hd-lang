thread_local! {
    static CONTEXT_MAP_STEPS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Counts actual persistent-context map union nodes on this thread.
pub fn count_context_map_steps<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = CONTEXT_MAP_STEPS.with(|c| c.replace(Some(0)));
    let result = run();
    let work = CONTEXT_MAP_STEPS.with(|c| {
        let work = c.get().unwrap_or(0);
        c.set(previous.map(|n| n + work));
        work
    });
    (result, work)
}
pub(crate) fn context_map_step() {
    CONTEXT_MAP_STEPS.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
