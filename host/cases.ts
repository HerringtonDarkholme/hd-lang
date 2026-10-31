// The test runner's case loop, shared by `hd test` on Node (test.ts) and
// the playground (website/playground/src/rust-runner.ts). It uses
// web-standard APIs only; the caller hands it what touches the platform.
// The test program is compiled once; each case runs in its own fresh
// instance (module.testing.instance); a property reuses it while bodies
// succeed (std-testing.prop.shared-instance). Its module's init export runs first, then
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
//   per case in one instance while bodies succeed: the examples first, then a saved
//   regression stream, then `cases` generated cases from consecutive
//   seeds (std-testing.prop.examples, .prop.regression-replay,
//   .runner.case.seed-step). A discarded case does not count, and more
//   than 10 times `cases` discards fail the property
//   (std-testing.prop.discard, .prop.discard-limit). A failing case's
//   choice stream is shrunk, at most `shrink` attempts, its shrunk stream
//   saved, and the shrunk input's `Debug` text reported
//   (std-testing.prop.generate-and-shrink, .prop.regression-file, .prop.failure-input).
//
// Each run of a body gets a temporary directory of its own: the case's
// `tempDir` for the first, a numbered directory inside it for each later
// one, removed once that run ends (std-testing.temp-dir.per-run).
//
// One record per result goes to `emit`: its case's `test` index, then
// `status`, `trapped`, its captured output and its time. A trapped run's
// record adds its Wasm `frames`, and the `category` and `message` of its
// panic as the host recorded them, which a runner judges instead of the
// report on standard error. A row's record also has `row` and `rows`;
// an empty table is one record with `rows: 0` and no result. A property's
// record adds its failing input's text as `input` and an optional runner
// `failure` when fresh replay cannot reproduce it. An update
// run's record adds `updates`, its recorded snapshots (`encodeUpdates`).
//
// `judge` and `panicOf` are what a result comes to for a runner that
// reads these records itself (the playground; `hd test` judges them in
// test_cmd.rs).

import { EndCase } from "./core.ts";
import type { Host, Sink } from "./core.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;

/** The files of a test run: temporary directories, snapshots, regression streams. */
export interface TestFiles {
  join(dir: string, name: string): string;
  read(path: string): string | null;
  write(path: string, text: string): void;
  remove(dir: string): void;
}

export interface CaseConfig {
  test: number;
  init: number;
  kind: string;
  tempDir: string | null;
  seed: number | null;
  grants: Record<string, false | string[]>;
  snapshot?: { dir: string; shown?: string | null; slug: string; update: boolean } | null;
  regression?: { file: string; shown?: string } | null;
}

export interface RunConfig {
  program: string;
  args: string[];
  programs?: null;
  cases: CaseConfig[];
}

export interface CaseRecord extends DynamicRecord {
  test: number;
  row?: number;
  rows?: number;
  status?: number;
  trapped?: boolean;
  stdout?: string;
  stderr?: string;
  us?: number;
  frames?: [number, number][];
  input?: string;
  failure?: string;
  category?: string;
  message?: string;
}

export interface RunCasesOptions {
  module: WebAssembly.Module;
  config: RunConfig;
  createHost(sink: Sink, env: DynamicRecord): Host;
  files: TestFiles;
  notify(message: { type: string } & DynamicRecord): void;
  emit(record: CaseRecord): void;
}

export interface Panic {
  category: string;
  message: string;
  debug: string[];
}

export interface JudgedRecord {
  failure?: string;
  status: number;
  trapped: boolean;
  stderr: string;
  category?: string;
  message?: string;
}

export type Verdict =
  | { outcome: "passed" }
  | { outcome: "failed"; message: string }
  | ({ outcome: "panicked" } & Panic);

// The wall clock, comparable across threads.
const now = () => performance.timeOrigin + performance.now();

// The message of the discard panic (std-testing.runner.discard-panic).
const DISCARD = "std.testing: case discarded";

function finishRun({ live, ran, dir, started, updates, files }: DynamicRecord): DynamicRecord {
  const { host, io } = live;
  const { trapped, ended, frames, panic } = ran;
  const leaks = host.leaks?.() ?? [];
  const leaked = !trapped && ran.status === 0 && leaks.length > 0;
  const status = leaked ? 1 : ran.status;
  if (leaked) {
    io.stderr += leakReport(leaks);
  }
  if (dir) {
    files.remove(dir);
  }
  const st = host.core.st;
  if (st.ownTemp) {
    files.remove(st.ownTemp);
    st.ownTemp = null;
  }
  return {
    release: trapped || status !== 0 || ended,
    result: {
      status,
      trapped,
      ended,
      stdout: io.stdout,
      stderr: io.stderr,
      us: Math.round((performance.now() - started) * 1000),
      frames,
      panic: panic ?? null,
      updates,
    },
  };
}

// What one run of the cases needs from its platform:
//
// - `module`: the compiled test program; `config`: the run's
//   configuration (`program`, `args`, `programs` and per case its `test`
//   and `init` export indices, `kind`, `tempDir`, `seed`, `grants`,
//   `snapshot` and `regression`; test.ts has the format).
// - `createHost(sink, env)`: a host for one instance.
// - `files`: `join(dir, name)`, `read(path)` (the text, or null when there
//   is no such file), `write(path, text)` (making its directory) and
//   `remove(dir)`, for the temporary directories and regression files.
// - `notify(message)`: what the case loop is doing, for a supervisor that
//   stops a body running past its timeout: `case` (`ci`), `rows`
//   (`rows`), `row` (`row`), `limit` (`at`, the end of a `--time-limit`
//   for the case or row that began), `deadline` (`at`, `ms`), `fact`
//   (`label`, of the facts body that runs, or null) and `done`.
// - `emit(record)`: one result.
class Runner {
  module: WebAssembly.Module;
  config: Dynamic;
  createHost: RunCasesOptions["createHost"];
  files: TestFiles;
  notify: RunCasesOptions["notify"];
  emit: RunCasesOptions["emit"];
  deadline: number | null;
  live: Dynamic = null;

  constructor({ module, config, createHost, files, notify, emit }: RunCasesOptions) {
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
  begin(): void {
    const ms = this.config.timeLimitMs ?? null;
    if (ms === null) {
      return;
    }
    this.deadline = now() + ms;
    this.notify({ type: "limit", at: this.deadline });
  }

  release(): void {
    this.live?.host.close();
    this.live = null;
  }

  createLive(context: Dynamic, dir: string | null, runner: Dynamic, updates: Dynamic): Dynamic {
    const io = { stdout: "", stderr: "" };
    const env = {
      args: this.config.args,
      program: this.config.program,
      programs: this.config.programs ?? null,
      processLauncher: this.config.processLauncher,
      stdin: null,
      grants: context.grants,
      tempDir: dir,
      seed: context.seed,
      snapshot: context.snapshot ?? null,
      updates,
      runner,
      deadline: this.deadline,
    };
    const host = this.createHost(
      {
        out: (text: string) => {
          io.stdout += text;
        },
        err: (text: string) => {
          io.stderr += text;
        },
        fact: (label: string | null) => this.notify({ type: "fact", label }),
      },
      env,
    );
    return { host, env, io, instance: null, repeated: null };
  }

  prepareLive(context: Dynamic, dir: string | null, runner: Dynamic, updates: Dynamic): Dynamic {
    if (!this.live) {
      this.live = this.createLive(context, dir, runner, updates);
    }
    const { host, env, io } = this.live;
    io.stdout = "";
    io.stderr = "";
    Object.assign(env, { runner, tempDir: dir, updates });
    Object.assign(host.core.st, {
      reported: false,
      panic: null,
      timeoutMs: null,
      deadline: host.core.st.baseDeadline,
      snapshots: 0,
      tempMade: false,
      payload: null,
    });
    return this.live;
  }

  async execute(live: Dynamic, context: Dynamic): Promise<Dynamic> {
    const fresh = live.instance === null;
    if (fresh) {
      live.instance = await WebAssembly.instantiate(this.module, live.host.imports);
      // The entry driver always calls an init export. A retained instance
      // has already initialized its modules, so give that driver a no-op.
      live.repeated = { exports: { ...live.instance.exports, "case.init": () => {} } };
    }
    return live.host.run(
      fresh ? live.instance : live.repeated,
      fresh ? `hd.init.${context.init}` : "case.init",
      `hd.test.${context.test}`,
    );
  }

  // Successful property bodies retain their instance and import object.
  // Rows, failures and every shrink attempt get a fresh instance.
  async runOnce(context: Dynamic, k: number, runner: Dynamic, reuse = false): Promise<Dynamic> {
    const dir =
      context.tempDir && k > 0 ? this.files.join(context.tempDir, String(k)) : context.tempDir;
    const updates = context.snapshot?.update && context.kind === "it" ? [] : null;
    if (!reuse) {
      this.release();
    }
    const live = this.prepareLive(context, dir, runner, updates);
    const started = performance.now();
    const ran = await this.execute(live, context);
    this.notify({ type: "done" });
    const finished = finishRun({ live, ran, dir, started, updates, files: this.files });
    if (finished.release) {
      this.release();
    }
    return finished.result;
  }
}

// The failure message of a run that left host handles open: each kind
// with its count (cli.test.leak.message).
export const leakReport = (leaks: [string, number][]): string =>
  `the test case ended with open handles: ${leaks.map(([kind, count]) => `${count} ${kind}`).join(", ")}\n`;

// A trapped run's record adds its Wasm frames, [function, offset] pairs
// innermost first, when the trap had any.
const result = (context: Dynamic, result: Dynamic, extra: DynamicRecord = {}): CaseRecord => ({
  test: context.test,
  ...extra,
  status: result.status,
  trapped: result.trapped,
  stdout: result.stdout,
  stderr: result.stderr,
  us: result.us,
  ...(result.trapped && result.panic
    ? { category: result.panic.category, message: result.panic.message }
    : {}),
  ...(result.trapped && result.frames?.length ? { frames: result.frames } : {}),
  ...(result.failure === undefined ? {} : { failure: result.failure }),
  ...(result.updates?.length ? { updates: encodeUpdates(result.updates) } : {}),
});

// The snapshots an update run recorded, as one string: a record per line,
// the call's frames as `function:offset` pairs, a tab, and the text as a
// JSON string.
const encodeUpdates = (updates: Dynamic[]): string =>
  updates
    .map(
      (update) =>
        `${update.frames.map((frame: Dynamic[]) => frame.join(":")).join(",")}\t${JSON.stringify(update.text)}`,
    )
    .join("\n");

// What one run of a property case did: the draws it recorded, its input's
// text once shown, and what it was asked to run.
class Run {
  draws: bigint[];
  shown: string | null;
  plan: Dynamic;

  constructor() {
    this.draws = [];
    this.shown = null;
    this.plan = null;
  }
}

// The runner state of one configured case, and the hooks that each of its
// instances calls.
class Case {
  runner: Runner;
  context: Dynamic;
  kind: string | null;
  row: number;
  rows: number;
  prop: Property | null;

  constructor(runner: Runner, context: Dynamic) {
    this.runner = runner;
    this.context = context;
    this.kind = null;
    this.row = 0;
    this.rows = 0;
    this.prop = null;
  }

  hooks(run: Run): DynamicRecord {
    const notify = this.runner.notify;
    return {
      // Only an `it_each` case runs rows; any other case runs row 0.
      row: (count: number) => {
        if (this.kind === null && this.context.kind === "it_each") {
          this.kind = "rows";
          this.rows = count;
          notify({ type: "rows", rows: count });
        }
        return this.row;
      },
      // A supervisor stops a run still going a little past this.
      timeout: (ms: number) => notify({ type: "deadline", at: now() + ms, ms }),
      slugSuffix: () => (this.kind === "rows" ? `.${this.row}` : ""),
      start: (cases: number, shrink: number, examples: number) => {
        if (this.kind === null) {
          this.kind = "prop";
          this.prop = new Property({
            c: this.context,
            files: this.runner.files,
            cases,
            shrink,
            examples,
          });
        }
        const plan = this.prop!.next();
        if (plan === null) {
          throw new EndCase();
        }
        run.plan = plan;
        return plan;
      },
      record: (value: bigint) => run.draws.push(value),
      show: (text: string) => {
        if (run.shown === null) {
          run.shown = text;
        }
      },
    };
  }
}

// The cases of one property test and its counts
// (spec/std/testing.md#runner-capabilities).
class Property {
  cases: number;
  shrink: number;
  examples: number;
  example: number;
  seed: bigint;
  size: number;
  passed: number;
  discarded: number;
  regression: Dynamic;
  replays: bigint[][];
  pending: Dynamic;
  retry: Dynamic = null;

  constructor({ c, files, cases, shrink, examples }: DynamicRecord) {
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
        .map((line: string) => line.trim())
        .filter((line: string) => line !== "")
        .map((line: string) => BigInt(line));
      this.replays.push(stream);
    }
    // What the next instance runs, when the runner chose it.
    this.pending = null;
  }

  // The case the next `start` returns, or null when nothing is left.
  next(): Dynamic {
    if (this.retry) {
      const plan = this.retry;
      this.retry = null;
      return plan;
    }
    const seed = this.seed;
    this.seed += 1n;
    if (this.pending !== null) {
      const pendingCase = this.pending;
      this.pending = null;
      return {
        example: null,
        seed,
        size: pendingCase.size,
        replay: pendingCase.replay,
        kind: pendingCase.kind,
      };
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
  more(): boolean {
    return this.example < this.examples || this.replays.length > 0 || this.passed < this.cases;
  }
}

// Whether a run's recorded panic is the discard panic; never its
// standard error text, which a program's own message could imitate.
const isDiscard = (result: Dynamic): boolean =>
  result.trapped && result.panic?.category === "explicit-panic" && result.panic.message === DISCARD;

// What a run of a property case came to: a discard is the discard panic
// before `show` (std-testing.runner.discard-read); the same panic after it
// is a failure (std-testing.runner.discard-after-show).
const outcome = (run: Run, result: Dynamic): string => {
  if (result.ended) {
    return "ended";
  }
  if (result.trapped) {
    return run.shown === null && isDiscard(result) ? "discard" : "fail";
  }
  return result.status === 0 ? "pass" : "fail";
};

// `a` is simpler than `b`: shorter, or as long and smaller draw by draw.
const simpler = (left: bigint[], right: bigint[]): boolean => {
  if (left.length !== right.length) {
    return left.length < right.length;
  }
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) {
      return left[i] < right[i];
    }
  }
  return false;
};

// Shrinks a failing case's choice stream (std-testing.prop.generate-and-shrink): deletes
// blocks of draws, zeroes them, and moves each draw toward 0 by halving
// the distance, keeping each attempt that still fails with a simpler
// stream. Each attempt is a run of its own; there are at most `budget`.
async function tryShrink(state: Dynamic, stream: bigint[]): Promise<boolean> {
  if (state.left <= 0) {
    return false;
  }
  state.left -= 1;
  const got = await state.attempt(stream);
  if (got !== null && simpler(got.run.draws, state.best.run.draws)) {
    state.best = got;
    return true;
  }
  return false;
}

async function deleteDraws(state: Dynamic): Promise<void> {
  for (const k of [8, 4, 2, 1]) {
    for (let i = 0; i + k <= state.best.run.draws.length && state.left > 0;) {
      const draws = state.best.run.draws;
      if (await tryShrink(state, [...draws.slice(0, i), ...draws.slice(i + k)])) {
        state.improved = true;
      } else {
        i += 1;
      }
    }
  }
}

async function zeroDraws(state: Dynamic): Promise<void> {
  for (const k of [8, 4, 2, 1]) {
    for (let i = 0; i + k <= state.best.run.draws.length && state.left > 0; i++) {
      const draws = state.best.run.draws;
      if (draws.slice(i, i + k).every((value: bigint) => value === 0n)) {
        continue;
      }
      const candidate = [...draws];
      candidate.fill(0n, i, i + k);
      if (await tryShrink(state, candidate)) {
        state.improved = true;
      }
    }
  }
}

async function reduceDraws(state: Dynamic): Promise<void> {
  for (let i = 0; i < state.best.run.draws.length && state.left > 0; i++) {
    let lo = 0n;
    while (state.left > 0) {
      const draws = state.best.run.draws;
      if (i >= draws.length || draws[i] <= lo) {
        break;
      }
      const mid = lo + (draws[i] - lo) / 2n;
      const candidate = [...draws];
      candidate[i] = mid;
      if (await tryShrink(state, candidate)) {
        state.improved = true;
      } else {
        lo = mid + 1n;
      }
    }
  }
}

async function shrink(
  failing: Dynamic,
  attempt: (stream: bigint[]) => Promise<Dynamic>,
  budget: number,
): Promise<Dynamic> {
  const state = { best: failing, left: budget, attempt, improved: true };
  while (state.improved && state.left > 0) {
    state.improved = false;
    await deleteDraws(state);
    await zeroDraws(state);
    await reduceDraws(state);
  }
  return state.best;
}

async function reportPropertyFailure({
  runner,
  c,
  p,
  current,
  again,
  timing,
}: DynamicRecord): Promise<void> {
  let { run } = current;
  if (current.reused && run.plan) {
    p.retry = { ...run.plan, replay: [...run.draws] };
    const replayed = await again(null);
    if (replayed.outcome === "pass" || replayed.outcome === "discard") {
      const ended = replayed.outcome === "pass" ? "passed" : "was discarded";
      const failure = `input ${run.shown ?? ""} (seed ${run.plan.seed}) failed only after earlier cases ran in the same instance and ${ended} when replayed in a fresh instance; the property depends on state left by earlier cases`;
      runner.emit(
        result(
          c,
          { ...current.r, status: 1, trapped: false, us: timing.total, failure },
          { input: run.shown ?? "" },
        ),
      );
      return;
    }
    current = replayed;
    run = current.run;
  }
  let failing = current;
  if (run.plan && run.plan.kind !== "example") {
    const size = run.plan.size;
    const attempt = async (stream: bigint[]) => {
      const got = await again({ replay: stream, size, kind: "shrink" });
      return got.outcome === "fail" ? got : null;
    };
    failing = await shrink(current, attempt, p.shrink);
    if (p.regression) {
      runner.files.write(
        p.regression.file,
        failing.run.draws.map((value: bigint) => `${value}\n`).join(""),
      );
    }
  }
  const caseResult = { ...failing.r, us: timing.total };
  runner.emit(result(c, caseResult, { input: failing.run.shown ?? "" }));
}

function reportDiscardLimit(
  runner: Runner,
  context: Dynamic,
  path: Property,
  total: number,
): boolean {
  if (path.discarded <= 10 * path.cases) {
    return false;
  }
  runner.emit(
    result(context, {
      status: 1,
      trapped: false,
      stdout: "",
      stderr: `the property discarded ${path.discarded} cases, more than 10 times its ${path.cases} cases\n`,
      us: total,
    }),
  );
  return true;
}

// Runs a property case to its end and reports it in one record.
async function runProperty({ runner, c, ctl, first, firstResult }: DynamicRecord): Promise<void> {
  const property = ctl.prop;
  let k = 1;
  const timing = { total: firstResult.us };
  const again = async (pending: Dynamic, reuse = false): Promise<Dynamic> => {
    property.pending = pending;
    const run = new Run();
    const result = await runner.runOnce(c, k++, ctl.hooks(run), reuse);
    timing.total += result.us;
    return { run, r: result, outcome: outcome(run, result), reused: reuse };
  };
  let current = { run: first, r: firstResult, outcome: outcome(first, firstResult) };
  for (;;) {
    const { run, outcome: caseOutcome } = current;
    if (caseOutcome === "ended") {
      break;
    }
    if (caseOutcome === "fail") {
      await reportPropertyFailure({ runner, c, p: property, current, again, timing });
      return;
    }
    if (caseOutcome === "discard") {
      property.discarded += 1;
      if (reportDiscardLimit(runner, c, property, timing.total)) {
        return;
      }
    } else if (caseOutcome === "pass" && run.plan && run.plan.kind === "fresh") {
      property.passed += 1;
      property.size += 1;
    }
    if (!property.more()) {
      break;
    }
    current = await again(null, caseOutcome === "pass");
  }
  runner.emit(result(c, { ...firstResult, status: 0, trapped: false, us: timing.total }));
}

// Runs the cases of `options.config` from `start` (a case index, and for a
// table resumed after a stopped row, its next row and its row count) and
// emits each result; `options` is described at `Runner`.
export async function runCases(
  options: RunCasesOptions,
  start: { case: number; row: number; rows: number } = { case: 0, row: 0, rows: 0 },
): Promise<void> {
  const runner = new Runner(options);
  const { cases } = runner.config;
  try {
    for (let ci = start.case; ci < cases.length; ci++) {
      const context = cases[ci];
      runner.notify({ type: "case", ci });
      runner.release();
      runner.begin();
      const ctl = new Case(runner, context);
      // Every row runs; one failing row hides no other.
      const rowsFrom = async (from: number, rows: number): Promise<void> => {
        for (let row = from; row < rows; row++) {
          ctl.row = row;
          runner.notify({ type: "row", row });
          runner.begin();
          runner.emit(
            result(context, await runner.runOnce(context, row, ctl.hooks(new Run())), {
              row,
              rows,
            }),
          );
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
      const caseResult = await runner.runOnce(context, 0, ctl.hooks(first));
      if (ctl.kind === "rows") {
        const rows = ctl.rows;
        if (rows === 0) {
          runner.emit({ test: context.test, row: 0, rows: 0 });
          continue;
        }
        runner.emit(result(context, caseResult, { row: 0, rows }));
        await rowsFrom(1, rows);
      } else if (ctl.kind === "prop") {
        await runProperty({ runner, c: context, ctl, first, firstResult: caseResult });
      } else {
        runner.emit(result(context, caseResult));
      }
    }
  } finally {
    runner.release();
  }
}

// The category and message of a trapped run's panic, as its record has
// them from the host, and the other lines of its standard error, the
// `dbg` output before the report (cli.dbg.test). A record with no panic
// is the runner's own failure, `internal`.
export function panicOf(record: JudgedRecord): Panic {
  const stderr = record.stderr ?? "";
  if (record.category === undefined) {
    return { category: "internal", message: stderr.trim(), debug: [] };
  }
  const { category, message = "" } = record;
  const before = [`panic: ${category}: ${message}\n`, `panic: ${category}\n`]
    .filter((report) => stderr.endsWith(report))
    .map((report) => stderr.slice(0, stderr.length - report.length))[0];
  const debug = (before ?? stderr).split("\n").filter((line) => line !== "");
  // The host's `deadlock` report is the `suspension-deadlock` panic.
  return {
    category: category === "deadlock" ? "suspension-deadlock" : category,
    message,
    debug,
  };
}

// What one result record of the case `expectPanic` (its `expect_panic`
// option, or null) comes to (module.testing.passes, module.testing.fail-cases,
// module.testing.expect-panic-fail): `{ outcome: "passed" }`, `{ outcome:
// "failed", message }`, or `{ outcome: "panicked", category, message,
// debug }`.
export function judge(record: JudgedRecord, expectPanic: string | null): Verdict {
  if (record.failure !== undefined) {
    return { outcome: "failed", message: record.failure };
  }
  if (record.trapped) {
    const panic = panicOf(record);
    if (panic.category === expectPanic) {
      return { outcome: "passed" };
    }
    return { outcome: "panicked", ...panic };
  }
  if (expectPanic !== null) {
    return {
      outcome: "failed",
      message: `the case completed, but it expects the panic \`${expectPanic}\``,
    };
  }
  if (record.status === 0) {
    return { outcome: "passed" };
  }
  const text = record.stderr.trimEnd();
  return {
    outcome: "failed",
    message: text === "" ? `\`report()\` returned \`ExitCode(${record.status})\`` : text,
  };
}
