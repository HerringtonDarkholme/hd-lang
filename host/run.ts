// `hd run` on V8 (Node): one instance of the program, `hd.init` then
// `hd.poll` to completion (core.ts); its status is the process's.
// `argv[3]`, when it ends in `.json`, is the run's configuration that `hd`
// writes: the program's `Args.program` (`program`), its capability grant
// (`grants`), its `--time-limit` in milliseconds (`timeLimitMs`), the file
// where a trap's Wasm frames go (`framesFile`), and the file where the
// run's status goes when this process has to kill itself (`statusFile`).
// The program's arguments follow (`cli.args.pass`). The conformance
// runner's configuration may name a fixture's runtime `profile` instead,
// whose providers profiles.ts holds.
//
// A run with a time limit (cli.limit.time) runs the instance in a worker
// thread. The host cuts its own waits at the deadline and reports the
// `time-limit` panic after flushing the program's output. A Wasm loop, or
// a read the host cannot cut, never returns to the host, so this main
// thread stops a worker still running a little past the deadline. Only
// this thread writes the timeout diagnostic: a worker that reaches its own
// deadline sends a typed report instead of writing it. A worker blocked
// in a system call, such
// as a read of a FIFO with no writer, does not stop, and Node cannot exit
// while it runs, so once the run has ended this thread kills the process
// if the worker has not exited soon after (node.ts `exitPast`). Standard
// input belongs to the input provider's I/O worker in both profiles.
//
// The configuration's `cleanupMs`, which `hd` always writes, is the cleanup
// bound of a Ctrl-C (cli.cancel.ctrl-c, cli.cancel.bound): a Ctrl-C that
// finds the program waiting on the host cancels its entry, and the run
// goes on through the entry's cleanup until it ends, the next Ctrl-C, or
// the bound, which abandon it (core.ts `entryCancel`). A Ctrl-C while
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
import { createOutput } from "./output.ts";
import { runHere } from "./run-here.ts";
import {
  CANCEL_STATUS,
  entryCancel,
  exitPast,
  PROGRAM_TIME_LIMIT,
  reportIn,
} from "./run-support.ts";
import type { Sink } from "./core.ts";
import type { CancelControl, RunConfig } from "./run-here.ts";
import type { EntryCancel } from "./run-support.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;

// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.ts).
const { readFileSync } = process.getBuiltinModule("node:fs");
const { isMainThread, MessageChannel, receiveMessageOnPort, Worker, workerData } =
  process.getBuiltinModule("node:worker_threads");

// How long past the deadline the main thread waits for the host to end
// the run on its own before it stops the worker.
const GRACE_MS = 50;

// The signal that is a Ctrl-C to this process (see the header).
const interruptOf = (config: RunConfig): NodeJS.Signals => config.interruptSignal ?? "SIGINT";

// The wall clock, comparable across threads.
const now = () => performance.timeOrigin + performance.now();

// The worker's side of a Ctrl-C under `supervise`, as an `entryCancel`:
// `cell[0]` counts the requests the main thread passed on, and `cell[1]`
// is 1 while the host waits, when a request ends the wait at once.
function sharedCancel(boundMs: number, cell: Int32Array<SharedArrayBuffer>): CancelControl {
  let active: object | null = null;
  return {
    boundMs,
    requests: () => Atomics.load(cell, 0),
    wait: async (ms: number, signal?: AbortSignal) => {
      const seen = Atomics.load(cell, 0);
      const token = {};
      active = token;
      Atomics.store(cell, 1, 1);
      const clear = () => {
        if (active === token) {
          active = null;
          Atomics.store(cell, 1, 0);
        }
      };
      const abort = () => {
        clear();
        // Remove this native waiter as soon as another completion wins.
        Atomics.notify(cell, 0);
      };
      signal?.addEventListener("abort", abort, { once: true });
      try {
        if (!signal?.aborted) {
          // The Node host's idle port keeps the worker alive. waitAsync
          // needs only its real deadline; no parallel timer is necessary.
          await Atomics.waitAsync(cell, 0, seen, ms).value;
        }
      } finally {
        signal?.removeEventListener("abort", abort);
        clear();
      }
    },
  };
}

// Runs `module` in a worker until its deadline, `limit` milliseconds from
// now, and returns the status: the program's, or 3 for a worker it stopped.
function supervise({
  module,
  config,
  args,
  limit,
  output,
}: {
  module: WebAssembly.Module;
  config: RunConfig;
  args: string[];
  limit: number;
  output: DynamicRecord;
}): Promise<number> {
  return new Promise((resolve, fail) => {
    const { port1: port, port2 } = new MessageChannel();
    // The Ctrl-C requests passed on, and whether the worker's host waits
    // (`sharedCancel`).
    const cell = config.cleanupMs == null ? null : new Int32Array(new SharedArrayBuffer(8));
    const signal = interruptOf(config);
    const outputConnection = output.connection();
    const worker = new Worker(new URL(import.meta.url), {
      workerData: { module, config, args, limit, port: port2, cell, outputConnection },
      transferList: [port2, outputConnection.port],
    });
    let status: number | null = null;
    let ended = false;
    // Whether this thread has written the timeout diagnostic.
    let reported = false;
    // The deadline's timer, once the worker has started the program.
    let timer: ReturnType<typeof setTimeout> | undefined;
    // The label of the facts body the worker runs, or null: a time limit
    // that stops it names its facts (annot.fact.eval.panic-runtime).
    let fact: string | null = null;
    // A message is a timeout report, the facts body, or the program's status.
    const take = (message: Dynamic): void => {
      if (message.started !== undefined) {
        arm(message.started);
      } else if (message.fact !== undefined) {
        fact = message.fact;
      } else if (message.timeLimit !== undefined) {
        if (!reported) {
          reported = true;
          output.err(message.timeLimit);
        }
      } else {
        status = message.status;
      }
    };
    const end = (value: number): void => {
      if (ended) {
        return;
      }
      ended = true;
      clearTimeout(timer);
      if (cell) {
        process.off(signal, interrupt);
      }
      port.close();
      output.drain().then(() => {
        exitPast([worker], value, config.statusFile);
        resolve(value);
      }, fail);
    };
    // A Ctrl-C reaches a worker whose host waits, which cancels or
    // abandons the entry; one that computes is stopped at once, after what
    // it sent is written (cli.cancel.computing).
    const interrupt = () => {
      if (ended) {
        return;
      }
      if (Atomics.load(cell!, 1) === 1) {
        Atomics.add(cell!, 0, 1);
        Atomics.notify(cell!, 0);
        return;
      }
      for (let received; status === null && (received = receiveMessageOnPort(port));) {
        take(received.message);
      }
      if (status !== null) {
        return end(status);
      }
      worker.terminate();
      output.err("stopped\n");
      end(CANCEL_STATUS);
    };
    if (cell) {
      process.on(signal, interrupt);
    }
    // What the worker sent before the deadline is written first; a worker
    // that ended the run itself is not stopped. The limit counts from when
    // the worker starts the program (`started`, its deadline), not from
    // its spawn: a loaded machine can take most of a short limit to boot
    // a worker, which then ran out of time before the program's first line.
    function arm(deadline: number): void {
      if (ended || timer !== undefined) {
        return;
      }
      timer = setTimeout(
        () => {
          for (let received; status === null && (received = receiveMessageOnPort(port));) {
            take(received.message);
          }
          if (status !== null) {
            return end(status);
          }
          worker.terminate();
          if (!reported) {
            output.err(reportIn(fact, PROGRAM_TIME_LIMIT));
          }
          end(3);
        },
        Math.max(0, deadline - now()) + GRACE_MS,
      );
    }
    port.on("message", (message) => {
      if (!ended) {
        take(message);
      }
    });
    worker.on("error", (error: Error) => {
      if (ended) {
        return;
      }
      ended = true;
      clearTimeout(timer);
      if (cell) {
        process.off(signal, interrupt);
      }
      port.close();
      fail(error);
    });
    worker.on("exit", () => {
      for (let received; !ended && (received = receiveMessageOnPort(port));) {
        take(received.message);
      }
      end(status ?? 1);
    });
  });
}

if (isMainThread) {
  const bytes = readFileSync(process.argv[2]);
  const spec = process.argv[3] ?? "";
  const config: RunConfig = spec.endsWith(".json") ? JSON.parse(readFileSync(spec, "utf8")) : {};
  const args = process.argv.slice(spec.endsWith(".json") ? 4 : 3);
  const limit = config.timeLimitMs ?? null;
  // A Ctrl-C is heard only while the host waits (`entryCancel`'s `onWait`).
  const signal = interruptOf(config);
  // `hd` passes the interrupt on as another signal: ignore the terminal's
  // own SIGINT for good, so it is not heard twice.
  if (config.cleanupMs != null && signal !== "SIGINT") {
    process.on("SIGINT", () => {});
  }
  let cancel: EntryCancel | null = null;
  if (config.cleanupMs != null) {
    cancel = entryCancel(config.cleanupMs, (waiting) => {
      if (waiting) {
        process.on(signal, cancel!.request);
      } else {
        process.off(signal, cancel!.request);
      }
    });
  }
  const output = createOutput();
  // A supervised run shares the writer thread with its worker, and on
  // Windows every write uses it, so there it boots while the module
  // compiles. Otherwise writes go direct and a full pipe starts it.
  if (limit !== null || output.threaded()) {
    output.start();
  }
  try {
    process.exitCode =
      limit === null
        ? await runHere({ module: bytes, config, args, sink: output as Sink, cancel })
        : await supervise({
            module: await WebAssembly.compile(bytes),
            config,
            args,
            limit,
            output,
          });
  } finally {
    await output.close();
  }
} else {
  const { module, config, args, limit, port, cell, outputConnection } = workerData;
  const output = createOutput(outputConnection);
  // The program's time starts now (`supervise`'s `arm`).
  const deadline = now() + limit;
  port.postMessage({ started: deadline });
  const status = await runHere({
    module,
    config,
    args,
    sink: {
      out: output.out,
      write: output.write,
      err: output.err,
      programTimeLimit: (text: string) => port.postMessage({ timeLimit: text }),
      fact: (label: string | null) => port.postMessage({ fact: label }),
    },
    deadline,
    cancel: cell ? sharedCancel(config.cleanupMs, cell) : null,
  });
  port.postMessage({ status });
  port.close();
}
