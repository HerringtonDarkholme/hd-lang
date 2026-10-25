//! A REPL session on wasmtime (spec `cli.repl.*`; `docs/future-work/live-execution.md`
//! §4 and §5).
//!
//! A session holds no instance from one input to the next. Its state is
//! its host (the standard streams, standard input, open sockets and HTTP
//! clients, which outlive an input) and its journal: the record of every
//! input that ran and of each host call it made (`hd_run::journal`).
//!
//! The caller builds one module that holds every input the session keeps
//! and the new one (`hd_wasm_linear::Entry::Inputs`: `hd.init.N` and
//! `hd.poll.N` run input N). `WasmtimeSession::run` instantiates it once
//! in a new store and runs the journal's inputs again in order, with each
//! host call and wake served from the record (`hd_run::replay::Replay`)
//! and their output dropped, so no host call is made twice
//! (`cli.repl.host.once`) and nothing is printed again; that is a rebuild
//! by `cli.repl.rebuild`. Then it runs the new input live, recording it.
//!
//! - An input that was stopped or cancelled is skipped and reported
//!   (`cli.repl.rebuild.stopped`): it ended partway, so running it again
//!   could not repeat it.
//! - The first input whose text is not the one it ran, or that asks the
//!   host for what its record does not hold, or ends otherwise, is
//!   dropped with every later input, and reported
//!   (`cli.repl.rebuild.prefix`). One that ran before it diverged may have
//!   changed earlier values, so the session then replays the kept inputs
//!   once more in a new store. The new input then runs only when every
//!   dropped input is one the caller left out of the program, as a
//!   rebuild that no longer builds it does.
//! - After an input that did not end with status 0, the session drops the
//!   host operations it left pending and calls `hd.reset`, which clears
//!   the runtime state a trap leaves set, so the next input starts as the
//!   first did (`cli.repl.panic`). What it changed in earlier inputs'
//!   values stays (`cli.repl.panic.mutations`).
//!
//! A `Stopper` interrupts the input that runs. While Wasm runs, the input
//! traps at the next epoch tick or host call and is `stopped`
//! (`cli.repl.stop`). While it waits on the host, the wait wakes at once
//! and the session calls `hd.cancel`, which cancels its root suspension by
//! `req.cancel.steps`: its `defer` suites run and it is `cancelled`
//! (`cli.repl.cancel`). Either way it then ends as a panicking input does.
//!
//! Past its cap, the journal stops recording; the session then cannot run
//! another input, and says so once (`take_journal_notice`).

use hd_base::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hd_run::journal::{End, Recorder, SESSION_CAP};
use hd_run::replay::{Recorded, RecordedInput, Replay, recorded};
use hd_run::{HostSetup, Instance, Outcome, StartError};
use wasmtime::Linker;

use crate::cache;
use crate::engine::{EngineConfig, WasmtimeEngine, new_store};
use crate::host::{Host, HostIo, define_session, supported, unsupported_message};
use crate::instance::WasmtimeInstance;
use crate::profiles::RuntimeProfile;
use crate::runtime::reactor::Waker;

/// A REPL session: its host and its journal. Like the `Engine` methods,
/// it runs on a thread with at least `thread_stack` of stack.
pub struct WasmtimeSession {
    /// The store, and the input that runs or ran last.
    run: WasmtimeInstance,
    /// The host imports of the module that runs.
    linker: Linker<Host>,
    /// The host imports `linker` defines, as (module, name).
    hosts: HashSet<(String, String)>,
    config: EngineConfig,
    profile: RuntimeProfile,
    /// Set by a `Stopper`; the store's epoch callback, every host call and
    /// the host's wait read it.
    flag: Arc<AtomicBool>,
}

/// What one [`WasmtimeSession::run`] did, by input number, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    /// The inputs that ran again as before.
    pub kept: Vec<u32>,
    /// The stopped or cancelled inputs it skipped
    /// (`cli.repl.rebuild.stopped`).
    pub skipped: Vec<u32>,
    /// The first input that no longer replays, then every later input
    /// (`cli.repl.rebuild.prefix`); empty when all were kept.
    pub dropped: Vec<u32>,
    /// Why the first dropped input was dropped.
    pub reason: Option<String>,
    /// How the new input ended: its outcome, as `hd run` answers it, and
    /// its journal `End`. `None` when no new input was asked for, or an
    /// input was dropped, so it did not run.
    pub live: Option<(Outcome, End)>,
}

/// Interrupts the input a `WasmtimeSession` is running. It is `Send +
/// Sync` and cheap to clone, for a Ctrl-C handler on another thread. An
/// input that computes traps at the next epoch tick (about ten
/// milliseconds) or host call, reported as `stopped` (`cli.repl.stop`).
/// An input that waits on the host is cancelled at once, its `defer`
/// suites run, reported as `cancelled` (`cli.repl.cancel`). Either way it
/// ends as a panicking input does. A stop while no input runs does
/// nothing: the flag clears when the next input starts.
#[derive(Clone)]
pub struct Stopper {
    flag: Arc<AtomicBool>,
    waker: Waker,
}

impl Stopper {
    pub fn stop(&self) {
        self.flag.store(true, Ordering::Relaxed);
        self.waker.wake();
    }

    /// Whether the running input waits on the host now, so a stop cancels
    /// it rather than stopping it.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.waker.parked()
    }
}

impl WasmtimeEngine {
    /// A session with no input yet, its standard streams on `io`.
    pub fn session(&self, setup: &HostSetup, io: HostIo) -> Result<WasmtimeSession, StartError> {
        let mut host = Host::new(
            io,
            setup.args.clone(),
            setup.program.clone(),
            setup.grants.clone(),
            setup.limits.wall_ms,
        );
        host.pending_writes = self.profile == RuntimeProfile::PendingWrite;
        let flag = Arc::new(AtomicBool::new(false));
        host.interrupt = Some(Arc::clone(&flag));
        let execution = crate::sys::random(16)
            .ok()
            .and_then(|b| <[u8; 16]>::try_from(b).ok())
            .map_or(0, u128::from_le_bytes);
        host.journal = Some(Recorder::new(SESSION_CAP, execution));
        let mut run =
            WasmtimeInstance::stopped(new_store(&self.engine, host)?, Arc::clone(&self.engine));
        // Sockets and HTTP clients outlive an input: a later input may use
        // a handle an earlier one opened.
        run.close_on_end = false;
        Ok(WasmtimeSession {
            run,
            linker: Linker::new(&self.engine),
            hosts: HashSet::default(),
            config: self.config,
            profile: self.profile,
            flag,
        })
    }
}

impl WasmtimeSession {
    /// Runs the session program `wasm` (see the module text): replays the
    /// journal's inputs, each of the text `sources` gives for its number,
    /// then runs input `live`, also of its text in `sources`, when one is
    /// asked for. The new input's standard output is written before this
    /// returns; a panic is reported on standard error. `Err` is a session
    /// that cannot run: its journal passed its cap, the module is one
    /// wasmtime refuses or imports what wasmtime does not have, or a new
    /// store could not be made.
    pub fn run(
        &mut self,
        wasm: &[u8],
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<Ran, String> {
        let journal = self.run.store.data().journal.as_ref();
        let (Some(bytes), Some(cap)) = (
            journal.and_then(Recorder::journal),
            journal.map(Recorder::cap),
        ) else {
            return Err(
                "the session's record of host calls is full, so it cannot run another input".into(),
            );
        };
        let record = recorded(bytes)?;
        self.run_record(&record, cap, wasm, sources, live)
    }

    /// Runs as `run` does over another session's `journal`: one the JS
    /// host recorded, which must replay here as the session's own does.
    #[cfg(test)]
    pub(crate) fn run_from(
        &mut self,
        journal: &[u8],
        wasm: &[u8],
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<Ran, String> {
        let cap = self
            .run
            .store
            .data()
            .journal
            .as_ref()
            .map_or(SESSION_CAP, Recorder::cap);
        self.run_record(&recorded(journal)?, cap, wasm, sources, live)
    }

    fn run_record(
        &mut self,
        record: &Recorded,
        cap: usize,
        wasm: &[u8],
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<Ran, String> {
        let source = |n: u32| sources.iter().find(|(k, _)| *k == n).map(|(_, s)| *s);
        // A stop that came while no input ran is dropped.
        self.flag.store(false, Ordering::Relaxed);
        let module = self.load(wasm)?;
        let mut ran = Ran::default();
        self.restart(Recorder::new(cap, record.execution))?;
        let instance = self.instantiate(&module)?;
        // The kept inputs, for a second start.
        let mut kept: Vec<(&RecordedInput, &str)> = Vec::new();
        for (at, input) in record.inputs.iter().enumerate() {
            if matches!(input.end, Some(End::Stopped | End::Cancelled)) {
                ran.skipped.push(input.number);
                continue;
            }
            let replayed = match source(input.number) {
                Some(text) if input.ran(text) => {
                    self.replay(instance, input, text, wasm).map(|()| text)
                }
                Some(_) => Err(format!(
                    "input {} is not the text the session ran",
                    input.number
                )),
                None => Err(format!("input {} is not in the session", input.number)),
            };
            match replayed {
                Ok(text) => {
                    ran.kept.push(input.number);
                    kept.push((input, text));
                }
                Err(reason) => {
                    ran.dropped = record.inputs[at..].iter().map(|i| i.number).collect();
                    ran.reason = Some(reason);
                    break;
                }
            }
        }
        let mut instance = instance;
        if !ran.dropped.is_empty() {
            self.restart(Recorder::new(cap, record.execution))?;
            instance = self.instantiate(&module)?;
            for (input, text) in kept {
                self.replay(instance, input, text, wasm)
                    .map_err(|e| format!("the rebuild did not repeat itself: {e}"))?;
            }
            // An input left out of `sources` is one the caller forgot: the
            // program does not hold it, so the new input still runs. One
            // that diverged may have declared what the new input uses.
            if ran.dropped.iter().any(|n| source(*n).is_some()) {
                return Ok(ran);
            }
        }
        if let Some(n) = live {
            let text = source(n).ok_or_else(|| format!("input {n} has no text"))?;
            ran.live = Some(self.input(instance, n, text, wasm)?);
        }
        Ok(ran)
    }

    /// Runs input `input` again from `instance`, with its host calls and
    /// wakes served from its record and its output dropped. `Err` is a
    /// divergence: it asked for what the record does not hold, or ended
    /// otherwise.
    fn replay(
        &mut self,
        instance: wasmtime::Instance,
        input: &RecordedInput,
        source: &str,
        wasm: &[u8],
    ) -> Result<(), String> {
        let host = self.run.store.data_mut();
        host.replay = Some(Replay::new(input));
        host.muted = true;
        // A stop is for the new input, the one the user sees run: it waits
        // until the replay is done.
        let interrupt = host.interrupt.take();
        let result = self.input(instance, input.number, source, wasm);
        let host = self.run.store.data_mut();
        host.muted = false;
        host.interrupt = interrupt;
        let mut replay = host
            .replay
            .take()
            .ok_or("the replay ended before its input")?;
        let (_, end) = result?;
        replay.end(end, input.end)
    }

    /// Runs input `number` of the text `source` from `instance`'s exports
    /// `hd.init.N` and `hd.poll.N`, recording it, and answers how it ended
    /// and its `End` row. After one that did not end with status 0, it
    /// resets the instance for the next.
    fn input(
        &mut self,
        instance: wasmtime::Instance,
        number: u32,
        source: &str,
        wasm: &[u8],
    ) -> Result<(Outcome, End), String> {
        let host = self.run.store.data_mut();
        // A panic or standard error of an earlier input does not hide this
        // one's internal error.
        host.reported = false;
        if let Some(journal) = host.journal.as_mut() {
            journal.input(number, source.as_bytes(), wasm);
        }
        self.run.halt = None;
        self.run.last_trapped = false;
        let result = self.run_entry(
            instance,
            &format!("hd.init.{number}"),
            &format!("hd.poll.{number}"),
        );
        let end = self.ended(&result);
        // The stop that ended a new input, if one did, is spent before its
        // reset runs.
        if self.run.store.data().replay.is_none() {
            self.flag.store(false, Ordering::Relaxed);
        }
        // An input that failed to run is an outcome too.
        let outcome = result.unwrap_or_else(Outcome::Internal);
        if outcome != Outcome::Exit(0) {
            self.reset(instance)?;
        }
        Ok((outcome, end))
    }

    /// How the input that ran ended, as its `End` row says, which this
    /// writes.
    fn ended(&mut self, result: &Result<Outcome, String>) -> End {
        let end = match (result, self.run.halt) {
            (Ok(_), Some(halt)) => halt,
            (Ok(Outcome::Exit(_)), None) if self.run.last_trapped => End::Trapped,
            (Ok(Outcome::Exit(status)), None) => End::Exit(*status),
            (Ok(Outcome::Panic(_)), None) => End::Trapped,
            (Ok(Outcome::Internal(_)) | Err(_), _) => End::Failed,
        };
        if let Some(journal) = self.run.store.data_mut().journal.as_mut() {
            journal.end(end);
        }
        end
    }

    /// Instantiates the session program, whose host imports this defines.
    /// `Err` is a module the session cannot instantiate.
    fn instantiate(&mut self, module: &wasmtime::Module) -> Result<wasmtime::Instance, String> {
        define_session(&mut self.linker, module, self.profile, &mut self.hosts)?;
        self.linker
            .instantiate(&mut self.run.store, module)
            .map_err(|error| format!("{error:#}"))
    }

    /// Runs an input from `instance`'s entry exports `init` and `poll`, as
    /// a program's entry runs, and answers how it ended. A panic is
    /// reported on standard error and ends it with status 3.
    fn run_entry(
        &mut self,
        instance: wasmtime::Instance,
        init: &str,
        poll: &str,
    ) -> Result<Outcome, String> {
        self.run.enter(instance, init, poll).map_err(|e| match e {
            StartError::Refused(m) | StartError::Engine(m) => m,
        })?;
        let outcome = match self.run.init() {
            Outcome::Exit(0) => hd_run::drive(&mut self.run, usize::MAX),
            other => other,
        };
        let outcome = match outcome {
            Outcome::Panic(report) => {
                let host = self.run.store.data_mut();
                host.err(&format!("panic: {}: {}\n", report.category, report.message));
                self.run.last_trapped = true;
                Outcome::Exit(3)
            }
            other => other,
        };
        self.run.store.data_mut().drain_output();
        Ok(outcome)
    }

    /// The session's journal so far, or `None` once recording stopped at
    /// its cap.
    #[must_use]
    pub fn journal(&self) -> Option<&[u8]> {
        self.run.store.data().journal.as_ref()?.journal()
    }

    /// Changes the journal's cap, `hd_run::journal::SESSION_CAP` until
    /// then; a journal already past it stops at its next row.
    pub fn set_journal_cap(&mut self, cap: usize) {
        if let Some(journal) = self.run.store.data_mut().journal.as_mut() {
            journal.set_cap(cap);
        }
    }

    /// True once, after the input in which the journal passed its cap and
    /// recording stopped: the REPL then says that the session can run no
    /// further input.
    pub fn take_journal_notice(&mut self) -> bool {
        self.run
            .store
            .data_mut()
            .journal
            .as_mut()
            .is_some_and(Recorder::take_notice)
    }

    /// The host calls the session made, not served from a record.
    #[cfg(test)]
    pub(crate) fn host_calls(&self) -> u64 {
        self.run.store.data().calls
    }

    /// Whether a host operation is pending in the session's reactor.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool {
        self.run.store.data().waiting()
    }

    /// A handle that stops or cancels the running input from another
    /// thread.
    #[must_use]
    pub fn stopper(&self) -> Stopper {
        Stopper {
            flag: Arc::clone(&self.flag),
            waker: self.run.store.data().reactor.waker(),
        }
    }

    /// Compiles the session program, or names the first import that does
    /// not run on wasmtime yet.
    fn load(&self, wasm: &[u8]) -> Result<Arc<wasmtime::Module>, String> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err("not a Wasm module".into());
        }
        let imports = hd_run::module_imports(wasm)?;
        if let Some((m, n)) = imports.iter().find(|(m, n)| !supported(m, n, self.profile)) {
            return Err(unsupported_message(m, n));
        }
        let shared = cache::ondemand(&self.config)?;
        shared
            .cache
            .get_or_compile(&shared.engine, wasm)
            .map_err(|e| format!("{e:#}"))
    }

    /// After an input that did not end with status 0: drops the host
    /// operations it left pending, a panicking or stopped input's or a
    /// timer a cancel did not abort, so none wakes a later input or hides
    /// its deadlock. Inputs run one at a time and one that ends with
    /// status 0 has finished every operation it started, so each one left
    /// is this input's. Then `hd.reset` clears the runtime state a trap
    /// leaves set.
    fn reset(&mut self, instance: wasmtime::Instance) -> Result<(), String> {
        let store = &mut self.run.store;
        store.data_mut().drop_pending();
        let Some(reset) = instance.get_func(&mut *store, "hd.reset") else {
            return Ok(());
        };
        reset
            .typed::<(), ()>(&*store)
            .and_then(|f| f.call(&mut *store, ()))
            .map_err(|e| format!("`hd.reset`: {e:#}"))
    }

    /// Replaces the store with a new one over the same host, whose journal
    /// starts again as `journal`.
    fn restart(&mut self, journal: Recorder) -> Result<(), String> {
        let engine = Arc::clone(self.run.engine());
        let mut host = std::mem::replace(self.run.store.data_mut(), Host::placeholder());
        host.restart(journal);
        let store = new_store(&engine, host).map_err(|e| match e {
            StartError::Refused(m) | StartError::Engine(m) => m,
        })?;
        let mut run = WasmtimeInstance::stopped(store, Arc::clone(&engine));
        run.close_on_end = false;
        self.run = run;
        self.linker = Linker::new(&engine);
        self.hosts.clear();
        Ok(())
    }
}
