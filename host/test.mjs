// `hd test` on V8 (Node): the test program is compiled once, and the
// shared case loop (cases.mjs) runs each case in its own fresh instance
// (docs/compiler/engines-and-test-runner.md §19.3). `argv[3]` is the run's
// configuration, a `.json` file that `hd test` writes: the program's
// `Args.program` and `args` (Test Environments), the package's executables
// and tasks that its `Process` provider starts by name (`programs`,
// cli.test.process), then per case its `test` and `init` export indices,
// its registration function's name as `kind`, its own `tempDir`, the base `seed` of a property test, its `grants`, its
// `snapshot` files, and its `regression` file. Standard input is closed
// (cli.test.env.stdin).
//
// This file keeps what needs Node: the files of the temporary directories,
// snapshots and regression streams on disk, the worker thread the cases
// run in, its supervision, and standard output.
//
// A case's `timeout` (`TestRunner.report_timeout`, std-testing.runner.timeout)
// fails a body that runs longer: the host checks it at the end of the run
// and cuts a wait at it, and since a Wasm loop never returns to the host,
// the cases run in a worker thread that this file's main thread
// supervises. When a body is still running a little past its deadline,
// the supervisor stops the worker, reports the run as a `time-limit`
// panic, and starts a new worker at the next row or case.
//
// A `--time-limit` (`timeLimitMs`, cli.limit.test) gives each case, and each
// row of a table, its own deadline, which the host cuts its waits at
// (core.mjs `env.deadline`). A body still running past it is stopped the
// same way, with the report of a program that ran past its time limit.
// A worker stopped while blocked in a system call, such as a read of a FIFO
// with no writer, keeps running, and Node cannot exit while it does, so
// after the last case the supervisor kills the process if such a worker
// has not exited soon after (node.mjs `exitPast`).
//
// One JSON line per result record (cases.mjs) goes to standard output.
import { readFileSync, writeSync } from "node:fs";
import { exitPast } from "./node.mjs";
import { configOf, supervise } from "./test-run.mjs";

// Writes all of `text` to standard output. A pipe takes a long line, such
// as one with many frames, in several writes, and refuses a write while
// it is full.
function writeOut(text) {
  const buf = Buffer.from(text);
  let at = 0;
  while (at < buf.length) {
    try {
      at += writeSync(1, buf, at, buf.length - at);
    } catch (e) {
      if (e.code !== "EAGAIN") {
        throw e;
      }
    }
  }
}

const compiled = await WebAssembly.compile(readFileSync(process.argv[2]));
// Every result is written; `hd` reads the results, not the status.
exitPast(await supervise(compiled, configOf(process.argv[3] ?? ""), writeOut));
