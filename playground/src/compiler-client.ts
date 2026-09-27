// The page's handle on the compiler worker. Requests run one at a time; a
// request that runs too long, or `stop()`, terminates the worker and starts a
// fresh one.

import type { Project } from "./project.ts";
import type { RunMode, RunResult } from "./runner.ts";
import type { WorkerMessage, WorkerRequest } from "./worker.ts";

export const TIME_LIMIT_MS = 15_000;

interface Pending {
  readonly id: number;
  readonly onStdout: (line: string) => void;
  readonly resolve: (result: RunResult | "stopped" | "timeout") => void;
  readonly timer: ReturnType<typeof setTimeout>;
}

export class CompilerClient {
  private worker!: Worker;
  private ready!: Promise<void>;
  private nextId = 1;
  private pending: Pending | undefined;
  private readonly onReady: () => void;

  constructor(onReady: () => void) {
    this.onReady = onReady;
    this.start();
  }

  get busy(): boolean {
    return this.pending !== undefined;
  }

  private start(): void {
    this.worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    this.ready = new Promise((resolve, reject) => {
      this.worker.addEventListener("error", (event) => reject(new Error(event.message)), {
        once: true,
      });
      this.worker.addEventListener("message", ({ data }: MessageEvent<WorkerMessage>) => {
        if (data.kind === "ready") {
          resolve();
          this.onReady();
        } else if (data.id === this.pending?.id) {
          if (data.kind === "stdout") this.pending.onStdout(data.line);
          else this.settle(data.result);
        }
      });
    });
  }

  private settle(result: RunResult | "stopped" | "timeout"): void {
    const pending = this.pending;
    if (!pending) return;
    clearTimeout(pending.timer);
    this.pending = undefined;
    pending.resolve(result);
  }

  async run(
    mode: RunMode,
    project: Project,
    onStdout: (line: string) => void,
  ): Promise<RunResult | "stopped" | "timeout"> {
    if (this.pending) this.stop();
    await this.ready;
    const id = this.nextId++;
    return new Promise((resolve) => {
      const timer = setTimeout(() => this.restart("timeout"), TIME_LIMIT_MS);
      this.pending = { id, onStdout, resolve, timer };
      const request: WorkerRequest = { id, mode, project };
      this.worker.postMessage(request);
    });
  }

  stop(): void {
    this.restart("stopped");
  }

  private restart(reason: "stopped" | "timeout"): void {
    this.worker.terminate();
    this.settle(reason);
    this.start();
  }
}
