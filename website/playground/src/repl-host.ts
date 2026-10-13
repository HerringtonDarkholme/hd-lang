// The worker side of the REPL (repl-plan.md job W5): the compiler's REPL
// session (W3's goals over the compiler wasm) and the JS session that runs
// its modules (host/core.mjs `createSession`, W4) live in one worker, the
// playground's compiler worker. The page numbers the inputs; each input is
// built under its number (`encodeReplInput(text, number)`), so diagnostics,
// `dbg` lines and the journal all name `repl#N` as the page shows it.
//
// The protocol, page to worker:
//
// - `repl-start` starts a session over `files` (its package: `src/lib.hd`
//   when present) and runs its prelude, input 0. It answers `started`.
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
//   and replayed with every host call served from the journal. It answers
//   `rebuilt` with what it kept, skipped and dropped.
// - `repl-quit` ends the session; the next input needs a `repl-start`.

import type { End } from "../../../host/journal.mjs";
import { createWebSession, type Session } from "../../../host/web.mjs";
import {
  OUTCOME,
  STATUS,
  compileRequest,
  encodeReplInput,
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

/** A session started: its prelude built and ran. */
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
  /** Whether a session runs: the prelude built and replayed. */
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
  if (trait && !["rt", "prim"].includes(trait))
    return `repl#${number} needs ${trait}, which the playground does not provide`;
  return `repl#${number}: ${error}`;
}

/** The REPL of one worker over the compiler wasm `compiler`; `post` sends to the page. */
export function createReplHost(compiler: Promise<CompilerExports>, post: ReplPost) {
  let session: Session | null = null;
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
  const outcome = (exports: CompilerExports, input: number, joined: boolean): void => {
    compileRequest(exports, encodeReplOutcome(input, joined ? OUTCOME.joined : OUTCOME.dropped));
  };

  async function start(id: number, files: ReplFiles): Promise<ReplStarted> {
    const failed = (diagnostics: readonly string[], error: string | null): ReplStarted => {
      session?.close();
      session = null;
      return { type: "started", ok: false, diagnostics, error, journal: null };
    };
    const exports = await compiler;
    const prelude = compileRequest(exports, encodeReplStart(files));
    if (prelude.status === STATUS.malformed) return failed([], prelude.json.error ?? "refused");
    if (prelude.status !== STATUS.ok || prelude.wasm.length === 0)
      return failed(lines(prelude), "the session's package does not build");
    session = fresh();
    current = id;
    const ran = await session.input(0, "", prelude.wasm);
    outcome(exports, 0, ran.joined);
    if (!ran.joined)
      return failed(lines(prelude), `the session's prelude: ${ran.error ?? "failed"}`);
    return {
      type: "started",
      ok: true,
      diagnostics: lines(prelude),
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
    if (!session) return ran({ error: "no session: start one first" });
    const exports = await compiler;
    const compiled = compileRequest(exports, encodeReplInput(source, number));
    if (compiled.status === STATUS.malformed)
      return ran({ error: compiled.json.error ?? "refused" });
    const diagnostics = lines(compiled);
    if (compiled.status !== STATUS.ok || compiled.wasm.length === 0) return ran({ diagnostics });
    current = id;
    const result = await session.input(number, source, compiled.wasm);
    outcome(exports, number, result.joined);
    return ran({
      diagnostics,
      end: result.end,
      joined: result.joined,
      error: result.error ? runError(number, result.error) : null,
      journalFull: session.takeJournalNotice(),
    });
  }

  async function rebuild(
    id: number,
    files: ReplFiles,
    journal: Uint8Array | null,
    sources: readonly (readonly [number, string])[],
  ): Promise<ReplRebuilt> {
    const texts = new Map(sources);
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
    if (journal === null)
      return done({
        error: "the session's record of host calls is full, so it cannot be rebuilt",
      });
    const exports = await compiler;
    const prelude = compileRequest(exports, encodeReplStart(files));
    if (prelude.status !== STATUS.ok || prelude.wasm.length === 0)
      return done({
        diagnostics: lines(prelude),
        error: prelude.json.error ?? "the session's package does not build",
      });
    const next = fresh();
    current = id;
    // The module the compiler built that waits for its outcome, and the
    // diagnostics of the input that no longer builds.
    let waiting: number | null = null;
    let diagnostics: string[] = [];
    const rebuilder = {
      build(number: number) {
        if (number === 0) {
          waiting = 0;
          return { source: "", wasm: prelude.wasm };
        }
        const source = texts.get(number);
        if (source === undefined) throw new Error(`the text of repl#${number} is gone`);
        const compiled = compileRequest(exports, encodeReplInput(source, number));
        if (compiled.status === STATUS.malformed) throw new Error(compiled.json.error);
        if (compiled.status !== STATUS.ok || compiled.wasm.length === 0) {
          diagnostics = lines(compiled);
          throw new Error(`repl#${number} no longer builds`);
        }
        waiting = number;
        return { source, wasm: compiled.wasm };
      },
      replayed(_number: number, joined: boolean) {
        if (waiting !== null) outcome(exports, waiting, joined);
        waiting = null;
      },
    };
    let rebuilt;
    try {
      rebuilt = await next.rebuild(rebuilder, journal);
    } catch (error) {
      next.close();
      return done({ error: error instanceof Error ? error.message : String(error) });
    }
    // An input that built but did not replay adds nothing.
    if (waiting !== null) outcome(exports, waiting, false);
    const ok = rebuilt.kept.includes(0);
    if (ok) session = next;
    else next.close();
    return done({
      ...rebuilt,
      ok,
      diagnostics,
      error: ok ? null : "the session's prelude did not replay",
      journal: ok ? next.journal() : null,
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
