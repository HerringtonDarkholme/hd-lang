//! Opt-in work counters for generated complexity regressions, as
//! `hd_check::work`. Normal runs keep no count. A serial driver run stays
//! on the calling thread, so each test measures independently.

use std::cell::Cell;

thread_local! {
    static SCOPE_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
    static OUTER_PROBES: Cell<Option<u64>> = const { Cell::new(None) };
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
