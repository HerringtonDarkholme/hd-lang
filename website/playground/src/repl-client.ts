// The page's handle on a REPL session (repl-plan.md job W5). The session
// runs in a playground compiler worker (worker.ts, repl-host.ts): the
// compiler's REPL session and the JS session that runs its modules. The
// page numbers the inputs, as `hd` does, and keeps what a fresh worker
// needs to rebuild the session: the files it started over, the text of
// each input, and the journal the worker answers after each input.
//
// Stop first asks the worker to cancel the input, which ends an input that
// waits on the host at once (`cli.repl.cancel`). An input that computes
// never reads that message; after STOP_GRACE_MS the page terminates the
// worker, and `rebuild()` starts a fresh one and rebuilds the session from
// the journal (`cli.repl.rebuild`), without the stopped input.

import type {
  ReplAnswer,
  ReplFiles,
  ReplRan,
  ReplRebuilt,
  ReplRequest,
  ReplStarted,
} from "./repl-host.ts";
import type { WorkerMessage } from "./worker.ts";

/** How long a stopped input may take to end before the worker is terminated. */
export const STOP_GRACE_MS = 250;

export interface ReplSink {
  out(text: string): void;
  err(text: string): void;
}

/** An input the page stopped by terminating the worker. */
export interface ReplStopped {
  readonly type: "stopped";
  readonly number: number;
}

interface Pending {
  readonly sink: ReplSink | undefined;
  readonly resolve: (answer: ReplAnswer | ReplStopped) => void;
}

interface ReplClientOptions {
  /** The worker script; defaults to `worker.js` beside this bundle. */
  readonly workerUrl?: string | URL;
  /** Called each time a worker has loaded the compiler. */
  readonly onReady?: () => void;
}

type Sent = ReplRequest extends infer R
  ? R extends { id: number }
    ? Omit<R, "id">
    : never
  : never;

export class ReplClient {
  private worker: Worker | undefined;
  private ready: Promise<void> = Promise.resolve();
  private readonly options: ReplClientOptions;
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  /** The input that runs: its request and number. */
  private running: { id: number; number: number } | undefined;
  private stopTimer: ReturnType<typeof setTimeout> | undefined;
  /** What a rebuild needs: the session's files, each input's text, the journal. */
  private files: ReplFiles = {};
  private readonly sources = new Map<number, string>();
  private journal: Uint8Array | null = null;
  /** Whether the worker holds a session, and whether a terminated one needs a rebuild. */
  private live = false;
  private lost = false;
  private next = 1;

  constructor(options: ReplClientOptions = {}) {
    this.options = options;
  }

  /** Whether a session runs, or a stopped one waits for `rebuild()`. */
  get started(): boolean {
    return this.live || this.lost;
  }

  /** Whether the session was lost with its worker and needs `rebuild()`. */
  get needsRebuild(): boolean {
    return this.lost;
  }

  /** The number the next input gets: `repl#N`. */
  get nextNumber(): number {
    return this.next;
  }

  get busy(): boolean {
    return this.running !== undefined;
  }

  /** Loads the worker if it is not loaded yet. */
  load(): void {
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
        } else if (data.kind === "repl-out") {
          const sink = this.pending.get(data.id)?.sink;
          if (data.stream === "out") sink?.out(data.text);
          else sink?.err(data.text);
        } else if (data.kind === "repl") {
          const pending = this.pending.get(data.id);
          this.pending.delete(data.id);
          pending?.resolve(data.answer);
        }
      });
    });
  }

  private async send(request: Sent, sink?: ReplSink): Promise<ReplAnswer | ReplStopped> {
    this.load();
    await this.ready;
    const id = this.nextId++;
    return new Promise((resolve) => {
      this.pending.set(id, { sink, resolve });
      if (request.kind === "repl-input") this.running = { id, number: request.number };
      this.worker!.postMessage({ ...request, id });
    });
  }

  private keep(journal: Uint8Array | null): void {
    this.journal = journal;
  }

  /** Starts a session over `files` (its package), dropping any earlier one. */
  async start(files: ReplFiles): Promise<ReplStarted> {
    this.reset();
    this.files = files;
    const answer = (await this.send({ kind: "repl-start", files })) as ReplStarted;
    this.live = answer.ok;
    this.keep(answer.journal);
    if (answer.ok) this.sources.set(0, "");
    return answer;
  }

  /** Builds and runs `source` as the next input; its output streams to `sink`. */
  async input(source: string, sink: ReplSink): Promise<ReplRan | ReplStopped> {
    const number = this.next++;
    this.sources.set(number, source);
    try {
      const answer = await this.send({ kind: "repl-input", number, source }, sink);
      if (answer.type === "ran") this.keep(answer.journal);
      return answer as ReplRan | ReplStopped;
    } finally {
      clearTimeout(this.stopTimer);
      this.stopTimer = undefined;
      this.running = undefined;
    }
  }

  /**
   * Stops the input that runs: a cancel first; if it has not ended after
   * STOP_GRACE_MS, terminates the worker, and the input answers `stopped`.
   */
  stop(): void {
    const running = this.running;
    if (!running || this.stopTimer !== undefined) return;
    this.worker?.postMessage({ kind: "repl-stop" } satisfies ReplRequest);
    this.stopTimer = setTimeout(() => {
      this.stopTimer = undefined;
      if (this.running !== running) return;
      this.terminate();
      this.lost = true;
      this.pending.get(running.id)?.resolve({ type: "stopped", number: running.number });
      this.pending.delete(running.id);
    }, STOP_GRACE_MS);
  }

  /**
   * Rebuilds the session from its journal in the worker, a fresh one after
   * a stop: over `files` for `:reload`, else over the files it started
   * over. The inputs a rebuild skips or drops are forgotten.
   */
  async rebuild(files: ReplFiles = this.files): Promise<ReplRebuilt> {
    this.files = files;
    const answer = (await this.send({
      kind: "repl-rebuild",
      files,
      journal: this.journal,
      sources: [...this.sources],
    })) as ReplRebuilt;
    this.lost = false;
    this.live = answer.ok;
    this.keep(answer.journal);
    const kept = new Set(answer.kept);
    // Deleting from a Map while iterating its keys is safe.
    for (const number of this.sources.keys()) if (!kept.has(number)) this.sources.delete(number);
    if (!answer.ok) this.reset();
    return answer;
  }

  /** Ends the session; the next `start` begins a new one, numbered from 1. */
  async quit(): Promise<void> {
    if (this.worker && !this.lost) await this.send({ kind: "repl-quit" });
    this.reset();
  }

  /** Forgets the session. */
  private reset(): void {
    this.live = false;
    this.lost = false;
    this.sources.clear();
    this.journal = null;
    this.next = 1;
  }

  /** Terminates the worker; every request in flight is left unanswered. */
  private terminate(): void {
    this.worker?.terminate();
    this.worker = undefined;
    this.live = false;
    clearTimeout(this.stopTimer);
    this.stopTimer = undefined;
  }
}
