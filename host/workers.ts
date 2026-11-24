// std.remote's host workers on Node (std-remote.workers.host): a
// `worker_threads` thread holds a second instance of the program's module.
// It runs `hd.init`, then the root `hd.worker` (`std.remote.worker_serve`),
// whose `hd:WorkerPort` takes each frame the parent sends and gives back
// its reply. Builtins come from `process.getBuiltinModule` (node.ts).
const { MessageChannel, Worker, receiveMessageOnPort, workerData } =
  process.getBuiltinModule("node:worker_threads");
import type { MessagePort } from "node:worker_threads";
import { Frames, type Link, workerSends } from "./worker-link.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type Flag = Int32Array<SharedArrayBuffer>;

// Posts `message` on `port`, moving `frame`'s buffer, and wakes the side
// that waits on `flag`.
const post = (port: MessagePort, flag: Flag, message: Dynamic, frame?: Uint8Array): void => {
  port.postMessage(message, frame ? [frame.buffer as ArrayBuffer] : []);
  Atomics.add(flag, 0, 1);
  Atomics.notify(flag, 0);
};

// `hd:Workers` (std-remote.workers.host, worker-link.ts): the first send
// starts one worker thread on `module`. The worker posts each reply and
// wakes the instance's `reactor`, and `drain` delivers it; the port's
// listener delivers while the instance awaits a Promise. A host without
// the module answers `Unreachable`.
// Cost: one thread and one instance per run; per send, one wake of the
// worker and one of the reactor; no polling.
export function workersProvider(
  module: WebAssembly.Module | undefined,
  reactor: Flag,
): { provider: Record<string, (frame: Uint8Array) => Dynamic>; drain: () => void } {
  type Port = Link & { port: MessagePort; deliver: (message: Dynamic) => void };
  const { send, current } = workerSends((deliver): Port | null => {
    if (!module) {
      return null;
    }
    const { port1, port2 } = new MessageChannel();
    const down = new Int32Array(new SharedArrayBuffer(4));
    new Worker(new URL(import.meta.url), {
      workerData: { hdWorker: { module, port: port2, down, up: reactor } },
      transferList: [port2],
    }).unref();
    port1.on("message", deliver);
    port1.unref();
    return {
      port: port1,
      deliver,
      post: (message: Dynamic, frame?: Uint8Array) => post(port1, down, message, frame),
    };
  });
  return {
    provider: { send },
    drain: () => {
      for (let from = current(); from !== null; from = current()) {
        const item = receiveMessageOnPort(from.port);
        if (!item) {
          return;
        }
        from.deliver(item.message);
      }
    },
  };
}

// The worker thread: `hd:WorkerPort` takes the parent's frames in order,
// and an instance that ends tells the parent so. Not awaited at the top
// level: node.ts imports this module, so a top-level wait for node.ts
// would wait on itself.
async function serve(worker: {
  module: WebAssembly.Module;
  port: MessagePort;
  down: Flag;
  up: Flag;
}): Promise<void> {
  const { module, port, down, up } = worker;
  const frames = new Frames();
  const next = (): Uint8Array => {
    for (;;) {
      const seen = Atomics.load(down, 0);
      for (let item; (item = receiveMessageOnPort(port));) {
        frames.add(item.message);
      }
      const frame = frames.take();
      if (frame) {
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
        reply: (frame: Uint8Array) => post(port, up, { id: frames.taken, frame }, frame),
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
