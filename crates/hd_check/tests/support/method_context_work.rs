use std::cell::Cell;
thread_local! {
    static METHOD_CONTEXT_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts method-context registrations and persistent-map union nodes.
/// Normal compiler runs retain no counter or table.
pub fn count_method_context_steps<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let prior = METHOD_CONTEXT_STEPS.with(|c| c.replace(Some(0)));
    let (result, maps) = hd_types::work::count_context_map_steps(run);
    let steps = METHOD_CONTEXT_STEPS.with(|c| c.replace(prior)).unwrap_or(0);
    (result, steps + maps)
}

pub(crate) fn method_context_step() {
    METHOD_CONTEXT_STEPS.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
