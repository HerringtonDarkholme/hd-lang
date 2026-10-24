// The platform-neutral core of the JS host (V8 in Node, and browsers): the
// import object of the ABI in `hd_host_abi` (crates/hd_run_wasmtime/DESIGN.md §17.2,
// §17.10), built from the method descriptors and codecs generated from the
// ABI model (gen/abi.mjs, over wire.mjs's encoding of §17.4); the providers
// that need nothing from the platform (`Console`, `Clock`, `Random`,
// `Args`, `rt`, `prim`, and the test and property runners' hooks); and the
// entry driver of crates/hd_wasm_linear/DESIGN.md §14.4. It uses web-standard APIs
// only. A platform layer (node.mjs, web.mjs) adds the providers that touch
// the machine by passing `layer` to `createCoreHost`.
//
// - A provider is a host trait's methods as JS functions by name: each
//   takes its arguments and returns its result as plain values (wire.mjs
//   says which), and `traitImports` makes the imports of §17.2 around it.
//   A method that never waits is one import; one that may wait is
//   `.start`, which returns `[status, value]`, then `.finish(h)`. Status 0:
//   done at once, the result is in the buffer and `value` is its length.
//   Status 1: pending, `value` is its handle. `hd:Console` `write`
//   and `write_error` can wait for output backpressure; captured/browser
//   sinks complete immediately. `hd:Clock`
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
//   (journal.mjs), as `hd_run_wasmtime`'s session does.
import { cycleReport } from "./checked-report.mjs";
import { shortest, fixed, lower, upper, parse } from "./float.mjs";
import { HttpError, METHODS } from "./gen/abi.mjs";
import {
  Diverged,
  Recorder,
  Replay,
  SESSION_CAP,
  callArgs,
  importTypes,
  ran as ranText,
  recorded,
} from "./journal.mjs";
import { Dec, decodeUtf8, Enc, Err, HostContract, Ok, str, variant } from "./wire.mjs";
import { createPackageRuntime, readPackageContract } from "./package-providers.mjs";

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
const wasmFrames = (e) =>
  Array.from(String(e.stack ?? "").matchAll(/wasm-function\[(\d+)\]:0x([0-9a-f]+)/g), (m) => [
    Number(m[1]),
    parseInt(m[2], 16),
  ]);

// The panic report of a body that overran its timeout of `ms`
// milliseconds.
export const timeLimitReport = (ms) =>
  `panic: time-limit: the test case ran longer than its timeout of ${ms}ms\n`;

// The panic report of a program that ran past its `--time-limit`.
export const PROGRAM_TIME_LIMIT = "panic: time-limit: the program exceeded its time limit\n";

// What `hd.poll` returns for a root that ended cancelled, its cleanup run
// to its end, and one less when a part of it was cut short (rt.rs
// `CANCELLED_END`, req.entry.cancelled).
const CANCELLED_END = -2;

// The status of a run whose entry ended cancelled, and of one a Ctrl-C
// stopped while it computed: 128 plus SIGINT's number, as a shell reports a
// process that SIGINT ended (cli.cancel.status).
export const CANCEL_STATUS = 130;

// A host's cancellation of the entry (req.entry.cancel, cli.cancel.ctrl-c):
// `request()` asks for it, as a Ctrl-C does. `run` cancels the entry at the
// first request that finds it waiting on the host, and abandons it at the
// next one, or once `boundMs` milliseconds have passed since that cancel,
// its cleanup bound (cli.cancel.bound). `wait(ms)` is a host wait that a
// request ends at once; `onWait(true)` and `onWait(false)` hear each such
// wait start and end, so a platform can listen for its interrupt only
// while the program waits. `run` reads `requests()`, the count so far.
export function entryCancel(boundMs, onWait = null) {
  let requests = 0;
  let wake = null;
  return {
    boundMs,
    request: () => {
      requests += 1;
      wake?.();
    },
    requests: () => requests,
    wait: (ms) =>
      new Promise((resolve) => {
        const done = () => {
          clearTimeout(timer);
          wake = null;
          onWait?.(false);
          resolve();
        };
        const timer = setTimeout(done, ms);
        wake = done;
        onWait?.(true);
      }),
  };
}

// The start of every panic report after its `panic: `, the instance's
// and the host's own, while the facts labeled `fact` are evaluated, or ""
// when none are (`fact` null): a panic while a facts body runs is
// `fact-evaluation-failed`, naming its facts before the original category
// (annot.fact.eval.panic-runtime).
export const factHead = (fact) =>
  fact == null ? "" : `fact-evaluation-failed: the facts of \`${fact}\` panicked with `;

// The host's own panic report `report`, `panic: CATEGORY: MESSAGE` and a
// newline, as it is written while the facts labeled `fact` are evaluated.
export const reportIn = (fact, report) =>
  `panic: ${factHead(fact)}${report.slice("panic: ".length)}`;

// The category and message of a recorded panic report, the text after
// `panic: `: `CATEGORY: MESSAGE` and a newline, or `CATEGORY` alone. The
// category is the compiler's or the host's, never program text, so the
// first `: ` ends it; the message is the rest, whatever it holds.
export const panicOfReport = (report) => {
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
export const isAsciiUrl = (text) => {
  if (!/^[\x21-\x7e]*$/.test(text)) return false;
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(text)?.[1] ?? "";
  return !authority.slice(authority.lastIndexOf("@") + 1).includes("%");
};

// The host of an absolute URL as written, in lowercase and without the
// brackets of an IPv6 literal (std-http.url.grant, .no-normalize): `new URL`
// rewrites `127.1` to `127.0.0.1` and drops a trailing dot's meaning, which
// a grant must not see.
export const writtenHost = (text) => {
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(text)?.[1] ?? "";
  const hostport = authority.slice(authority.lastIndexOf("@") + 1);
  const v6 = /^\[([^\]]*)\]/.exec(hostport);
  return (v6 ? v6[1] : hostport.replace(/:[^:]*$/, "")).toLowerCase();
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
      const text = decodeUtf8(view(len));
      sink.err(text);
      if (st.panic !== null) st.panic += text;
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
      if (len < 0) st.facts.pop();
      else st.facts.push(decodeUtf8(view(len)));
      sink.fact?.(st.facts.at(-1) ?? null);
    },
    block: () => {
      for (;;) {
        const n = complete();
        if (n) return n;
        const d = nextDeadline();
        if (d === null) deadlock();
        flush();
        sleep(Math.max(0, until(d) - performance.now()));
        if (overran()) throw new TimeLimit();
      }
    },
    stdout: (len) => {
      st.out.push(decodeUtf8(view(len)));
    },
    abort: (h) => abortOperation(st, h),
    // The run's map hash key: 64 random bits. The instance asks once and
    // keeps it in a runtime global; a session's journal records the call,
    // so a rebuild hashes with the same key.
    hash_seed: () => new DataView(randomBytes(8).buffer).getBigInt64(0, true),
  };
}

// The host primitives of std (spec/std/README.md#standard-library-
// primitives): a float arrives as its value, and the text goes back
// in the exchange buffer as a string.
function primImports({ put, text, env }) {
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
      if (!env.updates) return 0;
      env.updates.push({ text: actual, frames: wasmFrames(new Error()) });
      return 1;
    },
  };
}

// A provider's answer to a method that may wait when the operation is
// pending: it completes at `at`, on `performance.now()`'s clock, and
// `done()`, when given, does it then and returns its result.
class Later {
  constructor(at, done, poll = null, abort = null, finish = null) {
    this.at = at;
    this.done = done;
    this.poll = poll;
    this.abort = abort;
    this.finish = finish;
  }
}
const later = (at, done = null) => new Later(at, done);

// External I/O supplies a nonblocking readiness probe and a cancellation
// hook, including when complete() ran but the guest never took .finish.
// Timed operations keep their existing path. A pending probe is retried
// after a short wait, from either the async driver or synchronous rt.block.
// The optional delivery hook can replace the kept result, such as Closed
// after a resource closes between readiness and guest delivery.
export const pendingOperation = (poll, done, abort, finish = null) =>
  new Later(performance.now(), done, poll, abort, finish);

function abortOperation(st, h) {
  const operation = st.ops.get(h);
  st.ops.delete(h);
  operation?.abort?.();
}
function clearOperations(st) {
  for (const h of st.ops.keys()) abortOperation(st, h);
}

// A method's provider arguments from its import's (§17.4): its scalar
// arguments are the import's parameters, in order, and its structured ones
// are encoded in the exchange buffer, whose length is the last parameter;
// a lone `string` crosses as its bytes alone. They are decoded from a copy
// of the buffer, which the result may overwrite.
function argsDecoder(m, view, text) {
  const scalars = m.params.filter((p) => typeof p === "string").length;
  if (scalars === m.params.length) return (raw) => raw;
  if (m.params.length === 1 && m.params[0] === str) return (raw) => [text(raw[0])];
  return (raw) => {
    const d = new Dec(Uint8Array.from(view(raw[scalars])));
    let at = 0;
    return m.params.map((p) => (typeof p === "string" ? raw[at++] : p.dec(d)));
  };
}

// The imports of the host trait module `module` (§17.2) around `provider`,
// its methods' JS functions by name, as its generated descriptors
// (gen/abi.mjs `METHODS`) shape them. `io` reads and writes the exchange
// buffer and keeps the pending operations.
function traitImports(module, provider, { view, text, put, pending, finished }) {
  const out = {};
  for (const [name, f] of Object.entries(provider)) {
    const m = METHODS[module]?.[name];
    if (!m) throw new Error(`the host ABI has no \`${module}\` \`${name}\``);
    const decode = argsDecoder(m, view, text);
    const key = `${module} ${name}`;
    // The import's result: a scalar as itself; a structured value written
    // at offset 0 and its length; `void` nothing, or 0 from a `.finish`.
    const written = (v) => {
      if (typeof m.result !== "string") {
        const e = new Enc();
        m.result.enc(e, v);
        return put(e);
      }
      if (m.result !== "void") return v;
      return m.wait === "may" ? 0 : undefined;
    };
    if (m.wait === "never") {
      out[name] = (...raw) => written(f(...decode(raw)));
      continue;
    }
    out[`${name}.start`] = (...raw) => {
      const v = f(...decode(raw));
      return v instanceof Later
        ? pending(v.at, v.done, key, v.poll, v.abort, v.finish)
        : [0, written(v)];
    };
    out[`${name}.finish`] = (h) => written(finished(h, key));
  }
  return out;
}

// `hd:Console`: one text chunk per `write`, written when it completes, at
// once; `write_error` writes its text to standard error then, after
// the held output (module.console.write-error.raw).
function consoleProvider({ st, sink, flush, grant }) {
  const denied = (subject) => Err({ tag: "NotGranted", subject });
  return {
    write: (text) =>
      grant.allows("Console")
        ? sink.write
          ? (flush(), sink.write(1, text))
          : later(performance.now(), () => {
              st.out.push(text);
              return Ok(undefined);
            })
        : denied("stdout"),
    write_error: (text) =>
      grant.allows("Console")
        ? sink.write
          ? (flush(), sink.write(2, text))
          : later(performance.now(), () => {
              flush();
              sink.err(text);
              return Ok(undefined);
            })
        : denied("stderr"),
  };
}

// `hd:Clock`: the time, and sleeps that complete when due.
function clockProvider() {
  return {
    now: () => BigInt(Date.now()),
    monotonic: () => BigInt(Math.floor(performance.now())),
    sleep: (ms) => later(performance.now() + Number(ms)),
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
function randomProvider() {
  return {
    next_u64: () => new DataView(randomBytes(8).buffer).getBigInt64(0, true),
    fill: (count) => randomBytes(count >>> 0),
  };
}

// `hd:Args`: the program's path and arguments.
function argsProvider({ program, args }) {
  return { program: () => program, list: () => args };
}

// `hd:PropertyRunner`: property cases ask the test runner for draws.
function propertyRunnerProvider({ env }) {
  return {
    // The property runner's side of a property case
    // (std-testing.runner.start-case, .record, .show): the test runner
    // decides each case and keeps its draws and its input's text.
    start: (cases, shrink, examples) => {
      if (!env.runner) throw new HostContract("no property runner binds this run");
      return env.runner.start(cases, shrink, examples);
    },
    record: (value) => env.runner?.record(BigInt(value)),
    show: (text) => env.runner?.show(text),
  };
}

// `hd:TestRunner`: the hooks every host has (a platform layer adds
// `temp_dir`). Snapshot files are read and written through `files`
// (cases.mjs describes it): the disk on Node, memory in the playground.
function testRunnerProvider({ env, st, files }) {
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
    // instance that never returns. A program deadline that ends first
    // stays (cli.limit.time).
    report_timeout: (millis) => {
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

// `hd:Http` over `send`, a client that takes `{ method, url, headers,
// body, redirects }` and answers
// `{ status, headers, body, url }`, or `{ error, text }` with an `HttpError`
// variant's name and text: http.mjs's thread on Node, `browserSend` in the
// browser (std-http.send.*).
export function httpProvider(send) {
  return {
    send: (r) => {
      const method = r.method.tag === "Other" ? r.method.name : r.method.tag.toUpperCase();
      const a = send({
        method,
        url: r.url,
        headers: r.headers,
        body: r.body,
        redirects: r.redirects,
      });
      const result = (answer) =>
        answer.error ? Err(variant(HttpError, answer.error, answer.text)) : Ok(answer);
      // Node's worker can keep the request pending; browsers still answer
      // synchronously here. Encode the result only after readiness wins.
      return a instanceof Later
        ? new Later(a.at, () => result(a.done()), a.poll, a.abort)
        : result(a);
    },
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
// `env.deadline` is when a `--time-limit` ends (cli.limit.time), in
// milliseconds on `performance.timeOrigin + performance.now()`, a clock
// that threads share.
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
  };
  const view = (len) => new Uint8Array(st.memory.buffer, 0, len);
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
  const flush = () => {
    if (st.out.length) {
      sink.out(st.out.join(""));
      st.out.length = 0;
    }
  };
  // Pending operations by handle: `at` is when it completes, `done()` what
  // it does then, its result kept as `value`, and `method` the import
  // module and method that started it, whose `.finish` alone takes it.
  const pending = (at, done, method = null, poll = null, abort = null, finish = null) => {
    const h = st.nextHandle++;
    st.ops.set(h, { at, done, method, poll, abort, finish, completed: false, value: undefined });
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
      .filter(([, o]) => !o.completed && (o.poll || o.at <= now))
      .sort((a, b) => a[1].at - b[1].at || a[0] - b[0])
      .filter(([, o]) => {
        if (!o.poll || o.poll()) return true;
        o.at = performance.now() + 1;
        return false;
      });
    if (due.length * 4 > st.memory.buffer.byteLength) {
      st.memory.grow(Math.ceil((due.length * 4) / 65536));
    }
    const words = new DataView(st.memory.buffer);
    due.forEach(([h, o], i) => {
      o.completed = true;
      if (o.done) o.value = o.done();
      words.setInt32(4 * i, h, true);
    });
    return due.length;
  };
  // Writes the host's own panic report `report`, `panic: CATEGORY:
  // MESSAGE` and a newline, and records it.
  const hostPanic = (report) => {
    const text = reportIn(st.facts.at(-1), report);
    flush();
    sink.err(text);
    st.reported = true;
    st.panic = text.slice("panic: ".length);
  };
  const deadlock = () => {
    hostPanic("panic: deadlock: the program waits, but no host operation is pending\n");
    throw new Deadlock();
  };
  const finish = (h) => {
    st.ops.delete(h);
  };
  // The result of the completed operation `h` that `method` started, which
  // its `.finish` takes; any other handle is a broken contract (§17.2).
  const finished = (h, method) => {
    const o = st.ops.get(h);
    if (o?.method !== method || !o.completed) {
      throw new HostContract("a finish of an operation that is not pending");
    }
    // Ownership transfers only at the validated guest .finish, not when
    // complete() computes the value. Abort still owns an undelivered result.
    const delivered = o.finish?.();
    st.ops.delete(h);
    return delivered === undefined ? o.value : delivered;
  };
  // Waits at most until `d`, and never past the timeout: a pending host
  // wait is cut at the deadline (crates/hd_run_wasmtime/DESIGN.md §17.8).
  const until = (d) => (st.deadline === null ? d : Math.min(d, st.deadline));
  const overran = () => st.deadline !== null && performance.now() > st.deadline;
  // The milliseconds left before the deadline. A provider that waits
  // inside its `.start` throws `TimeLimit` when none are left.
  const left = () => (st.deadline === null ? Infinity : st.deadline - performance.now());
  const providers = {
    "hd:Console": consoleProvider({ st, sink, flush, grant }),
    "hd:Clock": clockProvider(),
    "hd:Random": randomProvider(),
    "hd:Args": argsProvider({ program, args }),
    "hd:TestRunner": testRunnerProvider({ env, st, files: env.files ?? layer?.files }),
    "hd:PropertyRunner": propertyRunnerProvider({ env }),
  };

  // The platform layer's providers; a module it names extends the core's
  // provider for that module rather than replacing it.
  const platform = layer?.providers({ env, sink, st, grant, flush, left }) ?? null;
  for (const [module, provider] of Object.entries(platform?.traits ?? {})) {
    providers[module] = { ...providers[module], ...provider };
  }
  const imports = {
    "hd:rt": rtImports({ sink, flush, view, st, complete, nextDeadline, deadlock, until, overran }),
    "hd:prim": primImports({ put, text, env }),
  };
  const io = { view, text, put, pending, finished };
  for (const [module, provider] of Object.entries(providers)) {
    imports[module] = traitImports(module, provider, io);
  }

  // A runtime profile's own providers (`env.providers`, profiles.mjs):
  // `pending` registers a wait the driver wakes, `finish` drops one.
  if (env.providers) Object.assign(imports, env.providers({ put, Enc, pending, finish }));

  // Invoker-selected package providers use signature-generated glue. A guest
  // descriptor is data only: it never selects or loads a JavaScript module.
  let packageRuntime = null;
  if (env.packageBindings) {
    packageRuntime = createPackageRuntime(io, env);
    try {
      Object.assign(imports, env.packageBindings(packageRuntime));
    } catch (error) {
      packageRuntime.close();
      platform?.close?.();
      throw error;
    }
  } else if (env.module) {
    try {
      const sections = WebAssembly.Module.customSections(env.module, "hd.capabilities");
      if (sections.length > 1) throw new HostContract("duplicate package host contracts");
      if (sections.length) {
        const required = readPackageContract(new Uint8Array(sections[0]));
        const i = required.find((i) => !i.handle) ?? required[0];
        if (i)
          throw new HostContract(
            `missing package host provider for ${i.identity.package}@${i.identity.version}/${i.identity.trait_path}; register it explicitly`,
          );
      }
    } catch (error) {
      platform?.close?.();
      throw error;
    }
  }

  // Points the host at `instance`'s memory, for a driver that calls the
  // exports itself instead of `run` (a scenario's drivers): reports and
  // codecs read the exchange buffer through it.
  const prime = (instance) => {
    st.memory = instance.exports["hd.x"] ?? null;
    // No facts body runs yet, whatever an earlier input left.
    st.facts = [];
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
  //
  // A run the host may cancel passes `cancel`, an `entryCancel`: a request
  // found while the root waits calls `hd.cancel` and sets the cleanup
  // deadline, and the next request or that deadline calls `hd.abandon`.
  // The deadline is a timer of its own, so a cleanup that waits on nothing
  // else waits for it instead of deadlocking. A root that ends cancelled,
  // whoever cancelled it, ends the run with `cancelled` on standard error
  // and status 130 (cli.cancel.end).
  const run = async (instance, init, poll, driver = null, cancel = null) => {
    prime(instance);
    const ex = instance.exports;
    let status = 0;
    let trapped = false;
    let ended = false;
    let limited = false;
    let cancelled = false;
    let frames = [];
    const wait = (ms) =>
      driver?.wait(ms) ?? cancel?.wait(ms) ?? new Promise((r) => setTimeout(r, ms));
    // The requests acted on, the cleanup deadline once the entry is
    // cancelled, and whether it was abandoned.
    let handled = 0;
    let cleanupBy = null;
    let abandoned = false;
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
            if (cancel) {
              const asked = cancel.requests() > handled;
              if (asked) handled += 1;
              if (asked && cleanupBy === null) {
                cleanupBy = performance.now() + cancel.boundMs;
                ex["hd.cancel"]();
                status = ex[poll](0);
                continue;
              }
              if (cleanupBy !== null && !abandoned && (asked || performance.now() >= cleanupBy)) {
                abandoned = true;
                ex["hd.abandon"]();
                status = ex[poll](0);
                continue;
              }
            }
            let d = nextDeadline();
            if (cleanupBy !== null && !abandoned && (d === null || cleanupBy < d)) d = cleanupBy;
            if (d === null) deadlock();
            flush();
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
      if (status === CANCELLED_END || status === CANCELLED_END - 1) {
        flush();
        sink.err(status === CANCELLED_END ? "cancelled\n" : "cancelled, cleanup abandoned\n");
        status = CANCEL_STATUS;
        cancelled = true;
      }
    } catch (e) {
      flush();
      if (e instanceof EndCase) {
        const report = cycleReport(env.module, instance);
        if (report !== null) sink.err(report);
        return { status: 0, trapped: false, ended: true };
      }
      if (e instanceof HostContract) {
        hostPanic(`panic: host-contract: ${e.message}\n`);
      } else if (e instanceof TimeLimit) {
        st.reported = true;
        limited = true;
      } else if (e instanceof RangeError && /call stack size/i.test(e.message)) {
        // V8 reports a guest stack overflow as a RangeError ("Maximum call
        // stack size exceeded"), not a WebAssembly.RuntimeError; it is the
        // stack-exhausted panic. Other RangeErrors are host bugs and rethrow.
        hostPanic(
          "panic: stack-exhausted: the call stack ran out; check for recursion that never ends\n",
        );
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
    if (limited || overran()) {
      hostPanic(st.timeoutMs === null ? PROGRAM_TIME_LIMIT : timeLimitReport(st.timeoutMs));
      status = 3;
      trapped = true;
      frames = [];
    }
    // A trap's panic as the host recorded it, or null.
    const panic = trapped && st.panic !== null ? panicOfReport(st.panic) : null;
    const report = cycleReport(env.module, instance);
    if (report !== null) sink.err(report);
    return { status, trapped, ended, frames, panic, cancelled: cancelled && !trapped };
  };

  // Releases what the platform layer holds (its client threads, its
  // scratch directory).
  const close = () => {
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
  const leaks = () => platform?.leaks?.() ?? [];

  // What a REPL session reaches past the import object: the instance
  // state, the result writer, the deadlock report and the output.
  const core = { st, put, deadlock, flush };

  return {
    imports,
    run,
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

// --- REPL sessions ------------------------------------------------------

// Which journal row a call of an import writes (`Kind::record` in
// hd_run_wasmtime): none for `hd:prim`, which answers from its arguments
// alone, or `hd:rt` `fact`, which only marks where a facts body runs, so a
// replay makes them again.
function recordOf(module, name) {
  if (module === "hd:prim" || (module === "hd:rt" && name === "fact")) return "skip";
  if (module === "hd:rt" && (name === "block" || name === "abort")) return name;
  return name.endsWith(".finish") ? "finish" : "call";
}

// The parameter that holds the length of the exchange buffer bytes a call
// reads, with its scalar parameters the call's request; null for none
// (`Kind::input`). A host method's is after its scalar parameters, when
// it takes a structured one (gen/abi.mjs `METHODS`).
function readsArgs(module, name) {
  if (module === "hd:rt") return name === "stderr" || name === "stdout" ? 0 : null;
  const starts = name.endsWith(".start");
  const m = METHODS[module]?.[starts ? name.slice(0, -".start".length) : name];
  if (!m || (m.wait === "may") !== starts) return null;
  const scalars = m.params.filter((p) => typeof p === "string").length;
  return scalars < m.params.length ? scalars : null;
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

// A REPL session (spec `cli.repl.*`), as `hd_run_wasmtime`'s
// `WasmtimeSession` runs one. It holds no instance from one input to the
// next: its state is its host and its journal (journal.mjs), the record of
// every input that ran and of each host call it made. `sink`, `env` and
// `layer` are `createCoreHost`'s: one host serves every input.
//
// - `run(wasm, sources, live, journal)` runs the session program `wasm`,
//   one module of every input the session keeps and the new one
//   (`hd_wasm_linear::Entry::Inputs`: `hd.init.N` and `hd.poll.N` run input
//   N). It instantiates it once and replays the journal's inputs (the
//   session's own, or `journal`), each of its text in `sources` (pairs of
//   number and text), with every host result served from the record
//   (`cli.repl.host.once`) and nothing printed: a rebuild by
//   `cli.repl.rebuild`. It skips stopped and cancelled inputs
//   (`cli.repl.rebuild.stopped`) and drops the first input whose text
//   changed or that asks for what its record does not hold, with every
//   later one (`cli.repl.rebuild.prefix`); then the kept ones replay
//   again in a new instance, and the new input runs only when each
//   dropped input is one `sources` leaves out. It runs input `live` (a
//   number, or null for none) as `hd run` runs an entry, recording it. It answers `{ kept, skipped, dropped, reason,
//   live }`, with `live` the new input's `{ status, end, joined, error }`
//   or null: status 0 joins the session; a panicking input ends with
//   status 3, its report on standard error. It throws when the journal
//   passed its cap or the module cannot be linked or instantiated.
// - After an input that did not end with status 0, the session drops the
//   host operations it left pending and calls `hd.reset`, so the next
//   input starts as the first did (`cli.repl.panic`); what it changed in
//   earlier values stays (`cli.repl.panic.mutations`).
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
  // operations it left pending, then clears the runtime state a trap
  // leaves set.
  const reset = (instance) => {
    clearOperations(st);
    instance.exports["hd.reset"]?.();
  };

  // Links the session program: its host imports through their call
  // sites. Throws for a host import this host does not have.
  const link = (module, wasm) => {
    const types = importTypes(wasm);
    const object = {};
    for (const { module: m, name, kind } of WebAssembly.Module.imports(module)) {
      const f = host.imports[m]?.[name];
      if (kind !== "function" || typeof f !== "function") {
        throw new Error(`the import \`${m}\` \`${name}\` is not in this host`);
      }
      const key = `${m}\0${name}`;
      if (!sites.has(key)) sites.set(key, site(m, name, f, types.get(key)));
      object[m] ??= {};
      object[m][name] = sites.get(key);
    }
    return object;
  };

  // Runs input `number` of the text `source` from `instance`'s exports
  // `hd.init.N` and `hd.poll.N`, recording it.
  const runInput = async (instance, number, source, wasm) => {
    halt = null;
    // A panic or standard error of an earlier input does not hide this
    // one's internal error.
    st.reported = false;
    st.panic = null;
    recorder.input(number, source, wasm);
    let outcome;
    try {
      const { status, trapped } = await host.run(
        instance,
        `hd.init.${number}`,
        `hd.poll.${number}`,
        driver,
      );
      const end = halt ?? (trapped ? { kind: "trapped" } : { kind: "exit", status });
      outcome = { status, end, joined: status === 0 && !trapped, error: null };
    } catch (e) {
      flush();
      outcome = { status: null, end: { kind: "failed" }, joined: false };
      outcome.error = String(e?.message ?? e);
    }
    if (!outcome.joined) reset(instance);
    recorder.end(outcome.end);
    return outcome;
  };

  // Starts the instance again with a journal that starts as `journal`,
  // over the same host.
  const restart = (journal) => {
    flush();
    clearOperations(st);
    st.reported = false;
    st.panic = null;
    st.memory = null;
    st.facts = [];
    recorder = journal;
    replay = null;
    muted = false;
  };

  // Runs `input` again from `instance`, of the text `source`, with its host
  // calls and wakes served from its record and its output dropped. Throws
  // the divergence: it asked for what the record does not hold, or ended
  // otherwise.
  const replayInput = async (instance, input, source, wasm) => {
    replay = new Replay(input);
    muted = true;
    let outcome;
    try {
      outcome = await runInput(instance, input.number, source, wasm);
    } finally {
      muted = false;
    }
    const served = replay;
    replay = null;
    served.end(outcome.end, input.end);
  };

  const run = async (wasm, sources, live = null, journal = recorder.journal()) => {
    if (journal === null) {
      throw new Error("the session's record of host calls is full, so it cannot run another input");
    }
    const record = recorded(journal);
    const texts = new Map(sources);
    // A stop that came while no input ran is dropped; one that comes
    // while the inputs replay is for the new input.
    stopped = false;
    const cap = recorder.cap;
    const module = await WebAssembly.compile(wasm);
    const start = async () => {
      // Refusing an import or failing instantiation must not replace the
      // last good journal. HD modules have no Wasm start function; their
      // effects begin only when runInput invokes the numbered entry.
      const instance = await WebAssembly.instantiate(module, link(module, wasm));
      restart(new Recorder(cap, record.execution));
      return instance;
    };
    let instance = await start();
    const ran = { kept: [], skipped: [], dropped: [], reason: null, live: null };
    const kept = [];
    for (const [at, input] of record.inputs.entries()) {
      if (input.end?.kind === "stopped" || input.end?.kind === "cancelled") {
        ran.skipped.push(input.number);
        continue;
      }
      const source = texts.get(input.number);
      try {
        if (source === undefined) throw new Error(`input ${input.number} is not in the session`);
        if (!ranText(input, source)) {
          throw new Error(`input ${input.number} is not the text the session ran`);
        }
        await replayInput(instance, input, source, wasm);
      } catch (e) {
        ran.dropped = record.inputs.slice(at).map((i) => i.number);
        ran.reason = String(e?.message ?? e);
        break;
      }
      ran.kept.push(input.number);
      kept.push([input, source]);
    }
    if (ran.dropped.length > 0) {
      instance = await start();
      for (const [input, source] of kept) {
        try {
          await replayInput(instance, input, source, wasm);
        } catch (e) {
          throw new Error(`the rebuild did not repeat itself: ${e.message}`);
        }
      }
      // An input left out of `sources` is one the caller forgot: the
      // program does not hold it, so the new input still runs. One that
      // diverged may have declared what the new input uses.
      if (ran.dropped.some((n) => texts.has(n))) return ran;
    }
    if (live !== null) {
      const source = texts.get(live);
      if (source === undefined) throw new Error(`input ${live} has no text`);
      ran.live = await runInput(instance, live, source, wasm);
    }
    return ran;
  };

  return {
    run,
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
