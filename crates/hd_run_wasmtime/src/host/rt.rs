//! The runtime imports, which no host trait owns: `hd:rt`'s `block`,
//! `abort`, `stderr`, `stdout`, `panic`, `fact`, `hash_seed`,
//! `thread_spawn` and `thread_watch`, and the `hd:prim` primitives,
//! whose text and float work is `prim.rs`.

use wasmtime::{Caller, Val};

use super::kind::{Prim, Rt};
use super::link::deadlock;
use super::reply::{Ret, complete, float_arg, handle, int_arg, text};
use super::{Control, Host, Stop};
use crate::prim;

/// The method of a task's wait for a thread among the reactor's native
/// calls: no package interface has this index.
const WATCH: (usize, usize) = (usize::MAX, 0);

/// An `hd:rt` import.
pub(super) fn rt(
    import: Rt,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match import {
        Rt::Stderr => {
            let t = text(caller, int_arg(params, 0)?)?;
            caller.data_mut().guest_err(&t);
            Ret::void()
        }
        Rt::Panic => {
            caller.data_mut().begin_panic();
            Ret::void()
        }
        // A facts body starts, with its label in the buffer, or ends (-1).
        Rt::Fact => {
            let len = int_arg(params, 0)?;
            if len < 0 {
                caller.data_mut().facts.pop();
            } else {
                let label = text(caller, len)?;
                caller.data_mut().facts.push(label);
            }
            Ret::void()
        }
        // Held with `Console`'s output, so a later `stderr` write puts it
        // first.
        Rt::Stdout => {
            let t = text(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            host.out.extend_from_slice(t.as_bytes());
            host.flush_over_bound();
            Ret::void()
        }
        Rt::Block => loop {
            let handles = complete(caller)?;
            if !handles.is_empty() {
                return Ret::batch(&handles);
            }
            if !caller.data_mut().wait()? {
                return Err(deadlock(caller.data_mut()));
            }
        },
        Rt::Abort => {
            let h = int_arg(params, 0)?;
            caller.data_mut().control(Control::Abort(handle(h)));
            Ret::void()
        }
        // The run's map hash key: 64 random bits. The instance asks once
        // and keeps it in a runtime global; a session's journal records
        // the call, so a rebuild hashes with the same key.
        Rt::HashSeed => {
            let bytes = crate::sys::random(8)?;
            let mut seed = [0u8; 8];
            seed.copy_from_slice(&bytes);
            Ret::int(i64::from_le_bytes(seed))
        }
        Rt::ThreadSpawn => {
            let block = int_arg(params, 0)?;
            let host = caller.data();
            let started = (host.workers.spawn.as_ref())
                .zip(host.workers.heap.clone())
                .is_some_and(|(spawn, heap)| crate::workers::start_thread(spawn, heap, block));
            Ret::int(i64::from(started))
        }
        // A task's wait for a thread's end (`module.rt.thread.running`):
        // always pending, on its waiter thread (`workers::watch`).
        Rt::ThreadWatch { finish: false } => {
            // ruled out: both are i32 parameters, whose values fit an i32.
            let word = |k| {
                int_arg(params, k).map(|v| i32::try_from(v).unwrap_or_default().cast_unsigned())
            };
            let (address, seen) = (u64::from(word(0)?), word(1)?);
            let host = caller.data_mut();
            let heap = (host.workers.heap.clone())
                .ok_or_else(|| Stop::Internal("a thread's wait with no shared heap".into()))?;
            let call = crate::workers::watch(heap, address, seen)
                .map_err(|e| Stop::Internal(format!("no thread waits for a thread: {e}")))?;
            let h = (host.reactor.native(call, WATCH))
                .ok_or_else(|| Stop::Internal("a thread's wait ended before it began".into()))?;
            Ret::pair(1, h)
        }
        Rt::ThreadWatch { finish: true } => {
            let h = handle(int_arg(params, 0)?);
            let host = caller.data_mut();
            (host.reactor.finish_native(h, WATCH, &mut host.handles))
                .and_then(Result::ok)
                .ok_or_else(|| {
                    Stop::Contract("a finish of an operation that is not completed".into())
                })?;
            Ret::int(0)
        }
        // ruled out: the heap is a memory, never called (`link.rs`).
        Rt::Heap => return Err(Stop::Internal("the heap is not a function".into()).into()),
    })
}

/// An `hd:prim` import: answered from its arguments alone.
pub(super) fn prim(
    import: Prim,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match import {
        Prim::FormatF64 => Ret::value(&prim::shortest_f64(float_arg(params, 0)?)),
        Prim::FormatF32 => {
            let s = match params.first() {
                Some(Val::F32(bits)) => prim::shortest_f32(f32::from_bits(*bits)),
                _ => return Err(Stop::Contract("format_f32 takes an f32".into()).into()),
            };
            Ret::value(&s)
        }
        Prim::FormatFixed => {
            let value = float_arg(params, 0)?;
            let places = u32::try_from(int_arg(params, 1)?)
                .map_err(|_| Stop::Internal("format_f64_fixed: digits out of range".into()))?;
            let s = prim::fixed(value, places);
            Ret::value(&s)
        }
        Prim::Lower | Prim::Upper => {
            let t = text(caller, int_arg(params, 0)?)?;
            let s = if matches!(import, Prim::Lower) {
                prim::lower(&t)
            } else {
                prim::upper(&t)
            };
            Ret::value(&s)
        }
        // `std.testing.snapshot`'s hook: the failing call's text, which
        // holds its site, goes to the case's state in an update run.
        Prim::SnapshotUpdate => {
            let t = text(caller, int_arg(params, 0)?)?;
            let recorded = caller
                .data()
                .testing
                .as_ref()
                .is_some_and(|state| state.record_snapshot(t));
            Ret::int(i64::from(recorded))
        }
        Prim::ParseF64 => {
            let t = text(caller, int_arg(params, 0)?)?;
            match prim::parse(&t) {
                Some(v) => Ret::float(v),
                None => {
                    return Err(Stop::Internal(format!(
                        "parse_f64: text outside the grammar: {t}"
                    ))
                    .into());
                }
            }
        }
    })
}
