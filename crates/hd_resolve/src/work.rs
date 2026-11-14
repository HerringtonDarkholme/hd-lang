//! Opt-in work counters for generated complexity regressions, as
//! `hd_check::work`. Normal runs keep no count. A serial driver run stays
//! on the calling thread, so each test measures independently.

use std::cell::Cell;

thread_local! {
    static SCOPE_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
    static OUTER_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
    static ITEM_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
    static ENCLOSING_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the probes header lowering makes into a generic scope on this
/// thread ([`crate::generic_scope::Scoped`]): one per name or type looked
/// up, however many parameters are in scope. Use the driver's serial
/// executor to measure a whole build.
pub fn count_scope_probes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = SCOPE_PROBES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = SCOPE_PROBES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One probe into a generic scope.
pub(crate) fn scope_probe() {
    hd_base::work::add(hd_base::work::Counter::ScopeProbes, 1);
    SCOPE_PROBES.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}

/// Count the work of finding a name among the parameters around a local
/// declaration on this thread: one per name hashed, and one per lookup.
/// Use the driver's serial executor to measure a whole build.
pub fn count_outer_probes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = OUTER_PROBES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = OUTER_PROBES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

fn add_outer(n: usize) {
    hd_base::work::add(hd_base::work::Counter::OuterProbes, n as u64);
    OUTER_PROBES.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n as u64));
        }
    });
}

/// `n` enclosing parameter names hashed for one local declaration.
pub(crate) fn outer_names(n: usize) {
    add_outer(n);
}

/// One lookup of a name among them.
pub(crate) fn outer_probe() {
    add_outer(1);
}

thread_local! {
    static LOCAL_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the lookups into a module's block-local declarations on this
/// thread ([`crate::Locals`]): one per name, byte or item asked about,
/// however many locals the module declares.
pub fn count_local_probes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = LOCAL_PROBES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = LOCAL_PROBES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One lookup into a module's locals.
pub(crate) fn local_probe() {
    LOCAL_PROBES.with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}

thread_local! {
    static ENCLOSING: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the work of the checker's enclosing-parameter index on this
/// thread ([`crate::EnclosingParams`]): one per parameter name indexed,
/// and one per question.
pub fn count_enclosing_work<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = ENCLOSING.with(|c| c.replace(Some(0)));
    let result = run();
    let n = ENCLOSING.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// `n` units of enclosing-parameter work.
pub(crate) fn enclosing_names(n: u64) {
    ENCLOSING.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n));
        }
    });
}

thread_local! {
    static HINT: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the work of std import hints on this thread
/// ([`crate::iface::std_import_hint`]): one per export indexed, one per
/// interface asked, and one per export of the asked name.
pub fn count_hint_work<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = HINT.with(|c| c.replace(Some(0)));
    let result = run();
    let n = HINT.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// `n` units of import-hint work.
pub(crate) fn hint_work(n: u64) {
    HINT.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n));
        }
    });
}

/// Count the work of matching the folder's implementations with their
/// traits on this thread: one per item looked up by `DefId`, and one per
/// implementation method checked against its trait's names. Use the
/// driver's serial executor to measure a whole build.
pub fn count_item_probes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = ITEM_PROBES.with(|c| c.replace(Some(0)));
    let result = run();
    let n = ITEM_PROBES.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// `n` steps of matching implementations with their traits.
pub(crate) fn item_probes(n: usize) {
    ITEM_PROBES.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n as u64));
        }
    });
}

/// One item looked up by `DefId`.
pub(crate) fn item_probe() {
    item_probes(1);
}

/// Count the work of finding the declarations around each local
/// declaration on this thread: one per node of the module walk that
/// indexes them, and one per enclosing generic list read back. Use the
/// driver's serial executor to measure a whole build.
pub fn count_enclosing_steps<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = ENCLOSING_STEPS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = ENCLOSING_STEPS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// One step of finding the declarations around a local declaration.
pub(crate) fn enclosing_step() {
    ENCLOSING_STEPS.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + 1));
        }
    });
}

thread_local! {
    static MEMBER_PAIRS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count the work of checking that inherent members of one name do not
/// clash on this thread: one per member, one per earlier member it is
/// compared with, and one per item indexed for numeric families. Use the
/// driver's serial executor to measure a whole build.
pub fn count_member_pairs<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = MEMBER_PAIRS.with(|c| c.replace(Some(0)));
    let result = run();
    let n = MEMBER_PAIRS.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// `n` units of inherent member comparison.
pub(crate) fn member_pairs(n: u64) {
    hd_base::work::add(hd_base::work::Counter::MemberPairs, n);
    MEMBER_PAIRS.with(|c| {
        if let Some(w) = c.get() {
            c.set(Some(w + n));
        }
    });
}
