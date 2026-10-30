// A page's handle on the compiler worker, shared by the playground and the
// website's tour, and on the run worker that runs what it builds. Requests
// run one at a time. A request that runs too long, or `stop()`, terminates
// the worker doing it and starts a fresh one: a program's run ends in the
// run worker, so the compiler worker keeps its warm store (#254).

import type { Project } from "./project.ts";
import type { RunMessage, RunRequest } from "./run-worker.ts";
import type { RunMode, RunResult, WatResult } from "./runner.ts";
import type { WorkerMessage, WorkerRequest } from "./worker.ts";

const TIME_LIMIT_MS = 15_000;

export type Interrupted = "stopped" | "timeout";

type Answer = Extract<WorkerMessage, { kind: "result" | "wat" | "compiled" }>;

interface Pending {
  readonly id: number;
  /** The worker doing the request: the compiler's, or the run worker. */
  readonly phase: "compile" | "run";
  readonly onStdout: (line: string, debug?: boolean) => void;
  readonly resolve: (answer: Answer | Interrupted) => void;
  readonly timer: ReturnType<typeof setTimeout>;
}

interface CompilerClientOptions {
  /** The worker script; defaults to `worker.js` beside this bundle. */
  readonly workerUrl?: string | URL;
  /** Called each time a worker has loaded the compiler. */
  readonly onReady?: () => void;
  /** The first WAT request is loading its optional printer. */
  readonly onWatLoading?: () => void;
  /** Wait for the first request, or `start()`, before loading the worker. */
  readonly lazy?: boolean;
}

export class CompilerClient {
  private worker: Worker | undefined;
  private ready: Promise<void> = Promise.resolve();
  private runner: Worker | undefined;
  private runnerReady: Promise<void> = Promise.resolve();
  private nextId = 1;
  private pending: Pending | undefined;
  private readonly options: CompilerClientOptions;
  /** A WAT request in progress; a run waits for it instead of stopping it. */
  private watRequest: Promise<unknown> | undefined;
  private runs = 0;

  constructor(options: CompilerClientOptions = {}) {
    this.options = options;
    if (!options.lazy) {
      this.start();
    }
  }

  get busy(): boolean {
    return this.pending !== undefined;
  }

  /** Whether a worker has been created. */
  get started(): boolean {
    return this.worker !== undefined;
  }

  /** Loads the worker if it is not loaded yet. */
  start(): void {
    if (this.worker) {
      return;
    }
    const url = this.workerUrl("worker.js");
    const worker = new Worker(url, { type: "module" });
    this.worker = worker;
    this.ready = new Promise((resolve, reject) => {
      worker.addEventListener(
        "error",
        (event) => reject(new Error(event.message || "the compiler worker failed to load")),
        { once: true },
      );
      worker.addEventListener("message", ({ data }: MessageEvent<WorkerMessage>) => {
        if (data.kind === "ready") {
          resolve();
          this.options.onReady?.();
        } else {
          this.deliver(data);
        }
      });
    });
  }

  /** The run worker's script, beside the compiler worker's. */
  private workerUrl(file: string): URL {
    const base = this.options.workerUrl ?? new URL("./worker.js", import.meta.url);
    return new URL(file, new URL(base, location.href));
  }

  /** Loads the run worker if it is not loaded yet. */
  private startRunner(): void {
    if (this.runner) {
      return;
    }
    const runner = new Worker(this.workerUrl("run-worker.js"), { type: "module" });
    this.runner = runner;
    this.runnerReady = new Promise((resolve, reject) => {
      runner.addEventListener(
        "error",
        (event) => reject(new Error(event.message || "the run worker failed to load")),
        { once: true },
      );
      runner.addEventListener("message", ({ data }: MessageEvent<RunMessage>) => {
        if (data.kind === "ready") {
          resolve();
        } else {
          this.deliver(data);
        }
      });
    });
  }

  /** A worker's answer or output for the request in progress. */
  private deliver(data: Exclude<WorkerMessage | RunMessage, { kind: "ready" }>): void {
    if (!("id" in data) || data.id !== this.pending?.id) {
      return;
    }
    if (data.kind === "wat-loading") {
      this.options.onWatLoading?.();
    } else if (data.kind === "stdout") {
      this.pending.onStdout(data.line, data.debug);
    } else if (data.kind === "result" || data.kind === "wat" || data.kind === "compiled") {
      this.settle(data);
    }
  }

  private settle(answer: Answer | Interrupted): void {
    const pending = this.pending;
    if (!pending) {
      return;
    }
    clearTimeout(pending.timer);
    this.pending = undefined;
    pending.resolve(answer);
  }

  /** Sends `request` to the worker of `phase`, with `limit` milliseconds left. */
  private async send(
    request: Extract<WorkerRequest, { kind: "project" | "wat" }> | RunRequest,
    onStdout: (line: string, debug?: boolean) => void = () => undefined,
    limit = TIME_LIMIT_MS,
  ): Promise<Answer | Interrupted> {
    const phase = request.kind === "run" ? "run" : "compile";
    if (phase === "run") {
      this.startRunner();
      await this.runnerReady;
    } else {
      this.start();
      await this.ready;
    }
    return new Promise((resolve) => {
      const timer = setTimeout(() => this.restart("timeout"), Math.max(limit, 0));
      this.pending = { id: request.id, phase, onStdout, resolve, timer };
      const worker = phase === "run" ? this.runner! : this.worker!;
      if (request.kind === "run") {
        worker.postMessage(request, [request.run.compiled.wasm.buffer as ArrayBuffer]);
      } else {
        worker.postMessage(request);
      }
    });
  }

  /** Runs or checks a playground project, stopping any request in progress. */
  async run(
    mode: RunMode,
    project: Project,
    onStdout: (line: string, debug?: boolean) => void,
  ): Promise<RunResult | Interrupted> {
    this.runs += 1;
    try {
      await this.watRequest;
      if (this.pending) {
        this.stop();
      }
      const deadline = performance.now() + TIME_LIMIT_MS;
      const id = this.nextId++;
      let answer = await this.send({ kind: "project", id, mode, project } as const, onStdout);
      // The compiler built a program: the run worker runs it.
      if (typeof answer !== "string" && answer.kind === "compiled") {
        answer = await this.send(
          { kind: "run", id, run: answer.run },
          onStdout,
          deadline - performance.now(),
        );
      }
      if (typeof answer === "string") {
        return answer;
      }
      if (answer.kind !== "result") {
        throw new Error(`unexpected ${answer.kind} answer`);
      }
      return answer.result;
    } finally {
      this.runs -= 1;
    }
  }

  /**
   * The WAT of the module `project` compiles to, or of the module its last run
   * compiled. Returns `busy` while another request is in progress rather than
   * stop it.
   */
  async wat(project: Project): Promise<WatResult | Interrupted | "busy"> {
    if (this.pending || this.runs > 0 || this.watRequest) {
      return "busy";
    }
    const request = this.send({ kind: "wat", id: this.nextId++, project });
    this.watRequest = request.catch(() => undefined);
    try {
      const answer = await request;
      if (typeof answer === "string") {
        return answer;
      }
      if (answer.kind !== "wat") {
        throw new Error(`unexpected ${answer.kind} answer`);
      }
      return answer.result;
    } finally {
      this.watRequest = undefined;
    }
  }

  stop(): void {
    this.restart("stopped");
  }

  /**
   * Ends the request in progress: a program's run ends with the run worker,
   * which loads again on the next run; a compile ends with the compiler
   * worker, which starts again now.
   */
  private restart(reason: Interrupted): void {
    if (this.pending?.phase === "run") {
      this.runner?.terminate();
      this.runner = undefined;
      this.settle(reason);
      return;
    }
    this.worker?.terminate();
    this.worker = undefined;
    this.settle(reason);
    this.start();
  }
}
