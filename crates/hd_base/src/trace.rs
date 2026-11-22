//! A timing trace (#1802 draft 31): with `HD_TRACE=FILE`, `hd` records one
//! event per scheduler task and per named serial phase, and writes them as
//! Chrome trace-event JSON, the format of Clang's `-ftime-trace` and
//! rustc's `-Z self-profile` converters, which Perfetto opens. Off, a span
//! costs one relaxed load; on, two clock reads and one locked push.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static ON: AtomicBool = AtomicBool::new(false);
static START: OnceLock<Instant> = OnceLock::new();
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static THREADS: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// This thread's number in the trace, given on its first event.
    static THREAD: u32 = THREADS.fetch_add(1, Ordering::Relaxed);
}

/// One complete event: what ran, its kind (`task` or `phase`), its thread,
/// and its start and length in µs from [`start`].
pub struct Event {
    pub name: String,
    pub cat: &'static str,
    pub thread: u32,
    pub start_us: u64,
    pub dur_us: u64,
}

/// Turns the trace on for the rest of the process; times count from now.
pub fn start() {
    START.get_or_init(Instant::now);
    ON.store(true, Ordering::Relaxed);
}

/// A running span: recorded when it drops, if the trace is on.
pub struct Span(Option<(String, &'static str, Instant)>);

/// Starts a span of kind `cat`; `name` is made only when the trace is on.
#[must_use]
pub fn span(cat: &'static str, name: impl FnOnce() -> String) -> Span {
    Span(
        ON.load(Ordering::Relaxed)
            .then(|| (name(), cat, Instant::now())),
    )
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some((name, cat, at)) = self.0.take() else {
            return;
        };
        let end = Instant::now();
        let origin = *START.get_or_init(|| at);
        let us =
            |t: Instant| u64::try_from(t.duration_since(origin).as_micros()).unwrap_or(u64::MAX);
        let thread = THREAD.with(|t| *t);
        let event = Event {
            name,
            cat,
            thread,
            start_us: us(at),
            dur_us: us(end) - us(at),
        };
        EVENTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event);
    }
}

/// Every event recorded so far, in order of their ends.
#[must_use]
pub fn take() -> Vec<Event> {
    std::mem::take(
        &mut *EVENTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}
