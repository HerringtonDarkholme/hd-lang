use hd_base::HashSet;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use hd_run::{Grants, Outcome};
use wasmtime::Memory;

use super::{Control, Stop};
use crate::net;
use crate::runtime::reactor::{Done, Reactor, Work};

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

/// The most standard output held in memory between flushes: output past
/// this many bytes is written at once, though the program has not waited.
pub(crate) const HELD_OUTPUT_BOUND: usize = 64 * 1024;

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
    /// The panic report being written or written, after its `panic: `
    /// prefix: what a test runner reads the category and message of
    /// (`take_panic`), never the standard error text.
    panic: Option<String>,
    pub(crate) reactor: Reactor,
    pub(crate) args: Vec<String>,
    pub(crate) program: String,
    pub(crate) grants: Grants,
    /// The handles of the granted directories that file operations
    /// resolve under (`fs::sandbox`).
    pub(crate) sandbox: crate::fs::Sandbox,
    pub(crate) noticed: HashSet<String>,
    pub(crate) net: net::Client,
    deadline: Option<Instant>,
    pub(crate) fixture: crate::profiles::State,
    pub(crate) testing: Option<crate::testing::State>,
    pub(crate) environment: Option<crate::test_engine::TestEnvironment>,
    base_deadline: Option<Instant>,
    /// A session's stop flag: set from another thread, read at every
    /// deadline check and epoch tick.
    pub(crate) interrupt: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    timeout_millis: Option<i64>,
    /// A REPL session's journal (`docs/future-work/live-execution.md` §4), which every host
    /// call and driver event is recorded in; `None` for every other run.
    pub(crate) journal: Option<hd_run::journal::Recorder>,
    /// While a session's rebuild replays an input: the record its host
    /// calls and wakes are served from, instead of being made again
    /// (`cli.repl.host.once`).
    pub(crate) replay: Option<hd_run::replay::Replay>,
    /// Whether standard output and error are dropped: a replayed input's
    /// output was shown when it first ran.
    pub(crate) muted: bool,
    /// The host calls made, not served from a replay's record, but for the
    /// `hd:prim` functions: what a session's tests count.
    pub(crate) calls: u64,
}

impl Host {
    #[must_use]
    pub fn new(
        io: HostIo,
        args: Vec<String>,
        program: String,
        grants: Grants,
        wall_ms: Option<u64>,
    ) -> Host {
        let deadline = wall_ms.and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
        Host {
            out_stream: io.out,
            err_stream: io.err,
            out: Vec::new(),
            memory: None,
            reported: false,
            panic: None,
            reactor: Reactor::new(io.input),
            args,
            program,
            grants,
            sandbox: crate::fs::Sandbox::default(),
            noticed: HashSet::default(),
            net: net::Client::default(),
            deadline,
            fixture: crate::profiles::State::default(),
            testing: None,
            environment: None,
            base_deadline: deadline,
            interrupt: None,
            timeout_millis: None,
            journal: None,
            replay: None,
            muted: false,
            calls: 0,
        }
    }

    /// A host with no streams, arguments or grant, which holds a store's
    /// place while its host moves to another store.
    pub(crate) fn placeholder() -> Host {
        let io = HostIo {
            out: Box::new(std::io::sink()),
            err: Box::new(std::io::sink()),
            input: None,
        };
        Host::new(io, Vec::new(), String::new(), Grants::default(), None)
    }

    /// Readies this host for a new store, as a session's rebuild makes one:
    /// the store's memory and heap are new, nothing is pending, and the
    /// journal starts again as `journal`. The streams, standard input and open
    /// sockets stay: they are the world outside the store.
    pub(crate) fn restart(&mut self, journal: hd_run::journal::Recorder) {
        self.flush();
        self.memory = None;
        self.reported = false;
        self.panic = None;
        self.drop_pending();
        self.journal = Some(journal);
        self.replay = None;
        self.muted = false;
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

    /// Reports the `time-limit` panic.
    pub(crate) fn report_time_limit(&mut self) {
        let message = match self.timeout_millis {
            Some(millis) => format!("the test case ran longer than its timeout of {millis}ms"),
            None => "the program exceeded its time limit".into(),
        };
        self.report_panic("time-limit", &message);
    }

    /// The guest's `hd:rt` `panic` import: writes `panic: ` and records the
    /// standard error that follows until the trap as the report.
    pub(crate) fn begin_panic(&mut self) {
        self.reported = true;
        self.err("panic: ");
        self.panic = Some(String::new());
    }

    /// Writes the host's own panic report, `panic: CATEGORY: MESSAGE`.
    pub(crate) fn report_panic(&mut self, category: &str, message: &str) {
        let report = format!("{category}: {message}\n");
        self.reported = true;
        self.err(&format!("panic: {report}"));
        self.panic = Some(report);
    }

    /// Writes `text`, a guest's standard error, which a panic report being
    /// written also records.
    pub(crate) fn guest_err(&mut self, text: &str) {
        self.reported = true;
        self.err(text);
        if let Some(report) = &mut self.panic {
            report.push_str(text);
        }
    }

    /// The category and message of the panic reported, if one was, and
    /// forgets it.
    pub(crate) fn take_panic(&mut self) -> Option<crate::testing::PanicInfo> {
        self.panic
            .take()
            .map(|report| crate::testing::PanicInfo::from_report(&report))
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
        Ok(())
    }

    /// The one boundary for operation abort and completed-run cleanup.
    /// Reusable test instances retain providers only when close is false.
    pub(crate) fn control(&mut self, event: Control) -> Option<Outcome> {
        let (status, close) = match event {
            Control::Abort(handle) => {
                if let Some(Work::Offloaded(Some(Done::Net(done)))) =
                    self.reactor.remove(handle).map(|op| op.work)
                {
                    self.net.unread(done);
                }
                return None;
            }
            Control::Exit { status, close } => (status, close),
            // A trapped run's pending operations can never finish: end
            // them, so a child it started is killed before the run ends.
            Control::Trap { close } => {
                self.drop_pending();
                (3, close)
            }
        };
        self.flush();
        if close {
            self.close();
        }
        Some(Outcome::Exit(u8::try_from(status & 0xff).unwrap_or(1)))
    }

    /// Ends every pending operation (`Reactor::drop_all`). What completed
    /// reads and receives took stays for the next call on their sockets,
    /// which outlive a session's input (`std-net.cancel.read`).
    pub(crate) fn drop_pending(&mut self) {
        for done in self.reactor.drop_all() {
            if let Done::Net(done) = done {
                self.net.unread(done);
            }
        }
    }

    /// Releases sockets even for a retained instance.
    fn close(&mut self) {
        self.net.stop();
    }

    /// Writes the held standard output once it passes `HELD_OUTPUT_BOUND`, so
    /// a program that prints without ever waiting cannot hold it all.
    pub(crate) fn flush_over_bound(&mut self) {
        if self.out.len() > HELD_OUTPUT_BOUND {
            self.flush();
        }
    }

    /// Writes the held standard output.
    pub fn flush(&mut self) {
        if self.muted {
            self.out.clear();
        } else if !self.out.is_empty() {
            let _ = self.out_stream.write_all(&self.out);
            let _ = self.out_stream.flush();
            self.out.clear();
        }
    }

    /// Writes `text` to standard error, after the held output.
    pub fn err(&mut self, text: &str) {
        self.flush();
        if self.muted {
            return;
        }
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
    ///
    /// Held output reaches its stream first (`std-console.host.visible`),
    /// so every park of the host, here, shows what the program printed.
    pub fn wait(&mut self) -> wasmtime::Result<bool> {
        self.check_deadline()?;
        self.flush();
        let waiting = self
            .reactor
            .wait_until(self.deadline, self.interrupt.as_deref());
        self.check_deadline()?;
        Ok(waiting)
    }
}
