// One writer orders stdout, stderr and session frames. Control messages
// never wait on a full pipe. Aborting a pending write drops its unwritten
// suffix; bytes already passed to the stream cannot be undone.
//
// While nothing is queued, a write goes straight to its nonblocking
// descriptor on the calling thread. A short write or `EAGAIN` starts the
// writer thread and hands it the unwritten rest and everything queued
// behind it. Later writes go through that thread until its queue is empty,
// which keeps stdout, stderr and session frames in order; then direct
// writes resume. A shared connection (a supervised guest and its
// supervisor) always uses the thread, and so does Windows, where a pipe
// write would block.
import { receiveReply } from "./worker-control.mjs";
import { pendingOperation } from "./core.mjs";
import { failed, full } from "./output-worker.mjs";
import { Ok } from "./wire.mjs";

// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.mjs).
const { writeSync } = process.getBuiltinModule("node:fs");
const { MessageChannel, Worker } = process.getBuiltinModule("node:worker_threads");

const DIRECT = process.platform !== "win32";

export function createOutput(connection = null) {
  let client = connection;
  let worker = null;
  let nextCall = 1;
  let used = false;
  // Writes go through the thread while `remote`; `pinned` keeps them there.
  let pinned = connection !== null || !DIRECT;
  let remote = pinned;
  // The direct queue, in order: `{ fd, bytes, offset, wait, started,
  // result, remote }`. Its head is written once started; entries behind an
  // unpolled pending write wait for it. `remote` is the thread's operation
  // once the entry was handed over.
  const local = [];
  let prepared = false;
  // Construct the main thread's pipe handles before any write uses their
  // descriptors: libuv makes pipes nonblocking, so a direct write returns
  // short instead of blocking, and an in-flight thread write cannot keep
  // writing a cancelled suffix when a reader resumes.
  const prepare = () => {
    if (prepared) return;
    prepared = true;
    process.stdout._handle?.setBlocking(false);
    process.stderr._handle?.setBlocking(false);
  };
  // Starts the writer thread; the first handed-over write otherwise waits
  // for it.
  const start = () => {
    if (client === null) {
      prepare();
      const { port1, port2 } = new MessageChannel();
      const flag = new Int32Array(new SharedArrayBuffer(4));
      worker = new Worker(new URL("./output-worker.mjs", import.meta.url), {
        execArgv: [],
        workerData: { output: true, port: port2, flag },
        transferList: [port2],
      });
      worker.unref();
      port1.unref();
      client = { port: port1, flag };
    }
  };
  const call = (request, transfer = []) => {
    start();
    used = true;
    Atomics.store(client.flag, 0, 0);
    const id = nextCall++;
    const deadline = performance.now() + 5000;
    client.port.postMessage({ id, request }, transfer);
    const reply = receiveReply(client.port, client.flag, id, () => deadline - performance.now());
    if (reply === null) throw new Error("output control reply timed out");
    return reply.result;
  };
  // Moves the direct queue, in order, to the thread: the head's unwritten
  // rest, then what waits behind it. Writes go there from now on.
  const handOff = () => {
    remote = true;
    for (const e of local.splice(0)) {
      const bytes = new Uint8Array(e.bytes.subarray(e.offset));
      const op = e.wait ? "wait_start" : "enqueue";
      e.remote = call({ op, fd: e.fd, bytes, started: e.started }, [bytes.buffer]).ok;
    }
  };
  // Writes the started head entries directly, until the queue is empty, its
  // head waits for a poll, or a descriptor is full.
  const pump = () => {
    while (local.length && local[0].started) {
      const e = local[0];
      try {
        while (e.offset < e.bytes.length) {
          const count = writeSync(e.fd, e.bytes, e.offset, e.bytes.length - e.offset);
          if (count === 0) return handOff();
          e.offset += count;
        }
        e.result = Ok(undefined);
      } catch (error) {
        if (full(error)) return handOff();
        e.result = failed(error);
      }
      local.shift();
    }
  };
  // Whether a new write may go direct: the thread, if it took over, has
  // written everything it was given.
  const direct = () => {
    if (!remote) return true;
    if (pinned || !call({ op: "idle" }).ok) return false;
    remote = false;
    return true;
  };
  const queue = (fd, text, wait) => {
    prepare();
    const e = { fd, bytes: Buffer.from(text), offset: 0, wait, started: !wait };
    e.result = e.remote = null;
    local.push(e);
    pump();
    return e;
  };
  const send = (fd, text) => {
    if (direct()) queue(fd, text, false);
    else call({ op: "enqueue", fd, text });
  };
  const output = {
    start,
    // Whether writes go through the writer thread now (for tests).
    threaded: () => remote,
    out: (text) => send(1, text),
    err: (text) => send(2, text),
    // A pending write: written from its first poll, in its place in the
    // queue, and ready at once when that direct write completes.
    write: (fd, text) => {
      const e = direct()
        ? queue(fd, text, true)
        : { remote: call({ op: "wait_start", fd, text }).ok };
      let result;
      return pendingOperation(
        () => {
          if (e.remote === null) {
            e.started = true;
            pump();
          }
          if (e.remote === null) {
            result = e.result;
            return result !== null;
          }
          const r = call({ op: "wait_poll", operation: e.remote });
          if (r.pending) return false;
          result = r.result;
          return true;
        },
        () => result,
        () => {
          if (e.remote !== null) return call({ op: "wait_abort", operation: e.remote });
          const i = local.indexOf(e);
          if (i >= 0) local.splice(i, 1);
          pump();
        },
        () => {
          if (e.remote !== null) call({ op: "wait_finish", operation: e.remote });
        },
      );
    },
    connection: () => {
      if (!remote) handOff();
      pinned = true;
      const { port1, port2 } = new MessageChannel();
      const flag = new Int32Array(new SharedArrayBuffer(4));
      call({ op: "attach", port: port2, flag }, [port2]);
      return { port: port1, flag };
    },
    drain: async () => {
      for (;;) {
        pump();
        if (!local.length && (client === null || call({ op: "idle" }).ok)) return;
        await new Promise((resolve) => setTimeout(resolve, 1));
      }
    },
    close: async () => {
      // A writer that was started but never written to has nothing to
      // flush; its unref'd thread need not delay the exit.
      if (used || local.length) await output.drain();
      client?.port.close();
      client = null;
      if (worker !== null) {
        if (used) await worker.terminate();
        else void worker.terminate();
      }
      worker = null;
    },
  };
  return output;
}
