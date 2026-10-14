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
//   error; `stdout(len)` holds them with the console's output (the REPL's
//   value line).
// - Each provider asks the grant before it touches a resource
//   (cli.host.default-profile.granted); a refused call returns the
//   method's `NotGranted` result, or `.None` and a notice for `Env.get`
//   (cli.cap.partial.refuse, cli.cap.env.*).
// - The driver: an init export, then a poll export until it is not -1;
//   while it is pending, the host waits for completions in its event loop
//   and hands them over with `hd.wake(n)`. A pending root with nothing
//   pending is a deadlock.
// - A REPL session (`createSession`) runs each input's module as an entry
//   in one growing set of instances, records a journal of its host calls
//   at the import call sites, and rebuilds itself from a journal
//   (journal.mjs), as `hd_run_wasmtime`'s session does.
import { shortest, fixed, lower, upper, parse } from "./float.mjs";
import {
  Diverged,
  Recorder,
  Replay,
  SESSION_CAP,
  callArgs,
  importTypes,
  ran,
  recorded,
} from "./journal.mjs";

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

// Whether `text` can be an `Http` request URL (std-http.url.valid,
// .non-ascii, .no-normalize): RFC 3986 has no space, control or non-ASCII
// character, and a provider does not percent-decode or IDNA-map a host.
// `new URL` does both, so a program's URL is checked before it is parsed;
// a host is written in punycode.
export const isAsciiUrl = (text) => {
  if (!/^[\x21-\x7e]*$/.test(text)) return false;
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(text)?.[1] ?? "";
  return !authority.slice(authority.lastIndexOf("@") + 1).includes("%");
};

// A program's capability grant as `hd` resolved it (Grant Precedence):
// each trait with a limit maps to `false`, a total deny, or the list of its
// scope entries, whose paths are absolute and resolved; a trait it leaves
// out has no limit. A provider asks these before it touches a resource
// (cli.host.default-profile.granted), as `hd_run::Grants` does.
//
// `resolveTarget(path, cwd)` names the path a scope entry is compared with
// (cli.cap.scope.path.resolved); the Node layer resolves `..` and symbolic
// links, and a host with no file system leaves it out. `sep` is the
// separator of the paths it returns and of the entries: "\\" on Windows.
export function createGrant(limits = {}, resolveTarget = (path) => path, sep = "/") {
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
      return check(key, (e) => target === e || target.startsWith(e.endsWith(sep) ? e : e + sep));
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
    stdout: (len) => {
      st.out.push(decodeUtf8(view(len)));
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
function consoleImports({ text, pending, finish, put, st }) {
  return {
    "write_line.start": (len) => {
      const line = text(len);
      return pending(performance.now(), () => st.out.push(line + "\n"));
    },
    // `.Ok(())`.
    "write_line.finish": (h) => {
      finish(h);
      return put(Uint8Array.of(0));
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
  const grant = createGrant(env.grants ?? {}, layer?.resolveTarget, layer?.sep);
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
    payload: null,
  };
  const view = (len) => new Uint8Array(st.memory.buffer, 0, len);
  // A copy of the argument bytes, which a result may overwrite.
  const argsOf = (len) => new Dec(Uint8Array.from(view(len)));
  const text = (len) => decodeUtf8(view(len));
  // Writes an encoded result (or its bytes) at offset 0, growing the
  // buffer to fit, and returns its length (§17.3). A session's recorder
  // reads the bytes back as the call's result (`st.payload`).
  const put = (e) => {
    const bytes = e instanceof Uint8Array ? e : e.done();
    if (bytes.length > st.memory.buffer.byteLength) {
      st.memory.grow(Math.ceil((bytes.length - st.memory.buffer.byteLength) / 65536));
    }
    new Uint8Array(st.memory.buffer, 0, bytes.length).set(bytes);
    st.payload = bytes;
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
    "hd:Console": consoleImports({ text, pending, finish, put, st }),
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

  // A runtime profile's own providers (`env.providers`, profiles.mjs):
  // `pending` registers a wait the driver wakes, `finish` drops one.
  if (env.providers) Object.assign(imports, env.providers({ put, Enc, pending, finish }));

  // Points the host at `instance`'s memory, for a driver that calls the
  // exports itself instead of `run` (a scenario's drivers): reports and
  // codecs read the exchange buffer through it.
  const prime = (instance) => {
    st.memory = instance.exports["hd.x"] ?? null;
  };

  // Runs `init`, then polls `poll` to completion. Returns the status (3
  // for a trap), whether the instance trapped, and a Wasm trap's frames.
  // The run is one driver, context 0: a scenario's further drivers poll
  // the exports with their own contexts instead of this loop.
  //
  // A REPL session passes `driver`, its hooks: `poll()` before each poll;
  // `replaying()`, true while wakes come from a record, and then
  // `served()`, which writes the recorded wake's handles and returns their
  // count; `woke(n)` before each wake; `wait(ms)`, a wait that a stop
  // request ends at once; and `stopping()`, which answers how a stop
  // request ends a waiting input: null for none, else `cancelled` (it calls
  // `hd.cancel`) or `stopped`.
  const run = async (instance, init, poll, driver = null) => {
    prime(instance);
    const ex = instance.exports;
    let status = 0;
    let trapped = false;
    let ended = false;
    let frames = [];
    const wait = (ms) => driver?.wait(ms) ?? new Promise((r) => setTimeout(r, ms));
    try {
      ex[init]();
      driver?.poll();
      status = ex[poll](0);
      while (status === -1) {
        let n;
        if (driver?.replaying()) n = driver.served();
        else {
          n = complete();
          if (n === 0) {
            const d = nextDeadline();
            if (d === null) deadlock();
            await wait(Math.max(0, until(d) - performance.now()));
            if (overran()) throw new TimeLimit();
            const stop = driver?.stopping(Boolean(ex["hd.cancel"]));
            if (stop) {
              if (stop === "cancelled") ex["hd.cancel"]();
              flush();
              sink.err(`${stop}\n`);
              st.reported = true;
              status = 3;
              trapped = true;
              break;
            }
            continue;
          }
        }
        driver?.woke(n);
        ex["hd.wake"](n);
        driver?.poll();
        status = ex[poll](0);
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
      } else if (e instanceof Diverged) {
        st.reported = true;
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

  // What a REPL session reaches past the import object: the instance
  // state, the result writer, the deadlock report and the output.
  const core = { st, put, deadlock, flush };

  return {
    imports,
    run,
    prime,
    close,
    args,
    program,
    grant,
    seed,
    tempDir: platform?.tempDir,
    core,
  };
}

// --- REPL sessions ------------------------------------------------------

// The Wasm module that a session's input imports earlier inputs' exports
// from, and the session key of the memory (`hd_wasm::SESSION_MODULE`,
// `MEMORY_KEY`).
const SESSION_MODULE = "hd.s";
const MEMORY_KEY = "hd.x";
// The runtime globals every input shares (`hd_wasm::rt::runtime_globals`).
const RUNTIME_GLOBALS = new Set(["rt.wake", "rt.fact", "rt.forbid", "rt.root", "rt.owner"]);

// Whether an input's export is a session key (`hd_wasm::is_session_key`):
// the memory, a function or a global; not the entry exports.
export const isSessionKey = (name) =>
  name === MEMORY_KEY || /^(inst|bind|rt|canon|wit|witf)\./.test(name);

// Which journal row a call of an import writes (`Kind::record` in
// hd_run_wasmtime): none for `hd:prim`, which answers from its arguments
// alone, so a replay computes it again.
function recordOf(module, name) {
  if (module === "hd:prim") return "skip";
  if (module === "hd:rt" && (name === "block" || name === "abort")) return name;
  return name.endsWith(".finish") ? "finish" : "call";
}

// The imports whose first parameter is the length of the exchange buffer
// bytes they read (`Kind::input`).
const READS_ARGS = new Set([
  "hd:TestRunner snapshot_check",
  "hd:PropertyRunner show",
  "hd:Process run.start",
  "hd:Http send.start",
  "hd:rt stderr",
  "hd:rt stdout",
  "hd:prim string_lower",
  "hd:prim string_upper",
  "hd:prim parse_f64",
  "hd:Console write_line.start",
  "hd:Env get",
]);
const NET_READS = { lookup: 0, connect: 1, listen: 1, bind_udp: 1, "TcpStream.write": 1 };

// The parameter that holds the length of the exchange buffer bytes a call
// reads, with its scalar parameters the call's request; null for none.
function readsArgs(module, name) {
  const method = name.endsWith(".start") ? name.slice(0, -".start".length) : null;
  if (module === "hd:Net") return NET_READS[method] ?? (method === "UdpSocket.send_to" ? 2 : null);
  if (module === "hd:FsRead" || module === "hd:FsWrite") return method === null ? null : 0;
  return READS_ARGS.has(`${module} ${name}`) ? 0 : null;
}

// A reply value as a Wasm result of type `type`: a narrower integer keeps
// its low bits.
function resultOf(v, type) {
  if (type === "i64") return BigInt.asIntN(64, BigInt(v));
  if (type === "f64" || type === "f32") return Number(v);
  return Number(BigInt.asIntN(32, BigInt(v)));
}

// A host import's reply as the journal holds it, from the JS value it
// returned and its result types.
function replyOf(ret, results) {
  if (ret === undefined) return { tag: 0 };
  if (Array.isArray(ret)) return { tag: 3, v: Number(ret[0]) | 0, w: Number(ret[1]) | 0 };
  if (results[0] === "f64" || results[0] === "f32") {
    const dv = new DataView(new ArrayBuffer(8));
    dv.setFloat64(0, Number(ret), true);
    return { tag: 2, v: dv.getBigUint64(0, true) };
  }
  return { tag: 1, v: typeof ret === "bigint" ? ret : BigInt(Math.trunc(Number(ret))) };
}

// The JS value of a served reply, for an import with result types
// `results`.
function servedValue(value, results) {
  switch (value.tag) {
    case 0:
      return undefined;
    case 1:
      return resultOf(value.v, results[0]);
    case 2: {
      const dv = new DataView(new ArrayBuffer(8));
      dv.setBigUint64(0, value.v, true);
      return dv.getFloat64(0, true);
    }
    default:
      return [resultOf(value.v, results[0]), resultOf(value.w, results[1])];
  }
}

// The handle a `.finish` or `abort` names; no live one is negative.
const handleParam = (args) => {
  const h = Number(args[0]);
  return Number.isInteger(h) && h >= 0 && h <= 0xffffffff ? h : 0xffffffff;
};

// A REPL session (repl-plan.md job W4): the instances of every input that
// ran, sharing the first one's memory, as `hd_run_wasmtime`'s
// `WasmtimeSession` holds them. `sink`, `env` and `layer` are
// `createCoreHost`'s: one host serves every input.
//
// - `input(number, source, wasm)` runs the input module `wasm`, numbered
//   `number`, of the text `source`, as `hd run` runs an entry: `hd.init`,
//   then `hd.poll` and `hd.wake` until it ends. It imports the session
//   keys of every earlier input that ended with status 0 from `hd.s`, and
//   the host imports. It answers `{ status, end, joined, error }`: status
//   0 joins the session, so the caller reports the outcome to the compiler
//   (W3's repl-outcome request); a panicking input ends with status 3,
//   its report on standard error. `error` names why a module cannot run;
//   a module the host cannot link does not start (`end` null).
// - After an input that did not end with status 0, the session drops the
//   host operations it left pending and sets the runtime globals back to
//   their initial values, so the next input starts as the first did
//   (`cli.repl.panic`).
// - The session records a journal of every input's host calls
//   (journal.mjs) at the import call sites; `journal()` returns it, or null
//   once it passed its cap. `rebuild(rebuilder, journal)` starts the
//   instances again and replays the journal's inputs with every host result
//   served from it (`cli.repl.host.once`) and nothing printed. It skips
//   stopped and cancelled inputs (`cli.repl.rebuild.stopped`) and drops the
//   first input that no longer builds or asks for what its record does not
//   hold, with every later one (`cli.repl.rebuild.prefix`).
//   `rebuilder.build(number)` returns `{ source, wasm }` (or throws the
//   reason it no longer builds), and `rebuilder.replayed(number, joined)`
//   hears each input replayed. It answers `{ kept, skipped, dropped,
//   reason }`.
// - `stop()` cancels the input that waits on the host: it calls the
//   input's `hd.cancel`, so its `defer` suites run, and the input ends as
//   a panicking input does, `cancelled` (`cli.repl.cancel`). An input that
//   computes cannot hear it; the page terminates the worker and rebuilds.
export function createSession(sink, env = {}, layer = null, options = {}) {
  // Output is dropped while a rebuild replays (`cli.repl.host.once`).
  let muted = false;
  const host = createCoreHost(
    {
      out: (text) => muted || sink.out(text),
      err: (text) => muted || sink.err(text),
    },
    env,
    layer,
  );
  const { st, put, deadlock, flush } = host.core;
  const execution = crypto.getRandomValues(new Uint8Array(16));
  let recorder = new Recorder(options.journalCap ?? SESSION_CAP, execution);
  let replay = null;
  // The session keys of the inputs that ended with status 0, whether one
  // of them is the memory, and the runtime globals among them.
  let keys = {};
  let memory = false;
  let runtime = [];
  // The host calls made, not served from a record.
  let calls = 0;
  // A stop request, the wait it ends, and how it ended the input.
  let stopped = false;
  let wakeWait = null;
  let halt = null;

  const handlesAt = (n) => {
    const words = new DataView(st.memory.buffer);
    return Array.from({ length: n }, (_, i) => words.getUint32(4 * i, true));
  };
  const handleBytes = (handles) => {
    const out = new Uint8Array(4 * handles.length);
    handles.forEach((h, i) => new DataView(out.buffer).setUint32(4 * i, h, true));
    return out;
  };
  // A call's arguments as the journal holds them: its scalar parameters,
  // then the exchange buffer bytes it reads.
  const requestOf = (args, params, reads) => {
    if (reads === null) return callArgs(args, params, null);
    const len = Number(args[reads]);
    if (!(len >= 0 && len <= st.memory.buffer.byteLength)) {
      throw new HostContract("an argument length is past the exchange buffer");
    }
    return callArgs(args, params, new Uint8Array(st.memory.buffer, 0, len));
  };
  // Answers a call of a replayed input from its record; a call the record
  // does not hold next throws `Diverged`, which ends the input.
  const serve = (record, module, name, args, request) => {
    switch (record) {
      case "call":
        return replay.call(module, name, request);
      case "finish":
        return replay.finish(handleParam(args));
      case "abort":
        replay.abort(handleParam(args));
        return { value: { tag: 0 }, result: null };
      default: {
        const handles = replay.block();
        // The first run's block found nothing pending either.
        if (handles === null) deadlock();
        const result = handles.length ? handleBytes(handles) : null;
        return { value: { tag: 1, v: BigInt(handles.length) }, result };
      }
    }
  };
  // The import `module` `name` of type `type`, as a session calls it: the
  // recorder and the replayer sit here, where every host call is made.
  const site = (module, name, f, type) => {
    const record = recordOf(module, name);
    if (record === "skip") return f;
    const reads = readsArgs(module, name);
    const params = type?.params ?? [];
    const results = type?.results ?? [];
    // A replayed call is recorded again, so the new journal holds it.
    const write = (args, request, value, result) => {
      if (record === "call") recorder.call(module, name, request, value, result);
      else if (record === "finish") recorder.finish(handleParam(args), value, result);
      else if (record === "abort") recorder.abort(handleParam(args));
      else recorder.block(handlesAt(Number(value.v)));
    };
    return (...args) => {
      const request = record === "call" ? requestOf(args, params, reads) : null;
      if (replay) {
        const { value, result } = serve(record, module, name, args, request);
        if (result) put(result);
        write(args, request, value, result);
        return servedValue(value, results);
      }
      calls += 1;
      st.payload = null;
      const ret = f(...args);
      const value = replyOf(ret, results);
      write(args, request, value, st.payload);
      return ret;
    };
  };
  const sites = new Map();

  const driver = {
    poll: () => {
      recorder.poll();
      replay?.poll();
    },
    replaying: () => replay !== null,
    served: () => {
      // A replayed input waits when its record wakes it next.
      if (!replay.wakeNext()) deadlock();
      const handles = replay.wake();
      if (handles.length) put(handleBytes(handles));
      return handles.length;
    },
    woke: (n) => recorder.wake(handlesAt(n)),
    wait: (ms) =>
      new Promise((resolve) => {
        if (stopped) {
          resolve();
          return;
        }
        const timer = setTimeout(resolve, ms);
        wakeWait = () => {
          clearTimeout(timer);
          resolve();
        };
      }).finally(() => {
        wakeWait = null;
      }),
    stopping: (cancels) => {
      if (!stopped) return null;
      stopped = false;
      if (!cancels) {
        halt = { kind: "stopped" };
        return "stopped";
      }
      recorder.cancel();
      halt = { kind: "cancelled" };
      return "cancelled";
    },
  };

  // After an input that did not end with status 0: drops the host
  // operations it left pending, then sets every runtime global the inputs
  // share back to its initial value.
  const reset = () => {
    st.ops.clear();
    for (const g of runtime) {
      const v = g.value;
      g.value = typeof v === "bigint" ? 0n : typeof v === "number" ? 0 : null;
    }
  };
  // Adds the session keys `instance` exports, for later inputs to import;
  // every input exports the memory, and the first one's is the session's.
  const add = (instance) => {
    for (const [name, value] of Object.entries(instance.exports)) {
      if (!isSessionKey(name) || (name === MEMORY_KEY && memory)) continue;
      memory ||= name === MEMORY_KEY;
      if (RUNTIME_GLOBALS.has(name) && !runtime.includes(value)) runtime.push(value);
      keys[name] = value;
    }
  };

  // Links an input's module: its host imports through their call sites,
  // and its session imports from the keys so far. Null and the reason for
  // a host import this host does not have.
  const link = (module, wasm) => {
    const types = importTypes(wasm);
    const object = { [SESSION_MODULE]: keys };
    for (const { module: m, name, kind } of WebAssembly.Module.imports(module)) {
      if (m === SESSION_MODULE) continue;
      const f = host.imports[m]?.[name];
      if (kind !== "function" || typeof f !== "function") {
        return { error: `the import \`${m}\` \`${name}\` is not in this host` };
      }
      const key = `${m}\0${name}`;
      if (!sites.has(key)) sites.set(key, site(m, name, f, types.get(key)));
      object[m] ??= {};
      object[m][name] = sites.get(key);
    }
    return { object };
  };

  const runInput = async (number, source, wasm) => {
    stopped = false;
    halt = null;
    let module;
    try {
      module = await WebAssembly.compile(wasm);
    } catch (e) {
      return { status: null, end: null, joined: false, error: String(e?.message ?? e) };
    }
    const { object, error } = link(module, wasm);
    if (error) return { status: null, end: null, joined: false, error };
    // A panic or standard error of an earlier input does not hide this
    // one's internal error.
    st.reported = false;
    recorder.input(number, source, wasm);
    let outcome;
    try {
      const instance = await WebAssembly.instantiate(module, object);
      const { status, trapped } = await host.run(instance, "hd.init", "hd.poll", driver);
      const end = halt ?? (trapped ? { kind: "trapped" } : { kind: "exit", status });
      const joined = status === 0 && !trapped;
      if (joined) add(instance);
      outcome = { status, end, joined, error: null };
    } catch (e) {
      flush();
      outcome = { status: null, end: { kind: "failed" }, joined: false };
      outcome.error = String(e?.message ?? e);
    }
    if (!outcome.joined) reset();
    recorder.end(outcome.end);
    return outcome;
  };

  // Starts the instances again with a journal that starts as `journal`,
  // over the same host; every input is forgotten.
  const restart = (journal) => {
    flush();
    st.ops.clear();
    st.reported = false;
    st.memory = null;
    recorder = journal;
    replay = null;
    muted = false;
    keys = {};
    memory = false;
    runtime = [];
  };

  // Runs `input` again, of the text `source` built as `wasm`, with its host
  // calls and wakes served from its record and its output dropped. Throws
  // the divergence: it asked for what the record does not hold, or ended
  // otherwise.
  const replayInput = async (input, source, wasm) => {
    replay = new Replay(input);
    muted = true;
    let outcome;
    try {
      outcome = await runInput(input.number, source, wasm);
    } finally {
      muted = false;
    }
    const served = replay;
    replay = null;
    if (outcome.end === null) throw new Error(`input ${input.number} cannot run: ${outcome.error}`);
    served.end(outcome.end, input.end);
    return outcome;
  };

  const rebuild = async (rebuilder, journal = recorder.journal()) => {
    if (journal === null) {
      throw new Error("the session's record of host calls is full, so it cannot be rebuilt");
    }
    const record = recorded(journal);
    const cap = recorder.cap;
    restart(new Recorder(cap, record.execution));
    const rebuilt = { kept: [], skipped: [], dropped: [], reason: null };
    const kept = [];
    let ranAndDropped = false;
    for (const [at, input] of record.inputs.entries()) {
      if (input.end?.kind === "stopped" || input.end?.kind === "cancelled") {
        rebuilt.skipped.push(input.number);
        continue;
      }
      let built;
      let outcome;
      try {
        built = await rebuilder.build(input.number);
        if (!ran(input, built.source)) {
          throw new Error(`input ${input.number} is not the text the session ran`);
        }
        ranAndDropped = true;
        outcome = await replayInput(input, built.source, built.wasm);
        ranAndDropped = false;
      } catch (e) {
        rebuilt.dropped = record.inputs.slice(at).map((i) => i.number);
        rebuilt.reason = String(e?.message ?? e);
        break;
      }
      await rebuilder.replayed?.(input.number, outcome.joined);
      rebuilt.kept.push(input.number);
      kept.push([input, built]);
    }
    if (ranAndDropped) {
      restart(new Recorder(cap, record.execution));
      for (const [input, { source, wasm }] of kept) {
        try {
          await replayInput(input, source, wasm);
        } catch (e) {
          throw new Error(`the rebuild did not repeat itself: ${e.message}`);
        }
      }
    }
    return rebuilt;
  };

  return {
    input: runInput,
    rebuild,
    // A copy of the journal so far, or null once it passed its cap.
    journal: () => recorder.journal()?.slice() ?? null,
    setJournalCap: (cap) => recorder.setCap(cap),
    // True once, after the input in which the journal passed its cap.
    takeJournalNotice: () => recorder.takeNotice(),
    stop: () => {
      stopped = true;
      wakeWait?.();
    },
    hostCalls: () => calls,
    close: () => host.close(),
  };
}
