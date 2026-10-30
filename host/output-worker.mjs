// The output writer thread of output.mjs: one ordered queue of stdout,
// stderr and session-frame writes, with synchronous console writes and short
// queue/status control requests. It imports only what it needs, so it boots
// fast when a full pipe first needs it.
import { postReply } from "./worker-control.mjs";
import { Err, Ok } from "./wire.mts";

// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.mjs).
const { write } = process.getBuiltinModule("node:fs");
const { workerData } = process.getBuiltinModule("node:worker_threads");

// A failed write's result.
export const failed = (error) =>
  Err(
    error.code === "EPIPE" || error.code === "ENOTCONN"
      ? { tag: "Closed" }
      : { tag: "Other", code: Number.isInteger(error?.errno) ? error.errno : 0 },
  );
// Whether a write error means "try again later": a full pipe.
export const full = (error) =>
  error?.code === "EAGAIN" || error?.code === "EWOULDBLOCK" || error?.code === "EINTR";

if (workerData?.output) {
  const queue = [];
  let active = false;
  const pump = () => {
    if (active || !queue.length) {
      return;
    }
    active = true;
    const state = queue[0];
    const finish = (error = null) => {
      const result = error ? failed(error) : Ok(undefined);
      queue.shift();
      active = false;
      if (state.reply !== null && !state.cancelled) {
        state.reply(result);
      }
      pump();
    };
    const step = () => {
      if (state.cancelled || state.offset === state.bytes.length) {
        return finish();
      }
      write(
        state.fd,
        state.bytes,
        state.offset,
        Math.min(65536, state.bytes.length - state.offset),
        (error, count) => {
          if (full(error)) {
            return setTimeout(step, 1);
          }
          if (error) {
            return finish(error);
          }
          state.offset += count;
          if (count === 0) {
            setTimeout(step, 1);
          } else {
            step();
          }
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
      for (const state of owned) {
        state.cancelled = true;
      }
      owned.clear();
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
          // `bytes`, when given, is the unwritten rest of a direct write.
          const state = {
            fd: request.fd,
            bytes: request.bytes ?? Buffer.from(request.text),
            offset: 0,
            cancelled: false,
            reply: null,
          };
          if (request.op === "wait_start") {
            state.reply = (written) => {
              owned.delete(state);
              postReply(port, flag, id, written);
            };
            owned.add(state);
          }
          queue.push(state);
          pump();
          if (request.op === "wait_start") {
            return;
          }
          result = { ok: null };
          break;
        }
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
