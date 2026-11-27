//! Opt-in work counters of the driver's own steps, as `hd_types::work`.
//! Normal runs keep no count. A serial driver run stays on the calling
//! thread, so each test measures independently.

use std::cell::Cell;

thread_local! {
    static ROOT_LOOKUPS: Cell<Option<u64>> = const { Cell::new(None) };
    static INIT_ORDER_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
    static LAYOUT_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
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

/// Counts the steps of the init order on this thread: each module, cycle
/// and cycle edge the global order visits, and each cycle and edge a
/// root's restriction of it visits.
pub fn count_init_order_steps<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = INIT_ORDER_STEPS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = INIT_ORDER_STEPS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// `n` steps of the init order.
pub(crate) fn init_order_steps(n: usize) {
    let n = u64::try_from(n).unwrap_or(u64::MAX);
    hd_base::work::add(hd_base::work::Counter::InitOrderSteps, n);
    INIT_ORDER_STEPS.with(|c| {
        if let Some(k) = c.get() {
            c.set(Some(k.saturating_add(n)));
        }
    });
}

/// Counts the ownership passes' layout questions on this thread: each
/// probe of the run's layout memo (`Run::own_types`), and each entry it
/// makes from a type's parts.
pub fn count_layout_probes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = LAYOUT_PROBES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = LAYOUT_PROBES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One probe of the layout memo, or one entry made.
pub(crate) fn layout_probe() {
    LAYOUT_PROBES.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}

#[cfg(feature = "testing")]
#[path = "../tests/support/context_work.rs"]
mod contexts;
#[cfg(feature = "testing")]
pub use contexts::{ContextWork, count_context_work};
#[cfg(feature = "testing")]
pub(crate) use contexts::{context_graph_step, context_item_step};
#[cfg(not(feature = "testing"))]
#[inline]
pub(crate) fn context_graph_step() {}
#[cfg(not(feature = "testing"))]
#[inline]
pub(crate) fn context_item_step() {}
