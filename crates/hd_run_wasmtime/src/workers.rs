//! `std.remote`'s host workers (`std-remote.workers.host`): the first
//! `Workers.send!` starts an OS thread with a second instance of the
//! program's module, which runs `hd.init`, then `hd.worker`
//! (`std.remote.worker_serve`). Its `WorkerPort.next` takes each frame
//! from a channel, and `reply` answers the send that frame came from. A
//! send waits on the `hd-io` runtime, so the program's other operations
//! go on meanwhile. Cancelling it aborts that wait, which drops its reply's
//! receiver, and the worker skips a frame whose receiver is gone. Cost:
//! one thread and one instance per run; per send, one copy of the frame
//! and one of the reply each way.

use std::sync::{Arc, mpsc};

use tokio::sync::oneshot;
use wasmtime::{Caller, Val};

use crate::abi::{CrossError, HostMethod, Id};
use crate::codec::{Enc, encode};
use crate::host::offload::{finished, not_completed, pending};
use crate::host::{Host, HostIo, Ret, bytes, int_arg};
use crate::native::Suspension;
use crate::runtime::reactor::Done;

/// A frame for the worker, and where its reply goes.
type Job = (Vec<u8>, oneshot::Sender<Vec<u8>>);
/// A worker's frames, and the reply of the one taken.
type Port = (mpsc::Receiver<Job>, Option<oneshot::Sender<Vec<u8>>>);

/// What a worker is made of: the parent's engine and module.
pub(crate) struct Spawn {
    pub(crate) config: crate::engine::EngineConfig,
    pub(crate) engine: Arc<wasmtime::Engine>,
    pub(crate) profile: crate::profiles::RuntimeProfile,
    pub(crate) module: Arc<wasmtime::Module>,
    /// A test program's host kind, whose imports its threads link too.
    pub(crate) test: Option<crate::test_engine::TestHostKind>,
}

/// One instance's workers: in a parent, the running worker's channel; in
/// a worker, its frames and the reply of the one taken.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) spawn: Option<Spawn>,
    /// The heap a program that starts threads shares with them
    /// (`module.rt.thread.heap`).
    pub(crate) heap: Option<wasmtime::SharedMemory>,
    link: Option<mpsc::Sender<Job>>,
    port: Option<Port>,
}

/// The `List[u8]` argument, as its codec wrote it.
fn frame(caller: &mut Caller<'_, Host>, params: &[Val]) -> wasmtime::Result<Vec<u8>> {
    let encoded = bytes(caller, int_arg(params, 0)?)?;
    Ok(crate::codec::Dec::new(&encoded).bytes()?.to_vec())
}

fn lost() -> Enc {
    encode(&Err::<Vec<u8>, _>(CrossError::Unreachable))
}

/// `Workers.send`'s `.start` or `.finish`, or a `WorkerPort` method.
pub(crate) fn call(
    method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    match method.id {
        Id::WorkersSend if finish => match finished(caller, params)? {
            Done::Workers(reply) => Ok(Ret::encoded(reply)),
            _ => Err(not_completed()),
        },
        Id::WorkersSend => {
            let frame = frame(caller, params)?;
            let host = caller.data_mut();
            let (tx, rx) = oneshot::channel();
            // A worker that ended takes no job: the next one starts.
            let mut job = (frame, tx);
            let mut sent = false;
            for _ in 0..2 {
                let Some(link) = host.workers.link() else {
                    break;
                };
                match link.send(job) {
                    Ok(()) => {
                        sent = true;
                        break;
                    }
                    Err(mpsc::SendError(back)) => {
                        job = back;
                        host.workers.link = None;
                    }
                }
            }
            if !sent {
                return Ret::ready(lost());
            }
            let reply = async move {
                Done::Workers(
                    rx.await
                        .map_or_else(|_| lost(), |reply| encode(&Ok::<_, CrossError>(reply))),
                )
            };
            pending(host.reactor.offload(reply), |_| lost())
        }
        Id::WorkerPortNext => {
            let next = caller.data_mut().workers.port.as_mut().and_then(take);
            Ok(Ret::value(&next))
        }
        Id::WorkerPortReply => {
            let frame = frame(caller, params)?;
            if let Some((_, reply)) = &mut caller.data_mut().workers.port
                && let Some(to) = reply.take()
            {
                // ruled out: a parent that stopped waiting drops the reply.
                let _ = to.send(frame);
            }
            Ok(Ret::void())
        }
        _ => Err(crate::host::misrouted(method)),
    }
}

/// The next frame whose send still waits, which `reply` then answers. A
/// frame whose send was cancelled before the worker took it is dropped
/// unrun (`std-host.op.cancel`), as tokio's `spawn_blocking` abort drops a
/// task that has not started; one already taken runs to its end, and its
/// reply finds no receiver. Cost: one closed check per frame.
fn take((frames, reply): &mut Port) -> Option<Vec<u8>> {
    let (frame, to) = frames.iter().find(|(_, to)| !to.is_closed())?;
    *reply = Some(to);
    Some(frame)
}

impl State {
    /// The running worker's channel, after starting one if none runs.
    fn link(&mut self) -> Option<mpsc::Sender<Job>> {
        if self.link.is_none() {
            self.link = self.spawn.as_ref().and_then(start);
        }
        self.link.clone()
    }
}

/// Starts a worker thread on `spawn`'s module. Its channel closes when its
/// instance ends, a trap included.
fn start(spawn: &Spawn) -> Option<mpsc::Sender<Job>> {
    let (tx, rx) = mpsc::channel();
    let engine = crate::engine::WasmtimeEngine {
        config: spawn.config,
        engine: Arc::clone(&spawn.engine),
        profile: spawn.profile,
    };
    let module = crate::engine::Module(Arc::clone(&spawn.module), None, None);
    std::thread::Builder::new()
        .name("hd-worker".into())
        .stack_size(crate::engine::thread_stack(&spawn.config))
        // cost: one OS thread per started worker (the first send, or one after a worker ended)
        .spawn(move || {
            use hd_run::Instance as _;
            let mut host = Host::new(
                HostIo::process(),
                Vec::new(),
                String::new(),
                hd_run::Grants::default(),
                None,
            );
            host.workers.port = Some((rx, None));
            if let Ok(mut inst) = engine.instantiate_worker(&module, host)
                && inst.init() == hd_run::Outcome::Exit(0)
            {
                hd_run::drive(&mut inst, usize::MAX);
            }
        })
        .ok()?;
    Some(tx)
}

/// Starts a thread of a program on `heap` (`module.rt.thread.spawn`): an OS
/// thread with an instance of `spawn`'s module on `heap`, which runs
/// `hd.thread(block, 0)`, or `hd.thread(block, 2)` once that stopped, so
/// the threads that wait for it wake. Whether its instance started; the
/// caller waits for that. Cost: one thread and one instance per thread.
pub(crate) fn start_thread(spawn: &Spawn, heap: wasmtime::SharedMemory, block: i64) -> bool {
    let (tx, rx) = mpsc::channel();
    let (engine, module, test) = (
        Arc::clone(&spawn.engine),
        Arc::clone(&spawn.module),
        spawn.test,
    );
    let profile = spawn.profile;
    let started = std::thread::Builder::new()
        .name("hd-thread".into())
        .stack_size(crate::engine::thread_stack(&spawn.config))
        // cost: one OS thread per program thread_spawn call
        .spawn(move || {
            let mut host = Host::new(
                HostIo::process(),
                Vec::new(),
                String::new(),
                hd_run::Grants::default(),
                None,
            );
            host.workers.heap = Some(heap);
            let run = crate::engine::new_store(&engine, host)
                .map_err(|e| format!("{e:?}"))
                .and_then(|mut store| {
                    let mut linker = wasmtime::Linker::new(&engine);
                    match test {
                        Some(kind) => crate::host::define_test(&mut linker, &module, kind)?,
                        None => crate::host::define(
                            &mut linker,
                            &module,
                            profile,
                            &mut hd_base::HashSet::default(),
                        )?,
                    }
                    crate::engine::link_heap(&mut linker, &mut store, &module)?;
                    let instance = linker
                        .instantiate(&mut store, &module)
                        .map_err(|e| format!("{e:#}"))?;
                    store.data_mut().memory = instance.get_memory(&mut store, "hd.x");
                    let start = instance
                        .get_typed_func::<(i32, i32), ()>(&mut store, "hd.thread")
                        .map_err(|e| format!("{e:#}"))?;
                    Ok((store, start))
                });
            let Ok((mut store, start)) = run else {
                // ruled out: the parent stopped waiting only when it ended.
                let _ = tx.send(false);
                return;
            };
            let _ = tx.send(true);
            // ruled out: a block is a heap address, below 2^32.
            let block = i32::try_from(block).unwrap_or_default();
            if start.call(&mut store, (block, 0)).is_err() {
                // ruled out: a trap here leaves its waiters as they were.
                let _ = start.call(&mut store, (block, 2));
            }
        });
    started.is_ok() && rx.recv() == Ok(true)
}

/// A task's wait for a thread of the program to end
/// (`module.rt.thread.running`): a pending call that one waiter thread
/// completes once the word at `address` of `heap` no longer holds `seen`,
/// or a notify there wakes it. The call's abort wakes the waiter until it
/// has left the wait, so it never sleeps on a word whose block, which the
/// task owns until the abort returns, was freed. An error when no thread
/// starts. Cost: one OS thread per pending wait, asleep in the wait.
pub(crate) fn watch(
    heap: wasmtime::SharedMemory,
    address: u64,
    seen: u32,
) -> std::io::Result<Suspension> {
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    let (stop, left) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let (call, done) = crate::native::pending::<()>({
        let (heap, stop, left) = (heap.clone(), Arc::clone(&stop), Arc::clone(&left));
        move || {
            stop.store(true, SeqCst);
            while !left.load(SeqCst) {
                let _ = heap.atomic_notify(address, u32::MAX);
                std::thread::yield_now();
            }
        }
    });
    let waiter = std::thread::Builder::new().name("hd-watch".into());
    // cost: one OS thread per pending wait for a thread's end
    let waiting = Arc::clone(&left);
    if let Err(error) = waiter.spawn(move || {
        if !stop.load(SeqCst) {
            let _ = heap.atomic_wait32(address, seen, None);
        }
        done.complete(Ok(()));
        waiting.store(true, SeqCst);
    }) {
        // No waiter: the call's abort, as it drops, has none to wake.
        left.store(true, SeqCst);
        return Err(error);
    }
    Ok(Suspension::new(call, |(), _| Ok(Vec::new())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cancelled send's frame is skipped, and the next open one is taken.
    #[test]
    fn a_frame_whose_send_was_cancelled_is_not_taken() {
        let (tx, rx) = mpsc::channel();
        let mut port: Port = (rx, None);
        let (cancelled, gone) = oneshot::channel();
        let (open, mut waits) = oneshot::channel();
        tx.send((vec![1], cancelled)).unwrap();
        tx.send((vec![2], open)).unwrap();
        drop((tx, gone));
        assert_eq!(take(&mut port), Some(vec![2]));
        port.1.take().unwrap().send(vec![4]).unwrap();
        assert_eq!(waits.try_recv(), Ok(vec![4]));
        assert_eq!(take(&mut port), None, "a closed channel has no frame");
    }
}
