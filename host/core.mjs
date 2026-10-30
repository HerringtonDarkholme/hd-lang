// The platform-neutral core of the JS host (V8 in Node, and browsers): the
// import object of the ABI in `hd_host_abi` (crates/hd_run_wasmtime/DESIGN.md §17.2,
// §17.10), built from the method descriptors and codecs generated from the
// ABI model (gen/abi.mts, over wire.mts's encoding of §17.4); the providers
// that need nothing from the platform (`Console`, `Clock`, `Random`,
// `Args`, `rt`, `prim`, and the test and property runners' hooks); and the
// entry driver of crates/hd_wasm_linear/DESIGN.md §14.4. It uses web-standard APIs
// only. A platform layer (node.mjs, web.mjs) adds the providers that touch
// the machine by passing `layer` to `createCoreHost`.
//
// - A provider is a host trait's methods as JS functions by name: each
//   takes its arguments and returns its result as plain values (wire.mts
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
//   (journal.mts), as `hd_run_wasmtime`'s session does.
import { shortest, fixed, lower, upper, parse } from "./float.mts";
import { WireHttpError as HttpError, METHODS } from "./gen/abi.mts";
import { Dec, decodeUtf8, Enc, Err, HostContract, Ok, str, variant } from "./wire.mts";
export {
  CANCEL_STATUS,
  entryCancel,
  factHead,
  PROGRAM_TIME_LIMIT,
  reportIn,
} from "./run-support.mts";
import { CANCEL_STATUS, factHead, PROGRAM_TIME_LIMIT, reportIn } from "./run-support.mts";

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
// each trait maps to `true`, unrestricted, `false`, a total deny, or its
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
    return l === null || l === true ? true : l === false ? false : l.some(covers);
  };
  return {
    allows: (key) => limit(key) !== false,
    limited: (key) => limit(key) !== null && limit(key) !== true,
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
        if (st.readyOnly)
          throw new HostContract("a synchronous host wait is pending in a synchronous step driver");
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
    hash_seed: () => new DataView(st.random(8).buffer).getBigInt64(0, true),
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
export function clearOperations(st) {
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
// (gen/abi.mts `METHODS`) shape them. `io` reads and writes the exchange
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

// `hd:Console`: one text chunk per `write_sync`; `write_error_sync` writes
// its text to standard error, after the held output
// (module.console.write-error-sync.raw). Each write returns once its text
// is written (module.console.write-sync.never-suspends): a captured sink
// takes it at once, and a direct sink (output.mjs) blocks until the
// stream has taken every byte.
function consoleProvider({ st, sink, flush, grant }) {
  const denied = (stream) => Err({ tag: "NotGranted", stream: { tag: stream } });
  return {
    write_sync: (text) =>
      !grant.allows("Console")
        ? denied("Stdout")
        : sink.write
          ? (flush(), sink.write(1, text))
          : (st.out.push(text), Ok(undefined)),
    write_error_sync: (text) =>
      !grant.allows("Console")
        ? denied("Stderr")
        : sink.write
          ? (flush(), sink.write(2, text))
          : (flush(), sink.err(text), Ok(undefined)),
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
// A platform layer may supply its own (`layer.randomBytes`): on Node the
// first use of `crypto` loads its module, about 3 ms of a run.
function randomBytes(count) {
  const out = new Uint8Array(count);
  for (let at = 0; at < count; at += 65536) crypto.getRandomValues(out.subarray(at, at + 65536));
  return out;
}

// The operating system's random source (cli.host.default-profile).
// `Random` has no scope entries: a grant of `false` refuses the whole
// program at startup (cli.cap.total.refuse).
function randomProvider(random) {
  return {
    next_u64: () => new DataView(random(8).buffer).getBigInt64(0, true),
    fill: (count) => random(count >>> 0),
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

// A wire request's transport settings (std-http.settings.*): `dnsServers`
// is `null` or the literal addresses, `tlsRoots` the extra PEM roots,
// `tlsIdentity` `null` or `{ certificateChain, privateKey }`, and each
// timeout `null` or whole milliseconds.
export const httpSettings = (r) => {
  const ms = (duration) => (duration == null ? null : Number(duration.millis));
  return {
    dnsServers: r.dns_servers ?? null,
    tlsRoots: r.tls_roots ?? [],
    tlsIdentity:
      r.tls_identity == null
        ? null
        : {
            certificateChain: r.tls_identity.certificate_chain,
            privateKey: r.tls_identity.private_key,
          },
    connectTimeout: ms(r.connect_timeout),
    tlsHandshakeTimeout: ms(r.tls_handshake_timeout),
    responseHeaderTimeout: ms(r.response_header_timeout),
    bodyReadTimeout: ms(r.body_read_timeout),
  };
};

// The stable name of each setting a request changes from the host default,
// in a fixed order (std-http.settings.unsupported-name).
export const changedSettings = (settings) =>
  [
    ["resolver", settings?.dnsServers != null],
    ["tls.roots", (settings?.tlsRoots?.length ?? 0) > 0],
    ["tls.identity", settings?.tlsIdentity != null],
    ["connect_timeout", settings?.connectTimeout != null],
    ["tls_handshake_timeout", settings?.tlsHandshakeTimeout != null],
    ["response_header_timeout", settings?.responseHeaderTimeout != null],
    ["body_read_timeout", settings?.bodyReadTimeout != null],
  ]
    .filter(([, changed]) => changed)
    .map(([name]) => name);

// The preflight refusal of a host that honours only the settings named in
// `supported`: `{ error: "UnsupportedSetting", text }` for the first other
// changed setting, or `null` (std-http.settings.preflight).
export const unsupportedSetting = (settings, supported = []) => {
  const name = changedSettings(settings).find((each) => !supported.includes(each));
  return name === undefined ? null : { error: "UnsupportedSetting", text: name };
};

// `hd:Http` over `send`, a client that takes `{ method, url, headers,
// body, redirects, settings }` and answers
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
        settings: httpSettings(r),
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

function initialState(env, layer) {
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
function createStepDriver(instance, init, poll, { st, prime, complete, flush }) {
  if (st.closed) throw new HostContract("instance is closed");
  if (st.drivingMode !== null) throw new HostContract("the host already has a driver");
  const ex = instance.exports;
  if (![init, poll, "hd.wake"].every((name) => typeof ex[name] === "function"))
    throw new HostContract("the instance is missing root driver exports");
  st.drivingMode = "sync";
  st.readyOnly = true;
  prime(instance);
  let started = false,
    busy = false,
    failed = false,
    status = -1;
  return {
    step() {
      if (st.closed) throw new HostContract("instance is closed");
      if (busy) throw new HostContract("reentrant instance poll");
      if (failed) throw new HostContract("the driver has failed");
      if (started && status !== -1) return { status, wakes: 0 };
      busy = true;
      try {
        try {
          if (!started) {
            started = true;
            ex[init]();
          }
          status = ex[poll](0);
          if (typeof status !== "number" || (status | 0) !== status)
            throw new HostContract("root poll returned an invalid status");
          const wakes = status === -1 ? complete() : 0;
          if (wakes) ex["hd.wake"](wakes);
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
  const st = initialState(env, layer);
  const reportsCycles =
    env.module instanceof WebAssembly.Module &&
    WebAssembly.Module.customSections(env.module, "hd.cycles").length === 1;
  const writeCycleReport = async (instance) => {
    if (!reportsCycles) return;
    const { cycleReport } = await import("./checked-report.mts");
    const report = cycleReport(env.module, instance);
    if (report !== null) sink.err(report);
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
    "hd:Random": randomProvider(st.random),
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
  const io = {
    view,
    text,
    put,
    pending,
    finished,
    prepare: pendingOperation,
    prepared: (value) => (value instanceof Later ? value : null),
  };
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
    // A narrow policy view for SDK-owned outgoing Http handles. Reuse the
    // core's resolved grant, never a provider's reconstructed policy table.
    const http = Object.freeze({
      coversHost: (host, port) => grant.coversHost("Http", host, port),
      isAsciiUrl,
      writtenHost,
    });
    const console = Object.freeze({ allows: () => grant.allows("Console") });
    if (typeof env.createPackageRuntime !== "function")
      throw new HostContract("package host runtime was not supplied");
    packageRuntime = env.createPackageRuntime({ ...io, http, console }, env);
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
        if (typeof env.readPackageContract !== "function")
          throw new HostContract("package host contract reader was not supplied");
        const required = env.readPackageContract(new Uint8Array(sections[0]));
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

  const createDriver = (instance, init = "hd.init", poll = "hd.poll") =>
    createStepDriver(instance, init, poll, { st, prime, complete, flush });

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
  const continueRun = async (instance, ex, poll, status, driver, cancel, firstError = null) => {
    let trapped = false;
    const ended = false;
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
      if (firstError !== null) throw firstError;
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
        await writeCycleReport(instance);
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
      } else if (env.isReplayDivergence?.(e)) {
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
    await writeCycleReport(instance);
    return { status, trapped, ended, frames, panic, cancelled: cancelled && !trapped };
  };

  // Keep the common no-suspension path apart from the full async driver:
  // a fresh Node process can compile this small wrapper without compiling
  // cancellation, waits, traps and wake delivery that it never reaches.
  const run = (instance, init, poll, driver = null, cancel = null) => {
    if (st.drivingMode === "sync")
      return Promise.reject(new HostContract("the host already has a synchronous driver"));
    st.drivingMode = "async";
    try {
      prime(instance);
    } catch (error) {
      return Promise.reject(error);
    }
    const ex = instance.exports;
    let status;
    try {
      ex[init]();
      driver?.poll();
      status = ex[poll](0);
    } catch (error) {
      return continueRun(instance, ex, poll, 0, driver, cancel, error);
    }
    if (status === -1 || status === CANCELLED_END || status === CANCELLED_END - 1)
      return continueRun(instance, ex, poll, status, driver, cancel);
    try {
      flush();
      if (overran()) return continueRun(instance, ex, poll, status, driver, cancel);
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
  const close = () => {
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
  const leaks = () => platform?.leaks?.() ?? [];

  // What a REPL session reaches past the import object: the instance
  // state, the result writer, the deadlock report and the output.
  const core = { st, put, deadlock, flush };

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
