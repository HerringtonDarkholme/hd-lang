// Special-file operations use the same short start/poll/finish/abort
// protocol as Net and Process. Each blocking OS call runs in a child:
// a FIFO open cannot be interrupted by terminating a Node worker, and
// blocked libuv pool jobs would also starve unrelated special-file calls.
// Only these uncommon calls start a child; regular files stay inline.
import { serve } from "./worker-control.mjs";
import { fork } from "node:child_process";
import { parentPort, Worker, workerData } from "node:worker_threads";
import { fileOperation } from "./node-files.mjs";
import { Err } from "./wire.mjs";

if (workerData?.port) {
  const operations = new Map();
  const children = new Set();
  let nextOperation = 1;
  const failure = (error) => ({
    result: Err({ tag: "Other", message: String(error?.message ?? error) }),
  });
  const begin = (request) => {
    const state = { result: null, child: null };
    let reaped;
    state.closed = new Promise((resolve) => {
      reaped = resolve;
    });
    state.abort = () => state.child?.kill("SIGKILL");
    try {
      const child = fork(new URL(import.meta.url), [], {
        execArgv: [],
        serialization: "advanced",
        stdio: ["ignore", "ignore", "ignore", "ipc"],
        windowsHide: true,
      });
      state.child = child;
      children.add(state);
      child.on("message", (result) => {
        state.result ??= { result };
      });
      child.on("error", (error) => {
        state.result ??= failure(error);
      });
      child.on("close", (code, signal) => {
        state.result ??= failure(`file helper stopped (${signal ?? code})`);
        children.delete(state);
        reaped();
      });
      child.send(request, (error) => {
        if (error) {
          state.result ??= failure(error);
          state.abort();
        }
      });
    } catch (error) {
      state.result = failure(error);
      state.abort();
      if (state.child === null) reaped();
    }
    return state;
  };
  const run = (request) => {
    switch (request.op) {
      case "wait_start": {
        const operation = nextOperation++;
        operations.set(operation, begin(request));
        return { ok: operation };
      }
      case "wait_poll": {
        const state = operations.get(request.operation);
        if (!state) throw new Error("unknown file wait");
        return state.result ?? { pending: true };
      }
      case "wait_finish": {
        const state = operations.get(request.operation);
        if (!state || state.result === null) throw new Error("unfinished file wait");
        operations.delete(request.operation);
        return { ok: null };
      }
      case "wait_abort":
        operations.get(request.operation)?.abort();
        operations.delete(request.operation);
        return { ok: null };
      case "close": {
        const waiting = [...children];
        for (const state of waiting) state.abort();
        operations.clear();
        return Promise.all(waiting.map((state) => state.closed)).then(() => ({ ok: null }));
      }
      default:
        throw new Error("unknown file control call");
    }
  };
  const { port, flag } = workerData;
  serve(port, flag, run, (request) => request.op === "close");
} else if (workerData?.job) {
  const { module, name, args } = workerData.job;
  parentPort.postMessage(fileOperation(module, name, args));
} else {
  // Keep this child's IPC loop responsive while its one OS call blocks.
  // If a supervisor terminates the control worker, its IPC closes and
  // this process kills itself, including the blocked thread. process.exit
  // would wait for that thread. An in-flight write may have taken effect.
  let worker = null;
  process.once("disconnect", () => {
    if (worker !== null && worker.threadId !== -1) process.kill(process.pid, "SIGKILL");
  });
  process.once("message", ({ module, name, args }) => {
    if (!process.connected) return;
    worker = new Worker(new URL(import.meta.url), { workerData: { job: { module, name, args } } });
    worker.on("message", (result) => {
      // Sending the result is asynchronous; wait for the worker's exit
      // before disconnecting so ordinary completion is never killed.
      worker.once("exit", () => process.send(result, () => process.disconnect()));
    });
    worker.on("error", (error) => {
      process.send(Err({ tag: "Other", message: String(error.message) }), () =>
        process.disconnect(),
      );
    });
  });
}
