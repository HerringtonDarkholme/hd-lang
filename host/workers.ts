// std.remote's host workers on Node (std-remote.workers.host): a
// `worker_threads` thread holds a second instance of the program's module.
// It runs `hd.init`, then the root `hd.worker` (`std.remote.worker_serve`),
// whose `hd:WorkerPort` takes each frame the parent sends and gives back
// its reply. Builtins come from `process.getBuiltinModule` (node.ts).
const { MessageChannel, Worker, receiveMessageOnPort, workerData } =
  process.getBuiltinModule("node:worker_threads");
import type { MessagePort } from "node:worker_threads";
import { CrossError } from "./gen/abi.ts";
import { Err, Ok, variant } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type Flag = Int32Array<SharedArrayBuffer>;

// Posts `message` on `port` and wakes the side that waits on `flag`.
const post = (port: MessagePort, flag: Flag, message: Dynamic): void => {
  port.postMessage(message);
  Atomics.add(flag, 0, 1);
  Atomics.notify(flag, 0);
};

// The next message on `port`, whose sender counts its posts on `flag`:
// waits without spinning until one is there.
const take = (port: MessagePort, flag: Flag): Dynamic => {
  for (;;) {
    const seen = Atomics.load(flag, 0);
    const item = receiveMessageOnPort(port);
    if (item) {
      return item.message;
    }
    Atomics.wait(flag, 0, seen);
  }
};

// `hd:Workers` (std-remote.workers.host): the first send starts one worker
// thread on `module`, and each send waits for its frame's reply. A worker
// whose instance ended, or a host without the module, is `Unreachable`.
// Cost: one thread and one instance per run; per send, two copies of the
// frame and two of the reply.
export function workersProvider(
  module: WebAssembly.Module | undefined,
): Record<string, (frame: Uint8Array) => Dynamic> {
  let link: { port: MessagePort; down: Flag; up: Flag } | null = null;
  const lost = () => Err(variant(CrossError, "Unreachable"));
  return {
    send: (frame: Uint8Array) => {
      if (!module) {
        return lost();
      }
      if (!link) {
        const { port1, port2 } = new MessageChannel();
        const flag = () => new Int32Array(new SharedArrayBuffer(4));
        link = { port: port1, down: flag(), up: flag() };
        const hdWorker = { module, port: port2, down: link.down, up: link.up };
        new Worker(new URL(import.meta.url), {
          workerData: { hdWorker },
          transferList: [port2],
        }).unref();
        port1.unref();
      }
      post(link.port, link.down, { frame });
      const reply = take(link.port, link.up);
      if (reply.frame) {
        return Ok(reply.frame);
      }
      link = null;
      return lost();
    },
  };
}

// The worker thread: `hd:WorkerPort` takes the parent's frames, and an
// instance that ends tells the parent so. Not awaited at the top level:
// node.ts imports this module, so a top-level wait for node.ts would wait
// on itself.
async function serve(worker: {
  module: WebAssembly.Module;
  port: MessagePort;
  down: Flag;
  up: Flag;
}): Promise<void> {
  const { module, port, down, up } = worker;
  const { createHost } = await import("./node.ts");
  const sink = {
    out: (text: string) => void process.stdout.write(text),
    err: (text: string) => void process.stderr.write(text),
  };
  const host = createHost(sink, {
    module,
    workerPort: {
      next: () => take(port, down).frame ?? null,
      reply: (frame: Uint8Array) => post(port, up, { frame }),
    },
  });
  try {
    await host.run(await WebAssembly.instantiate(module, host.imports), "hd.init", "hd.worker");
  } finally {
    post(port, up, { lost: true });
    host.close();
  }
}
if (workerData?.hdWorker) {
  void serve(workerData.hdWorker);
}
