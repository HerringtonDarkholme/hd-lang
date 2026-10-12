// The compiler worker: the compiler runs here so the page stays responsive,
// and a runaway program can be stopped by terminating the worker.
//
// Run, Check and Test go to the Rust compiler (rust-runner.ts), whose
// wasm, hd_web.wasm, sits beside this script; the worker loads it once and
// says it is ready after that. The WAT view's module comes from the same
// wasm (the wat goal).

import type { Project } from "./project.ts";
import type { RunMode, RunResult, WatResult } from "./runner.ts";
import { createRustRunner } from "./rust-runner.ts";

export type WorkerRequest =
  | {
      readonly kind: "project";
      readonly id: number;
      readonly mode: RunMode;
      readonly project: Project;
    }
  /** The WAT of the module `project` compiles to; replies `wat`. */
  | { readonly kind: "wat"; readonly id: number; readonly project: Project };

export type WorkerMessage =
  | { readonly kind: "ready" }
  | {
      readonly kind: "stdout";
      readonly id: number;
      readonly line: string;
      readonly debug?: boolean;
    }
  | { readonly kind: "result"; readonly id: number; readonly result: RunResult }
  | { readonly kind: "wat"; readonly id: number; readonly result: WatResult };

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

scope.onmessage = async ({ data: request }) => {
  if (request.kind === "project") {
    const { id, mode, project } = request;
    const onStdout = (line: string, debug?: boolean): void =>
      scope.postMessage({ kind: "stdout", id, line, ...(debug ? { debug } : {}) });
    const result = await rust.runProject(project, mode, onStdout);
    scope.postMessage({ kind: "result", id, result });
  } else {
    const result = await rust.watProject(request.project);
    scope.postMessage({ kind: "wat", id: request.id, result });
  }
};
void rust.ready.then(() => scope.postMessage({ kind: "ready" }));
