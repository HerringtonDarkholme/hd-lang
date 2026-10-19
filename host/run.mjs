// `hd run` on V8 (Node): one instance of the program, `hd.init` then
// `hd.poll` to completion (core.mjs); its status is the process's.
// `argv[3]`, when it ends in `.json`, is the run's configuration that `hd`
// writes: the program's `Args.program` (`program`), its capability grant
// (`grants`), its `--time-limit` in milliseconds (`timeLimitMs`), the file
// where a trap's Wasm frames go (`framesFile`), and the file where the
// run's status goes when this process has to kill itself (`statusFile`).
// The program's arguments follow (`cli.args.pass`). The conformance
// runner's configuration may name a fixture's runtime `profile` instead,
// whose providers profiles.mjs holds.
//
// A run with a time limit (cli.limit.time) runs the instance in a worker
// thread. The host cuts its own waits at the deadline and reports the
// `time-limit` panic after flushing the program's output. A Wasm loop, or
// a read the host cannot cut, never returns to the host, so this main
// thread stops a worker still running a little past the deadline and
// reports the panic for it. The worker sends its output here, in order,
// so the two never both report. A worker blocked in a system call, such
// as a read of a FIFO with no writer, does not stop, and Node cannot exit
// while it runs, so once the run has ended this thread kills the process
// if the worker has not exited soon after (node.mjs `exitPast`). This
// thread also reads standard input for the worker: a thread blocked in a
// read cannot be stopped, and the process cannot exit while it waits.
//
// The configuration's `cleanupMs`, which `hd` always writes, is the cleanup
// bound of a Ctrl-C (cli.cancel.ctrl-c, cli.cancel.bound): a Ctrl-C that
// finds the program waiting on the host cancels its entry, and the run
// goes on through the entry's cleanup until it ends, the next Ctrl-C, or
// the bound, which abandon it (core.mjs `entryCancel`). A Ctrl-C while
// Wasm runs stops the program at once (cli.cancel.computing). On this
// thread that is the interrupt signal's default action, since the listener
// is on only while the host waits, and `hd` reports the signal as
// `stopped`; a supervising thread stops its worker and reports it itself.
//
// The interrupt is SIGINT, unless the configuration's `interruptSignal`
// names another signal. `hd` names one on Unix (cli.cancel.forward): it
// hears the interrupt itself, from a terminal or from a signal sent to it
// alone, and passes it on as that signal, once. A terminal's Ctrl-C also
// reaches this process, so a process told of such a signal ignores SIGINT
// (the listener below), else the one interrupt would arrive twice.
import { readFileSync, writeFileSync, writeSync } from "node:fs";
import {
  isMainThread,
  MessageChannel,
  receiveMessageOnPort,
  Worker,
  workerData,
} from "node:worker_threads";
import { CANCEL_STATUS, entryCancel } from "./core.mjs";
import { createHost, exitPast, PROGRAM_TIME_LIMIT, reportIn } from "./node.mjs";

// How long past the deadline the main thread waits for the host to end
// the run on its own before it stops the worker.
const GRACE_MS = 50;

// The signal that is a Ctrl-C to this process (see the header).
const interruptOf = (config) => config.interruptSignal ?? "SIGINT";

// The wall clock, comparable across threads.
const now = () => performance.timeOrigin + performance.now();

// Standard input, read without blocking this thread, one line at a time
// for `next(answer)`: the line without its ending, `null` at the end of
// input (and every time after), or `false` when it cannot be read, as
// node.mjs `lineReader` answers.
function inputLines() {
  let pending = Buffer.alloc(0);
  // What every read answers once the input ended: `null`, or `false`.
  let last;
  const waiting = [];
  const serve = () => {
    while (waiting.length) {
      const nl = pending.indexOf(10);
      let line;
      if (nl >= 0) {
        line = pending.subarray(0, nl);
        pending = pending.subarray(nl + 1);
        if (line.length && line[line.length - 1] === 13) line = line.subarray(0, line.length - 1);
        line = line.toString("utf8");
      } else if (last === undefined) {
        return;
      } else if (pending.length && last === null) {
        line = pending.toString("utf8");
        pending = Buffer.alloc(0);
      } else {
        line = last;
      }
      waiting.shift()(line);
    }
  };
  let stdin = null;
  return {
    next: (answer) => {
      if (stdin === null) {
        stdin = process.stdin;
        stdin.on("data", (chunk) => {
          pending = Buffer.concat([pending, chunk]);
          serve();
        });
        stdin.on("end", () => {
          last ??= null;
          serve();
        });
        stdin.on("error", () => {
          last ??= false;
          serve();
        });
      }
      waiting.push(answer);
      serve();
    },
    close: () => stdin?.destroy(),
  };
}

// The worker's side of a Ctrl-C under `supervise`, as an `entryCancel`:
// `cell[0]` counts the requests the main thread passed on, and `cell[1]`
// is 1 while the host waits, when a request ends the wait at once.
function sharedCancel(boundMs, cell) {
  return {
    boundMs,
    requests: () => Atomics.load(cell, 0),
    wait: async (ms) => {
      const seen = Atomics.load(cell, 0);
      Atomics.store(cell, 1, 1);
      // A pending `waitAsync` does not keep the worker's event loop alive,
      // so a timer of the same length does.
      let timer;
      await Promise.race([
        Atomics.waitAsync(cell, 0, seen, ms).value,
        new Promise((resolve) => {
          timer = setTimeout(resolve, ms);
        }),
      ]);
      clearTimeout(timer);
      Atomics.store(cell, 1, 0);
    },
  };
}

// Runs `module` (bytes or a compiled module) in this thread under
// `config`, writing through `sink`, and returns the status. `readLine`,
// when given, reads standard input; `cancel`, when given, is how a
// Ctrl-C reaches the run.
async function runHere(
  module,
  config,
  args,
  sink,
  deadline = null,
  readLine = undefined,
  cancel = null,
) {
  const profile = config.profile ? await import("./profiles.mjs") : null;
  const host = createHost(sink, {
    args,
    program: config.program ?? "",
    grants: config.grants ?? {},
    providers: profile ? (io) => profile.providers(config.profile, io) : null,
    deadline,
    readLine,
  });
  try {
    const instance = await WebAssembly.instantiate(module, host.imports);
    const { status, frames } = await host.run(
      instance.instance ?? instance,
      "hd.init",
      "hd.poll",
      null,
      cancel,
    );
    // The frames of a trap, for `hd` to name (`flow.panic.report`).
    if (frames?.length && config.framesFile)
      writeFileSync(config.framesFile, JSON.stringify(frames));
    return status;
  } finally {
    host.close();
  }
}

// Runs `module` in a worker until its deadline, `limit` milliseconds from
// now, and returns the status: the program's, or 3 for a worker it stopped.
function supervise(module, config, args, limit) {
  return new Promise((done, fail) => {
    const deadline = now() + limit;
    const { port1: port, port2 } = new MessageChannel();
    // Set to 1 when an input line is ready for the worker.
    const flag = new Int32Array(new SharedArrayBuffer(4));
    // The Ctrl-C requests passed on, and whether the worker's host waits
    // (`sharedCancel`).
    const cell = config.cleanupMs == null ? null : new Int32Array(new SharedArrayBuffer(8));
    const signal = interruptOf(config);
    const input = inputLines();
    const worker = new Worker(new URL(import.meta.url), {
      workerData: { module, config, args, deadline, port: port2, flag, cell },
      transferList: [port2],
    });
    let status = null;
    let ended = false;
    // Whether the host already reported the panic.
    let reported = false;
    // The label of the facts body the worker runs, or null: a time limit
    // that stops it names its facts (annot.fact.eval.panic-runtime).
    let fact = null;
    // A message is output (`fd`, `text`), a read of an input line, the
    // facts body that runs (`fact`), or the program's status.
    const take = (m) => {
      if (m.fact !== undefined) fact = m.fact;
      else if (m.read) {
        input.next((line) => {
          if (ended) return;
          port.postMessage({ line });
          Atomics.store(flag, 0, 1);
          Atomics.notify(flag, 0);
        });
      } else if (m.fd === undefined) status = m.status;
      else {
        writeSync(m.fd, m.text);
        if (m.fd === 2 && m.text === reportIn(fact, PROGRAM_TIME_LIMIT)) reported = true;
      }
    };
    const end = (s) => {
      if (ended) return;
      ended = true;
      clearTimeout(timer);
      if (cell) process.off(signal, interrupt);
      port.close();
      input.close();
      exitPast([worker], s, config.statusFile);
      done(s);
    };
    // A Ctrl-C reaches a worker whose host waits, which cancels or
    // abandons the entry; one that computes is stopped at once, after what
    // it sent is written (cli.cancel.computing).
    const interrupt = () => {
      if (ended) return;
      if (Atomics.load(cell, 1) === 1) {
        Atomics.add(cell, 0, 1);
        Atomics.notify(cell, 0);
        return;
      }
      for (let m; status === null && (m = receiveMessageOnPort(port));) take(m.message);
      if (status !== null) return end(status);
      worker.terminate();
      writeSync(2, "stopped\n");
      end(CANCEL_STATUS);
    };
    if (cell) process.on(signal, interrupt);
    // What the worker sent before the deadline is written first; a worker
    // that ended the run itself is not stopped.
    const timer = setTimeout(() => {
      for (let m; status === null && (m = receiveMessageOnPort(port));) take(m.message);
      if (status !== null) return end(status);
      worker.terminate();
      if (!reported) writeSync(2, reportIn(fact, PROGRAM_TIME_LIMIT));
      end(3);
    }, limit + GRACE_MS);
    port.on("message", (m) => {
      if (!ended) take(m);
    });
    worker.on("error", (e) => {
      if (ended) return;
      ended = true;
      clearTimeout(timer);
      if (cell) process.off(signal, interrupt);
      port.close();
      input.close();
      fail(e);
    });
    worker.on("exit", () => {
      for (let m; !ended && (m = receiveMessageOnPort(port));) take(m.message);
      end(status ?? 1);
    });
  });
}

if (isMainThread) {
  const bytes = readFileSync(process.argv[2]);
  const spec = process.argv[3] ?? "";
  const config = spec.endsWith(".json") ? JSON.parse(readFileSync(spec, "utf8")) : {};
  const args = process.argv.slice(spec.endsWith(".json") ? 4 : 3);
  const limit = config.timeLimitMs ?? null;
  // A Ctrl-C is heard only while the host waits (`entryCancel`'s `onWait`).
  const signal = interruptOf(config);
  // `hd` passes the interrupt on as another signal: ignore the terminal's
  // own SIGINT for good, so it is not heard twice.
  if (config.cleanupMs != null && signal !== "SIGINT") process.on("SIGINT", () => {});
  const cancel =
    config.cleanupMs == null
      ? null
      : entryCancel(config.cleanupMs, (waiting) => {
          if (waiting) process.on(signal, cancel.request);
          else process.off(signal, cancel.request);
        });
  process.exitCode =
    limit === null
      ? await runHere(
          bytes,
          config,
          args,
          {
            out: (text) => writeSync(1, text),
            err: (text) => writeSync(2, text),
          },
          null,
          undefined,
          cancel,
        )
      : await supervise(await WebAssembly.compile(bytes), config, args, limit);
} else {
  const { module, config, args, deadline, port, flag, cell } = workerData;
  // The next input line, which the main thread reads.
  const readLine = () => {
    Atomics.store(flag, 0, 0);
    port.postMessage({ read: true });
    Atomics.wait(flag, 0, 0);
    return receiveMessageOnPort(port).message.line;
  };
  const status = await runHere(
    module,
    config,
    args,
    {
      out: (text) => port.postMessage({ fd: 1, text }),
      err: (text) => port.postMessage({ fd: 2, text }),
      fact: (label) => port.postMessage({ fact: label }),
    },
    deadline,
    readLine,
    cell ? sharedCancel(config.cleanupMs, cell) : null,
  );
  port.postMessage({ status });
  port.close();
}
