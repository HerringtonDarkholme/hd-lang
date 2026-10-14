//! The host's pending operations (runtime-and-host.md §17.2;
//! wasmtime-host-audit.md §4): each `.start` that cannot finish at once
//! gets a handle and completes later, when its time comes (`Clock.sleep`,
//! `Console.write_line`) or when another thread delivers its result into
//! the inbox: a line of standard input (`ConsoleInput.read_line`, read by
//! a thread of its own), or the `Done` of an operation offloaded to the
//! `hd-io` runtime (`io_runtime.rs`). So the program never blocks inside a
//! host call. The driver and `hd:rt` `block` wait here for the next
//! completion: a timer's deadline or a delivery, whichever comes first.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::Future;
use std::io::BufRead;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::task::AbortHandle;

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

/// What an offloaded operation delivers, for its `.finish` to encode.
/// Each capability that moves onto the `hd-io` runtime adds its variant
/// (wasmtime-host-audit.md §5, T2-T7).
pub enum Done {
    /// `Process.run!`: how the child's run ended.
    Process(crate::core::process::Ran),
    /// A `Net` operation's result.
    Net(crate::net::Done),
    /// `Http.send!`: the response or the failure.
    Http(crate::http::client::Sent),
    /// The reactor tests' offloaded operation.
    #[cfg(test)]
    Test(u32),
}

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
    /// An operation offloaded to the `hd-io` runtime: its result, once
    /// its task delivers it.
    Offloaded(Option<Done>),
}

pub struct Op {
    /// When a timed operation completes; `None` for one that completes on
    /// a delivery.
    at: Option<i128>,
    pub work: Work,
    completed: bool,
    /// An offloaded operation's task, until it delivers: aborted when the
    /// operation ends first.
    task: Option<Task>,
    /// The resource whose handle this operation is on, so closing it can
    /// end its pending operations (`dispose_owned`).
    owner: Option<Owner>,
}

/// A resource that owns pending operations: the capability's name and the
/// resource's handle.
pub type Owner = (&'static str, i64);

/// An offloaded operation's running task.
struct Task {
    abort: AbortHandle,
    /// Disconnected once the task's work is dropped, when it ends or is
    /// aborted.
    ended: Receiver<()>,
}

impl Task {
    fn abort(&self) {
        self.abort.abort();
    }

    /// Waits until the aborted task's work is dropped, so whatever its drop
    /// does (killing a child, for one) has happened when an abort returns.
    /// The `hd-io` thread never waits on a program thread, so this cannot
    /// deadlock.
    fn ended(&self) {
        let _ = self.ended.recv();
    }
}

/// A task's work, then the sender that `Task::ended` waits on: fields drop
/// in order, so the sender goes only after the work.
struct Tracked<F> {
    work: Pin<Box<F>>,
    _ended: Sender<()>,
}

impl<F: Future> Future for Tracked<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        self.work.as_mut().poll(cx)
    }
}

/// What other threads delivered and the reactor has not taken yet.
#[derive(Default)]
struct Deliveries {
    /// Lines of standard input, each for the oldest waiting read.
    lines: VecDeque<Line>,
    /// Offloaded operations' results, by handle.
    done: Vec<(i32, Done)>,
}

impl Deliveries {
    fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.done.is_empty()
    }
}

/// The deliveries and the condvar that announces them.
#[derive(Default)]
struct Inbox {
    queue: Mutex<Deliveries>,
    ready: Condvar,
    /// Set, under `queue`, while a wait sleeps on `ready`.
    parked: AtomicBool,
}

/// Wakes a reactor's wait from another thread, for a session's stop
/// request: the waiter then reads the request's flag, which `wait_until`
/// checks under the same lock, so the wake is never lost.
#[derive(Clone)]
pub struct Waker(Arc<Inbox>);

impl Waker {
    pub fn wake(&self) {
        let _queue = self.0.queue.lock().unwrap_or_else(PoisonError::into_inner);
        self.0.ready.notify_all();
    }

    /// Whether the reactor's thread sleeps in `wait_until` now, so a wake
    /// reaches it there: no Wasm runs and no host call is made.
    pub fn parked(&self) -> bool {
        self.0.parked.load(Ordering::SeqCst)
    }
}

pub struct Reactor {
    ops: BTreeMap<i32, Op>,
    next_handle: i32,
    /// The operations not completed yet.
    open: usize,
    /// The timed operations not completed yet, by deadline.
    timers: BTreeSet<(i128, i32)>,
    /// The operations whose delivery was taken, with when, to complete at
    /// the next `due`: so `due` looks at these and the timers only, not at
    /// every pending operation.
    ready: Vec<(i128, i32)>,
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
            open: 0,
            timers: BTreeSet::new(),
            ready: Vec::new(),
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
        if let Some(at) = at {
            self.timers.insert((at, h));
        }
        self.open += 1;
        self.ops.insert(
            h,
            Op {
                at,
                work,
                completed: false,
                task: None,
                owner: None,
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

    /// An operation whose work is `work`, run as a task on the `hd-io`
    /// runtime, which starts on the first call. The task delivers its
    /// `Done` into the inbox, to complete at the next `due`; ending the
    /// operation first (`remove`, `drop_all`) aborts the task, and a
    /// result it delivered too late is dropped. An abort returns once the
    /// task's work is dropped.
    pub fn offload<F>(&mut self, work: F) -> Result<i32, String>
    where
        F: Future<Output = Done> + Send + 'static,
    {
        self.offload_for(None, work)
    }

    /// `offload` for an operation on the resource `owner`.
    pub fn offload_for<F>(&mut self, owner: Option<Owner>, work: F) -> Result<i32, String>
    where
        F: Future<Output = Done> + Send + 'static,
    {
        let runtime = crate::io_runtime::handle()?;
        let h = self.start(None, Work::Offloaded(None));
        if let Some(op) = self.ops.get_mut(&h) {
            op.owner = owner;
        }
        let inbox = Arc::clone(&self.inbox);
        let (sender, ended) = channel();
        let task = runtime.spawn(Tracked {
            work: Box::pin(async move {
                let done = work.await;
                let mut queue = inbox.queue.lock().unwrap_or_else(PoisonError::into_inner);
                queue.done.push((h, done));
                inbox.ready.notify_all();
            }),
            _ended: sender,
        });
        if let Some(op) = self.ops.get_mut(&h) {
            op.task = Some(Task {
                abort: task.abort_handle(),
                ended,
            });
        }
        Ok(h)
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
                let mut queue = self
                    .inbox
                    .queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                queue.lines.push_back(Ok(None));
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
                    let mut queue = inbox.queue.lock().unwrap_or_else(PoisonError::into_inner);
                    queue.lines.push_back(line);
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
            self.ready.push((now_ns(), h));
        }
    }

    /// Takes the deliveries: each line for the oldest waiting read, and
    /// each result for its operation. A result whose operation has ended,
    /// aborted while its task ran, is dropped.
    fn take_delivered(&mut self) {
        let (lines, done) = {
            let mut queue = self
                .inbox
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let lines: Vec<Line> = queue.lines.drain(..).collect();
            (lines, std::mem::take(&mut queue.done))
        };
        for line in lines {
            self.in_flight = self.in_flight.saturating_sub(1);
            match self.reads.pop_front() {
                Some(h) => self.deliver(h, line),
                None => self.spare.push_back(line),
            }
        }
        for (h, done) in done {
            if let Some(op) = self.ops.get_mut(&h)
                && matches!(op.work, Work::Offloaded(None))
            {
                op.task = None;
                op.work = Work::Offloaded(Some(done));
                self.ready.push((now_ns(), h));
            }
        }
    }

    /// Completes every due operation, oldest first, writing each
    /// `write_line`'s line to `out`, and returns their handles.
    pub fn due(&mut self, out: &mut Vec<u8>) -> Vec<i32> {
        self.take_delivered();
        let now = now_ns();
        let mut due = std::mem::take(&mut self.ready);
        while let Some(&(at, h)) = self.timers.first()
            && at <= now
        {
            self.timers.pop_first();
            due.push((at, h));
        }
        due.sort_unstable();
        for (_, h) in &due {
            if let Some(op) = self.ops.get_mut(h)
                && !op.completed
            {
                op.completed = true;
                self.open -= 1;
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
        self.open > 0
    }

    /// Waits for the next timer deadline or delivery; `false` when nothing
    /// is pending, a deadlock.
    #[cfg(test)]
    pub fn wait(&self) -> bool {
        self.wait_until(None, None)
    }

    /// The handle that wakes `wait_until` from another thread.
    pub fn waker(&self) -> Waker {
        Waker(Arc::clone(&self.inbox))
    }

    /// Like `wait`, but wakes no later than the program's wall deadline,
    /// and at once when `interrupt` is set, before or during the wait, by
    /// a thread that then uses `waker`.
    pub fn wait_until(&self, limit: Option<Instant>, interrupt: Option<&AtomicBool>) -> bool {
        if !self.waiting() {
            return false;
        }
        if !self.ready.is_empty() {
            return true;
        }
        let deadline = self.timers.first().map(|&(at, _)| at);
        let queue = self
            .inbox
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !queue.is_empty() || interrupt.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
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
        // Set under the lock: a waker that reads it as set takes the lock
        // only once this thread sleeps, so its wake is never lost.
        self.inbox.parked.store(true, Ordering::SeqCst);
        let queue = match timeout {
            Some(timeout) => {
                let (queue, _) = self
                    .inbox
                    .ready
                    .wait_timeout(queue, timeout)
                    .unwrap_or_else(PoisonError::into_inner);
                queue
            }
            None => self
                .inbox
                .ready
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner),
        };
        self.inbox.parked.store(false, Ordering::SeqCst);
        drop(queue);
        true
    }

    /// Ends an operation at its `.finish` or `abort`. A pending offloaded
    /// operation's task is aborted. A result its task delivered, before or
    /// while the abort ran, comes back in the operation, for a caller
    /// that keeps what it holds (`std-net.cancel.read`).
    pub fn remove(&mut self, h: i32) -> Option<Op> {
        self.reads.retain(|r| *r != h);
        let mut op = self.ops.remove(&h)?;
        if !op.completed {
            self.open -= 1;
            if let Some(at) = op.at {
                self.timers.remove(&(at, h));
            }
            self.ready.retain(|&(_, r)| r != h);
        }
        if let Some(task) = op.task.take() {
            task.abort();
            task.ended();
            let mut queue = self
                .inbox
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(i) = queue.done.iter().position(|(r, _)| *r == h) {
                op.work = Work::Offloaded(Some(queue.done.swap_remove(i).1));
            }
        }
        Some(op)
    }

    /// Completes every offloaded operation on `resource` that the program has
    /// not finished with the result `done` makes: each task is aborted and
    /// its work dropped before this returns, so whatever the work held (a
    /// socket) is released. A result a task delivered and the program has
    /// not taken is replaced.
    pub fn dispose_owned(&mut self, resource: Owner, mut done: impl FnMut() -> Done) {
        let ending: Vec<i32> = self
            .ops
            .iter()
            .filter(|(_, op)| op.owner == Some(resource))
            .map(|(h, _)| *h)
            .collect();
        for h in &ending {
            if let Some(task) = self.ops.get(h).and_then(|op| op.task.as_ref()) {
                task.abort();
                task.ended();
            }
        }
        let mut queue = self
            .inbox
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        queue.done.retain(|(h, _)| !ending.contains(h));
        drop(queue);
        for h in ending {
            if let Some(op) = self.ops.get_mut(&h) {
                op.task = None;
                op.work = Work::Offloaded(Some(done()));
                if !op.completed {
                    self.ready.push((now_ns(), h));
                }
            }
        }
    }

    /// Ends every operation, as `abort` ends one: a session's input that
    /// did not end with status 0 may leave some, and none of them may wake
    /// a later input or keep its wait from being a deadlock. A line already
    /// asked of the reader thread is kept for the next read; every
    /// offloaded task is aborted. Returns the results the tasks delivered,
    /// as `remove` does.
    pub fn drop_all(&mut self) -> Vec<Done> {
        self.abort_tasks();
        self.reads.clear();
        self.open = 0;
        self.timers.clear();
        self.ready.clear();
        let mut delivered: Vec<Done> = std::mem::take(&mut self.ops)
            .into_values()
            .filter_map(|op| match op.work {
                Work::Offloaded(done) => done,
                _ => None,
            })
            .collect();
        delivered.extend(
            std::mem::take(
                &mut self
                    .inbox
                    .queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .done,
            )
            .into_iter()
            .map(|(_, done)| done),
        );
        delivered
    }

    /// Aborts every pending task, then waits until each one's work is
    /// dropped.
    fn abort_tasks(&self) {
        let tasks = || self.ops.values().filter_map(|op| op.task.as_ref());
        for task in tasks() {
            task.abort();
        }
        for task in tasks() {
            task.ended();
        }
    }
}

impl Drop for Reactor {
    /// The runtime outlives every instance, so an instance's end aborts
    /// the tasks it left.
    fn drop(&mut self) {
        self.abort_tasks();
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use super::{Done, Reactor, Work};
    use crate::host::{Control, Host};

    /// Waits until `due` completes something, and returns it.
    fn next_due(r: &mut Reactor) -> Vec<i32> {
        let mut out = Vec::new();
        loop {
            assert!(r.wait(), "an operation is pending");
            let done = r.due(&mut out);
            if !done.is_empty() {
                return done;
            }
        }
    }

    /// The `.finish` of offloaded operation `h`: the result it delivered.
    fn finish(r: &mut Reactor, h: i32) -> u32 {
        match r.remove(h).map(|op| op.work) {
            Some(Work::Offloaded(Some(Done::Test(n)))) => n,
            _ => panic!("operation {h} has no result"),
        }
    }

    /// An offloaded operation that never completes, and the flag its task
    /// sets when it is dropped, as an abort drops it.
    fn pending_task() -> (impl Future<Output = Done> + Send + 'static, Arc<AtomicBool>) {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        let work = async move {
            let _guard = guard;
            std::future::pending::<Done>().await
        };
        (work, dropped)
    }

    /// Whether `flag` is set within a generous bound: an abort reaches the
    /// task on the `hd-io` thread.
    fn eventually(flag: &AtomicBool) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if flag.load(Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    #[test]
    fn offloaded_ops_complete_out_of_order_with_their_own_results() {
        let mut r = Reactor::new(None);
        let (release, gate) = tokio::sync::oneshot::channel::<()>();
        let slow = r
            .offload(async move {
                let _ = gate.await;
                Done::Test(1)
            })
            .expect("offloads");
        let fast = r.offload(async { Done::Test(2) }).expect("offloads");
        assert_eq!(next_due(&mut r), [fast], "the later op completes first");
        assert!(r.waiting(), "the earlier op is still pending");
        release.send(()).expect("the slow op waits");
        assert_eq!(next_due(&mut r), [slow]);
        assert_eq!(finish(&mut r, fast), 2);
        assert_eq!(finish(&mut r, slow), 1);
        assert!(!r.waiting(), "nothing pending");
    }

    #[test]
    fn an_aborted_ops_late_result_is_dropped() {
        let mut r = Reactor::new(None);
        let h = r.offload(async { Done::Test(1) }).expect("offloads");
        // The task delivers, and the op is aborted before `due` takes it.
        while r.inbox.queue.lock().expect("inbox").done.is_empty() {
            assert!(r.wait());
        }
        assert!(
            matches!(
                r.remove(h).map(|op| op.work),
                Some(Work::Offloaded(Some(Done::Test(1))))
            ),
            "the abort ends the op and hands back its result"
        );
        assert!(!r.waiting());
        let mut out = Vec::new();
        assert!(
            r.due(&mut out).is_empty(),
            "the late result completes nothing"
        );
        assert!(
            r.inbox.queue.lock().expect("inbox").done.is_empty(),
            "the late result is dropped"
        );
        let next = r.offload(async { Done::Test(2) }).expect("offloads");
        assert_eq!(next_due(&mut r), [next]);
        assert_eq!(finish(&mut r, next), 2, "a later op gets its own result");
    }

    #[test]
    fn a_stop_wakes_the_wait_while_an_op_is_pending() {
        let mut r = Reactor::new(None);
        let (work, _) = pending_task();
        let h = r.offload(work).expect("offloads");
        let flag = Arc::new(AtomicBool::new(false));
        let stopper = {
            let (flag, waker) = (Arc::clone(&flag), r.waker());
            // As a session's `Stopper` does, once the wait sleeps.
            std::thread::spawn(move || {
                while !waker.parked() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                flag.store(true, Ordering::Relaxed);
                waker.wake();
            })
        };
        let started = Instant::now();
        while !flag.load(Ordering::Relaxed) {
            assert!(r.wait_until(None, Some(&flag)));
        }
        stopper.join().expect("the stopper ends");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(r.waiting(), "the op is still pending");
        let mut out = Vec::new();
        assert!(r.due(&mut out).is_empty(), "the wake completes nothing");
        r.remove(h);
    }

    #[test]
    fn control_abort_aborts_a_pending_task() {
        let mut host = Host::placeholder();
        let (work, dropped) = pending_task();
        let h = host.reactor.offload(work).expect("offloads");
        assert!(host.control(Control::Abort(h)).is_none());
        assert!(eventually(&dropped), "the task is aborted");
        assert!(!host.waiting(), "nothing pending");
    }

    #[test]
    fn drop_all_aborts_every_pending_task() {
        let mut r = Reactor::new(None);
        let (first, first_dropped) = pending_task();
        let (second, second_dropped) = pending_task();
        r.offload(first).expect("offloads");
        r.offload(second).expect("offloads");
        r.sleep(60_000);
        assert!(r.drop_all().is_empty(), "no task delivered");
        assert!(!r.waiting(), "nothing pending");
        assert!(eventually(&first_dropped) && eventually(&second_dropped));
    }

    #[test]
    fn an_instances_end_aborts_its_tasks() {
        let mut r = Reactor::new(None);
        let (work, dropped) = pending_task();
        r.offload(work).expect("offloads");
        drop(r);
        assert!(eventually(&dropped), "the task is aborted");
    }

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
