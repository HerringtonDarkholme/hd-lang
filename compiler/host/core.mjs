// The JS host of `hd run` and `hd test` on V8 (Node): the import object of
// the ABI in `hd_host_abi` (runtime-and-host.md §17.2, §17.10), the
// exchange-buffer codecs of §17.4, the default profile's providers, the
// test runner's, and the entry driver of suspension.md §14.4.
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
//   left to wait for is a deadlock.
// A trap is a panic: the run's status is 3 (never 101,
// module.profile.panic-status).

import {
  appendFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  readSync,
  realpathSync,
  renameSync,
  rmdirSync,
  rmSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { isIP } from "node:net";
import { MessageChannel, receiveMessageOnPort, Worker } from "node:worker_threads";
import { randomBytes } from "node:crypto";
import { arch as osArch, availableParallelism, hostname, platform, tmpdir } from "node:os";
import { dirname, basename, join, resolve } from "node:path";

export class Deadlock extends Error {}

// Thrown by a test runner's `PropertyRunner.start` when the case has
// nothing left to run: the instance ends, and the runner counts the case
// as discarded (std-testing.runner.examples-done).
export class EndCase extends Error {}

// A host result that breaks the ABI: the `host-contract` panic.
class HostContract extends Error {}

// A test body that ran longer than its `timeout`: the `time-limit` panic
// (std-testing.option.timeout-any-duration, flow.panic.time-limit).
class TimeLimit extends Error {}

// The panic report of a body that overran its timeout of `ms`
// milliseconds.
export const timeLimitReport = (ms) =>
  `panic: time-limit: the test case ran longer than its timeout of ${ms}ms\n`;

// A path with `..` and `.` resolved and symbolic links followed as far as
// it exists, so no path escapes a granted directory
// (cli.cap.scope.path.resolved); `hd_run::resolve_path` on the Rust side.
export function resolvePath(path) {
  const rest = [];
  let base = resolve(path);
  for (;;) {
    try {
      return join(realpathSync(base), ...rest.reverse());
    } catch {
      const parent = dirname(base);
      if (parent === base) return resolve(path);
      rest.push(basename(base));
      base = parent;
    }
  }
}

// Whether `host` is a name or an IP address
// (std-net.error.invalid-address).
export function validHost(host) {
  const name = /^(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*\.?$/;
  return isIP(host) !== 0 || name.test(host);
}

// A program's capability grant as `hd` resolved it (Grant Precedence):
// each trait with a limit maps to `false`, a total deny, or the list of its
// scope entries, whose paths are absolute and resolved; a trait it leaves
// out has no limit. A provider asks these before it touches a resource
// (cli.host.default-profile.granted), as `hd_run::Grants` does.
export function createGrant(limits = {}) {
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
    coversPath: (key, path, cwd = process.cwd()) => {
      const target = resolvePath(resolve(cwd, path));
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
      check(key, (e) => (key === "Env" && e.endsWith("*") ? name.startsWith(e.slice(0, -1)) : e === name)),
  };
}

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
    return this.bytes(Buffer.from(s, "utf8"));
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
    return Buffer.from(this.bytes()).toString("utf8");
  }
  list(each) {
    const n = this.num();
    const out = [];
    for (let i = 0; i < n; i++) out.push(each(this));
    return out;
  }
}

// `FsError`'s variants (std.fs), in declaration order.
const FS = {
  NotFound: 0,
  PermissionDenied: 1,
  NotGranted: 2,
  AlreadyExists: 3,
  NotADirectory: 4,
  IsADirectory: 5,
  InvalidUtf8: 6,
  Other: 7,
};
const FS_CODES = {
  ENOENT: FS.NotFound,
  EACCES: FS.PermissionDenied,
  EPERM: FS.PermissionDenied,
  EEXIST: FS.AlreadyExists,
  ENOTDIR: FS.NotADirectory,
  EISDIR: FS.IsADirectory,
};
// `EntryKind`'s variants.
const KIND = { File: 0, Directory: 1, Symlink: 2 };
// `Method`'s variants (std.http), by index, as the wire names them.
const METHODS = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];
// `HttpError`'s variants (std.http).
const HTTP = {
  NotGranted: 0,
  InvalidUrl: 1,
  Dns: 2,
  Connect: 3,
  Tls: 4,
  Timeout: 5,
  TooManyRedirects: 6,
  Other: 7,
};

// A client thread running `file` (`http.mjs`, `net.mjs`), started on its
// first call. `call(request)` posts the request and waits for the thread's
// answer synchronously, so an operation finishes inside its `.start`.
const syncClient = (file) => {
  let client = null;
  return {
    call: (request) => {
      if (client === null) {
        const { port1, port2 } = new MessageChannel();
        const flag = new Int32Array(new SharedArrayBuffer(4));
        const worker = new Worker(new URL(`./${file}`, import.meta.url), {
          workerData: { port: port2, flag },
          transferList: [port2],
        });
        worker.unref();
        port1.unref();
        client = { port: port1, flag, worker };
      }
      const { port, flag } = client;
      Atomics.store(flag, 0, 0);
      port.postMessage(request);
      Atomics.wait(flag, 0, 0);
      return receiveMessageOnPort(port).message;
    },
    // Stops the thread, closing whatever it holds open.
    stop: () => {
      client?.worker.terminate();
      client = null;
    },
  };
};

// HTTP requests hold nothing open, so one client serves every instance.
const httpClient = syncClient("http.mjs");
const httpSend = (request) => httpClient.call(request);

// `NetError`'s variants (std.net), and `ResourceError`'s.
const NET = { NotGranted: 0, InvalidAddress: 1, Dns: 2, Refused: 3, Other: 4 };
const RES = { Operation: 0, Disposed: 1 };

// `SysError`'s variants (std.sys).
const SYS = { NotGranted: 0, Unsupported: 1 };
// The names `std.sys` gives Node's platforms and architectures
// (std-sys.os, std-sys.arch); any other is Node's own name.
const OS_NAMES = { darwin: "macos", win32: "windows" };
const ARCH_NAMES = { x64: "x86_64", arm64: "aarch64", ia32: "x86" };
// `ProcessError`'s variants (std.process).
const PROC = { NotFound: 0, PermissionDenied: 1, NotGranted: 2, Other: 3 };

// `.Err(FsError)` of a failed file operation on `path`.
const fsErr = (e, path, variant) => {
  e.leb(1).leb(variant);
  return variant === FS.Other ? e : e.str(path);
};
const fsFail = (e, path, err) => {
  const v = FS_CODES[err?.code];
  return v === undefined ? e.leb(1).leb(FS.Other).str(String(err?.message ?? err)) : fsErr(e, path, v);
};
// An `Entry`: path, kind, size.
const entry = (e, path, st) => {
  const kind = st.isSymbolicLink() ? KIND.Symlink : st.isDirectory() ? KIND.Directory : KIND.File;
  return e.str(path).leb(kind).leb(kind === KIND.Directory ? 0 : st.size);
};

// Reads standard input one line at a time, synchronously
// (cli.host.default-profile.input-closed): the line without its ending,
// `null` at the end of input (and every time after), or `false` when
// standard input cannot be read.
function lineReader(fd) {
  // A closed standard input is at its end (cli.test.env.stdin).
  if (fd === null) return () => null;
  let pending = Buffer.alloc(0);
  let ended = false;
  const chunk = Buffer.alloc(65536);
  const waiter = new Int32Array(new SharedArrayBuffer(4));
  return () => {
    for (;;) {
      const nl = pending.indexOf(10);
      if (nl >= 0) {
        let line = pending.subarray(0, nl);
        pending = pending.subarray(nl + 1);
        if (line.length && line[line.length - 1] === 13) line = line.subarray(0, line.length - 1);
        return line.toString("utf8");
      }
      if (ended) {
        if (!pending.length) return null;
        const line = pending.toString("utf8");
        pending = Buffer.alloc(0);
        return line;
      }
      let n;
      try {
        n = readSync(fd, chunk, 0, chunk.length, null);
      } catch (err) {
        if (err.code === "EAGAIN") {
          Atomics.wait(waiter, 0, 0, 5);
          continue;
        }
        if (err.code === "EOF") {
          n = 0;
        } else {
          return false;
        }
      }
      if (n === 0) ended = true;
      else pending = Buffer.concat([pending, chunk.subarray(0, n)]);
    }
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
// (cli.test.process), and its `runner`, the hooks through which
// `TestRunner.row` and `PropertyRunner` reach it (`row`, `slugSuffix`,
// `start`, `record`, `show`; test.mjs).
export function createHost(sink, env = {}) {
  const args = env.args ?? [];
  const program = env.program ?? "";
  const grant = createGrant(env.grants ?? {});
  const seed = env.seed ?? null;
  const readLine = lineReader(env.stdin === undefined ? 0 : env.stdin);
  let tempMade = false;
  let ownTemp = null;
  const tempDir = () => {
    if (!env.tempDir) {
      // A run without a configured directory makes its own.
      ownTemp ??= mkdtempSync(join(tmpdir(), "hd-case-"));
      return ownTemp;
    }
    if (!tempMade) mkdirSync(env.tempDir, { recursive: true });
    tempMade = true;
    return env.tempDir;
  };
  const out = [];
  let memory = null;
  let reported = false;
  // The case's timeout, which `TestRunner.report_timeout` reports before
  // the body runs (std-testing.runner.timeout): its length, and the
  // `performance.now()` time by which the body must end.
  let timeoutMs = null;
  let deadline = null;
  const overran = () => deadline !== null && performance.now() > deadline;
  // Waits at most until `d`, and never past the timeout: a pending host
  // wait is cut at the deadline (runtime-and-host.md §17.8).
  const until = (d) => (deadline === null ? d : Math.min(d, deadline));
  const view = (len) => new Uint8Array(memory.buffer, 0, len);
  // A copy of the argument bytes, which a result may overwrite.
  const argsOf = (len) => new Dec(Uint8Array.from(view(len)));
  const text = (len) => Buffer.from(view(len)).toString("utf8");
  // Writes an encoded result at offset 0, growing the buffer to fit, and
  // returns its length (§17.3).
  const put = (e) => {
    const bytes = e.done();
    if (bytes.length > memory.buffer.byteLength) {
      memory.grow(Math.ceil((bytes.length - memory.buffer.byteLength) / 65536));
    }
    new Uint8Array(memory.buffer, 0, bytes.length).set(bytes);
    return bytes.length;
  };
  // A waiting method that finished at once (§17.2): status 0, the length.
  const ready = (e) => [0, put(e)];
  const flush = () => {
    if (out.length) {
      sink.out(out.join(""));
      out.length = 0;
    }
  };
  const noticed = new Set();

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
  const deadlock = () => {
    flush();
    sink.err("panic: deadlock: the program waits, but no host operation is pending\n");
    reported = true;
    throw new Deadlock();
  };
  const sleeper = new Int32Array(new SharedArrayBuffer(4));
  const finish = (h) => {
    ops.delete(h);
  };
  // A `.finish` of an operation that never pends: a stale handle.
  const never = () => {
    throw new HostContract("a finish of an operation that is not pending");
  };
  // A waiting file operation on the paths that `paths(d)` reads from the
  // arguments; `run(e, paths, d)` does it once the grant covers each path.
  const fsOp = (key, paths, run) => ({
    start: (len) => {
      const d = argsOf(len);
      const ps = paths(d);
      const e = new Enc();
      const refused = ps.find((p) => !grant.coversPath(key, p));
      if (refused !== undefined) return ready(fsErr(e, refused, FS.NotGranted));
      try {
        run(e, ps, d);
      } catch (err) {
        return ready(fsFail(new Enc(), ps[ps.length - 1], err));
      }
      return ready(e);
    },
    finish: never,
  });
  const waiting = (ops) => {
    const o = {};
    for (const [name, { start, finish }] of Object.entries(ops)) {
      o[`${name}.start`] = start;
      o[`${name}.finish`] = finish;
    }
    return o;
  };
  const one = (d) => [d.str()];
  // Snapshot files of the case (std-testing.snapshot-file.*).
  let snapshots = 0;
  const snapshotCheck = (actual) => {
    const s = env.snapshot;
    snapshots += 1;
    if (!s) return "no snapshot file: this run names no snapshot directory";
    // An `it_each` row's slug adds the row's index
    // (std-testing.snapshot-file.row).
    const slug = s.slug + (env.runner?.slugSuffix() ?? "");
    const file = join(s.dir, `${slug}-${snapshots}.snap`);
    const shown = s.shown ? `${s.shown}/${slug}-${snapshots}.snap` : file;
    if (existsSync(file)) {
      const expected = readFileSync(file, "utf8");
      if (expected === actual) return "";
      if (!s.update) {
        return `the text differs from the snapshot file ${shown}: expected ${JSON.stringify(expected)}, actual ${JSON.stringify(actual)}; run \`hd test --update\` to record it`;
      }
    } else if (!s.update) {
      return `the snapshot file ${shown} is missing; run \`hd test --update\` to record it`;
    }
    mkdirSync(s.dir, { recursive: true });
    writeFileSync(file, actual);
    return "";
  };
  // `Process.run!` (cli.host.default-profile, cli.test.process): the test
  // runner's provider starts the package's executables and tasks by name,
  // the default profile's a host program.
  const runProcess = (name, argv, input) => {
    const e = new Enc();
    if (!grant.coversName("Process", name)) return e.leb(1).leb(PROC.NotGranted);
    let r;
    if (env.programs) {
      const p = env.programs[name];
      if (!p) return e.leb(1).leb(PROC.NotFound);
      r = spawnSync(process.execPath, [p.host, p.wasm, p.config, ...argv], {
        input,
        cwd: p.cwd,
        maxBuffer: 1 << 30,
      });
    } else {
      r = spawnSync(name, argv, { input, cwd: process.cwd(), env: process.env, maxBuffer: 1 << 30 });
    }
    if (r.error) {
      const code = r.error.code;
      if (code === "ENOENT") return e.leb(1).leb(PROC.NotFound);
      if (code === "EACCES" || code === "EPERM") return e.leb(1).leb(PROC.PermissionDenied);
      return e.leb(1).leb(PROC.Other).str(String(r.error.message));
    }
    const status = r.status ?? 128;
    return e
      .leb(0)
      .str(r.stdout.toString("utf8"))
      .str(r.stderr.toString("utf8"))
      .zz(status);
  };

  // `Result[T, SysError]` of the read `name`: `.Ok` of what `value`
  // encodes, `.Err(.NotGranted(name))` outside the grant, or
  // `.Err(.Unsupported(name))` when the host cannot answer it.
  const sysRead = (name, value) => {
    const e = new Enc();
    if (!grant.coversName("Sys", name)) return put(e.leb(1).leb(SYS.NotGranted).str(name));
    try {
      return put(value(new Enc().leb(0)));
    } catch {
      return put(e.leb(1).leb(SYS.Unsupported).str(name));
    }
  };

  // `Http.send!` (std-http.send.*): decodes the `Request` (its method,
  // URL, header pairs, body, and timeout), makes it on the client thread
  // under the program's grant, and encodes `Result[Response, HttpError]`.
  const httpRequest = (d) => {
    const m = d.num();
    const method = m < METHODS.length ? METHODS[m] : d.str();
    const url = d.str();
    const headers = d.list((d) => [d.str(), d.str()]);
    const body = Uint8Array.from(d.bytes());
    const timeout = d.num() === 1 ? Number(d.zz()) : null;
    const r = httpSend({ method, url, headers, body, timeout, grants: env.grants ?? {} });
    const e = new Enc();
    if (r.error) {
      e.leb(1).leb(HTTP[r.error]);
      return r.error === "Timeout" ? e : e.str(r.text);
    }
    return e
      .leb(0)
      .leb(r.status)
      .list(r.headers, (e, [n, v]) => e.str(n).str(v))
      .bytes(r.body);
  };

  // `Net` and its socket handles (spec/std/net.md) on a client thread of
  // this instance's own, which holds its sockets; `close` stops it.
  const netClient = syncClient("net.mjs");
  // `.Err(NetError)`, for a `Net` method, from a client outcome.
  const netErr = (e, [variant, text]) => e.leb(NET[variant]).str(text);
  // `Result[T, NetError]` of a `Net` method: `ok(e, value)` encodes `T`.
  const netResult = (r, ok) => {
    const e = new Enc();
    return r.error ? netErr(e.leb(1), r.error) : ok(e.leb(0), r.ok);
  };
  // `Result[T, ResourceError[NetError]]` of a handle's method.
  const handleResult = (r, ok) => {
    const e = new Enc();
    if (r.unknown) throw new HostContract("an operation on a handle the host never made");
    if (r.disposed) return e.leb(1).leb(RES.Disposed);
    return r.error ? netErr(e.leb(1).leb(RES.Operation), r.error) : ok(e.leb(0), r.ok);
  };
  const handle = (e, h) => e.leb(h);
  const none = (e) => e;
  // A call names its address `host:port` (std-net.error.address): one
  // whose host is no host is `InvalidAddress`, and one the grant does not
  // cover `NotGranted` (cli.cap.scope.net), before any socket opens.
  const netAt = (op, host, port, more = {}) => {
    const address = `${host}:${port}`;
    if (!validHost(host)) return { error: ["InvalidAddress", address] };
    if (!grant.coversHost("Net", host, port)) return { error: ["NotGranted", address] };
    return netClient.call({ op, host, port, address, ...more });
  };
  const closeHandle = (h) => put(handleResult(netClient.call({ op: "close", handle: h }), none));
  const opened = (op) => (port, len) => {
    const host = argsOf(len).str();
    return ready(netResult(netAt(op, host, port), handle));
  };

  const imports = {
    "hd:rt": {
      stderr: (len) => {
        reported = true;
        flush();
        sink.err(Buffer.from(view(len)).toString("utf8"));
      },
      block: () => {
        for (;;) {
          const n = complete();
          if (n) return n;
          const d = nextDeadline();
          if (d === null) deadlock();
          Atomics.wait(sleeper, 0, 0, Math.max(0, until(d) - performance.now()));
          if (overran()) throw new TimeLimit();
        }
      },
      abort: (h) => {
        ops.delete(h);
      },
    },
    "hd:Console": {
      "write_line.start": (len) => {
        const line = text(len);
        return pending(performance.now(), () => out.push(line + "\n"));
      },
      "write_line.finish": (h) => {
        finish(h);
        view(1)[0] = 0;
        return 1;
      },
    },
    "hd:ConsoleInput": {
      // `Result[string?, ConsoleError]`: the next line, `.None` at the end
      // of input, `.Err(.Closed)` when it cannot be read.
      "read_line.start": () => {
        const line = readLine();
        const e = new Enc();
        if (line === false) return ready(e.leb(1).leb(0));
        return ready(line === null ? e.leb(0).leb(0) : e.leb(0).leb(1).str(line));
      },
      "read_line.finish": never,
    },
    "hd:Clock": {
      now: () => BigInt(Date.now()),
      monotonic: () => BigInt(Math.floor(performance.now())),
      "sleep.start": (ms) => pending(performance.now() + Number(ms), null),
      "sleep.finish": (h) => {
        finish(h);
        return 0;
      },
    },
    // The operating system's random source (cli.host.default-profile).
    // `Random` has no scope entries: a grant of `false` refuses the whole
    // program at startup (cli.cap.total.refuse).
    "hd:Random": {
      next_u64: () => randomBytes(8).readBigInt64LE(0),
      fill: (count) => put(new Enc().bytes(randomBytes(count >>> 0))),
    },
    // The host system's four reads (std-sys.*). The grant covers each by
    // its method's name (cli.cap.scope.sys); a refused or unanswerable
    // read is `.Err` with that name (std-sys.not-granted, .unsupported).
    "hd:Sys": {
      os: () => sysRead("os", (e) => e.str(OS_NAMES[platform()] ?? platform())),
      arch: () => sysRead("arch", (e) => e.str(ARCH_NAMES[osArch()] ?? osArch())),
      hostname: () => sysRead("hostname", (e) => e.str(hostname())),
      cpu_count: () => sysRead("cpu_count", (e) => e.leb(availableParallelism())),
    },
    "hd:Http": waiting({
      send: { start: (len) => ready(httpRequest(argsOf(len))), finish: never },
    }),
    "hd:Net": {
      ...waiting({
        // cli.cap.scope.net.lookup: an entry covers a lookup of its host.
        lookup: {
          start: (len) => {
            const host = text(len);
            const r = !validHost(host)
              ? { error: ["InvalidAddress", host] }
              : grant.coversHostName("Net", host)
                ? netClient.call({ op: "lookup", host })
                : { error: ["NotGranted", host] };
            return ready(netResult(r, (e, found) => e.list(found, (e, a) => e.str(a))));
          },
          finish: never,
        },
        connect: { start: opened("connect"), finish: never },
        listen: { start: opened("listen"), finish: never },
        bind_udp: { start: opened("bind_udp"), finish: never },
        "TcpStream.read": {
          start: (h, max) =>
            ready(handleResult(netClient.call({ op: "read", handle: h, max }), (e, b) => e.bytes(b))),
          finish: never,
        },
        "TcpStream.write": {
          start: (h, len) => {
            const bytes = Uint8Array.from(argsOf(len).bytes());
            return ready(handleResult(netClient.call({ op: "write", handle: h, bytes }), none));
          },
          finish: never,
        },
        "TcpListener.accept": {
          start: (h) => ready(handleResult(netClient.call({ op: "accept", handle: h }), handle)),
          finish: never,
        },
        // std-net.send-to.grant: each datagram's own address.
        "UdpSocket.send_to": {
          start: (h, port, len) => {
            const d = argsOf(len);
            const host = d.str();
            const bytes = Uint8Array.from(d.bytes());
            const r = netAt("send_to", host, port, { handle: h, bytes });
            return ready(handleResult(r, none));
          },
          finish: never,
        },
        "UdpSocket.receive": {
          start: (h, max) =>
            ready(
              handleResult(netClient.call({ op: "receive", handle: h, max }), (e, d) =>
                e.bytes(d.bytes).str(d.host).leb(d.port),
              ),
            ),
          finish: never,
        },
      }),
      // A second `close` is `Disposed` (std-net.handles.closable).
      "TcpStream.close": (h) => closeHandle(h),
      "TcpListener.close": (h) => closeHandle(h),
      "UdpSocket.close": (h) => closeHandle(h),
    },
    "hd:Args": {
      program: () => put(new Enc().str(program)),
      list: () => put(new Enc().list(args, (e, a) => e.str(a))),
    },
    "hd:Env": {
      // A variable outside the grant reads as unset; when it is set, a
      // notice names the flag, once per name (cli.cap.env.notice).
      get: (len) => {
        const name = text(len);
        const value = process.env[name];
        const e = new Enc();
        if (!grant.coversName("Env", name)) {
          if (value !== undefined && !noticed.has(name)) {
            noticed.add(name);
            flush();
            sink.err(`hd: env ${name} is set but not granted; run with --cap Env=${name}\n`);
          }
          return put(e.leb(0));
        }
        return put(value === undefined ? e.leb(0) : e.leb(1).str(value));
      },
      names: () =>
        put(
          new Enc().list(
            Object.keys(process.env).filter((n) => grant.coversName("Env", n)),
            (e, n) => e.str(n),
          ),
        ),
    },
    "hd:FsRead": waiting({
      read_bytes: fsOp("FsRead", one, (e, [p]) => e.leb(0).bytes(readFileSync(p))),
      read_text: fsOp("FsRead", one, (e, [p]) => {
        const bytes = readFileSync(p);
        let s;
        try {
          s = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
        } catch {
          return fsErr(e, p, FS.InvalidUtf8);
        }
        return e.leb(0).str(s);
      }),
      // The entries in the byte order of their names; each path is the
      // directory's, `/`, and the name (as `MemoryFs` lists them).
      list_dir: fsOp("FsRead", one, (e, [p]) => {
        const names = readdirSync(p).sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
        const under = p.endsWith("/") ? p : `${p}/`;
        const found = [];
        for (const n of names) {
          try {
            found.push([under + n, lstatSync(under + n)]);
          } catch {
            // Gone since the listing.
          }
        }
        e.leb(0).list(found, (e, [path, st]) => entry(e, path, st));
      }),
      stat: fsOp("FsRead", one, (e, [p]) => {
        let st;
        try {
          st = lstatSync(p);
        } catch (err) {
          if (err.code === "ENOENT" || err.code === "ENOTDIR") return e.leb(0).leb(0);
          throw err;
        }
        return entry(e.leb(0).leb(1), p, st);
      }),
    }),
    "hd:FsWrite": waiting({
      write_bytes: fsOp("FsWrite", one, (e, [p], d) => {
        writeFileSync(p, d.bytes());
        e.leb(0);
      }),
      write_text: fsOp("FsWrite", one, (e, [p], d) => {
        writeFileSync(p, d.bytes());
        e.leb(0);
      }),
      append_text: fsOp("FsWrite", one, (e, [p], d) => {
        appendFileSync(p, d.bytes());
        e.leb(0);
      }),
      create_dir_all: fsOp("FsWrite", one, (e, [p]) => {
        if (existsSync(p) && !statSync(p).isDirectory()) {
          return fsErr(e, p, FS.NotADirectory);
        }
        mkdirSync(p, { recursive: true });
        e.leb(0);
      }),
      remove: fsOp("FsWrite", one, (e, [p]) => {
        if (lstatSync(p).isDirectory()) rmdirSync(p);
        else unlinkSync(p);
        e.leb(0);
      }),
      // cli.cap.scope.rename: the grant covers both paths.
      rename: fsOp(
        "FsWrite",
        (d) => [d.str(), d.str()],
        (e, [from, to]) => {
          renameSync(from, to);
          e.leb(0);
        },
      ),
    }),
    "hd:Process": waiting({
      run: {
        start: (len) => {
          const d = argsOf(len);
          const name = d.str();
          const argv = d.list((d) => d.str());
          const input = Buffer.from(d.bytes());
          return ready(runProcess(name, argv, input));
        },
        finish: never,
      },
    }),
    "hd:TestRunner": {
      // The test runner (`env.runner`, test.mjs) picks the row an `it_each`
      // case runs; without one, row 0. Timeouts run under no limit here.
      row: (count) => (env.runner ? env.runner.row(count) : 0),
      // The runner fails a body that runs longer than its timeout: here at
      // the end of the run or of a wait, and in test.mjs by stopping an
      // instance that never returns.
      report_timeout: (millis) => {
        timeoutMs = Number(millis);
        deadline = performance.now() + timeoutMs;
        env.runner?.timeout?.(timeoutMs);
      },
      snapshot_check: (len) => put(new Enc().str(snapshotCheck(text(len)))),
      temp_dir: () => put(new Enc().str(tempDir())),
    },
    // The property runner's side of a property case
    // (std-testing.runner.start-case, .record, .show): the test runner
    // decides each case and keeps its draws and its input's text.
    "hd:PropertyRunner": {
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
    },
  };

  // Runs `init`, then polls `poll` to completion. Returns the status (3
  // for a trap) and whether the instance trapped.
  const run = async (instance, init, poll) => {
    memory = instance.exports["hd.x"] ?? null;
    const ex = instance.exports;
    let status = 0;
    let trapped = false;
    let ended = false;
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
        reported = true;
      } else if (e instanceof TimeLimit) {
        reported = true;
      } else if (!(e instanceof WebAssembly.RuntimeError) && !(e instanceof Deadlock)) {
        throw e;
      }
      // A panic stub wrote its report; any other trap is an internal error.
      if (!reported) sink.err(`internal error: ${e.message}\n`);
      status = 3;
      trapped = true;
    }
    flush();
    // A body that ended after its deadline ran longer than its timeout,
    // whatever it did after the deadline.
    if (overran()) {
      sink.err(timeLimitReport(timeoutMs));
      status = 3;
      trapped = true;
    }
    return { status, trapped, ended };
  };

  // Removes a directory the host made for itself, and closes its sockets.
  const close = () => {
    netClient.stop();
    if (ownTemp) {
      try {
        rmSync(ownTemp, { recursive: true, force: true });
      } catch {
        // Already gone.
      }
    }
  };

  return { imports, run, close, args, program, grant, seed, tempDir };
}
