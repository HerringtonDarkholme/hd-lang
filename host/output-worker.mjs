// The output writer thread of output.mjs: one ordered queue of stdout,
// stderr and session-frame writes, with short control requests. It imports
// only what it needs, so it boots fast when a full pipe first needs it.
import { write } from "node:fs";
import { workerData } from "node:worker_threads";
import { postReply } from "./worker-control.mjs";
import { Err, Ok } from "./wire.mjs";

// A failed write's result.
export const failed = (error) =>
  Err(
    error.code === "EPIPE" || error.code === "ENOTCONN"
      ? { tag: "Closed" }
      : { tag: "Other", message: String(error.message) },
  );
// Whether a write error means "try again later": a full pipe.
export const full = (error) =>
  error?.code === "EAGAIN" || error?.code === "EWOULDBLOCK" || error?.code === "EINTR";

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
      state.result = error ? failed(error) : Ok(undefined);
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
          if (full(error)) return setTimeout(step, 1);
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
          // `bytes`, when given, is the unwritten rest of a direct write,
          // `started` whether it was already polled.
          const operation = nextOperation++;
          const state = {
            fd: request.fd,
            bytes: request.bytes ?? Buffer.from(request.text),
            offset: 0,
            result: null,
            started: request.op === "enqueue" || request.started === true,
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
