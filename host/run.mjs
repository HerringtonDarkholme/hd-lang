// `hd run` on V8 (Node): one instance of the program, `hd.init` then
// `hd.poll` to completion (core.mjs); its status is the process's.
// `argv[3]`, when it ends in `.json`, is the run's configuration that `hd`
// writes: the program's `Args.program` (`program`), its capability grant
// (`grants`), and its `--time-limit` in milliseconds (`timeLimitMs`). The
// program's arguments follow (`cli.args.pass`). The conformance runner's
// configuration may name a fixture's runtime `profile` instead, whose
// providers profiles.mjs holds.
//
// A run with a time limit (cli.limit.time) runs the instance in a worker
// thread. The host cuts its own waits at the deadline and reports the
// `time-limit` panic after flushing the program's output. A Wasm loop, or
// a read the host cannot cut, never returns to the host, so this main
// thread stops a worker still running a little past the deadline and
// reports the panic for it. The worker sends its output here, in order,
// so the two never both report. This thread also reads standard input for
// the worker: a thread blocked in a read cannot be stopped, and the
// process cannot exit while it waits.
import { readFileSync, writeSync } from "node:fs";
import {
  isMainThread,
  MessageChannel,
  receiveMessageOnPort,
  Worker,
  workerData,
} from "node:worker_threads";
import { createHost, PROGRAM_TIME_LIMIT } from "./node.mjs";

// How long past the deadline the main thread waits for the host to end
// the run on its own before it stops the worker.
const GRACE_MS = 50;

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

// Runs `module` (bytes or a compiled module) in this thread under
// `config`, writing through `sink`, and returns the status. `readLine`,
// when given, reads standard input.
async function runHere(module, config, args, sink, deadline = null, readLine = undefined) {
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
    const { status } = await host.run(instance.instance ?? instance, "hd.init", "hd.poll");
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
    const input = inputLines();
    const worker = new Worker(new URL(import.meta.url), {
      workerData: { module, config, args, deadline, port: port2, flag },
      transferList: [port2],
    });
    let status = null;
    let ended = false;
    // Whether the host already reported the panic.
    let reported = false;
    // A message is output (`fd`, `text`), a read of an input line, or the
    // program's status.
    const take = (m) => {
      if (m.read) {
        input.next((line) => {
          if (ended) return;
          port.postMessage({ line });
          Atomics.store(flag, 0, 1);
          Atomics.notify(flag, 0);
        });
      } else if (m.fd === undefined) status = m.status;
      else {
        writeSync(m.fd, m.text);
        if (m.fd === 2 && m.text === PROGRAM_TIME_LIMIT) reported = true;
      }
    };
    const end = (s) => {
      if (ended) return;
      ended = true;
      clearTimeout(timer);
      port.close();
      input.close();
      done(s);
    };
    // What the worker sent before the deadline is written first; a worker
    // that ended the run itself is not stopped.
    const timer = setTimeout(() => {
      for (let m; status === null && (m = receiveMessageOnPort(port));) take(m.message);
      if (status !== null) return end(status);
      worker.terminate();
      if (!reported) writeSync(2, PROGRAM_TIME_LIMIT);
      end(3);
    }, limit + GRACE_MS);
    port.on("message", (m) => {
      if (!ended) take(m);
    });
    worker.on("error", (e) => {
      if (ended) return;
      ended = true;
      clearTimeout(timer);
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
  process.exitCode =
    limit === null
      ? await runHere(bytes, config, args, {
          out: (text) => writeSync(1, text),
          err: (text) => writeSync(2, text),
        })
      : await supervise(await WebAssembly.compile(bytes), config, args, limit);
} else {
  const { module, config, args, deadline, port, flag } = workerData;
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
    },
    deadline,
    readLine,
  );
  port.postMessage({ status });
  port.close();
}
