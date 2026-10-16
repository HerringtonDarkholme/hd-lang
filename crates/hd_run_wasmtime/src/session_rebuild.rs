//! Rebuilding a session from its journal (`docs/future-work/live-execution.md` §5.4, spec
//! `cli.repl.rebuild`, `.prefix`, `.stopped`; `f1c55020:future-work/compiler/repl-plan.md` job J2).
//!
//! A rebuild starts a new store over the same host: the standard streams,
//! standard input, open sockets and HTTP clients stay, as the world outside
//! the store does. Then it runs the recorded inputs again in order, each
//! built again by the caller (after `:reload`, over the changed package),
//! with every host call and wake served from the record
//! (`hd_run::replay::Replay`): no file, clock, console, network or random
//! call is made twice (`cli.repl.host.once`), and nothing is printed again.
//!
//! - An input that was stopped or cancelled is skipped and reported
//!   (`cli.repl.rebuild.stopped`): it ended partway, so running it again
//!   could not repeat it.
//! - The first input that no longer builds, or that asks the host for what
//!   its record does not hold, or ends otherwise, is dropped with every
//!   later input, and reported (`cli.repl.rebuild.prefix`). One that ran
//!   before it diverged may have changed earlier values, so the rebuild
//!   then starts once more and replays only the kept inputs.
//!
//! The new journal records the replayed inputs again, so after the last
//! one the session goes on live, recording after them.
//!
//! A rebuild first tries to hold the inputs in few instances, since
//! wasmtime frees an instance only with its store (#228). Each run of
//! consecutive inputs that joined the session, the prelude aside, is built
//! as one module (`Rebuilder::build_joined`) and instantiated once; input
//! N runs from its exports `hd.init.N` and `hd.poll.N`, each with its own
//! record. An input that panicked or exited otherwise added nothing for
//! the inputs after it, so it is built alone and ends its run. When an
//! input no longer builds or replays, the rebuild starts again with one
//! module per input, which finds the prefix to keep as described above.

use std::sync::Arc;

use hd_run::Outcome;
use hd_run::journal::{End, Recorder};
use hd_run::replay::{Recorded, RecordedInput, Replay, recorded};
use wasmtime::Linker;

use super::WasmtimeSession;
use crate::host::{Host, define_session};
use crate::instance::WasmtimeInstance;
use crate::new_store;

/// Builds a session's inputs again for a rebuild, which the session cannot
/// do itself: it holds modules, not sources.
pub trait Rebuilder {
    /// Builds input `number` again, over the inputs kept so far: its source
    /// text and module, or the diagnostics that say why it no longer
    /// builds. Input 0 is the session's prelude.
    fn build(&mut self, number: u32) -> Result<(String, Vec<u8>), String>;

    /// Builds inputs `numbers`, consecutive inputs that each joined the
    /// session, again as one module over the inputs kept so far: their
    /// source texts, in order, and the module, whose exports `hd.init.N`
    /// and `hd.poll.N` run input N. `Err` when one no longer builds.
    fn build_joined(&mut self, numbers: &[u32]) -> Result<(Vec<String>, Vec<u8>), String>;

    /// Input `number`, just built, ran again as it first did; `joined` when
    /// it ended with status 0, so later inputs see what it declares. After
    /// `build_joined`, each of its inputs is told once all of them ran.
    fn replayed(&mut self, number: u32, joined: bool);

    /// Forgets every input built so far: the rebuild starts again, from
    /// the prelude.
    fn restart(&mut self);
}

/// What a rebuild kept, skipped and dropped, by input number, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rebuilt {
    /// The inputs that ran again as before.
    pub kept: Vec<u32>,
    /// The stopped or cancelled inputs it skipped
    /// (`cli.repl.rebuild.stopped`).
    pub skipped: Vec<u32>,
    /// The first input that no longer builds or replays, then every later
    /// input (`cli.repl.rebuild.prefix`); empty when all were kept.
    pub dropped: Vec<u32>,
    /// Why the first dropped input was dropped.
    pub reason: Option<String>,
}

impl WasmtimeSession {
    /// Rebuilds the session from its journal; see the module text. `Err`
    /// is a session that cannot be rebuilt: its journal passed its cap, or
    /// a new store could not be made, which leaves the session unusable.
    pub fn rebuild(&mut self, rebuilder: &mut dyn Rebuilder) -> Result<Rebuilt, String> {
        let journal = self.run.store.data().journal.as_ref();
        let (Some(bytes), Some(cap)) = (
            journal.and_then(Recorder::journal),
            journal.map(Recorder::cap),
        ) else {
            return Err(
                "the session's record of host calls is full, so it cannot be rebuilt".into(),
            );
        };
        let record = recorded(bytes)?;
        self.rebuild_record(&record, cap, rebuilder)
    }

    /// Rebuilds the session from another session's `journal`: one the JS
    /// host recorded, which must replay here as the session's own does.
    #[cfg(test)]
    pub(crate) fn rebuild_from(
        &mut self,
        journal: &[u8],
        rebuilder: &mut dyn Rebuilder,
    ) -> Result<Rebuilt, String> {
        let cap = self
            .run
            .store
            .data()
            .journal
            .as_ref()
            .map_or(hd_run::journal::SESSION_CAP, Recorder::cap);
        self.rebuild_record(&recorded(journal)?, cap, rebuilder)
    }

    /// Rebuilds the session from `record`, with a journal of at most `cap`
    /// bytes from then on.
    fn rebuild_record(
        &mut self,
        record: &Recorded,
        cap: usize,
        rebuilder: &mut dyn Rebuilder,
    ) -> Result<Rebuilt, String> {
        if let Some(rebuilt) = self.rebuild_joined(record, cap, rebuilder)? {
            return Ok(rebuilt);
        }
        rebuilder.restart();
        self.restart(Recorder::new(cap, record.execution))?;
        let mut rebuilt = Rebuilt::default();
        // The kept inputs, built, for a second start.
        let mut kept: Vec<(&RecordedInput, String, Vec<u8>)> = Vec::new();
        let mut ran_and_dropped = false;
        for (at, input) in record.inputs.iter().enumerate() {
            if matches!(input.end, Some(End::Stopped | End::Cancelled)) {
                rebuilt.skipped.push(input.number);
                continue;
            }
            let built = rebuilder.build(input.number).and_then(|(source, wasm)| {
                if input.ran(&source) {
                    Ok((source, wasm))
                } else {
                    Err(format!(
                        "input {} is not the text the session ran",
                        input.number
                    ))
                }
            });
            let replayed = built.and_then(|(source, wasm)| {
                let outcome = self.replay_input(input, &source, &wasm);
                ran_and_dropped = outcome.is_err();
                Ok((outcome?, source, wasm))
            });
            match replayed {
                Ok((outcome, source, wasm)) => {
                    rebuilder.replayed(input.number, outcome == Outcome::Exit(0));
                    rebuilt.kept.push(input.number);
                    kept.push((input, source, wasm));
                }
                Err(reason) => {
                    rebuilt.dropped = record.inputs[at..].iter().map(|i| i.number).collect();
                    rebuilt.reason = Some(reason);
                    break;
                }
            }
        }
        if ran_and_dropped {
            self.restart(Recorder::new(cap, record.execution))?;
            for (input, source, wasm) in &kept {
                self.replay_input(input, source, wasm)
                    .map_err(|e| format!("the rebuild did not repeat itself: {e}"))?;
            }
        }
        Ok(rebuilt)
    }

    /// Rebuilds the session from `record` with each run of consecutive
    /// inputs that joined it held in one instance; see the module text.
    /// `None` when an input no longer builds or replays as before.
    fn rebuild_joined(
        &mut self,
        record: &Recorded,
        cap: usize,
        rebuilder: &mut dyn Rebuilder,
    ) -> Result<Option<Rebuilt>, String> {
        self.restart(Recorder::new(cap, record.execution))?;
        let mut rebuilt = Rebuilt::default();
        let mut inputs: Vec<&RecordedInput> = Vec::new();
        for input in &record.inputs {
            if matches!(input.end, Some(End::Stopped | End::Cancelled)) {
                rebuilt.skipped.push(input.number);
            } else {
                inputs.push(input);
            }
        }
        let joins = |i: &&RecordedInput| i.number != 0 && i.end == Some(End::Exit(0));
        let mut at = 0;
        while let Some(&input) = inputs.get(at) {
            let run = inputs[at..].iter().take_while(|i| joins(i)).count();
            if run == 0 {
                let Ok((source, wasm)) = rebuilder.build(input.number) else {
                    return Ok(None);
                };
                if !input.ran(&source) {
                    return Ok(None);
                }
                let Ok(outcome) = self.replay_input(input, &source, &wasm) else {
                    return Ok(None);
                };
                rebuilder.replayed(input.number, outcome == Outcome::Exit(0));
                rebuilt.kept.push(input.number);
                at += 1;
                continue;
            }
            let run = &inputs[at..at + run];
            let numbers: Vec<u32> = run.iter().map(|i| i.number).collect();
            let Ok((sources, wasm)) = rebuilder.build_joined(&numbers) else {
                return Ok(None);
            };
            let same =
                sources.len() == run.len() && run.iter().zip(&sources).all(|(i, s)| i.ran(s));
            if !same || self.replay_joined(run, &sources, &wasm).is_err() {
                return Ok(None);
            }
            for n in numbers {
                rebuilder.replayed(n, true);
                rebuilt.kept.push(n);
            }
            at += run.len();
        }
        Ok(Some(rebuilt))
    }

    /// Runs `inputs` again, consecutive inputs that each joined the
    /// session, of the texts `sources`, from one instance of `wasm`, which
    /// holds them all: input N from its exports `hd.init.N` and
    /// `hd.poll.N`, with its host calls and wakes served from its record
    /// and its output dropped. `Err` is a module the session cannot run,
    /// or a divergence.
    fn replay_joined(
        &mut self,
        inputs: &[&RecordedInput],
        sources: &[String],
        wasm: &[u8],
    ) -> Result<(), String> {
        self.flag.store(false, std::sync::atomic::Ordering::Relaxed);
        let module = self.load(wasm)?;
        define_session(&mut self.linker, &module, self.profile, &mut self.hosts)?;
        self.run.store.data_mut().muted = true;
        let result = self.replay_each(inputs, sources, wasm, &module);
        let host = self.run.store.data_mut();
        host.muted = false;
        host.replay = None;
        result
    }

    /// `replay_joined`'s inputs, in order, then the instance's session
    /// keys added to the linker for later inputs.
    fn replay_each(
        &mut self,
        inputs: &[&RecordedInput],
        sources: &[String],
        wasm: &[u8],
        module: &wasmtime::Module,
    ) -> Result<(), String> {
        let mut instance = None;
        for (input, source) in inputs.iter().zip(sources) {
            let host = self.run.store.data_mut();
            host.replay = Some(Replay::new(input));
            host.reported = false;
            if let Some(journal) = host.journal.as_mut() {
                journal.input(input.number, source.as_bytes(), wasm);
            }
            self.run.halt = None;
            self.run.last_trapped = false;
            let made = match instance {
                Some(made) => Ok(made),
                None => self.instantiate(module)?,
            };
            let result = match made {
                Ok(made) => {
                    instance = Some(made);
                    let n = input.number;
                    self.run_entry(made, &format!("hd.init.{n}"), &format!("hd.poll.{n}"))
                }
                Err(outcome) => Ok(outcome),
            };
            let end = self.ended(&result);
            let mut replay = self
                .run
                .store
                .data_mut()
                .replay
                .take()
                .ok_or("the replay ended before its input")?;
            replay.end(end, input.end)?;
            if result? != Outcome::Exit(0) {
                return Err(format!("input {} did not end as before", input.number));
            }
        }
        match instance {
            Some(instance) => self.add(instance),
            None => Ok(()),
        }
    }

    /// Runs `input` again, of the text `source` built as `wasm`, with its
    /// host calls and wakes served from its record and its output dropped.
    /// `Err` is a divergence: it asked for what the record does not hold,
    /// or ended otherwise.
    fn replay_input(
        &mut self,
        input: &RecordedInput,
        source: &str,
        wasm: &[u8],
    ) -> Result<Outcome, String> {
        let host = self.run.store.data_mut();
        host.replay = Some(Replay::new(input));
        host.muted = true;
        let (result, end) = self.run_ended(input.number, source, wasm);
        let host = self.run.store.data_mut();
        host.muted = false;
        let mut replay = host
            .replay
            .take()
            .ok_or("the replay ended before its input")?;
        let Some(end) = end else {
            let error = result.err().unwrap_or_default();
            return Err(format!("input {} cannot run: {error}", input.number));
        };
        replay.end(end, input.end)?;
        // An input that failed to run before failed again.
        Ok(result.unwrap_or_else(Outcome::Internal))
    }

    /// Replaces the store with a new one over the same host, whose journal
    /// starts again as `journal`, and forgets every input.
    fn restart(&mut self, journal: Recorder) -> Result<(), String> {
        let engine = Arc::clone(self.run.engine());
        let mut host = std::mem::replace(self.run.store.data_mut(), Host::placeholder());
        host.restart(journal);
        let store = new_store(&engine, host).map_err(|e| match e {
            hd_run::StartError::Refused(m) | hd_run::StartError::Engine(m) => m,
        })?;
        let mut run = WasmtimeInstance::stopped(store, Arc::clone(&engine));
        run.close_on_end = false;
        self.run = run;
        self.linker = Linker::new(&engine);
        self.hosts.clear();
        self.memory = false;
        self.runtime.clear();
        self.instances = 0;
        self.ran = 0;
        Ok(())
    }
}
