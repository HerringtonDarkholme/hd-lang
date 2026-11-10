//! Opt-in work counters for generated complexity regressions. Normal
//! checks do not allocate a table. A serial driver run stays on the
//! calling thread, so each test can measure independently.

use std::cell::RefCell;

use hd_base::{DefId, HashMap, NodeIdx};

/// Actual entries into a body, including speculative passes which were
/// subsequently rolled back. Active depth counts registered item checks,
/// not the lexical nesting of closures inside one such check.
pub struct BodyChecks {
    pub owner: String,
    pub at: NodeIdx,
    pub checks: usize,
    /// Fuel charged to item checks, including speculative passes.
    pub fuel_spent: u64,
    pub max_active_depth: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Local,
    Item,
}

struct Measurement {
    kind: Kind,
    counts: HashMap<(DefId, NodeIdx), BodyChecks>,
}

thread_local! {
    static CHECKS: RefCell<Option<Measurement>> = const { RefCell::new(None) };
}

struct Restore(Option<Measurement>);

impl Drop for Restore {
    fn drop(&mut self) {
        CHECKS.with(|sink| {
            sink.replace(self.0.take());
        });
    }
}

fn count<R>(kind: Kind, run: impl FnOnce() -> R) -> (R, Vec<BodyChecks>) {
    let previous = CHECKS.with(|sink| {
        sink.replace(Some(Measurement {
            kind,
            counts: HashMap::default(),
        }))
    });
    let restore = Restore(previous);
    let result = run();
    let counts = CHECKS.with(|sink| {
        sink.take()
            .map(|m| m.counts.into_values().collect())
            .unwrap_or_default()
    });
    drop(restore);
    (result, counts)
}

/// Count actual local-body checking work performed by `run` on this
/// thread. Use the driver's serial executor to measure a whole build.
pub fn count_local_body_checks<R>(run: impl FnOnce() -> R) -> (R, Vec<BodyChecks>) {
    count(Kind::Local, run)
}

/// Count actual named-item body checks and their maximum active depth.
/// Use the driver's serial executor to measure a whole build.
pub fn count_item_body_checks<R>(run: impl FnOnce() -> R) -> (R, Vec<BodyChecks>) {
    count(Kind::Item, run)
}

fn body(
    cx: &crate::body::BodyCx<'_>,
    owner: DefId,
    at: NodeIdx,
    kind: Kind,
    fuel_spent: impl FnOnce() -> u64,
) {
    CHECKS.with(|sink| {
        if let Some(m) = &mut *sink.borrow_mut()
            && m.kind == kind
        {
            let depth = if kind == Kind::Item {
                cx.results.borrow().active_depth()
            } else {
                0
            };
            let entry = m.counts.entry((owner, at)).or_insert_with(|| BodyChecks {
                owner: cx.names.path(owner),
                at,
                checks: 0,
                fuel_spent: 0,
                max_active_depth: 0,
            });
            entry.checks += 1;
            entry.fuel_spent += fuel_spent();
            entry.max_active_depth = entry.max_active_depth.max(depth);
        }
    });
}

pub(crate) fn local_body(ck: &crate::body::Ck<'_, '_>, at: NodeIdx) {
    body(ck.cx, ck.gen_owner, at, Kind::Local, || 0);
}

pub(crate) fn item_body(
    cx: &crate::body::BodyCx<'_>,
    owner: DefId,
    at: NodeIdx,
    fuel_spent: impl FnOnce() -> u64,
) {
    body(cx, owner, at, Kind::Item, fuel_spent);
}
