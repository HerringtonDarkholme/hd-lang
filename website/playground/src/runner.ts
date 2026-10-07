// Compiles and runs a playground project with the prototype compiler. The
// worker calls this; tests bundle it for the browser and call it directly.
//
// A project whose entry module declares `main` runs as a program. Without
// `main`, the entry module's top-level inputs go through a REPL session in
// order, and expression values print as the REPL prints them.
//
// `watProject` gives the WebAssembly text of the module a project compiles
// to, for the playground's WAT view.

import { DEFAULT_PROFILE_TRAITS } from "../../../src/checker/standard-library.ts";
import {
  analyze,
  compileToWasm,
  instantiate,
  type HostSuspensionCall,
} from "../../../src/compiler.ts";
import type { Diagnostic, SourceSpan } from "../../../src/diagnostics.ts";
import { emitWat } from "../../../src/emitter/index.ts";
import type { HirFunction, HirTrait } from "../../../src/hir.ts";
import { linkPackage, type LinkedPackage, type PackageDiagnostic } from "../../../src/package.ts";
import { parse } from "../../../src/parser/index.ts";
import {
  classifyInput,
  parseReplMessage,
  ReplSession,
  splitInputs,
  type ReplHost,
  type ReplMessage,
  type SourceInput,
} from "../../../src/repl.ts";
import { RuntimePanicError } from "../../../src/runtime-panic.ts";
import { runSelected } from "../../../src/test-runner.ts";
import { PLAYGROUND_TRAITS, playgroundAnswer } from "./playground-host.ts";
import type { Project } from "./project.ts";

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

/** A module a run compiled to Wasm GC, as WebAssembly text. */
export interface CompiledModule {
  readonly wat: string;
  /** `program`: the one module of the project; `top-level`: the last module REPL semantics compiled. */
  readonly origin: "program" | "top-level";
  /** How many modules the run compiled; REPL semantics compiles one per input that runs. */
  readonly count: number;
}

export interface WatResult {
  /**
   * `ok`: `module` is set. `not-run`: top-level code without `main` compiles
   * its modules only as Run evaluates it, so there is no module until then.
   */
  readonly status: "ok" | "compile-error" | "not-run" | "failure";
  readonly module?: CompiledModule;
  readonly diagnostics: readonly RunDiagnostic[];
  readonly summary: string;
}

/**
 * A linked source joins modules, each of which may hold a `tests:` block,
 * with the linker's initialization-group starts beside it.
 */
const joinedParse = (linked: LinkedPackage) => ({
  joinedModules: true as const,
  initGroupStarts: linked.initGroups,
});

/**
 * The host a playground run binds: of the default profile
 * (spec/cli/command-line.md#r-cli.host.default-profile), the traits a browser
 * can provide: `Clock` and `Random`, with the same answers as `hd run`, and
 * `Http` to the page's own origin (playground-host.ts). `Console` is built
 * in. The REPL panel's session binds it too.
 */
export const PLAYGROUND_HOST: ReplHost = { traits: PLAYGROUND_TRAITS, invoke: playgroundAnswer };

/** The default profile's traits the playground does not provide, by qualified name. */
const UNPROVIDED_TRAITS: ReadonlySet<string> = new Set(
  DEFAULT_PROFILE_TRAITS.map(([module, name]) => `std.${module}.${name}`).filter(
    (name) =>
      name !== "std.console.Console" &&
      !PLAYGROUND_TRAITS.some((trait) => `${trait.module}.${trait.name}` === name),
  ),
);

const shortName = (qualified: string): string => qualified.slice(qualified.lastIndexOf(".") + 1);

/** The traits `entry`'s row names that the playground does not provide, such as `FsRead`. */
function unprovidedTraits(traits: readonly HirTrait[], entry: HirFunction): string[] {
  return entry.requirements.flatMap((requirement) => {
    const standardName = traits.find(({ name }) => name === requirement)?.standardName;
    return standardName && UNPROVIDED_TRAITS.has(standardName) ? [shortName(standardName)] : [];
  });
}

/** A call on a trait no playground provider answers, which the entry check let through. */
function unprovidedCall(call: HostSuspensionCall): never {
  throw new RuntimePanicError(
    "host-contract",
    `the playground does not provide ${shortName(call.standardName ?? call.providerKey)}; run the program with hd run`,
  );
}

const hasErrors = (diagnostics: readonly RunDiagnostic[]): boolean =>
  diagnostics.some(({ severity }) => severity === "error");

type Finish = (
  status: RunResult["status"],
  diagnostics: readonly RunDiagnostic[],
  summary: string,
) => RunResult;

/**
 * Runs, checks, or tests a project. `onModule` receives the module the run
 * compiled, or with REPL semantics the last of them; `check` compiles none.
 */
export async function runProject(
  project: Project,
  mode: RunMode,
  onStdout: (line: string, debug?: boolean) => void = () => undefined,
  onModule: (module: CompiledModule) => void = () => undefined,
): Promise<RunResult> {
  const started = performance.now();
  const stdout: string[] = [];
  // A `dbg` line goes to the output panel too, marked as debug output
  // (spec/cli/command-line.md#debug-output).
  const emit = (text: string, debug = false): void => {
    for (const line of text.split("\n")) {
      stdout.push(line);
      onStdout(line, debug);
    }
  };
  const finish: Finish = (status, diagnostics, summary) => ({
    status,
    diagnostics,
    stdout,
    summary,
    milliseconds: Math.round(performance.now() - started),
  });

  // A test build links every `*_test.hd` test module (src/package.ts).
  const linked = linkPackage(project.files, project.main, { tests: mode === "test" });
  const linkDiagnostics = linked.diagnostics.map(toRunDiagnostic);
  if (!linked.source || hasErrors(linkDiagnostics))
    return finish("compile-error", linkDiagnostics, "compilation failed");
  const source = linked.source;
  const program = parse(source, joinedParse(linked)).program;
  if (mode !== "test") {
    const inputs = topLevelInputs(linked, program);
    if (inputs)
      return evaluateTopLevel(linked, project.main, inputs, mode === "run", emit, finish, onModule);
  }

  const analysis = analyze(source, { parse: joinedParse(linked) });
  const diagnostics = analysis.diagnostics.map((diagnostic: Diagnostic) =>
    toRunDiagnostic(linked.locate(diagnostic)),
  );
  if (!analysis.hir || hasErrors(diagnostics))
    return finish("compile-error", diagnostics, "compilation failed");
  if (mode === "check") return finish("ok", diagnostics, "no errors");

  let current = "module initialization";
  try {
    const options = {
      console: (text: string) => emit(text),
      debugOutput: (text: string) => emit(text, true),
      debugLocation: (span: SourceSpan) => {
        const { path, span: located } = linked.locate({ code: "", message: "", span });
        return `${path}:${located.start.line}:${located.start.column}`;
      },
      providerConfigurationId: "playground",
      parse: joinedParse(linked),
      hostSuspensionInvoke: (call: HostSuspensionCall) =>
        playgroundAnswer(call) ?? unprovidedCall(call),
    };
    // A trait the browser cannot provide stops the run before it starts.
    const checked = analysis.hir.functions.find((declaration) => declaration.entry === true);
    const missing = checked && mode === "run" ? unprovidedTraits(analysis.hir.traits, checked) : [];
    if (checked && missing.length > 0)
      return finish(
        "failure",
        diagnostics,
        `${checked.name} needs ${missing.join(" and ")}, which the playground does not provide; run it with hd run`,
      );
    const { instance, compilation } = await instantiate(source, options);
    onModule({ wat: compilation.wat, origin: "program", count: 1 });
    // `hd run` and `hd test` judge outcomes with the same runner: exit codes,
    // `it_each` rows, `expect_panic`, `timeout`, and a fresh program instance
    // for each test case (src/test-runner.ts).
    const fresh = async (): Promise<WebAssembly.Exports> =>
      (await instantiate(source, { ...options, compilation })).instance.exports;
    const functions = compilation.hir.functions;
    const entry = functions.find((declaration) => declaration.entry === true);
    if (entry && mode === "run") {
      current = entry.name;
      const outcome = await runSelected([entry], instance.exports, fresh);
      if (outcome.kind === "exit")
        return finish("failure", diagnostics, `${entry.name} exited with code ${outcome.code}`);
      if (outcome.kind === "failed")
        return finish("failure", diagnostics, `${entry.name} returned Err`);
      return finish("ok", diagnostics, "exited normally");
    }
    // A test case with the `ignore` option does not run
    // (spec/lang/10-modules.md#r-module.testing.option.ignore).
    const cases = functions.filter(({ name }) => /^\$test\.\d+$/.test(name));
    const tests = cases.filter(({ testOptions }) => testOptions?.ignore === undefined);
    if (tests.length === 0)
      return finish(
        "failure",
        diagnostics,
        mode === "test"
          ? "nothing to test: add a `tests:` block or a `_test.hd` module with `it(...)` test cases"
          : "nothing to run: declare `pub fn main()`, top-level code, or a `tests:` block",
      );
    let passed = 0;
    for (const test of tests) {
      current = `test case "${test.testOptions?.name ?? test.name}"`;
      const outcome = await runSelected([test], instance.exports, fresh);
      if (outcome.kind !== "passed") {
        const subject =
          outcome.kind === "failed"
            ? [outcome.subject, outcome.outcome].filter(Boolean).join(" ")
            : current;
        return finish("failure", diagnostics, `${subject} failed`);
      }
      passed += outcome.count;
    }
    const ignored = cases.length - tests.length;
    return finish(
      "ok",
      diagnostics,
      `${passed} ${passed === 1 ? "test" : "tests"} passed${ignored > 0 ? `, ${ignored} ignored` : ""}`,
    );
  } catch (error) {
    if (error instanceof RuntimePanicError) {
      const notes = error.notes.map((note) => `; note: ${note}`).join("");
      return finish("panic", diagnostics, `${error.message} in ${current}${notes}`);
    }
    const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    return finish("failure", diagnostics, `${message} (in ${current})`);
  }
}

/**
 * The WAT of the module `project` compiles to, without running it. Top-level
 * code without `main` has no single module (see `WatResult`); the worker
 * answers for it from the last run.
 */
export async function watProject(project: Project): Promise<WatResult> {
  const linked = linkPackage(project.files, project.main);
  const linkDiagnostics = linked.diagnostics.map(toRunDiagnostic);
  if (!linked.source || hasErrors(linkDiagnostics))
    return { status: "compile-error", diagnostics: linkDiagnostics, summary: "compilation failed" };
  if (topLevelInputs(linked))
    return {
      status: "not-run",
      diagnostics: [],
      summary:
        "Without main, Run compiles one module for each top-level input it evaluates. Run the project to see the last one.",
    };
  const analysis = analyze(linked.source, { parse: joinedParse(linked) });
  const diagnostics = analysis.diagnostics.map((diagnostic: Diagnostic) =>
    toRunDiagnostic(linked.locate(diagnostic)),
  );
  if (!analysis.hir || hasErrors(diagnostics))
    return { status: "compile-error", diagnostics, summary: "compilation failed" };
  try {
    // Assembling validates the WAT, as a run would. The WAT is the module a
    // run assembles, with its panic sites' debug-location lines.
    const { wat } = await compileToWasm({
      wat: emitWat(analysis.hir, { sites: [] }),
      hir: analysis.hir,
      diagnostics: analysis.diagnostics,
    });
    return { status: "ok", module: { wat, origin: "program", count: 1 }, diagnostics, summary: "" };
  } catch (error) {
    const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    return { status: "failure", diagnostics, summary: message };
  }
}

/**
 * What the WAT view shows after a run or test: its compile errors, or the
 * module it compiled. Undefined when neither applies, such as after Check.
 */
export function watFromRun(
  result: RunResult,
  module: CompiledModule | undefined,
): WatResult | undefined {
  if (result.status === "compile-error")
    return { status: "compile-error", diagnostics: result.diagnostics, summary: result.summary };
  if (!module) return undefined;
  return { status: "ok", module, diagnostics: result.diagnostics, summary: "" };
}

/**
 * The entry module's top-level inputs when Run evaluates them with REPL
 * semantics: the entry declares no `main` and has top-level code.
 */
function topLevelInputs(
  linked: LinkedPackage,
  program = parse(linked.source!, joinedParse(linked)).program,
): readonly SourceInput[] | undefined {
  if (!program || program.functions.some(({ name }) => name === "main")) return undefined;
  const inputs = splitInputs(entryText(linked));
  return inputs.some(({ text }) => classifyInput(text) !== "declaration") ? inputs : undefined;
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
  emit: (text: string, debug?: boolean) => void,
  finishRun: Finish,
  onModule: (module: CompiledModule) => void,
): Promise<RunResult> {
  const session = new ReplSession({}, undefined, PLAYGROUND_HOST);
  const finish: Finish = (status, diagnostics, summary) => {
    const compiled = session.compiledModule();
    if (compiled) onModule({ wat: compiled.wat, origin: "top-level", count: compiled.count });
    return finishRun(status, diagnostics, summary);
  };
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
      notes: [...(message.notes ?? [])],
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
    outcome.output.forEach((text, index) => emit(text, outcome.debug?.includes(index)));
    if (outcome.value !== undefined) emit(`${outcome.value} : ${outcome.type}`);
    diagnostics.push(...outcome.warnings.map((text) => inEntry(parseReplMessage(text), input)));
    if (outcome.accepted) continue;
    const messages = outcome.errors.map(parseReplMessage);
    diagnostics.push(...messages.map((message) => inEntry(message, input)));
    const panic = messages.find(({ code }) => code === "runtime-panic");
    if (panic) {
      // The panic names its operation in the input, when the host found it.
      const { line, column } = inEntry(panic, input);
      const notes = (panic.notes ?? []).map((note) => `; note: ${note}`).join("");
      const at = panic.line === undefined ? `${line}` : `${line}:${column}`;
      return finish(
        "panic",
        diagnostics,
        `${panic.message}: runtime panic at ${path}:${at}${notes}`,
      );
    }
    const internal = messages.find(({ code }) => code === "internal-error");
    if (internal) return finish("failure", diagnostics, internal.message);
    return finish("compile-error", diagnostics, "compilation failed");
  }
  if (!execute) return finish("ok", diagnostics, "no errors");
  const count = inputs.length;
  return finish("ok", diagnostics, `ran ${count} top-level ${count === 1 ? "input" : "inputs"}`);
}
