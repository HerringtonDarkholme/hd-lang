//! A REPL session on wasmtime (spec `cli.repl.*`; `docs/future-work/live-execution.md`
//! §4 and §5).
//!
//! A linked session retains initialized code epochs in one Store. Its
//! first module owns the memories, table and allocator; later modules
//! import the earlier epochs and run only their new input. A completed
//! input that takes the active journal past its threshold captures the
//! memories and numeric globals and archives the historical call rows.
//! Restore reconstructs the same code epochs over the original provider,
//! restores that image and replays only the active tail.
//!
//! Historical reload compiles a fresh linked base containing the kept
//! source prefix, replays its recorded effects once without live calls,
//! and adopts the initialized instance for the next extension. Ordinary
//! whole-history modules retain the replay path for existing embedders.
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
//! The journal threshold bounds the active tail in linked sessions;
//! archived host results remain available for historical reload. The old
//! whole-history API still stops recording if its journal exceeds its cap.

use hd_base::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

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

/// Private compiler/host ABI, absent from ordinary programs.
#[derive(Clone)]
struct SessionInfo {
    epoch: u32,
    data_bytes: u32,
    table_len: u32,
    relocations: Vec<u32>,
}

fn leb(bytes: &[u8], at: &mut usize) -> Result<usize, String> {
    let mut value = 0_usize;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*at).ok_or("truncated session metadata")?;
        *at += 1;
        if shift == 28 && byte > 15 {
            return Err("session metadata length overflows u32".into());
        }
        value |= usize::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
    }
    Err("session metadata length overflows u32".into())
}

impl SessionInfo {
    fn read(wasm: &[u8]) -> Result<Option<Self>, String> {
        if wasm.get(..8) != Some(b"\0asm\x01\0\0\0") {
            return Err("not a Wasm module".into());
        }
        let mut at = 8;
        let mut info = None;
        while at < wasm.len() {
            let kind = wasm[at];
            at += 1;
            let length = leb(wasm, &mut at)?;
            let end = at.checked_add(length).ok_or("session section overflow")?;
            let body = wasm.get(at..end).ok_or("truncated Wasm section")?;
            at = end;
            if kind != 0 {
                continue;
            }
            let mut start = 0;
            let length = leb(body, &mut start)?;
            let end = start.checked_add(length).ok_or("session name overflow")?;
            if body.get(start..end) != Some(b"hd.session") {
                continue;
            }
            if info.is_some() {
                return Err("duplicate session manifests".into());
            }
            let data = &body[end..];
            if data.len() < 20 || !data.len().is_multiple_of(4) {
                return Err("truncated session manifest".into());
            }
            let words: Vec<_> = data
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")))
                .collect();
            if words[0] != 1 || words[4] as usize != words.len() - 5 {
                return Err("unsupported session manifest".into());
            }
            if words[5..]
                .iter()
                .any(|offset| offset.checked_add(4).is_none_or(|end| end > words[2]))
            {
                return Err("session relocation is outside its static segment".into());
            }
            info = Some(Self {
                epoch: words[1],
                data_bytes: words[2],
                table_len: words[3],
                relocations: words[5..].to_vec(),
            });
        }
        Ok(info)
    }
}

struct LinkedModule {
    module: Arc<wasmtime::Module>,
    wasm: Vec<u8>,
    info: SessionInfo,
    number: u32,
    source: String,
    data: u32,
}

struct Checkpoint {
    memories: Vec<(&'static str, Vec<u8>)>,
    globals: Vec<(usize, String, wasmtime::Val)>,
    epochs: usize,
    table_len: u64,
}

struct Linked {
    instances: Vec<wasmtime::Instance>,
    modules: Vec<LinkedModule>,
    /// Completed source inputs, including every input in a rebuilt base.
    sources: Vec<(u32, String)>,
    checkpoint: Option<Checkpoint>,
    archives: Vec<Vec<u8>>,
    history: OnceLock<Vec<u8>>,
    execution: u128,
    cap: usize,
    cancelled: Option<u32>,
    /// Retains recoverable evidence if a restore fails after replacing the
    /// Store. Its old instance handles must never be used in the new Store.
    restore_failed: Option<Vec<u8>>,
}

const MEMORIES: [&str; 3] = ["hd.x", "hd.session.heap", "hd.session.stack"];

fn session_import(module: &str) -> bool {
    module == "hd:session" || module.starts_with("hd:session:")
}

// Reindex archived chunks without changing their recorded request/reply
// payloads. A chunk's method dictionary starts again from zero.
fn join_journals(chunks: &[&[u8]]) -> Result<Vec<u8>, String> {
    use hd_run::journal::{self, Event, RowKind};
    let mut out = Vec::new();
    let mut execution = None;
    let mut methods = HashMap::default();
    let mut next = 0;
    let mut last = None;
    for bytes in chunks {
        let record = recorded(bytes)?;
        if execution.is_some_and(|id| id != record.execution) {
            return Err("archived journals belong to different executions".into());
        }
        if record.inputs.iter().any(|input| input.end.is_none()) {
            return Err("an archived journal has an unfinished input".into());
        }
        let (rows, valid) = journal::read(bytes);
        if valid != bytes.len() {
            return Err("an archived journal is cut short".into());
        }
        let events = journal::decode(&rows)?;
        let mut local_methods = 0;
        for (mut row, event) in rows.into_iter().zip(events) {
            if row.kind == RowKind::Header {
                if execution.is_some() {
                    continue;
                }
                execution = Some(record.execution);
            }
            if let Event::Input { number, .. } = event {
                if last.is_some_and(|last| last >= number) {
                    return Err("archived inputs are not in increasing order".into());
                }
                last = Some(number);
            }
            if let Event::Call { module, name, .. } = event {
                let mut at = 0;
                let method = leb(&row.payload, &mut at)?;
                if method == local_methods {
                    local_methods += 1;
                    for _ in 0..2 {
                        let length = leb(&row.payload, &mut at)?;
                        at += length;
                    }
                }
                let key = (module, name);
                let mut payload = Vec::new();
                let index = methods.len();
                let known = methods.get(&key).copied();
                journal::put_uleb(&mut payload, known.unwrap_or(index) as u64);
                if known.is_none() {
                    for text in [&key.0, &key.1] {
                        journal::put_uleb(&mut payload, text.len() as u64);
                        payload.extend_from_slice(text.as_bytes());
                    }
                    methods.insert(key, index);
                }
                payload.extend_from_slice(&row.payload[at..]);
                row.payload = payload;
            }
            row.index = next;
            next += 1;
            journal::append(&mut out, &row);
        }
    }
    Ok(out)
}

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
    linked: Option<Linked>,
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
            linked: None,
        })
    }
}

impl WasmtimeSession {
    /// Runs `wasm` against initialized linked storage, or replays history
    /// into a whole-session module. `sources` supplies each input's text;
    /// `live` selects a new input to run after restoration or replay. Its
    /// standard output is written before this returns; a panic is reported
    /// on standard error. `Err` means the module or session cannot run,
    /// including an invalid linking ABI or an unavailable store or import.
    pub fn run(
        &mut self,
        wasm: &[u8],
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<Ran, String> {
        if let Some(info) = SessionInfo::read(wasm)? {
            let same_code = self
                .linked
                .as_ref()
                .is_some_and(|linked| linked.modules.last().is_some_and(|part| part.wasm == wasm));
            if info.epoch == 0 && live.is_none() && !same_code {
                let module = self.load(wasm)?;
                if sources
                    .iter()
                    .all(|(number, _)| module.get_export(&format!("hd.init.{number}")).is_some())
                {
                    return self.rebuild_linked(wasm, info, sources);
                }
            }
            return self.run_linked(wasm, info, sources, live);
        }
        if self.linked.is_some() {
            let bytes = self.journal().ok_or("the session has no journal")?;
            let record = recorded(bytes)?;
            self.linked = None;
            return self
                .run_record(&record, usize::MAX, wasm, sources, live)
                .map(|(ran, _)| ran);
        }
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
            .map(|(ran, _)| ran)
    }

    /// Replays historical evidence into a fresh linked base, then adopts
    /// that initialized instance. Later inputs import it without replay.
    fn rebuild_linked(
        &mut self,
        wasm: &[u8],
        info: SessionInfo,
        sources: &[(u32, &str)],
    ) -> Result<Ran, String> {
        let cap = self.linked.as_ref().map_or_else(
            || {
                self.run
                    .store
                    .data()
                    .journal
                    .as_ref()
                    .map_or(SESSION_CAP, Recorder::cap)
            },
            |linked| linked.cap,
        );
        let record = recorded(
            self.journal()
                .ok_or("the session has no historical journal")?,
        )?;
        let module = self.load(wasm)?;
        self.linked = None;
        let (ran, instance) = self.run_record(&record, usize::MAX, wasm, sources, None)?;
        // A divergent suffix must first be removed from the compiler's
        // program. Its declared ABI is not yet the initialized prefix.
        if !ran.dropped.is_empty() || ran.kept.is_empty() {
            return Ok(ran);
        }
        let inputs: Vec<_> = ran
            .kept
            .iter()
            .map(|number| {
                let text = sources
                    .iter()
                    .find(|(n, _)| n == number)
                    .expect("replayed source")
                    .1;
                (*number, text.to_owned())
            })
            .collect();
        let (number, source) = inputs.last().expect("kept input").clone();
        let bytes = self
            .run
            .store
            .data()
            .journal
            .as_ref()
            .and_then(Recorder::journal)
            .ok_or("the rebuilt journal is missing")?
            .to_vec();
        let mut linked = Linked {
            instances: vec![instance],
            modules: vec![LinkedModule {
                module,
                wasm: wasm.to_vec(),
                info,
                number,
                source,
                data: 16,
            }],
            sources: inputs,
            checkpoint: None,
            archives: vec![bytes],
            history: OnceLock::new(),
            execution: record.execution,
            cap,
            cancelled: None,
            restore_failed: None,
        };
        linked.checkpoint = Some(self.capture_linked(&linked)?);
        self.run.store.data_mut().journal = Some(Recorder::new(usize::MAX, record.execution));
        self.linked = Some(linked);
        Ok(ran)
    }

    fn run_linked(
        &mut self,
        wasm: &[u8],
        info: SessionInfo,
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<Ran, String> {
        self.flag.store(false, Ordering::Relaxed);
        let mut linked = if let Some(linked) = self.linked.take() {
            linked
        } else {
            if live.is_none() {
                return Err("restoring linked storage needs its original provider session".into());
            }
            if info.epoch != 0 {
                return Err("a linked session must start with its base module".into());
            }
            let recorder = self.run.store.data().journal.as_ref().ok_or("no journal")?;
            let cap = recorder.cap();
            let execution = recorded(recorder.journal().ok_or("the journal is full")?)?.execution;
            self.run
                .store
                .data_mut()
                .journal
                .as_mut()
                .expect("journal")
                .set_cap(usize::MAX);
            Linked {
                instances: Vec::new(),
                modules: Vec::new(),
                sources: Vec::new(),
                checkpoint: None,
                archives: Vec::new(),
                history: OnceLock::new(),
                cap,
                execution,
                cancelled: None,
                restore_failed: None,
            }
        };
        let result = (|| {
            if linked.restore_failed.is_some() {
                return Err(
                    "the linked restore failed; rebuild from its historical journal".into(),
                );
            }
            for (number, previous) in &linked.sources {
                if sources.iter().find(|(n, _)| n == number).map(|(_, s)| *s)
                    != Some(previous.as_str())
                {
                    return Err(format!("input {number} needs a session rebuild"));
                }
            }
            let mut ran = Ran {
                kept: linked.sources.iter().map(|(number, _)| *number).collect(),
                skipped: linked.cancelled.into_iter().collect(),
                ..Ran::default()
            };
            if live.is_none() {
                if linked.modules.last().is_none_or(|part| part.wasm != wasm) {
                    return Err("changed session code needs a historical rebuild".into());
                }
                self.restore_linked(&mut linked)?;
                return Ok(ran);
            }
            if linked.cancelled.is_some() {
                self.restore_linked(&mut linked)?;
            }
            let number = live.expect("live input");
            let source = sources
                .iter()
                .find(|(n, _)| *n == number)
                .map(|(_, text)| *text)
                .ok_or_else(|| format!("input {number} has no text"))?;
            if info.epoch as usize != linked.instances.len() {
                return Err("the module is not an extension of this initialized session".into());
            }
            let mut part = LinkedModule {
                module: self.load(wasm)?,
                wasm: wasm.to_vec(),
                info,
                number,
                source: source.to_owned(),
                data: 0,
            };
            // A stop is for the input, not the linked module setup before
            // it. In particular, `hd.session.alloc` may call `rt:alloc`;
            // an interrupt there used to escape as an input setup error.
            let interrupt = self.run.store.data_mut().interrupt.take();
            let instantiated = self.instantiate_linked(&part, &mut linked.instances, None);
            self.run.store.data_mut().interrupt = interrupt;
            self.flag.store(false, Ordering::Relaxed);
            part.data = instantiated?;
            linked.history.take();
            let instance = *linked.instances.last().expect("new instance");
            let outcome = self.input(instance, number, source, wasm)?;
            let cancelled = matches!(outcome.1, End::Stopped | End::Cancelled);
            ran.live = Some(outcome);
            if cancelled {
                linked.cancelled = Some(number);
                return Ok(ran);
            }
            linked.sources.push((number, source.to_owned()));
            linked.modules.push(part);
            let journal = self
                .run
                .store
                .data()
                .journal
                .as_ref()
                .and_then(Recorder::journal)
                .ok_or("the linked journal is full")?;
            if journal.len() > linked.cap {
                let archive = journal.to_vec();
                linked.checkpoint = Some(self.capture_linked(&linked)?);
                linked.archives.push(archive);
                self.run.store.data_mut().journal =
                    Some(Recorder::new(usize::MAX, linked.execution));
            }
            Ok(ran)
        })();
        self.linked = Some(linked);
        result
    }

    fn instantiate_linked(
        &mut self,
        part: &LinkedModule,
        instances: &mut Vec<wasmtime::Instance>,
        checkpoint: Option<&Checkpoint>,
    ) -> Result<u32, String> {
        let mut data = 0_u32;
        if part.info.epoch > 0 {
            let base = *instances.first().ok_or("missing session base")?;
            let store = &mut self.run.store;
            if checkpoint.is_some() {
                data = part.data;
            } else {
                let bytes = part
                    .info
                    .data_bytes
                    .checked_add(8)
                    .ok_or("session data overflow")?;
                let alloc = base
                    .get_typed_func::<i32, i32>(&mut *store, "hd.session.alloc")
                    .map_err(|e| e.to_string())?;
                let block = alloc
                    .call(&mut *store, bytes.cast_signed())
                    .map_err(|e| e.to_string())?
                    .cast_unsigned();
                let heap = base
                    .get_memory(&mut *store, "hd.session.heap")
                    .ok_or("missing session heap")?;
                heap.write(&mut *store, block as usize, &[0; 8])
                    .map_err(|e| e.to_string())?;
                data = block
                    .checked_add(8)
                    .ok_or("session data address overflow")?;
            }
            let table = base
                .get_table(&mut *store, "hd.session.table")
                .ok_or("missing session table")?;
            let length = checkpoint.map_or(u64::from(part.info.table_len), |c| c.table_len);
            let size = table.size(&*store);
            if size < length {
                table
                    .grow(&mut *store, length - size, wasmtime::Ref::Func(None))
                    .map_err(|e| e.to_string())?;
            }
            self.linker.allow_shadowing(true);
            for import in part.module.imports().filter(|i| session_import(i.module())) {
                let value = if import.module() == "hd:session" && import.name() == "data" {
                    let global = wasmtime::Global::new(
                        &mut *store,
                        wasmtime::GlobalType::new(
                            wasmtime::ValType::I32,
                            wasmtime::Mutability::Const,
                        ),
                        wasmtime::Val::I32(data.cast_signed()),
                    )
                    .map_err(|e| e.to_string())?;
                    wasmtime::Extern::Global(global)
                } else {
                    let epoch: usize = import
                        .module()
                        .strip_prefix("hd:session:")
                        .ok_or("unknown session import")?
                        .parse()
                        .map_err(|_| "bad session epoch")?;
                    instances
                        .get(epoch)
                        .ok_or("missing session epoch")?
                        .get_export(&mut *store, import.name())
                        .ok_or("missing session export")?
                };
                self.linker
                    .define(&*store, import.module(), import.name(), value)
                    .map_err(|e| e.to_string())?;
                self.hosts
                    .insert((import.module().to_owned(), import.name().to_owned()));
            }
        }
        let instance = self.instantiate(&part.module)?;
        let store = &mut self.run.store;
        if part.info.epoch == 0 {
            if let Some(checkpoint) = checkpoint {
                for (name, bytes) in &checkpoint.memories {
                    let memory = instance
                        .get_memory(&mut *store, name)
                        .ok_or("missing session memory")?;
                    let pages = (bytes.len() as u64).div_ceil(65536);
                    let size = memory.size(&*store);
                    if size < pages {
                        memory
                            .grow(&mut *store, pages - size)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        } else if checkpoint.is_none() {
            let memory = instance
                .get_memory(&mut *store, "hd.session.heap")
                .ok_or("missing session heap")?;
            for offset in &part.info.relocations {
                let at = data
                    .checked_add(*offset)
                    .ok_or("session relocation overflow")? as usize;
                let mut word = [0; 4];
                memory
                    .read(&*store, at, &mut word)
                    .map_err(|e| e.to_string())?;
                let value = u32::from_le_bytes(word).wrapping_add(data).wrapping_sub(16);
                memory
                    .write(&mut *store, at, &value.to_le_bytes())
                    .map_err(|e| e.to_string())?;
            }
        }
        instances.push(instance);
        Ok(data)
    }

    fn capture_linked(&mut self, linked: &Linked) -> Result<Checkpoint, String> {
        let store = &mut self.run.store;
        let base = *linked.instances.first().ok_or("missing session base")?;
        let mut memories = Vec::new();
        for name in MEMORIES {
            let memory = base
                .get_memory(&mut *store, name)
                .ok_or("missing session memory")?;
            memories.push((name, memory.data(&*store).to_vec()));
        }
        let mut globals = Vec::new();
        for (epoch, instance) in linked.instances.iter().enumerate() {
            let exported: Vec<_> = instance
                .exports(&mut *store)
                .filter_map(|e| {
                    let name = e.name().to_owned();
                    name.starts_with("hd.session.g.")
                        .then(|| e.into_global().map(|g| (name, g)))
                        .flatten()
                })
                .collect();
            for (name, global) in exported {
                let value = global.get(&mut *store);
                if !matches!(
                    value,
                    wasmtime::Val::I32(_)
                        | wasmtime::Val::I64(_)
                        | wasmtime::Val::F32(_)
                        | wasmtime::Val::F64(_)
                ) {
                    return Err("a session global is not numeric".into());
                }
                globals.push((epoch, name, value));
            }
        }
        let table_len = base
            .get_table(&mut *store, "hd.session.table")
            .ok_or("missing session table")?
            .size(&*store);
        Ok(Checkpoint {
            memories,
            globals,
            epochs: linked.instances.len(),
            table_len,
        })
    }

    fn restore_linked(&mut self, linked: &mut Linked) -> Result<(), String> {
        let bytes = self
            .run
            .store
            .data()
            .journal
            .as_ref()
            .and_then(Recorder::journal)
            .ok_or("missing active journal")?
            .to_vec();
        let mut tail = recorded(&bytes)?;
        tail.inputs
            .retain(|input| Some(input.number) != linked.cancelled);
        linked.restore_failed = Some(bytes);
        linked.history.take();
        linked.instances.clear();
        self.restart(Recorder::new(usize::MAX, linked.execution))?;
        let mut instances = Vec::new();
        let mut data_offsets = Vec::new();
        let count = linked.checkpoint.as_ref().map_or(0, |c| c.epochs);
        for part in &linked.modules[..count] {
            self.instantiate_linked(part, &mut instances, linked.checkpoint.as_ref())?;
        }
        if let Some(checkpoint) = &linked.checkpoint {
            let store = &mut self.run.store;
            for (name, bytes) in &checkpoint.memories {
                instances[0]
                    .get_memory(&mut *store, name)
                    .ok_or("missing checkpoint memory")?
                    .write(&mut *store, 0, bytes)
                    .map_err(|e| e.to_string())?;
            }
            for (epoch, name, value) in &checkpoint.globals {
                instances[*epoch]
                    .get_global(&mut *store, name)
                    .ok_or("missing checkpoint global")?
                    .set(&mut *store, *value)
                    .map_err(|e| e.to_string())?;
            }
        }
        if tail.inputs.len() != linked.modules.len() - count {
            return Err("the active journal does not match its session epochs".into());
        }
        for (part, input) in linked.modules[count..].iter().zip(&tail.inputs) {
            if part.number != input.number || !input.ran(&part.source) {
                return Err("the active journal does not match its session input".into());
            }
            data_offsets.push(self.instantiate_linked(part, &mut instances, None)?);
            self.replay(
                *instances.last().expect("tail instance"),
                input,
                &part.source,
                &part.wasm,
            )?;
        }
        linked.instances = instances;
        for (part, data) in linked.modules[count..].iter_mut().zip(data_offsets) {
            part.data = data;
        }
        linked.cancelled = None;
        linked.restore_failed = None;
        linked.history.take();
        Ok(())
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
            .map(|(ran, _)| ran)
    }

    fn run_record(
        &mut self,
        record: &Recorded,
        cap: usize,
        wasm: &[u8],
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<(Ran, wasmtime::Instance), String> {
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
                return Ok((ran, instance));
            }
        }
        if let Some(n) = live {
            let text = source(n).ok_or_else(|| format!("input {n} has no text"))?;
            ran.live = Some(self.input(instance, n, text, wasm)?);
        }
        Ok((ran, instance))
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
        let live = self.run.store.data().replay.is_none();
        // An input that failed to run is an outcome too.
        let outcome = result.unwrap_or_else(Outcome::Internal);
        if outcome != Outcome::Exit(0) {
            if live {
                // The stop is spent when the input ends. Keep a repeated
                // request from interrupting the runtime reset between
                // inputs; that is session setup, not user code.
                let interrupt = self.run.store.data_mut().interrupt.take();
                let reset = self.reset(instance);
                self.run.store.data_mut().interrupt = interrupt;
                self.flag.store(false, Ordering::Relaxed);
                reset?;
            } else {
                self.reset(instance)?;
            }
        } else if live {
            self.flag.store(false, Ordering::Relaxed);
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
        let current = self.run.store.data().journal.as_ref()?.journal()?;
        let Some(linked) = &self.linked else {
            return Some(current);
        };
        let tail = linked.restore_failed.as_deref().unwrap_or(current);
        if linked.archives.is_empty() {
            return Some(tail);
        }
        Some(linked.history.get_or_init(|| {
            let mut chunks: Vec<_> = linked.archives.iter().map(Vec::as_slice).collect();
            chunks.push(tail);
            join_journals(&chunks).expect("the session recorded complete journal epochs")
        }))
    }

    /// Forget the guest inputs after a rebuild drops the entire prefix,
    /// keeping the same provider and its configured journal threshold.
    pub fn clear(&mut self) -> Result<(), String> {
        let cap = self.linked.as_ref().map_or_else(
            || {
                self.run
                    .store
                    .data()
                    .journal
                    .as_ref()
                    .map_or(SESSION_CAP, Recorder::cap)
            },
            |linked| linked.cap,
        );
        let execution = recorded(self.journal().ok_or("the session has no journal")?)?.execution;
        let mut recorder = Recorder::new(usize::MAX, execution);
        recorder.set_cap(cap);
        self.restart(recorder)?;
        self.linked = None;
        Ok(())
    }

    /// Changes the journal's cap, `hd_run::journal::SESSION_CAP` until
    /// then; a journal already past it stops at its next row.
    pub fn set_journal_cap(&mut self, cap: usize) {
        if let Some(linked) = &mut self.linked {
            linked.cap = cap;
            return;
        }
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
        let linked = SessionInfo::read(wasm)?.is_some();
        if let Some((m, n)) = imports
            .iter()
            .find(|(m, n)| !(supported(m, n, self.profile) || linked && session_import(m)))
        {
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
