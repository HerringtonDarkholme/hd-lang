// A child's I/O belongs to this worker, so a synchronous hd:rt.block can
// keep polling sibling tasks. As with Net, control calls only start, probe,
// commit or cancel an operation; completion never posts unsolicited replies.
import { serve } from "./worker-control.mjs";
import { spawn } from "node:child_process";
import { constants } from "node:os";
import { workerData } from "node:worker_threads";

const operations = new Map();
// Aborted operations still own child reaping until the close event.
const children = new Set();
let nextOperation = 1;
const brokenInput = new Set(["EPIPE", "ECONNRESET", "ENOTCONN", "EOF"]);
const MAX_BUFFER = 1 << 30;
const failure = (error) => ({ error: { code: error.code, message: String(error.message) } });

function begin(request) {
  if (process.platform === "win32") {
    return beginWindows(request);
  }
  const state = { result: null, child: null, timer: null };
  let reaped;
  state.closed = new Promise((resolve) => {
    reaped = resolve;
  });
  const closed = () => {
    children.delete(state);
    reaped();
  };
  const kill = () => {
    clearTimeout(state.timer);
    const child = state.child;
    if (!child?.pid || state.result !== null) {
      return;
    }
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch (error) {
      if (error.code !== "ESRCH") {
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
      detached: process.platform !== "win32",
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    state.child = child;
    children.add(state);
    let error = null;
    let timeout = false;
    const stdout = [],
      stderr = [];
    let outSize = 0,
      errSize = 0;
    const failed = (e) => {
      error ??= e;
    };
    child.on("error", failed);
    child.stdin.on("error", (e) => {
      // A child that never started still reports its spawn error. A closed
      // reader drops remaining input, keeping the real output and status.
      if (!brokenInput.has(e.code)) {
        failed(e);
      }
    });
    const collect = (chunks, stream) => (bytes) => {
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
      clearTimeout(state.timer);
      closed();
      state.result = timeout
        ? { timeout: true }
        : error
          ? failure(error)
          : {
              ok: {
                stdout: Buffer.concat(stdout).toString("utf8"),
                stderr: Buffer.concat(stderr).toString("utf8"),
                status: status ?? 128 + (constants.signals[signal] ?? 0),
              },
            };
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
function beginWindows(request) {
  const state = { result: null, child: null, timer: null };
  let resolveClosed;
  state.closed = new Promise((resolve) => {
    resolveClosed = resolve;
  });
  const closed = () => {
    children.delete(state);
    resolveClosed();
  };
  let aborted = false;
  let settle = () => {};
  const abort = () => {
    clearTimeout(state.timer);
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
    let error = null,
      timeout = false,
      size = 0;
    const output = [];
    let exited = false,
      ended = false,
      framed = false;
    const failed = (reason) => {
      error ??= reason;
    };
    child.on("error", (reason) =>
      failed(new Error(`cannot start Windows processLauncher: ${reason.message}`)),
    );
    child.stdin.on("error", (reason) => {
      if (!brokenInput.has(reason.code)) {
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
      clearTimeout(state.timer);
      try {
        if (timeout) {
          state.result = { timeout: true };
        } else if (error) {
          state.result = failure(error);
        } else if (aborted) {
          state.result = failure(new Error("process cancelled"));
        } else {
          const result = JSON.parse(Buffer.concat(output).toString("utf8"));
          if (
            result?.ok &&
            typeof result.ok.stdout === "string" &&
            typeof result.ok.stderr === "string" &&
            Number.isInteger(result.ok.status)
          ) {
            state.result = result;
          } else if (
            result?.error &&
            typeof result.error.code === "string" &&
            typeof result.error.message === "string"
          ) {
            state.result = result;
          } else {
            throw new Error("invalid result");
          }
        }
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

const run = (r) => {
  switch (r.op) {
    case "wait_start": {
      const operation = nextOperation++;
      operations.set(operation, begin(r));
      return { ok: operation };
    }
    case "wait_poll": {
      const state = operations.get(r.operation);
      if (!state) {
        throw new Error("unknown process wait");
      }
      return state.result ?? { pending: true };
    }
    case "wait_finish": {
      const state = operations.get(r.operation);
      if (!state || state.result === null) {
        throw new Error("unfinished process wait");
      }
      operations.delete(r.operation);
      return { ok: null };
    }
    case "wait_abort":
      operations.get(r.operation)?.abort();
      operations.delete(r.operation);
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

const { port, flag } = workerData;
serve(port, flag, run, (request) => request.op === "close");
