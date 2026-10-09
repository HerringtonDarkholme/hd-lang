// The JS host of `hd run` and `hd test` on V8 (Node): the import object of
// the ABI in `hd_host_abi` (runtime-and-host.md §17.2, §17.10) for what the
// compiler emits today, and the entry driver of suspension.md §14.4.
//
// - Host operations that may wait start pending: `.start` returns status
//   1 and a handle. `hd:Console` `write_line` completes at once (the line
//   is written at completion); `hd:Clock` `sleep` completes when its
//   duration has passed. `.finish(h)` writes the result into the exchange
//   buffer; `hd:rt` `abort(h)` drops a pending operation.
// - `hd:rt` `block()` waits synchronously for at least one completion,
//   writes the completed handles to the exchange buffer and returns their
//   count; `stderr(len)` writes the buffer's first `len` bytes to standard
//   error.
// - The driver: an init export, then a poll export until it is not -1;
//   while it is pending, the host waits for completions in its event loop
//   and hands them over with `hd.wake(n)`. A pending root with nothing
//   left to wait for is a deadlock.
// A trap is a panic: the run's status is 3 (never 101,
// module.profile.panic-status).

import { mkdirSync, realpathSync } from "node:fs";
import { dirname, basename, join, resolve } from "node:path";

export class Deadlock extends Error {}

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
    // cli.cap.scope.env, .process, .sys.
    coversName: (key, name) =>
      check(key, (e) => (key === "Env" && e.endsWith("*") ? name.startsWith(e.slice(0, -1)) : e === name)),
  };
}

// One host per instance: `sink.out(text)` and `sink.err(text)` receive the
// program's standard output and standard error. `env` holds what the
// providers hand out once the emitter lowers their methods: `args` and
// `program` for `Args`, `grants` for the scope checks, and in a test case
// its `tempDir` (made on the first `tempDir()` call, which
// `TestRunner.temp_dir` makes) and the base `seed` of a property test
// (Test Environments, cli.test.seed).
export function createHost(sink, env = {}) {
  const args = env.args ?? [];
  const program = env.program ?? "";
  const grant = createGrant(env.grants ?? {});
  const seed = env.seed ?? null;
  let tempMade = false;
  const tempDir = () => {
    if (!env.tempDir) return "";
    if (!tempMade) mkdirSync(env.tempDir, { recursive: true });
    tempMade = true;
    return env.tempDir;
  };
  const out = [];
  let memory = null;
  let reported = false;
  const view = (len) => new Uint8Array(memory.buffer, 0, len);
  const flush = () => {
    if (out.length) {
      sink.out(out.join(""));
      out.length = 0;
    }
  };

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
          Atomics.wait(sleeper, 0, 0, Math.max(0, d - performance.now()));
        }
      },
      abort: (h) => {
        ops.delete(h);
      },
    },
    "hd:Console": {
      "write_line.start": (len) => {
        const text = Buffer.from(view(len)).toString("utf8");
        return pending(performance.now(), () => out.push(text + "\n"));
      },
      "write_line.finish": (h) => {
        finish(h);
        view(1)[0] = 0;
        return 1;
      },
    },
    "hd:Clock": {
      "sleep.start": (ms) => pending(performance.now() + Number(ms), null),
      "sleep.finish": (h) => {
        finish(h);
        return 0;
      },
    },
  };

  // Runs `init`, then polls `poll` to completion. Returns the status (3
  // for a trap) and whether the instance trapped.
  const run = async (instance, init, poll) => {
    memory = instance.exports["hd.x"] ?? null;
    const ex = instance.exports;
    let status = 0;
    let trapped = false;
    try {
      ex[init]();
      status = ex[poll]();
      while (status === -1) {
        const n = complete();
        if (n === 0) {
          const d = nextDeadline();
          if (d === null) deadlock();
          await new Promise((r) => setTimeout(r, Math.max(0, d - performance.now())));
          continue;
        }
        ex["hd.wake"](n);
        status = ex[poll]();
      }
    } catch (e) {
      flush();
      if (!(e instanceof WebAssembly.RuntimeError) && !(e instanceof Deadlock)) throw e;
      // A panic stub wrote its report; any other trap is an internal error.
      if (!reported) sink.err(`internal error: ${e.message}\n`);
      status = 3;
      trapped = true;
    }
    flush();
    return { status, trapped };
  };

  return { imports, run, args, program, grant, seed, tempDir };
}
