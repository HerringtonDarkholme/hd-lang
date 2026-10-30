// The worker side of the REPL: the compiler's REPL session (the REPL goals
// over the compiler wasm) and the JS session that runs its programs
// (host/core.mts `createSession`) live in one worker, the playground's
// compiler worker. The page numbers the inputs; each input is built under
// its number (`encodeReplInput(text, number)`), so diagnostics, `dbg` lines
// and the journal all name `repl#N` as the page shows it. Each input's
// build imports the initialized modules of completed inputs and runs
// only its own entry. Historical rebuilds compile a fresh linked base,
// replay the retained call history and then adopt that initialized base.
//
// The protocol, page to worker:
//
// - `repl-start` starts a session over `files` (its package: `src/lib.hd`
//   when present). It answers `started`.
// - `repl-input` builds and runs input `number`. Its output streams as
//   `repl-out` messages while it runs; it answers `ran`, with the session's
//   journal so far, which the page keeps for a rebuild in a fresh worker.
// - `repl-stop` cancels the input that waits on the host (`cli.repl.cancel`):
//   its `defer` suites run and it answers `ran`, ended `cancelled`. An input
//   that computes never reads the message; the page then terminates this
//   worker and sends `repl-rebuild` to a fresh one.
// - `repl-rebuild` starts a fresh compiler session over `files` and a fresh
//   JS session, and rebuilds it from `journal` (`cli.repl.rebuild`): each
//   input is built again from its text in `sources` under its old number
//   and the last one's program replays the journal with every host call
//   served from it. It answers `rebuilt` with what it kept, skipped and
//   dropped.
// - `repl-quit` ends the session; the next input needs a `repl-start`.

import { recorded, type End } from "../../../host/journal.mts";
import type { InputOutcome, Ran } from "../../../host/core.mts";
import { createWebSession, type Session } from "../../../host/web.mjs";
import {
  OUTCOME,
  STATUS,
  compileRequest,
  encodeReplInput,
  encodeReplRebuildInput,
  encodeReplOutcome,
  encodeReplStart,
  type Compiled,
  type CompilerExports,
  type WireDiagnostic,
} from "./compiler-wire.ts";

export type ReplFiles = Readonly<Record<string, string>>;

export type ReplRequest =
  | { readonly kind: "repl-start"; readonly id: number; readonly files: ReplFiles }
  | {
      readonly kind: "repl-input";
      readonly id: number;
      readonly number: number;
      readonly source: string;
    }
  | { readonly kind: "repl-stop" }
  | {
      readonly kind: "repl-rebuild";
      readonly id: number;
      readonly files: ReplFiles;
      /** The journal of the session to rebuild; null once it passed its cap. */
      readonly journal: Uint8Array | null;
      /** The text of each input the journal holds, by number. */
      readonly sources: readonly (readonly [number, string])[];
    }
  | { readonly kind: "repl-quit"; readonly id: number };

/** A session started. */
export interface ReplStarted {
  readonly type: "started";
  readonly ok: boolean;
  /** The package's diagnostics, as `hd` prints them. */
  readonly diagnostics: readonly string[];
  readonly error: string | null;
  readonly journal: Uint8Array | null;
}

/** How one input ended. */
export interface ReplRan {
  readonly type: "ran";
  readonly number: number;
  /** Its diagnostics, as `hd` prints them: `error: repl#3:1:5: code: message`. */
  readonly diagnostics: readonly string[];
  /** Its `End` row; null when it did not build or could not run. */
  readonly end: End | null;
  readonly joined: boolean;
  /** Why it could not run, or null. */
  readonly error: string | null;
  /** The session's journal after it; null once the journal passed its cap. */
  readonly journal: Uint8Array | null;
  /** True once, after the input in which the journal passed its cap. */
  readonly journalFull: boolean;
}

/** What a rebuild kept, skipped and dropped, by input number. */
export interface ReplRebuilt {
  readonly type: "rebuilt";
  /** Whether a session runs. */
  readonly ok: boolean;
  readonly kept: readonly number[];
  readonly skipped: readonly number[];
  readonly dropped: readonly number[];
  readonly reason: string | null;
  /** The diagnostics of the package, or of the first input that no longer builds. */
  readonly diagnostics: readonly string[];
  readonly error: string | null;
  readonly journal: Uint8Array | null;
}

export interface ReplEnded {
  readonly type: "ended";
}

export type ReplAnswer = ReplStarted | ReplRan | ReplRebuilt | ReplEnded;

export type ReplMessage =
  | {
      readonly kind: "repl-out";
      readonly id: number;
      readonly stream: "out" | "err";
      readonly text: string;
    }
  | { readonly kind: "repl"; readonly id: number; readonly answer: ReplAnswer };

export type ReplPost = (message: ReplMessage, transfer?: Transferable[]) => void;

/** One diagnostic as `hd` prints it: `error: repl#3:1:5: unknown-name: ...`. */
export function diagnosticLine(d: WireDiagnostic): string {
  const where =
    d.path === null ? "" : d.line === null ? `${d.path}: ` : `${d.path}:${d.line}:${d.column}: `;
  return `${d.severity}: ${where}${d.code === null ? "" : `${d.code}: `}${d.message}`;
}

const lines = (compiled: Compiled): string[] =>
  (compiled.json.diagnostics ?? []).map(diagnosticLine);

/** A link error of the session, in the words Run uses for a trait the browser lacks. */
function runError(number: number, error: string): string {
  const trait = /the import `hd:(\w+)`/.exec(error)?.[1];
  if (trait && !["rt", "prim"].includes(trait)) {
    return `repl#${number} needs ${trait}, which the playground does not provide`;
  }
  return `repl#${number}: ${error}`;
}

/** The outcome byte of an input that ran as `ran` (`OUTCOME`). */
function outcomeOf(ran: InputOutcome | null): number {
  if (ran === null || ran.end.kind === "stopped" || ran.end.kind === "cancelled") {
    return OUTCOME.dropped;
  }
  return ran.joined ? OUTCOME.joined : OUTCOME.failed;
}

/** The REPL of one worker over the compiler wasm `compiler`; `post` sends to the page. */
export function createReplHost(compiler: Promise<CompilerExports>, post: ReplPost) {
  let session: Session | null = null;
  // The text of each input the session keeps, by number.
  let texts = new Map<number, string>();
  // The request whose input runs; the session's output goes to it.
  let current = 0;
  const sink = {
    out: (text: string) => post({ kind: "repl-out", id: current, stream: "out", text }),
    err: (text: string) => post({ kind: "repl-out", id: current, stream: "err", text }),
  };
  const fresh = (): Session => {
    session?.close();
    // `cli.repl.host.args`: no program name and no arguments.
    return createWebSession(sink, { args: [], program: "" });
  };
  const answer = (id: number, value: ReplAnswer): void => {
    const journal = "journal" in value ? value.journal : null;
    post({ kind: "repl", id, answer: value }, journal ? [journal.buffer as ArrayBuffer] : []);
  };
  const outcome = (exports: CompilerExports, input: number, value: number): void => {
    compileRequest(exports, encodeReplOutcome(input, value));
  };
  const message = (error: unknown): string =>
    error instanceof Error ? error.message : String(error);

  async function start(id: number, files: ReplFiles): Promise<ReplStarted> {
    const failed = (diagnostics: readonly string[], error: string | null): ReplStarted => {
      session?.close();
      session = null;
      return { type: "started", ok: false, diagnostics, error, journal: null };
    };
    const exports = await compiler;
    const started = compileRequest(exports, encodeReplStart(files));
    if (started.status === STATUS.malformed) {
      return failed([], started.json.error ?? "refused");
    }
    if (started.status !== STATUS.ok) {
      return failed(lines(started), "the session's package does not build");
    }
    session = fresh();
    texts = new Map();
    current = id;
    return {
      type: "started",
      ok: true,
      diagnostics: lines(started),
      error: null,
      journal: session.journal(),
    };
  }

  async function input(id: number, number: number, source: string): Promise<ReplRan> {
    const ran = (fields: Partial<ReplRan>): ReplRan => ({
      type: "ran",
      number,
      diagnostics: [],
      end: null,
      joined: false,
      error: null,
      journal: session?.journal() ?? null,
      journalFull: false,
      ...fields,
    });
    if (!session) {
      return ran({ error: "no session: start one first" });
    }
    const exports = await compiler;
    const compiled = compileRequest(exports, encodeReplInput(source, number));
    if (compiled.status === STATUS.malformed) {
      return ran({ error: compiled.json.error ?? "refused" });
    }
    const diagnostics = lines(compiled);
    if (compiled.status !== STATUS.ok || compiled.wasm.length === 0) {
      return ran({ diagnostics });
    }
    current = id;
    let result;
    try {
      result = await session.run(compiled.wasm, [...texts, [number, source]], number);
    } catch (error) {
      outcome(exports, number, OUTCOME.dropped);
      return ran({ diagnostics, error: runError(number, message(error)) });
    }
    for (const n of [...result.skipped, ...result.dropped]) {
      texts.delete(n);
    }
    const live = result.live;
    const value = outcomeOf(live);
    outcome(exports, number, value);
    if (live?.end.kind === "cancelled" || live?.end.kind === "stopped") {
      sink.err(
        `note: repl#${number} was ${live.end.kind}, so the next rebuild skips it; its changes are not restored\n`,
      );
    }
    if (value !== OUTCOME.dropped) {
      texts.set(number, source);
    }
    return ran({
      diagnostics,
      end: live?.end ?? null,
      joined: live?.joined ?? false,
      error: live?.error ? runError(number, live.error) : null,
      journalFull: session.takeJournalNotice(),
    });
  }

  async function rebuild(
    id: number,
    files: ReplFiles,
    journal: Uint8Array | null,
    sources: readonly (readonly [number, string])[],
  ): Promise<ReplRebuilt> {
    const given = new Map(sources);
    const done = (fields: Partial<ReplRebuilt>): ReplRebuilt => ({
      type: "rebuilt",
      ok: false,
      kept: [],
      skipped: [],
      dropped: [],
      reason: null,
      diagnostics: [],
      error: null,
      journal: null,
      ...fields,
    });
    session?.close();
    session = null;
    texts = new Map();
    if (journal === null) {
      return done({
        error: "the session's record of host calls is full, so it cannot be rebuilt",
      });
    }
    const exports = await compiler;
    const record = recorded(journal);
    // Starts the compiler's session again and builds `inputs` in order,
    // reporting each one's outcome as its journal row says: the last one's
    // program, the inputs that built, and the first that did not with its
    // diagnostics.
    const build = (inputs: readonly { number: number; end: End | null }[]) => {
      compileRequest(exports, encodeReplStart(files));
      let wasm: Uint8Array | null = null;
      const built = new Map<number, string>();
      for (const i of inputs) {
        const source = given.get(i.number);
        const compiled =
          source === undefined
            ? null
            : compileRequest(exports, encodeReplRebuildInput(source, i.number));
        if (
          source === undefined ||
          compiled === null ||
          compiled.status !== STATUS.ok ||
          compiled.wasm.length === 0
        ) {
          return { wasm, built, cut: i.number, diagnostics: compiled ? lines(compiled) : [] };
        }
        wasm = compiled.wasm;
        built.set(i.number, source);
        const joined = i.end?.kind === "exit" && i.end.status === 0;
        outcome(exports, i.number, joined ? OUTCOME.joined : OUTCOME.failed);
      }
      return { wasm, built, cut: null, diagnostics: [] as string[] };
    };
    const started = compileRequest(exports, encodeReplStart(files));
    if (started.status !== STATUS.ok) {
      return done({
        diagnostics: lines(started),
        error: started.json.error ?? "the session's package does not build",
      });
    }
    const replayable = record.inputs.filter(
      (i) => i.end?.kind !== "stopped" && i.end?.kind !== "cancelled",
    );
    let { wasm, built, cut, diagnostics } = build(replayable);
    const next = fresh();
    current = id;
    let rebuilt: Ran = { kept: [], skipped: [], dropped: [], reason: null, live: null };
    if (wasm !== null) {
      try {
        rebuilt = await next.run(wasm, built, null, journal);
      } catch (error) {
        next.close();
        return done({ error: message(error) });
      }
    } else {
      rebuilt.skipped = record.inputs.filter((i) => !replayable.includes(i)).map((i) => i.number);
    }
    if (cut !== null && (rebuilt.dropped.length === 0 || rebuilt.dropped[0] >= cut)) {
      rebuilt.dropped = replayable.filter((i) => i.number >= cut!).map((i) => i.number);
      rebuilt.reason = `repl#${cut} no longer builds`;
    } else {
      diagnostics = [];
    }
    // Publish exactly the initialized prefix after a divergence or compile
    // cut: rebuild its code ABI and adopt its replayed base before appending.
    if (rebuilt.dropped.length > 0) {
      const prefix = build(replayable.filter((i) => rebuilt.kept.includes(i.number)));
      built = prefix.built;
      if (prefix.wasm !== null) {
        try {
          await next.run(prefix.wasm, built, null);
        } catch (error) {
          next.close();
          return done({ error: message(error) });
        }
      } else {
        next.clear();
      }
    }
    texts = new Map([...built].filter(([n]) => rebuilt.kept.includes(n)));
    session = next;
    return done({
      kept: rebuilt.kept,
      skipped: rebuilt.skipped,
      dropped: rebuilt.dropped,
      reason: rebuilt.reason,
      ok: true,
      diagnostics,
      error: null,
      journal: next.journal(),
    });
  }

  /** Handles one REPL request; `repl-stop` answers nothing itself. */
  async function handle(request: ReplRequest): Promise<void> {
    switch (request.kind) {
      case "repl-stop":
        session?.stop();
        return;
      case "repl-start":
        answer(request.id, await start(request.id, request.files));
        return;
      case "repl-input":
        answer(request.id, await input(request.id, request.number, request.source));
        return;
      case "repl-rebuild": {
        const { id, files, journal, sources } = request;
        answer(id, await rebuild(id, files, journal, sources));
        return;
      }
      case "repl-quit":
        session?.close();
        session = null;
        answer(request.id, { type: "ended" });
    }
  }

  return { handle };
}
