// Check, Run and Test on the Rust compiler (crates/hd_web/DESIGN.md §3).
// The compiler wasm of crates/hd_web compiles the project; Run instantiates
// the program with the browser host, host/web.mts, as `hd run` does on
// Node with host/node.mjs. Test runs each case with the case loop that
// `hd test` runs on Node (host/cases.mjs), over the browser host, and
// reports what `hd test` would. The worker loads one runner, so the
// compiler and its in-memory store live as long as the worker.
//
// The WAT view's module is the program the compiler built for Run, printed
// by the separately loaded hd_wat module.

import { instantiateCompiler } from "./compiler-host.ts";

import { judge, runCases, type CaseRecord, type Verdict } from "../../../host/cases.mjs";
import { createWebHost, memoryFiles, unboundTraits } from "../../../host/web.mts";
import {
  GOAL,
  STATUS,
  compileRequest,
  encodeRequest,
  programBytes,
  type Compiled,
  type CompilerExports,
  type WireDiagnostic,
  type WireTest,
} from "./compiler-wire.ts";
import { createWatPrinter } from "./wat-printer.ts";
import type { Project } from "./project.ts";
import type { RunDiagnostic, RunResult, WatResult } from "./runner.ts";

/** The modes the Rust compiler serves. */
export type RustMode = "run" | "check" | "test";

/**
 * A project the compiler built for Run or Test: what the run worker
 * (run-worker.ts) needs to run it, so the compiler's worker never runs a
 * program and a Stop leaves its warm store (crates/hd_cli/DESIGN.md §7.9).
 */
export interface CompiledRun {
  readonly mode: "run" | "test";
  readonly compiled: Compiled;
  readonly diagnostics: readonly RunDiagnostic[];
  /** `main` or `the program`, as the outcome names it. */
  readonly who: string;
  /** How long the compile took. */
  readonly milliseconds: number;
}

export interface RustRunner {
  /** Settles once the compiler wasm is instantiated, or failed to be. */
  readonly ready: Promise<void>;
  /** The compiler wasm's exports, which the worker's REPL shares (repl-host.ts). */
  readonly compiler: Promise<CompilerExports>;
  /** Compiles and runs: `compileProject`, then `runCompiled`. */
  runProject(
    project: Project,
    mode: RustMode,
    onStdout?: (line: string, debug?: boolean) => void,
  ): Promise<RunResult>;
  /**
   * Compiles `project`: the result when nothing is left to run (Check, a
   * compile error, a refused request), else what `runCompiled` runs.
   */
  compileProject(project: Project, mode: RustMode): Promise<RunResult | CompiledRun>;
  /** The WebAssembly text of the module Run would build for `project`. */
  watProject(project: Project, onLoading?: () => void): Promise<WatResult>;
}

/** The program's `Args.program`, as `hd run` gives a package's executable name. */
const PROGRAM = "main";

/** Whether the entry file declares `pub fn main`; without it, Run builds a script. */
const declaresMain = (source: string): boolean => /^pub fn main\b/m.test(source);

function toRunDiagnostic(diagnostic: WireDiagnostic, entry: string): RunDiagnostic {
  const line = diagnostic.line ?? 1;
  const column = diagnostic.column ?? 1;
  return {
    path: diagnostic.path ?? entry,
    code: diagnostic.code ?? "error",
    message: diagnostic.message,
    severity: diagnostic.severity === "warning" ? "warning" : "error",
    line,
    column,
    endLine: diagnostic.endLine ?? line,
    endColumn: diagnostic.endColumn ?? column,
    notes: [],
  };
}

/**
 * A stream that hands `each` its complete lines, since a host may write a
 * line in pieces; `end` hands over an unfinished last line.
 */
function lineStream(each: (line: string) => void): { write(text: string): void; end(): void } {
  let rest = "";
  return {
    write(text) {
      const lines = (rest + text).split("\n");
      rest = lines.pop()!;
      lines.forEach(each);
    },
    end() {
      if (rest !== "") {
        each(rest);
      }
      rest = "";
    },
  };
}

type Finish = (
  status: RunResult["status"],
  diagnostics: readonly RunDiagnostic[],
  summary: string,
) => RunResult;

/** `count` and the noun, plural unless it is one: `1 test`, `2 tests`. */
const tests = (count: number): string => `${count} ${count === 1 ? "test" : "tests"}`;

/**
 * The lines `hd test` prints for a result other than a pass
 * (docs/compiler/engines-and-test-runner.md §19.5), without the repro command, which
 * names a file on disk, and without frames, which need the source
 * positions the playground does not have yet (crates/hd_web/DESIGN.md §5).
 */
function reportLines(at: string, name: string, verdict: Verdict, input?: string): string[] {
  const indent = (text: string): string[] => text.split("\n").map((line) => `    ${line}`);
  const shown = input === undefined ? [] : [`    input: ${input}`];
  if (verdict.outcome === "failed") {
    return [`FAIL ${at}: ${name}`, ...indent(verdict.message), ...shown];
  }
  if (verdict.outcome === "panicked") {
    return [
      `PANIC ${at}: ${name}`,
      `    panic: ${verdict.category}${verdict.message === "" ? "" : `: ${verdict.message}`}`,
      ...verdict.debug.map((line) => `    ${line}`),
      ...shown,
    ];
  }
  return [];
}

const runs = (test: WireTest): boolean =>
  test.run !== null && test.ignore === null && test.unsupported === null;

async function collectRecords({
  programs,
  cases,
  bytes,
  files,
  records,
}: {
  programs: NonNullable<Compiled["json"]["programs"]>;
  cases: readonly WireTest[];
  bytes: Uint8Array[];
  files: ReturnType<typeof memoryFiles>;
  records: CaseRecord[][];
}): Promise<string | undefined> {
  for (const [index, program] of programs.entries()) {
    const own = cases.flatMap((test, at) => (test.program === index && runs(test) ? [at] : []));
    if (own.length === 0) {
      continue;
    }
    const module = await WebAssembly.compile(bytes[index] as BufferSource);
    const missing = unboundTraits(module, createWebHost({ out() {}, err() {} }));
    if (missing.length > 0) {
      return `${program.file ?? "the tests"} needs ${missing.join(" and ")}, which the playground does not provide; run it with hd test`;
    }
    const byExport = new Map(own.map((at) => [cases[at]!.run![0], at]));
    await runCases({
      module,
      config: {
        program: program.file ?? "",
        args: [],
        programs: null,
        cases: own.map((at) => {
          const test = cases[at]!;
          return {
            test: test.run![0],
            init: test.run![1],
            kind: test.kind,
            tempDir: `/tmp/hd-test/${at}`,
            seed: test.seed,
            grants: {},
            snapshot: { dir: `/__snapshots__/${at}`, slug: "case", update: true },
            regression: test.seed === null ? null : { file: `/__regressions__/${at}` },
          };
        }),
      },
      createHost: (sink, env) => createWebHost(sink, Object.assign(env, { files })),
      files,
      notify: () => undefined,
      emit: (record) => {
        const at = byExport.get(record.test);
        if (at !== undefined) {
          records[at]!.push(record);
        }
      },
    });
  }
  return undefined;
}

const cannotRun = (test: WireTest): boolean => test.unsupported !== null || test.run === null;

function summarizeCases(
  cases: readonly WireTest[],
  records: CaseRecord[][],
  emit: (line: string) => void,
) {
  let passed = 0;
  let failed = 0;
  let ignored = 0;
  let unsupported = 0;
  let first: { status: RunResult["status"]; summary: string } | undefined;
  for (const [at, test] of cases.entries()) {
    const where = `${test.file}:${test.line}`;
    if (test.unsupported === null && test.ignore !== null) {
      ignored += 1;
      emit(`IGNORED ${where}: ${test.name} (${test.ignore})`);
      continue;
    }
    if (cannotRun(test)) {
      unsupported += 1;
      const what = test.unsupported ?? "a case without a checked body";
      emit(`UNSUPPORTED ${where}: ${test.name} (${what} cannot run yet)`);
      continue;
    }
    for (const record of records[at]!) {
      if (record.status === undefined) {
        continue;
      }
      const name = record.row === undefined ? test.name : `${test.name}[${record.row}]`;
      const verdict = judge(
        {
          status: record.status,
          trapped: record.trapped ?? false,
          stderr: record.stderr ?? "",
          category: record.category,
          message: record.message,
          failure: record.failure,
        },
        test.expectPanic,
      );
      if (verdict.outcome === "passed") {
        passed += 1;
        continue;
      }
      failed += 1;
      reportLines(where, name, verdict, record.input).forEach((line) => emit(line));
      first ??=
        verdict.outcome === "panicked"
          ? {
              status: "panic",
              summary: `${verdict.category}${verdict.message === "" ? "" : `: ${verdict.message}`} in test case "${name}"`,
            }
          : {
              status: "failure",
              summary: `test case "${name}" failed: ${verdict.message.split("\n")[0]}`,
            };
    }
  }
  return { passed, failed, ignored, unsupported, first };
}

/**
 * Test: runs every case of every test program as `hd test` does, a fresh
 * instance per case and per row, with the property runner and shrinking
 * of host/cases.mjs. A case running past its `timeout` while it waits is
 * cut at its deadline; one that never yields is cut only by the
 * playground's run limit. Snapshot and regression files live in memory
 * for the run (crates/hd_web/DESIGN.md §5). The output lists each case that did
 * not pass as `hd test` prints it; the summary counts the passes, or names
 * the first failure.
 */
async function runTests(
  compiled: Compiled,
  diagnostics: readonly RunDiagnostic[],
  emit: (line: string) => void,
  finish: Finish,
): Promise<RunResult> {
  const cases = compiled.json.tests ?? [];
  if (cases.length === 0) {
    return finish(
      "failure",
      diagnostics,
      "nothing to test: add a `tests:` block or a `_test.hd` module with `it(...)` test cases",
    );
  }
  const programs = compiled.json.programs ?? [];
  const bytes = programBytes(compiled);
  const files = memoryFiles();
  // Each case's result records, by its index in `cases`.
  const records: CaseRecord[][] = cases.map(() => []);
  const unavailable = await collectRecords({ programs, cases, bytes, files, records });
  if (unavailable) {
    return finish("failure", diagnostics, unavailable);
  }
  const { passed, failed, ignored, unsupported, first } = summarizeCases(cases, records, emit);
  const others = [
    ignored > 0 ? `, ${ignored} ignored` : "",
    unsupported > 0 ? `, ${unsupported} unsupported` : "",
  ].join("");
  if (first === undefined) {
    return finish("ok", diagnostics, `${tests(passed)} passed${others}`);
  }
  const more = failed > 1 ? `; ${failed} of ${tests(passed + failed)} failed` : "";
  return finish(first.status, diagnostics, `${first.summary}${more}`);
}

/**
 * A runner over the compiler wasm `bytes` (the file
 * `pnpm run build:web-compiler` writes); it instantiates them once.
 */
export function createRustRunner(
  bytes: Promise<BufferSource> | BufferSource,
  loadWat: () => Promise<BufferSource> = async () => {
    const response = await fetch(new URL("./hd_wat.wasm", import.meta.url));
    if (!response.ok) {
      throw new Error(`hd_wat.wasm: HTTP ${response.status}`);
    }
    return response.arrayBuffer();
  },
): RustRunner {
  const printer = createWatPrinter(loadWat);
  const compiler = Promise.resolve(bytes).then(
    async (source) =>
      (await instantiateCompiler(source)).instance.exports as unknown as CompilerExports,
  );
  const ready = compiler.then(
    () => undefined,
    () => undefined,
  );

  async function compileProject(
    project: Project,
    mode: RustMode,
  ): Promise<RunResult | CompiledRun> {
    const started = performance.now();
    const finish: Finish = (status, diagnostics, summary) => ({
      status,
      diagnostics,
      stdout: [],
      summary,
      milliseconds: Math.round(performance.now() - started),
    });
    let exports: CompilerExports;
    try {
      exports = await compiler;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return finish("failure", [], `the compiler did not load: ${message}`);
    }
    const request = encodeRequest(project.files, project.main, GOAL[mode]);
    const compiled = compileRequest(exports, request);
    if (compiled.status === STATUS.malformed) {
      return finish("failure", [], `the compiler refused the request: ${compiled.json.error}`);
    }
    const diagnostics = (compiled.json.diagnostics ?? []).map((diagnostic) =>
      toRunDiagnostic(diagnostic, project.main),
    );
    if (compiled.status === STATUS.errors) {
      return finish("compile-error", diagnostics, "compilation failed");
    }
    if (mode === "check") {
      return finish("ok", diagnostics, "no errors");
    }
    if (mode === "run" && compiled.wasm.length === 0) {
      return finish("failure", diagnostics, "no program to run");
    }
    return {
      mode,
      compiled,
      diagnostics,
      who: declaresMain(project.files[project.main] ?? "") ? "main" : "the program",
      milliseconds: performance.now() - started,
    };
  }

  async function runProject(
    project: Project,
    mode: RustMode,
    onStdout: (line: string, debug?: boolean) => void = () => undefined,
  ): Promise<RunResult> {
    const built = await compileProject(project, mode);
    return "mode" in built ? runCompiled(built, onStdout) : built;
  }

  async function watProject(project: Project, onLoading?: () => void): Promise<WatResult> {
    let exports: CompilerExports;
    try {
      exports = await compiler;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return {
        status: "failure",
        diagnostics: [],
        summary: `the compiler did not load: ${message}`,
      };
    }
    const compiled = compileRequest(exports, encodeRequest(project.files, project.main, GOAL.run));
    if (compiled.status === STATUS.malformed) {
      return {
        status: "failure",
        diagnostics: [],
        summary: `the compiler refused the request: ${compiled.json.error}`,
      };
    }
    const diagnostics = (compiled.json.diagnostics ?? []).map((diagnostic) =>
      toRunDiagnostic(diagnostic, project.main),
    );
    if (compiled.status === STATUS.errors) {
      return { status: "compile-error", diagnostics, summary: "compilation failed" };
    }
    if (compiled.wasm.length === 0) {
      return { status: "failure", diagnostics, summary: "no module to show" };
    }
    try {
      return {
        status: "ok",
        module: { wat: await printer(compiled.wasm, onLoading) },
        diagnostics,
        summary: "",
      };
    } catch (error) {
      return {
        status: "failure",
        diagnostics,
        summary: `the WAT printer failed: ${String(error)}`,
      };
    }
  }

  return { ready, compiler, runProject, compileProject, watProject };
}

/**
 * Runs what `compileProject` built: Run instantiates the program with the
 * browser host, and Test runs its cases. It needs no compiler, so the run
 * worker calls it.
 */
export async function runCompiled(
  run: CompiledRun,
  onStdout: (line: string, debug?: boolean) => void = () => undefined,
): Promise<RunResult> {
  const started = performance.now() - run.milliseconds;
  const stdout: string[] = [];
  const emit = (line: string, debug = false): void => {
    stdout.push(line);
    onStdout(line, debug);
  };
  const finish: Finish = (status, diagnostics, summary) => ({
    status,
    diagnostics,
    stdout,
    summary,
    milliseconds: Math.round(performance.now() - started),
  });
  const { compiled, diagnostics, who } = run;
  if (run.mode === "test") {
    try {
      return await runTests(compiled, diagnostics, emit, finish);
    } catch (error) {
      const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
      return finish("failure", diagnostics, message);
    }
  }
  // Standard output streams line by line. Standard error carries `dbg`
  // lines, shown marked as debug output (spec/cli/command-line.md#debug-output),
  // and the report of a panic, which becomes the summary.
  let report: string | undefined;
  const out = lineStream((line) => emit(line));
  const err = lineStream((line) => {
    if (report !== undefined) {
      report += `; ${line}`;
    } else if (/^(panic|internal error): /.test(line)) {
      report = line.replace(/^panic: /, "");
    } else {
      emit(line, true);
    }
  });
  const host = createWebHost(
    { out: (text) => out.write(text), err: (text) => err.write(text) },
    { args: [], program: PROGRAM },
  );
  try {
    const module = await WebAssembly.compile(compiled.wasm as BufferSource);
    // A trait the browser cannot provide stops the run before it starts.
    const missing = unboundTraits(module, host);
    if (missing.length > 0) {
      return finish(
        "failure",
        diagnostics,
        `${who} needs ${missing.join(" and ")}, which the playground does not provide; run it with hd run`,
      );
    }
    const instance = await WebAssembly.instantiate(module, host.imports);
    const { status, trapped } = await host.run(instance, "hd.init", "hd.poll");
    out.end();
    err.end();
    if (trapped) {
      const summary = report ?? "the program trapped";
      return finish(
        summary.startsWith("internal error") ? "failure" : "panic",
        diagnostics,
        summary,
      );
    }
    if (status !== 0) {
      return finish("failure", diagnostics, `${who} exited with code ${status}`);
    }
    return finish("ok", diagnostics, "exited normally");
  } catch (error) {
    const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    return finish("failure", diagnostics, message);
  } finally {
    host.close();
  }
}
