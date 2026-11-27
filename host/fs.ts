// Special-file operations use the same short start/poll/finish/abort
// protocol as Net and Process. Each blocking OS call runs in a child:
// a FIFO open cannot be interrupted by terminating a Node worker, and
// blocked libuv pool jobs would also starve unrelated special-file calls.
// Only these uncommon calls start a child; regular files stay inline.
//
// A special file that `FsRead.open` opens (std-fs.open.special-file) gets
// a helper child of its own, which opens it and holds its descriptor for
// the file's life: the program thread never waits on the open or on a
// read. A worker thread could share the descriptor, but not as well:
// Node's exit joins a worker, or a libuv pool thread, blocked in a read
// until the peer writes, and a readiness-driven stream never sees a
// FIFO's end on macOS, whose kqueue and poll report no event when the
// last writer closes (as Go issue #24164 found; Go reads such a FIFO with
// a blocking thread too). So the child reads with blocking calls, one
// request at a time, and disposing the file kills it, which releases the
// descriptor at once: every read still pending answers `Closed` and no
// later byte reaches the program (std-io.read.pending-closed).
// cost: one child process per open special file, as each other
// special-file call already starts, then per read one IPC message each
// way and one read system call.
import { serve, notifyResult } from "./worker-control.ts";
import type { Flag } from "./worker-control.ts";
import { fork } from "node:child_process";
import { parentPort, Worker, workerData } from "node:worker_threads";
import { fileOperation, readSpecial } from "./node-files.ts";
import type { FileJob } from "./node-files.ts";
import { Err, Ok } from "./wire.ts";
import type { ChildProcess, Serializable } from "node:child_process";
import type { MessagePort } from "node:worker_threads";

type Outcome = { result: unknown } | null;

interface FileState {
  result: Outcome;
  child: ChildProcess | null;
  closed: Promise<void>;
  abort: () => void;
}

// A read of an open special file. An aborted one is still answered by
// the child, and its bytes are dropped.
interface ReadState {
  result: Outcome;
  file: number;
  aborted: boolean;
}

// An open special file: its helper child, the reads sent to it, which it
// answers in order, and the answered reads the program has not taken.
interface SpecialFile {
  helper: FileState;
  asked: ReadState[];
  answered: Set<ReadState>;
  // The failure every read gets once the child died undisposed.
  stopped: Outcome;
}

type FileControlRequest =
  | ({ op: "wait_start" } & FileJob)
  | { op: "read_start"; file: number; max: number }
  | { op: "file_dispose"; file: number }
  | { op: "wait_poll" | "wait_finish" | "wait_abort"; operation: number }
  | { op: "close" };

interface ControlWorkerData {
  port: MessagePort;
  flag: Flag;
  reactor?: Flag;
}

type SpecialJob = FileJob | { fd: number };
interface JobWorkerData {
  job: SpecialJob;
}

const data = workerData as ControlWorkerData | JobWorkerData | null;
const message = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);
const disconnect = (): void => process.disconnect!();
const sendAndDisconnect = (result: unknown): void => {
  process.send!(result as Serializable, disconnect);
};
// An opened file or inherited descriptor keeps its child until disposal.
const opensFile = (job: SpecialJob): boolean =>
  "fd" in job || (job.module === "hd:FsRead" && job.name === "open");

if (data && "port" in data) {
  const operations = new Map<number, FileState | ReadState>();
  const children = new Set<FileState>();
  // The open special files, by a number `open` answers with once.
  const files = new Map<number, SpecialFile>();
  // The file an open made that the program has not taken yet.
  const opening = new Map<FileState, number>();
  let nextOperation = 1;
  let nextFile = 1;
  const failure = (error: unknown): Outcome => ({
    result: Err({ tag: "Other", message: message(error) }),
  });
  // Disposes an open special file: each read on it that the program has
  // not taken, answered or not, answers `Closed`
  // (std-io.read.pending-closed). Its child is killed, which closes the
  // descriptor, and no later answer reaches a read.
  // cost: O(the reads pending on the file).
  const dispose = (number: number): void => {
    const file = files.get(number);
    if (!file) {
      return;
    }
    files.delete(number);
    for (const read of [...file.asked, ...file.answered]) {
      read.result = { result: Err({ tag: "Closed" }) };
    }
    file.asked.length = 0;
    file.answered.clear();
    file.helper.abort();
  };
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
    const keeps = opensFile(request);
    // The file this child holds, once its open succeeded.
    let number: number | null = null;
    try {
      const child = fork(new URL(import.meta.url), [], {
        execArgv: [],
        serialization: "advanced",
        stdio: ["ignore", "ignore", "ignore", "ipc"],
        windowsHide: true,
      });
      state.child = child;
      children.add(state);
      // A kept child's first answer is its open; each later one answers
      // the oldest read sent to it. A disposed file's are dropped.
      child.on("message", (result: { ok: boolean }) => {
        if (number !== null) {
          const file = files.get(number);
          const read = file?.asked.shift();
          if (file && read && !read.aborted) {
            read.result = { result };
            file.answered.add(read);
          }
          return;
        }
        if (keeps && result.ok) {
          number = nextFile++;
          files.set(number, { helper: state, asked: [], answered: new Set(), stopped: null });
          opening.set(state, number);
          state.result ??= { result: Ok(number) };
          return;
        }
        state.result ??= { result };
        if (keeps) {
          state.abort();
        }
      });
      child.on("error", (error) => {
        state.result ??= failure(error);
      });
      child.on("close", (code, signal) => {
        const stopped = failure(`file helper stopped (${signal ?? code})`);
        state.result ??= stopped;
        const file = number === null ? undefined : files.get(number);
        if (file) {
          file.stopped = stopped;
          for (const read of file.asked.splice(0)) {
            read.result = stopped;
          }
        }
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
  // One read of at most `max` bytes, which the file's child makes after
  // the reads sent before it. cost: one IPC message each way.
  const read = (number: number, max: number): ReadState => {
    const state: ReadState = { result: null, file: number, aborted: false };
    const file = files.get(number);
    if (!file) {
      state.result = { result: Err({ tag: "Closed" }) };
    } else if (file.stopped) {
      state.result = file.stopped;
    } else {
      file.asked.push(state);
      file.helper.child!.send({ max } as Serializable, (error) => {
        if (error && state.result === null) {
          state.result = failure(error);
        }
      });
    }
    return state;
  };
  const started = (state: FileState | ReadState): { ok: number } => {
    const operation = nextOperation++;
    operations.set(operation, notifyResult(state, data, `wait:${operation}`));
    return { ok: operation };
  };
  // The result the program takes: a dispose since the poll has made a
  // read's `Closed`.
  const finish = (operation: number): Outcome => {
    const state = operations.get(operation);
    if (!state || state.result === null) {
      throw new Error("unfinished file wait");
    }
    operations.delete(operation);
    if ("aborted" in state) {
      files.get(state.file)?.answered.delete(state);
    } else {
      opening.delete(state);
    }
    return state.result;
  };
  const abort = (operation: number): void => {
    const state = operations.get(operation);
    operations.delete(operation);
    if (state && "aborted" in state) {
      state.aborted = true;
      files.get(state.file)?.answered.delete(state);
    } else if (state) {
      // An open the program never took leaves no file behind.
      const number = opening.get(state);
      opening.delete(state);
      if (number !== undefined) {
        dispose(number);
      }
      state.abort();
    }
  };
  const run = (request: FileControlRequest): unknown | Promise<{ ok: null }> => {
    switch (request.op) {
      case "wait_start":
        return started(begin(request));
      case "read_start":
        return started(read(request.file, request.max));
      case "file_dispose":
        dispose(request.file);
        return { ok: null };
      case "wait_poll": {
        const state = operations.get(request.operation);
        if (!state) {
          throw new Error("unknown file wait");
        }
        return state.result ?? { pending: true };
      }
      case "wait_finish":
        return finish(request.operation);
      case "wait_abort":
        abort(request.operation);
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
  const job = data.job;
  // An inherited descriptor is already open; stdin keeps its original
  // access mode and error codes without reopening a filesystem path.
  const result = ("fd" in job ? Ok(job.fd) : fileOperation(job.module, job.name, job.args)) as {
    ok: boolean;
    value: unknown;
  };
  if (!opensFile(data.job) || !result.ok) {
    parentPort!.postMessage(result);
  } else {
    // This thread holds the open file and reads it, one request at a
    // time, until the process ends.
    const fd = result.value as number;
    parentPort!.postMessage(Ok(null));
    parentPort!.on("message", ({ max }: { max: number }) => {
      parentPort!.postMessage(readSpecial(fd, max));
    });
  }
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
    const job = childMessage as SpecialJob;
    const operationWorker = new Worker(new URL(import.meta.url), {
      workerData: { job },
    });
    worker = operationWorker;
    operationWorker.on("error", (error: Error) => {
      sendAndDisconnect(Err({ tag: "Other", message: error.message }));
    });
    if (opensFile(job)) {
      // The open's answer, then one answer per read request; the parent
      // kills this process when the file is disposed.
      operationWorker.on("message", (result) => {
        process.send!(result as Serializable);
      });
      process.on("message", (request) => operationWorker.postMessage(request));
      return;
    }
    operationWorker.on("message", (result) => {
      // Sending the result is asynchronous; wait for the worker's exit
      // before disconnecting so ordinary completion is never killed.
      operationWorker.once("exit", () => sendAndDisconnect(result));
    });
  });
}
