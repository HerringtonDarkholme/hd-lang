// One writer orders stdout, stderr and session frames. Control messages
// never wait on a full pipe. Aborting a pending write drops its unwritten
// suffix; bytes already passed to the stream cannot be undone.
import { write } from "node:fs";
import { MessageChannel, Worker, workerData } from "node:worker_threads";
import { postReply, receiveReply } from "./worker-control.mjs";
import { pendingOperation } from "./core.mjs";
import { Err, Ok } from "./wire.mjs";

export function createOutput(connection = null) {
  let client = connection;
  let worker = null;
  let nextCall = 1;
  const call = (request, transfer = []) => {
    if (client === null) {
      const { port1, port2 } = new MessageChannel();
      const flag = new Int32Array(new SharedArrayBuffer(4));
      // Construct the main thread's pipe handles before the worker uses their
      // descriptors: libuv makes pipes nonblocking, so an in-flight write
      // cannot keep writing a cancelled suffix when a reader resumes.
      process.stdout._handle?.setBlocking(false);
      process.stderr._handle?.setBlocking(false);
      worker = new Worker(new URL(import.meta.url), {
        execArgv: [],
        workerData: { output: true, port: port2, flag },
        transferList: [port2],
      });
      worker.unref();
      port1.unref();
      client = { port: port1, flag };
    }
    Atomics.store(client.flag, 0, 0);
    const id = nextCall++;
    const deadline = performance.now() + 5000;
    client.port.postMessage({ id, request }, transfer);
    const reply = receiveReply(client.port, client.flag, id, () => deadline - performance.now());
    if (reply === null) throw new Error("output control reply timed out");
    return reply.result;
  };
  const send = (fd, text) => call({ op: "enqueue", fd, text });
  const output = {
    out: (text) => send(1, text),
    err: (text) => send(2, text),
    write: (fd, text) => {
      const { ok: operation } = call({ op: "wait_start", fd, text });
      let result;
      return pendingOperation(
        () => {
          const r = call({ op: "wait_poll", operation });
          if (r.pending) return false;
          result = r.result;
          return true;
        },
        () => result,
        () => call({ op: "wait_abort", operation }),
        () => {
          call({ op: "wait_finish", operation });
        },
      );
    },
    connection: () => {
      const { port1, port2 } = new MessageChannel();
      const flag = new Int32Array(new SharedArrayBuffer(4));
      call({ op: "attach", port: port2, flag }, [port2]);
      return { port: port1, flag };
    },
    drain: async () => {
      if (client === null) return;
      while (!call({ op: "idle" }).ok) await new Promise((resolve) => setTimeout(resolve, 1));
    },
    close: async () => {
      await output.drain();
      client?.port.close();
      client = null;
      if (worker !== null) await worker.terminate();
      worker = null;
    },
  };
  return output;
}

if (workerData?.output) {
  const queue = [];
  const operations = new Map();
  let nextOperation = 1;
  let active = false;
  const pump = () => {
    if (active || !queue.length) return;
    if (!queue[0].started && !queue[0].cancelled) return;
    active = true;
    const state = queue[0];
    const finish = (error = null) => {
      state.result = error
        ? Err(
            error.code === "EPIPE" || error.code === "ENOTCONN"
              ? { tag: "Closed" }
              : { tag: "Other", message: String(error.message) },
          )
        : Ok(undefined);
      queue.shift();
      active = false;
      pump();
    };
    const step = () => {
      if (state.cancelled || state.offset === state.bytes.length) return finish();
      write(
        state.fd,
        state.bytes,
        state.offset,
        Math.min(65536, state.bytes.length - state.offset),
        (error, count) => {
          if (error?.code === "EAGAIN" || error?.code === "EWOULDBLOCK" || error?.code === "EINTR")
            return setTimeout(step, 1);
          if (error) return finish(error);
          state.offset += count;
          if (count === 0) setTimeout(step, 1);
          else step();
        },
      );
    };
    step();
  };
  const attach = ({ port, flag }) => {
    const owned = new Set();
    // A supervised guest may be terminated without running its abort hooks.
    // Its closed connection must not leave pending output in this shared queue.
    port.on("close", () => {
      for (const operation of owned) {
        const state = operations.get(operation);
        if (state) state.cancelled = true;
        operations.delete(operation);
      }
      pump();
    });
    port.on("message", ({ id, request }) => {
      let result;
      switch (request.op) {
        case "attach":
          attach(request);
          result = { ok: null };
          break;
        case "enqueue":
        case "wait_start": {
          const operation = nextOperation++;
          const state = {
            fd: request.fd,
            bytes: Buffer.from(request.text),
            offset: 0,
            result: null,
            started: request.op === "enqueue",
            cancelled: false,
          };
          if (request.op === "wait_start") {
            operations.set(operation, state);
            owned.add(operation);
          }
          queue.push(state);
          pump();
          result = { ok: operation };
          break;
        }
        case "wait_poll": {
          const state = operations.get(request.operation);
          if (state) state.started = true;
          pump();
          result = state?.result;
          result = result === null ? { pending: true } : { result };
          break;
        }
        case "wait_abort": {
          const state = operations.get(request.operation);
          if (state) state.cancelled = true;
          owned.delete(request.operation);
          operations.delete(request.operation);
          pump();
          result = { ok: null };
          break;
        }
        case "wait_finish":
          owned.delete(request.operation);
          operations.delete(request.operation);
          result = { ok: null };
          break;
        case "idle":
          result = { ok: !active && queue.length === 0 };
          break;
        default:
          result = { error: "unknown output control request" };
      }
      postReply(port, flag, id, result);
    });
  };
  attach(workerData);
}
