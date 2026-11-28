//! An opt-in work counter for generated complexity regressions. A normal
//! build reads an empty thread-local once per type it hashes or walks; a
//! serial driver run collects on the calling thread, so each test
//! measures on its own.

use std::cell::Cell;

/// Work in collection, emission, conversion enumeration and path queries of
/// instantiated dyn supertrait DAGs. Count real requests/edges, not only
/// successfully interned final tables: duplicate traversal must be visible.
#[derive(Clone, Copy, Debug, Default)]
pub struct DynWork {
    pub collected: u64,
    pub emitted: u64,
    pub paths: u64,
    pub conversions: u64,
    /// Table slots left pending and resolved when a later call through a
    /// trait value first names their method (`Cx::note_dyn`).
    pub late_slots: u64,
    /// Tables, traits and records the slot facts fold visits
    /// (`Cx::fold_slot_facts`).
    pub folded: u64,
}

thread_local! {
    static DYN_WORK: Cell<Option<DynWork>> = const { Cell::new(None) };
}

pub fn count_dyn_work<R>(run: impl FnOnce() -> R) -> (R, DynWork) {
    struct Restore(Option<DynWork>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DYN_WORK.set(self.0);
        }
    }
    let restore = Restore(DYN_WORK.replace(Some(DynWork::default())));
    let result = run();
    let work = DYN_WORK.get().unwrap_or_default();
    drop(restore);
    (result, work)
}

fn dyn_add(n: usize, field: impl FnOnce(&mut DynWork) -> &mut u64) {
    DYN_WORK.with(|cell| {
        if let Some(mut work) = cell.get() {
            *field(&mut work) += n as u64;
            cell.set(Some(work));
        }
    });
}

pub fn dyn_collect(n: usize) {
    dyn_add(n, |w| &mut w.collected);
}
pub fn dyn_emit(n: usize) {
    dyn_add(n, |w| &mut w.emitted);
}
pub fn dyn_path(n: usize) {
    dyn_add(n, |w| &mut w.paths);
}
pub fn dyn_conversion(n: usize) {
    dyn_add(n, |w| &mut w.conversions);
}
pub fn dyn_slot(n: usize) {
    dyn_add(n, |w| &mut w.late_slots);
}
pub fn dyn_fold(n: usize) {
    dyn_add(n, |w| &mut w.folded);
}

thread_local! {
    static REPS: std::cell::RefCell<Option<Vec<(String, hd_base::Hash128)>>> =
        const { std::cell::RefCell::new(None) };
}

/// Each root instance's final `callee_reps`, by its item's description,
/// of every collection `run` makes on this thread: what a code key holds
/// besides the instance's own TIR, for tests of what an edit invalidates.
pub fn capture_reps<R>(run: impl FnOnce() -> R) -> (R, Vec<(String, hd_base::Hash128)>) {
    struct Restore(Option<Vec<(String, hd_base::Hash128)>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            REPS.set(self.0.take());
        }
    }
    let restore = Restore(REPS.replace(Some(Vec::new())));
    let result = run();
    let reps = REPS.replace(None).unwrap_or_default();
    drop(restore);
    (result, reps)
}

pub(crate) fn reps_wanted() -> bool {
    REPS.with(|r| r.borrow().is_some())
}

pub(crate) fn note_reps(name: String, reps: hd_base::Hash128) {
    REPS.with(|r| {
        if let Some(all) = r.borrow_mut().as_mut() {
            all.push((name, reps));
        }
    });
}

thread_local! {
    static TYPE_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
    static ENUM_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Counts declaration reads, type/parameter visits and graph entries of enum
/// classification. A cached query performs none of these operations.
pub fn count_enum_steps<R>(run: impl FnOnce() -> R) -> (R, u64) {
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ENUM_STEPS.set(self.0);
        }
    }
    let restore = Restore(ENUM_STEPS.replace(Some(0)));
    let result = run();
    let work = ENUM_STEPS.get().unwrap_or(0);
    drop(restore);
    (result, work)
}

pub(crate) fn enum_step() {
    hd_base::work::add(hd_base::work::Counter::EnumSteps, 1);
    ENUM_STEPS.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
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
    hd_base::work::add(hd_base::work::Counter::TypeSteps, 1);
    TYPE_STEPS.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
