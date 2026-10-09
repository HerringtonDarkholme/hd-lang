// `hd test` on V8 (Node): the test program is compiled once; each case
// runs in its own fresh instance (module.testing.instance), its module's
// init export first, then its test export polled to completion
// (engines-and-test-runner.md §19.3). `argv[3]` is the run's
// configuration, a `.json` file that `hd test` writes: the program's
// `Args.program` and `args` (Test Environments), the package's executables
// and tasks that its `Process` provider starts by name (`programs`,
// cli.test.process), then per case its `test` and `init` export indices,
// its registration function's name as `kind`, its own `tempDir`, the base `seed` of a property test, its `grants`, its
// `snapshot` files, and its `regression` file. Standard input is closed
// (cli.test.env.stdin).
//
// This file is the test runner's side of `std.testing`
// (spec/std/testing.md#runner-capabilities). A case's std body tells the
// runner what the case is:
//
// - `TestRunner.row(count)` in an `it_each` case: `count` rows. Row 0
//   ran in the first instance; each later row runs in an instance of its
//   own, and every row is reported (std-testing.it-each, .it-each.name).
// - `PropertyRunner.start(...)`: a property case. The runner runs it once
//   per case, each in a fresh instance: the examples first, then a saved
//   regression stream, then `cases` generated cases from consecutive
//   seeds (std-testing.prop.examples, .prop.regression-replay,
//   .runner.case.seed-step). A discarded case does not count, and more
//   than 10 times `cases` discards fail the property
//   (std-testing.prop.discard, .prop.discard-limit). A failing case's
//   choice stream is shrunk, at most `shrink` attempts, its shrunk stream
//   saved, and the shrunk input's `Debug` text reported
//   (std-testing.it-prop, .prop.regression-file, .prop.report).
//
// Each run of a body gets a temporary directory of its own: the case's
// `tempDir` for the first, a numbered directory inside it for each later
// one, removed once that run ends (std-testing.temp-dir.per-run).
//
// A case's `timeout` (`TestRunner.report_timeout`, std-testing.runner.timeout)
// fails a body that runs longer: the host checks it at the end of the run
// and cuts a wait at it, and since a Wasm loop never returns to the host,
// the cases run in a worker thread that this file's main thread
// supervises. When a body is still running a little past its deadline,
// the supervisor stops the worker, reports the run as a `time-limit`
// panic, and starts a new worker at the next row or case.
//
// One JSON line per result goes to standard output: its case's `test`
// index, then `status`, `trapped`, its captured output and its time. A row
// line also has `row` and `rows`; an empty table is one line with
// `"rows":0` and no result. A property's line adds the shrunk input's text
// as `input` when it fails.
import { mkdirSync, readFileSync, rmSync, writeFileSync, existsSync, writeSync } from "node:fs";
import { dirname, join } from "node:path";
import { isMainThread, parentPort, Worker, workerData } from "node:worker_threads";
import { createHost, EndCase, timeLimitReport } from "./core.mjs";

// How long past a deadline the supervisor waits for a body to end on its
// own before it stops the worker.
const GRACE_MS = 50;

// The wall clock, comparable across threads.
const now = () => performance.timeOrigin + performance.now();
// Without a configuration file, `argv[3]` lists the cases alone, as
// `test:init:kind` (export indices and the registration function)
// separated by commas, which run with no arguments, no temporary
// directory and no grant limit.
const plain = (list) => ({
  program: "",
  args: [],
  cases: list
    .split(",")
    .filter((c) => c !== "")
    .map((c) => {
      const [test, init, kind] = c.split(":");
      return { test: Number(test), init: Number(init), kind: kind ?? "it", tempDir: null, seed: null, grants: {} };
    }),
});
function configOf(spec) {
  return spec.endsWith(".json") ? JSON.parse(readFileSync(spec, "utf8")) : plain(spec);
}

// Runs the cases in a worker from `start` (a case index, and for a table
// resumed after a stopped row, its next row and its row count), restarting
// it after each run it stops for overrunning its timeout.
async function supervise(module, config) {
  let start = { case: 0, row: 0, rows: 0 };
  while (start.case < config.cases.length) {
    start = await new Promise((next, fail) => {
      const worker = new Worker(new URL(import.meta.url), { workerData: { module, config, start } });
      // What the worker runs: its case, the row of a table and its count,
      // and the current run's timeout.
      let at = { ci: start.case, row: start.row > 0 ? start.row : null, rows: start.rows || null };
      let timer = null;
      let stopped = false;
      const clear = () => {
        if (timer !== null) clearTimeout(timer);
        timer = null;
      };
      const stop = (ms) => {
        stopped = true;
        worker.terminate();
        const c = config.cases[at.ci];
        const line = {
          test: c.test,
          status: 3,
          trapped: true,
          stdout: "",
          stderr: timeLimitReport(ms),
          us: Math.round((ms + GRACE_MS) * 1000),
        };
        if (at.row !== null && at.rows !== null) {
          writeSync(1, JSON.stringify({ test: c.test, row: at.row, rows: at.rows, ...line }) + "\n");
          next(at.row + 1 < at.rows ? { case: at.ci, row: at.row + 1, rows: at.rows } : { case: at.ci + 1, row: 0, rows: 0 });
        } else {
          writeSync(1, JSON.stringify(line) + "\n");
          next({ case: at.ci + 1, row: 0, rows: 0 });
        }
      };
      worker.on("message", (m) => {
        if (stopped) return;
        if (m.type === "line") {
          writeSync(1, m.text);
        } else if (m.type === "case") {
          clear();
          at = { ci: m.ci, row: null, rows: null };
        } else if (m.type === "rows") {
          at.rows = m.rows;
          if (at.row === null) at.row = 0;
        } else if (m.type === "row") {
          clear();
          at.row = m.row;
        } else if (m.type === "deadline") {
          clear();
          timer = setTimeout(() => stop(m.ms), Math.max(0, m.at - now()) + GRACE_MS);
        } else if (m.type === "done") {
          clear();
        }
      });
      worker.on("error", fail);
      worker.on("exit", () => {
        clear();
        if (!stopped) next({ case: config.cases.length, row: 0, rows: 0 });
      });
    });
  }
}

// The worker's compiled module and configuration.
let module = null;
let config = null;

// The message of the discard panic (std-testing.runner.discard-panic).
const DISCARD = "std.testing: case discarded";

// One run of a case's body in a fresh instance: its `k`th, with the
// runner hooks `runner` (null for none).
async function runOnce(c, k, runner) {
  let stdout = "";
  let stderr = "";
  const dir = c.tempDir && k > 0 ? join(c.tempDir, String(k)) : c.tempDir;
  const host = createHost(
    {
      out: (text) => {
        stdout += text;
      },
      err: (text) => {
        stderr += text;
      },
    },
    {
      args: config.args,
      program: config.program,
      programs: config.programs ?? null,
      stdin: null,
      grants: c.grants,
      tempDir: dir,
      seed: c.seed,
      snapshot: c.snapshot ?? null,
      runner,
    },
  );
  const t0 = performance.now();
  const instance = await WebAssembly.instantiate(module, host.imports);
  const { status, trapped, ended } = await host.run(instance, `hd.init.${c.init}`, `hd.test.${c.test}`);
  parentPort.postMessage({ type: "done" });
  host.close();
  if (dir) rmSync(dir, { recursive: true, force: true });
  const us = Math.round((performance.now() - t0) * 1000);
  return { status, trapped, ended, stdout, stderr, us };
}

const emit = (line) => parentPort.postMessage({ type: "line", text: JSON.stringify(line) + "\n" });

const result = (c, r, extra = {}) => ({
  test: c.test,
  ...extra,
  status: r.status,
  trapped: r.trapped,
  stdout: r.stdout,
  stderr: r.stderr,
  us: r.us,
});

// What one run of a property case did: the draws it recorded, its input's
// text once shown, and what it was asked to run.
class Run {
  constructor() {
    this.draws = [];
    this.shown = null;
    this.plan = null;
  }
}

// The runner state of one configured case, and the hooks that each of its
// instances calls.
class Case {
  constructor(c) {
    this.c = c;
    this.kind = null;
    this.row = 0;
    this.rows = 0;
    this.prop = null;
  }

  hooks(run) {
    return {
      // Only an `it_each` case runs rows; any other case runs row 0.
      row: (count) => {
        if (this.kind === null && this.c.kind === "it_each") {
          this.kind = "rows";
          this.rows = count;
          parentPort.postMessage({ type: "rows", rows: count });
        }
        return this.row;
      },
      // The supervisor stops a run still going a little past this.
      timeout: (ms) => parentPort.postMessage({ type: "deadline", at: now() + ms, ms }),
      slugSuffix: () => (this.kind === "rows" ? `.${this.row}` : ""),
      start: (cases, shrink, examples) => {
        if (this.kind === null) {
          this.kind = "prop";
          this.prop = new Property(this.c, cases, shrink, examples);
        }
        const plan = this.prop.next();
        if (plan === null) throw new EndCase();
        run.plan = plan;
        return plan;
      },
      record: (value) => run.draws.push(value),
      show: (text) => {
        if (run.shown === null) run.shown = text;
      },
    };
  }
}

// The cases of one property test and its counts
// (spec/std/testing.md#runner-capabilities).
class Property {
  constructor(c, cases, shrink, examples) {
    this.cases = cases;
    this.shrink = shrink;
    this.examples = examples;
    this.example = 0;
    // The base seed (cli.test.seed); each case's is one more than the
    // case's before it (std-testing.runner.case.seed-step).
    this.seed = BigInt(c.seed ?? 0);
    this.size = 0;
    this.passed = 0;
    this.discarded = 0;
    this.regression = c.regression ?? null;
    this.replays = [];
    if (this.regression && existsSync(this.regression.file)) {
      const text = readFileSync(this.regression.file, "utf8");
      const stream = text
        .split("\n")
        .map((l) => l.trim())
        .filter((l) => l !== "")
        .map((l) => BigInt(l));
      this.replays.push(stream);
    }
    // What the next instance runs, when the runner chose it.
    this.pending = null;
  }

  // The case the next `start` returns, or null when nothing is left.
  next() {
    const seed = this.seed;
    this.seed += 1n;
    if (this.pending !== null) {
      const p = this.pending;
      this.pending = null;
      return { example: null, seed, size: p.size, replay: p.replay, kind: p.kind };
    }
    if (this.example < this.examples) {
      return { example: this.example++, seed, size: 0, replay: [], kind: "example" };
    }
    if (this.replays.length > 0) {
      return { example: null, seed, size: this.size, replay: this.replays.shift(), kind: "saved" };
    }
    if (this.passed < this.cases) {
      return { example: null, seed, size: this.size, replay: [], kind: "fresh" };
    }
    return null;
  }

  // Whether anything is left to run.
  more() {
    return this.example < this.examples || this.replays.length > 0 || this.passed < this.cases;
  }
}

// The panic message of a run's report, if it panicked.
const panicMessage = (r) => {
  if (!r.trapped) return null;
  const line = r.stderr
    .split("\n")
    .reverse()
    .find((l) => l.startsWith("panic: "));
  if (line === undefined) return null;
  const rest = line.slice("panic: ".length);
  return rest.startsWith("explicit-panic: ") ? rest.slice("explicit-panic: ".length) : rest;
};

// What a run of a property case came to: a discard is the discard panic
// before `show` (std-testing.runner.discard-read); the same panic after it
// is a failure (std-testing.runner.discard-after-show).
const outcome = (run, r) => {
  if (r.ended) return "ended";
  if (r.trapped) return run.shown === null && panicMessage(r) === DISCARD ? "discard" : "fail";
  return r.status === 0 ? "pass" : "fail";
};

// `a` is simpler than `b`: shorter, or as long and smaller draw by draw.
const simpler = (a, b) => {
  if (a.length !== b.length) return a.length < b.length;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return a[i] < b[i];
  }
  return false;
};

// Shrinks a failing case's choice stream (std-testing.it-prop): deletes
// blocks of draws, zeroes them, and moves each draw toward 0 by halving
// the distance, keeping each attempt that still fails with a simpler
// stream. Each attempt is a run of its own; there are at most `budget`.
async function shrink(failing, attempt, budget) {
  let best = failing;
  let left = budget;
  const tryStream = async (stream) => {
    if (left <= 0) return false;
    left -= 1;
    const got = await attempt(stream);
    if (got !== null && simpler(got.run.draws, best.run.draws)) {
      best = got;
      return true;
    }
    return false;
  };
  let improved = true;
  while (improved && left > 0) {
    improved = false;
    for (const k of [8, 4, 2, 1]) {
      for (let i = 0; i + k <= best.run.draws.length && left > 0; ) {
        const s = best.run.draws;
        if (await tryStream([...s.slice(0, i), ...s.slice(i + k)])) improved = true;
        else i += 1;
      }
    }
    for (const k of [8, 4, 2, 1]) {
      for (let i = 0; i + k <= best.run.draws.length && left > 0; i++) {
        const s = best.run.draws;
        if (s.slice(i, i + k).every((v) => v === 0n)) continue;
        const z = [...s];
        z.fill(0n, i, i + k);
        if (await tryStream(z)) improved = true;
      }
    }
    for (let i = 0; i < best.run.draws.length && left > 0; i++) {
      let lo = 0n;
      while (left > 0) {
        const s = best.run.draws;
        if (i >= s.length || s[i] <= lo) break;
        const mid = lo + (s[i] - lo) / 2n;
        const t = [...s];
        t[i] = mid;
        if (await tryStream(t)) improved = true;
        else lo = mid + 1n;
      }
    }
  }
  return best;
}

// Runs a property case to its end and reports it in one line.
async function runProperty(c, ctl, first, firstResult) {
  const p = ctl.prop;
  let k = 1;
  let total = firstResult.us;
  const again = async (pending) => {
    p.pending = pending;
    const run = new Run();
    const r = await runOnce(c, k++, ctl.hooks(run));
    total += r.us;
    return { run, r, outcome: outcome(run, r) };
  };
  let current = { run: first, r: firstResult, outcome: outcome(first, firstResult) };
  for (;;) {
    const { run, outcome: o } = current;
    if (o === "ended") break;
    if (o === "fail") {
      // An example is reported as it is; a generated or saved case is
      // shrunk first.
      let failing = current;
      if (run.plan && run.plan.kind !== "example") {
        const size = run.plan.size;
        const attempt = async (stream) => {
          const got = await again({ replay: stream, size, kind: "shrink" });
          return got.outcome === "fail" ? got : null;
        };
        failing = await shrink(current, attempt, p.shrink);
        if (p.regression) {
          mkdirSync(dirname(p.regression.file), { recursive: true });
          writeFileSync(p.regression.file, failing.run.draws.map((v) => `${v}\n`).join(""));
        }
      }
      const r = { ...failing.r, us: total };
      emit(result(c, r, { input: failing.run.shown ?? "" }));
      return;
    }
    if (o === "discard") {
      p.discarded += 1;
      if (p.discarded > 10 * p.cases) {
        emit(
          result(c, {
            status: 1,
            trapped: false,
            stdout: "",
            stderr: `the property discarded ${p.discarded} cases, more than 10 times its ${p.cases} cases\n`,
            us: total,
          }),
        );
        return;
      }
    } else if (o === "pass" && run.plan && run.plan.kind === "fresh") {
      p.passed += 1;
      p.size += 1;
    }
    if (!p.more()) break;
    current = await again(null);
  }
  emit(result(c, { ...firstResult, status: 0, trapped: false, us: total }));
}

// Runs the cases from `start` and reports each result.
async function runCases(m, cfg, start) {
  module = m;
  config = cfg;
  for (let ci = start.case; ci < config.cases.length; ci++) {
    const c = config.cases[ci];
    parentPort.postMessage({ type: "case", ci });
    const ctl = new Case(c);
    // Every row runs; one failing row hides no other.
    const rowsFrom = async (from, rows) => {
      for (let row = from; row < rows; row++) {
        ctl.row = row;
        parentPort.postMessage({ type: "row", row });
        emit(result(c, await runOnce(c, row, ctl.hooks(new Run())), { row, rows }));
      }
    };
    // A table resumed after a row its supervisor stopped.
    if (ci === start.case && start.row > 0) {
      ctl.kind = "rows";
      ctl.rows = start.rows;
      await rowsFrom(start.row, start.rows);
      continue;
    }
    const first = new Run();
    const r = await runOnce(c, 0, ctl.hooks(first));
    if (ctl.kind === "rows") {
      const rows = ctl.rows;
      if (rows === 0) {
        emit({ test: c.test, row: 0, rows: 0 });
        continue;
      }
      emit(result(c, r, { row: 0, rows }));
      await rowsFrom(1, rows);
    } else if (ctl.kind === "prop") {
      await runProperty(c, ctl, first, r);
    } else {
      emit(result(c, r));
    }
  }
}

if (isMainThread) {
  const compiled = await WebAssembly.compile(readFileSync(process.argv[2]));
  await supervise(compiled, configOf(process.argv[3] ?? ""));
} else {
  await runCases(workerData.module, workerData.config, workerData.start);
}
