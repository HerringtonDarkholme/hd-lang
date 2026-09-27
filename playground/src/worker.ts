// The compiler worker: Binaryen and the prototype compiler run here so the
// page stays responsive, and a runaway program can be stopped by terminating
// the worker.

import type { Project } from "./project.ts";
import { runProject, type RunMode, type RunResult } from "./runner.ts";

export interface WorkerRequest {
  readonly id: number;
  readonly mode: RunMode;
  readonly project: Project;
}

export type WorkerMessage =
  | { readonly kind: "ready" }
  | { readonly kind: "stdout"; readonly id: number; readonly line: string }
  | { readonly kind: "result"; readonly id: number; readonly result: RunResult };

interface WorkerScope {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerMessage): void;
}

const scope = self as unknown as WorkerScope;

scope.onmessage = async ({ data: { id, mode, project } }) => {
  const result = await runProject(project, mode, (line) =>
    scope.postMessage({ kind: "stdout", id, line }),
  );
  scope.postMessage({ kind: "result", id, result });
};
scope.postMessage({ kind: "ready" });
