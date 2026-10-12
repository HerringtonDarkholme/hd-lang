// The platform-neutral core of the JS host (V8 in Node, and browsers): the
// import object of the ABI in `hd_host_abi` (runtime-and-host.md §17.2,
// §17.10), the exchange-buffer codecs of §17.4, the providers that need
// nothing from the platform (`Console`, `Clock`, `Random`, `Args`, `rt`,
// `prim`, and the test and property runners' hooks), and the entry driver
// of suspension.md §14.4. It uses web-standard APIs only. A platform layer
// (node.mjs) adds the providers that touch the machine by passing `layer`
// to `createCoreHost`.
//
// - Import shapes (§17.2): a method that never waits is one import; its
//   structured arguments are encoded one after another at offset 0 of the
//   exchange buffer and their length is its last parameter, a lone string
//   argument crosses as its bytes alone, and scalars are parameters. A
//   structured result is written at offset 0 and its length returned.
// - A method that may wait is `.start`, which returns `[status, value]`,
//   then `.finish(h)`. Status 0: done at once, the result is in the buffer
//   and `value` is its length. Status 1: pending, `value` is its handle.
//   `hd:Console` `write_line` completes at once (the line is written at
//   completion); `hd:Clock` `sleep` completes when its duration has
//   passed. Every other method runs synchronously and finishes at once.
// - `hd:rt` `block()` waits synchronously for at least one completion,
//   writes the completed handles to the exchange buffer and returns their
//   count; `stderr(len)` writes the buffer's first `len` bytes to standard
//   error.
// - Each provider asks the grant before it touches a resource
//   (cli.host.default-profile.granted); a refused call returns the
//   method's `NotGranted` result, or `.None` and a notice for `Env.get`
//   (cli.cap.partial.refuse, cli.cap.env.*).
// - The driver: an init export, then a poll export until it is not -1;
//   while it is pending, the host waits for completions in its event loop
//   and hands them over with `hd.wake(n)`. A pending root with nothing
import { shortest, fixed, lower, upper, parse } from "./float.mjs";

export class Deadlock extends Error {}

// Thrown by a test runner's `PropertyRunner.start` when the case has
// nothing left to run: the instance ends, and the runner counts the case
// as discarded (std-testing.runner.examples-done).
export class EndCase extends Error {}

// A host result that breaks the ABI: the `host-contract` panic.
export class HostContract extends Error {}

// A test body that ran longer than its `timeout`: the `time-limit` panic
// (std-testing.option.timeout-any-duration, flow.panic.time-limit).
class TimeLimit extends Error {}

// A trap's stack holds every Wasm frame, not V8's default ten: a panic
// report lists them all (cli.test.report.frame). A stack is captured only
// when an error is made, so a run that never traps pays nothing.
Error.stackTraceLimit = Infinity;

// The Wasm frames of a trap's stack, innermost first, as [function index,
// module byte offset] pairs from V8's `wasm-function[i]:0xOFF`; the test
// command maps them through the module's line table (wasm-layout.md §15.9).
const wasmFrames = (e) =>
  Array.from(String(e.stack ?? "").matchAll(/wasm-function\[(\d+)\]:0x([0-9a-f]+)/g), (m) => [
    Number(m[1]),
    parseInt(m[2], 16),
  ]);

// The panic report of a body that overran its timeout of `ms`
// milliseconds.
export const timeLimitReport = (ms) =>
  `panic: time-limit: the test case ran longer than its timeout of ${ms}ms\n`;

// A program's capability grant as `hd` resolved it (Grant Precedence):
// each trait with a limit maps to `false`, a total deny, or the list of its
// scope entries, whose paths are absolute and resolved; a trait it leaves
// out has no limit. A provider asks these before it touches a resource
// (cli.host.default-profile.granted), as `hd_run::Grants` does.
//
// `resolveTarget(path, cwd)` names the path a scope entry is compared with
// (cli.cap.scope.path.resolved); the Node layer resolves `..` and symbolic
// links, and a host with no file system leaves it out.
export function createGrant(limits = {}, resolveTarget = (path) => path) {
  const limit = (key) => (Object.hasOwn(limits, key) ? limits[key] : null);
  const check = (key, covers) => {
    const l = limit(key);
    return l === null ? true : l === false ? false : l.some(covers);
  };
  return {
    allows: (key) => limit(key) !== false,
    limited: (key) => limit(key) !== null,
    // An entry covers the file it names or every path under the directory
    // it names (cli.cap.scope.path); a relative path is from `cwd`.
    coversPath: (key, path, cwd) => {
      const target = resolveTarget(path, cwd);
      return check(key, (e) => target === e || target.startsWith(e.endsWith("/") ? e : e + "/"));
    },
    // cli.cap.scope.host, .host.forms, cli.cap.scope.net.
    coversHost: (key, host, port = null) =>
      check(key, (e) => {
        let [h, p] = [e, null];
        const v6 = /^\[([^\]]*)\](?::(\d+))?$/.exec(e);
        if (v6) [h, p] = [v6[1], v6[2] ?? null];
        else if (e.split(":").length === 2) [h, p] = e.split(":");
        if (p !== null && !/^\d+$/.test(p)) p = null;
        const hostOk = h.startsWith("*.")
          ? host.endsWith(h.slice(1)) && host.length > h.length - 1
          : h.toLowerCase() === host.toLowerCase();
        return hostOk && (p === null || Number(p) === port);
      }),
    // cli.cap.scope.net.lookup: the host match alone, whatever port an
    // entry names.
    coversHostName: (key, host) =>
      check(key, (e) => {
        const v6 = /^\[([^\]]*)\]/.exec(e);
        const h = v6 ? v6[1] : e.split(":").length === 2 ? e.split(":")[0] : e;
        return h.startsWith("*.")
          ? host.endsWith(h.slice(1)) && host.length > h.length - 1
          : h.toLowerCase() === host.toLowerCase();
      }),
    // cli.cap.scope.env, .process, .sys.
    coversName: (key, name) =>
      check(key, (e) =>
        key === "Env" && e.endsWith("*") ? name.startsWith(e.slice(0, -1)) : e === name,
      ),
  };
}

const utf8 = new TextEncoder();
// Lossy UTF-8 text that keeps a leading byte-order mark, as Node's
// Node's UTF-8 decoding does.
const lossy = new TextDecoder("utf-8", { ignoreBOM: true });
export const decodeUtf8 = (bytes) => lossy.decode(bytes);

// The encoder of §17.4: integers as LEB128 (zigzag when signed), strings
// and byte lists as their length then their bytes, lists as their count
// then their elements, an optional as 0, or 1 and the value, an enum as
// its variant index then its payload, data as its fields in order.
export class Enc {
  constructor() {
    this.buf = new Uint8Array(64);
    this.len = 0;
  }
  room(n) {
    if (this.len + n <= this.buf.length) return;
    const next = new Uint8Array(Math.max(this.buf.length * 2, this.len + n));
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }
  leb(v) {
    let x = BigInt(v);
    this.room(10);
    do {
      let b = Number(x & 0x7fn);
      x >>= 7n;
      if (x) b |= 0x80;
      this.buf[this.len++] = b;
    } while (x);
    return this;
  }
  zz(v) {
    const x = BigInt(v);
    return this.leb(x >= 0n ? x << 1n : (-x << 1n) - 1n);
  }
  bytes(u8) {
    this.leb(u8.length);
    this.room(u8.length);
    this.buf.set(u8, this.len);
    this.len += u8.length;
    return this;
  }
  str(s) {
    return this.bytes(utf8.encode(s));
  }
  // An `f64` as its raw IEEE 754 bits, little-endian.
  f64(v) {
    this.room(8);
    new DataView(this.buf.buffer, this.buf.byteOffset).setFloat64(this.len, v, true);
    this.len += 8;
    return this;
  }
  list(xs, each) {
    this.leb(xs.length);
    for (const x of xs) each(this, x);
    return this;
  }
  done() {
    return this.buf.subarray(0, this.len);
  }
}

// The decoder of §17.4, over a copy of the argument bytes.
export class Dec {
  constructor(bytes) {
    this.b = bytes;
    this.at = 0;
  }
  leb() {
    let r = 0n;
    let s = 0n;
    let b;
    do {
      b = this.b[this.at++];
      if (b === undefined) throw new HostContract("an argument does not decode");
      r |= BigInt(b & 0x7f) << s;
      s += 7n;
    } while (b & 0x80);
    return r;
  }
  num() {
    return Number(this.leb());
  }
  zz() {
    const x = this.leb();
    return (x >> 1n) ^ -(x & 1n);
  }
  bytes() {
    const n = this.num();
    if (this.at + n > this.b.length) throw new HostContract("an argument does not decode");
    const out = this.b.subarray(this.at, this.at + n);
    this.at += n;
    return out;
  }
  str() {
    return decodeUtf8(this.bytes());
  }
  list(each) {
    const n = this.num();
    const out = [];
    for (let i = 0; i < n; i++) out.push(each(this));
    return out;
  }
}
// A synchronous sleep of `ms` milliseconds. Where `SharedArrayBuffer`
// exists and the context may block (Node, a worker), it waits with
// `Atomics.wait`. Elsewhere (a page that is not cross-origin isolated, or a
// browser's main thread) it spins until the deadline.
function sleeper() {
  if (typeof SharedArrayBuffer === "function") {
    const cell = new Int32Array(new SharedArrayBuffer(4));
    try {
      Atomics.wait(cell, 0, 0, 0);
      return (ms) => {
        Atomics.wait(cell, 0, 0, ms);
      };
    } catch {
      // This context may not block.
    }
  }
  return (ms) => {
    const end = performance.now() + ms;
    while (performance.now() < end);
  };
}

// `hd:rt`: the runtime imports both hosts share (§17.2, §17.10).
function rtImports({ sink, flush, view, st, complete, nextDeadline, deadlock, until, overran }) {
  const sleep = sleeper();
  return {
    stderr: (len) => {
      st.reported = true;
      flush();
      sink.err(decodeUtf8(view(len)));
    },
    block: () => {
      for (;;) {
        const n = complete();
        if (n) return n;
        const d = nextDeadline();
        if (d === null) deadlock();
        sleep(Math.max(0, until(d) - performance.now()));
        if (overran()) throw new TimeLimit();
      }
    },
    abort: (h) => {
      st.ops.delete(h);
    },
  };
}

// The host primitives of std (spec/std/README.md#standard-library-
// primitives): a float arrives as its value, and the text goes back
// in the exchange buffer as a string.
function primImports({ put, text }) {
  return {
    format_f64: (value) => put(new Enc().str(shortest(value, 64))),
    format_f32: (value) => put(new Enc().str(shortest(value, 32))),
    format_f64_fixed: (value, digits) => put(new Enc().str(fixed(value, Number(digits)))),
    string_lower: (len) => put(new Enc().str(lower(text(len)))),
    string_upper: (len) => put(new Enc().str(upper(text(len)))),
    parse_f64: (len) => parse(text(len)),
  };
}

// `hd:Console`: one line per `write_line`, completed at once.
function consoleImports({ text, pending, finish, view, st }) {
  return {
    "write_line.start": (len) => {
      const line = text(len);
      return pending(performance.now(), () => st.out.push(line + "\n"));
    },
    "write_line.finish": (h) => {
      finish(h);
      view(1)[0] = 0;
      return 1;
    },
  };
}

// `hd:Clock`: the time, and sleeps that complete when due.
function clockImports({ pending, finish }) {
  return {
    now: () => BigInt(Date.now()),
    monotonic: () => BigInt(Math.floor(performance.now())),
    "sleep.start": (ms) => pending(performance.now() + Number(ms), null),
    "sleep.finish": (h) => {
      finish(h);
      return 0;
    },
  };
}

// `count` random bytes (`getRandomValues` takes at most 65536 at a time).
function randomBytes(count) {
  const out = new Uint8Array(count);
  for (let at = 0; at < count; at += 65536) crypto.getRandomValues(out.subarray(at, at + 65536));
  return out;
}

// The operating system's random source (cli.host.default-profile).
// `Random` has no scope entries: a grant of `false` refuses the whole
// program at startup (cli.cap.total.refuse).
function randomImports({ put }) {
  return {
    next_u64: () => new DataView(randomBytes(8).buffer).getBigInt64(0, true),
    fill: (count) => put(new Enc().bytes(randomBytes(count >>> 0))),
  };
}

// `hd:Args`: the program's path and arguments.
function argsImports({ put, program, args }) {
  return {
    program: () => put(new Enc().str(program)),
    list: () => put(new Enc().list(args, (e, a) => e.str(a))),
  };
}

// `hd:PropertyRunner`: property cases ask the test runner for draws.
function propertyRunnerImports({ env, put, text }) {
  return {
    // The property runner's side of a property case
    // (std-testing.runner.start-case, .record, .show): the test runner
    // decides each case and keeps its draws and its input's text.
    start: (cases, shrink, examples) => {
      if (!env.runner) throw new HostContract("no property runner binds this run");
      const c = env.runner.start(cases, shrink, examples);
      const e = new Enc();
      if (c.example === null) e.leb(0);
      else e.leb(1).leb(c.example);
      e.zz(c.seed).zz(c.size);
      e.list(c.replay, (e, v) => e.zz(v));
      return put(e);
    },
    record: (value) => env.runner?.record(BigInt(value)),
    show: (len) => env.runner?.show(text(len)),
  };
}

// `hd:TestRunner`: the hooks every host has (a platform layer adds
// `temp_dir`). Snapshot files are read and written through `files`
// (cases.mjs describes it): the disk on Node, memory in the playground.
function testRunnerImports({ env, st, put, text, files }) {
  // Snapshot files of the case (std-testing.snapshot-file.*).
  const snapshotCheck = (actual) => {
    const s = env.snapshot;
    st.snapshots += 1;
    if (!s || !files) return "no snapshot file: this run names no snapshot directory";
    // An `it_each` row's slug adds the row's index
    // (std-testing.snapshot-file.row).
    const slug = s.slug + (env.runner?.slugSuffix() ?? "");
    const file = files.join(s.dir, `${slug}-${st.snapshots}.snap`);
    const shown = s.shown ? `${s.shown}/${slug}-${st.snapshots}.snap` : file;
    const expected = files.read(file);
    if (expected !== null) {
      if (expected === actual) return "";
      if (!s.update) {
        return `the text differs from the snapshot file ${shown}: expected ${JSON.stringify(expected)}, actual ${JSON.stringify(actual)}; run \`hd test --update\` to record it`;
      }
    } else if (!s.update) {
      return `the snapshot file ${shown} is missing; run \`hd test --update\` to record it`;
    }
    files.write(file, actual);
    return "";
  };
  return {
    // The test runner (`env.runner`, cases.mjs) picks the row an `it_each`
    // case runs; without one, row 0. Timeouts run under no limit here.
    row: (count) => (env.runner ? env.runner.row(count) : 0),
    // The runner fails a body that runs longer than its timeout: here at
    // the end of the run or of a wait, and in test.mjs by stopping an
    // instance that never returns.
    report_timeout: (millis) => {
      st.timeoutMs = Number(millis);
      st.deadline = performance.now() + st.timeoutMs;
      env.runner?.timeout?.(st.timeoutMs);
    },
    snapshot_check: (len) => put(new Enc().str(snapshotCheck(text(len)))),
  };
}

// One host per instance: `sink.out(text)` and `sink.err(text)` receive the
// program's standard output and standard error. `env` holds what the
// providers hand out: `args` and `program` for `Args`, `grants` for the
// scope checks, `stdin` (a file descriptor, or `null` for a closed
// standard input), and in a test case its `tempDir` (made on the first
// `tempDir()` call, which `TestRunner.temp_dir` makes), the base `seed`
// of a property test (Test Environments, cli.test.seed), its `snapshot`
// files (`dir`, `shown`, `slug`, `update`), the test runner's
// `programs`, the package's executables and tasks by name
// (cli.test.process), its `runner`, the hooks through which
// `TestRunner.row` and `PropertyRunner` reach it (`row`, `slugSuffix`,
// `start`, `record`, `show`; cases.mjs), and `files`, where its snapshot
// files are when the platform layer's `files` is not where they are.
export function createCoreHost(sink, env = {}, layer = null) {
  const args = env.args ?? [];
  const program = env.program ?? "";
  const grant = createGrant(env.grants ?? {}, layer?.resolveTarget);
  const seed = env.seed ?? null;
  // The instance's mutable state, shared with the provider functions
  // below: the exchange memory, trap reporting, the test timeout, case
  // files, pending operations, output, and noticed names.
  const st = {
    memory: null,
    reported: false,
    timeoutMs: null,
    deadline: null,
    snapshots: 0,
    tempMade: false,
    nextHandle: 1,
    out: [],
    ops: new Map(),
    noticed: new Set(),
  };
  const view = (len) => new Uint8Array(st.memory.buffer, 0, len);
  // A copy of the argument bytes, which a result may overwrite.
  const argsOf = (len) => new Dec(Uint8Array.from(view(len)));
  const text = (len) => decodeUtf8(view(len));
  // Writes an encoded result at offset 0, growing the buffer to fit, and
  // returns its length (§17.3).
  const put = (e) => {
    const bytes = e.done();
    if (bytes.length > st.memory.buffer.byteLength) {
      st.memory.grow(Math.ceil((bytes.length - st.memory.buffer.byteLength) / 65536));
    }
    new Uint8Array(st.memory.buffer, 0, bytes.length).set(bytes);
    return bytes.length;
  };
  // A waiting method that finished at once (§17.2): status 0, the length.
  const ready = (e) => [0, put(e)];
  const flush = () => {
    if (st.out.length) {
      sink.out(st.out.join(""));
      st.out.length = 0;
    }
  };
  // Pending operations by handle: `at` is when it completes.
  const pending = (at, done) => {
    const h = st.nextHandle++;
    st.ops.set(h, { at, done, completed: false });
    return [1, h];
  };
  const nextDeadline = () => {
    let d = null;
    for (const o of st.ops.values()) {
      if (!o.completed && (d === null || o.at < d)) d = o.at;
    }
    return d;
  };
  // Completes every due operation and writes the handles to the buffer.
  const complete = () => {
    const now = performance.now();
    const due = [...st.ops]
      .filter(([, o]) => !o.completed && o.at <= now)
      .sort((a, b) => a[1].at - b[1].at || a[0] - b[0]);
    if (due.length * 4 > st.memory.buffer.byteLength) {
      st.memory.grow(Math.ceil((due.length * 4) / 65536));
    }
    const words = new DataView(st.memory.buffer);
    due.forEach(([h, o], i) => {
      o.completed = true;
      if (o.done) o.done();
      words.setInt32(4 * i, h, true);
    });
    return due.length;
  };
  const deadlock = () => {
    flush();
    sink.err("panic: deadlock: the program waits, but no host operation is pending\n");
    st.reported = true;
    throw new Deadlock();
  };
  const finish = (h) => {
    st.ops.delete(h);
  };
  // Waits at most until `d`, and never past the timeout: a pending host
  // wait is cut at the deadline (runtime-and-host.md §17.8).
  const until = (d) => (st.deadline === null ? d : Math.min(d, st.deadline));
  const overran = () => st.deadline !== null && performance.now() > st.deadline;
  const imports = {
    "hd:rt": rtImports({ sink, flush, view, st, complete, nextDeadline, deadlock, until, overran }),
    "hd:prim": primImports({ put, text }),
    "hd:Console": consoleImports({ text, pending, finish, view, st }),
    "hd:Clock": clockImports({ pending, finish }),
    "hd:Random": randomImports({ put }),
    "hd:Args": argsImports({ put, program, args }),
    "hd:TestRunner": testRunnerImports({ env, st, put, text, files: env.files ?? layer?.files }),
    "hd:PropertyRunner": propertyRunnerImports({ env, put, text }),
  };

  // The platform layer's providers; a module it names extends the core's
  // table for that module rather than replacing it.
  const platform =
    layer?.providers({ env, sink, st, grant, flush, put, ready, text, argsOf }) ?? null;
  for (const [module, table] of Object.entries(platform?.imports ?? {})) {
    imports[module] = { ...imports[module], ...table };
  }

  // A runtime profile's own providers (`env.providers`, profiles.mjs).
  if (env.providers) Object.assign(imports, env.providers({ put, Enc }));

  // Runs `init`, then polls `poll` to completion. Returns the status (3
  // for a trap), whether the instance trapped, and a Wasm trap's frames.
  const run = async (instance, init, poll) => {
    st.memory = instance.exports["hd.x"] ?? null;
    const ex = instance.exports;
    let status = 0;
    let trapped = false;
    let ended = false;
    let frames = [];
    try {
      ex[init]();
      status = ex[poll]();
      while (status === -1) {
        const n = complete();
        if (n === 0) {
          const d = nextDeadline();
          if (d === null) deadlock();
          await new Promise((r) => setTimeout(r, Math.max(0, until(d) - performance.now())));
          if (overran()) throw new TimeLimit();
          continue;
        }
        ex["hd.wake"](n);
        status = ex[poll]();
      }
    } catch (e) {
      flush();
      if (e instanceof EndCase) {
        return { status: 0, trapped: false, ended: true };
      }
      if (e instanceof HostContract) {
        sink.err(`panic: host-contract: ${e.message}\n`);
        st.reported = true;
      } else if (e instanceof TimeLimit) {
        st.reported = true;
      } else if (e instanceof RangeError && /call stack size/i.test(e.message)) {
        // V8 reports a guest stack overflow as a RangeError ("Maximum call
        // stack size exceeded"), not a WebAssembly.RuntimeError; it is the
        // stack-exhausted panic. Other RangeErrors are host bugs and rethrow.
        sink.err(
          "panic: stack-exhausted: the call stack ran out; check for recursion that never ends\n",
        );
        st.reported = true;
        frames = wasmFrames(e);
      } else if (e instanceof WebAssembly.RuntimeError) {
        frames = wasmFrames(e);
      } else if (!(e instanceof Deadlock)) {
        throw e;
      }
      // A panic stub wrote its report; any other trap is an internal error.
      if (!st.reported) sink.err(`internal error: ${e.message}\n`);
      status = 3;
      trapped = true;
    }
    flush();
    // A body that ended after its deadline ran longer than its timeout,
    // whatever it did after the deadline.
    if (overran()) {
      sink.err(timeLimitReport(st.timeoutMs));
      status = 3;
      trapped = true;
      frames = [];
    }
    return { status, trapped, ended, frames };
  };

  // Releases what the platform layer holds (its client threads, its
  // scratch directory).
  const close = () => platform?.close?.();

  return { imports, run, close, args, program, grant, seed, tempDir: platform?.tempDir };
}
