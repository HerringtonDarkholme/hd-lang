// The JS host of `hd run` on V8 (Node): the import object of the ABI in
// `hd_host_abi` (runtime-and-host.md §17.2, §17.10) for what the compiler
// emits today, and the entry driver of suspension.md §14.4.
//
// - Host operations that may wait start pending: `.start` returns status
//   1 and a handle. `hd:Console` `write_line` completes at once (the line
//   is written at completion); `hd:Clock` `sleep` completes when its
//   duration has passed. `.finish(h)` writes the result into the exchange
//   buffer; `hd:rt` `abort(h)` drops a pending operation.
// - `hd:rt` `block()` waits synchronously for at least one completion,
//   writes the completed handles to the exchange buffer and returns their
//   count; `stderr(len)` writes the buffer's first `len` bytes to standard
//   error.
// - The driver: `hd.init()`, then `hd.poll()` until it is not -1; while
//   it is pending, the host waits for completions in its event loop and
//   hands them over with `hd.wake(n)`. A pending root with nothing left to
//   wait for is a deadlock.
// A trap is a panic: the run exits with status 3 (never 101,
// module.profile.panic-status).
import { readFileSync, writeSync } from "node:fs";

const bytes = readFileSync(process.argv[2]);
const out = [];
let memory = null;
let reported = false;
const view = (len) => new Uint8Array(memory.buffer, 0, len);
const flush = () => {
  if (out.length) {
    writeSync(1, out.join(""));
    out.length = 0;
  }
};

// Pending operations by handle: `at` is when it completes.
const ops = new Map();
let nextHandle = 1;
const pending = (at, done) => {
  const h = nextHandle++;
  ops.set(h, { at, done, completed: false });
  return [1, h];
};
const nextDeadline = () => {
  let d = null;
  for (const o of ops.values()) {
    if (!o.completed && (d === null || o.at < d)) d = o.at;
  }
  return d;
};
// Completes every due operation and writes the handles to the buffer.
const complete = () => {
  const now = performance.now();
  const due = [...ops]
    .filter(([, o]) => !o.completed && o.at <= now)
    .sort((a, b) => a[1].at - b[1].at || a[0] - b[0]);
  if (due.length * 4 > memory.buffer.byteLength) {
    memory.grow(Math.ceil((due.length * 4) / 65536));
  }
  const words = new DataView(memory.buffer);
  due.forEach(([h, o], i) => {
    o.completed = true;
    if (o.done) o.done();
    words.setInt32(4 * i, h, true);
  });
  return due.length;
};
class Deadlock extends Error {}
const deadlock = () => {
  flush();
  writeSync(2, "panic: deadlock: the program waits, but no host operation is pending\n");
  reported = true;
  throw new Deadlock();
};
const sleeper = new Int32Array(new SharedArrayBuffer(4));
const finish = (h) => {
  ops.delete(h);
};

const imports = {
  "hd:rt": {
    stderr: (len) => {
      reported = true;
      flush();
      writeSync(2, Buffer.from(view(len)));
    },
    block: () => {
      for (;;) {
        const n = complete();
        if (n) return n;
        const d = nextDeadline();
        if (d === null) deadlock();
        Atomics.wait(sleeper, 0, 0, Math.max(0, d - performance.now()));
      }
    },
    abort: (h) => {
      ops.delete(h);
    },
  },
  "hd:Console": {
    "write_line.start": (len) => {
      const text = Buffer.from(view(len)).toString("utf8");
      return pending(performance.now(), () => out.push(text + "\n"));
    },
    "write_line.finish": (h) => {
      finish(h);
      view(1)[0] = 0;
      return 1;
    },
  },
  "hd:Clock": {
    "sleep.start": (ms) => pending(performance.now() + Number(ms), null),
    "sleep.finish": (h) => {
      finish(h);
      return 0;
    },
  },
};
const t0 = performance.now();
const { instance } = await WebAssembly.instantiate(bytes, imports);
memory = instance.exports["hd.x"] ?? null;
const t1 = performance.now();
const ex = instance.exports;
let status = 0;
try {
  ex["hd.init"]();
  status = ex["hd.poll"]();
  while (status === -1) {
    const n = complete();
    if (n === 0) {
      const d = nextDeadline();
      if (d === null) deadlock();
      await new Promise((r) => setTimeout(r, Math.max(0, d - performance.now())));
      continue;
    }
    ex["hd.wake"](n);
    status = ex["hd.poll"]();
  }
} catch (e) {
  flush();
  if (!(e instanceof WebAssembly.RuntimeError) && !(e instanceof Deadlock)) throw e;
  // A panic stub wrote its report; any other trap is an internal error.
  if (!reported) process.stderr.write(`internal error: ${e.message}\n`);
  status = 3;
}
const t2 = performance.now();
flush();
process.stderr.write(`instantiate_ms=${(t1 - t0).toFixed(3)} run_ms=${(t2 - t1).toFixed(3)}\n`);
process.exitCode = status;
