// The results the playground's runner (rust-runner.ts) reports, shared by
// the worker, the page, the website's tour, and the tests.

/** `run` runs `main` or the top-level code, `check` type-checks, `test` runs the test cases. */
export type RunMode = "run" | "check" | "test";

export interface RunDiagnostic {
  readonly path: string;
  readonly code: string;
  readonly message: string;
  readonly severity: "error" | "warning";
  readonly line: number;
  readonly column: number;
  readonly endLine: number;
  readonly endColumn: number;
  readonly notes: readonly string[];
}

export interface RunResult {
  /** `ok`: checked or ran to completion. */
  readonly status: "ok" | "compile-error" | "panic" | "failure";
  readonly diagnostics: readonly RunDiagnostic[];
  /** Console lines, in order (also streamed through `onStdout`). */
  readonly stdout: readonly string[];
  /** One-line outcome such as `2 tests passed` or `integer-overflow: runtime panic`. */
  readonly summary: string;
  readonly milliseconds: number;
}

/** Formats a diagnostic the way `hd check` prints one. */
export function formatRunDiagnostic(diagnostic: RunDiagnostic): string {
  const severity = diagnostic.severity === "warning" ? "warning: " : "";
  const notes = diagnostic.notes.map((note) => `\n  note: ${note}`).join("");
  return `${diagnostic.path}:${diagnostic.line}:${diagnostic.column}: ${severity}${diagnostic.code}: ${diagnostic.message}${notes}`;
}

/** The module a project compiles to, as WebAssembly text. */
export interface CompiledModule {
  readonly wat: string;
}

export interface WatResult {
  /** `ok`: `module` is set. */
  readonly status: "ok" | "compile-error" | "failure";
  readonly module?: CompiledModule;
  readonly diagnostics: readonly RunDiagnostic[];
  readonly summary: string;
}
