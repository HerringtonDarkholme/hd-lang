//! Opt-in work counters of the driver's own steps, as `hd_types::work`.
//! Normal runs keep no count. A serial driver run stays on the calling
//! thread, so each test measures independently.

use std::cell::Cell;

thread_local! {
    static CONTEXT_WORK: Cell<Option<ContextWork>> = const { Cell::new(None) };
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

/// Actual context construction and reader work. `items` counts each
/// unit interface registered with the readers plus every member actually
/// enumerated. Separating it from the graph fold exposes quadratic reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContextWork {
    pub graph: u64,
    pub tree: u64,
    pub items: u64,
}

pub fn count_context_work<R>(run: impl FnOnce() -> R) -> (R, ContextWork) {
    let previous = CONTEXT_WORK.with(|c| c.replace(Some(ContextWork::default())));
    let result = run();
    let work = CONTEXT_WORK
        .with(|c| c.replace(previous))
        .unwrap_or_default();
    (result, work)
}

fn context_step(add: impl FnOnce(&mut ContextWork)) {
    CONTEXT_WORK.with(|c| {
        if let Some(mut work) = c.get() {
            add(&mut work);
            c.set(Some(work));
        }
    });
}

pub(crate) fn context_graph_step() {
    context_step(|w| w.graph += 1);
}
pub(crate) fn context_tree_step() {
    context_step(|w| w.tree += 1);
}
pub(crate) fn context_item_step() {
    context_step(|w| w.items += 1);
}
