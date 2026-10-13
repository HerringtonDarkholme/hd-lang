use std::sync::Arc;
use std::task::Poll;

use hd_run::journal::End;
use hd_run::{Instance, Outcome, StartError};
use wasmtime::{Store, Trap, TypedFunc, WasmBacktrace};

use crate::host::{Control, Host, Stop, complete, deadlock, deliver_batch};
use crate::limits;

/// A running program.
pub struct WasmtimeInstance {
    pub(crate) store: Store<Host>,
    /// Keep the ticker's weak registration alive for the entire instance.
    /// Store owns Wasmtime's internal Engine clone, not this outer Arc; an
    /// embedding may drop `WasmtimeEngine` while its instance is still running.
    engine: Arc<wasmtime::Engine>,
    init: Option<TypedFunc<(), ()>>,
    instance: Option<wasmtime::Instance>,
    poll: Option<TypedFunc<i32, i32>>,
    wake: Option<TypedFunc<i32, ()>>,
    /// `hd.cancel`, which only a session's input exports: a stop request
    /// that arrives while the input waits on the host cancels its root
    /// suspension through it (`cli.repl.cancel`).
    cancel: Option<TypedFunc<(), ()>>,
    /// How the run ended, once it has.
    pub(crate) done: Option<Outcome>,
    /// A property keeps successful instances and their host state between bodies.
    pub(crate) close_on_end: bool,
    pub(crate) last_trapped: bool,
    /// The last Wasm trap's frames, innermost first: function index and
    /// module byte offset, which a test report symbolizes
    /// (wasm-layout.md §15.9).
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
            init: None,
            instance: None,
            poll: None,
            wake: None,
            cancel: None,
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
        let cancel = match instance.get_func(&mut *store, "hd.cancel") {
            Some(f) => Some(
                f.typed::<(), ()>(&*store)
                    .map_err(|e| StartError::Engine(format!("`hd.cancel`: {e}")))?,
            ),
            None => None,
        };
        self.init = Some(init);
        self.instance = Some(instance);
        self.poll = Some(poll);
        self.wake = wake;
        self.cancel = cancel;
        self.done = None;
        self.last_trapped = false;
        self.last_frames.clear();
        self.halt = None;
        Ok(())
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
        let o = self
            .store
            .data_mut()
            .control(event)
            .expect("a run termination returns its outcome");
        self.done = Some(o.clone());
        o
    }

    /// Reports a trap as the Node host does and ends the run with status 3.
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
        let host = self.store.data_mut();
        host.flush();
        if limits::is_heap_exhausted(e) {
            host.err("panic: heap-exhausted: the program heap cannot allocate more memory\n");
        } else if let Some(stop) = e.downcast_ref::<Stop>() {
            match stop {
                Stop::Contract(m) => host.err(&format!("panic: host-contract: {m}\n")),
                Stop::HeapExhausted => {
                    host.err("panic: heap-exhausted: the exchange buffer cannot grow\n");
                }
                Stop::TimeLimit => {
                    let report = host.time_limit_report();
                    host.err(&report);
                }
                Stop::Interrupted => {
                    host.err("stopped\n");
                    self.halt = Some(End::Stopped);
                }
                Stop::Internal(m) => host.err(&format!("internal error: {m}\n")),
                Stop::Deadlock | Stop::Diverged => {}
                Stop::EndCase => unreachable!("handled before panic reporting"),
            }
        } else if let Some(Trap::StackOverflow) = e.downcast_ref::<Trap>() {
            host.err(
                "panic: stack-exhausted: the call stack ran out; check for recursion that never ends\n",
            );
        } else if !host.reported {
            let text = match e.downcast_ref::<Trap>() {
                Some(t) => trap_text(*t),
                None => format!("{e:#}"),
            };
            host.err(&format!("internal error: {text}\n"));
        }
        self.finish(Control::Trap {
            close: self.close_on_end,
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

    /// One `hd.poll` as the run's driver, context 0: the status,
    /// pending, or the end of a trap.
    fn step(&mut self) -> Poll<Outcome> {
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
                // A replayed input waits when its record wakes it next.
                let host = self.store.data();
                let waiting = host
                    .replay
                    .as_ref()
                    .map_or_else(|| host.waiting(), hd_run::replay::Replay::wake_next);
                if waiting {
                    Poll::Pending
                } else {
                    let e = deadlock(self.store.data_mut());
                    Poll::Ready(self.trapped(&e))
                }
            }
            Ok(status) => match self.store.data().check_deadline() {
                Ok(()) => Poll::Ready(self.end(status)),
                Err(error) => Poll::Ready(self.trapped(&error)),
            },
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
                // The run is over; the next poll returns how.
                Err(e) => {
                    self.interrupted(&e);
                    return vec![0];
                }
            }
            match self.store.data().wait() {
                Ok(true) => {}
                Ok(false) => return Vec::new(),
                Err(error) => {
                    self.interrupted(&error);
                    return vec![0];
                }
            }
        }
    }
}
