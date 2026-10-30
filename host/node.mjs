// The Node layer of the JS host: the providers that touch the machine
// (`FsRead`, `FsWrite`, `Process`, `Net`, `Http`, `Env`, `Sys`,
// `ConsoleInput`, the test runner's temp-directory hook, and the files of
// a test run), added to the platform-neutral core (core.mjs) through its
// `layer` parameter. `createHost` is what `run.mjs`, `test.mjs` and the
// conformance worker call. Node-specific helpers shared with its client
// threads live below it in `node-files.mjs`.

// module.profile.panic-status).

import { receiveReply } from "./worker-control.mjs";
export { exitPast } from "./run-support.mjs";
import {
  createCoreHost,
  createGrant as createCoreGrant,
  Deadlock,
  EndCase,
  HostContract,
  httpProvider,
  panicOfReport,
  pendingOperation,
  PROGRAM_TIME_LIMIT,
  reportIn,
  TimeLimit,
  timeLimitReport,
} from "./core.mjs";
import { METHODS, NetError, Path } from "./gen/abi.mjs";
import {
  beforeDotDot,
  fileOperation,
  fileOperations,
  fsErr,
  resolvePath,
  validHost,
} from "./node-files.mjs";
import { Err, Ok, variant } from "./wire.mjs";

// Builtins come from `process.getBuiltinModule`, not `import`: an ESM
// import of a builtin builds its module facade, which for `node:fs` loads
// the streams, about 1 ms of every run's startup. `node:net` loads only
// when a host name is checked.
const {
  closeSync,
  openSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readSync,
  rmSync,
  statSync,
  writeFileSync,
} = process.getBuiltinModule("node:fs");
const { MessageChannel, Worker } = process.getBuiltinModule("node:worker_threads");
const {
  arch: osArch,
  availableParallelism,
  hostname,
  platform,
  tmpdir,
} = process.getBuiltinModule("node:os");
const { dirname, join, resolve, sep } = process.getBuiltinModule("node:path");

export { Deadlock, EndCase, panicOfReport, PROGRAM_TIME_LIMIT, reportIn, timeLimitReport };
export { fileOperation, resolvePath, validHost };

// Worker clients get this long to acknowledge cancellation before the
// control thread is replaced.
const EXIT_GRACE_MS = 500;

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

// The names `std.host` gives Node's platforms and architectures
// (std-host.sys.os, std-host.sys.arch); any other is Node's own name.
const OS_NAMES = { darwin: "macos", win32: "windows" };
const ARCH_NAMES = { x64: "x86_64", arm64: "aarch64", ia32: "x86" };

// A client thread running `file` (`http.mjs`, `net.mjs`), started on its
// first call. `call(request, left)` posts the request and waits for the
// thread's control reply synchronously. Pending providers use short
// start/poll/finish/abort requests rather than waiting for their I/O here.
// A control call waits at most `left()` milliseconds before stopping its
// client and throwing the `time-limit` panic.
export const syncClient = (file, close = null) => {
  let client = null;
  let nextCall = 1;
  // Stops the thread, allowing an optional close request to release OS children.
  const stop = () => {
    if (client !== null) {
      if (close === null) client.worker.terminate();
      else {
        // Child processes outlive worker termination. Let this worker kill
        // and reap them even if a short control call hit the run deadline.
        client.worker.ref();
        client.port.postMessage({ id: 0, request: { op: close } });
        client.port.close();
      }
      client = null;
    }
  };
  return {
    // Whether the thread has started, so it may hold something open.
    started: () => client !== null,
    call: (request, left = () => Infinity) => {
      if (left() <= 0) throw new TimeLimit();
      if (client === null) {
        const { port1, port2 } = new MessageChannel();
        const flag = new Int32Array(new SharedArrayBuffer(4));
        const worker = new Worker(new URL(file, import.meta.url), {
          workerData: { port: port2, flag },
          transferList: [port2],
        });
        worker.unref();
        port1.unref();
        client = { port: port1, flag, worker };
      }
      const { port, flag } = client;
      const id = nextCall++;
      Atomics.store(flag, 0, 0);
      port.postMessage({ id, request });
      const reply = receiveReply(port, flag, id, left);
      if (reply === null) {
        stop();
        throw new TimeLimit();
      }
      if (reply.fault !== undefined) {
        stop();
        throw new HostContract(reply.fault);
      }
      return reply.result;
    },
    stop,
  };
};

// Start/poll/abort are short worker control calls. The response body stays
// in the worker until complete; the guest can race its clock meanwhile.
const httpSend = (client, request) => {
  const { ok: operation } = client.call({ ...request, op: "start" });
  let result = null;
  const poll = () => {
    const outcome = client.call({ op: "poll", operation });
    if (outcome.pending) return false;
    result = outcome;
    return true;
  };
  if (poll()) return result;
  return pendingOperation(
    poll,
    () => result,
    () => client.cancel({ op: "abort", operation }),
  );
};

// Reads standard input one line at a time, synchronously
// (cli.host.default-profile.input-closed): the line without its ending,
// `null` at the end of input (and every time after), or `false` when
// standard input cannot be read.
export function lineReader(fd) {
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
  // (std-console.input.read-all-host-coded-error).
  line.all = () => {
    while (!ended) if (!fill()) return false;
    const text = pending.toString("utf8");
    pending = Buffer.alloc(0);
    return text.replaceAll("\r\n", "\n");
  };
  return line;
}

// `hd:ConsoleInput`: the next input line, `.None` at the end, `.Err(.Other(code))`
// when the host descriptor cannot be read. `env.readLine`, when a host has one, reads it instead
// of the input worker (the REPL keeps its framed control reader).
function consoleInputProvider({ env, flush, inputClient }) {
  if (env.readLine === undefined && env.stdin !== null) {
    const read = (kind) => {
      flush();
      const { ok: operation } = inputClient.call({
        op: "wait_start",
        kind,
        fd: env.stdin ?? 0,
      });
      let result = null;
      const poll = () => {
        const outcome = inputClient.call({ op: "wait_poll", operation });
        if (outcome.pending) return false;
        result = outcome;
        return true;
      };
      const finish = () => {
        inputClient.call({ op: "wait_finish", operation });
      };
      const output = () =>
        result.error === undefined
          ? result.closed
            ? Err({ tag: "Closed" })
            : Ok(result.ok)
          : Err({ tag: "Other", code: result.error });
      if (poll()) {
        finish();
        return output();
      }
      return pendingOperation(
        poll,
        output,
        () => inputClient.cancel({ op: "wait_abort", operation }),
        finish,
      );
    };
    return { read_line: () => read("line"), read_all: () => read("all") };
  }
  // The REPL's framed control reader and a test's explicitly closed stdin.

  const readLine = env.readLine ?? lineReader(env.stdin === undefined ? 0 : env.stdin);
  // A custom reader without a read of the rest joins its lines here,
  // each ended by "\n", as the default body of
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

// A missing/unreadable cgroup limit leaves the OS affinity count intact.
const quotaText = (path) => {
  try {
    return readFileSync(path, "utf8").trim();
  } catch {
    return null;
  }
};

// std-host.sys.cpu-count: every ancestor constrains the offered processors.
// Keep the walk rooted at its mount, including when tested on macOS.
// Like Rust's available_parallelism, round quotas down, with at least one.
export function cgroupCpuCount(root, group, offered) {
  root = resolve(root);
  let count = offered;
  let dir = resolve(root, group.replace(/^\/+/, ""));
  const prefix = root.endsWith(sep) ? root : root + sep;
  while (dir === root || dir.startsWith(prefix)) {
    const maximum = quotaText(join(dir, "cpu.max"));
    const [quota, period] =
      maximum === null
        ? [quotaText(join(dir, "cpu.cfs_quota_us")), quotaText(join(dir, "cpu.cfs_period_us"))]
        : maximum.split(/\s+/);
    if (/^\d+$/.test(quota ?? "") && /^\d+$/.test(period ?? "") && BigInt(period) > 0n) {
      count = Math.min(count, Number(BigInt(quota) / BigInt(period)));
    }
    if (dir === root) break;
    dir = dirname(dir);
  }
  return Math.max(1, count);
}

function cpuCount() {
  const offered = availableParallelism();
  if (platform() !== "linux") return offered;
  const membership = quotaText("/proc/self/cgroup");
  if (membership === null) return offered;
  const groups = membership.split("\n").map((line) => /^(\d+):([^:]*):(\/.*)$/.exec(line));
  const cpu = groups.find((group) => group?.[2].split(",").includes("cpu"));
  const unified = groups.find((group) => group?.[1] === "0" && group[2] === "");
  if (!cpu) return unified ? cgroupCpuCount("/sys/fs/cgroup", unified[3], offered) : offered;

  // Rust checks these conventional v1 mounts before scanning mountinfo.
  const group = cpu[3];
  for (const root of ["/sys/fs/cgroup/cpu", "/sys/fs/cgroup/cpu,cpuacct"]) {
    if (existsSync(join(root, group))) return cgroupCpuCount(root, group, offered);
  }
  for (const line of (quotaText("/proc/self/mountinfo") ?? "").split("\n")) {
    const [mounted, filesystem] = line.split(" - ");
    const [kind, , options] = filesystem?.split(" ") ?? [];
    if (kind !== "cgroup" || !options?.split(",").includes("cpu")) continue;
    const [, , , subtree, root] = mounted.split(" ");
    if (group === subtree || group.startsWith(subtree === "/" ? "/" : subtree + "/")) {
      return cgroupCpuCount(root, group.slice(subtree.length), offered);
    }
  }
  return offered;
}

// The host system's four reads (std-host.sys.*). The grant covers each by
// its method's name (cli.cap.scope.sys); a refused or unanswerable
// read is `.Err` with that name (std-host.sys.not-granted, .unsupported).
function sysProvider({ grant }) {
  // `Result[T, HostError]` of the read `name`: `.Ok` of what `value`
  // returns, `.Err(.NotGranted(name))` outside the grant, or
  // `.Err(.Unsupported(name))` when the host cannot answer it.
  const sysRead = (name, value) => {
    if (!grant.coversName("Sys", name)) return Err({ tag: "NotGranted", subject: name });
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
    cpu_count: () => sysRead("cpu_count", cpuCount),
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
  const waitFor = (started, resultOf) => {
    if (started.ok === undefined) return resultOf(started);
    const operation = started.ok;
    let result = null;
    const poll = () => {
      const outcome = netClient.call({ op: "wait_poll", operation });
      if (outcome.pending) return false;
      result = outcome;
      return true;
    };
    const finish = () => {
      const committed = netClient.call({ op: "wait_finish", operation });
      if (committed.disposed) return resultOf(committed);
    };
    if (poll()) return finish() ?? resultOf(result);
    return pendingOperation(
      poll,
      () => resultOf(result),
      () => netClient.cancel({ op: "wait_abort", operation }),
      finish,
    );
  };
  const opened = (kind) => (host, port) =>
    waitFor(netAt("wait_start", host, port, { kind }), netResult);
  const handleOp = (op) => (h, more) => handleResult(netClient.call({ op, handle: h, ...more }));
  const waitOn =
    (kind) =>
    (h, more = {}) =>
      waitFor(netClient.call({ op: "wait_start", handle: h, kind, ...more }), handleResult);
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
    lookup: (host) => {
      if (!validHost(host)) return netResult({ error: ["InvalidAddress", host] });
      if (!grant.coversHostName("Net", host)) return netResult({ error: ["NotGranted", host] });
      return waitFor(netClient.call({ op: "wait_start", kind: "lookup", host }), netResult);
    },
    connect: (host, port) => {
      const started = netAt("connect_start", host, port);
      if (started.error) return netResult(started);
      const operation = started.ok;
      let result = null;
      const poll = () => {
        const outcome = netClient.call({ op: "connect_poll", operation });
        if (outcome.pending) return false;
        result = outcome;
        return true;
      };
      // An already ready connect wins its first-poll tie. Otherwise the
      // guest can poll its clock while the worker owns the pending socket.
      if (poll()) return netResult(result);
      return pendingOperation(
        poll,
        () => netResult(result),
        () => {
          if (result === null) netClient.cancel({ op: "connect_abort", operation });
          // complete() can run before the guest consumes .finish: the
          // cancelled outcome still owns an undelivered stream in that gap.
          else if (result.ok !== undefined) netClient.cancel({ op: "dispose", handle: result.ok });
        },
      );
    },
    listen: opened("listen"),
    bind_udp: opened("bind_udp"),
    "TcpStream.read": (h, max) => waitOn("read")(h, { max }),
    "TcpStream.write": (h, bytes) => waitOn("write")(h, { bytes }),
    "TcpStream.set_timeout": setBound,
    "TcpStream.set_nodelay": setNoDelay,
    "TcpStream.set_keepalive": setKeepAlive,
    "TcpStream.close": close,
    "TcpStream.dispose": dispose,
    "TcpListener.accept": waitOn("accept"),
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
      return waitFor(
        netAt("wait_start", host, port, { kind: "send_to", handle: h, bytes }),
        handleResult,
      );
    },
    "UdpSocket.receive": (h, max) => waitOn("receive")(h, { max }),
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

// Only methods that open a path can wait for a special-file peer. A
// missing or uninspectable path stays inline and reports its ordinary error.
const fileOpening = new Set([
  "read_bytes",
  "read_text",
  "write_bytes",
  "write_text",
  "append_text",
  "copy",
]);
const specialFile = (path) => {
  if (process.platform === "win32") {
    // Inspecting a Windows pipe can itself open/wait on it. Native uses
    // these same device namespace spellings without touching the device.
    const normalized = path.replaceAll("/", "\\").toLowerCase();
    if (
      normalized.startsWith("\\\\.\\") ||
      normalized.startsWith("\\\\?\\pipe\\") ||
      normalized.startsWith("\\\\?\\globalroot\\device\\") ||
      /^\\\\(?:\?\\unc\\)?[^\\]+\\pipe\\/.test(normalized)
    )
      return true;
  }
  try {
    const st = statSync(path);
    return !st.isFile() && !st.isDirectory();
  } catch {
    return false;
  }
};

// `hd:FsRead` and `hd:FsWrite`: the file providers (std.fs).
function fsProviders({ grant, flush, fsClient }) {
  // The methods `ops` of `module`, each `run` once the grant covers every
  // path it names, its `Path` arguments (its resource, gen/abi.mjs). A
  // special-file call is offloaded after these checks. Output is flushed
  // first (std-console.host.visible-text-sync). A method that
  // creates its directories keeps the OS's own rule for a `..` after a
  // missing name, as the Rust host's `create` does.
  const fsOps = (module, ops) =>
    Object.fromEntries(
      Object.entries(ops).map(([name, op]) => {
        const m = METHODS[module][name];
        const { create = false } = typeof op === "function" ? {} : op;
        const provider = (...args) => {
          const ps = args.filter((_, i) => m.resource === "path" && m.params[i] === Path);
          const refused = ps.find(
            (p, i) =>
              !grant.coversPath(
                name === "copy" && i === 0 ? "FsRead" : module.slice("hd:".length),
                p,
              ),
          );
          if (refused !== undefined) return fsErr(refusedPath(refused), "NotGranted");
          for (const p of create ? [] : ps) {
            const missing = beforeDotDot(p);
            if (missing !== undefined) return fsErr(p, missing);
          }
          flush();
          if (fileOpening.has(name) && ps.some(specialFile)) {
            const { ok: operation } = fsClient.call({ op: "wait_start", module, name, args });
            let result = null;
            const poll = () => {
              const outcome = fsClient.call({ op: "wait_poll", operation });
              if (outcome.pending) return false;
              result = outcome.result;
              return true;
            };
            return pendingOperation(
              poll,
              () => result,
              () => fsClient.cancel({ op: "wait_abort", operation }),
              () => {
                fsClient.call({ op: "wait_finish", operation });
              },
            );
          }
          return fileOperation(module, name, args);
        };
        return [name, provider];
      }),
    );
  return Object.fromEntries(
    Object.entries(fileOperations).map(([module, ops]) => [module, fsOps(module, ops)]),
  );
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
function processProvider({ grant, env, left, processClient }) {
  const run = ({ program: name, args: argv, env: pairs, current_dir: dir, stdin: text }) => {
    if (!grant.coversName("Process", name)) return Err({ tag: "NotGranted" });
    const ms = left();
    if (ms <= 0) throw new TimeLimit();
    // module.process.command-env: later pairs win, without case on Windows.
    const childEnv = { ...process.env };
    for (const [key, value] of pairs) {
      const own =
        platform() === "win32"
          ? Object.keys(childEnv).find((k) => k.toUpperCase() === key.toUpperCase())
          : undefined;
      childEnv[own ?? key] = value;
    }
    let program = name;
    let args = argv;
    let cwd;
    if (env.programs) {
      const p = env.programs[name];
      if (!p) return Err({ tag: "NotFound", program: name });
      const bad = refusal(name, argv, pairs, dir);
      if (bad) return bad;
      cwd = dir === null ? p.cwd : resolve(p.cwd, dir);
      program = process.execPath;
      args = [p.host, p.wasm, p.config, ...argv];
    } else {
      if (name === "") return Err({ tag: "NotFound", program: name });
      const bad = refusal(name, argv, pairs, dir);
      if (bad) return bad;
      cwd = dir === null ? process.cwd() : resolve(process.cwd(), dir);
    }
    if (!isDirectory(cwd)) return noDirectory(dir);
    const { ok: operation } = processClient.call({
      op: "wait_start",
      launcher: env.processLauncher ?? null,
      program,
      args,
      cwd,
      env: childEnv,
      input: text,
      timeout: ms,
    });
    let result = null;
    const poll = () => {
      const outcome = processClient.call({ op: "wait_poll", operation });
      if (outcome.pending) return false;
      result = outcome;
      return true;
    };
    const finish = () => processClient.call({ op: "wait_finish", operation });
    const output = () => {
      if (result.timeout) throw new TimeLimit();
      if (result.error) {
        const { code, message } = result.error;
        if (code === "ENOENT") return Err({ tag: "NotFound", program: name });
        if (code === "EACCES" || code === "EPERM")
          return Err({ tag: "PermissionDenied", program: name });
        return Err({ tag: "Other", message });
      }
      return Ok({ ...result.ok, program: name });
    };
    if (poll()) {
      finish();
      return output();
    }
    return pendingOperation(
      poll,
      output,
      () => processClient.cancel({ op: "wait_abort", operation }),
      () => {
        finish();
      },
    );
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

// `count` bytes of the operating system's random source. Reading the
// device needs no `crypto`, whose module costs about 3 ms to load.
function urandom(count) {
  const out = new Uint8Array(count);
  const fd = openSync("/dev/urandom", "r");
  try {
    for (let at = 0; at < count;) at += readSync(fd, out, at, count - at, null);
  } finally {
    closeSync(fd);
  }
  return out;
}

// The Node layer for `createCoreHost`: the providers that touch the
// machine, and what releases them.
export const nodeLayer = {
  randomBytes: process.platform === "win32" ? undefined : urandom,
  resolveTarget,
  sep,
  files: nodeFiles,
  providers: ({ env, sink, st, grant, flush, left }) => {
    // `Net` and its socket handles (spec/std/net.md) on a client thread of
    // this instance's own, which holds its sockets; `close` stops it.
    const net = syncClient("net.mjs");
    const http = syncClient("http.mjs");
    const child = syncClient("process.mjs", "close");
    const files = syncClient("fs.mjs", "close");
    const input = syncClient("input.mjs");
    // The program's output reaches its stream before it waits on the host
    // (std-console.host.visible-text-sync): a run stopped in a wait loses none. Each
    // client call and child process asks `waiting()` first.
    const waiting = () => {
      flush();
      return left();
    };
    const client = (worker) => ({
      call: (request) => worker.call(request, waiting),
      // Cancellation owns no I/O wait. It still releases resources after
      // the program deadline expired, and never starts a replacement worker.
      cancel: (request) =>
        worker.started() ? worker.call(request, () => EXIT_GRACE_MS) : { ok: null },
    });
    const netClient = client(net);
    const httpClient = client(http);
    const grants = env.grants ?? {};
    const testRunner = testRunnerProvider({ env, st });
    return {
      traits: {
        "hd:ConsoleInput": consoleInputProvider({ env, flush, inputClient: client(input) }),
        "hd:Sys": sysProvider({ grant }),
        "hd:Http": httpProvider((request) => httpSend(httpClient, { ...request, grants })),
        "hd:Net": netProvider({ grant, netClient }),
        "hd:Env": envProvider({ grant, sink, st, flush }),
        ...fsProviders({ grant, flush, fsClient: client(files) }),
        "hd:Process": processProvider({ grant, env, left: waiting, processClient: client(child) }),
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
        http.stop();
        child.stop();
        files.stop();
        input.stop();
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

// Loaded only by the REPL child; ordinary programs skip the session graph.
export async function driveNodeSession() {
  const { driveNodeSession: drive } = await import("./node-session.mjs");
  return drive();
}
