// A page's handle on the compiler worker, shared by the playground and the
// website's tour. Requests run one at a time. A request that runs too long,
// or `stop()`, terminates the worker and starts a fresh one.

import type { Project } from "./project.ts";
import type { RunMode, RunResult, WatResult } from "./runner.ts";
import type { ReplReply, WorkerMessage, WorkerRequest } from "./worker.ts";

const TIME_LIMIT_MS = 15_000;

export type Interrupted = "stopped" | "timeout";

type Answer = Extract<WorkerMessage, { kind: "result" | "wat" | "repl" }>;

interface Pending {
  readonly id: number;
  readonly onStdout: (line: string, debug?: boolean) => void;
  readonly resolve: (answer: Answer | Interrupted) => void;
  readonly timer: ReturnType<typeof setTimeout>;
}

interface CompilerClientOptions {
  /** The worker script; defaults to `worker.js` beside this bundle. */
  readonly workerUrl?: string | URL;
  /** Called each time a worker has loaded the compiler. */
  readonly onReady?: () => void;
  /** Wait for the first request, or `start()`, before loading the worker. */
  readonly lazy?: boolean;
}

export class CompilerClient {
  private worker: Worker | undefined;
  private ready: Promise<void> = Promise.resolve();
  private nextId = 1;
  private pending: Pending | undefined;
  private readonly options: CompilerClientOptions;
  private replQueue: Promise<unknown> = Promise.resolve();
  /** A WAT request in progress; a run waits for it instead of stopping it. */
  private watRequest: Promise<unknown> | undefined;
  private runs = 0;

  constructor(options: CompilerClientOptions = {}) {
    this.options = options;
    if (!options.lazy) this.start();
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
    if (this.worker) return;
    const url = this.options.workerUrl ?? new URL("./worker.js", import.meta.url);
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
        } else if (data.id === this.pending?.id) {
          if (data.kind === "stdout") this.pending.onStdout(data.line, data.debug);
          else this.settle(data);
        }
      });
    });
  }

  private settle(answer: Answer | Interrupted): void {
    const pending = this.pending;
    if (!pending) return;
    clearTimeout(pending.timer);
    this.pending = undefined;
    pending.resolve(answer);
  }

  private async send(
    request: WorkerRequest,
    onStdout: (line: string, debug?: boolean) => void = () => undefined,
  ): Promise<Answer | Interrupted> {
    this.start();
    await this.ready;
    return new Promise((resolve) => {
      const timer = setTimeout(() => this.restart("timeout"), TIME_LIMIT_MS);
      this.pending = { id: request.id, onStdout, resolve, timer };
      this.worker!.postMessage(request);
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
      if (this.pending) this.stop();
      const request = { kind: "project", id: this.nextId++, mode, project } as const;
      const answer = await this.send(request, onStdout);
      if (typeof answer === "string") return answer;
      if (answer.kind !== "result") throw new Error(`unexpected ${answer.kind} answer`);
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
    if (this.pending || this.runs > 0 || this.watRequest) return "busy";
    const request = this.send({ kind: "wat", id: this.nextId++, project });
    this.watRequest = request.catch(() => undefined);
    try {
      const answer = await request;
      if (typeof answer === "string") return answer;
      if (answer.kind !== "wat") throw new Error(`unexpected ${answer.kind} answer`);
      return answer.result;
    } finally {
      this.watRequest = undefined;
    }
  }

  /** Sends one REPL input or `:` command, after any earlier one; the worker has no REPL until #17. */
  repl(input: string): Promise<ReplReply | Interrupted> {
    const next = this.replQueue.then(() => this.replNow(input));
    this.replQueue = next.catch(() => undefined);
    return next;
  }

  private async replNow(input: string): Promise<ReplReply | Interrupted> {
    const answer = await this.send({ kind: "repl", id: this.nextId++, input });
    if (typeof answer === "string") return answer;
    if (answer.kind !== "repl") throw new Error(`unexpected ${answer.kind} answer`);
    return answer.reply;
  }

  stop(): void {
    this.restart("stopped");
  }

  private restart(reason: Interrupted): void {
    this.worker?.terminate();
    this.worker = undefined;
    this.settle(reason);
    this.start();
  }
}
