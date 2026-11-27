// Small driver primitives shared by the full host and the CLI's import-free
// startup path. Keep this module dependency-free: loading it is part of every
// program's fixed cost.

export class HostContract extends Error {}

// A worker thread that runs the program's Wasm gets a 16 MiB stack, as the
// main thread's V8 stack (`hd`'s `--stack-size`) and wasmtime's
// `max_wasm_stack` (#2396). Node sizes the thread and its V8 limit from it.
export const PROGRAM_STACK = { stackSizeMb: 16 };

export interface EntryCancel {
  boundMs: number;
  request(): void;
  requests(): number;
  wait(ms: number, signal?: AbortSignal): Promise<void>;
}

interface WorkerLike {
  threadId: number;
  once(event: "exit", listener: () => void): unknown;
}

export const PROGRAM_TIME_LIMIT = "panic: time-limit: the program exceeded its time limit\n";
export const CANCEL_STATUS = 130;

// Infinite waits have no timer. A Promise completion cancels a finite
// deadline rather than leaving one timer behind for each completed call.
export function completionDelay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    let finished = false;
    const done = () => {
      if (finished) {
        return;
      }
      finished = true;
      if (timer !== null) {
        clearTimeout(timer);
      }
      signal.removeEventListener("abort", done);
      resolve();
    };
    const timer = Number.isFinite(ms) ? setTimeout(done, ms) : null;
    signal.addEventListener("abort", done, { once: true });
    if (signal.aborted) {
      done();
    }
  });
}

export function entryCancel(
  boundMs: number,
  onWait: ((waiting: boolean) => void) | null = null,
): EntryCancel {
  let requests = 0;
  let wake: (() => void) | null = null;
  return {
    boundMs,
    request: () => {
      requests += 1;
      wake?.();
    },
    requests: () => requests,
    wait: (ms, signal) =>
      new Promise((resolve) => {
        let finished = false;
        const done = () => {
          if (finished) {
            return;
          }
          finished = true;
          if (timer !== null) {
            clearTimeout(timer);
          }
          signal?.removeEventListener("abort", done);
          wake = null;
          onWait?.(false);
          resolve();
        };
        const timer = Number.isFinite(ms) ? setTimeout(done, ms) : null;
        signal?.addEventListener("abort", done, { once: true });
        wake = done;
        onWait?.(true);
        if (signal?.aborted) {
          done();
        }
      }),
  };
}

export const factHead = (fact: string | null | undefined): string =>
  fact == null ? "" : `fact-evaluation-failed: the facts of \`${fact}\` panicked with `;

export const reportIn = (fact: string | null | undefined, report: string): string =>
  `panic: ${factHead(fact)}${report.slice("panic: ".length)}`;

const EXIT_GRACE_MS = 500;

export function exitPast(
  workers: readonly WorkerLike[],
  status = 0,
  statusFile: string | null = null,
): void {
  const running = workers.filter((worker) => worker.threadId !== -1);
  if (!running.length) {
    return;
  }
  let left = running.length;
  const timer = setTimeout(() => {
    if (statusFile) {
      process.getBuiltinModule("node:fs").writeFileSync(statusFile, `${status}\n`);
    }
    process.kill(process.pid, "SIGKILL");
  }, EXIT_GRACE_MS);
  for (const worker of running) {
    worker.once("exit", () => {
      if (--left === 0) {
        clearTimeout(timer);
      }
    });
  }
}
