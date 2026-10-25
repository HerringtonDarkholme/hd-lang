// The case loop of `hd test` on V8 (Node), shared by `test.mjs` and the
// driver conformance worker (`conformance-worker.mjs`): the cases run in
// a worker thread that the caller's thread supervises, so a body that
// overruns its timeout or `--time-limit` is stopped (see `test.mjs`).
import { readFileSync } from "node:fs";
import { parentPort, Worker, workerData } from "node:worker_threads";
import { runCases } from "./cases.mjs";
import {
  createHost,
  nodeFiles,
  panicOfReport,
  PROGRAM_TIME_LIMIT,
  reportIn,
  timeLimitReport,
} from "./node.mjs";

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
      return {
        test: Number(test),
        init: Number(init),
        kind: kind ?? "it",
        tempDir: null,
        seed: null,
        grants: {},
      };
    }),
});
export function configOf(spec) {
  return spec.endsWith(".json") ? JSON.parse(readFileSync(spec, "utf8")) : plain(spec);
}

// Runs the cases in a worker from `start` (a case index, and for a table
// resumed after a stopped row, its next row and its row count), restarting
// it after each run it stops for overrunning its timeout. Each result
// line goes to `writeOut`. Returns the workers it stopped.
export async function supervise(module, config, writeOut) {
  let start = { case: 0, row: 0, rows: 0 };
  const stoppedWorkers = [];
  while (start.case < config.cases.length) {
    start = await new Promise((next, fail) => {
      const worker = new Worker(new URL(import.meta.url), {
        workerData: { hdTestRun: true, module, config, start },
      });
      // What the worker runs: its case, the row of a table and its count,
      // and the current run's timeout.
      let at = { ci: start.case, row: start.row > 0 ? start.row : null, rows: start.rows || null };
      let timer = null;
      let stopped = false;
      // When the case's or row's `--time-limit` ends, and the current
      // run's timeout (`at`, `ms`); the earlier of the two stops the run.
      let limitAt = null;
      let timeout = null;
      // The label of the facts body the run is in, or null: a stop names
      // its facts (annot.fact.eval.panic-runtime).
      let fact = null;
      const clear = () => {
        if (timer !== null) clearTimeout(timer);
        timer = null;
      };
      const arm = () => {
        clear();
        const program = limitAt !== null && (timeout === null || limitAt <= timeout.at);
        const target = program ? limitAt : timeout?.at;
        if (target === undefined || target === null) return;
        timer = setTimeout(
          () => stop(program ? null : timeout.ms),
          Math.max(0, target - now()) + GRACE_MS,
        );
      };
      // `ms` is the timeout that ran out, or null for the program's
      // `--time-limit`.
      const stop = (ms) => {
        stopped = true;
        worker.terminate();
        stoppedWorkers.push(worker);
        const c = config.cases[at.ci];
        const shown = ms ?? config.timeLimitMs;
        const report = reportIn(fact, ms === null ? PROGRAM_TIME_LIMIT : timeLimitReport(ms));
        const line = {
          test: c.test,
          status: 3,
          trapped: true,
          stdout: "",
          stderr: report,
          ...panicOfReport(report.slice("panic: ".length)),
          us: Math.round((shown + GRACE_MS) * 1000),
        };
        if (at.row !== null && at.rows !== null) {
          writeOut(JSON.stringify({ test: c.test, row: at.row, rows: at.rows, ...line }) + "\n");
          next(
            at.row + 1 < at.rows
              ? { case: at.ci, row: at.row + 1, rows: at.rows }
              : { case: at.ci + 1, row: 0, rows: 0 },
          );
        } else {
          writeOut(JSON.stringify(line) + "\n");
          next({ case: at.ci + 1, row: 0, rows: 0 });
        }
      };
      worker.on("message", (m) => {
        if (stopped) return;
        if (m.type === "line") {
          // A result ends the case or row being watched, so nothing is
          // left to stop until the next one begins. A stop between this
          // result and the worker's next message, or its exit, would
          // report the same case or row a second time.
          limitAt = timeout = null;
          clear();
          writeOut(m.text);
        } else if (m.type === "case") {
          limitAt = timeout = null;
          arm();
          at = { ci: m.ci, row: null, rows: null };
        } else if (m.type === "rows") {
          at.rows = m.rows;
          if (at.row === null) at.row = 0;
        } else if (m.type === "row") {
          limitAt = timeout = null;
          arm();
          at.row = m.row;
        } else if (m.type === "limit") {
          limitAt = m.at;
          arm();
        } else if (m.type === "deadline") {
          timeout = { at: m.at, ms: m.ms };
          arm();
        } else if (m.type === "done") {
          timeout = null;
          fact = null;
          arm();
        } else if (m.type === "fact") {
          fact = m.label;
        }
      });
      worker.on("error", fail);
      worker.on("exit", () => {
        clear();
        if (!stopped) next({ case: config.cases.length, row: 0, rows: 0 });
      });
    });
  }
  return stoppedWorkers;
}

if (workerData?.hdTestRun === true) {
  await runCases(
    {
      module: workerData.module,
      config: workerData.config,
      createHost: (sink, env) => {
        env.module = workerData.module;
        return createHost(sink, env);
      },
      files: nodeFiles,
      notify: (message) => parentPort.postMessage(message),
      emit: (record) =>
        parentPort.postMessage({ type: "line", text: JSON.stringify(record) + "\n" }),
    },
    workerData.start,
  );
}
