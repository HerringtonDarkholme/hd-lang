// The in-process conformance adapter for this repository's compiler
// (spec/tools/README.md, Adapters). It runs each `hd` command line on a pool
// of worker threads that import the compiler once, instead of starting one
// process per command. A worker that runs past the timeout, or dies, is
// terminated and replaced, so a hung case cannot hang the run.
//
//   node --experimental-strip-types spec/tools/run-conformance.ts --adapter test/hd-adapter.ts

import { availableParallelism } from "node:os";
import { inspect } from "node:util";
import { Worker } from "node:worker_threads";

import type { AdapterRunnerOptions } from "./hd-in-process.ts";

/** What one command line did, as a spawned `hd` process would report it. */
export interface AdapterResult {
  /** The exit status; null when the command did not finish. */
  readonly status: number | null;
  readonly stdout: string;
  readonly stderr: string;
  readonly timedOut: boolean;
}

export interface Adapter {
  /**
   * Runs `hd ARGS...` as if started in `cwd` (the process's own directory when
   * unset), with the runner's `options` (never passed as `hd` flags); a run
   * longer than `timeoutMs` stops with `timedOut`.
   */
  run(
    args: readonly string[],
    timeoutMs: number,
    cwd?: string,
    options?: AdapterRunnerOptions,
  ): Promise<AdapterResult>;
  /** Terminates the workers. */
  close(): Promise<void>;
}

interface Reply {
  readonly status: number;
  readonly stdout: string;
  readonly stderr: string;
}

const workerUrl = new URL("./hd-adapter-worker.ts", import.meta.url);

/** Starts a worker and resolves once it has imported the compiler. */
function startWorker(): Promise<Worker> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(workerUrl);
    const fail = (error: unknown): void => reject(error);
    worker.once("error", fail);
    worker.once("message", () => {
      worker.off("error", fail);
      resolve(worker);
    });
  });
}

/** A pool of at most `jobs` workers; the default is min(8, CPUs). */
export function createAdapter(options: { readonly jobs?: number } = {}): Adapter {
  const jobs = options.jobs ?? Math.min(8, availableParallelism());
  const idle: Worker[] = [];
  const all = new Set<Worker>();
  const waiting: Array<(worker: Worker) => void> = [];
  let starting = 0;

  const grow = (): void => {
    starting += 1;
    void startWorker().then(
      (worker) => {
        starting -= 1;
        all.add(worker);
        release(worker);
      },
      (error: unknown) => {
        starting -= 1;
        throw error;
      },
    );
  };
  const acquire = (): Promise<Worker> => {
    const worker = idle.pop();
    if (worker) return Promise.resolve(worker);
    if (all.size + starting < jobs) grow();
    return new Promise((resolve) => waiting.push(resolve));
  };
  const release = (worker: Worker): void => {
    const next = waiting.shift();
    if (next) next(worker);
    else idle.push(worker);
  };
  const discard = (worker: Worker): void => {
    all.delete(worker);
    void worker.terminate();
    if (waiting.length > 0) grow();
  };

  return {
    async run(args, timeoutMs, cwd, options) {
      const worker = await acquire();
      return new Promise((resolve) => {
        const finish = (result: AdapterResult, keep: boolean): void => {
          clearTimeout(timer);
          worker.off("message", onMessage);
          worker.off("error", onError);
          worker.off("exit", onExit);
          if (keep) release(worker);
          else discard(worker);
          resolve(result);
        };
        const onMessage = (reply: Reply): void => finish({ ...reply, timedOut: false }, true);
        // An error that escapes the command ends a process with status 1.
        const onError = (error: unknown): void =>
          finish({ status: 1, stdout: "", stderr: `${inspect(error)}\n`, timedOut: false }, false);
        const onExit = (code: number): void =>
          finish({ status: code, stdout: "", stderr: "", timedOut: false }, false);
        const timer = setTimeout(
          () => finish({ status: null, stdout: "", stderr: "", timedOut: true }, false),
          timeoutMs,
        );
        worker.on("message", onMessage);
        worker.on("error", onError);
        worker.on("exit", onExit);
        worker.postMessage({ args, cwd, options });
      });
    },
    async close() {
      const workers = [...all];
      all.clear();
      idle.length = 0;
      await Promise.all(workers.map((worker) => worker.terminate()));
    },
  };
}
