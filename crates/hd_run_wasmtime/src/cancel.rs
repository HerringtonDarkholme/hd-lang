//! A host's cancellation of the entry (`req.entry.cancel`,
//! `cli.cancel.ctrl-c`): a request, as a Ctrl-C makes, that finds the
//! root suspension waiting on the host cancels it through `hd.cancel`, as
//! `cancel!()` does with the host as the waiter. The run then goes on
//! through the root's cleanup. The next request, or the cleanup bound
//! passing, abandons the root through `hd.abandon`. The bound is a
//! deadline fixed at the first cancel (`req.entry.cancel.bound`), and the
//! host waits for it as for a timer of its own, so a cleanup that waits on
//! nothing else is abandoned at the deadline instead of deadlocking. A
//! request while Wasm runs stops the run at once, as a REPL stop does
//! (`cli.cancel.computing`). `instance.rs` drives all of this.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::runtime::reactor::Waker;

/// Asks a run to cancel its entry. It is `Send + Sync` and cheap to
/// clone, for a Ctrl-C handler on another thread; a request made before
/// the run starts waits for it.
#[derive(Clone, Default)]
pub struct HostInterrupt {
    pub(crate) flag: Arc<AtomicBool>,
    /// The reactor of the run, once it started, which a request wakes.
    pub(crate) waker: Arc<Mutex<Option<Waker>>>,
}

impl HostInterrupt {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One request, as one Ctrl-C makes.
    pub fn request(&self) {
        self.flag.store(true, Ordering::Relaxed);
        if let Some(waker) = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            waker.wake();
        }
    }
}

/// How far the host's cancellation of a run went: its cleanup bound, and
/// whether it cancelled, with the deadline it then set, or abandoned.
pub(crate) struct HostCancel {
    bound: Duration,
    cancelled: bool,
    /// When the bound passes; `None` before the cancel, or for a bound past
    /// what an `Instant` holds.
    deadline: Option<Instant>,
    abandoned: bool,
}

/// What a request or the bound asks of the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CancelStep {
    Cancel,
    Abandon,
}

impl HostCancel {
    pub(crate) fn new(bound: Duration) -> Self {
        Self {
            bound,
            cancelled: false,
            deadline: None,
            abandoned: false,
        }
    }

    /// The step a request (`asked`) or the bound asks for now, if any,
    /// recorded as taken: the first request cancels and sets the
    /// deadline, and the next one or the deadline abandons.
    pub(crate) fn next(&mut self, asked: bool, now: Instant) -> Option<CancelStep> {
        if !self.cancelled {
            if !asked {
                return None;
            }
            self.cancelled = true;
            self.deadline = now.checked_add(self.bound);
            return Some(CancelStep::Cancel);
        }
        let due = self.deadline.is_some_and(|d| now >= d);
        if self.abandoned || !(asked || due) {
            return None;
        }
        self.abandoned = true;
        Some(CancelStep::Abandon)
    }

    /// The deadline the host waits for while it bounds a cleanup.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline.filter(|_| !self.abandoned)
    }
}

#[cfg(test)]
mod tests {
    use super::{CancelStep, HostCancel};
    use std::time::{Duration, Instant};

    #[test]
    fn the_first_request_cancels_and_the_next_or_the_bound_abandons() {
        let t0 = Instant::now();
        let mut c = HostCancel::new(Duration::from_millis(100));
        assert_eq!(c.next(false, t0), None);
        assert_eq!(c.next(true, t0), Some(CancelStep::Cancel));
        let deadline = t0 + Duration::from_millis(100);
        assert_eq!(c.deadline(), Some(deadline));
        assert_eq!(c.next(false, t0 + Duration::from_millis(50)), None);
        assert_eq!(c.next(false, deadline), Some(CancelStep::Abandon));
        assert_eq!(c.deadline(), None);
        assert_eq!(c.next(true, deadline), None);

        let mut twice = HostCancel::new(Duration::from_secs(10));
        assert_eq!(twice.next(true, t0), Some(CancelStep::Cancel));
        assert_eq!(twice.next(true, t0), Some(CancelStep::Abandon));
    }
}
