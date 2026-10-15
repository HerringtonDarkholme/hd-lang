// The test runner's case loop, shared by `hd test` on Node (test.mjs) and
// the playground (website/playground/src/rust-runner.ts). It uses
// web-standard APIs only; the caller hands it what touches the platform.
// The test program is compiled once; each case runs in its own fresh
// instance (module.testing.instance), its module's init export first, then
// its test export polled to completion (docs/compiler/engines-and-test-runner.md §19.3).
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
// One record per result goes to `emit`: its case's `test` index, then
// `status`, `trapped`, its captured output and its time. A trapped run's
// record adds its Wasm `frames`. A row's record also has `row` and `rows`;
// an empty table is one record with `rows: 0` and no result. A property's
// record adds the shrunk input's text as `input` when it fails.
//
// `judge` and `panicOf` are what a result comes to for a runner that
// reads these records itself (the playground; `hd test` judges them in
// test_cmd.rs).

import { EndCase } from "./core.mjs";

// The wall clock, comparable across threads.
const now = () => performance.timeOrigin + performance.now();

// The message of the discard panic (std-testing.runner.discard-panic).
const DISCARD = "std.testing: case discarded";

// What one run of the cases needs from its platform:
//
// - `module`: the compiled test program; `config`: the run's
//   configuration (`program`, `args`, `programs` and per case its `test`
//   and `init` export indices, `kind`, `tempDir`, `seed`, `grants`,
//   `snapshot` and `regression`; test.mjs has the format).
// - `createHost(sink, env)`: a host for one instance.
// - `files`: `join(dir, name)`, `read(path)` (the text, or null when there
//   is no such file), `write(path, text)` (making its directory) and
//   `remove(dir)`, for the temporary directories and regression files.
// - `notify(message)`: what the case loop is doing, for a supervisor that
//   stops a body running past its timeout: `case` (`ci`), `rows`
//   (`rows`), `row` (`row`), `limit` (`at`, the end of a `--time-limit`
//   for the case or row that began), `deadline` (`at`, `ms`) and `done`.
// - `emit(record)`: one result.
class Runner {
  constructor({ module, config, createHost, files, notify, emit }) {
    this.module = module;
    this.config = config;
    this.createHost = createHost;
    this.files = files;
    this.notify = notify;
    this.emit = emit;
    // When the `--time-limit` of the case or row now running ends
    // (`config.timeLimitMs`; cli.limit.test), or null.
    this.deadline = null;
  }

  // Starts the time limit of a case, or of the next row of a table; the
  // runs of a property case share the case's.
  begin() {
    const ms = this.config.timeLimitMs ?? null;
    if (ms === null) return;
    this.deadline = now() + ms;
    this.notify({ type: "limit", at: this.deadline });
  }

  // One run of a case's body in a fresh instance: its `k`th, with the
  // runner hooks `runner` (null for none).
  async runOnce(c, k, runner) {
    let stdout = "";
    let stderr = "";
    const dir = c.tempDir && k > 0 ? this.files.join(c.tempDir, String(k)) : c.tempDir;
    const host = this.createHost(
      {
        out: (text) => {
          stdout += text;
        },
        err: (text) => {
          stderr += text;
        },
      },
      {
        args: this.config.args,
        program: this.config.program,
        programs: this.config.programs ?? null,
        stdin: null,
        grants: c.grants,
        tempDir: dir,
        seed: c.seed,
        snapshot: c.snapshot ?? null,
        runner,
        deadline: this.deadline,
      },
    );
    const t0 = performance.now();
    const instance = await WebAssembly.instantiate(this.module, host.imports);
    const { status, trapped, ended, frames } = await host.run(
      instance,
      `hd.init.${c.init}`,
      `hd.test.${c.test}`,
    );
    this.notify({ type: "done" });
    host.close();
    if (dir) this.files.remove(dir);
    const us = Math.round((performance.now() - t0) * 1000);
    return { status, trapped, ended, stdout, stderr, us, frames };
  }
}

// A trapped run's record adds its Wasm frames, [function, offset] pairs
// innermost first, when the trap had any.
const result = (c, r, extra = {}) => ({
  test: c.test,
  ...extra,
  status: r.status,
  trapped: r.trapped,
  stdout: r.stdout,
  stderr: r.stderr,
  us: r.us,
  ...(r.trapped && r.frames?.length ? { frames: r.frames } : {}),
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
  constructor(runner, c) {
    this.runner = runner;
    this.c = c;
    this.kind = null;
    this.row = 0;
    this.rows = 0;
    this.prop = null;
  }

  hooks(run) {
    const notify = this.runner.notify;
    return {
      // Only an `it_each` case runs rows; any other case runs row 0.
      row: (count) => {
        if (this.kind === null && this.c.kind === "it_each") {
          this.kind = "rows";
          this.rows = count;
          notify({ type: "rows", rows: count });
        }
        return this.row;
      },
      // A supervisor stops a run still going a little past this.
      timeout: (ms) => notify({ type: "deadline", at: now() + ms, ms }),
      slugSuffix: () => (this.kind === "rows" ? `.${this.row}` : ""),
      start: (cases, shrink, examples) => {
        if (this.kind === null) {
          this.kind = "prop";
          this.prop = new Property(this.c, this.runner.files, cases, shrink, examples);
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
  constructor(c, files, cases, shrink, examples) {
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
    const text = this.regression ? files.read(this.regression.file) : null;
    if (text !== null) {
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
      for (let i = 0; i + k <= best.run.draws.length && left > 0;) {
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

// Runs a property case to its end and reports it in one record.
async function runProperty(runner, c, ctl, first, firstResult) {
  const p = ctl.prop;
  let k = 1;
  let total = firstResult.us;
  const again = async (pending) => {
    p.pending = pending;
    const run = new Run();
    const r = await runner.runOnce(c, k++, ctl.hooks(run));
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
          runner.files.write(p.regression.file, failing.run.draws.map((v) => `${v}\n`).join(""));
        }
      }
      const r = { ...failing.r, us: total };
      runner.emit(result(c, r, { input: failing.run.shown ?? "" }));
      return;
    }
    if (o === "discard") {
      p.discarded += 1;
      if (p.discarded > 10 * p.cases) {
        runner.emit(
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
  runner.emit(result(c, { ...firstResult, status: 0, trapped: false, us: total }));
}

// Runs the cases of `options.config` from `start` (a case index, and for a
// table resumed after a stopped row, its next row and its row count) and
// emits each result; `options` is described at `Runner`.
export async function runCases(options, start = { case: 0, row: 0, rows: 0 }) {
  const runner = new Runner(options);
  const { cases } = runner.config;
  for (let ci = start.case; ci < cases.length; ci++) {
    const c = cases[ci];
    runner.notify({ type: "case", ci });
    runner.begin();
    const ctl = new Case(runner, c);
    // Every row runs; one failing row hides no other.
    const rowsFrom = async (from, rows) => {
      for (let row = from; row < rows; row++) {
        ctl.row = row;
        runner.notify({ type: "row", row });
        runner.begin();
        runner.emit(result(c, await runner.runOnce(c, row, ctl.hooks(new Run())), { row, rows }));
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
    const r = await runner.runOnce(c, 0, ctl.hooks(first));
    if (ctl.kind === "rows") {
      const rows = ctl.rows;
      if (rows === 0) {
        runner.emit({ test: c.test, row: 0, rows: 0 });
        continue;
      }
      runner.emit(result(c, r, { row: 0, rows }));
      await rowsFrom(1, rows);
    } else if (ctl.kind === "prop") {
      await runProperty(runner, c, ctl, first, r);
    } else {
      runner.emit(result(c, r));
    }
  }
}

// The category and message of a trapped run's panic report (`panic:
// <category>: <message>`, the last such line of its standard error), and
// its other lines, the `dbg` output (cli.dbg.test). A report with no
// `panic:` line is the runner's own failure, `internal`.
export function panicOf(stderr) {
  const lines = stderr.split("\n").filter((l) => l !== "");
  const at = lines.findLastIndex((l) => l.startsWith("panic: "));
  if (at < 0) return { category: "internal", message: stderr.trim(), debug: [] };
  const rest = lines[at].slice("panic: ".length);
  const colon = rest.indexOf(": ");
  const head = colon < 0 ? rest : rest.slice(0, colon);
  const message = colon < 0 ? "" : rest.slice(colon + 2);
  const debug = lines.filter((_, i) => i !== at);
  // The host's `deadlock` report is the `suspension-deadlock` panic.
  return { category: head === "deadlock" ? "suspension-deadlock" : head, message, debug };
}

// What one result record of the case `expectPanic` (its `expect_panic`
// option, or null) comes to (module.testing.pass, module.testing.fail,
// module.testing.expect-panic-fail): `{ outcome: "passed" }`, `{ outcome:
// "failed", message }`, or `{ outcome: "panicked", category, message,
// debug }`.
export function judge(record, expectPanic) {
  if (record.trapped) {
    const panic = panicOf(record.stderr);
    if (panic.category === expectPanic) return { outcome: "passed" };
    return { outcome: "panicked", ...panic };
  }
  if (expectPanic !== null) {
    return {
      outcome: "failed",
      message: `the case completed, but it expects the panic \`${expectPanic}\``,
    };
  }
  if (record.status === 0) return { outcome: "passed" };
  const text = record.stderr.trimEnd();
  return {
    outcome: "failed",
    message: text === "" ? `\`report()\` returned \`ExitCode(${record.status})\`` : text,
  };
}
