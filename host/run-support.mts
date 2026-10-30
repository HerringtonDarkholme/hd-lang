// Small driver primitives shared by the full host and the CLI's import-free
// startup path. Keep this module dependency-free: loading it is part of every
// program's fixed cost.

export class HostContract extends Error {}

export interface EntryCancel {
  boundMs: number;
  request(): void;
  requests(): number;
  wait(ms: number): Promise<void>;
}

interface WorkerLike {
  threadId: number;
  once(event: "exit", listener: () => void): unknown;
}

export const PROGRAM_TIME_LIMIT = "panic: time-limit: the program exceeded its time limit\n";
export const CANCEL_STATUS = 130;

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
    wait: (ms) =>
      new Promise((resolve) => {
        const done = () => {
          clearTimeout(timer);
          wake = null;
          onWait?.(false);
          resolve();
        };
        const timer = setTimeout(done, ms);
        wake = done;
        onWait?.(true);
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
