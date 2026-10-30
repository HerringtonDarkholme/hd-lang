// The compiler worker: the compiler runs here so the page stays responsive.
// It runs no program: Run and Test answer with what they built
// (`compiled`), which the run worker (run-worker.ts) runs, so stopping a
// runaway program leaves this worker and its warm store.
//
// Run, Check and Test go to the Rust compiler (rust-runner.ts), whose
// wasm, hd_web.wasm, sits beside this script; the worker loads it once and
// says it is ready after that. Only a WAT request loads hd_wat.wasm,
// which prints the compiler's program bytes. REPL requests go to repl-host.ts,
// which runs the compiler's REPL session and the JS session that runs its
// modules in this worker too.

import type { Project } from "./project.ts";
import type { RunMode, RunResult, WatResult } from "./runner.ts";
import { createReplHost, type ReplMessage, type ReplRequest } from "./repl-host.ts";
import { createRustRunner, type CompiledRun } from "./rust-runner.ts";

export type WorkerRequest =
  | {
      readonly kind: "project";
      readonly id: number;
      readonly mode: RunMode;
      readonly project: Project;
    }
  /** The WAT of the module `project` compiles to; replies `wat`. */
  | { readonly kind: "wat"; readonly id: number; readonly project: Project }
  | ReplRequest;

export type WorkerMessage =
  | { readonly kind: "ready" }
  | {
      readonly kind: "stdout";
      readonly id: number;
      readonly line: string;
      readonly debug?: boolean;
    }
  | { readonly kind: "result"; readonly id: number; readonly result: RunResult }
  /** A project to run, for the run worker. */
  | { readonly kind: "compiled"; readonly id: number; readonly run: CompiledRun }
  | { readonly kind: "wat"; readonly id: number; readonly result: WatResult }
  | { readonly kind: "wat-loading"; readonly id: number }
  | ReplMessage;

interface WorkerScope {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerMessage, transfer?: Transferable[]): void;
}

const scope = self as unknown as WorkerScope;
const rust = createRustRunner(
  fetch(new URL("./hd_web.wasm", import.meta.url)).then((response) => {
    if (!response.ok) {
      throw new Error(`hd_web.wasm: HTTP ${response.status}`);
    }
    return response.arrayBuffer();
  }),
);

const repl = createReplHost(rust.compiler, (message, transfer) =>
  scope.postMessage(message, transfer),
);

scope.onmessage = async ({ data: request }) => {
  if (request.kind.startsWith("repl-")) {
    await repl.handle(request as ReplRequest);
  } else if (request.kind === "project") {
    const { id, mode, project } = request;
    const built = await rust.compileProject(project, mode);
    if ("mode" in built) {
      scope.postMessage({ kind: "compiled", id, run: built }, [
        built.compiled.wasm.buffer as ArrayBuffer,
      ]);
    } else {
      scope.postMessage({ kind: "result", id, result: built });
    }
  } else if (request.kind === "wat") {
    const result = await rust.watProject(request.project, () =>
      scope.postMessage({ kind: "wat-loading", id: request.id }),
    );
    scope.postMessage({ kind: "wat", id: request.id, result });
  }
};
void rust.ready.then(() => scope.postMessage({ kind: "ready" }));
