// One writer orders stdout, stderr and session frames. Control messages
// never wait on a full pipe. Accepted chunks remain queued after an abort;
// the guest discards only its result, and terminal drains retain the bytes.
import { write } from "node:fs";
import { MessageChannel, receiveMessageOnPort, Worker, workerData } from "node:worker_threads";
import { pendingOperation } from "./core.mjs";
import { Err, Ok } from "./wire.mjs";

export function createOutput(connection = null) {
  let client = connection;
  let worker = null;
  const call = (request, transfer = []) => {
    if (client === null) {
      const { port1, port2 } = new MessageChannel();
      const flag = new Int32Array(new SharedArrayBuffer(4));
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
    client.port.postMessage(request, transfer);
    if (Atomics.wait(client.flag, 0, 0, 5000) === "timed-out")
      throw new Error("output control reply timed out");
    return receiveMessageOnPort(client.port).message;
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
      if (state.offset === state.bytes.length) return finish();
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
  const attach = ({ port, flag }) =>
    port.on("message", (request) => {
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
          };
          if (request.op === "wait_start") operations.set(operation, state);
          queue.push(state);
          pump();
          result = { ok: operation };
          break;
        }
        case "wait_poll":
          result = operations.get(request.operation)?.result;
          result = result === null ? { pending: true } : { result };
          break;
        case "wait_finish":
        case "wait_abort":
          operations.delete(request.operation);
          result = { ok: null };
          break;
        case "idle":
          result = { ok: !active && queue.length === 0 };
          break;
        default:
          result = { error: "unknown output control request" };
      }
      port.postMessage(result);
      Atomics.store(flag, 0, 1);
      Atomics.notify(flag, 0);
    });
  attach(workerData);
}
