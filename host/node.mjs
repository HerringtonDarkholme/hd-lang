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
  readlinkSync,
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
import { dirname, isAbsolute, join, parse, resolve, sep } from "node:path";
import {
  createCoreHost,
  createGrant as createCoreGrant,
  createSession,
  Deadlock,
  EndCase,
  HostContract,
  httpProvider,
  panicOfReport,
  PROGRAM_TIME_LIMIT,
  reportIn,
  TimeLimit,
  timeLimitReport,
} from "./core.mjs";
import { FsError, METHODS, NetError, Path } from "./gen/abi.mjs";
import { Err, Ok, variant } from "./wire.mjs";

export { Deadlock, EndCase, panicOfReport, PROGRAM_TIME_LIMIT, reportIn, timeLimitReport };

// A path resolved as the OS resolves it, so no path escapes a granted
// directory (cli.cap.scope.path.resolved); `hd_run::resolve_path` on the
// Rust side. The longest existing prefix goes to the OS, then each further
// name follows a symbolic link there (even one whose target does not
// exist) before the `..` after it, as a write or `create_dir_all` would. A
// name that does not exist is kept as it is, and a `..` after it removes
// it. Windows resolves every `..` by its text before it follows any link,
// so there the path is made lexical first. A relative path is from `cwd`.
// `realpathSync.native`, like Rust's canonicalize, asks the OS, so on Windows
// both expand 8.3 short names (RUNNER~1) and agree on the drive letter case.
export function resolvePath(path, cwd = process.cwd(), links = 0) {
  const full = sep === "\\" ? resolve(cwd, path) : isAbsolute(path) ? path : `${cwd}/${path}`;
  const { root } = parse(full);
  const parts = full
    .slice(root.length)
    .split(sep)
    .filter((p) => p !== "" && p !== ".");
  let start = parts.length;
  let out = realOf(root + parts.join(sep));
  while (out === null && start > 0) {
    start -= 1;
    out = realOf(root + parts.slice(0, start).join(sep));
  }
  out ??= root;
  // How many names at the end of `out` do not exist.
  let missing = 0;
  for (const name of parts.slice(start)) {
    if (name === "..") {
      out = dirname(out);
      missing = Math.max(0, missing - 1);
      continue;
    }
    const next = join(out, name);
    const resolved = missing === 0 ? linkStep(next, links) : null;
    if (resolved === null) missing += 1;
    out = resolved ?? next;
  }
  return out;
}

// The same refusal spelling as hd_run::refused_path. Resolving cwd first
// avoids mistaking a link in the working directory itself for a change in
// the requested relative path. Path hints use std.path's / separators.
function refusedPath(path, cwd = process.cwd()) {
  const base = resolvePath(cwd);
  const target = resolvePath(path, cwd);
  if (target === resolve(base, path)) return path;
  if (target === base) return ".";
  const prefix = base.endsWith(sep) ? base : base + sep;
  return (target.startsWith(prefix) ? target.slice(prefix.length) : target).split(sep).join("/");
}

// How many symbolic links one resolution follows before it stops, as the
// OS does with `ELOOP`; the access then fails on its own.
const MAX_LINKS = 40;

// `path` resolved by the OS, or `null` when it does not exist.
function realOf(path) {
  try {
    return realpathSync.native(path);
  } catch {
    return null;
  }
}

// `path`, whose parent is resolved, resolved by the OS, or through the
// symbolic link it names when the link's target does not exist; `null`
// when no such file or link exists.
function linkStep(path, links) {
  const real = realOf(path);
  if (real !== null || links >= MAX_LINKS) return real;
  let target;
  try {
    target = readlinkSync(path);
  } catch {
    return null;
  }
  return resolvePath(target, dirname(path), links + 1);
}

// Whether `host` is a name or an IP address
// (std-net.error.invalid-address).
export function validHost(host) {
  const name =
    /^(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*\.?$/;
  return isIP(host) !== 0 || name.test(host);
}

// The `FsError` variant of a failed file operation's error code.
const FS_CODES = {
  ENOENT: "NotFound",
  EACCES: "PermissionDenied",
  EPERM: "PermissionDenied",
  EEXIST: "AlreadyExists",
  ENOTDIR: "NotADirectory",
  EISDIR: "IsADirectory",
};
// The names `std.sys` gives Node's platforms and architectures
// (std-sys.os, std-sys.arch); any other is Node's own name.
const OS_NAMES = { darwin: "macos", win32: "windows" };
const ARCH_NAMES = { x64: "x86_64", arm64: "aarch64", ia32: "x86" };

// `.Err(FsError)` of the variant `tag` for a file operation on `path`.
const fsErr = (path, tag) => Err(variant(FsError, tag, path));
const fsFail = (path, err) => {
  const tag = FS_CODES[err?.code];
  return tag === undefined
    ? Err({ tag: "Other", message: String(err?.message ?? err) })
    : fsErr(path, tag);
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
// The `FsError` variant for `path` when a name before one of its `..` is
// not an existing directory, as in `data/missing/../a.txt`: NotFound, or
// NotADirectory for a file; `undefined` otherwise. Unix fails such a path
// itself, but Windows resolves `..` as text and skips the missing name, so
// the host checks it on every OS (cli.engine.same-behavior, #277). The
// prefix is joined by hand: `path.join` would resolve the `..` away.
const beforeDotDot = (path) => {
  const parts = path.split(/[\\/]/);
  let prefix = parts[0];
  for (const part of parts.slice(1)) {
    if (part === ".." && prefix !== "" && prefix !== ".") {
      let st;
      try {
        st = statSync(prefix);
      } catch {
        return "NotFound";
      }
      if (!st.isDirectory()) return "NotADirectory";
    }
    prefix = `${prefix}/${part}`;
  }
  return undefined;
};
// An `Entry`: path, kind, size.
const entry = (path, st) => {
  const kind = st.isSymbolicLink() ? "Symlink" : st.isDirectory() ? "Directory" : "File";
  return { path, kind: { tag: kind }, size: kind === "Directory" ? 0 : st.size };
};

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
    // Whether the thread has started, so it may hold something open.
    started: () => client !== null,
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

// How long the workers still running when a run ends have to exit before
// the host ends its own process.
const EXIT_GRACE_MS = 500;

// A worker thread blocked in a system call, such as a read of a FIFO that
// has no writer, cannot be stopped, and Node cannot exit while one runs:
// its exit, `process.exit` too, waits for every worker thread. Once the
// run's output is written, `exitPast(workers, status, statusFile)` gives
// the `workers` still running EXIT_GRACE_MS to exit. Then it writes the
// run's `status` to `statusFile`, when there is one, where `hd` reads it
// in place of the process's exit status, and kills this process.
export function exitPast(workers, status = 0, statusFile = null) {
  // A worker's `threadId` is -1 once its thread stopped.
  const running = workers.filter((w) => w.threadId !== -1);
  if (!running.length) return;
  let left = running.length;
  const timer = setTimeout(() => {
    if (statusFile) writeFileSync(statusFile, `${status}\n`);
    process.kill(process.pid, "SIGKILL");
  }, EXIT_GRACE_MS);
  for (const w of running) {
    w.once("exit", () => {
      if (--left === 0) clearTimeout(timer);
    });
  }
}

// HTTP requests hold nothing open, so one client serves every instance.
const httpClient = syncClient("http.mjs");
const httpSend = (request, left) => httpClient.call(request, left);

// Reads standard input one line at a time, synchronously
// (cli.host.default-profile.input-closed): the line without its ending,
// `null` at the end of input (and every time after), or `false` when
// standard input cannot be read.
function lineReader(fd) {
  // A closed standard input is at its end (cli.test.env.stdin).
  if (fd === null) {
    const none = () => null;
    none.all = () => "";
    return none;
  }
  let pending = Buffer.alloc(0);
  let ended = false;
  const chunk = Buffer.alloc(65536);
  const waiter = new Int32Array(new SharedArrayBuffer(4));
  // Fills `pending` from the descriptor: `false` when it cannot be read.
  const fill = () => {
    let n;
    try {
      n = readSync(fd, chunk, 0, chunk.length, null);
    } catch (err) {
      if (err.code === "EAGAIN") {
        Atomics.wait(waiter, 0, 0, 5);
        return true;
      }
      if (err.code !== "EOF") return false;
      n = 0;
    }
    if (n === 0) ended = true;
    else pending = Buffer.concat([pending, chunk.subarray(0, n)]);
    return true;
  };
  const line = () => {
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
      if (!fill()) return false;
    }
  };
  // The rest of the input as read, with `\r\n` read as `\n`
  // (std-console.input.read-all-host).
  line.all = () => {
    while (!ended) if (!fill()) return false;
    const text = pending.toString("utf8");
    pending = Buffer.alloc(0);
    return text.replaceAll("\r\n", "\n");
  };
  return line;
}

// `hd:ConsoleInput`: the next input line, `.None` at the end, `.Err` when
// it cannot be read. `env.readLine`, when a host has one, reads it instead
// of `lineReader` (run.mjs, whose main thread reads for a worker).
function consoleInputProvider({ env, flush }) {
  const readLine = env.readLine ?? lineReader(env.stdin === undefined ? 0 : env.stdin);
  // A host that reads lines for a worker has no read of the rest: the
  // lines are joined here, each ended by "\n", as the default body of
  // `read_all!` does.
  const readAll =
    readLine.all ??
    (() => {
      let text = "";
      for (let line; (line = readLine()) !== null;) {
        if (line === false) return false;
        text += `${line}\n`;
      }
      return text;
    });
  return {
    read_line: () => {
      // A completed prompt must reach the sink before this read blocks.
      flush();
      const line = readLine();
      return line === false ? Err({ tag: "Closed" }) : Ok(line);
    },
    read_all: () => {
      flush();
      const text = readAll();
      return text === false ? Err({ tag: "Closed" }) : Ok(text);
    },
  };
}

// The host system's four reads (std-sys.*). The grant covers each by
// its method's name (cli.cap.scope.sys); a refused or unanswerable
// read is `.Err` with that name (std-sys.not-granted, .unsupported).
function sysProvider({ grant }) {
  // `Result[T, SysError]` of the read `name`: `.Ok` of what `value`
  // returns, `.Err(.NotGranted(name))` outside the grant, or
  // `.Err(.Unsupported(name))` when the host cannot answer it.
  const sysRead = (name, value) => {
    if (!grant.coversName("Sys", name)) return Err({ tag: "NotGranted", name });
    try {
      return Ok(value());
    } catch {
      return Err({ tag: "Unsupported", name });
    }
  };
  return {
    os: () => sysRead("os", () => OS_NAMES[platform()] ?? platform()),
    arch: () => sysRead("arch", () => ARCH_NAMES[osArch()] ?? osArch()),
    hostname: () => sysRead("hostname", () => hostname()),
    cpu_count: () => sysRead("cpu_count", () => availableParallelism()),
  };
}

// `hd:Net` and its socket handles (spec/std/net.md).
function netProvider({ grant, netClient }) {
  // `Result[T, NetError]` of a `Net` method from a client outcome.
  const netResult = (r) => (r.error ? Err(variant(NetError, ...r.error)) : Ok(r.ok));
  // `Result[T, NetError]` of a handle's method: an operation on a closed
  // handle is `Closed` (std-net.handles.closed).
  const handleResult = (r) => {
    if (r.unknown) throw new HostContract("an operation on a handle the host never made");
    if (r.disposed) return Err(variant(NetError, "Closed"));
    return r.error ? Err(variant(NetError, ...r.error)) : Ok(r.ok);
  };
  // A call names its address `host:port` (std-net.error.address): one
  // whose host is no host is `InvalidAddress`, and one the grant does not
  // cover `NotGranted` (cli.cap.scope.net), before any socket opens.
  const netAt = (op, host, port, more = {}) => {
    const address = `${host}:${port}`;
    if (!validHost(host)) return { error: ["InvalidAddress", address] };
    if (!grant.coversHost("Net", host, port)) return { error: ["NotGranted", address] };
    return netClient.call({ op, host, port, address, ...more });
  };
  const opened = (op) => (host, port) => netResult(netAt(op, host, port));
  const handleOp = (op) => (h, more) => handleResult(netClient.call({ op, handle: h, ...more }));
  // std-net.timeout.set: the `Duration?` crosses to the client thread as
  // milliseconds, or `null` for no bound.
  const setBound = (h, timeout) =>
    handleOp("set_timeout")(h, { timeout: timeout === null ? null : Number(timeout.millis) });
  // std-net.option.nodelay and std-net.option.keepalive: the flag crosses
  // as a number, and the idle `Duration?` as milliseconds, or `null` for off.
  const setNoDelay = (h, enabled) => handleOp("set_nodelay")(h, { enabled: enabled !== 0 });
  const setKeepAlive = (h, idle) =>
    handleOp("set_keepalive")(h, { idle: idle === null ? null : Number(idle.millis) });
  // A second `close` is `Closed` (std-net.handles.closed).
  const close = (h) => handleResult(netClient.call({ op: "close", handle: h }));
  // `dispose!` closes an open handle and reports nothing (std-net.dispose).
  const dispose = (h) => {
    const r = netClient.call({ op: "dispose", handle: h });
    if (r.unknown) throw new HostContract("an operation on a handle the host never made");
  };
  return {
    // cli.cap.scope.net.lookup: an entry covers a lookup of its host.
    lookup: (host) =>
      netResult(
        !validHost(host)
          ? { error: ["InvalidAddress", host] }
          : grant.coversHostName("Net", host)
            ? netClient.call({ op: "lookup", host })
            : { error: ["NotGranted", host] },
      ),
    connect: opened("connect"),
    // std-net.connect-timeout: the `Duration` crosses as milliseconds.
    connect_timeout: (host, port, timeout) =>
      netResult(netAt("connect", host, port, { timeout: Number(timeout) })),
    listen: opened("listen"),
    bind_udp: opened("bind_udp"),
    "TcpStream.read": (h, max) => handleOp("read")(h, { max }),
    "TcpStream.write": (h, bytes) => handleOp("write")(h, { bytes }),
    "TcpStream.set_timeout": setBound,
    "TcpStream.set_nodelay": setNoDelay,
    "TcpStream.set_keepalive": setKeepAlive,
    "TcpStream.close": close,
    "TcpStream.dispose": dispose,
    "TcpListener.accept": (h) => handleOp("accept")(h),
    "TcpListener.set_timeout": setBound,
    "TcpListener.local_address": (h) => handleOp("local_address")(h, {}),
    "TcpListener.close": close,
    "TcpListener.dispose": dispose,
    // std-net.send-to.grant: each datagram's own address.
    "UdpSocket.send_to": (h, host, port, bytes) => {
      // Reject closed and unknown handles before destination validation.
      // This checks metadata only.
      const checked = netClient.call({ op: "check", handle: h });
      if (checked.unknown || checked.disposed) return handleResult(checked);
      // Each live send still checks its destination grant.
      return handleResult(netAt("send_to", host, port, { handle: h, bytes }));
    },
    "UdpSocket.receive": (h, max) => handleOp("receive")(h, { max }),
    "UdpSocket.set_timeout": setBound,
    "UdpSocket.local_address": (h) => handleOp("local_address")(h, {}),
    "UdpSocket.close": close,
    "UdpSocket.dispose": dispose,
  };
}

// `hd:Env`: granted variables read, the rest read as unset with one
// notice per name.
function envProvider({ grant, sink, st, flush }) {
  return {
    // A variable outside the grant reads as unset; when it is set, a
    // notice names the flag, once per name (cli.cap.env.notice).
    get: (name) => {
      const value = process.env[name];
      if (!grant.coversName("Env", name)) {
        if (value !== undefined && !st.noticed.has(name)) {
          st.noticed.add(name);
          flush();
          sink.err(`hd: env ${name} is set but not granted; run with --cap Env=${name}\n`);
        }
        return null;
      }
      return value ?? null;
    },
    names: () => Object.keys(process.env).filter((n) => grant.coversName("Env", n)),
  };
}

// `hd:FsRead` and `hd:FsWrite`: the file providers (std.fs).
function fsProviders({ grant, flush }) {
  // The methods `ops` of `module`, each `run` once the grant covers every
  // path it names, its `Path` arguments (its resource, gen/abi.mjs). A
  // method can block, as a read of a FIFO with no writer does, so the
  // output is flushed first (std-console.host.visible). A method that
  // creates its directories keeps the OS's own rule for a `..` after a
  // missing name, as the Rust host's `create` does.
  const fsOps = (module, ops) =>
    Object.fromEntries(
      Object.entries(ops).map(([name, op]) => {
        const m = METHODS[module][name];
        const { run, create = false } = typeof op === "function" ? { run: op } : op;
        const provider = (...args) => {
          const ps = args.filter((_, i) => m.resource === "path" && m.params[i] === Path);
          const refused = ps.find((p) => !grant.coversPath(module.slice("hd:".length), p));
          if (refused !== undefined) return fsErr(refusedPath(refused), "NotGranted");
          for (const p of create ? [] : ps) {
            const missing = beforeDotDot(p);
            if (missing !== undefined) return fsErr(p, missing);
          }
          flush();
          try {
            return run(...args);
          } catch (err) {
            return fsFail(ps[ps.length - 1], err);
          }
        };
        return [name, provider];
      }),
    );
  return {
    "hd:FsRead": fsOps("hd:FsRead", {
      read_bytes: (p) => Ok(opening(p, () => readFileSync(p))),
      read_text: (p) => {
        const bytes = opening(p, () => readFileSync(p));
        try {
          return Ok(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
        } catch {
          return fsErr(p, "InvalidUtf8");
        }
      },
      // The entries in the byte order of their names; each path is the
      // directory's, `/`, and the name (as `MemoryFs` lists them).
      list_dir: (p) => {
        const names = opening(p, () => readdirSync(p)).sort((a, b) =>
          Buffer.compare(Buffer.from(a), Buffer.from(b)),
        );
        const under = p.endsWith("/") ? p : `${p}/`;
        const found = [];
        for (const n of names) {
          try {
            found.push(entry(under + n, lstatSync(under + n)));
          } catch {
            // Gone since the listing.
          }
        }
        return Ok(found);
      },
      stat: (p) => {
        let st;
        try {
          st = lstatSync(p);
        } catch (err) {
          if (err.code === "ENOENT" || err.code === "ENOTDIR") return Ok(null);
          throw err;
        }
        return Ok(entry(p, st));
      },
    }),
    "hd:FsWrite": fsOps("hd:FsWrite", {
      write_bytes: (p, bytes) => Ok(opening(p, () => writeFileSync(p, bytes))),
      write_text: (p, text) => Ok(opening(p, () => writeFileSync(p, text))),
      append_text: (p, text) => Ok(opening(p, () => appendFileSync(p, text))),
      create_dir_all: {
        run: (p) => {
          if (existsSync(p) && !statSync(p).isDirectory()) return fsErr(p, "NotADirectory");
          mkdirSync(p, { recursive: true });
          return Ok(undefined);
        },
        create: true,
      },
      remove: (p) => {
        if (lstatSync(p).isDirectory()) rmdirSync(p);
        else unlinkSync(p);
        return Ok(undefined);
      },
      // cli.cap.scope.rename: the grant covers both paths.
      rename: (from, to) => Ok(renameSync(from, to)),
    }),
  };
}

// The checks the Rust host makes before it starts a child
// (module.process.command-env, module.process.command-dir): no NUL byte in
// the program, an argument, an environment pair or the directory, an
// environment name that is neither empty nor holds `=`, and a directory
// that exists.
const refusal = (name, argv, pairs, dir) => {
  const other = (message) => Err({ tag: "Other", message });
  if (name.includes("\0")) return other("program name contains a NUL byte");
  const arg = argv.findIndex((a) => a.includes("\0"));
  if (arg >= 0) return other(`process argument ${arg} contains a NUL byte`);
  const bad = pairs.findIndex(([key]) => key === "" || /[=\0]/.test(key));
  if (bad >= 0) return other(`process environment name ${bad} is empty or holds = or a NUL byte`);
  const nul = pairs.findIndex(([, value]) => value.includes("\0"));
  if (nul >= 0) return other(`process environment value ${nul} contains a NUL byte`);
  if (dir !== null && dir.includes("\0")) return other("working directory contains a NUL byte");
  return null;
};
const isDirectory = (path) => {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
};
const noDirectory = (dir) => Err({ tag: "Other", message: `working directory not found: ${dir}` });

// `hd:Process`: running host programs (`Process.run!`, cli.host.default-
// profile, cli.test.process): the test runner's provider starts the
// package's executables and tasks by name, the default profile's a host
// program.
function processProvider({ grant, env, left }) {
  const run = ({ program: name, args: argv, env: pairs, current_dir: dir, stdin: text }) => {
    if (!grant.coversName("Process", name)) return Err({ tag: "NotGranted" });
    // The child runs at most until the run's deadline.
    const ms = left();
    if (ms <= 0) throw new TimeLimit();
    const timeout = Number.isFinite(ms) ? Math.ceil(ms) : undefined;
    // module.process.command-env: the program's environment, then the
    // command's pairs in order, so a later pair for a name wins.
    const childEnv = { ...process.env };
    for (const [key, value] of pairs) {
      // Windows names variables without case.
      const own =
        platform() === "win32"
          ? Object.keys(childEnv).find((k) => k.toUpperCase() === key.toUpperCase())
          : undefined;
      childEnv[own ?? key] = value;
    }
    let r;
    if (env.programs) {
      const p = env.programs[name];
      if (!p) return Err({ tag: "NotFound", program: name });
      const bad = refusal(name, argv, pairs, dir);
      if (bad) return bad;
      const cwd = dir === null ? p.cwd : resolve(p.cwd, dir);
      if (!isDirectory(cwd)) return noDirectory(dir);
      r = spawnSync(process.execPath, [p.host, p.wasm, p.config, ...argv], {
        input: text,
        cwd,
        env: childEnv,
        maxBuffer: 1 << 30,
        timeout,
      });
    } else {
      if (name === "") return Err({ tag: "NotFound", program: name });
      const bad = refusal(name, argv, pairs, dir);
      if (bad) return bad;
      const cwd = dir === null ? process.cwd() : resolve(process.cwd(), dir);
      if (!isDirectory(cwd)) return noDirectory(dir);
      try {
        r = spawnSync(name, argv, {
          input: text,
          cwd,
          env: childEnv,
          maxBuffer: 1 << 30,
          timeout,
        });
      } catch (error) {
        return Err({ tag: "Other", message: String(error.message) });
      }
    }
    // std-process.stdin.unread: a child that exited without reading all of
    // its stdin breaks the pipe; the rest is dropped and the run is `.Ok`.
    // The pipe reports it as EPIPE or ECONNRESET, ENOTCONN on macOS, and EOF
    // on Windows (libuv maps ERROR_BROKEN_PIPE to EOF, as `fill` above meets
    // on reads). A child that never started has neither a status nor a signal.
    const stdinClosed =
      ["EPIPE", "ECONNRESET", "ENOTCONN", "EOF"].includes(r.error?.code) &&
      (r.status !== null || r.signal !== null);
    if (r.error && !stdinClosed) {
      const code = r.error.code;
      if (code === "ETIMEDOUT") throw new TimeLimit();
      if (code === "ENOENT") return Err({ tag: "NotFound", program: name });
      if (code === "EACCES" || code === "EPERM")
        return Err({ tag: "PermissionDenied", program: name });
      return Err({ tag: "Other", message: String(r.error.message) });
    }
    // std-process.child.signal: a child ended by a signal reports 128 plus
    // the signal number.
    const status = r.status ?? 128 + (osConstants.signals[r.signal] ?? 0);
    return Ok({ stdout: r.stdout.toString("utf8"), stderr: r.stderr.toString("utf8"), status });
  };
  return { run };
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
function testRunnerProvider({ env, st }) {
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
  return { provider: { temp_dir: tempDir }, tempDir };
}

// A relative path is from `cwd` (the process's by default), with `..` and
// links resolved (cli.cap.scope.path.resolved).
const resolveTarget = (path, cwd = process.cwd()) => resolvePath(path, cwd);

// The grant with Node's path resolution.
export const createGrant = (limits = {}) => createCoreGrant(limits, resolveTarget, sep);

// The Node layer for `createCoreHost`: the providers that touch the
// machine, and what releases them.
const nodeLayer = {
  resolveTarget,
  sep,
  files: nodeFiles,
  providers: ({ env, sink, st, grant, flush, left }) => {
    // `Net` and its socket handles (spec/std/net.md) on a client thread of
    // this instance's own, which holds its sockets; `close` stops it.
    const net = syncClient("net.mjs");
    // The program's output reaches its stream before it waits on the host
    // (std-console.host.visible): a run stopped in a wait loses none. Each
    // client call and child process asks `waiting()` first.
    const waiting = () => {
      flush();
      return left();
    };
    const netClient = { call: (request) => net.call(request, waiting) };
    const grants = env.grants ?? {};
    const testRunner = testRunnerProvider({ env, st });
    return {
      traits: {
        "hd:ConsoleInput": consoleInputProvider({ env, flush }),
        "hd:Sys": sysProvider({ grant }),
        "hd:Http": httpProvider((request) => httpSend({ ...request, grants }, waiting)),
        "hd:Net": netProvider({ grant, netClient }),
        "hd:Env": envProvider({ grant, sink, st, flush }),
        ...fsProviders({ grant, flush }),
        "hd:Process": processProvider({ grant, env, left: waiting }),
        "hd:TestRunner": testRunner.provider,
      },
      tempDir: testRunner.tempDir,
      // The sockets still open, by kind (cli.test.leak).
      leaks: () => {
        if (!net.started()) return [];
        const counts = net.call({ op: "open" }).ok ?? {};
        return [
          ["TcpStream", counts.stream ?? 0],
          ["TcpListener", counts.listener ?? 0],
          ["UdpSocket", counts.udp ?? 0],
        ].filter(([, n]) => n > 0);
      },
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
