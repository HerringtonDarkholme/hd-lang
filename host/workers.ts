// std.remote's host workers on Node (std-remote.workers.host): a
// `worker_threads` thread holds a second instance of the program's module.
// It runs `hd.init`, then the root `hd.worker` (`std.remote.worker_serve`),
// whose `hd:WorkerPort` takes each frame the parent sends and gives back
// its reply. Builtins come from `process.getBuiltinModule` (node.ts).
const { MessageChannel, Worker, receiveMessageOnPort, workerData } =
  process.getBuiltinModule("node:worker_threads");
import type { MessagePort } from "node:worker_threads";
import { CrossError } from "./gen/abi.ts";
import { pendingOperation } from "./core.ts";
import { Err, Ok, variant } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type Flag = Int32Array<SharedArrayBuffer>;

// Posts `message` on `port`, moving `frame`'s buffer, and wakes the side
// that waits on `flag`.
const post = (port: MessagePort, flag: Flag, message: Dynamic, frame?: Uint8Array): void => {
  port.postMessage(message, frame ? [frame.buffer as ArrayBuffer] : []);
  Atomics.add(flag, 0, 1);
  Atomics.notify(flag, 0);
};

// A send in flight: its reply once it came, and the wake of its pending
// operation.
type Send = { reply: Dynamic; ready: (() => void) | null };
type Link = { port: MessagePort; down: Flag; sends: Map<number, Send> };

// `hd:Workers` (std-remote.workers.host): the first send starts one worker
// thread on `module`. A send is a pending operation (std-host.op.wait), so
// sends from several tasks are in flight together: the worker posts each
// reply with its send's number and wakes the instance's `reactor`, and
// `drain` delivers it. A cancelled send (std-host.op.cancel) stops
// waiting, and the worker drops its frame unless it already runs it, as
// tokio's `spawn_blocking` abort does; a late reply finds no send. A
// worker whose instance ended, or a host without the module, makes each
// send still in flight `Unreachable`, and the next send starts another.
// Cost: one thread and one instance per run; per send, one map entry and
// one message each way with its frame's buffer moved, not copied; one
// wake of the worker and one of the reactor; no polling.
export function workersProvider(
  module: WebAssembly.Module | undefined,
  reactor: Flag,
): { provider: Record<string, (frame: Uint8Array) => Dynamic>; drain: () => void } {
  let link: Link | null = null;
  let last = 0;
  const lost = () => Err(variant(CrossError, "Unreachable"));
  const settle = (send: Send, reply: Dynamic): void => {
    send.reply = reply;
    send.ready?.();
  };
  const deliver = (from: Link, message: Dynamic): void => {
    if (message.lost) {
      if (link === from) {
        link = null;
      }
      for (const send of from.sends.values()) {
        settle(send, lost());
      }
      from.sends.clear();
      return;
    }
    const send = from.sends.get(message.id);
    if (send) {
      from.sends.delete(message.id);
      settle(send, Ok(message.frame));
    }
  };
  const start = (code: WebAssembly.Module): Link => {
    const { port1, port2 } = new MessageChannel();
    const down = new Int32Array(new SharedArrayBuffer(4));
    const started: Link = { port: port1, down, sends: new Map() };
    new Worker(new URL(import.meta.url), {
      workerData: { hdWorker: { module: code, port: port2, down, up: reactor } },
      transferList: [port2],
    }).unref();
    // The event loop delivers while the instance awaits a Promise; a
    // synchronous wait on the reactor delivers through `drain`.
    port1.on("message", (message: Dynamic) => deliver(started, message));
    port1.unref();
    return started;
  };
  return {
    provider: {
      send: (frame: Uint8Array) => {
        if (!module) {
          return lost();
        }
        link ??= start(module);
        const to = link;
        const id = ++last;
        const send: Send = { reply: null, ready: null };
        to.sends.set(id, send);
        post(to.port, to.down, { id, frame }, frame);
        return pendingOperation(
          () => send.reply !== null,
          () => send.reply,
          () => {
            if (to.sends.delete(id)) {
              post(to.port, to.down, { cancel: id });
            }
          },
          null,
          (ready) => {
            send.ready = ready;
            if (send.reply !== null) {
              ready();
            }
            return () => {
              send.ready = null;
            };
          },
        );
      },
    },
    drain: () => {
      for (let from = link; from !== null; from = link) {
        const item = receiveMessageOnPort(from.port);
        if (!item) {
          return;
        }
        deliver(from, item.message);
      }
    },
  };
}

// The worker thread: `hd:WorkerPort` takes the parent's frames in order,
// skipping the ones cancelled before they were taken, and an instance
// that ends tells the parent so. Not awaited at the top level: node.ts
// imports this module, so a top-level wait for node.ts would wait on
// itself.
async function serve(worker: {
  module: WebAssembly.Module;
  port: MessagePort;
  down: Flag;
  up: Flag;
}): Promise<void> {
  const { module, port, down, up } = worker;
  // The frames not yet taken, by send, in the order they came.
  const frames = new Map<number, Uint8Array>();
  let taken = 0;
  const next = (): Uint8Array => {
    for (;;) {
      const seen = Atomics.load(down, 0);
      for (let item; (item = receiveMessageOnPort(port));) {
        const { id, frame, cancel } = item.message;
        if (cancel === undefined) {
          frames.set(id, frame);
        } else {
          frames.delete(cancel);
        }
      }
      for (const [id, frame] of frames) {
        frames.delete(id);
        taken = id;
        return frame;
      }
      Atomics.wait(down, 0, seen);
    }
  };
  try {
    const { createHost } = await import("./node.ts");
    const sink = {
      out: (text: string) => void process.stdout.write(text),
      err: (text: string) => void process.stderr.write(text),
    };
    const host = createHost(sink, {
      module,
      workerPort: {
        next,
        reply: (frame: Uint8Array) => post(port, up, { id: taken, frame }, frame),
      },
    });
    try {
      await host.run(await WebAssembly.instantiate(module, host.imports), "hd.init", "hd.worker");
    } finally {
      host.close();
    }
  } finally {
    post(port, up, { lost: true });
  }
}
if (workerData?.hdWorker) {
  void serve(workerData.hdWorker);
}

// A thread of a program on a shared heap (module.rt.thread.spawn): a
// `worker_threads` thread with an instance of `module` on `heap`, which
// runs `hd.thread` on `block`. Whether the thread started.
// Cost: one thread and one instance per thread the program starts.
export function startThread(
  module: WebAssembly.Module | undefined,
  heap: WebAssembly.Memory | null,
  block: number,
): boolean {
  if (!module || !heap) {
    return false;
  }
  new Worker(new URL(import.meta.url), { workerData: { hdThread: { module, heap, block } } });
  return true;
}

// The thread: `hd.thread(block, 0)` runs its work; an instance that stopped
// gets `hd.thread(block, 2)`, which tells the threads that wait for it.
// Its output goes to the process's streams at once, since the thread that
// started it may be waiting.
async function runThread(thread: {
  module: WebAssembly.Module;
  heap: WebAssembly.Memory;
  block: number;
}): Promise<void> {
  const { module, heap, block } = thread;
  const { createHost } = await import("./node.ts");
  const { writeSync } = process.getBuiltinModule("node:fs");
  const sink = {
    out: (text: string) => writeSync(1, text),
    err: (text: string) => writeSync(2, text),
  };
  const host = createHost(sink, { module, heap });
  const instance = await WebAssembly.instantiate(module, host.imports);
  host.prime(instance);
  const start = instance.exports["hd.thread"] as (block: number, stopped: number) => void;
  try {
    start(block, 0);
  } catch {
    start(block, 2);
  } finally {
    host.close();
  }
}
if (workerData?.hdThread) {
  void runThread(workerData.hdThread);
}
