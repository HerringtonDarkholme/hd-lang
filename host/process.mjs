// A child's I/O belongs to this worker, so a synchronous hd:rt.block can
// keep polling sibling tasks. As with Net, control calls only start, probe,
// commit or cancel an operation; completion never posts unsolicited replies.
import { postReply } from "./worker-control.mjs";
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
    if (!child?.pid || state.result !== null) return;
    if (process.platform === "win32") {
      // Node has no Job-object API. Keep tree cancellation while the parent
      // is alive; descendants surviving its exit remain tracked by #819.
      const killer = spawn("taskkill", ["/pid", String(child.pid), "/t", "/f"], {
        stdio: "ignore",
        windowsHide: true,
      });
      killer.on("error", () => child.kill("SIGKILL"));
    } else {
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
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
      if (!brokenInput.has(e.code)) failed(e);
    });
    const collect = (chunks, stream) => (bytes) => {
      if (stream === "stdout") outSize += bytes.length;
      else errSize += bytes.length;
      if (outSize > MAX_BUFFER || errSize > MAX_BUFFER) {
        failed(Object.assign(new Error("child output exceeds maxBuffer"), { code: "ENOBUFS" }));
        kill();
      } else chunks.push(bytes);
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
        } else state.timer = setTimeout(expire, Math.min(left, 2_147_483_647));
      };
      expire();
    }
    child.stdin.end(request.input);
  } catch (error) {
    kill();
    state.result = failure(error);
    if (!state.child) closed();
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
      if (!state) throw new Error("unknown process wait");
      return state.result ?? { pending: true };
    }
    case "wait_finish": {
      const state = operations.get(r.operation);
      if (!state || state.result === null) throw new Error("unfinished process wait");
      operations.delete(r.operation);
      return { ok: null };
    }
    case "wait_abort":
      operations.get(r.operation)?.abort();
      operations.delete(r.operation);
      return { ok: null };
    case "close": {
      const waiting = [...children];
      for (const state of waiting) state.abort();
      operations.clear();
      return Promise.all(waiting.map((state) => state.closed)).then(() => ({ ok: null }));
    }
    default:
      throw new Error("unknown process control call");
  }
};

const { port, flag } = workerData;
port.on("message", async ({ id, request }) => {
  let result;
  try {
    result = await run(request);
  } catch (error) {
    result = failure(error);
  }
  postReply(port, flag, id, result);
  if (request.op === "close") port.close();
});
