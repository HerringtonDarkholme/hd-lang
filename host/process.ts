// A child's I/O belongs to this worker, so a synchronous hd:rt.block can
// keep polling sibling tasks. As with Net, control calls only start, probe,
// commit or cancel an operation; completion never posts unsolicited replies.
import { serve } from "./worker-control.ts";
import type { Flag } from "./worker-control.ts";
import { spawn } from "node:child_process";
import { constants } from "node:os";
import { workerData } from "node:worker_threads";
import type { ChildProcessWithoutNullStreams } from "node:child_process";
import type { MessagePort } from "node:worker_threads";

interface SystemError extends Error {
  code?: string;
}

interface ProcessOutput {
  stdout: string;
  stderr: string;
  status: number;
}

type ProcessResult =
  | { timeout: true }
  | { error: { code?: string; message: string } }
  | { ok: ProcessOutput };

interface ProcessStartRequest {
  op: "wait_start";
  launcher: string | null;
  program: string;
  args: string[];
  cwd: string;
  env: NodeJS.ProcessEnv;
  input: string;
  timeout: number;
}

type ProcessControlRequest =
  | ProcessStartRequest
  | { op: "wait_poll" | "wait_finish" | "wait_abort"; operation: number }
  | { op: "close" };

interface ProcessState {
  result: ProcessResult | null;
  child: ChildProcessWithoutNullStreams | null;
  timer: NodeJS.Timeout | null;
  closed: Promise<void>;
  abort: () => void;
}

interface WindowsStatus {
  timeout: boolean;
  error: SystemError | null;
  aborted: boolean;
}

interface ProcessWorkerData {
  port: MessagePort;
  flag: Flag;
}

const systemError = (error: unknown): SystemError =>
  error instanceof Error ? (error as SystemError) : new Error(String(error));
const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null;
const clearTimer = (state: ProcessState): void => {
  if (state.timer !== null) {
    clearTimeout(state.timer);
  }
};
const detached = (): boolean => process.platform !== "win32";

const operations = new Map<number, ProcessState>();
// Aborted operations still own child reaping until the close event.
const children = new Set<ProcessState>();
let nextOperation = 1;
const brokenInput = new Set(["EPIPE", "ECONNRESET", "ENOTCONN", "EOF"]);
const MAX_BUFFER = 1 << 30;
const failure = (error: unknown): ProcessResult => {
  const detail = systemError(error);
  return { error: { code: detail.code, message: detail.message } };
};

function windowsResult(output: readonly Buffer[], status: WindowsStatus): ProcessResult {
  if (status.timeout) {
    return { timeout: true };
  }
  if (status.error) {
    return failure(status.error);
  }
  if (status.aborted) {
    return failure(new Error("process cancelled"));
  }
  const result: unknown = JSON.parse(Buffer.concat(output).toString("utf8"));
  const ok = record(result) ? result.ok : undefined;
  if (
    record(ok) &&
    typeof ok.stdout === "string" &&
    typeof ok.stderr === "string" &&
    Number.isInteger(ok.status)
  ) {
    return result as { ok: ProcessOutput };
  }
  const error = record(result) ? result.error : undefined;
  if (record(error) && typeof error.code === "string" && typeof error.message === "string") {
    return result as { error: { code: string; message: string } };
  }
  throw new Error("invalid result");
}

function begin(request: ProcessStartRequest): ProcessState {
  if (process.platform === "win32") {
    return beginWindows(request);
  }
  let reaped!: () => void;
  const state: ProcessState = {
    result: null,
    child: null,
    timer: null,
    closed: new Promise<void>((resolve) => {
      reaped = resolve;
    }),
    abort: () => {},
  };
  const closed = () => {
    children.delete(state);
    reaped();
  };
  const kill = () => {
    clearTimer(state);
    const child = state.child;
    if (!child?.pid || state.result !== null) {
      return;
    }
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch (error) {
      if (systemError(error).code !== "ESRCH") {
        throw error;
      }
    }
    child.stdin.destroy();
    child.stdout.destroy();
    child.stderr.destroy();
  };
  state.abort = kill;
  try {
    const child = spawn(request.program, request.args, {
      cwd: request.cwd,
      env: request.env,
      detached: detached(),
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    state.child = child;
    children.add(state);
    let error: SystemError | null = null;
    let timeout = false;
    const stdout: Buffer[] = [],
      stderr: Buffer[] = [];
    let outSize = 0,
      errSize = 0;
    const failed = (reason: unknown) => {
      error ??= systemError(reason);
    };
    child.on("error", failed);
    child.stdin.on("error", (reason) => {
      // A child that never started still reports its spawn error. A closed
      // reader drops remaining input, keeping the real output and status.
      if (!brokenInput.has(systemError(reason).code ?? "")) {
        failed(reason);
      }
    });
    const collect = (chunks: Buffer[], stream: "stdout" | "stderr") => (bytes: Buffer) => {
      if (stream === "stdout") {
        outSize += bytes.length;
      } else {
        errSize += bytes.length;
      }
      if (outSize > MAX_BUFFER || errSize > MAX_BUFFER) {
        failed(Object.assign(new Error("child output exceeds maxBuffer"), { code: "ENOBUFS" }));
        kill();
      } else {
        chunks.push(bytes);
      }
    };
    child.stdout.on("data", collect(stdout, "stdout"));
    child.stderr.on("data", collect(stderr, "stderr"));
    child.stdout.on("error", failed);
    child.stderr.on("error", failed);
    child.on("close", (status, signal) => {
      clearTimer(state);
      closed();
      if (timeout) {
        state.result = { timeout: true };
      } else if (error) {
        state.result = failure(error);
      } else {
        state.result = {
          ok: {
            stdout: Buffer.concat(stdout).toString("utf8"),
            stderr: Buffer.concat(stderr).toString("utf8"),
            status: status ?? 128 + (constants.signals[signal as NodeJS.Signals] ?? 0),
          },
        };
      }
    });
    if (Number.isFinite(request.timeout)) {
      const deadline = Date.now() + request.timeout;
      const expire = () => {
        const left = deadline - Date.now();
        if (left <= 0) {
          timeout = true;
          kill();
        } else {
          state.timer = setTimeout(expire, Math.min(left, 2_147_483_647));
        }
      };
      expire();
    }
    child.stdin.end(request.input);
  } catch (error) {
    kill();
    state.result = failure(error);
    if (!state.child) {
      closed();
    }
  }
  return state;
}

// The helper creates the child suspended and assigns its Job before resuming
// it. Closing this lifetime pipe asks the helper to kill and reap the Job.
// Terminating the helper during spawn could strand an unassigned child.
function beginWindows(request: ProcessStartRequest): ProcessState {
  let resolveClosed!: () => void;
  const state: ProcessState = {
    result: null,
    child: null,
    timer: null,
    closed: new Promise<void>((resolve) => {
      resolveClosed = resolve;
    }),
    abort: () => {},
  };
  const closed = () => {
    children.delete(state);
    resolveClosed();
  };
  let aborted = false;
  let settle = () => {};
  const abort = () => {
    clearTimer(state);
    if (aborted || state.result !== null) {
      return;
    }
    aborted = true;
    state.child?.stdin.destroy();
    settle();
  };
  state.abort = abort;
  if (typeof request.launcher !== "string" || request.launcher.length === 0) {
    state.result = failure(new Error("Windows Process requires an explicit processLauncher path"));
    closed();
    return state;
  }
  try {
    const child = spawn(request.launcher, ["--process-job"], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    state.child = child;
    children.add(state);
    let error: SystemError | null = null,
      timeout = false,
      size = 0;
    const output: Buffer[] = [];
    let exited = false,
      ended = false,
      framed = false;
    const failed = (reason: unknown) => {
      error ??= systemError(reason);
    };
    child.on("error", (reason) =>
      failed(new Error(`cannot start Windows processLauncher: ${reason.message}`)),
    );
    child.stdin.on("error", (reason) => {
      if (!brokenInput.has(systemError(reason).code ?? "")) {
        failed(reason);
      }
    });
    child.stdout.on("error", failed);
    child.stderr.on("error", failed);
    // Each guest stream is independently bounded by the helper. JSON can
    // expand one byte to six, on both streams; its framing is not guest data.
    child.stdout.on("data", (bytes) => {
      size += bytes.length;
      if (size > 12 * MAX_BUFFER + 1024) {
        failed(new Error("Windows processLauncher output exceeds its protocol bound"));
        abort();
      } else {
        output.push(bytes);
        framed ||= bytes.includes(10);
        settle();
      }
    });
    child.stderr.resume();
    settle = () => {
      if (
        state.result !== null ||
        !exited ||
        (!framed && !ended && !timeout && !error && !aborted)
      ) {
        return;
      }
      clearTimer(state);
      try {
        state.result = windowsResult(output, { timeout, error, aborted });
      } catch {
        state.result = failure(new Error("invalid Windows processLauncher result"));
      } finally {
        // A Windows descendant can inherit the helper's protocol handles.
        // The framed reply already contains all guest output. After the
        // helper exits, waiting for protocol EOF would wait for unrelated
        // detached descendants too (including a live HTTP fixture server).
        child.stdin.destroy();
        child.stdout.destroy();
        child.stderr.destroy();
        closed();
      }
    };
    child.on("exit", (status, signal) => {
      exited = true;
      if (status !== 0 && !aborted && !timeout) {
        failed(new Error(`Windows processLauncher exited with ${signal ?? status}`));
      }
      settle();
    });
    child.stdout.on("end", () => {
      ended = true;
      settle();
    });
    child.on("close", () => {
      exited = true;
      ended = true;
      settle();
    });
    // Keep stdin open after the single request; EOF is cancellation, not the
    // end of the guest's stdin (the helper owns that separately).
    child.stdin.write(JSON.stringify(request) + "\n");
    if (Number.isFinite(request.timeout)) {
      const deadline = Date.now() + request.timeout;
      const expire = () => {
        const left = deadline - Date.now();
        if (left <= 0) {
          timeout = true;
          abort();
        } else {
          state.timer = setTimeout(expire, Math.min(left, 2_147_483_647));
        }
      };
      expire();
    }
  } catch (reason) {
    abort();
    state.result = failure(reason);
    if (!state.child) {
      closed();
    }
  }
  return state;
}

const run = (request: ProcessControlRequest): unknown | Promise<{ ok: null }> => {
  switch (request.op) {
    case "wait_start": {
      const operation = nextOperation++;
      operations.set(operation, begin(request));
      return { ok: operation };
    }
    case "wait_poll": {
      const state = operations.get(request.operation);
      if (!state) {
        throw new Error("unknown process wait");
      }
      return state.result ?? { pending: true };
    }
    case "wait_finish": {
      const state = operations.get(request.operation);
      if (!state || state.result === null) {
        throw new Error("unfinished process wait");
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
      throw new Error("unknown process control call");
  }
};

const { port, flag } = workerData as ProcessWorkerData;
serve(port, flag, run, (request) => request.op === "close");
