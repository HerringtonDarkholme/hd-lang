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

use std::sync::Arc;

use hd_run::Outcome;
use hd_run::journal::{End, Recorder};
use hd_run::replay::{Recorded, RecordedInput, Replay, recorded};
use wasmtime::Linker;

use super::WasmtimeSession;
use crate::host::Host;
use crate::instance::WasmtimeInstance;
use crate::new_store;

/// Builds a session's inputs again for a rebuild, which the session cannot
/// do itself: it holds modules, not sources.
pub trait Rebuilder {
    /// Builds input `number` again, over the inputs kept so far: its source
    /// text and module, or the diagnostics that say why it no longer
    /// builds. Input 0 is the session's prelude.
    fn build(&mut self, number: u32) -> Result<(String, Vec<u8>), String>;

    /// Input `number`, just built, ran again as it first did; `joined` when
    /// it ended with status 0, so later inputs see what it declares.
    fn replayed(&mut self, number: u32, joined: bool);
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
        Ok(())
    }
}
