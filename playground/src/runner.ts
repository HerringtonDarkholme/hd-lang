// Compiles and runs a playground project with the prototype compiler. The
// worker calls this; tests bundle it for the browser and call it directly.
//
// A project whose entry module declares `main` runs as a program. Without
// `main`, the entry module's top-level inputs go through a REPL session in
// order, and expression values print as the REPL prints them.

import { analyze, instantiate } from "../../src/compiler.ts";
import type { Diagnostic } from "../../src/diagnostics.ts";
import { linkPackage, type LinkedPackage, type PackageDiagnostic } from "../../src/package.ts";
import { parse } from "../../src/parser/index.ts";
import {
  classifyInput,
  parseReplMessage,
  ReplSession,
  splitInputs,
  type ReplMessage,
  type SourceInput,
} from "../../src/repl.ts";
import { RuntimePanicError } from "../../src/runtime-panic.ts";
import { resultParts } from "../../src/types.ts";
import type { Project } from "./project.ts";

/** `run` runs `main` or the top-level code, `check` type-checks, `test` runs test blocks. */
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

type Finish = (
  status: RunResult["status"],
  diagnostics: readonly RunDiagnostic[],
  summary: string,
) => RunResult;

export async function runProject(
  project: Project,
  mode: RunMode,
  onStdout: (line: string) => void = () => undefined,
): Promise<RunResult> {
  const started = performance.now();
  const stdout: string[] = [];
  const emit = (text: string): void => {
    for (const line of text.split("\n")) {
      stdout.push(line);
      onStdout(line);
    }
  };
  const finish: Finish = (status, diagnostics, summary) => ({
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
  const program = parse(source).program;
  if (mode !== "test" && program && !program.functions.some(({ name }) => name === "main")) {
    const inputs = splitInputs(entryText(linked));
    if (inputs.some(({ text }) => classifyInput(text) !== "declaration"))
      return evaluateTopLevel(linked, project.main, inputs, mode === "run", emit, finish);
  }

  const analysis = analyze(source);
  const diagnostics = analysis.diagnostics.map((diagnostic: Diagnostic) =>
    toRunDiagnostic(linked.locate(diagnostic)),
  );
  if (!analysis.hir || hasErrors(diagnostics))
    return finish("compile-error", diagnostics, "compilation failed");
  if (mode === "check") return finish("ok", diagnostics, "no errors");

  const testNames = program?.tests.map(({ name }) => name) ?? [];
  let current = "module initialization";
  try {
    const { instance, compilation } = await instantiate(source, {
      console: emit,
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
    if (entry && mode === "run") {
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
        mode === "test"
          ? "nothing to test: declare a `test` block"
          : "nothing to run: declare `pub fn main()`, top-level code, or a `test` block",
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

/** The entry module's text in the linked source, with its package uses blanked. */
function entryText(linked: LinkedPackage): string {
  return linked
    .source!.split("\n")
    .slice((linked.entryLine ?? 1) - 1)
    .join("\n");
}

/**
 * Runs (or with `execute` false, checks) the entry module with REPL
 * semantics: the other linked modules become the session's first
 * declarations, then each top-level input of the entry module is evaluated in
 * order. The first rejected input ends the run.
 */
async function evaluateTopLevel(
  linked: LinkedPackage,
  path: string,
  inputs: readonly SourceInput[],
  execute: boolean,
  emit: (text: string) => void,
  finish: Finish,
): Promise<RunResult> {
  const session = new ReplSession();
  const lines = linked.source!.split("\n");
  const entryLine = linked.entryLine ?? 1;
  const diagnostics: RunDiagnostic[] = [];
  const endColumn = (line: number): number => (lines[entryLine + line - 2]?.length ?? 0) + 1;
  const inEntry = (message: ReplMessage, input: SourceInput): RunDiagnostic => {
    const line = message.line === undefined ? input.line : input.line + message.line - 1;
    const column = message.column ?? 1;
    return {
      path,
      code: message.code,
      message: message.message,
      severity: message.severity,
      line,
      column,
      endLine: line,
      endColumn: Math.max(column, endColumn(line)),
      notes: [],
    };
  };

  const prelude = lines.slice(0, entryLine - 1).join("\n");
  if (prelude.trim() !== "") {
    const outcome = await session.declare(prelude);
    const located = [...outcome.warnings, ...outcome.errors].map((text) => {
      const message = parseReplMessage(text);
      const line = message.line ?? message.sessionLine ?? 1;
      const position = { offset: 0, line, column: message.column ?? 1 };
      return toRunDiagnostic(
        linked.locate({ ...message, span: { start: position, end: position } }),
      );
    });
    diagnostics.push(...located);
    if (!outcome.accepted) return finish("compile-error", diagnostics, "compilation failed");
  }

  for (const input of inputs) {
    const outcome = await session.evaluate(input.text, { run: execute });
    for (const text of outcome.output) emit(text);
    if (outcome.value !== undefined) emit(`${outcome.value} : ${outcome.type}`);
    diagnostics.push(...outcome.warnings.map((text) => inEntry(parseReplMessage(text), input)));
    if (outcome.accepted) continue;
    const messages = outcome.errors.map(parseReplMessage);
    diagnostics.push(...messages.map((message) => inEntry(message, input)));
    const panic = messages.find(({ code }) => code === "runtime-panic");
    if (panic)
      return finish(
        "panic",
        diagnostics,
        `${panic.message}: runtime panic at ${path}:${input.line}`,
      );
    const internal = messages.find(({ code }) => code === "internal-error");
    if (internal) return finish("failure", diagnostics, internal.message);
    return finish("compile-error", diagnostics, "compilation failed");
  }
  if (!execute) return finish("ok", diagnostics, "no errors");
  const count = inputs.length;
  return finish("ok", diagnostics, `ran ${count} top-level ${count === 1 ? "input" : "inputs"}`);
}
