use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Instant;

use hd_run::journal::End;
use hd_run::{Instance, Outcome, StartError};
const CANCELLED_END: i32 = -2;
use wasmtime::{Store, Trap, TypedFunc, WasmBacktrace};

use crate::cancel::{CancelStep, HostCancel};
use crate::host::{Control, Host, Stop, complete, deadlock, deliver_batch};
use crate::limits;

/// The status of a run whose root ended cancelled, or that a request
/// stopped while Wasm ran (`cli.cancel.status`), as the Node host's
/// `CANCEL_STATUS`.
pub(crate) const CANCEL_STATUS: i32 = 130;

/// A running program.
pub struct WasmtimeInstance {
    pub(crate) store: Store<Host>,
    /// Keep the ticker's weak registration alive for the entire instance.
    /// Store owns Wasmtime's internal Engine clone, not this outer Arc; an
    /// embedding may drop `WasmtimeEngine` while its instance is still running.
    engine: Arc<wasmtime::Engine>,
    pub(crate) report: Option<crate::checked_report::Report>,
    init: Option<TypedFunc<(), ()>>,
    instance: Option<wasmtime::Instance>,
    poll: Option<TypedFunc<i32, i32>>,
    wake: Option<TypedFunc<i32, ()>>,
    /// `hd.cancel`, when the module links it: a stop request that arrives
    /// while the input waits on the host cancels its root suspension
    /// through it (`cli.repl.cancel`); scenario builds link it to cancel
    /// the entry instead.
    cancel: Option<TypedFunc<(), ()>>,
    /// `hd.abandon`, when the module links it: the host abandons the root
    /// through it (`req.entry.cancel.abandon`).
    abandon: Option<TypedFunc<(), ()>>,
    /// A run the host may cancel (`cancel.rs`): its cleanup bound and how
    /// far the cancel went. A stop request then cancels a waiting root
    /// instead of ending the run as a REPL input's does.
    pub(crate) host_cancel: Option<HostCancel>,
    /// Whether the root's last poll was pending, so a request has a root
    /// to cancel.
    root_pending: bool,
    /// Set when the next wake carries no completion, only the poll that a
    /// cancel request or a passed bound asks for.
    repoll: bool,
    /// How the root ended cancelled, when it did: whether a part of its
    /// cleanup was cut short.
    pub(crate) cancelled: Option<bool>,
    /// A scenario's `hd.scenario.cleanup`, when the module links it.
    cleanup: Option<TypedFunc<(), i32>>,
    /// How the run ended, once it has.
    pub(crate) done: Option<Outcome>,
    /// A property keeps successful instances and their host state between bodies.
    pub(crate) close_on_end: bool,
    pub(crate) last_trapped: bool,
    /// The last Wasm trap's frames, innermost first: function index and
    /// module byte offset, which a test report symbolizes
    /// (`crates/hd_wasm_linear/DESIGN.md` §15.9).
    pub(crate) last_frames: Vec<(u32, u32)>,
    /// Whether a session's stop ended the run: `Stopped` while Wasm ran,
    /// `Cancelled` while it waited. For the input's `End` row.
    pub(crate) halt: Option<End>,
}

impl WasmtimeInstance {
    /// The engine the store belongs to.
    pub(crate) fn engine(&self) -> &Arc<wasmtime::Engine> {
        &self.engine
    }

    pub(crate) fn stopped(store: Store<Host>, engine: Arc<wasmtime::Engine>) -> Self {
        Self {
            store,
            engine,
            report: None,
            init: None,
            instance: None,
            poll: None,
            wake: None,
            cancel: None,
            abandon: None,
            host_cancel: None,
            root_pending: false,
            repoll: false,
            cancelled: None,
            cleanup: None,
            done: None,
            close_on_end: true,
            last_trapped: false,
            last_frames: Vec::new(),
            halt: None,
        }
    }

    pub(crate) fn from_instance(
        store: Store<Host>,
        engine: Arc<wasmtime::Engine>,
        instance: wasmtime::Instance,
        init_name: &str,
        poll_name: &str,
    ) -> Result<Self, StartError> {
        let mut this = Self::stopped(store, engine);
        this.enter(instance, init_name, poll_name)?;
        Ok(this)
    }

    /// Makes `instance`, in this store, the one that `init`, `poll` and
    /// `wake` drive, through its exports `init_name`, `poll_name` and
    /// `hd.wake`; the run it starts has not ended. A session enters each
    /// input this way.
    pub(crate) fn enter(
        &mut self,
        instance: wasmtime::Instance,
        init_name: &str,
        poll_name: &str,
    ) -> Result<(), StartError> {
        let store = &mut self.store;
        store.data_mut().memory = instance.get_memory(&mut *store, "hd.x");
        // No facts body runs yet, whatever an earlier input left.
        store.data_mut().facts.clear();
        let export = |store: &mut Store<Host>, name: &str| {
            instance
                .get_func(&mut *store, name)
                .ok_or_else(|| StartError::Engine(format!("the module has no export `{name}`")))
        };
        let init = export(store, init_name)?
            .typed::<(), ()>(&*store)
            .map_err(|e| StartError::Engine(format!("`{init_name}`: {e}")))?;
        let poll = export(store, poll_name)?
            .typed::<i32, i32>(&*store)
            .map_err(|e| StartError::Engine(format!("`{poll_name}`: {e}")))?;
        let wake = match instance.get_func(&mut *store, "hd.wake") {
            Some(f) => Some(
                f.typed::<i32, ()>(&*store)
                    .map_err(|e| StartError::Engine(format!("`hd.wake`: {e}")))?,
            ),
            None => None,
        };
        self.init = Some(init);
        self.instance = Some(instance);
        self.poll = Some(poll);
        self.wake = wake;
        // A scenario's further exports, when the module links them: only
        // scenario builds carry them, so their absence is not an error.
        self.cancel = optional_typed(store, instance, "hd.cancel")?;
        self.abandon = optional_typed(store, instance, "hd.abandon")?;
        self.cleanup = optional_typed(store, instance, "hd.scenario.cleanup")?;
        self.root_pending = false;
        self.repoll = false;
        self.cancelled = None;
        self.done = None;
        self.last_trapped = false;
        self.last_frames.clear();
        self.halt = None;
        Ok(())
    }
}

/// An optional export, when the module links it: only a suspending entry
/// carries `hd.cancel` and `hd.abandon`, and only a scenario build
/// `hd.scenario.cleanup`, so absence is not an error here.
fn optional_typed<Params, Results>(
    store: &mut Store<Host>,
    instance: wasmtime::Instance,
    name: &str,
) -> Result<Option<TypedFunc<Params, Results>>, StartError>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    instance
        .get_func(&mut *store, name)
        .map(|f| {
            f.typed(&*store)
                .map_err(|e| StartError::Engine(format!("`{name}`: {e}")))
        })
        .transpose()
}

impl WasmtimeInstance {
    /// One `hd.poll` with the driver's own context: the status, or the
    /// trap that ended the drive.
    /// The value of the module's exported `i32` global `name`, if it
    /// exports one.
    pub(crate) fn global_i32(&mut self, name: &str) -> Option<i32> {
        let g = self.instance?.get_global(&mut self.store, name)?;
        g.get(&mut self.store).i32()
    }

    pub(crate) fn poll_with(&mut self, ctx: i32) -> Result<i32, wasmtime::Error> {
        let status = self
            .poll
            .as_ref()
            .expect("a scenario module has hd.poll")
            .call(&mut self.store, ctx);
        self.root_pending = matches!(status, Ok(-1));
        status
    }

    /// One `hd.scenario.cleanup`: the cleanup result, or the end of a trap.
    pub(crate) fn cleanup_call(&mut self) -> Result<i32, wasmtime::Error> {
        self.cleanup
            .as_ref()
            .expect("a scenario module has hd.scenario.cleanup")
            .call(&mut self.store, ())
    }
}

/// V8's text of a trap, which the Node host prints as an internal error.
fn trap_text(trap: Trap) -> String {
    match trap {
        Trap::UnreachableCodeReached => "unreachable",
        Trap::MemoryOutOfBounds => "memory access out of bounds",
        Trap::IntegerDivisionByZero => "divide by zero",
        Trap::IntegerOverflow => "divide result unrepresentable",
        Trap::BadConversionToInteger => "float unrepresentable in integer range",
        Trap::NullReference => "dereferencing a null pointer",
        Trap::CastFailure => "illegal cast",
        Trap::ArrayOutOfBounds => "array element access out of bounds",
        Trap::AllocationTooLarge => "requested new array is too large",
        other => return other.to_string(),
    }
    .to_owned()
}

impl WasmtimeInstance {
    /// Ends the run with status `status`, writing the held output.
    fn end(&mut self, status: i32) -> Outcome {
        self.finish(Control::Exit {
            status,
            close: self.close_on_end,
        })
    }

    fn finish(&mut self, event: Control) -> Outcome {
        if let Some(report) = &self.report
            && let Some(instance) = self.instance
            && let Some(text) = report.summary(instance, &mut self.store)
        {
            self.store.data_mut().err(&text);
        }
        let o = self
            .store
            .data_mut()
            .control(event)
            .expect("a run termination returns its outcome");
        self.done = Some(o.clone());
        o
    }

    /// Reports a trap as the Node host does and ends the run with status 3,
    /// or with 130 for a run the host may cancel that a request stopped
    /// while Wasm ran (`cli.cancel.computing`).
    pub(crate) fn trapped(&mut self, e: &wasmtime::Error) -> Outcome {
        if matches!(e.downcast_ref::<Stop>(), Some(Stop::EndCase)) {
            return self.end(0);
        }
        self.last_trapped = true;
        // A guest trap has frames; a host stop (a time limit, a broken host
        // contract, an exhausted heap) has none, as on the Node host.
        self.last_frames = match (e.downcast_ref::<Trap>(), e.downcast_ref::<WasmBacktrace>()) {
            (Some(_), Some(trace)) if !limits::is_heap_exhausted(e) => trace
                .frames()
                .iter()
                .filter_map(|f| Some((f.func_index(), u32::try_from(f.module_offset()?).ok()?)))
                .collect(),
            _ => Vec::new(),
        };
        let stopped = self.host_cancel.is_some()
            && matches!(e.downcast_ref::<Stop>(), Some(Stop::Interrupted));
        let host = self.store.data_mut();
        host.flush();
        if limits::is_heap_exhausted(e) {
            host.report_panic(
                "heap-exhausted",
                "the program heap cannot allocate more memory",
            );
        } else if let Some(stop) = e.downcast_ref::<Stop>() {
            match stop {
                Stop::Contract(m) => host.report_panic("host-contract", m),
                Stop::HeapExhausted => {
                    host.report_panic("heap-exhausted", "the exchange buffer cannot grow");
                }
                Stop::TimeLimit => host.report_time_limit(),
                Stop::Interrupted => {
                    host.err("stopped\n");
                    self.halt = Some(End::Stopped);
                }
                Stop::Internal(m) => host.err(&format!("internal error: {m}\n")),
                Stop::Deadlock | Stop::Diverged => {}
                Stop::EndCase => unreachable!("handled before panic reporting"),
            }
        } else if let Some(Trap::StackOverflow) = e.downcast_ref::<Trap>() {
            host.report_panic(
                "stack-exhausted",
                "the call stack ran out; check for recursion that never ends",
            );
        } else if !host.reported {
            let text = match e.downcast_ref::<Trap>() {
                Some(t) => trap_text(*t),
                None => format!("{e:#}"),
            };
            host.err(&format!("internal error: {text}\n"));
        }
        let close = self.close_on_end;
        self.finish(if stopped {
            Control::Stop { close }
        } else {
            Control::Trap { close }
        })
    }

    /// Ends the run after `error` from the host wait, where the root
    /// suspension is pending and no Wasm runs. A session's stop request
    /// then cancels the root (`cli.repl.cancel`), so its `defer` suites
    /// run, and the input ends as a panicking input does, reported as
    /// `cancelled`. Any other error, or a stop of a module without
    /// `hd.cancel`, is a trap.
    fn interrupted(&mut self, error: &wasmtime::Error) -> Outcome {
        let Some(cancel) = self
            .cancel
            .clone()
            .filter(|_| matches!(error.downcast_ref::<Stop>(), Some(Stop::Interrupted)))
        else {
            return self.trapped(error);
        };
        // The request is spent on this cancel: the `defer` suites it runs
        // may call the host. Another request during them stops them.
        if let Some(flag) = &self.store.data().interrupt {
            flag.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(journal) = self.store.data_mut().journal.as_mut() {
            journal.cancel();
        }
        self.halt = Some(End::Cancelled);
        if let Err(error) = cancel.call(&mut self.store, ()) {
            return self.trapped(&error);
        }
        self.last_trapped = true;
        self.last_frames.clear();
        self.store.data_mut().err("cancelled\n");
        self.finish(Control::Trap {
            close: self.close_on_end,
        })
    }

    /// The wake a replayed input's record holds where it waits now; a
    /// record that holds none there is a divergence, which ends the run.
    fn replayed_wake(&mut self) -> Vec<u32> {
        let host = self.store.data_mut();
        let woken = host
            .check_deadline()
            .and_then(|()| match host.replay.as_mut() {
                Some(replay) => replay.wake().map_err(|_| Stop::Diverged.into()),
                None => Err(Stop::Internal("a wake served with no replay".into()).into()),
            });
        match woken {
            Ok(handles) => handles,
            // The run is over; the next poll returns how.
            Err(error) => {
                self.trapped(&error);
                vec![0]
            }
        }
    }

    /// Acts on a cancel request that finds the root pending, or on a
    /// cleanup bound that has passed (`cancel.rs`): the request is taken
    /// from the stop flag, and `hd.cancel` or `hd.abandon` runs. Its error
    /// is the trap of that call.
    fn host_cancel_step(&mut self) -> wasmtime::Result<()> {
        if !self.root_pending {
            return Ok(());
        }
        let Some(cancel) = self.host_cancel.as_mut() else {
            return Ok(());
        };
        let asked = self
            .store
            .data()
            .interrupt
            .as_ref()
            .is_some_and(|flag| flag.swap(false, Ordering::Relaxed));
        let func = match cancel.next(asked, Instant::now()) {
            Some(CancelStep::Cancel) => self.cancel.clone(),
            Some(CancelStep::Abandon) => self.abandon.clone(),
            None => return Ok(()),
        };
        match func {
            Some(func) => func.call(&mut self.store, ()),
            None => Ok(()),
        }
    }

    /// Whether `error`, from a host wait, is a cancel request this run
    /// acts on rather than the end of the run.
    fn cancel_request(&self, error: &wasmtime::Error) -> bool {
        self.host_cancel.is_some()
            && self.root_pending
            && matches!(error.downcast_ref::<Stop>(), Some(Stop::Interrupted))
    }

    /// The deadline the host waits for while it bounds a cleanup.
    fn cleanup_deadline(&self) -> Option<Instant> {
        self.host_cancel.as_ref().and_then(HostCancel::deadline)
    }

    /// The batch that asks the driver for a poll with no completion:
    /// `wake` delivers nothing for it, and the poll takes the request.
    fn repoll_now(&mut self) -> Vec<u32> {
        self.repoll = true;
        vec![0]
    }

    /// One `hd.poll` as the run's driver, context 0: the status,
    /// pending, or the end of a trap. A root that ended cancelled ends the
    /// run with `cancelled` on standard error and status 130
    /// (`cli.cancel.end`).
    fn step(&mut self) -> Poll<Outcome> {
        if let Err(error) = self.host_cancel_step() {
            return Poll::Ready(self.trapped(&error));
        }
        if let Err(error) = self.store.data().check_deadline() {
            return Poll::Ready(self.trapped(&error));
        }
        let host = self.store.data_mut();
        if let Some(journal) = host.journal.as_mut() {
            journal.poll();
        }
        if let Some(replay) = host.replay.as_mut() {
            replay.poll();
        }
        match self
            .poll
            .as_ref()
            .expect("a live instance has hd.poll")
            .call(&mut self.store, 0_i32)
        {
            Ok(-1) => {
                self.root_pending = true;
                // A replayed input waits when its record wakes it next. A
                // bounded cleanup waits for its deadline at least.
                let host = self.store.data();
                let waiting = host
                    .replay
                    .as_ref()
                    .map_or_else(|| host.waiting(), hd_run::replay::Replay::wake_next)
                    || self.cleanup_deadline().is_some();
                if waiting {
                    Poll::Pending
                } else {
                    let e = deadlock(self.store.data_mut());
                    Poll::Ready(self.trapped(&e))
                }
            }
            Ok(status) => {
                self.root_pending = false;
                if let Err(error) = self.store.data().check_deadline() {
                    return Poll::Ready(self.trapped(&error));
                }
                if status == CANCELLED_END || status == CANCELLED_END - 1 {
                    let abandoned = status != CANCELLED_END;
                    self.cancelled = Some(abandoned);
                    self.store.data_mut().err(if abandoned {
                        "cancelled, cleanup abandoned\n"
                    } else {
                        "cancelled\n"
                    });
                    return Poll::Ready(self.end(CANCEL_STATUS));
                }
                Poll::Ready(self.end(status))
            }
            Err(e) => Poll::Ready(self.trapped(&e)),
        }
    }
}

impl Instance for WasmtimeInstance {
    fn init(&mut self) -> Outcome {
        if let Some(outcome) = &self.done {
            return outcome.clone();
        }
        if let Err(error) = self.store.data().check_deadline() {
            return self.trapped(&error);
        }
        match self
            .init
            .as_ref()
            .expect("a live instance has hd.init")
            .call(&mut self.store, ())
        {
            Ok(()) => match self.store.data().check_deadline() {
                Ok(()) => Outcome::Exit(0),
                Err(error) => self.trapped(&error),
            },
            Err(e) => self.trapped(&e),
        }
    }
    fn poll(&mut self) -> Poll<Outcome> {
        if let Some(o) = &self.done {
            return Poll::Ready(o.clone());
        }
        self.step()
    }
    fn wake(&mut self, handles: &[u32]) {
        if std::mem::take(&mut self.repoll) {
            return;
        }
        let Some(wake) = self.wake.clone().filter(|_| self.done.is_none()) else {
            return;
        };
        if let Some(journal) = self.store.data_mut().journal.as_mut() {
            journal.wake(handles);
        }
        let n = match deliver_batch(&mut self.store, handles) {
            Ok(count) => count,
            Err(error) => {
                self.trapped(&error);
                return;
            }
        };
        if let Err(e) = wake.call(&mut self.store, n) {
            self.trapped(&e);
        }
    }
    fn call_test(&mut self, export: u32) -> Poll<Outcome> {
        if let Some(outcome) = &self.done {
            return Poll::Ready(outcome.clone());
        }
        let name = format!("hd.test.{export}");
        let Some(instance) = self.instance else {
            return Poll::Ready(Outcome::Internal("the instance has no live exports".into()));
        };
        let poll = match instance.get_typed_func::<i32, i32>(&mut self.store, &name) {
            Ok(poll) => poll,
            Err(error) => return Poll::Ready(Outcome::Internal(format!("`{name}`: {error}"))),
        };
        self.poll = Some(poll);
        self.step()
    }
    /// Waits for at least one completion. The returned batch is delivered
    /// only by wake; no exchange-buffer preload is required or performed.
    fn completed(&mut self) -> Vec<u32> {
        if self.store.data().replay.is_some() {
            return self.replayed_wake();
        }
        loop {
            match complete(&mut self.store) {
                Ok(handles) if handles.is_empty() => {}
                Ok(handles) => return handles,
                Err(e) if self.cancel_request(&e) => return self.repoll_now(),
                // The run is over; the next poll returns how.
                Err(e) => {
                    self.interrupted(&e);
                    return vec![0];
                }
            }
            let bound = self.cleanup_deadline();
            if bound.is_some_and(|d| Instant::now() >= d) {
                return self.repoll_now();
            }
            match self.store.data_mut().wait_bounded(bound) {
                Ok(true) => {}
                Ok(false) => return Vec::new(),
                Err(error) if self.cancel_request(&error) => return self.repoll_now(),
                Err(error) => {
                    self.interrupted(&error);
                    return vec![0];
                }
            }
        }
    }
}
