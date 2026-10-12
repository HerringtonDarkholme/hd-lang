// The compiler worker: the compilers run here so the page stays responsive,
// and a runaway program can be stopped by terminating the worker. The
// playground sends it projects; the website's REPL panel sends it REPL
// inputs, which one `ReplSession` per worker answers.
//
// Run and Check go to the Rust compiler (rust-runner.ts), whose wasm,
// hd_web.wasm, sits beside this script; the worker loads it once and says
// it is ready after that. Test, the WAT view and the REPL stay on the
// prototype compiler (runner.ts) for now.
//
// The worker keeps the module the last test compiled, so the WAT view can
// show it without compiling again. A Run on the Rust compiler keeps none:
// the view compiles the project with the prototype compiler until W8.

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
import { createRustRunner } from "./rust-runner.ts";

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
const rust = createRustRunner(
  fetch(new URL("./hd_web.wasm", import.meta.url)).then((response) => {
    if (!response.ok) throw new Error(`hd_web.wasm: HTTP ${response.status}`);
    return response.arrayBuffer();
  }),
);
let session = new ReplSession({}, undefined, PLAYGROUND_HOST);
/** What the WAT view shows for the project the last test compiled. */
let lastRun: { readonly key: string; readonly wat: WatResult } | undefined;

const projectKey = (project: Project): string =>
  JSON.stringify([
    project.main,
    Object.entries(project.files).sort(([a], [b]) => (a < b ? -1 : 1)),
  ]);

scope.onmessage = async ({ data: request }) => {
  if (request.kind === "project") {
    const { id, mode, project } = request;
    const onStdout = (line: string, debug?: boolean): void =>
      scope.postMessage({ kind: "stdout", id, line, ...(debug ? { debug } : {}) });
    if (mode !== "test") {
      const result = await rust.runProject(project, mode, onStdout);
      scope.postMessage({ kind: "result", id, result });
      return;
    }
    let module: CompiledModule | undefined;
    const result = await runProject(project, mode, onStdout, (compiled) => (module = compiled));
    const wat = watFromRun(result, module);
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
void rust.ready.then(() => scope.postMessage({ kind: "ready" }));
