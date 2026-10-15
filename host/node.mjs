// The Node layer of the JS host: the providers that touch the machine
// (`FsRead`, `FsWrite`, `Process`, `Net`, `Http`, `Env`, `Sys`,
// `ConsoleInput`, the test runner's temp-directory hook, and the files of
// a test run), added to the platform-neutral core (core.mjs) through its
// `layer` parameter. `createHost` is what `run.mjs`, `test.mjs` and the
// conformance worker call; the grant and the host-name check are what the
// client threads (`http.mjs`, `net.mjs`) share.

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
import {
  arch as osArch,
  availableParallelism,
  constants as osConstants,
  hostname,
  platform,
  tmpdir,
} from "node:os";
import { dirname, basename, join, resolve, sep } from "node:path";
import {
  createCoreHost,
  createGrant as createCoreGrant,
  createSession,
  Deadlock,
  Dec,
  EndCase,
  Enc,
  HostContract,
  PROGRAM_TIME_LIMIT,
  TimeLimit,
  timeLimitReport,
} from "./core.mjs";

export { Deadlock, Dec, EndCase, Enc, PROGRAM_TIME_LIMIT, timeLimitReport };

// A path with `..` and `.` resolved and symbolic links followed as far as
// it exists, so no path escapes a granted directory
// (cli.cap.scope.path.resolved); `hd_run::resolve_path` on the Rust side.
// `realpathSync.native`, like Rust's canonicalize, asks the OS, so on Windows
// both expand 8.3 short names (RUNNER~1) and agree on the drive letter case.
export function resolvePath(path) {
  const rest = [];
  let base = resolve(path);
  for (;;) {
    try {
      return join(realpathSync.native(base), ...rest.reverse());
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
  const name =
    /^(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*\.?$/;
  return isIP(host) !== 0 || name.test(host);
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
  return v === undefined
    ? e
        .leb(1)
        .leb(FS.Other)
        .str(String(err?.message ?? err))
    : fsErr(e, path, v);
};
// Windows reports a path below a file as ENOENT, or as ENOTDIR on some
// libuv paths, so the code is not stable there. A failed open whose nearest
// existing ancestor is not a directory is `NotADirectory`
// (std-fs.memory.error.not-directory); a missing ancestor chain stays
// `NotFound`. The Rust host's `below_a_file` walks the same way.
const opening = (path, open) => {
  try {
    return open();
  } catch (err) {
    if (err?.code === "ENOENT" || err?.code === "ENOTDIR") {
      for (let up = dirname(path); ; up = dirname(up)) {
        let st;
        try {
          st = statSync(up);
        } catch {
          if (dirname(up) === up) break;
          continue;
        }
        if (!st.isDirectory()) throw Object.assign(err, { code: "ENOTDIR" });
        break;
      }
    }
    throw err;
  }
};
// An `Entry`: path, kind, size.
const entry = (e, path, st) => {
  const kind = st.isSymbolicLink() ? KIND.Symlink : st.isDirectory() ? KIND.Directory : KIND.File;
  return e
    .str(path)
    .leb(kind)
    .leb(kind === KIND.Directory ? 0 : st.size);
};
// A waiting method's `.start`/`.finish` pair from one table.
const waiting = (ops) => {
  const o = {};
  for (const [name, { start, finish }] of Object.entries(ops)) {
    o[`${name}.start`] = start;
    o[`${name}.finish`] = finish;
  }
  return o;
};
// A `.finish` of an operation that never pends: a stale handle.
const never = () => {
  throw new HostContract("a finish of an operation that is not pending");
};
// One path argument, decoded.
const one = (d) => [d.str()];

// A client thread running `file` (`http.mjs`, `net.mjs`), started on its
// first call. `call(request, left)` posts the request and waits for the
// thread's answer synchronously, so an operation finishes inside its
// `.start`. It waits at most `left()` milliseconds, the time before the
// run's deadline: past it, the thread stops, since it still holds the
// operation, and the call throws the `time-limit` panic.
const syncClient = (file) => {
  let client = null;
  // Stops the thread, closing whatever it holds open.
  const stop = () => {
    client?.worker.terminate();
    client = null;
  };
  return {
    call: (request, left = () => Infinity) => {
      if (left() <= 0) throw new TimeLimit();
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
      if (Atomics.wait(flag, 0, 0, Math.max(0, left())) === "timed-out") {
        stop();
        throw new TimeLimit();
      }
      return receiveMessageOnPort(port).message;
    },
    stop,
  };
};

// HTTP requests hold nothing open, so one client serves every instance.
const httpClient = syncClient("http.mjs");
const httpSend = (request, left) => httpClient.call(request, left);

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

// `hd:ConsoleInput`: the next input line, `.None` at the end, `.Err` when
// it cannot be read. `env.readLine`, when a host has one, reads it instead
// of `lineReader` (run.mjs, whose main thread reads for a worker).
function consoleInputImports({ env, flush, ready }) {
  const readLine = env.readLine ?? lineReader(env.stdin === undefined ? 0 : env.stdin);
  return {
    // `Result[string?, ConsoleError]`: the next line, `.None` at the end
    // of input, `.Err(.Closed)` when it cannot be read.
    "read_line.start": () => {
      // A completed prompt must reach the sink before this read blocks.
      flush();
      const line = readLine();
      const e = new Enc();
      if (line === false) return ready(e.leb(1).leb(0));
      return ready(line === null ? e.leb(0).leb(0) : e.leb(0).leb(1).str(line));
    },
    "read_line.finish": never,
  };
}

// The host system's four reads (std-sys.*). The grant covers each by
// its method's name (cli.cap.scope.sys); a refused or unanswerable
// read is `.Err` with that name (std-sys.not-granted, .unsupported).
function sysImports({ grant, put }) {
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
  return {
    os: () => sysRead("os", (e) => e.str(OS_NAMES[platform()] ?? platform())),
    arch: () => sysRead("arch", (e) => e.str(ARCH_NAMES[osArch()] ?? osArch())),
    hostname: () => sysRead("hostname", (e) => e.str(hostname())),
    cpu_count: () => sysRead("cpu_count", (e) => e.leb(availableParallelism())),
  };
}

// `hd:Http`: one `send`, decoded, made, and encoded (std-http.send.*).
function httpImports({ argsOf, ready, grants, left }) {
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
    const r = httpSend({ method, url, headers, body, timeout, grants }, left);
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
  return waiting({
    send: { start: (len) => ready(httpRequest(argsOf(len))), finish: never },
  });
}

// `hd:Net` and its socket handles (spec/std/net.md).
function netImports({ argsOf, ready, put, grant, netClient, text }) {
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
  return {
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
          // Preserve ABI decoding, then reject closed and unknown handles
          // before destination validation. This checks metadata only.
          const checked = netClient.call({ op: "check", handle: h });
          if (checked.unknown || checked.disposed) return ready(handleResult(checked, none));
          // Each live send still checks its destination grant.
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
  };
}

// `hd:Env`: granted variables read, the rest read as unset with one
// notice per name.
function envImports({ grant, sink, st, flush, put, text }) {
  return {
    // A variable outside the grant reads as unset; when it is set, a
    // notice names the flag, once per name (cli.cap.env.notice).
    get: (len) => {
      const name = text(len);
      const value = process.env[name];
      const e = new Enc();
      if (!grant.coversName("Env", name)) {
        if (value !== undefined && !st.noticed.has(name)) {
          st.noticed.add(name);
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
  };
}

// `hd:FsRead` and `hd:FsWrite`: the file providers (std.fs).
function fsImports({ fsOp }) {
  return {
    "hd:FsRead": waiting({
      read_bytes: fsOp("FsRead", one, (e, [p]) =>
        e.leb(0).bytes(opening(p, () => readFileSync(p))),
      ),
      read_text: fsOp("FsRead", one, (e, [p]) => {
        const bytes = opening(p, () => readFileSync(p));
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
        const names = opening(p, () => readdirSync(p)).sort((a, b) =>
          Buffer.compare(Buffer.from(a), Buffer.from(b)),
        );
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
        opening(p, () => writeFileSync(p, d.bytes()));
        e.leb(0);
      }),
      write_text: fsOp("FsWrite", one, (e, [p], d) => {
        opening(p, () => writeFileSync(p, d.bytes()));
        e.leb(0);
      }),
      append_text: fsOp("FsWrite", one, (e, [p], d) => {
        opening(p, () => appendFileSync(p, d.bytes()));
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
  };
}

// `hd:Process`: running host programs (`Process.run!`).
function processImports({ argsOf, grant, env, ready, left }) {
  // `Process.run!` (cli.host.default-profile, cli.test.process): the test
  // runner's provider starts the package's executables and tasks by name,
  // the default profile's a host program.
  const runProcess = (name, argv, input) => {
    const e = new Enc();
    if (!grant.coversName("Process", name)) return e.leb(1).leb(PROC.NotGranted);
    // The child runs at most until the run's deadline.
    const ms = left();
    if (ms <= 0) throw new TimeLimit();
    const timeout = Number.isFinite(ms) ? Math.ceil(ms) : undefined;
    let r;
    if (env.programs) {
      const p = env.programs[name];
      if (!p) return e.leb(1).leb(PROC.NotFound);
      r = spawnSync(process.execPath, [p.host, p.wasm, p.config, ...argv], {
        input,
        cwd: p.cwd,
        maxBuffer: 1 << 30,
        timeout,
      });
    } else {
      try {
        r = spawnSync(name, argv, {
          input,
          cwd: process.cwd(),
          env: process.env,
          maxBuffer: 1 << 30,
          timeout,
        });
      } catch (error) {
        if (name === "") return e.leb(1).leb(PROC.NotFound);
        return e.leb(1).leb(PROC.Other).str(String(error.message));
      }
    }
    if (r.error) {
      const code = r.error.code;
      if (code === "ETIMEDOUT") throw new TimeLimit();
      if (code === "ENOENT") return e.leb(1).leb(PROC.NotFound);
      if (code === "EACCES" || code === "EPERM") return e.leb(1).leb(PROC.PermissionDenied);
      return e.leb(1).leb(PROC.Other).str(String(r.error.message));
    }
    // std-process.child.signal: a child ended by a signal reports 128 plus
    // the signal number.
    const status = r.status ?? 128 + (osConstants.signals[r.signal] ?? 0);
    return e.leb(0).str(r.stdout.toString("utf8")).str(r.stderr.toString("utf8")).zz(status);
  };
  return waiting({
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
  });
}

// The files of a test run on disk (cases.mjs): the case's temporary
// directories, its snapshot files and its regression streams.
export const nodeFiles = {
  join,
  read: (path) => (existsSync(path) ? readFileSync(path, "utf8") : null),
  write: (path, text) => {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, text);
  },
  remove: (dir) => rmSync(dir, { recursive: true, force: true }),
};

// `hd:TestRunner`: the temp directory of a case
// (std-testing.runner.temp-dir); the core has the other hooks.
function testRunnerImports({ env, put, st }) {
  const tempDir = () => {
    if (!env.tempDir) {
      // A run without a configured directory makes its own.
      st.ownTemp ??= mkdtempSync(join(tmpdir(), "hd-case-"));
      return st.ownTemp;
    }
    if (!st.tempMade) mkdirSync(env.tempDir, { recursive: true });
    st.tempMade = true;
    return env.tempDir;
  };
  return {
    imports: {
      temp_dir: () => put(new Enc().str(tempDir())),
    },
    tempDir,
  };
}

// A relative path is from `cwd` (the process's by default), with `..` and
// links resolved (cli.cap.scope.path.resolved).
const resolveTarget = (path, cwd = process.cwd()) => resolvePath(resolve(cwd, path));

// The grant with Node's path resolution.
export const createGrant = (limits = {}) => createCoreGrant(limits, resolveTarget, sep);

// The Node layer for `createCoreHost`: the providers that touch the
// machine, and what releases them.
const nodeLayer = {
  resolveTarget,
  sep,
  files: nodeFiles,
  providers: ({ env, sink, st, grant, flush, put, ready, text, argsOf, left }) => {
    // `Net` and its socket handles (spec/std/net.md) on a client thread of
    // this instance's own, which holds its sockets; `close` stops it.
    const net = syncClient("net.mjs");
    const netClient = { call: (request) => net.call(request, left) };
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
    const testRunner = testRunnerImports({ env, put, st });
    return {
      imports: {
        "hd:ConsoleInput": consoleInputImports({ env, flush, ready }),
        "hd:Sys": sysImports({ grant, put }),
        "hd:Http": httpImports({ argsOf, ready, grants: env.grants ?? {}, left }),
        "hd:Net": netImports({ argsOf, ready, put, grant, netClient, text }),
        "hd:Env": envImports({ grant, sink, st, flush, put, text }),
        ...fsImports({ fsOp }),
        "hd:Process": processImports({ argsOf, grant, env, ready, left }),
        "hd:TestRunner": testRunner.imports,
      },
      tempDir: testRunner.tempDir,
      // Removes a directory the host made for itself, and closes its
      // sockets.
      close: () => {
        net.stop();
        if (st.ownTemp) {
          try {
            rmSync(st.ownTemp, { recursive: true, force: true });
          } catch {
            // Already gone.
          }
        }
      },
    };
  },
};

// One host per instance on Node; `sink` and `env` are `createCoreHost`'s.
// `env.stdin` is a file descriptor, or `null` for a closed standard input.
export const createHost = (sink, env = {}) => createCoreHost(sink, env, nodeLayer);

// A REPL session on Node (core.mjs `createSession`); `sink` and `env` are
// `createHost`'s.
export const createNodeSession = (sink, env = {}, options = {}) =>
  createSession(sink, env, nodeLayer, options);
