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
// if the worker has not exited soon after (node.mjs `exitPast`). Standard
// input belongs to the input provider's I/O worker in both profiles.
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
import { readPackageContract } from "./package-providers.mjs";
import { CANCEL_STATUS, HostContract, entryCancel } from "./core.mjs";
import { createHost, exitPast, PROGRAM_TIME_LIMIT, reportIn } from "./node.mjs";
import { createOutput } from "./output.mjs";

// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.mjs).
const { readFileSync, writeFileSync } = process.getBuiltinModule("node:fs");
const { pathToFileURL } = process.getBuiltinModule("node:url");
const { isMainThread, MessageChannel, receiveMessageOnPort, Worker, workerData } =
  process.getBuiltinModule("node:worker_threads");

// How long past the deadline the main thread waits for the host to end
// the run on its own before it stops the worker.
const GRACE_MS = 50;

// The signal that is a Ctrl-C to this process (see the header).
const interruptOf = (config) => config.interruptSignal ?? "SIGINT";

// The wall clock, comparable across threads.
const now = () => performance.timeOrigin + performance.now();

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
// `config`, writing through `sink`, and returns the status. `cancel`,
// when given, is how a Ctrl-C reaches the run.
async function runHere(module, config, args, sink, deadline = null, cancel = null) {
  try {
    return await runHereUnchecked(module, config, args, sink, deadline, cancel);
  } catch (error) {
    if (!(error instanceof HostContract)) throw error;
    sink.err(`error: ${error.message}\n`);
    return 101;
  }
}

async function runHereUnchecked(module, config, args, sink, deadline, cancel) {
  // The unsupervised path may receive bytes; retain the compiled module
  // so checked reports can read its custom metadata too.
  module = module instanceof WebAssembly.Module ? module : await WebAssembly.compile(module);
  const sections = WebAssembly.Module.customSections(module, "hd.capabilities");
  if (sections.length > 1) throw new HostContract("duplicate package host contracts");
  const required = sections.length ? readPackageContract(new Uint8Array(sections[0])) : [];
  const registrations = new Map();
  const grants = { ...config.grants };
  for (const registration of config.hostProviders ?? []) {
    const i = required.find((i) => !i.handle && i.module === registration.module);
    if (!i || !i.selectors.includes(registration.grantPath))
      throw new HostContract("provider mapping names no required package capability");
    if (registrations.has(i.module))
      throw new HostContract(`duplicate provider for ${i.identity.trait_path}`);
    registrations.set(i.module, registration);
  }
  // Preflight every grant before importing any provider: initialization can
  // already touch the machine. Mappings come from the CLI's manifest check.
  for (const i of required.filter((i) => !i.handle)) {
    const name = `${i.identity.package}@${i.identity.version}/${i.identity.trait_path}`;
    if (i.selectors.some((s) => grants[s] === false))
      throw new HostContract(`package capability denied: ${name}`);
    const registration = registrations.get(i.module);
    if (!registration)
      throw new HostContract(
        `package capability ${name} is not granted; add its trait path to the app's [capabilities] table`,
      );
    grants[registration.grantPath] = true;
  }
  let packageBindings = null;
  if (required.length) {
    // This module was regenerated from validated data by hd, in its private
    // host directory. It is never JavaScript obtained from a guest section.
    const generated = await import("./main.host.mjs");
    const providers = new Map();
    for (const [module, registration] of registrations) {
      providers.set(module, (await import(pathToFileURL(registration.path).href)).default);
    }
    packageBindings = (runtime) => generated.bind(runtime, providers);
  }
  const profile = config.profile ? await import("./profiles.mjs") : null;
  const host = createHost(sink, {
    processLauncher: config.processLauncher,
    module,
    args,
    program: config.program ?? "",
    grants,
    providers: profile ? (io) => profile.providers(config.profile, io) : null,
    packageBindings,
    deadline,
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
function supervise(module, config, args, limit, output) {
  return new Promise((done, fail) => {
    const deadline = now() + limit;
    const { port1: port, port2 } = new MessageChannel();
    // The Ctrl-C requests passed on, and whether the worker's host waits
    // (`sharedCancel`).
    const cell = config.cleanupMs == null ? null : new Int32Array(new SharedArrayBuffer(8));
    const signal = interruptOf(config);
    const outputConnection = output.connection();
    const worker = new Worker(new URL(import.meta.url), {
      workerData: { module, config, args, deadline, port: port2, cell, outputConnection },
      transferList: [port2, outputConnection.port],
    });
    let status = null;
    let ended = false;
    // Whether the host already reported the panic.
    let reported = false;
    // The label of the facts body the worker runs, or null: a time limit
    // that stops it names its facts (annot.fact.eval.panic-runtime).
    let fact = null;
    // A message is output (`fd`, `text`), the facts body (`fact`),
    // or the program's status.
    const take = (m) => {
      if (m.fact !== undefined) fact = m.fact;
      else if (m.fd === undefined) status = m.status;
      else {
        // The guest already queued these bytes through its writer connection.
        if (m.fd === 2 && m.text === reportIn(fact, PROGRAM_TIME_LIMIT)) reported = true;
      }
    };
    const end = (s) => {
      if (ended) return;
      ended = true;
      clearTimeout(timer);
      if (cell) process.off(signal, interrupt);
      port.close();
      output.drain().then(() => {
        exitPast([worker], s, config.statusFile);
        done(s);
      }, fail);
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
      output.err("stopped\n");
      end(CANCEL_STATUS);
    };
    if (cell) process.on(signal, interrupt);
    // What the worker sent before the deadline is written first; a worker
    // that ended the run itself is not stopped.
    const timer = setTimeout(() => {
      for (let m; status === null && (m = receiveMessageOnPort(port));) take(m.message);
      if (status !== null) return end(status);
      worker.terminate();
      if (!reported) output.err(reportIn(fact, PROGRAM_TIME_LIMIT));
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
  const output = createOutput();
  // A supervised run shares the writer thread with its worker, and on
  // Windows every write uses it, so there it boots while the module
  // compiles. Otherwise writes go direct and a full pipe starts it.
  if (limit !== null || output.threaded()) output.start();
  try {
    process.exitCode =
      limit === null
        ? await runHere(bytes, config, args, output, null, cancel)
        : await supervise(await WebAssembly.compile(bytes), config, args, limit, output);
  } finally {
    await output.close();
  }
} else {
  const { module, config, args, deadline, port, cell, outputConnection } = workerData;
  const output = createOutput(outputConnection);
  const status = await runHere(
    module,
    config,
    args,
    {
      out: output.out,
      write: output.write,
      err: (text) => {
        output.err(text);
        port.postMessage({ fd: 2, text });
      },
      fact: (label) => port.postMessage({ fact: label }),
    },
    deadline,
    cell ? sharedCancel(config.cleanupMs, cell) : null,
  );
  port.postMessage({ status });
  port.close();
}
