// The Node layer of the JS host: the providers that touch the machine
// (`FsRead`, `FsWrite`, `Process`, `Net`, `Http`, `Env`, `Sys`,
// `ConsoleInput`, the test runner's temp-directory hook, and the files of
// a test run), added to the platform-neutral core (core.ts) through its
// `layer` parameter. `createHost` is what `run.ts`, `test.ts` and the
// conformance worker call. Node-specific helpers shared with its client
// threads live below it in `node-files.ts`.

// module.profile.panic-status).

import { isatty } from "node:tty";
import { signalSource } from "./signals.ts";
import { receiveReply } from "./worker-control.ts";
import type { MessagePort, Worker as NodeWorker } from "node:worker_threads";
import type { CoreEnv, Host, Sink } from "./core.ts";
export type { Host, Session, Sink } from "./core.ts";
export { exitPast } from "./run-support.ts";
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
} from "./core.ts";
import { METHODS, NetError, Path } from "./gen/abi.ts";
import {
  beforeDotDot,
  fileOperation,
  fileOperations,
  fsErr,
  readOpen,
  resolvePath,
  validHost,
} from "./node-files.ts";
import { Err, Ok, variant } from "./wire.ts";
import { startThread, workersProvider } from "./workers.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;

export interface HostEnv extends CoreEnv {
  /** A file descriptor, or `null` for a closed standard input. */
  stdin?: number | null;
  /** Explicit hd executable used for inherited Process groups and Windows Jobs. */
  processLauncher?: string;
  tempDir?: string;
  snapshot?: { dir: string; shown?: string; slug: string; update: boolean };
  programs?: Record<string, { host: string; wasm: string; config: string; cwd: string }>;
}

interface WorkerClient {
  port: MessagePort;
  flag: Int32Array<SharedArrayBuffer>;
  worker: NodeWorker;
}

interface SyncClient {
  started(): boolean;
  call(request: Dynamic, left?: () => number): Dynamic;
  stop(): void;
  subscribe(key: string, ready: () => void): () => void;
  drainReady(): void;
}

export interface LineReader {
  (): string | null | false;
  all(): string | false;
  bytes(max: number): Uint8Array | false;
}

interface ErrorWithCode {
  code?: string;
}

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
const { MessageChannel, Worker, receiveMessageOnPort } =
  process.getBuiltinModule("node:worker_threads");
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
function refusedPath(path: string, cwd = process.cwd()): string {
  const base = resolvePath(cwd);
  const target = resolvePath(path, cwd);
  if (target === resolve(base, path)) {
    return path;
  }
  if (target === base) {
    return ".";
  }
  const prefix = base.endsWith(sep) ? base : base + sep;
  return (target.startsWith(prefix) ? target.slice(prefix.length) : target).split(sep).join("/");
}

// The names `std.host` gives Node's platforms and architectures
// (std-host.sys.os, std-host.sys.arch); any other is Node's own name.
const OS_NAMES: Record<string, string> = { darwin: "macos", win32: "windows" };
const ARCH_NAMES: Record<string, string> = { x64: "x86_64", arm64: "aarch64", ia32: "x86" };

// A client thread running `file` (`./http.ts`, `./net.ts`), started on its
// first call. `call(request, left)` posts the request and waits for the
// thread's control reply synchronously. Pending providers use short
// start/poll/finish/abort requests rather than waiting for their I/O here.
// A control call waits at most `left()` milliseconds before stopping its
// client and throwing the `time-limit` panic.
export const syncClient = (
  file: string | URL,
  close: string | null = null,
  reactor?: Int32Array<SharedArrayBuffer>,
): SyncClient => {
  let client: WorkerClient | null = null;
  let nextCall = 1;
  const subscriptions = new Map<string, { ready: boolean; callback?: () => void }>();
  const messages: string[] = [];
  const dispatch = (): void => {
    for (const key of messages) {
      const entry = subscriptions.get(key);
      if (entry && !entry.ready) {
        entry.ready = true;
        entry.callback?.();
      }
    }
    messages.length = 0;
  };
  const operationKey = (request: Dynamic): string =>
    `${request.op.startsWith("connect_") ? "connect" : "wait"}:${request.operation}`;
  const trackReply = (request: Dynamic, result: Dynamic): void => {
    if (
      (request.op === "start" || request.op.endsWith("_start")) &&
      typeof result?.ok === "number"
    ) {
      const key = operationKey({ ...request, operation: result.ok });
      subscriptions.set(key, { ready: false });
    }
    dispatch();
    if (
      request.operation !== undefined &&
      ((request.op.endsWith("poll") && !result?.pending) ||
        request.op.endsWith("abort") ||
        request.op.endsWith("finish"))
    ) {
      subscriptions.delete(operationKey(request));
    }
  };
  const drainReady = (): void => {
    if (!client) {
      return;
    }
    for (;;) {
      const item = receiveMessageOnPort(client.port);
      if (!item) {
        break;
      }
      if (typeof item.message.ready === "string") {
        messages.push(item.message.ready);
      }
    }
    dispatch();
  };
  // Stops the thread, allowing an optional close request to release OS children.
  const stop = () => {
    subscriptions.clear();
    messages.length = 0;
    if (client !== null) {
      if (close === null) {
        client.port.close();
        client.worker.terminate();
      } else {
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
    drainReady,
    subscribe: (key, callback) => {
      const entry = subscriptions.get(key);
      if (!entry) {
        throw new HostContract("subscription of an unknown worker operation");
      }
      entry.callback = callback;
      if (entry.ready) {
        callback();
      }
      return () => {
        subscriptions.delete(key);
      };
    },
    call: (request: Dynamic, left = () => Infinity): Dynamic => {
      if (left() <= 0) {
        throw new TimeLimit();
      }
      if (client === null) {
        const { port1, port2 } = new MessageChannel();
        const flag = new Int32Array(new SharedArrayBuffer(4));
        const worker = new Worker(new URL(file, import.meta.url), {
          workerData: { port: port2, flag, reactor },
          transferList: [port2],
        });
        worker.unref();
        port1.on("message", (message: Dynamic) => {
          if (client?.port === port1 && typeof message.ready === "string") {
            messages.push(message.ready);
            dispatch();
          }
        });
        port1.unref();
        client = { port: port1, flag, worker };
      }
      const { port, flag } = client;
      const id = nextCall++;
      Atomics.store(flag, 0, 0);
      port.postMessage({ id, request });
      const reply = receiveReply(port, flag, id, { left, ready: (key) => messages.push(key) });
      if (reply === null) {
        stop();
        throw new TimeLimit();
      }
      if (reply.fault !== undefined) {
        stop();
        throw new HostContract(reply.fault);
      }
      const result = reply.result;
      trackReply(request, result);
      return result;
    },
    stop,
  };
};

// Start/poll/abort are short worker control calls. The response body stays
// in the worker until complete; the guest can race its clock meanwhile.
const httpSend = (client: Dynamic, request: Dynamic): Dynamic => {
  const { ok: operation } = client.call({ ...request, op: "start" });
  let result: Dynamic = null;
  const poll = () => {
    const outcome = client.call({ op: "poll", operation });
    if (outcome.pending) {
      return false;
    }
    result = outcome;
    return true;
  };
  if (poll()) {
    return result;
  }
  return pendingOperation(
    poll,
    () => result,
    () => client.cancel({ op: "abort", operation }),
    null,
    (ready) => client.subscribe(operation, ready),
  );
};

// Reads standard input one line at a time, synchronously
// (cli.host.default-profile.input-closed): the line without its ending,
// `null` at the end of input (and every time after), or `false` when
// standard input cannot be read.
export function lineReader(fd: number | null): LineReader {
  // A closed standard input is at its end (cli.test.env.stdin).
  if (fd === null) {
    const none = (() => null) as LineReader;
    none.all = () => "";
    none.bytes = () => new Uint8Array();
    return none;
  }
  let pending: Buffer = Buffer.alloc(0);
  let parts: Buffer[] = [];
  let length = 0;
  let ended = false;
  let chunk = Buffer.allocUnsafe(65536);
  // Retain complete fragments until one line (or all input) is requested.
  // `pending` starts at the next unsearched byte; previous fragments are
  // never searched again. Each byte is read once and joined at most once.
  const retain = (bytes: Buffer) => {
    if (bytes.length) {
      parts.push(bytes);
      length += bytes.length;
    }
  };
  const take = () => {
    const bytes = parts.length === 1 ? parts[0] : Buffer.concat(parts, length);
    parts = [];
    length = 0;
    return bytes;
  };
  const waiter = new Int32Array(new SharedArrayBuffer(4));
  // Fills `pending` from the descriptor: `false` when it cannot be read.
  const fill = () => {
    let count;
    try {
      count = readSync(fd, chunk, 0, chunk.length, null);
    } catch (err) {
      const code = (err as ErrorWithCode).code;
      if (code === "EAGAIN") {
        Atomics.wait(waiter, 0, 0, 5);
        return true;
      }
      if (code !== "EOF") {
        return false;
      }
      count = 0;
    }
    if (count === 0) {
      ended = true;
    } else {
      pending = chunk.subarray(0, count);
      chunk = Buffer.allocUnsafe(65536);
    }
    return true;
  };
  const line = (() => {
    for (;;) {
      const nl = pending.indexOf(10);
      if (nl >= 0) {
        retain(pending.subarray(0, nl));
        pending = pending.subarray(nl + 1);
        let line = take();
        if (line.length && line[line.length - 1] === 13) {
          line = line.subarray(0, line.length - 1);
        }
        return line.toString("utf8");
      }
      retain(pending);
      pending = Buffer.alloc(0);
      if (ended) {
        return length ? take().toString("utf8") : null;
      }
      if (!fill()) {
        return false;
      }
    }
  }) as LineReader;
  // The rest of the input without newline translation
  // (std-console.input.read-all-host-preserves-bytes).
  line.all = () => {
    retain(pending);
    pending = Buffer.alloc(0);
    while (!ended) {
      if (!fill()) {
        return false;
      }
      retain(pending);
      pending = Buffer.alloc(0);
    }
    return take().toString("utf8");
  };
  line.bytes = (max) => {
    max >>>= 0;
    if (max === 0) {
      return new Uint8Array();
    }
    // Only a failed text read can retain earlier fragments. Recover them
    // once before continuing with the same buffered descriptor.
    if (length) {
      retain(pending);
      pending = take();
    }
    while (!pending.length && !ended) {
      if (!fill()) {
        return false;
      }
    }
    const bytes = pending.subarray(0, Math.min(max, pending.length));
    pending = pending.subarray(bytes.length);
    return bytes;
  };
  return line;
}

// `hd:ConsoleInput`: the next input line, `.None` at the end, `.Err(.Other(code))`
// when the host descriptor cannot be read. `env.readLine`, when a host has one, reads it instead
// of the input worker (the REPL keeps its framed control reader).
function consoleInputProvider({ env, flush, inputClient }: DynamicRecord): DynamicRecord {
  if (env.readLine === undefined && env.stdin !== null) {
    const read = (kind: string, max = 0): Dynamic => {
      flush();
      const { ok: operation } = inputClient.call({
        op: "wait_start",
        kind,
        max,
        fd: env.stdin ?? 0,
      });
      let result: Dynamic = null;
      const poll = () => {
        const outcome = inputClient.call({ op: "wait_poll", operation });
        if (outcome.pending) {
          return false;
        }
        result = outcome;
        return true;
      };
      const finish = () => {
        inputClient.call({ op: "wait_finish", operation });
        return undefined;
      };
      const output = () => {
        if (result.error !== undefined) {
          return kind === "bytes"
            ? Err({ tag: "Other", message: `standard input read error ${result.error}` })
            : Err({ tag: "Other", code: result.error });
        }
        return result.closed ? Err({ tag: "Closed" }) : Ok(result.ok);
      };
      if (poll()) {
        finish();
        return output();
      }
      return pendingOperation(
        poll,
        output,
        () => inputClient.cancel({ op: "wait_abort", operation }),
        finish,
        (ready) => inputClient.subscribe(operation, ready),
      );
    };
    // Standard input's one handle; its reads share the worker's buffered
    // input with `read_line` and `read_all` (std-console.stdin.exact).
    return {
      read_line: () => read("line"),
      read_all: () => read("all"),
      stdin: () => 1,
      "StdinHandle.read": (_handle: number, max: number) => read("bytes", max >>> 0),
    };
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
        if (line === false) {
          return false;
        }
        text += `${line}\n`;
      }
      return text;
    });
  return {
    stdin: () => 1,
    "StdinHandle.read": (_handle: number, max: number) => {
      flush();
      const bytes = readLine.bytes(max >>> 0);
      return bytes === false ? Err({ tag: "Closed" }) : Ok(new Uint8Array(bytes));
    },
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
const quotaText = (path: string): string | null => {
  try {
    return readFileSync(path, "utf8").trim();
  } catch {
    return null;
  }
};

// std-host.sys.cpu-count: every ancestor constrains the offered processors.
// Keep the walk rooted at its mount, including when tested on macOS.
// Like Rust's available_parallelism, round quotas down, with at least one.
export function cgroupCpuCount(root: string, group: string, offered: number): number {
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
    if (
      quota !== null &&
      period !== null &&
      /^\d+$/.test(quota) &&
      /^\d+$/.test(period) &&
      BigInt(period) > 0n
    ) {
      count = Math.min(count, Number(BigInt(quota) / BigInt(period)));
    }
    if (dir === root) {
      break;
    }
    dir = dirname(dir);
  }
  return Math.max(1, count);
}

function v1CpuCount(group: string, offered: number): number {
  for (const root of ["/sys/fs/cgroup/cpu", "/sys/fs/cgroup/cpu,cpuacct"]) {
    if (existsSync(join(root, group))) {
      return cgroupCpuCount(root, group, offered);
    }
  }
  for (const line of (quotaText("/proc/self/mountinfo") ?? "").split("\n")) {
    const [mounted, filesystem] = line.split(" - ");
    const [kind, , options] = filesystem?.split(" ") ?? [];
    if (kind !== "cgroup" || !options?.split(",").includes("cpu")) {
      continue;
    }
    const [, , , subtree, root] = mounted.split(" ");
    if (group === subtree || group.startsWith(subtree === "/" ? "/" : subtree + "/")) {
      return cgroupCpuCount(root, group.slice(subtree.length), offered);
    }
  }
  return offered;
}

function cpuCount(): number {
  const offered = availableParallelism();
  if (platform() !== "linux") {
    return offered;
  }
  const membership = quotaText("/proc/self/cgroup");
  if (membership === null) {
    return offered;
  }
  const groups = membership.split("\n").map((line) => /^(\d+):([^:]*):(\/.*)$/.exec(line));
  const cpu = groups.find((group) => group?.[2].split(",").includes("cpu"));
  const unified = groups.find((group) => group?.[1] === "0" && group[2] === "");
  if (!cpu) {
    return unified ? cgroupCpuCount("/sys/fs/cgroup", unified[3], offered) : offered;
  }

  return v1CpuCount(cpu[3], offered);
}

// The host system's four reads (std-host.sys.*). The grant covers each by
// its method's name (cli.cap.scope.sys); a refused or unanswerable
// read is `.Err` with that name (std-host.sys.not-granted, .unsupported).
function sysProvider({ grant }: DynamicRecord): DynamicRecord {
  // `Result[T, HostError]` of the read `name`: `.Ok` of what `value`
  // returns, `.Err(.NotGranted(name))` outside the grant, or
  // `.Err(.Unsupported(name))` when the host cannot answer it.
  const sysRead = (name: string, value: () => Dynamic): Dynamic => {
    if (!grant.coversName("Sys", name)) {
      return Err({ tag: "NotGranted", subject: name });
    }
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
function netProvider({ grant, netClient }: DynamicRecord): DynamicRecord {
  const netError = (error: Dynamic): Dynamic => {
    const [tag, ...values] = error as [string, ...unknown[]];
    return variant(NetError, tag, ...values);
  };
  // `Result[T, NetError]` of a `Net` method from a client outcome.
  const netResult = (result: Dynamic): Dynamic =>
    result.error ? Err(netError(result.error)) : Ok(result.ok);
  // `Result[T, NetError]` of a handle's method: an operation on a closed
  // handle is `Closed` (std-net.handles.closed).
  const handleResult = (result: Dynamic): Dynamic => {
    if (result.unknown) {
      throw new HostContract("an operation on a handle the host never made");
    }
    if (result.disposed) {
      return Err(variant(NetError, "Closed"));
    }
    return result.error ? Err(netError(result.error)) : Ok(result.ok);
  };
  // A call names its address `host:port` (std-net.error.address): one
  // whose host is no host is `InvalidAddress`, and one the grant does not
  // cover `NotGranted` (cli.cap.scope.net), before any socket opens.
  const netAt = (op: string, host: string, port: number, more: DynamicRecord = {}): Dynamic => {
    const address = `${host}:${port}`;
    if (!validHost(host)) {
      return { error: ["InvalidAddress", address] };
    }
    if (!grant.coversHost("Net", host, port)) {
      return { error: ["NotGranted", address] };
    }
    return netClient.call({ op, host, port, address, ...more });
  };
  const waitFor = (started: Dynamic, resultOf: (result: Dynamic) => Dynamic): Dynamic => {
    if (started.ok === undefined) {
      return resultOf(started);
    }
    const operation = started.ok;
    let result: Dynamic = null;
    const poll = () => {
      const outcome = netClient.call({ op: "wait_poll", operation });
      if (outcome.pending) {
        return false;
      }
      result = outcome;
      return true;
    };
    const finish = () => {
      const committed = netClient.call({ op: "wait_finish", operation });
      if (committed.disposed) {
        return resultOf(committed);
      }
    };
    if (poll()) {
      return finish() ?? resultOf(result);
    }
    return pendingOperation(
      poll,
      () => resultOf(result),
      () => netClient.cancel({ op: "wait_abort", operation }),
      finish,
      (ready) => netClient.subscribe(operation, ready),
    );
  };
  const opened =
    (kind: string) =>
    (host: string, port: number): Dynamic =>
      waitFor(netAt("wait_start", host, port, { kind }), netResult);
  const handleOp =
    (op: string) =>
    (host: Dynamic, more: DynamicRecord): Dynamic =>
      handleResult(netClient.call({ op, handle: host, ...more }));
  const waitOn =
    (kind: string) =>
    (host: Dynamic, more: DynamicRecord = {}): Dynamic =>
      waitFor(netClient.call({ op: "wait_start", handle: host, kind, ...more }), handleResult);
  // std-net.timeout.set: the `Duration?` crosses to the client thread as
  // milliseconds, or `null` for no bound.
  const setBound = (host: Dynamic, timeout: Dynamic): Dynamic =>
    handleOp("set_timeout")(host, { timeout: timeout === null ? null : Number(timeout.millis) });
  // std-net.option.nodelay and std-net.option.keepalive: the flag crosses
  // as a number, and the idle `Duration?` as milliseconds, or `null` for off.
  const setNoDelay = (host: Dynamic, enabled: Dynamic): Dynamic =>
    handleOp("set_nodelay")(host, { enabled: enabled !== 0 });
  const setKeepAlive = (host: Dynamic, idle: Dynamic): Dynamic =>
    handleOp("set_keepalive")(host, { idle: idle === null ? null : Number(idle.millis) });
  // A second `close` is `Closed` (std-net.handles.closed).
  const close = (host: Dynamic): Dynamic =>
    handleResult(netClient.call({ op: "close", handle: host }));
  // `dispose!` closes an open handle and reports nothing (std-net.dispose).
  const dispose = (host: Dynamic): void => {
    const disposed = netClient.call({ op: "dispose", handle: host });
    if (disposed.unknown) {
      throw new HostContract("an operation on a handle the host never made");
    }
  };
  return {
    // cli.cap.scope.net.lookup: an entry covers a lookup of its host.
    lookup: (host: string): Dynamic => {
      if (!validHost(host)) {
        return netResult({ error: ["InvalidAddress", host] });
      }
      if (!grant.coversHostName("Net", host)) {
        return netResult({ error: ["NotGranted", host] });
      }
      return waitFor(netClient.call({ op: "wait_start", kind: "lookup", host }), netResult);
    },
    connect: (host: string, port: number): Dynamic => {
      const started = netAt("connect_start", host, port);
      if (started.error) {
        return netResult(started);
      }
      const operation = started.ok;
      let result: Dynamic = null;
      const poll = () => {
        const outcome = netClient.call({ op: "connect_poll", operation });
        if (outcome.pending) {
          return false;
        }
        result = outcome;
        return true;
      };
      // An already ready connect wins its first-poll tie. Otherwise the
      // guest can poll its clock while the worker owns the pending socket.
      if (poll()) {
        return netResult(result);
      }
      return pendingOperation(
        poll,
        () => netResult(result),
        () => {
          if (result === null) {
            netClient.cancel({ op: "connect_abort", operation });
          }
          // complete() can run before the guest consumes .finish: the
          // cancelled outcome still owns an undelivered stream in that gap.
          else if (result.ok !== undefined) {
            netClient.cancel({ op: "dispose", handle: result.ok });
          }
        },
        null,
        (ready) => netClient.subscribe(operation, ready, "connect"),
      );
    },
    listen: opened("listen"),
    bind_udp: opened("bind_udp"),
    "TcpStream.read": (host: Dynamic, max: number) => waitOn("read")(host, { max }),
    "TcpStream.write": (host: Dynamic, bytes: Uint8Array) => waitOn("write")(host, { bytes }),
    "TcpStream.set_timeout": setBound,
    "TcpStream.set_nodelay": setNoDelay,
    "TcpStream.set_keepalive": setKeepAlive,
    "TcpStream.close": close,
    "TcpStream.dispose": dispose,
    "TcpListener.accept": waitOn("accept"),
    "TcpListener.set_timeout": setBound,
    "TcpListener.local_address": (host: Dynamic) => handleOp("local_address")(host, {}),
    "TcpListener.close": close,
    "TcpListener.dispose": dispose,
    // std-net.send-to.grant: each datagram's own address.
    "UdpSocket.send_to": (handle: Dynamic, host: string, port: number, bytes: Uint8Array) => {
      // Reject closed and unknown handles before destination validation.
      // This checks metadata only.
      const checked = netClient.call({ op: "check", handle });
      if (checked.unknown || checked.disposed) {
        return handleResult(checked);
      }
      // Each live send still checks its destination grant.
      return waitFor(
        netAt("wait_start", host, port, { kind: "send_to", handle, bytes }),
        handleResult,
      );
    },
    "UdpSocket.receive": (host: Dynamic, max: number) => waitOn("receive")(host, { max }),
    "UdpSocket.set_timeout": setBound,
    "UdpSocket.local_address": (host: Dynamic) => handleOp("local_address")(host, {}),
    "UdpSocket.close": close,
    "UdpSocket.dispose": dispose,
  };
}

// `hd:Env`: granted variables read, the rest read as unset with one
// notice per name.
function envProvider({ grant, sink, st, flush }: DynamicRecord): DynamicRecord {
  return {
    // A variable outside the grant reads as unset; when it is set, a
    // notice names the flag, once per name (cli.cap.env.notice).
    get: (name: string) => {
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
    names: () => Object.keys(process.env).filter((item) => grant.coversName("Env", item)),
  };
}

// Only methods that open a path can wait for a special-file peer. A
// missing or uninspectable path stays inline and reports its ordinary error.
// `open` of a special file leaves its descriptor with the helper child
// that opened it (fs.ts), which makes the file's reads.
const fileOpening = new Set([
  "open",
  "read_bytes",
  "read_text",
  "write_bytes",
  "write_text",
  "append_text",
  "copy",
]);
// What a file-opening call finds at `path`: "special" for a FIFO, device
// or socket, whose call is offloaded; a regular file's size, so a read
// makes no second stat (#1901); or null (missing, a directory, a stat
// error), which the operation itself reports.
const fileKind = (path: string): "special" | number | null => {
  if (process.platform === "win32") {
    // Inspecting a Windows pipe can itself open/wait on it. Native uses
    // these same device namespace spellings without touching the device.
    const normalized = path.replaceAll("/", "\\").toLowerCase();
    if (
      normalized.startsWith("\\\\.\\") ||
      normalized.startsWith("\\\\?\\pipe\\") ||
      normalized.startsWith("\\\\?\\globalroot\\device\\") ||
      /^\\\\(?:\?\\unc\\)?[^\\]+\\pipe\\/.test(normalized)
    ) {
      return "special";
    }
  }
  let st;
  try {
    st = statSync(path);
  } catch {
    return null;
  }
  if (st.isFile()) {
    return st.size;
  }
  return st.isDirectory() ? null : "special";
};

// A special-file call the file worker started (fs.ts): pending until the
// worker has its result. `.finish` takes the result from the worker, since
// a dispose after the poll makes a read's `Closed`
// (std-io.read.pending-closed), and `deliver` makes it the program's.
const waitFile = (
  fsClient: Dynamic,
  { ok: operation }: { ok: number },
  deliver: (result: Dynamic) => Dynamic = (result) => result,
): Dynamic => {
  let result: Dynamic = null;
  const poll = () => {
    const outcome = fsClient.call({ op: "wait_poll", operation });
    if (outcome.pending) {
      return false;
    }
    result = outcome.result;
    return true;
  };
  return pendingOperation(
    poll,
    () => result,
    () => fsClient.cancel({ op: "wait_abort", operation }),
    () => deliver(fsClient.call({ op: "wait_finish", operation }).result),
    (ready) => fsClient.subscribe(operation, ready),
  );
};

// `hd:FsRead` and `hd:FsWrite`: the file providers (std.fs).
function fsProviders({ grant, flush, fsClient }: DynamicRecord): DynamicRecord {
  // An open file's handle is a number never issued twice, not its
  // descriptor, so an alias of a disposed file is `Closed` even once the
  // OS reuses the descriptor (std-fs.open.dispose). A regular file's
  // descriptor is this thread's; a special file is the file worker's
  // number for it.
  const files = new Map<number, { fd: number } | { special: number }>();
  let nextFile = 1;
  const issue = (file: { fd: number } | { special: number }): Dynamic => {
    files.set(nextFile, file);
    return Ok(nextFile++);
  };
  // A special file's handle is issued when the program takes its open.
  const openedSpecial = (opened: Dynamic): Dynamic =>
    opened.ok ? issue({ special: opened.value }) : opened;
  // The methods `ops` of `module`, each `run` once the grant covers every
  // path it names, its `Path` arguments (its resource, gen/abi.ts). A
  // special-file call is offloaded after these checks. Output is flushed
  // first (std-console.host.visible-text-sync). A method that
  // creates its directories keeps the OS's own rule for a `..` after a
  // missing name, as the Rust host's `create` does.
  const fsOps = (module: string, ops: DynamicRecord): DynamicRecord =>
    Object.fromEntries(
      Object.entries(ops).map(([name, op]) => {
        const message = (METHODS as DynamicRecord)[module][name];
        const { create = false } = typeof op === "function" ? {} : (op as DynamicRecord);
        const provider = (...args: Dynamic[]) => {
          const ps = args.filter(
            (_, i) => message.resource === "path" && message.params[i] === Path,
          );
          const refused = ps.find(
            (item, i) =>
              !grant.coversPath(
                name === "copy" && i === 0 ? "FsRead" : module.slice("hd:".length),
                item,
              ),
          );
          if (refused !== undefined) {
            return fsErr(refusedPath(refused), "NotGranted");
          }
          for (const path of create ? [] : ps) {
            const missing = beforeDotDot(path);
            if (missing !== undefined) {
              return fsErr(path, missing);
            }
          }
          flush();
          // cost: one stat per path a file-opening call names.
          const kinds = fileOpening.has(name) ? ps.map(fileKind) : [];
          if (kinds.includes("special")) {
            const started = fsClient.call({ op: "wait_start", module, name, args });
            return waitFile(fsClient, started, name === "open" ? openedSpecial : undefined);
          }
          if (module === "hd:FsRead" && typeof kinds[0] === "number") {
            return fileOperation(module, name, [...args, kinds[0]]);
          }
          return fileOperation(module, name, args);
        };
        return [name, provider];
      }),
    );
  const providers = Object.fromEntries(
    Object.entries(fileOperations).map(([module, ops]) => [module, fsOps(module, ops)]),
  );
  const reads = providers["hd:FsRead"];
  return {
    ...providers,
    "hd:FsRead": {
      ...reads,
      // A regular file opens inline; a special file's open is pending in
      // the file worker, and `openedSpecial` issues its handle.
      open: (path: string) => {
        const opened = reads.open(path);
        return opened.ok === true ? issue({ fd: opened.value }) : opened;
      },
      "File.read": (file: number, max: number) => {
        flush();
        const open = files.get(file);
        if (open === undefined) {
          return Err({ tag: "Closed" });
        }
        if ("special" in open) {
          // std-fs.open.special-file: the read waits off this thread.
          const started = fsClient.call({ op: "read_start", file: open.special, max });
          return waitFile(fsClient, started);
        }
        try {
          return Ok(readOpen(open.fd, max >>> 0));
        } catch (error) {
          return Err({ tag: "Other", message: `read: ${(error as Error).message}` });
        }
      },
      // `dispose!` closes an open file; disposing a closed one does
      // nothing. A special file's pending reads answer `Closed`.
      "File.dispose": (file: number) => {
        const open = files.get(file);
        if (open === undefined) {
          return;
        }
        files.delete(file);
        if ("special" in open) {
          // A release, so it runs past the run's deadline too.
          fsClient.cancel({ op: "file_dispose", file: open.special });
        } else {
          closeSync(open.fd);
        }
      },
    },
  };
}

// The checks the Rust host makes before it starts a child
// (module.process.command-env, module.process.command-dir): no NUL byte in
// the program, an argument, an environment pair or the directory, an
// environment name that is neither empty nor holds `=`, and a directory
// that exists.
const refusal = (
  name: string,
  argv: string[],
  pairs: [string, string][],
  dir: string | null,
): Dynamic => {
  const other = (message: string) => Err({ tag: "Other", message });
  if (name.includes("\0")) {
    return other("program name contains a NUL byte");
  }
  const arg = argv.findIndex((item) => item.includes("\0"));
  if (arg >= 0) {
    return other(`process argument ${arg} contains a NUL byte`);
  }
  const bad = pairs.findIndex(([key]) => key === "" || /[=\0]/.test(key));
  if (bad >= 0) {
    return other(`process environment name ${bad} is empty or holds = or a NUL byte`);
  }
  const nul = pairs.findIndex(([, value]) => value.includes("\0"));
  if (nul >= 0) {
    return other(`process environment value ${nul} contains a NUL byte`);
  }
  if (dir !== null && dir.includes("\0")) {
    return other("working directory contains a NUL byte");
  }
  return null;
};
const isDirectory = (path: string): boolean => {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
};
const noDirectory = (dir: string | null): Dynamic =>
  Err({ tag: "Other", message: `working directory not found: ${dir}` });

function childEnvironment(pairs: [string, string][]): NodeJS.ProcessEnv {
  const childEnv = { ...process.env };
  for (const [key, value] of pairs) {
    const own =
      platform() === "win32"
        ? Object.keys(childEnv).find((candidate) => candidate.toUpperCase() === key.toUpperCase())
        : undefined;
    childEnv[own ?? key] = value;
  }
  return childEnv;
}

function processCommand({ env, name, argv, pairs, dir }: DynamicRecord): Dynamic {
  const bad = refusal(name, argv, pairs, dir);
  if (bad) {
    return bad;
  }
  if (!env.programs) {
    if (name === "") {
      return Err({ tag: "NotFound", program: name });
    }
    return {
      program: name,
      args: argv,
      cwd: dir === null ? process.cwd() : resolve(process.cwd(), dir),
    };
  }
  const executable = env.programs[name];
  if (!executable) {
    return Err({ tag: "NotFound", program: name });
  }
  return {
    program: process.execPath,
    args: [executable.host, executable.wasm, executable.config, ...argv],
    cwd: dir === null ? executable.cwd : resolve(executable.cwd, dir),
  };
}

// `hd:Process`: running host programs (`Process.run!`, cli.host.default-
// profile, cli.test.process): the test runner's provider starts the
// package's executables and tasks by name, the default profile's a host
// program.
function processProvider({ grant, env, left, processClient }: DynamicRecord): DynamicRecord {
  const run = (
    inherit: boolean,
    { program: name, args: argv, env: pairs, current_dir: dir, stdin: text }: DynamicRecord,
  ): Dynamic => {
    if (!grant.coversName("Process", name)) {
      return Err({ tag: "NotGranted" });
    }
    const ms = left();
    if (ms <= 0) {
      throw new TimeLimit();
    }
    const command = processCommand({ env, name, argv, pairs, dir });
    if (command.ok === false) {
      return command;
    }
    const { program, args, cwd } = command;
    if (!isDirectory(cwd)) {
      return noDirectory(dir);
    }
    const { ok: operation } = processClient.call({
      op: "wait_start",
      launcher: env.processLauncher ?? null,
      program,
      args,
      cwd,
      env: childEnvironment(pairs),
      input: text,
      inherit,
      timeout: ms,
    });
    let result: Dynamic = null;
    const poll = () => {
      const outcome = processClient.call({ op: "wait_poll", operation });
      if (outcome.pending) {
        return false;
      }
      result = outcome;
      return true;
    };
    const finish = () => processClient.call({ op: "wait_finish", operation });
    const output = () => {
      if (result.timeout) {
        throw new TimeLimit();
      }
      if (result.error) {
        const { code, message } = result.error;
        if (code === "ENOENT") {
          return Err({ tag: "NotFound", program: name });
        }
        if (code === "EACCES" || code === "EPERM") {
          return Err({ tag: "PermissionDenied", program: name });
        }
        return Err({ tag: "Other", message });
      }
      return Ok(inherit ? result.ok.status : { ...result.ok, program: name });
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
      (ready) => processClient.subscribe(operation, ready),
    );
  };
  return {
    run: (command: Dynamic) => run(false, command),
    status: (command: Dynamic) => run(true, command),
  };
}

// The files of a test run on disk (cases.ts): the case's temporary
// directories, its snapshot files and its regression streams.
export const nodeFiles = {
  join,
  read: (path: string) => (existsSync(path) ? readFileSync(path, "utf8") : null),
  write: (path: string, text: string) => {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, text);
  },
  remove: (dir: string) => rmSync(dir, { recursive: true, force: true }),
};

// `hd:TestRunner`: the temp directory of a case
// (std-testing.runner.temp-dir); the core has the other hooks.
function testRunnerProvider({ env, st }: DynamicRecord): DynamicRecord {
  const tempDir = () => {
    if (!env.tempDir) {
      // A run without a configured directory makes its own.
      st.ownTemp ??= mkdtempSync(join(tmpdir(), "hd-case-"));
      return st.ownTemp;
    }
    if (!st.tempMade) {
      mkdirSync(env.tempDir, { recursive: true });
    }
    st.tempMade = true;
    return env.tempDir;
  };
  return { provider: { temp_dir: tempDir }, tempDir };
}

// A relative path is from `cwd` (the process's by default), with `..` and
// links resolved (cli.cap.scope.path.resolved).
const resolveTarget = (path: string, cwd = process.cwd()): string => resolvePath(path, cwd);

// The grant with Node's path resolution.
export const createGrant = (limits: DynamicRecord = {}) =>
  createCoreGrant(limits, resolveTarget, sep);

// `count` bytes of the operating system's random source. Reading the
// device needs no `crypto`, whose module costs about 3 ms to load.
function urandom(count: number): Uint8Array {
  const out = new Uint8Array(count);
  const fd = openSync("/dev/urandom", "r");
  try {
    for (let at = 0; at < count;) {
      at += readSync(fd, out, at, count - at, null);
    }
  } finally {
    closeSync(fd);
  }
  return out;
}

// Namespace availability is independent of whether its per-host state is used.
const NODE_PROVIDER_MODULES = [
  "hd:ConsoleInput",
  "hd:Sys",
  "hd:Http",
  "hd:Net",
  "hd:Env",
  "hd:FsRead",
  "hd:FsWrite",
  "hd:Process",
  "hd:TestRunner",
  "hd:Workers",
];

// The Node layer for `createCoreHost`: the providers that touch the
// machine, and what releases them.
export const nodeLayer = {
  startThread,
  randomBytes: process.platform === "win32" ? undefined : urandom,
  resolveTarget,
  sep,
  files: nodeFiles,
  providers: ({ env, sink, st, grant, flush, left }: DynamicRecord) => {
    let reactor: Int32Array<SharedArrayBuffer> | undefined;
    const completion = () => (reactor ??= new Int32Array(new SharedArrayBuffer(4)));
    st.completionSyncToken = () => Atomics.load(completion(), 0);
    st.waitCompletionSync = (ms: number, observed: number) =>
      Atomics.wait(completion(), 0, observed, ms);
    const initialize = () => {
      // `Net` and its socket handles (spec/std/net.md) on a client thread of
      // this instance's own, which holds its sockets; `close` stops it.
      const reactor = completion();
      const net = syncClient("./net.ts", null, reactor);
      const http = syncClient("./http.ts", null, reactor);
      const child = syncClient("./process.ts", "close", reactor);
      const files = syncClient("./fs.ts", "close", reactor);
      const input = syncClient("./input.ts", "close", reactor);
      const workers = workersProvider(env.module, reactor);
      // A bare Promise does not keep Node alive. Reference one idle message
      // port while the guest waits, without a polling timer or worker spin.
      let keeper: { port1: MessagePort; port2: MessagePort } | null = null;
      let idleWaits = 0;
      const releaseWait = () => {
        idleWaits--;
        if (idleWaits === 0) {
          keeper?.port1.unref();
        }
      };
      const keepAlive = () => {
        if (keeper === null) {
          keeper = new MessageChannel();
          keeper.port1.on("message", () => undefined);
          keeper.port2.unref();
        }
        idleWaits++;
        keeper.port1.ref();
        return releaseWait;
      };
      // The program's output reaches its stream before it waits on the host
      // (std-console.host.visible-text-sync): a run stopped in a wait loses none. Each
      // client call and child process asks `waiting()` first.
      const waiting = () => {
        flush();
        return left();
      };
      const client = (worker: SyncClient): Dynamic => ({
        subscribe: (operation: number, ready: () => void, kind = "wait") =>
          worker.subscribe(`${kind}:${operation}`, ready),
        call: (request: Dynamic) => worker.call(request, waiting),
        // Cancellation owns no I/O wait. It still releases resources after
        // the program deadline expired, and never starts a replacement worker.
        cancel: (request: Dynamic) =>
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
          "hd:Signals": {
            wait: (kind: { tag: import("./signals.ts").SignalKind }) => {
              if (!grant.allows("Signals")) {
                throw new HostContract("capability not granted: Signals");
              }
              let delivered = false;
              return pendingOperation(
                () => delivered,
                () => undefined,
                null,
                null,
                (notify) =>
                  signalSource()!.wait(kind.tag, () => {
                    delivered = true;
                    notify();
                  }),
              );
            },
          },
          "hd:Http": httpProvider((request) => httpSend(httpClient, { ...request, grants })),
          "hd:Net": netProvider({ grant, netClient }),
          "hd:Env": envProvider({ grant, sink, st, flush }),
          ...fsProviders({ grant, flush, fsClient: client(files) }),
          "hd:Process": processProvider({
            grant,
            env,
            left: waiting,
            processClient: client(child),
          }),
          "hd:TestRunner": testRunner.provider,
          "hd:Workers": workers.provider,
        },
        tempDir: testRunner.tempDir,
        keepAlive,
        drainReady: () => {
          net.drainReady();
          http.drainReady();
          child.drainReady();
          files.drainReady();
          input.drainReady();
          workers.drain();
        },
        // The sockets still open, by kind (cli.test.leak).
        leaks: () => {
          if (!net.started()) {
            return [];
          }
          const counts = net.call({ op: "open" }).ok ?? {};
          return (
            [
              ["TcpStream", counts.stream ?? 0],
              ["TcpListener", counts.listener ?? 0],
              ["UdpSocket", counts.udp ?? 0],
            ] as [string, number][]
          ).filter(([, count]) => count > 0);
        },
        // Removes a directory the host made for itself, and closes its
        // sockets.
        close: () => {
          keeper?.port1.close();
          keeper?.port2.close();
          keeper = null;
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
    };
    let platform: Dynamic;
    const get = () => (platform ??= initialize());
    const traits: DynamicRecord = {
      "hd:Console": {
        // Worker stdio wrappers have no isTTY; query the inherited OS fd.
        is_terminal: (stream: { tag: string }) => {
          if (!grant.allows("Console")) {
            return false;
          }
          switch (stream.tag) {
            case "Stdout":
              return isatty(1);
            case "Stderr":
              return isatty(2);
            case "Stdin":
              return isatty(0);
            default:
              throw new HostContract("unknown console stream");
          }
        },
      },
    };
    for (const module of [...NODE_PROVIDER_MODULES, ...(signalSource() ? ["hd:Signals"] : [])]) {
      Object.defineProperty(traits, module, {
        enumerable: true,
        get: () => get().traits[module],
      });
    }
    return {
      traits,
      tempDir: () => get().tempDir(),
      keepAlive: () => get().keepAlive(),
      drainReady: () => platform?.drainReady(),
      leaks: () => platform?.leaks() ?? [],
      close: () => platform?.close(),
    };
  },
};

// One host per instance on Node; `sink` and `env` are `createCoreHost`'s.
// `env.stdin` is a file descriptor, or `null` for a closed standard input.
export const createHost = (sink: Sink, env: HostEnv = {}): Host => {
  const report = process.env.HD_COUNT_AUDIT_REPORT;
  // Property runs update this same environment's runner between cases.
  // Preserve its identity when adding the optional reporting hook.
  if (report && !env.countAudit) {
    env.countAudit = (rows) => {
      process
        .getBuiltinModule("node:fs")
        .appendFileSync(report, `${JSON.stringify({ program: env.program ?? "", sites: rows })}\n`);
    };
  }
  return createCoreHost(sink, env, nodeLayer);
};

// Loaded only by the REPL child; ordinary programs skip the session graph.
export async function driveNodeSession(): Promise<void> {
  const { driveNodeSession: drive } = await import("./node-session.ts");
  return drive();
}
