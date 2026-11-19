//! Opt-in work counters of the layout builder, for generated complexity
//! regressions; off unless a test takes a count on its thread.

use std::cell::Cell;

thread_local! {
    static WALK_PLANS: Cell<Option<u64>> = const { Cell::new(None) };
    static MEMBER_LAYOUTS: Cell<Option<u64>> = const { Cell::new(None) };
    static LAYOUT_LEAVES: Cell<Option<u64>> = const { Cell::new(None) };
    static LAYOUT_PARTS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts recipe nodes/edges, mapping composition and view/import visits.
pub fn count_walk_plans<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&WALK_PLANS, run)
}

pub fn walk_plans(n: usize) {
    hd_base::work::add(hd_base::work::Counter::WalkPlans, n as u64);
    add(&WALK_PLANS, n);
}

/// Counts the members the layout's struct and tuple member memo lays
/// out, its probes, and the enum variants whose payload types are materialized
/// ([`crate::members`] and [`crate::variants`]), the inline
/// parts the walk for a release's `Drop` calls enters, and the types whose
/// `Drop` question is answered from their parts.
pub fn count_member_layouts<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&MEMBER_LAYOUTS, run)
}

pub fn member_layouts(n: usize) {
    add(&MEMBER_LAYOUTS, n);
}

/// Counts the types the layout queries compute from their parts on this
/// thread: one per type a layout walk (`by_parts`) answers,
/// which a cache in place answers once per scope.
pub fn count_layout_parts<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&LAYOUT_PARTS, run)
}

pub fn layout_parts(n: usize) {
    add(&LAYOUT_PARTS, n);
}

/// Counts scalar leaves physically copied when aggregate layouts are placed.
/// An empty ownership walk should never materialize those layouts.
pub fn count_layout_leaves<R>(run: impl FnOnce() -> R) -> (R, u64) {
    count(&LAYOUT_LEAVES, run)
}

pub fn layout_leaves(n: usize) {
    add(&LAYOUT_LEAVES, n);
}

/// Runs `run` with `counter` counting on this thread, and its count.
pub fn count<R>(
    counter: &'static std::thread::LocalKey<Cell<Option<u64>>>,
    run: impl FnOnce() -> R,
) -> (R, u64) {
    struct Restore(
        &'static std::thread::LocalKey<Cell<Option<u64>>>,
        Option<u64>,
    );
    impl Drop for Restore {
        fn drop(&mut self) {
            self.0.set(self.1);
        }
    }
    let restore = Restore(counter, counter.replace(Some(0)));
    let result = run();
    let work = counter.get().unwrap_or(0);
    drop(restore);
    (result, work)
}

/// Adds `n` to `counter`, when one is being taken.
pub fn add(counter: &'static std::thread::LocalKey<Cell<Option<u64>>>, n: usize) {
    counter.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n as u64));
        }
    });
}
