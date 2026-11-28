use std::cell::Cell;
thread_local! {
    static CONTEXT_WORK: Cell<Option<ContextWork>> = const { Cell::new(None) };
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
    let (result, tree) = hd_types::work::count_context_map_steps(run);
    let mut work = CONTEXT_WORK
        .with(|c| c.replace(previous))
        .unwrap_or_default();
    work.tree = tree;
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
pub(crate) fn context_item_step() {
    context_step(|w| w.items += 1);
}
