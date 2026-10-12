//! The host's pending operations (runtime-and-host.md §17.2, §17.5): each
//! `.start` that cannot finish at once gets a handle and completes later,
//! when its time comes (`Clock.sleep`, `Console.write_line`) or when
//! another thread delivers its result (`ConsoleInput.read_line`, read by
//! a thread of its own, so the program never blocks inside a host call).
//! The driver and `hd:rt` `block` wait here for the next completion: a
//! timer's deadline or a delivery, whichever comes first.

use std::collections::{BTreeMap, VecDeque};
use std::io::BufRead;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Nanoseconds since the host first started, the clock of
/// `performance.now()`.
pub fn now_ns() -> i128 {
    static START: OnceLock<Instant> = OnceLock::new();
    i128::try_from(START.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(i128::MAX)
}

pub const NS_PER_MS: i128 = 1_000_000;

/// One line of standard input without its ending: `Ok(None)` at the end
/// of input (and every time after), `Err(())` when it cannot be read.
pub type Line = Result<Option<String>, ()>;

/// What an operation does when it completes.
pub enum Work {
    /// `Console.write_line`: the line, written to standard output then.
    Write(Vec<u8>),
    /// `Clock.sleep`.
    Timer,
    /// `ConsoleInput.read_line`: the line, once it is delivered.
    Read(Option<Line>),
    /// `disposed-file` `open!`: allocates its handle at `.finish`.
    FixtureOpen,
    /// `disposed-file` `read!` of the file handle: decided at `.finish`.
    FixtureRead { file: i32 },
    /// `disposed-file` `close` of the file handle: decided at `.finish`.
    FixtureClose { file: i32 },
    /// `pending-gate` `wait!`: never completes, dropped at cancel.
    Gate,
}

pub struct Op {
    /// When it completes; `None` while a read waits for its line.
    at: Option<i128>,
    pub work: Work,
    completed: bool,
}

/// Lines the reader thread delivered and the reactor has not taken yet.
#[derive(Default)]
struct Inbox {
    lines: Mutex<VecDeque<Line>>,
    ready: Condvar,
}

pub struct Reactor {
    ops: BTreeMap<i32, Op>,
    next_handle: i32,
    inbox: Arc<Inbox>,
    /// Standard input, until the reader thread takes it; `None` when it is
    /// closed.
    input: Option<Box<dyn BufRead + Send>>,
    /// Asks the reader thread for one more line.
    requests: Option<Sender<()>>,
    /// The reads waiting for a line, oldest first.
    reads: VecDeque<i32>,
    /// Lines asked of the reader thread and not delivered yet.
    in_flight: usize,
    /// Lines delivered for a read that was aborted, for the next reads.
    spare: VecDeque<Line>,
}

impl Reactor {
    pub fn new(input: Option<Box<dyn BufRead + Send>>) -> Reactor {
        Reactor {
            ops: BTreeMap::new(),
            next_handle: 1,
            inbox: Arc::default(),
            input,
            requests: None,
            reads: VecDeque::new(),
            in_flight: 0,
            spare: VecDeque::new(),
        }
    }

    fn start(&mut self, at: Option<i128>, work: Work) -> i32 {
        let h = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        self.ops.insert(
            h,
            Op {
                at,
                work,
                completed: false,
            },
        );
        h
    }

    /// A `write_line` of `line`, which completes at once.
    pub fn write_line(&mut self, line: Vec<u8>) -> i32 {
        self.start(Some(now_ns()), Work::Write(line))
    }

    /// A sleep of `ms` milliseconds.
    pub fn sleep(&mut self, ms: i64) -> i32 {
        self.start(Some(now_ns() + i128::from(ms) * NS_PER_MS), Work::Timer)
    }

    /// A `disposed-file` `open!`, `read!`, or `close`: each completes at
    /// once, and its `.finish` decides the result from the provider state.
    pub fn fixture_open(&mut self) -> i32 {
        self.start(Some(now_ns()), Work::FixtureOpen)
    }

    /// A `disposed-file` `read!` of the file handle.
    pub fn fixture_read(&mut self, file: i32) -> i32 {
        self.start(Some(now_ns()), Work::FixtureRead { file })
    }

    /// A `disposed-file` `close` of the file handle.
    pub fn fixture_close(&mut self, file: i32) -> i32 {
        self.start(Some(now_ns()), Work::FixtureClose { file })
    }

    /// A `pending-gate` `wait!`: pending until its cancel drops it, so it
    /// has no deadline and no delivery.
    pub fn gate(&mut self) -> i32 {
        self.start(None, Work::Gate)
    }

    /// A read of the next line of standard input.
    pub fn read_line(&mut self) -> i32 {
        let h = self.start(None, Work::Read(None));
        if let Some(line) = self.spare.pop_front() {
            self.deliver(h, line);
        } else {
            self.reads.push_back(h);
            if self.in_flight < self.reads.len() {
                self.request();
                self.in_flight += 1;
            }
        }
        h
    }

    /// Asks for one more line: of the reader thread, which starts on the
    /// first request, or at once when standard input is closed.
    fn request(&mut self) {
        if self.requests.is_none() {
            let Some(input) = self.input.take() else {
                let mut lines = self
                    .inbox
                    .lines
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                lines.push_back(Ok(None));
                return;
            };
            let (tx, rx) = channel::<()>();
            let inbox = Arc::clone(&self.inbox);
            // Detached: a read blocked on standard input must not hold the
            // program's end.
            std::thread::spawn(move || {
                let mut input = input;
                let mut ended = false;
                while rx.recv().is_ok() {
                    let line = if ended {
                        Ok(None)
                    } else {
                        read_one(&mut *input, &mut ended)
                    };
                    let mut lines = inbox.lines.lock().unwrap_or_else(PoisonError::into_inner);
                    lines.push_back(line);
                    inbox.ready.notify_all();
                }
            });
            self.requests = Some(tx);
        }
        if let Some(tx) = &self.requests {
            // The thread only stops when this sender is dropped.
            let _ = tx.send(());
        }
    }

    fn deliver(&mut self, h: i32, line: Line) {
        if let Some(op) = self.ops.get_mut(&h) {
            op.work = Work::Read(Some(line));
            op.at = Some(now_ns());
        }
    }

    /// Takes the delivered lines, each for the oldest waiting read.
    fn take_delivered(&mut self) {
        let lines: Vec<Line> = {
            let mut inbox = self
                .inbox
                .lines
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            inbox.drain(..).collect()
        };
        for line in lines {
            self.in_flight = self.in_flight.saturating_sub(1);
            match self.reads.pop_front() {
                Some(h) => self.deliver(h, line),
                None => self.spare.push_back(line),
            }
        }
    }

    /// Completes every due operation, oldest first, writing each
    /// `write_line`'s line to `out`, and returns their handles.
    pub fn due(&mut self, out: &mut Vec<u8>) -> Vec<i32> {
        self.take_delivered();
        let now = now_ns();
        let mut due: Vec<(i128, i32)> = self
            .ops
            .iter()
            .filter_map(|(h, o)| match o.at {
                Some(at) if !o.completed && at <= now => Some((at, *h)),
                _ => None,
            })
            .collect();
        due.sort_unstable();
        for (_, h) in &due {
            if let Some(op) = self.ops.get_mut(h) {
                op.completed = true;
                if let Work::Write(line) = &mut op.work {
                    out.append(line);
                    out.push(b'\n');
                }
            }
        }
        due.into_iter().map(|(_, h)| h).collect()
    }

    /// Whether an operation has not completed: something can still wake
    /// the program.
    pub fn waiting(&self) -> bool {
        self.ops.values().any(|o| !o.completed)
    }

    /// Waits for the next timer deadline or delivery; `false` when nothing
    /// is pending, a deadlock.
    #[cfg(test)]
    pub fn wait(&self) -> bool {
        self.wait_until(None)
    }

    /// Like `wait`, but wakes no later than the program's wall deadline.
    pub fn wait_until(&self, limit: Option<Instant>) -> bool {
        if !self.waiting() {
            return false;
        }
        let deadline = self
            .ops
            .values()
            .filter(|o| !o.completed)
            .filter_map(|o| o.at)
            .min();
        let lines = self
            .inbox
            .lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !lines.is_empty() {
            return true;
        }
        // The guard is dropped as soon as the wait ends; a spurious wakeup
        // only makes the caller look again.
        let timeout = deadline.map(|d| {
            let ns = u64::try_from((d - now_ns()).max(0)).unwrap_or(u64::MAX);
            Duration::from_nanos(ns)
        });
        let timeout = match (timeout, limit) {
            (Some(timeout), Some(limit)) => {
                Some(timeout.min(limit.saturating_duration_since(Instant::now())))
            }
            (None, Some(limit)) => Some(limit.saturating_duration_since(Instant::now())),
            (timeout, None) => timeout,
        };
        match timeout {
            Some(timeout) => {
                drop(self.inbox.ready.wait_timeout(lines, timeout));
            }
            None => drop(self.inbox.ready.wait(lines)),
        }
        true
    }

    /// Ends an operation at its `.finish` or `abort`.
    pub fn remove(&mut self, h: i32) -> Option<Op> {
        self.reads.retain(|r| *r != h);
        self.ops.remove(&h)
    }
}

/// Reads one line as the Node host's line reader does: without its
/// `\n`, and a `\r` before it; a last line with no ending as it is.
fn read_one(input: &mut dyn BufRead, ended: &mut bool) -> Line {
    let mut line = Vec::new();
    match input.read_until(b'\n', &mut line) {
        Err(_) => Err(()),
        Ok(0) => {
            *ended = true;
            Ok(None)
        }
        Ok(_) => {
            if line.last() == Some(&b'\n') {
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
            } else {
                *ended = true;
            }
            Ok(Some(String::from_utf8_lossy(&line).into_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reactor, Work};

    #[test]
    fn reads_complete_in_order_and_an_aborted_read_loses_no_line() {
        let input: &'static [u8] = b"a\nb\r\nc";
        let mut r = Reactor::new(Some(Box::new(input)));
        let first = r.read_line();
        let second = r.read_line();
        r.remove(first);
        let mut out = Vec::new();
        let mut done = Vec::new();
        while done.is_empty() {
            assert!(r.wait());
            done = r.due(&mut out);
        }
        assert_eq!(done, [second]);
        let Some(op) = r.remove(second) else {
            panic!("the read is gone");
        };
        // The aborted read took no line: the next read gets the first, and
        // the line read for it is kept for the read after.
        assert!(matches!(op.work, Work::Read(Some(Ok(Some(ref l)))) if l == "a"));
        let third = r.read_line();
        let mut done = Vec::new();
        while done.is_empty() {
            assert!(r.wait());
            done = r.due(&mut out);
        }
        assert_eq!(done, [third]);
        let op = r.remove(third).expect("read");
        assert!(matches!(op.work, Work::Read(Some(Ok(Some(ref l)))) if l == "b"));
        assert!(!r.wait(), "nothing pending");
    }

    #[test]
    fn a_closed_input_ends_at_once() {
        let mut r = Reactor::new(None);
        let h = r.read_line();
        let mut out = Vec::new();
        assert!(r.wait());
        assert_eq!(r.due(&mut out), [h]);
    }

    #[test]
    fn gate_stays_pending_until_its_cancel_drops_it() {
        let mut r = Reactor::new(None);
        let h = r.gate();
        assert!(r.waiting(), "the wait is pending");
        let mut out = Vec::new();
        assert!(r.due(&mut out).is_empty(), "the wait never completes");
        assert!(r.waiting(), "still pending");
        let op = r.remove(h).expect("the cancel drops the wait");
        assert!(matches!(op.work, Work::Gate));
        assert!(!r.waiting(), "nothing pending");
    }
}
