// Check and Run on the Rust compiler (playground-wasm.md §3, W5). The
// compiler wasm of crates/hd_web compiles the project; Run instantiates
// the program with the browser host, host/web.mjs, as `hd run` does on
// Node with host/node.mjs. The worker loads one runner, so the compiler and
// its in-memory store live as long as the worker.
//
// Test, the WAT view and the REPL panel stay on the prototype compiler
// (runner.ts) until W6, W8 and W7.

import { createWebHost, unboundTraits } from "../../../host/web.mjs";
import {
  GOAL,
  STATUS,
  compileRequest,
  encodeRequest,
  type CompilerExports,
  type WireDiagnostic,
} from "./compiler-wire.ts";
import type { Project } from "./project.ts";
import type { RunDiagnostic, RunResult } from "./runner.ts";

/** The modes the Rust compiler serves so far. */
export type RustMode = "run" | "check";

export interface RustRunner {
  /** Settles once the compiler wasm is instantiated, or failed to be. */
  readonly ready: Promise<void>;
  runProject(
    project: Project,
    mode: RustMode,
    onStdout?: (line: string, debug?: boolean) => void,
  ): Promise<RunResult>;
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
      if (rest !== "") each(rest);
      rest = "";
    },
  };
}

/**
 * A runner over the compiler wasm `bytes` (the file
 * `pnpm run build:web-compiler` writes); it instantiates them once.
 */
export function createRustRunner(bytes: Promise<BufferSource> | BufferSource): RustRunner {
  const compiler = Promise.resolve(bytes).then(
    async (source) =>
      (await WebAssembly.instantiate(source, {})).instance.exports as unknown as CompilerExports,
  );
  const ready = compiler.then(
    () => undefined,
    () => undefined,
  );

  async function runProject(
    project: Project,
    mode: RustMode,
    onStdout: (line: string, debug?: boolean) => void = () => undefined,
  ): Promise<RunResult> {
    const started = performance.now();
    const stdout: string[] = [];
    const emit = (line: string, debug = false): void => {
      stdout.push(line);
      onStdout(line, debug);
    };
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

    let exports: CompilerExports;
    try {
      exports = await compiler;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return finish("failure", [], `the compiler did not load: ${message}`);
    }
    const request = encodeRequest(project.files, project.main, GOAL[mode]);
    const compiled = compileRequest(exports, request);
    if (compiled.status === STATUS.malformed)
      return finish("failure", [], `the compiler refused the request: ${compiled.json.error}`);
    const diagnostics = (compiled.json.diagnostics ?? []).map((diagnostic) =>
      toRunDiagnostic(diagnostic, project.main),
    );
    if (compiled.status === STATUS.errors)
      return finish("compile-error", diagnostics, "compilation failed");
    if (mode === "check") return finish("ok", diagnostics, "no errors");
    if (compiled.wasm.length === 0) return finish("failure", diagnostics, "no program to run");

    const who = declaresMain(project.files[project.main] ?? "") ? "main" : "the program";
    // Standard output streams line by line. Standard error carries `dbg`
    // lines, shown marked as debug output (spec/cli/command-line.md#debug-output),
    // and the report of a panic, which becomes the summary.
    let report: string | undefined;
    const out = lineStream((line) => emit(line));
    const err = lineStream((line) => {
      if (report !== undefined) report += `; ${line}`;
      else if (/^(panic|internal error): /.test(line)) report = line.replace(/^panic: /, "");
      else emit(line, true);
    });
    const host = createWebHost(
      { out: (text) => out.write(text), err: (text) => err.write(text) },
      { args: [], program: PROGRAM },
    );
    try {
      const module = await WebAssembly.compile(compiled.wasm as BufferSource);
      // A trait the browser cannot provide stops the run before it starts.
      const missing = unboundTraits(module, host);
      if (missing.length > 0)
        return finish(
          "failure",
          diagnostics,
          `${who} needs ${missing.join(" and ")}, which the playground does not provide; run it with hd run`,
        );
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
      if (status !== 0) return finish("failure", diagnostics, `${who} exited with code ${status}`);
      return finish("ok", diagnostics, "exited normally");
    } catch (error) {
      const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
      return finish("failure", diagnostics, message);
    } finally {
      host.close();
    }
  }

  return { ready, runProject };
}
