// The run worker: it runs what the compiler worker (worker.ts) built, Run's
// program or Test's cases, with the browser host. A Stop or the run limit
// terminates this worker alone, so the compiler keeps its warm store
// (crates/hd_cli/DESIGN.md §7.9, docs/compiler/engines-and-test-runner.md §18.4).

import type { RunResult } from "./runner.ts";
import { runCompiled, type CompiledRun } from "./rust-runner.ts";

export interface RunRequest {
  readonly kind: "run";
  readonly id: number;
  readonly run: CompiledRun;
}

export type RunMessage =
  | { readonly kind: "ready" }
  | {
      readonly kind: "stdout";
      readonly id: number;
      readonly line: string;
      readonly debug?: boolean;
    }
  | { readonly kind: "result"; readonly id: number; readonly result: RunResult };

interface WorkerScope {
  onmessage: ((event: MessageEvent<RunRequest>) => void) | null;
  postMessage(message: RunMessage): void;
}

const scope = self as unknown as WorkerScope;

scope.onmessage = async ({ data: { id, run } }) => {
  const onStdout = (line: string, debug?: boolean): void =>
    scope.postMessage({ kind: "stdout", id, line, ...(debug ? { debug } : {}) });
  const result = await runCompiled(run, onStdout);
  scope.postMessage({ kind: "result", id, result });
};
scope.postMessage({ kind: "ready" });
