// The compiler worker: the compiler runs here so the page stays responsive,
// and a runaway program can be stopped by terminating the worker.
//
// Run, Check and Test go to the Rust compiler (rust-runner.ts), whose
// wasm, hd_web.wasm, sits beside this script; the worker loads it once and
// says it is ready after that. The WAT view's module comes from the same
// wasm (the wat goal). There is no REPL until the Rust compiler has one
// (#17): REPL requests answer that it is not available.

import type { Project } from "./project.ts";
import type { RunMode, RunResult, WatResult } from "./runner.ts";
import { createRustRunner } from "./rust-runner.ts";

/** One line of a REPL reply. */
export interface ReplEntry {
  readonly kind: "output" | "debug" | "value" | "code" | "info" | "warning" | "error";
  readonly text: string;
  readonly type?: string;
}

/** What one REPL input or `:` command produced. */
export interface ReplReply {
  readonly entries: readonly ReplEntry[];
  /** True when the input was accepted and is now part of the session. */
  readonly kept: boolean;
  /** Set when the input was `:reset` or `:quit`. */
  readonly command?: "reset" | "quit";
}

export type WorkerRequest =
  | {
      readonly kind: "project";
      readonly id: number;
      readonly mode: RunMode;
      readonly project: Project;
    }
  /** The WAT of the module `project` compiles to; replies `wat`. */
  | { readonly kind: "wat"; readonly id: number; readonly project: Project }
  /** One complete REPL input or `:` command; replies `repl`. */
  | { readonly kind: "repl"; readonly id: number; readonly input: string }
  /** Starts a fresh session from inputs a stopped worker had accepted; replies `repl`. */
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
  | { readonly kind: "repl"; readonly id: number; readonly reply: ReplReply };

/** The answer to every REPL request until the Rust compiler has a REPL. */
const REPL_NOT_AVAILABLE: ReplReply = {
  entries: [{ kind: "error", text: "the REPL is not available yet; use the playground" }],
  kept: false,
};

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
  } else if (request.kind === "wat") {
    const result = await rust.watProject(request.project);
    scope.postMessage({ kind: "wat", id: request.id, result });
  } else {
    scope.postMessage({ kind: "repl", id: request.id, reply: REPL_NOT_AVAILABLE });
  }
};
void rust.ready.then(() => scope.postMessage({ kind: "ready" }));
