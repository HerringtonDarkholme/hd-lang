use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use hd_run::{Grants, Outcome};
use wasmtime::Memory;

use super::{Control, Stop};
use crate::reactor::Reactor;
use crate::{http, net};

/// Where a run's standard streams go: the process's own, or buffers a
/// caller reads afterwards.
pub struct HostIo {
    pub out: Box<dyn Write + Send>,
    pub err: Box<dyn Write + Send>,
    /// Standard input, or `None` when it is closed (end of input at once).
    pub input: Option<Box<dyn BufRead + Send>>,
}

impl HostIo {
    /// The process's standard output, error and input.
    #[must_use]
    pub fn process() -> HostIo {
        HostIo {
            out: Box::new(std::io::stdout()),
            err: Box::new(std::io::stderr()),
            input: Some(Box::new(std::io::BufReader::new(std::io::stdin()))),
        }
    }
}

/// The per-instance host state of the store.
pub struct Host {
    out_stream: Box<dyn Write + Send>,
    err_stream: Box<dyn Write + Send>,
    /// Standard output not yet written.
    pub(crate) out: Vec<u8>,
    pub memory: Option<Memory>,
    /// Whether a panic report or any standard error was written, so a trap
    /// is not also an internal error.
    pub reported: bool,
    pub(crate) reactor: Reactor,
    pub(crate) args: Vec<String>,
    pub(crate) program: String,
    pub(crate) grants: Grants,
    pub(crate) noticed: HashSet<String>,
    pub(crate) http: http::Client,
    pub(crate) net: net::Client,
    pub(crate) heap: crate::limits::HeapBudget,
    deadline: Option<Instant>,
    pub(crate) fixture: crate::profiles::State,
    pub(crate) testing: Option<crate::testing::State>,
    pub(crate) environment: Option<crate::test_engine::TestEnvironment>,
    base_deadline: Option<Instant>,
    /// A session's stop flag: set from another thread, read at every
    /// deadline check and epoch tick.
    pub(crate) interrupt: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) memory_bytes: u64,
    timeout_millis: Option<i64>,
    pub(crate) output_overflow: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) output_budget: Option<(std::sync::Arc<std::sync::atomic::AtomicUsize>, usize)>,
}

impl Host {
    #[must_use]
    pub fn new(
        io: HostIo,
        args: Vec<String>,
        program: String,
        grants: Grants,
        wall_ms: Option<u64>,
        memory_bytes: u64,
    ) -> Host {
        let deadline = wall_ms.and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
        Host {
            out_stream: io.out,
            err_stream: io.err,
            out: Vec::new(),
            memory: None,
            reported: false,
            reactor: Reactor::new(io.input),
            args,
            program,
            grants,
            noticed: HashSet::new(),
            http: http::Client::default(),
            net: net::Client::default(),
            heap: crate::limits::HeapBudget::new(memory_bytes),
            deadline,
            fixture: crate::profiles::State::default(),
            testing: None,
            environment: None,
            base_deadline: deadline,
            interrupt: None,
            memory_bytes,
            timeout_millis: None,
            output_overflow: None,
            output_budget: None,
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(crate) fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
        self.base_deadline = deadline;
    }

    pub(crate) fn begin_test_body(&mut self) {
        self.deadline = self.base_deadline;
        self.timeout_millis = None;
    }

    pub(crate) fn time_limit_report(&self) -> String {
        match self.timeout_millis {
            Some(millis) => format!(
                "panic: time-limit: the test case ran longer than its timeout of {millis}ms\n"
            ),
            None => "panic: time-limit: the program exceeded its time limit\n".into(),
        }
    }

    pub(crate) fn report_timeout(&mut self, millis: i64) {
        let millis = u64::try_from(millis).unwrap_or(0);
        let reported = Instant::now().checked_add(Duration::from_millis(millis));
        self.timeout_millis = reported
            .filter(|reported| self.base_deadline.is_none_or(|base| *reported <= base))
            .map(|_| i64::try_from(millis).unwrap_or(i64::MAX));
        self.deadline = match (self.base_deadline, reported) {
            (Some(base), Some(reported)) => Some(base.min(reported)),
            (base, reported) => base.or(reported),
        };
    }

    pub fn check_deadline(&self) -> wasmtime::Result<()> {
        if self
            .interrupt
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
        {
            return Err(Stop::Interrupted.into());
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(Stop::TimeLimit.into());
        }
        // Console stdout is held until a flush. Count it before it reaches the
        // capture sink as well, otherwise an endless write-only child could
        // evade the shared stdout/stderr cap.
        if self.output_budget.as_ref().is_some_and(|(count, limit)| {
            count
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_add(self.out.len())
                > *limit
        }) && let Some(flag) = &self.output_overflow
        {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if self
            .output_overflow
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
        {
            return Err(Stop::Internal("process output exceeds the host limit".into()).into());
        }
        Ok(())
    }

    /// The one boundary for operation abort and completed-run cleanup.
    /// Reusable test instances retain providers only when close is false.
    pub(crate) fn control(&mut self, event: Control) -> Option<Outcome> {
        let (status, close) = match event {
            Control::Abort(handle) => {
                self.reactor.remove(handle);
                return None;
            }
            Control::Exit { status, close } => (status, close),
            Control::Trap { close } => (3, close),
        };
        self.flush();
        if close {
            self.close();
        }
        Some(Outcome::Exit(u8::try_from(status & 0xff).unwrap_or(1)))
    }

    /// Releases sockets and HTTP worker guards even for a retained instance.
    fn close(&mut self) {
        self.net.stop();
        self.http = http::Client::default();
    }

    /// Writes the held standard output.
    pub fn flush(&mut self) {
        if !self.out.is_empty() {
            let _ = self.out_stream.write_all(&self.out);
            let _ = self.out_stream.flush();
            self.out.clear();
        }
    }

    /// Writes `text` to standard error, after the held output.
    pub fn err(&mut self, text: &str) {
        self.flush();
        let _ = self.err_stream.write_all(text.as_bytes());
        let _ = self.err_stream.flush();
    }

    /// Whether an operation is pending and not completed: something can
    /// still wake the program.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.reactor.waiting()
    }

    /// Waits for the next completion; `false` when nothing is pending.
    pub fn wait(&self) -> wasmtime::Result<bool> {
        self.check_deadline()?;
        let waiting = self.reactor.wait_until(self.deadline);
        self.check_deadline()?;
        Ok(waiting)
    }
}
