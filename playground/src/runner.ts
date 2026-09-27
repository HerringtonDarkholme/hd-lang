// Compiles and runs a playground project with the prototype compiler. The
// worker calls this; tests bundle it for the browser and call it directly.

import { analyze, instantiate } from "../../src/compiler.ts";
import type { Diagnostic } from "../../src/diagnostics.ts";
import { linkPackage, type PackageDiagnostic } from "../../src/package.ts";
import { parse } from "../../src/parser/index.ts";
import { RuntimePanicError } from "../../src/runtime-panic.ts";
import { resultParts } from "../../src/types.ts";
import type { Project } from "./project.ts";

export type RunMode = "run" | "check";

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

function toRunDiagnostic(diagnostic: PackageDiagnostic): RunDiagnostic {
  return {
    path: diagnostic.path,
    code: diagnostic.code,
    message: diagnostic.message,
    severity: diagnostic.severity === "warning" ? "warning" : "error",
    line: diagnostic.span.start.line,
    column: diagnostic.span.start.column,
    endLine: diagnostic.span.end.line,
    endColumn: diagnostic.span.end.column,
    notes: diagnostic.notes ?? [],
  };
}

/** Formats a diagnostic the way `hd check` prints one. */
export function formatRunDiagnostic(diagnostic: RunDiagnostic): string {
  const severity = diagnostic.severity === "warning" ? "warning: " : "";
  const notes = diagnostic.notes.map((note) => `\n  note: ${note}`).join("");
  return `${diagnostic.path}:${diagnostic.line}:${diagnostic.column}: ${severity}${diagnostic.code}: ${diagnostic.message}${notes}`;
}

const hasErrors = (diagnostics: readonly RunDiagnostic[]): boolean =>
  diagnostics.some(({ severity }) => severity === "error");

export async function runProject(
  project: Project,
  mode: RunMode,
  onStdout: (line: string) => void = () => undefined,
): Promise<RunResult> {
  const started = performance.now();
  const stdout: string[] = [];
  const finish = (
    status: RunResult["status"],
    diagnostics: readonly RunDiagnostic[],
    summary: string,
  ): RunResult => ({
    status,
    diagnostics,
    stdout,
    summary,
    milliseconds: Math.round(performance.now() - started),
  });

  const linked = linkPackage(project.files, project.main);
  const linkDiagnostics = linked.diagnostics.map(toRunDiagnostic);
  if (!linked.source || hasErrors(linkDiagnostics))
    return finish("compile-error", linkDiagnostics, "compilation failed");
  const source = linked.source;
  const analysis = analyze(source);
  const diagnostics = analysis.diagnostics.map((diagnostic: Diagnostic) =>
    toRunDiagnostic(linked.locate(diagnostic)),
  );
  if (!analysis.hir || hasErrors(diagnostics))
    return finish("compile-error", diagnostics, "compilation failed");
  if (mode === "check") return finish("ok", diagnostics, "no errors");

  const testNames = parse(source).program?.tests.map(({ name }) => name) ?? [];
  let current = "module initialization";
  try {
    const { instance, compilation } = await instantiate(source, {
      console: (text) => {
        for (const line of text.split("\n")) {
          stdout.push(line);
          onStdout(line);
        }
      },
      providerConfigurationId: "playground",
    });
    const functions = compilation.hir.functions;
    const entry = functions.find((declaration) => declaration.entry === true);
    const providers = (requirements: readonly string[]): unknown[] =>
      requirements.map((requirement) => ({ requirement }));
    const exported = (name: string): ((...values: unknown[]) => unknown) => {
      const value = instance.exports[name];
      if (typeof value !== "function") throw new Error(`${name} has no runnable export`);
      return value as (...values: unknown[]) => unknown;
    };
    if (entry) {
      current = entry.name;
      const result = exported(entry.name)(...providers(entry.requirements));
      if (resultParts(entry.result)?.ok === "void" && result !== 0)
        return finish("failure", diagnostics, `${entry.name} returned Err`);
      const returned = typeof result === "number" && result !== 0 ? ` with ${result}` : "";
      return finish("ok", diagnostics, `exited normally${returned}`);
    }
    const tests = functions.filter(({ name }) => /^\$test\.\d+$/.test(name));
    if (tests.length === 0)
      return finish(
        "failure",
        diagnostics,
        "nothing to run: declare `pub fn main()` or a `test` block",
      );
    for (const test of tests) {
      const index = Number(test.name.slice("$test.".length));
      current = `test "${testNames[index] ?? test.name}"`;
      exported(`__hd_test_${index}`)(...providers(test.requirements));
    }
    return finish(
      "ok",
      diagnostics,
      `${tests.length} ${tests.length === 1 ? "test" : "tests"} passed`,
    );
  } catch (error) {
    if (error instanceof RuntimePanicError)
      return finish("panic", diagnostics, `${error.code}: runtime panic in ${current}`);
    const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    return finish("failure", diagnostics, `${message} (in ${current})`);
  }
}
