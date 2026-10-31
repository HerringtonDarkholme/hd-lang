// One writer orders stdout, stderr and session frames. Queue and status
// control messages never wait on a full pipe. A console write (`write`)
// returns once the stream has taken every byte, so it may block the calling
// thread (module.console.write-sync.never-suspends).
//
// While nothing is queued, a write goes straight to its nonblocking
// descriptor on the calling thread. A short write or `EAGAIN` starts the
// writer thread and hands it the unwritten rest and everything queued
// behind it. Later writes go through that thread until its queue is empty,
// which keeps stdout, stderr and session frames in order; then direct
// writes resume. A shared connection (a supervised guest and its
// supervisor) always uses the thread, and so does Windows, where a pipe
// write would block.
import { receiveReply } from "./worker-control.ts";
import { failed, full } from "./output-worker.ts";
import { Ok } from "./wire.ts";
import type { MessagePort, Worker as WorkerType } from "node:worker_threads";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;
type Flag = Int32Array<SharedArrayBuffer>;
interface OutputConnection {
  port: MessagePort;
  flag: Flag;
}
interface LocalEntry {
  fd: number;
  bytes: Buffer;
  offset: number;
  wait: boolean;
  result: Dynamic | null;
}

// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.ts).
const { writeSync } = process.getBuiltinModule("node:fs");
const { MessageChannel, Worker } = process.getBuiltinModule("node:worker_threads");

const DIRECT = process.platform !== "win32";

export function createOutput(connection: OutputConnection | null = null): DynamicRecord {
  let client: OutputConnection | null = connection;
  let worker: WorkerType | null = null;
  let nextCall = 1;
  let used = false;
  // Writes go through the thread while `remote`; `pinned` keeps them there.
  let pinned = connection !== null || !DIRECT;
  let remote = pinned;
  // The direct queue, in order: `{ fd, bytes, offset, wait, result }`.
  // `wait` marks a write whose caller waits for its result.
  const local: LocalEntry[] = [];
  let prepared = false;
  // Construct the main thread's pipe handles before any write uses their
  // descriptors: libuv makes pipes nonblocking, so a direct write returns
  // short instead of blocking, and an in-flight thread write cannot keep
  // writing a cancelled suffix when a reader resumes.
  const prepare = (): void => {
    if (prepared) {
      return;
    }
    prepared = true;
    (process.stdout as Dynamic)._handle?.setBlocking(false);
    (process.stderr as Dynamic)._handle?.setBlocking(false);
  };
  // Starts the writer thread; the first handed-over write otherwise waits
  // for it.
  const start = (): void => {
    if (client === null) {
      prepare();
      const { port1, port2 } = new MessageChannel();
      const flag = new Int32Array(new SharedArrayBuffer(4));
      worker = new Worker(new URL("./output-worker.ts", import.meta.url), {
        execArgv: [],
        workerData: { output: true, port: port2, flag },
        transferList: [port2],
      });
      worker.unref();
      port1.unref();
      client = { port: port1, flag };
    }
  };
  const call = (request: Dynamic, transfer: Dynamic[] = []): Dynamic => {
    start();
    used = true;
    Atomics.store(client!.flag, 0, 0);
    const id = nextCall++;
    const deadline = performance.now() + 5000;
    (client!.port as Dynamic).postMessage({ id, request }, transfer);
    // A synchronous write waits as long as its stream takes the bytes.
    // Short control requests still diagnose a stopped writer thread.
    const left =
      request.op === "wait_start"
        ? () => Number.POSITIVE_INFINITY
        : () => deadline - performance.now();
    const reply = receiveReply(client!.port, client!.flag, id, left);
    if (reply === null) {
      throw new Error("output control reply timed out");
    }
    return reply.result;
  };
  // Moves the direct queue, in order, to the thread: the head's unwritten
  // rest, then what waits behind it. Writes go there from now on.
  const handOff = (): void => {
    remote = true;
    for (const error of local.splice(0)) {
      const bytes = new Uint8Array(error.bytes.subarray(error.offset));
      if (error.wait) {
        error.result = call({ op: "wait_start", fd: error.fd, bytes }, [
          bytes.buffer as ArrayBuffer,
        ]);
      } else {
        call({ op: "enqueue", fd: error.fd, bytes }, [bytes.buffer as ArrayBuffer]);
      }
    }
  };
  // Writes the queued entries directly, until the queue is empty or a
  // descriptor is full.
  const pump = (): void => {
    while (local.length) {
      const pendingWrite = local[0];
      try {
        while (pendingWrite.offset < pendingWrite.bytes.length) {
          const count = writeSync(
            pendingWrite.fd,
            pendingWrite.bytes,
            pendingWrite.offset,
            pendingWrite.bytes.length - pendingWrite.offset,
          );
          if (count === 0) {
            return handOff();
          }
          pendingWrite.offset += count;
        }
        pendingWrite.result = Ok(undefined);
      } catch (error) {
        if (full(error as NodeJS.ErrnoException)) {
          return handOff();
        }
        pendingWrite.result = failed(error as NodeJS.ErrnoException);
      }
      local.shift();
    }
  };
  // Whether a new write may go direct: the thread, if it took over, has
  // written everything it was given.
  const direct = (): boolean => {
    if (!remote) {
      return true;
    }
    if (pinned || !call({ op: "idle" }).ok) {
      return false;
    }
    remote = false;
    return true;
  };
  const queue = (fd: number, text: string, wait: boolean): LocalEntry => {
    prepare();
    const error: LocalEntry = { fd, bytes: Buffer.from(text), offset: 0, wait, result: null };
    local.push(error);
    pump();
    return error;
  };
  const send = (fd: number, text: string): void => {
    if (direct()) {
      queue(fd, text, false);
    } else {
      call({ op: "enqueue", fd, text });
    }
  };
  const output: DynamicRecord = {
    start,
    // Whether writes go through the writer thread now (for tests).
    threaded: () => remote,
    out: (text: string) => send(1, text),
    err: (text: string) => send(2, text),
    // A console write: its result once the stream has taken every byte.
    // A direct write that writes every byte answers at once. Otherwise the
    // writer thread has the rest, and this blocks until it has written it.
    write: (fd: number, text: string) => {
      if (direct()) {
        const pendingWrite = queue(fd, text, true);
        return pendingWrite.result;
      }
      return call({ op: "wait_start", fd, text });
    },
    connection: () => {
      if (!remote) {
        handOff();
      }
      pinned = true;
      const { port1, port2 } = new MessageChannel();
      const flag = new Int32Array(new SharedArrayBuffer(4));
      call({ op: "attach", port: port2, flag }, [port2]);
      return { port: port1, flag };
    },
    drain: async (): Promise<void> => {
      for (;;) {
        pump();
        if (!local.length && (client === null || call({ op: "idle" }).ok)) {
          return;
        }
        await new Promise<void>((resolve) => setTimeout(resolve, 1));
      }
    },
    close: async (): Promise<void> => {
      // A writer that was started but never written to has nothing to
      // flush; its unref'd thread need not delay the exit.
      if (used || local.length) {
        await output.drain();
      }
      client?.port.close();
      client = null;
      if (worker !== null) {
        if (used) {
          await worker.terminate();
        } else {
          void worker.terminate();
        }
      }
      worker = null;
    },
  };
  return output;
}
