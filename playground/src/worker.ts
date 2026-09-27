// The compiler worker: Binaryen and the prototype compiler run here so the
// page stays responsive, and a runaway program can be stopped by terminating
// the worker. The playground sends it projects; the website's REPL panel
// sends it REPL inputs, which one `ReplSession` per worker answers.

import { respond, ReplSession, type ReplReply } from "../../src/repl.ts";
import type { Project } from "./project.ts";
import { runProject, type RunMode, type RunResult } from "./runner.ts";

export type WorkerRequest =
  | {
      readonly kind: "project";
      readonly id: number;
      readonly mode: RunMode;
      readonly project: Project;
    }
  /** One complete REPL input or `:` command. */
  | { readonly kind: "repl"; readonly id: number; readonly input: string }
  /** Starts a fresh session from inputs a stopped worker had accepted; replies `restored`. */
  | { readonly kind: "repl-restore"; readonly id: number; readonly inputs: readonly string[] };

export type WorkerMessage =
  | { readonly kind: "ready" }
  | { readonly kind: "stdout"; readonly id: number; readonly line: string }
  | { readonly kind: "result"; readonly id: number; readonly result: RunResult }
  | { readonly kind: "repl"; readonly id: number; readonly reply: ReplReply }
  | { readonly kind: "restored"; readonly id: number; readonly count: number };

interface WorkerScope {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerMessage): void;
}

const scope = self as unknown as WorkerScope;
let session = new ReplSession();

scope.onmessage = async ({ data: request }) => {
  if (request.kind === "project") {
    const { id, mode, project } = request;
    const result = await runProject(project, mode, (line) =>
      scope.postMessage({ kind: "stdout", id, line }),
    );
    scope.postMessage({ kind: "result", id, result });
  } else if (request.kind === "repl") {
    const reply = await respond(session, request.input);
    scope.postMessage({ kind: "repl", id: request.id, reply });
  } else {
    session = new ReplSession();
    let count = 0;
    for (const input of request.inputs) if ((await session.evaluate(input)).accepted) count += 1;
    scope.postMessage({ kind: "restored", id: request.id, count });
  }
};
scope.postMessage({ kind: "ready" });
