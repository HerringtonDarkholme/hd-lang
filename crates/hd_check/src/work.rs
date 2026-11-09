//! Opt-in work counters for generated complexity regressions. Normal
//! checks do not allocate a table. A serial driver run stays on the
//! calling thread, so each test can measure independently.

use std::cell::RefCell;

use hd_base::{DefId, HashMap, NodeIdx};

/// Actual entries into a local function or closure body, including
/// speculative passes which were subsequently rolled back.
pub struct LocalBodyChecks {
    pub owner: String,
    pub at: NodeIdx,
    pub checks: usize,
}

thread_local! {
    static LOCAL_CHECKS: RefCell<Option<HashMap<(DefId, NodeIdx), LocalBodyChecks>>> = const { RefCell::new(None) };
}

struct Restore(Option<HashMap<(DefId, NodeIdx), LocalBodyChecks>>);

impl Drop for Restore {
    fn drop(&mut self) {
        LOCAL_CHECKS.with(|sink| {
            sink.replace(self.0.take());
        });
    }
}

/// Count actual local-body checking work performed by `run` on this
/// thread. Use the driver's serial executor to measure a whole build.
pub fn count_local_body_checks<R>(run: impl FnOnce() -> R) -> (R, Vec<LocalBodyChecks>) {
    let previous = LOCAL_CHECKS.with(|sink| sink.replace(Some(HashMap::default())));
    let restore = Restore(previous);
    let result = run();
    let counts = LOCAL_CHECKS.with(|sink| sink.take().unwrap_or_default().into_values().collect());
    drop(restore);
    (result, counts)
}

pub(crate) fn local_body(ck: &crate::body::Ck<'_, '_>, at: NodeIdx) {
    LOCAL_CHECKS.with(|sink| {
        if let Some(counts) = &mut *sink.borrow_mut() {
            let entry = counts
                .entry((ck.gen_owner, at))
                .or_insert_with(|| LocalBodyChecks {
                    owner: ck.cx.names.path(ck.gen_owner),
                    at,
                    checks: 0,
                });
            entry.checks += 1;
        }
    });
}
