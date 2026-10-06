// The compiler worker: Binaryen and the prototype compiler run here so the
// page stays responsive, and a runaway program can be stopped by terminating
// the worker. The playground sends it projects; the website's REPL panel
// sends it REPL inputs, which one `ReplSession` per worker answers.
//
// The worker keeps the module the last run or test compiled, so the WAT view
// can show it without compiling again. For top-level code without `main`,
// that module is the only one the view can show.

import { respond, ReplSession, type ReplReply } from "../../../src/repl.ts";
import type { Project } from "./project.ts";
import {
  PLAYGROUND_HOST,
  runProject,
  watFromRun,
  watProject,
  type CompiledModule,
  type RunMode,
  type RunResult,
  type WatResult,
} from "./runner.ts";

export type WorkerRequest =
  | {
      readonly kind: "project";
      readonly id: number;
      readonly mode: RunMode;
      readonly project: Project;
    }
  /** The WAT of the module `project` compiles to; replies `wat`. */
  | { readonly kind: "wat"; readonly id: number; readonly project: Project }
  /** One complete REPL input or `:` command. */
  | { readonly kind: "repl"; readonly id: number; readonly input: string }
  /** Starts a fresh session from inputs a stopped worker had accepted; replies `restored`. */
  | { readonly kind: "repl-restore"; readonly id: number; readonly inputs: readonly string[] };

export type WorkerMessage =
  | { readonly kind: "ready" }
  | {
      readonly kind: "stdout";
      readonly id: number;
      readonly line: string;
      readonly debug?: boolean;
    }
  | { readonly kind: "result"; readonly id: number; readonly result: RunResult }
  | { readonly kind: "wat"; readonly id: number; readonly result: WatResult }
  | { readonly kind: "repl"; readonly id: number; readonly reply: ReplReply }
  | { readonly kind: "restored"; readonly id: number; readonly count: number };

interface WorkerScope {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerMessage): void;
}

const scope = self as unknown as WorkerScope;
let session = new ReplSession({}, undefined, PLAYGROUND_HOST);
/** What the WAT view shows for the project the last run or test compiled. */
let lastRun: { readonly key: string; readonly wat: WatResult } | undefined;

const projectKey = (project: Project): string =>
  JSON.stringify([
    project.main,
    Object.entries(project.files).sort(([a], [b]) => (a < b ? -1 : 1)),
  ]);

scope.onmessage = async ({ data: request }) => {
  if (request.kind === "project") {
    const { id, mode, project } = request;
    let module: CompiledModule | undefined;
    const result = await runProject(
      project,
      mode,
      (line, debug) => scope.postMessage({ kind: "stdout", id, line, ...(debug ? { debug } : {}) }),
      (compiled) => (module = compiled),
    );
    const wat = mode === "check" ? undefined : watFromRun(result, module);
    if (wat) lastRun = { key: projectKey(project), wat };
    scope.postMessage({ kind: "result", id, result });
  } else if (request.kind === "wat") {
    const key = projectKey(request.project);
    const result = lastRun?.key === key ? lastRun.wat : await watProject(request.project);
    scope.postMessage({ kind: "wat", id: request.id, result });
  } else if (request.kind === "repl") {
    const reply = await respond(session, request.input);
    scope.postMessage({ kind: "repl", id: request.id, reply });
  } else {
    session = new ReplSession({}, undefined, PLAYGROUND_HOST);
    let count = 0;
    for (const input of request.inputs) if ((await session.evaluate(input)).accepted) count += 1;
    scope.postMessage({ kind: "restored", id: request.id, count });
  }
};
scope.postMessage({ kind: "ready" });
