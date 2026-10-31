// Special-file operations use the same short start/poll/finish/abort
// protocol as Net and Process. Each blocking OS call runs in a child:
// a FIFO open cannot be interrupted by terminating a Node worker, and
// blocked libuv pool jobs would also starve unrelated special-file calls.
// Only these uncommon calls start a child; regular files stay inline.
import { serve } from "./worker-control.ts";
import type { Flag } from "./worker-control.ts";
import { fork } from "node:child_process";
import { parentPort, Worker, workerData } from "node:worker_threads";
import { fileOperation } from "./node-files.ts";
import type { FileJob } from "./node-files.ts";
import { Err } from "./wire.ts";
import type { ChildProcess, Serializable } from "node:child_process";
import type { MessagePort } from "node:worker_threads";

interface FileState {
  result: { result: unknown } | null;
  child: ChildProcess | null;
  closed: Promise<void>;
  abort: () => void;
}

type FileControlRequest =
  | ({ op: "wait_start" } & FileJob)
  | { op: "wait_poll" | "wait_finish" | "wait_abort"; operation: number }
  | { op: "close" };

interface ControlWorkerData {
  port: MessagePort;
  flag: Flag;
}

interface JobWorkerData {
  job: FileJob;
}

const data = workerData as ControlWorkerData | JobWorkerData | null;
const message = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);
const disconnect = (): void => process.disconnect!();
const sendAndDisconnect = (result: unknown): void => {
  process.send!(result as Serializable, disconnect);
};

if (data && "port" in data) {
  const operations = new Map<number, FileState>();
  const children = new Set<FileState>();
  let nextOperation = 1;
  const failure = (error: unknown): FileState["result"] => ({
    result: Err({ tag: "Other", message: message(error) }),
  });
  const begin = (request: FileJob): FileState => {
    let reaped!: () => void;
    const state: FileState = {
      result: null,
      child: null,
      closed: new Promise<void>((resolve) => {
        reaped = resolve;
      }),
      abort: () => {},
    };
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
      child.send(request as Serializable, (error) => {
        if (error) {
          state.result ??= failure(error);
          state.abort();
        }
      });
    } catch (error) {
      state.result = failure(error);
      state.abort();
      if (state.child === null) {
        reaped();
      }
    }
    return state;
  };
  const run = (request: FileControlRequest): unknown | Promise<{ ok: null }> => {
    switch (request.op) {
      case "wait_start": {
        const operation = nextOperation++;
        operations.set(operation, begin(request));
        return { ok: operation };
      }
      case "wait_poll": {
        const state = operations.get(request.operation);
        if (!state) {
          throw new Error("unknown file wait");
        }
        return state.result ?? { pending: true };
      }
      case "wait_finish": {
        const state = operations.get(request.operation);
        if (!state || state.result === null) {
          throw new Error("unfinished file wait");
        }
        operations.delete(request.operation);
        return { ok: null };
      }
      case "wait_abort":
        operations.get(request.operation)?.abort();
        operations.delete(request.operation);
        return { ok: null };
      case "close": {
        const waiting = [...children];
        for (const state of waiting) {
          state.abort();
        }
        operations.clear();
        return Promise.all(waiting.map((state) => state.closed)).then(() => ({ ok: null }));
      }
      default:
        throw new Error("unknown file control call");
    }
  };
  const { port, flag } = data;
  serve(port, flag, run, (request) => request.op === "close");
} else if (data && "job" in data) {
  const { module, name, args } = data.job;
  parentPort!.postMessage(fileOperation(module, name, args));
} else {
  // Keep this child's IPC loop responsive while its one OS call blocks.
  // If a supervisor terminates the control worker, its IPC closes and
  // this process kills itself, including the blocked thread. process.exit
  // would wait for that thread. An in-flight write may have taken effect.
  let worker: Worker | null = null;
  process.once("disconnect", () => {
    if (worker !== null && worker.threadId !== -1) {
      process.kill(process.pid, "SIGKILL");
    }
  });
  process.once("message", (childMessage) => {
    if (!process.connected) {
      return;
    }
    const { module, name, args } = childMessage as FileJob;
    const operationWorker = new Worker(new URL(import.meta.url), {
      workerData: { job: { module, name, args } },
    });
    worker = operationWorker;
    operationWorker.on("message", (result) => {
      // Sending the result is asynchronous; wait for the worker's exit
      // before disconnecting so ordinary completion is never killed.
      operationWorker.once("exit", () => sendAndDisconnect(result));
    });
    operationWorker.on("error", (error: Error) => {
      sendAndDisconnect(Err({ tag: "Other", message: error.message }));
    });
  });
}
