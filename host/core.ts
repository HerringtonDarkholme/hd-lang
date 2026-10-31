/* oxlint-disable max-lines -- descriptive ABI identifiers push this legacy host past the limit. */
// The platform-neutral core of the JS host (V8 in Node, and browsers): the
// import object of the ABI in `hd_host_abi` (crates/hd_run_wasmtime/DESIGN.md §17.2,
// §17.10), built from the method descriptors and codecs generated from the
// ABI model (gen/abi.ts, over wire.ts's encoding of §17.4); the providers
// that need nothing from the platform (`Console`, `Clock`, `Random`,
// `Args`, `rt`, `prim`, and the test and property runners' hooks); and the
// entry driver of crates/hd_wasm_linear/DESIGN.md §14.4. It uses web-standard APIs
// only. A platform layer (node.ts, web.ts) adds the providers that touch
// the machine by passing `layer` to `createCoreHost`.
//
// - A provider is a host trait's methods as JS functions by name: each
//   takes its arguments and returns its result as plain values (wire.ts
//   says which), and `traitImports` makes the imports of §17.2 around it.
//   A method that never waits is one import; one that may wait is
//   `.start`, which returns `[status, value]`, then `.finish(h)`. Status 0:
//   done at once, the result is in the buffer and `value` is its length.
//   Status 1: pending, `value` is its handle. `hd:Console` `write_sync`
//   and `write_error_sync` never wait: each is one call, which may block
//   on output backpressure. `hd:Clock`
//   `sleep` completes when its duration has passed. External I/O can stay
//   pending through generic readiness and cancellation hooks; other methods
//   run synchronously and finish at once.
// - `hd:rt` `block()` waits synchronously for at least one completion,
//   writes the completed handles to the exchange buffer and returns their
//   count; `stderr(len)` writes the buffer's first `len` bytes to standard
//   error; `stdout(len)` holds them with the console's output (the REPL's
//   value line); `panic()` starts a panic stub's report: it writes
//   `panic: `, and the standard error written until the trap is the
//   report, whose category and message a run's result carries;
//   `fact(len)` marks that a facts body starts, its label in the buffer,
//   or ends (-1), and a panic report while one runs, the stub's or the
//   host's own, is `fact-evaluation-failed` naming it (factHead).
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
//   (journal.ts), as `hd_run_wasmtime`'s session does.
import { shortest, fixed, lower, upper, parse } from "./float.ts";
import { bindPackageRuntime } from "./core-package.ts";
import { WireHttpError as HttpError, METHODS } from "./gen/abi.ts";
import { Dec, decodeUtf8, Enc, Err, HostContract, Ok, str, variant } from "./wire.ts";
export {
  CANCEL_STATUS,
  entryCancel,
  factHead,
  PROGRAM_TIME_LIMIT,
  reportIn,
} from "./run-support.ts";
import { CANCEL_STATUS, factHead, PROGRAM_TIME_LIMIT, reportIn } from "./run-support.ts";
import type { End } from "./journal.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;
type ProviderFunction = (...args: Dynamic[]) => Dynamic;

interface HostInstance {
  readonly exports: DynamicRecord;
}

export interface Sink {
  out(text: string): void;
  err(text: string): void;
  fact?(label: string | null): void;
  write?(fd: number, text: string): Dynamic;
}

export interface ProviderHttpPolicy {
  readonly coversHost: (host: string, port: number) => boolean;
  readonly isAsciiUrl: (text: string) => boolean;
  readonly writtenHost: (text: string) => string;
}

export interface ProviderConsolePolicy {
  readonly allows: () => boolean;
}

export interface ProviderInterfaceIdentity {
  readonly package: string;
  readonly version: string;
  readonly trait_path: string;
}

export interface ProviderContext {
  readonly interfaceIdentity: ProviderInterfaceIdentity;
  readonly http: ProviderHttpPolicy;
  readonly console: ProviderConsolePolicy;
  signal: AbortSignal;
  instanceSignal: AbortSignal;
  pending<ResultValue>(
    poll: () => boolean,
    done: () => ResultValue,
    abort?: (() => void) | null,
    finish?: (() => ResultValue | undefined) | null,
  ): unknown;
}

export interface CoreEnv extends DynamicRecord {
  args?: string[];
  program?: string;
  grants?: Record<string, boolean | string[]>;
  module?: WebAssembly.Module;
  packageBindings?: ((runtime: unknown) => WebAssembly.Imports) | null;
  seed?: number | null;
  runner?: Dynamic;
  providers?: ((io: DynamicRecord) => DynamicRecord) | null;
}

export interface Grant {
  allows(key: string): boolean;
  limited(key: string): boolean;
  coversPath(key: string, path: string, cwd?: string): boolean;
  coversHost(key: string, host: string, port?: number | null): boolean;
  coversHostName(key: string, host: string): boolean;
  coversName(key: string, name: string): boolean;
}

export interface RunResult {
  status: number;
  trapped: boolean;
  ended?: boolean;
  frames?: [number, number][];
  panic?: { category: string; message: string } | null;
  cancelled?: boolean;
}

export interface StepDriver {
  step(): { status: number; wakes: number };
}

export interface Host {
  imports: DynamicRecord;
  run(
    instance: HostInstance,
    init: string,
    poll: string,
    options?: { driver?: Dynamic; cancel?: Dynamic },
  ): Promise<RunResult>;
  createDriver(instance: HostInstance, init?: string, poll?: string): StepDriver;
  prime(instance: HostInstance): void;
  close(): void;
  leaks(): [string, number][];
  args: string[];
  program: string;
  grant: Dynamic;
  seed: number | null;
  tempDir?: () => string;
  core: DynamicRecord;
}

export interface InputOutcome {
  status: number | null;
  end: End;
  joined: boolean;
  error?: string | null;
}

export interface Ran {
  kept: number[];
  skipped: number[];
  dropped: number[];
  reason: string | null;
  live: InputOutcome | null;
}

export interface Session {
  run(
    wasm: Uint8Array,
    sources: Iterable<readonly [number, string]>,
    live?: number | null,
    journal?: Uint8Array | null,
  ): Promise<Ran>;
  clear(): void;
  journal(): Uint8Array | null;
  setJournalCap(cap: number): void;
  takeJournalNotice(): boolean;
  stop(): void;
  hostCalls(): number;
  checkpointStats(): {
    epochs: number;
    archivedJournals: number;
    archivedBytes: number;
    snapshotBytes: number;
    activeJournalBytes: number;
  };
  close(): void;
}

export { HostContract };

export class Deadlock extends Error {}

// Thrown by a test runner's `PropertyRunner.start` when the case has
// nothing left to run: the instance ends, and the runner counts the case
// as discarded (std-testing.runner.examples-done).
export class EndCase extends Error {}

// A test body that ran longer than its `timeout`, or a program past its
// `--time-limit` (cli.limit.time): the `time-limit` panic
// (std-testing.option.timeout-any-duration, flow.panic.time-limit).
export class TimeLimit extends Error {}

// A trap's stack holds every Wasm frame, not V8's default ten: a panic
// report lists them all (cli.test.report.frame). A stack is captured only
// when an error is made, so a run that never traps pays nothing.
Error.stackTraceLimit = Infinity;

// The Wasm frames of a trap's stack, innermost first, as [function index,
// module byte offset] pairs from V8's `wasm-function[i]:0xOFF`; the test
// command maps them through the module's line table (crates/hd_wasm_linear/DESIGN.md §15.9).
const wasmFrames = (error: Dynamic): [number, number][] =>
  Array.from(
    String(error.stack ?? "").matchAll(/wasm-function\[(\d+)\]:0x([0-9a-f]+)/g),
    (message) => [Number(message[1]), parseInt(message[2], 16)],
  );

// The panic report of a body that overran its timeout of `ms`
// milliseconds.
export const timeLimitReport = (ms: number): string =>
  `panic: time-limit: the test case ran longer than its timeout of ${ms}ms\n`;

// What `hd.poll` returns for a root that ended cancelled, its cleanup run
// to its end, and one less when a part of it was cut short (rt.rs
// `CANCELLED_END`, req.entry.cancelled).
const CANCELLED_END = -2;

// The start of every panic report after its `panic: `, the instance's
// and the host's own, while the facts labeled `fact` are evaluated, or ""
// when none are (`fact` null): a panic while a facts body runs is
// `fact-evaluation-failed`, naming its facts before the original category
// (annot.fact.eval.panic-runtime).
// The host's own panic report `report`, `panic: CATEGORY: MESSAGE` and a
// newline, as it is written while the facts labeled `fact` are evaluated.
// The category and message of a recorded panic report, the text after
// `panic: `: `CATEGORY: MESSAGE` and a newline, or `CATEGORY` alone. The
// category is the compiler's or the host's, never program text, so the
// first `: ` ends it; the message is the rest, whatever it holds.
export const panicOfReport = (report: string): { category: string; message: string } => {
  const text = report.endsWith("\n") ? report.slice(0, -1) : report;
  const colon = text.indexOf(": ");
  return colon < 0
    ? { category: text, message: "" }
    : { category: text.slice(0, colon), message: text.slice(colon + 2) };
};

// Whether `text` can be an `Http` request URL (std-http.url.valid,
// .non-ascii, .no-normalize): RFC 3986 has no space, control or non-ASCII
// character, and a provider does not percent-decode or IDNA-map a host.
// `new URL` does both, so a program's URL is checked before it is parsed;
// a host is written in punycode.
export const isAsciiUrl = (text: string): boolean => {
  if (!/^[\x21-\x7e]*$/.test(text)) {
    return false;
  }
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(text)?.[1] ?? "";
  return !authority.slice(authority.lastIndexOf("@") + 1).includes("%");
};

// The host of an absolute URL as written, in lowercase and without the
// brackets of an IPv6 literal (std-http.url.grant, .no-normalize): `new URL`
// rewrites `127.1` to `127.0.0.1` and drops a trailing dot's meaning, which
// a grant must not see.
export const writtenHost = (text: string): string => {
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(text)?.[1] ?? "";
  const hostport = authority.slice(authority.lastIndexOf("@") + 1);
  const v6 = /^\[([^\]]*)\]/.exec(hostport);
  return (v6 ? v6[1]! : hostport.replace(/:[^:]*$/, "")).toLowerCase();
};

// A program's capability grant as `hd` resolved it (Grant Precedence):
// each trait maps to `true`, unrestricted, `false`, a total deny, or its
// scope entries, whose paths are absolute and resolved; a trait it leaves
// out has no limit. A provider asks these before it touches a resource
// (cli.host.default-profile.granted), as `hd_run::Grants` does.
//
// `resolveTarget(path, cwd)` names the path a scope entry is compared with
// (cli.cap.scope.path.resolved); the Node layer resolves `..` and symbolic
// links, and a host with no file system leaves it out. `sep` is the
// separator of the paths it returns and of the entries: "\\" on Windows.
export function createGrant(
  limits: Record<string, boolean | string[]> = {},
  resolveTarget: (path: string, cwd?: string) => string = (path) => path,
  sep = "/",
): Grant {
  const limit = (key: string): boolean | string[] | null =>
    Object.hasOwn(limits, key) ? limits[key]! : null;
  const check = (key: string, covers: (entry: string) => boolean): boolean => {
    const scope = limit(key);
    if (scope === null || scope === true) {
      return true;
    }
    return scope === false ? false : scope.some(covers);
  };
  return {
    allows: (key: string) => limit(key) !== false,
    limited: (key: string) => limit(key) !== null && limit(key) !== true,
    // An entry covers the file it names or every path under the directory
    // it names (cli.cap.scope.path); a relative path is from `cwd`.
    coversPath: (key: string, path: string, cwd?: string) => {
      const target = resolveTarget(path, cwd);
      return check(
        key,
        (entry) => target === entry || target.startsWith(entry.endsWith(sep) ? entry : entry + sep),
      );
    },
    // cli.cap.scope.host, .host.forms, cli.cap.scope.net.
    coversHost: (key: string, host: string, port: number | null = null) =>
      check(key, (entry) => {
        let offeredHost = entry;
        let text: string | null = null;
        const v6 = /^\[([^\]]*)\](?::(\d+))?$/.exec(entry);
        if (v6) {
          [offeredHost, text] = [v6[1]!, v6[2] ?? null];
        } else if (entry.split(":").length === 2) {
          [offeredHost, text] = [entry.split(":")[0]!, entry.split(":")[1]!];
        }
        if (text !== null && !/^\d+$/.test(text)) {
          text = null;
        }
        const hostOk = offeredHost.startsWith("*.")
          ? host.endsWith(offeredHost.slice(1)) && host.length > offeredHost.length - 1
          : offeredHost.toLowerCase() === host.toLowerCase();
        return hostOk && (text === null || Number(text) === port);
      }),
    // cli.cap.scope.net.lookup: the host match alone, whatever port an
    // entry names.
    coversHostName: (key: string, host: string) =>
      check(key, (entry) => {
        const v6 = /^\[([^\]]*)\]/.exec(entry);
        let offeredHost = entry;
        if (v6) {
          offeredHost = v6[1]!;
        } else if (entry.split(":").length === 2) {
          offeredHost = entry.split(":")[0]!;
        }
        return offeredHost.startsWith("*.")
          ? host.endsWith(offeredHost.slice(1)) && host.length > offeredHost.length - 1
          : offeredHost.toLowerCase() === host.toLowerCase();
      }),
    // cli.cap.scope.env, .process, .sys.
    coversName: (key: string, name: string) =>
      check(key, (error) =>
        key === "Env" && error.endsWith("*") ? name.startsWith(error.slice(0, -1)) : error === name,
      ),
  };
}

// A synchronous sleep of `ms` milliseconds. Where `SharedArrayBuffer`
// exists and the context may block (Node, a worker), it waits with
// `Atomics.wait`. Elsewhere (a page that is not cross-origin isolated, or a
// browser's main thread) it spins until the deadline.
function sleeper(): (ms: number) => void {
  if (typeof SharedArrayBuffer === "function") {
    const cell = new Int32Array(new SharedArrayBuffer(4));
    try {
      Atomics.wait(cell, 0, 0, 0);
      return (ms: number) => {
        Atomics.wait(cell, 0, 0, ms);
      };
    } catch {
      // This context may not block.
    }
  }
  return (ms: number) => {
    const end = performance.now() + ms;
    while (performance.now() < end) {}
  };
}

// `hd:rt`: the runtime imports both hosts share (§17.2, §17.10).
function rtImports({
  sink,
  flush,
  view,
  st,
  complete,
  nextDeadline,
  deadlock,
  until,
  overran,
}: DynamicRecord): Record<string, ProviderFunction> {
  const sleep = sleeper();
  return {
    stderr: (len) => {
      st.reported = true;
      flush();
      const text = decodeUtf8(view(len));
      sink.err(text);
      if (st.panic !== null) {
        st.panic += text;
      }
    },
    // A panic stub's start: the host writes `panic: `, and what the
    // instance writes to standard error until it traps is the report,
    // which a test runner reads instead of the standard error text.
    panic: () => {
      st.reported = true;
      flush();
      const head = factHead(st.facts.at(-1));
      sink.err(`panic: ${head}`);
      st.panic = head;
    },
    // A facts body starts, with its label in the buffer, or ends (-1). A
    // supervisor that reports a time limit for the instance hears of it
    // through `sink.fact`.
    fact: (len) => {
      if (len < 0) {
        st.facts.pop();
      } else {
        st.facts.push(decodeUtf8(view(len)));
      }
      sink.fact?.(st.facts.at(-1) ?? null);
    },
    block: () => {
      for (;;) {
        const completion = complete();
        if (completion) {
          return completion;
        }
        if (st.readyOnly) {
          throw new HostContract("a synchronous host wait is pending in a synchronous step driver");
        }
        const deadline = nextDeadline();
        if (deadline === null) {
          deadlock();
        }
        flush();
        sleep(Math.max(0, until(deadline) - performance.now()));
        if (overran()) {
          throw new TimeLimit();
        }
      }
    },
    stdout: (len) => {
      st.out.push(decodeUtf8(view(len)));
    },
    abort: (host) => abortOperation(st, host),
    // The run's map hash key: 64 random bits. The instance asks once and
    // keeps it in a runtime global; a session's journal records the call,
    // so a rebuild hashes with the same key.
    hash_seed: () => new DataView(st.random(8).buffer).getBigInt64(0, true),
  };
}

// The host primitives of std (spec/std/README.md#standard-library-
// primitives): a float arrives as its value, and the text goes back
// in the exchange buffer as a string.
function primImports({ put, text, env }: DynamicRecord): Record<string, ProviderFunction> {
  return {
    format_f64: (value) => put(new Enc().str(shortest(value, 64))),
    format_f32: (value) => put(new Enc().str(shortest(value, 32))),
    format_f64_fixed: (value, digits) => put(new Enc().str(fixed(value, Number(digits)))),
    string_lower: (len) => put(new Enc().str(lower(text(len)))),
    string_upper: (len) => put(new Enc().str(upper(text(len)))),
    parse_f64: (len) => parse(text(len)),
    // `std.testing.snapshot`'s hook (cli.test.snapshot.update): in an
    // update run of an `it` case (`env.updates` is a list) it records the
    // text with the Wasm frames of the call and answers 1, so the case goes
    // on; otherwise it answers 0 and the snapshot fails.
    snapshot_update: (len) => {
      const actual = text(len);
      if (!env.updates) {
        return 0;
      }
      env.updates.push({ text: actual, frames: wasmFrames(new Error()) });
      return 1;
    },
  };
}

// A provider's answer to a method that may wait when the operation is
// pending: it completes at `at`, on `performance.now()`'s clock, and
// `done()`, when given, does it then and returns its result.
class Later {
  at: number;
  done: (() => Dynamic) | null;
  poll: (() => boolean) | null;
  abort: (() => void) | null;
  finish: (() => Dynamic) | null;

  constructor(
    at: number,
    {
      done,
      poll = null,
      abort = null,
      finish = null,
    }: {
      done: (() => Dynamic) | null;
      poll?: (() => boolean) | null;
      abort?: (() => void) | null;
      finish?: (() => Dynamic) | null;
    },
  ) {
    this.at = at;
    this.done = done;
    this.poll = poll;
    this.abort = abort;
    this.finish = finish;
  }
}
const later = (at: number, done: (() => Dynamic) | null = null): Later => new Later(at, { done });

// External I/O supplies a nonblocking readiness probe and a cancellation
// hook, including when complete() ran but the guest never took .finish.
// Timed operations keep their existing path. A pending probe is retried
// after a short wait, from either the async driver or synchronous rt.block.
// The optional delivery hook can replace the kept result, such as Closed
// after a resource closes between readiness and guest delivery.
export const pendingOperation = <ResultValue>(
  poll: () => boolean,
  done: () => ResultValue,
  abort: (() => void) | null = null,
  finish: (() => ResultValue | undefined) | null = null,
): Later => new Later(performance.now(), { done, poll, abort, finish });

function abortOperation(st: DynamicRecord, handle: number): void {
  const operation = st.ops.get(handle);
  st.ops.delete(handle);
  operation?.abort?.();
}
export function clearOperations(st: DynamicRecord): void {
  for (const handle of st.ops.keys()) {
    abortOperation(st, handle);
  }
}

// A method's provider arguments from its import's (§17.4): its scalar
// arguments are the import's parameters, in order, and its structured ones
// are encoded in the exchange buffer, whose length is the last parameter;
// a lone `string` crosses as its bytes alone. They are decoded from a copy
// of the buffer, which the result may overwrite.
function argsDecoder(
  message: Dynamic,
  view: (length: number) => Uint8Array,
  text: (length: number) => string,
): (raw: Dynamic[]) => Dynamic[] {
  const scalars = message.params.filter((item: Dynamic) => typeof item === "string").length;
  if (scalars === message.params.length) {
    return (raw) => raw;
  }
  if (message.params.length === 1 && message.params[0] === str) {
    return (raw) => [text(raw[0])];
  }
  return (raw) => {
    const data = new Dec(Uint8Array.from(view(raw[scalars])));
    let at = 0;
    return message.params.map((item: Dynamic) =>
      typeof item === "string" ? raw[at++] : item.dec(data),
    );
  };
}

// The imports of the host trait module `module` (§17.2) around `provider`,
// its methods' JS functions by name, as its generated descriptors
// (gen/abi.ts `METHODS`) shape them. `io` reads and writes the exchange
// buffer and keeps the pending operations.
function traitImports(
  module: string,
  provider: Record<string, ProviderFunction>,
  { view, text, put, pending, finished }: DynamicRecord,
): DynamicRecord {
  const out: DynamicRecord = {};
  const methods: DynamicRecord = METHODS;
  for (const [name, providerFunction] of Object.entries(provider)) {
    const message = methods[module]?.[name];
    if (!message) {
      throw new Error(`the host ABI has no \`${module}\` \`${name}\``);
    }
    const decode = argsDecoder(message, view, text);
    const key = `${module} ${name}`;
    // The import's result: a scalar as itself; a structured value written
    // at offset 0 and its length; `void` nothing, or 0 from a `.finish`.
    const written = (value: Dynamic): Dynamic => {
      if (typeof message.result !== "string") {
        const encoder = new Enc();
        message.result.enc(encoder, value);
        return put(encoder);
      }
      if (message.result !== "void") {
        return value;
      }
      return message.wait === "may" ? 0 : undefined;
    };
    if (message.wait === "never") {
      out[name] = (...raw: Dynamic[]) => written(providerFunction(...decode(raw)));
      continue;
    }
    out[`${name}.start`] = (...raw: Dynamic[]) => {
      const value = providerFunction(...decode(raw));
      return value instanceof Later
        ? pending({
            at: value.at,
            done: value.done,
            method: key,
            poll: value.poll,
            abort: value.abort,
            finish: value.finish,
          })
        : [0, written(value)];
    };
    out[`${name}.finish`] = (value: number) => written(finished(value, key));
  }
  return out;
}

// `hd:Console`: one text chunk per `write_sync`; `write_error_sync` writes
// its text to standard error, after the held output
// (module.console.write-error-sync.raw). Each write returns once its text
// is written (module.console.write-sync.never-suspends): a captured sink
// takes it at once, and a direct sink (output.ts) blocks until the
// stream has taken every byte.
function consoleProvider({
  st,
  sink,
  flush,
  grant,
}: DynamicRecord): Record<string, ProviderFunction> {
  const denied = (stream: string): Dynamic => Err({ tag: "NotGranted", stream: { tag: stream } });
  return {
    write_sync: (text: string) => {
      if (!grant.allows("Console")) {
        return denied("Stdout");
      }
      if (sink.write) {
        flush();
        return sink.write(1, text);
      }
      st.out.push(text);
      return Ok(undefined);
    },
    write_error_sync: (text: string) => {
      if (!grant.allows("Console")) {
        return denied("Stderr");
      }
      flush();
      if (sink.write) {
        return sink.write(2, text);
      }
      sink.err(text);
      return Ok(undefined);
    },
  };
}

// `hd:Clock`: the time, and sleeps that complete when due.
function clockProvider(): Record<string, ProviderFunction> {
  return {
    now: () => BigInt(Date.now()),
    monotonic: () => BigInt(Math.floor(performance.now())),
    sleep: (ms: Dynamic) => later(performance.now() + Number(ms)),
  };
}

// `count` random bytes (`getRandomValues` takes at most 65536 at a time).
// A platform layer may supply its own (`layer.randomBytes`): on Node the
// first use of `crypto` loads its module, about 3 ms of a run.
function randomBytes(count: number): Uint8Array {
  const out = new Uint8Array(count);
  for (let at = 0; at < count; at += 65536) {
    crypto.getRandomValues(out.subarray(at, at + 65536));
  }
  return out;
}

// The operating system's random source (cli.host.default-profile).
// `Random` has no scope entries: a grant of `false` refuses the whole
// program at startup (cli.cap.total.refuse).
function randomProvider(random: (count: number) => Uint8Array): Record<string, ProviderFunction> {
  return {
    next_u64: () => new DataView(random(8).buffer).getBigInt64(0, true),
    fill: (count: number) => random(count >>> 0),
  };
}

// `hd:Args`: the program's path and arguments.
function argsProvider({
  program,
  args,
}: {
  program: string;
  args: string[];
}): Record<string, ProviderFunction> {
  return { program: () => program, list: () => args };
}

// `hd:PropertyRunner`: property cases ask the test runner for draws.
function propertyRunnerProvider({ env }: { env: CoreEnv }): Record<string, ProviderFunction> {
  return {
    // The property runner's side of a property case
    // (std-testing.runner.start-case, .record, .show): the test runner
    // decides each case and keeps its draws and its input's text.
    start: (cases: Dynamic, shrink: Dynamic, examples: Dynamic) => {
      if (!env.runner) {
        throw new HostContract("no property runner binds this run");
      }
      return env.runner.start(cases, shrink, examples);
    },
    record: (value: Dynamic) => env.runner?.record(BigInt(value)),
    show: (text: string) => env.runner?.show(text),
  };
}

// `hd:TestRunner`: the hooks every host has (a platform layer adds
// `temp_dir`). Snapshot files are read and written through `files`
// (cases.ts describes it): the disk on Node, memory in the playground.
function testRunnerProvider({ env, st, files }: DynamicRecord): Record<string, ProviderFunction> {
  // Snapshot files of the case (std-testing.snapshot-file.*).
  const snapshotCheck = (actual: string): string => {
    const snapshot = env.snapshot;
    st.snapshots += 1;
    if (!snapshot || !files) {
      return "no snapshot file: this run names no snapshot directory";
    }
    // An `it_each` row's slug adds the row's index
    // (std-testing.snapshot-file.row).
    const slug = snapshot.slug + (env.runner?.slugSuffix() ?? "");
    const file = files.join(snapshot.dir, `${slug}-${st.snapshots}.snap`);
    const shown = snapshot.shown ? `${snapshot.shown}/${slug}-${st.snapshots}.snap` : file;
    const expected = files.read(file);
    if (expected !== null) {
      if (expected === actual) {
        return "";
      }
      if (!snapshot.update) {
        return `the text differs from the snapshot file ${shown}: expected ${JSON.stringify(expected)}, actual ${JSON.stringify(actual)}; run \`hd test --update\` to record it`;
      }
    } else if (!snapshot.update) {
      return `the snapshot file ${shown} is missing; run \`hd test --update\` to record it`;
    }
    files.write(file, actual);
    return "";
  };
  return {
    // The test runner (`env.runner`, cases.ts) picks the row an `it_each`
    // case runs; without one, row 0. Timeouts run under no limit here.
    row: (count: Dynamic) => (env.runner ? env.runner.row(count) : 0),
    // The runner fails a body that runs longer than its timeout: here at
    // the end of the run or of a wait, and in test.ts by stopping an
    // instance that never returns. A program deadline that ends first
    // stays (cli.limit.time).
    report_timeout: (millis: Dynamic) => {
      const ms = Number(millis);
      const at = performance.now() + ms;
      const base = st.baseDeadline !== null && st.baseDeadline < at;
      st.timeoutMs = base ? null : ms;
      st.deadline = base ? st.baseDeadline : at;
      env.runner?.timeout?.(ms);
    },
    snapshot_check: snapshotCheck,
  };
}

// A wire request's transport settings (std-http.settings.*): `dnsServers`
// is `null` or the literal addresses, `tlsRoots` the extra PEM roots,
// `tlsIdentity` `null` or `{ certificateChain, privateKey }`, and each
// timeout `null` or whole milliseconds.
export const httpSettings = (result: Dynamic): DynamicRecord => {
  const ms = (duration: Dynamic): number | null =>
    duration == null ? null : Number(duration.millis);
  return {
    dnsServers: result.dns_servers ?? null,
    tlsRoots: result.tls_roots ?? [],
    tlsIdentity:
      result.tls_identity == null
        ? null
        : {
            certificateChain: result.tls_identity.certificate_chain,
            privateKey: result.tls_identity.private_key,
          },
    connectTimeout: ms(result.connect_timeout),
    tlsHandshakeTimeout: ms(result.tls_handshake_timeout),
    responseHeaderTimeout: ms(result.response_header_timeout),
    bodyReadTimeout: ms(result.body_read_timeout),
  };
};

// The stable name of each setting a request changes from the host default,
// in a fixed order (std-http.settings.unsupported-name).
export const changedSettings = (settings: Dynamic): string[] =>
  (
    [
      ["resolver", settings?.dnsServers != null],
      ["tls.roots", (settings?.tlsRoots?.length ?? 0) > 0],
      ["tls.identity", settings?.tlsIdentity != null],
      ["connect_timeout", settings?.connectTimeout != null],
      ["tls_handshake_timeout", settings?.tlsHandshakeTimeout != null],
      ["response_header_timeout", settings?.responseHeaderTimeout != null],
      ["body_read_timeout", settings?.bodyReadTimeout != null],
    ] satisfies [string, boolean][]
  )
    .filter(([, changed]) => changed)
    .map(([name]) => name);

// The preflight refusal of a host that honours only the settings named in
// `supported`: `{ error: "UnsupportedSetting", text }` for the first other
// changed setting, or `null` (std-http.settings.preflight).
export const unsupportedSetting = (
  settings: Dynamic,
  supported: string[] = [],
): { error: string; text: string } | null => {
  const name = changedSettings(settings).find((each) => !supported.includes(each));
  return name === undefined ? null : { error: "UnsupportedSetting", text: name };
};

// `hd:Http` over `send`, a client that takes `{ method, url, headers,
// body, redirects, settings }` and answers
// `{ status, headers, body, url }`, or `{ error, text }` with an `HttpError`
// variant's name and text: http.ts's thread on Node, `browserSend` in the
// browser (std-http.send.*).
export function httpProvider(
  send: (request: Dynamic) => Dynamic,
): Record<string, ProviderFunction> {
  return {
    send: (request: Dynamic) => {
      const method =
        request.method.tag === "Other" ? request.method.name : request.method.tag.toUpperCase();
      const sent = send({
        method,
        url: request.url,
        headers: request.headers,
        body: request.body,
        redirects: request.redirects,
        settings: httpSettings(request),
      });
      const result = (answer: Dynamic): Dynamic =>
        answer.error ? Err(variant(HttpError, answer.error, answer.text)) : Ok(answer);
      // Node's worker can keep the request pending; browsers still answer
      // synchronously here. Encode the result only after readiness wins.
      return sent instanceof Later
        ? new Later(sent.at, {
            done: () => result(sent.done!()),
            poll: sent.poll,
            abort: sent.abort,
          })
        : result(sent);
    },
  };
}

function initialState(env: CoreEnv, layer: Dynamic): DynamicRecord {
  return {
    memory: null,
    // The operating system's random bytes: the platform's, or webcrypto.
    random: layer?.randomBytes ?? randomBytes,
    // The labels of the facts bodies running, innermost last (`hd:rt`
    // `fact`): a panic report names the innermost.
    facts: [],
    reported: false,
    // The panic report being written or written, after its `panic: `.
    panic: null,
    timeoutMs: null,
    baseDeadline: env.deadline == null ? null : env.deadline - performance.timeOrigin,
    deadline: env.deadline == null ? null : env.deadline - performance.timeOrigin,
    snapshots: 0,
    tempMade: false,
    nextHandle: 1,
    out: [],
    ops: new Map(),
    noticed: new Set(),
    payload: null,
    readyOnly: false,
    drivingMode: null,
    closed: false,
  };
}

// Request-scoped callers own their event loop. A turn polls once and
// publishes ready host completions without retaining a Promise or timer.
function createStepDriver(
  instance: WebAssembly.Instance,
  init: string,
  poll: string,
  { st, prime, complete, flush }: DynamicRecord,
): StepDriver {
  if (st.closed) {
    throw new HostContract("instance is closed");
  }
  if (st.drivingMode !== null) {
    throw new HostContract("the host already has a driver");
  }
  const ex: DynamicRecord = instance.exports;
  if (![init, poll, "hd.wake"].every((name) => typeof ex[name] === "function")) {
    throw new HostContract("the instance is missing root driver exports");
  }
  st.drivingMode = "sync";
  st.readyOnly = true;
  prime(instance);
  let started = false,
    busy = false,
    failed = false,
    status = -1;
  return {
    step() {
      if (st.closed) {
        throw new HostContract("instance is closed");
      }
      if (busy) {
        throw new HostContract("reentrant instance poll");
      }
      if (failed) {
        throw new HostContract("the driver has failed");
      }
      if (started && status !== -1) {
        return { status, wakes: 0 };
      }
      busy = true;
      try {
        try {
          if (!started) {
            started = true;
            ex[init]();
          }
          status = ex[poll](0);
          if (typeof status !== "number" || (status | 0) !== status) {
            throw new HostContract("root poll returned an invalid status");
          }
          const wakes = status === -1 ? complete() : 0;
          if (wakes) {
            ex["hd.wake"](wakes);
          }
          return { status, wakes };
        } finally {
          flush();
        }
      } catch (error) {
        failed = true;
        throw error;
      } finally {
        busy = false;
      }
    },
  };
}

function coreState(env: CoreEnv, layer: Dynamic): Dynamic {
  const module = env.module;
  return {
    args: env.args ?? [],
    program: env.program ?? "",
    grant: createGrant(env.grants ?? {}, layer?.resolveTarget, layer?.sep),
    seed: env.seed ?? null,
    st: initialState(env, layer),
    module,
    reportsCycles:
      module instanceof WebAssembly.Module &&
      WebAssembly.Module.customSections(module, "hd.cycles").length === 1,
  };
}

function buildCoreImports(options: Dynamic): Dynamic {
  const {
    env,
    layer,
    sink,
    st,
    grant,
    flush,
    left,
    program,
    args,
    view,
    text,
    put,
    pending,
    finished,
    complete,
    nextDeadline,
    deadlock,
    until,
    overran,
  } = options;
  const providers: Record<string, Record<string, ProviderFunction>> = {
    "hd:Console": consoleProvider({ st, sink, flush, grant }),
    "hd:Clock": clockProvider(),
    "hd:Random": randomProvider(st.random),
    "hd:Args": argsProvider({ program, args }),
    "hd:TestRunner": testRunnerProvider({ env, st, files: env.files ?? layer?.files }),
    "hd:PropertyRunner": propertyRunnerProvider({ env }),
  };
  // The platform layer's providers; a module it names extends the core's
  // provider for that module rather than replacing it.
  const platform: Dynamic = layer?.providers({ env, sink, st, grant, flush, left }) ?? null;
  for (const [module, provider] of Object.entries(platform?.traits ?? {})) {
    providers[module] = {
      ...providers[module],
      ...(provider as Record<string, ProviderFunction>),
    };
  }
  const imports: DynamicRecord = {
    "hd:rt": rtImports({ sink, flush, view, st, complete, nextDeadline, deadlock, until, overran }),
    "hd:prim": primImports({ put, text, env }),
  };
  const io: DynamicRecord = {
    view,
    text,
    put,
    pending,
    finished,
    prepare: pendingOperation,
    prepared: (value: Dynamic) => (value instanceof Later ? value : null),
  };
  for (const [module, provider] of Object.entries(providers)) {
    imports[module] = traitImports(module, provider, io);
  }
  if (env.providers) {
    Object.assign(imports, env.providers({ put, Enc, pending, finish: options.finish }));
  }
  return { imports, io, platform };
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
// `start`, `record`, `show`; cases.ts), and `files`, where its snapshot
// files are when the platform layer's `files` is not where they are.
// `env.deadline` is when a `--time-limit` ends (cli.limit.time), in
// milliseconds on `performance.timeOrigin + performance.now()`, a clock
// that threads share.
/* oxlint-disable max-lines-per-function -- one host lifetime owns these closures. */
export function createCoreHost(sink: Sink, env: CoreEnv = {}, layer: Dynamic = null): Host {
  const { args, program, grant, seed, st, module, reportsCycles } = coreState(env, layer);
  // The instance's mutable state, shared with the provider functions
  // below: the exchange memory, trap reporting, the test timeout, case
  // files, pending operations, output, and noticed names.
  const writeCycleReport = async (instance: WebAssembly.Instance): Promise<void> => {
    if (!reportsCycles) {
      return;
    }
    const { cycleReport } = await import("./checked-report.ts");
    const report = cycleReport(module!, instance);
    if (report !== null) {
      sink.err(report);
    }
  };
  const view = (len: number): Uint8Array => new Uint8Array(st.memory.buffer, 0, len);
  const text = (len: number): string => decodeUtf8(view(len));
  // Writes an encoded result (or its bytes) at offset 0, growing the
  // buffer to fit, and returns its length (§17.3). A session's recorder
  // reads the bytes back as the call's result (`st.payload`).
  const put = (encoder: Dynamic): number => {
    const bytes = encoder instanceof Uint8Array ? encoder : encoder.done();
    if (bytes.length > st.memory.buffer.byteLength) {
      st.memory.grow(Math.ceil((bytes.length - st.memory.buffer.byteLength) / 65536));
    }
    new Uint8Array(st.memory.buffer, 0, bytes.length).set(bytes);
    st.payload = bytes;
    return bytes.length;
  };
  const flush = (): void => {
    if (st.out.length) {
      sink.out(st.out.join(""));
      st.out.length = 0;
    }
  };
  // Pending operations by handle: `at` is when it completes, `done()` what
  // it does then, its result kept as `value`, and `method` the import
  // module and method that started it, whose `.finish` alone takes it.
  const pending = ({
    at,
    done,
    method = null,
    poll = null,
    abort = null,
    finish = null,
  }: {
    at: number;
    done: (() => Dynamic) | null;
    method?: string | null;
    poll?: (() => boolean) | null;
    abort?: (() => void) | null;
    finish?: (() => Dynamic) | null;
  }): [number, number] => {
    const handle = st.nextHandle++;
    st.ops.set(handle, {
      at,
      done,
      method,
      poll,
      abort,
      finish,
      completed: false,
      value: undefined,
    });
    return [1, handle];
  };
  const nextDeadline = (): number | null => {
    let value: number | null = null;
    for (const options of st.ops.values()) {
      if (!options.completed && (value === null || options.at < value)) {
        value = options.at;
      }
    }
    return value;
  };
  // Completes every due operation and writes the handles to the buffer.
  const complete = (): number => {
    const now = performance.now();
    const due = [...st.ops]
      .filter(([, options]) => !options.completed && (options.poll || options.at <= now))
      .sort((left, right) => left[1].at - right[1].at || left[0] - right[0])
      .filter(([, options]) => {
        if (!options.poll || options.poll()) {
          return true;
        }
        options.at = performance.now() + 1;
        return false;
      });
    if (due.length * 4 > st.memory.buffer.byteLength) {
      st.memory.grow(Math.ceil((due.length * 4) / 65536));
    }
    const words = new DataView(st.memory.buffer);
    due.forEach(([handle, options], i) => {
      options.completed = true;
      if (options.done) {
        options.value = options.done();
      }
      words.setInt32(4 * i, handle, true);
    });
    return due.length;
  };
  // Writes the host's own panic report `report`, `panic: CATEGORY:
  // MESSAGE` and a newline, and records it.
  const hostPanic = (report: string): void => {
    const text = reportIn(st.facts.at(-1), report);
    flush();
    sink.err(text);
    st.reported = true;
    st.panic = text.slice("panic: ".length);
  };
  const deadlock = (): never => {
    hostPanic("panic: deadlock: the program waits, but no host operation is pending\n");
    throw new Deadlock();
  };
  const finish = (handle: number): void => {
    st.ops.delete(handle);
  };
  // The result of the completed operation `h` that `method` started, which
  // its `.finish` takes; any other handle is a broken contract (§17.2).
  const finished = (handle: number, method: string): Dynamic => {
    const options = st.ops.get(handle);
    if (options?.method !== method || !options.completed) {
      throw new HostContract("a finish of an operation that is not pending");
    }
    // Ownership transfers only at the validated guest .finish, not when
    // complete() computes the value. Abort still owns an undelivered result.
    const delivered = options.finish?.();
    st.ops.delete(handle);
    return delivered === undefined ? options.value : delivered;
  };
  // Waits at most until `d`, and never past the timeout: a pending host
  // wait is cut at the deadline (crates/hd_run_wasmtime/DESIGN.md §17.8).
  const until = (ms: number): number => (st.deadline === null ? ms : Math.min(ms, st.deadline));
  const overran = (): boolean => st.deadline !== null && performance.now() > st.deadline;
  // The milliseconds left before the deadline. A provider that waits
  // inside its `.start` throws `TimeLimit` when none are left.
  const left = (): number => (st.deadline === null ? Infinity : st.deadline - performance.now());
  const { imports, io, platform } = buildCoreImports({
    env,
    layer,
    sink,
    st,
    grant,
    flush,
    left,
    program,
    args,
    view,
    text,
    put,
    pending,
    finished,
    finish,
    complete,
    nextDeadline,
    deadlock,
    until,
    overran,
  });
  // Invoker-selected package providers use signature-generated glue. A guest
  // descriptor is data only: it never selects or loads a JavaScript module.
  const packageRuntime = bindPackageRuntime({
    env,
    module,
    io,
    imports,
    grant,
    platform,
    isAsciiUrl,
    writtenHost,
  });

  // Points the host at `instance`'s memory, for a driver that calls the
  // exports itself instead of `run` (a scenario's drivers): reports and
  // codecs read the exchange buffer through it.
  const prime = (instance: WebAssembly.Instance): void => {
    st.memory = instance.exports["hd.x"] ?? null;
    // No facts body runs yet, whatever an earlier input left.
    st.facts = [];
  };

  const createDriver = (
    instance: WebAssembly.Instance,
    init = "hd.init",
    poll = "hd.poll",
  ): StepDriver => createStepDriver(instance, init, poll, { st, prime, complete, flush });

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
  //
  // A run the host may cancel passes `cancel`, an `entryCancel`: a request
  // found while the root waits calls `hd.cancel` and sets the cleanup
  // deadline, and the next request or that deadline calls `hd.abandon`.
  // The deadline is a timer of its own, so a cleanup that waits on nothing
  // else waits for it instead of deadlocking. A root that ends cancelled,
  // whoever cancelled it, ends the run with `cancelled` on standard error
  // and status 130 (cli.cancel.end).
  const continueRun = async ({
    instance,
    ex,
    poll,
    status,
    driver,
    cancel,
    firstError = null,
  }: {
    instance: WebAssembly.Instance;
    ex: DynamicRecord;
    poll: string;
    status: number;
    driver: Dynamic;
    cancel: Dynamic;
    firstError?: unknown;
  }): Promise<RunResult> => {
    let trapped = false;
    const ended = false;
    let limited = false;
    let cancelled = false;
    let frames: [number, number][] = [];
    const wait = (ms: number): Promise<Dynamic> =>
      driver?.wait(ms) ?? cancel?.wait(ms) ?? new Promise((resolve) => setTimeout(resolve, ms));
    // The requests acted on, the cleanup deadline once the entry is
    // cancelled, and whether it was abandoned.
    let handled = 0;
    let cleanupBy: number | null = null;
    let abandoned = false;
    const pollCancellation = (): boolean => {
      if (!cancel) {
        return false;
      }
      const asked = cancel.requests() > handled;
      if (asked) {
        handled += 1;
      }
      if (asked && cleanupBy === null) {
        cleanupBy = performance.now() + cancel.boundMs;
        ex["hd.cancel"]();
        status = ex[poll](0);
        return true;
      }
      if (cleanupBy !== null && !abandoned && (asked || performance.now() >= cleanupBy)) {
        abandoned = true;
        ex["hd.abandon"]();
        status = ex[poll](0);
        return true;
      }
      return false;
    };
    const waitForDeadline = async (): Promise<boolean> => {
      let deadline = nextDeadline();
      if (cleanupBy !== null && !abandoned && (deadline === null || cleanupBy < deadline)) {
        deadline = cleanupBy;
      }
      if (deadline === null) {
        deadlock();
      }
      flush();
      await wait(Math.max(0, until(deadline!) - performance.now()));
      if (overran()) {
        throw new TimeLimit();
      }
      const stop = driver?.stopping(Boolean(ex["hd.cancel"]));
      if (!stop) {
        return false;
      }
      if (stop === "cancelled") {
        ex["hd.cancel"]();
      }
      flush();
      sink.err(`${stop}\n`);
      st.reported = true;
      status = 3;
      trapped = true;
      return true;
    };
    const resumeAfterIdle = async (): Promise<boolean> => {
      if (pollCancellation()) {
        return true;
      }
      return !(await waitForDeadline());
    };
    const drive = async (): Promise<void> => {
      while (status === -1) {
        let count;
        if (driver?.replaying()) {
          count = driver.served();
        } else {
          count = complete();
          const resume = count === 0 ? await resumeAfterIdle() : null;
          if (resume === false) {
            break;
          }
          if (resume === true) {
            continue;
          }
        }
        driver?.woke(count);
        ex["hd.wake"](count);
        driver?.poll();
        status = ex[poll](0);
      }
    };
    const finishCancellation = (): void => {
      if (status !== CANCELLED_END && status !== CANCELLED_END - 1) {
        return;
      }
      flush();
      sink.err(status === CANCELLED_END ? "cancelled\n" : "cancelled, cleanup abandoned\n");
      status = CANCEL_STATUS;
      cancelled = true;
    };
    const handleError = async (error: unknown): Promise<RunResult | null> => {
      flush();
      if (error instanceof EndCase) {
        await writeCycleReport(instance);
        return { status: 0, trapped: false, ended: true };
      }
      if (error instanceof HostContract) {
        hostPanic(`panic: host-contract: ${error.message}\n`);
      } else if (error instanceof TimeLimit) {
        st.reported = true;
        limited = true;
      } else if (error instanceof RangeError && /call stack size/i.test(error.message)) {
        // V8 reports a guest stack overflow as a RangeError ("Maximum call
        // stack size exceeded"), not a WebAssembly.RuntimeError; it is the
        // stack-exhausted panic. Other RangeErrors are host bugs and rethrow.
        hostPanic(
          "panic: stack-exhausted: the call stack ran out; check for recursion that never ends\n",
        );
        frames = wasmFrames(error);
      } else if (error instanceof WebAssembly.RuntimeError) {
        frames = wasmFrames(error);
      } else if (env.isReplayDivergence?.(error)) {
        st.reported = true;
      } else if (!(error instanceof Deadlock)) {
        throw error;
      }
      // A panic stub wrote its report; any other trap is an internal error.
      if (!st.reported) {
        sink.err(`internal error: ${(error as Dynamic).message}\n`);
      }
      status = 3;
      trapped = true;
      return null;
    };
    const enforceDeadline = (): void => {
      // A body that ended after its deadline ran longer than its timeout,
      // whatever it did after the deadline.
      if (!limited && !overran()) {
        return;
      }
      hostPanic(st.timeoutMs === null ? PROGRAM_TIME_LIMIT : timeLimitReport(st.timeoutMs));
      status = 3;
      trapped = true;
      frames = [];
    };
    try {
      if (firstError !== null) {
        throw firstError;
      }
      await drive();
      finishCancellation();
    } catch (error) {
      const endedRun = await handleError(error);
      if (endedRun) {
        return endedRun;
      }
    }
    flush();
    enforceDeadline();
    // A trap's panic as the host recorded it, or null.
    const panic = trapped && st.panic !== null ? panicOfReport(st.panic) : null;
    await writeCycleReport(instance);
    return { status, trapped, ended, frames, panic, cancelled: cancelled && !trapped };
  };

  // Keep the common no-suspension path apart from the full async driver:
  // a fresh Node process can compile this small wrapper without compiling
  // cancellation, waits, traps and wake delivery that it never reaches.
  const run = (
    instance: WebAssembly.Instance,
    init: string,
    poll: string,
    { driver = null, cancel = null }: { driver?: Dynamic; cancel?: Dynamic } = {},
  ): Promise<RunResult> => {
    if (st.drivingMode === "sync") {
      return Promise.reject(new HostContract("the host already has a synchronous driver"));
    }
    st.drivingMode = "async";
    try {
      prime(instance);
    } catch (error) {
      return Promise.reject(error);
    }
    const ex: DynamicRecord = instance.exports;
    let status: number;
    try {
      ex[init]();
      driver?.poll();
      status = ex[poll](0);
    } catch (error) {
      return continueRun({ instance, ex, poll, status: 0, driver, cancel, firstError: error });
    }
    if (status === -1 || status === CANCELLED_END || status === CANCELLED_END - 1) {
      return continueRun({ instance, ex, poll, status, driver, cancel });
    }
    try {
      flush();
      if (overran()) {
        return continueRun({ instance, ex, poll, status, driver, cancel });
      }
      const result = {
        status,
        trapped: false,
        ended: false,
        frames: [],
        panic: null,
        cancelled: false,
      };
      return reportsCycles
        ? writeCycleReport(instance).then(() => result)
        : Promise.resolve(result);
    } catch (error) {
      return Promise.reject(error);
    }
  };

  // Releases what the platform layer holds (its client threads, its
  // scratch directory).
  const close = (): void => {
    st.closed = true;
    try {
      clearOperations(st);
    } finally {
      try {
        packageRuntime?.close();
      } finally {
        platform?.close?.();
      }
    }
  };

  // The host handles the program left open, as `[kind, count]` pairs with
  // a nonzero count, for the leak report of `hd test` (cli.test.leak).
  const leaks = (): [string, number][] => platform?.leaks?.() ?? [];

  // What a REPL session reaches past the import object: the instance
  // state, the result writer, the deadlock report and the output.
  const core: DynamicRecord = { st, put, deadlock, flush };

  return {
    imports,
    run,
    createDriver,
    prime,
    close,
    leaks,
    args,
    program,
    grant,
    seed,
    tempDir: platform?.tempDir,
    core,
  };
}
/* oxlint-enable max-lines-per-function */
