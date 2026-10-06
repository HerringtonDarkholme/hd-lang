import {
  analyze,
  instantiate,
  type CompileOptions,
  type HostSuspensionCall,
  type HostSuspensionOutcome,
} from "./compiler.ts";
import type { Diagnostic, SourcePosition, SourceSpan } from "./diagnostics.ts";
import { physicalSpan, SOURCE_ORIGIN, sourceDocument } from "./diagnostics.ts";
import {
  LIB_FILE,
  linkedParseOptions,
  linkPackage,
  type PackageDependencies,
  type PackageDiagnostic,
} from "./package.ts";
import type { HirExpression, HirProgram } from "./hir.ts";
import { RuntimePanicError } from "./runtime-panic.ts";
import { classifyInput } from "./repl-input.ts";
import { displayType } from "./types.ts";

export {
  backspaceWidth,
  boundNames,
  classifyInput,
  continuationIndent,
  FollowOnErrors,
  INDENT_UNIT,
  needsMoreInput,
  splitInputs,
  type InputKind,
  type SourceInput,
} from "./repl-input.ts";

// The REPL keeps a session as ordinary hd source: accepted declarations at the
// top level, and accepted statements in the body of a synthesized entry point.
// Each input is checked and run as a whole program. Programs are
// deterministic, so console output already shown is skipped on reruns.

const VALUE = "__repl_value";
/**
 * `std.format`'s hidden `dbg_text`, the REPL's value display: a value's text
 * as `dbg` prints it (spec/cli/command-line.md#r-cli.repl.value).
 */
const DBG_TEXT = "__std_format_dbg_text";

interface ReplOutcome {
  /** Console output and `dbg` lines produced by this input only, in order. */
  readonly output: readonly string[];
  /** The indices of `output` that are `dbg` lines (spec/cli/command-line.md#r-cli.dbg.repl). */
  readonly debug?: readonly number[];
  /** The rendered value of an expression input, if any. */
  readonly value?: string;
  /** The static type of an expression input, if any. */
  readonly type?: string;
  /** Diagnostics and runtime failures, already formatted. */
  readonly errors: readonly string[];
  /** Non-fatal diagnostics located in this input. */
  readonly warnings: readonly string[];
  readonly accepted: boolean;
}

interface Attempt {
  readonly source: string;
  /** The line before the input's first line in `source`. */
  readonly inputLine: number;
  readonly indent: number;
  /** Extra columns on the input's first line, such as a synthesized binding. */
  readonly prefix?: number;
}

interface EvaluateOptions {
  /** False to type-check an input without running it; defaults to true. */
  readonly run?: boolean;
}

/** A line a run wrote: console output, or a `dbg` line. */
interface RunLine {
  readonly text: string;
  readonly debug?: true;
}

interface RunResult {
  readonly lines: readonly RunLine[];
  readonly error?: string;
  /** The host answers of the run: the accepted inputs' replayed, then this input's. */
  readonly hostAnswers: readonly HostAnswer[];
}

/**
 * The package a session started in (spec/cli/command-line.md#r-cli.repl.package.lib):
 * its `.hd` files by package path, and its executables' entry modules.
 */
export interface ReplPackage {
  readonly files: Readonly<Record<string, string>>;
  /** The `[package]` name (LinkOptions.packageName). */
  readonly name?: string;
  readonly programs: readonly string[];
  /**
   * The selected dependency packages. The session may use the dependencies
   * and the dev dependencies (spec/cli/command-line.md#r-cli.repl.package.dependencies).
   */
  readonly dependencies?: PackageDependencies;
}

/**
 * The host a session binds, as `hd FILE` does
 * (spec/cli/command-line.md#r-cli.repl.host.default-profile): the traits of
 * its profile, which the session's entry row names, and their answers.
 */
export interface ReplHost {
  /** Each trait by its module and name, such as `std.time` and `Clock`. */
  readonly traits: readonly { readonly module: string; readonly name: string }[];
  /** The answer to a call on one of `traits`, or undefined for any other call. */
  readonly invoke: (call: HostSuspensionCall) => HostSuspensionOutcome | undefined;
  /**
   * Sees the std host traits an input's module imports, before it runs; it
   * throws a {@link ReplRefusal} to refuse a totally denied one
   * (spec/cli/command-line.md#r-cli.cap.total.refuse).
   */
  readonly needs?: (traits: readonly string[]) => void;
}

/** A host's refusal to start an input's module, whose message the session shows. */
export class ReplRefusal extends Error {}

/** One host call of an accepted input, kept so a rerun answers it again without its effect. */
interface HostAnswer {
  readonly method: string;
  readonly outcome: HostSuspensionOutcome;
}

/** The prefix of the names the session gives the host traits, apart from the user's. */
const HOST_ALIAS = "__repl_host_";

interface PreparedSource {
  readonly source: string;
  readonly options: CompileOptions;
  /** Moves a diagnostic on `source` to session or package-file coordinates. */
  readonly located: (diagnostic: Diagnostic) => Diagnostic;
  /** The link errors, when the session does not link with its package. */
  readonly failure?: readonly Diagnostic[];
}

export class ReplSession {
  private declarations: string[] = [];
  private statements: string[] = [];
  private shownLines = 0;
  private lastModule: string | undefined;
  private modules = 0;
  /** The host answers of the accepted inputs, in call order. */
  private hostAnswers: readonly HostAnswer[] = [];
  private readonly options: CompileOptions;
  private readonly package?: ReplPackage;
  private readonly host?: ReplHost;

  /**
   * With `pkg`, the session acts as code inside the package's `src/lib.hd`
   * (spec/cli/command-line.md#r-cli.repl.package.lib): its source joins the
   * end of that file, and the linker joins the package's modules it uses.
   * With `host`, the session's entry row names the host's traits, and their
   * calls go to the host (spec/cli/command-line.md#r-cli.repl.host.default-profile).
   */
  constructor(options: CompileOptions = {}, pkg?: ReplPackage, host?: ReplHost) {
    this.options = options;
    if (pkg) this.package = pkg;
    if (host) this.host = host;
  }

  /** The session source as the compiler sees it: alone, or linked into its package. */
  private prepare(source: string): PreparedSource {
    const pkg = this.package;
    if (!pkg) return { source, options: this.options, located: (diagnostic) => diagnostic };
    const lib = pkg.files[LIB_FILE] ?? "";
    const prefix = lib === "" || lib.endsWith("\n") ? lib : `${lib}\n`;
    const libLines = prefix.split("\n").length - 1;
    const files: Readonly<Record<string, string>> = {
      ...pkg.files,
      [LIB_FILE]: `${prefix}${source}`,
    };
    // The session joins src/lib.hd, yet sees the dev dependencies too, so
    // both tables link as dependencies (cli.repl.package.dependencies).
    const graph = pkg.dependencies;
    const linked = linkPackage(files, LIB_FILE, {
      programs: pkg.programs,
      ...(pkg.name === undefined ? {} : { packageName: pkg.name }),
      ...(graph
        ? {
            dependencies: {
              ...graph,
              dependencies: { ...graph.devDependencies, ...graph.dependencies },
            },
          }
        : {}),
    });
    // A diagnostic in the session's own lines keeps its session line; one in
    // a package file names that file (cli.repl.package.lib).
    const located = (diagnostic: Diagnostic): Diagnostic => {
      if (sourceDocument(diagnostic.span)) return diagnostic;
      const found =
        "path" in diagnostic ? (diagnostic as PackageDiagnostic) : linked.locate(diagnostic);
      const line = found.span.start.line;
      if (found.path === LIB_FILE && line > libLines) {
        const move = (position: SourcePosition): SourcePosition => ({
          ...position,
          line: position.line - libLines,
        });
        return {
          ...diagnostic,
          span: { start: move(found.span.start), end: move(found.span.end) },
        };
      }
      const document = { file: found.path, text: files[found.path] ?? "" };
      const origin = (position: SourcePosition): SourcePosition => ({
        ...position,
        [SOURCE_ORIGIN]: { document, ...position },
      });
      return {
        ...diagnostic,
        span: { start: origin(found.span.start), end: origin(found.span.end) },
      };
    };
    if (!linked.source)
      return { source, options: this.options, located, failure: linked.diagnostics.map(located) };
    return {
      source: linked.source,
      options: {
        ...this.options,
        parse: { ...this.options.parse, ...linkedParseOptions(linked) },
      },
      located,
    };
  }

  /** Checks a session program, with its diagnostics in session coordinates. */
  private analyze(source: string): {
    readonly hir?: HirProgram;
    readonly diagnostics: readonly Diagnostic[];
  } {
    const prepared = this.prepare(source);
    if (prepared.failure) return { diagnostics: prepared.failure };
    const analysis = analyze(prepared.source, prepared.options);
    return { ...analysis, diagnostics: analysis.diagnostics.map(prepared.located) };
  }

  reset(): void {
    this.declarations = [];
    this.statements = [];
    this.shownLines = 0;
    this.lastModule = undefined;
    this.modules = 0;
    this.hostAnswers = [];
  }

  /**
   * Each input that runs compiles its own module. This is the WAT text of the
   * last one and how many the session has compiled, for tools that show the
   * generated Wasm.
   */
  compiledModule(): { readonly wat: string; readonly count: number } | undefined {
    return this.lastModule === undefined
      ? undefined
      : { wat: this.lastModule, count: this.modules };
  }

  /** The complete program the session currently stands for. */
  source(): string {
    return this.program(this.declarations, this.statements).source;
  }

  /**
   * Checks an input and, unless `run` is false, runs it. With `run: false`
   * an accepted input is kept after type-checking, and an expression reports
   * its type but no value. Checked statements run with the next input
   * that does run, so use a session either to check or to run.
   */
  async evaluate(input: string, { run = true }: EvaluateOptions = {}): Promise<ReplOutcome> {
    const text = input.replace(/\s+$/, "");
    if (text.trim() === "") return { output: [], errors: [], warnings: [], accepted: true };
    const kind = classifyInput(text);
    if (kind === "declaration") return this.declare(text);
    if (kind === "statement") return this.evaluateStatement(text, run);
    return this.evaluateExpression(text, run);
  }

  /** The type of an expression, without running or keeping it. */
  typeOf(input: string): { readonly type?: string; readonly errors: readonly string[] } {
    const text = input.trim();
    const attempt = this.program(this.declarations, [...this.statements, `${VALUE} := ${text}`]);
    const analysis = this.analyze(attempt.source);
    if (!analysis.hir)
      return {
        errors: this.format(analysis.diagnostics, { ...attempt, prefix: `${VALUE} := `.length }),
      };
    const found = valueType(analysis.hir);
    return found === undefined
      ? { errors: ["expression has no value"] }
      : { type: displayType(found.type), errors: [] };
  }

  /** Adds `text` as top-level declarations, whatever its leading word. */
  async declare(text: string): Promise<ReplOutcome> {
    const declarations = [...this.declarations, text];
    const attempt = this.program(declarations, this.statements, text);
    const analysis = this.analyze(attempt.source);
    if (!analysis.hir) return rejected(this.format(analysis.diagnostics, attempt));
    this.declarations = declarations;
    return {
      output: [],
      errors: [],
      warnings: this.warnings(analysis.diagnostics, attempt),
      accepted: true,
    };
  }

  private async evaluateStatement(text: string, execute: boolean): Promise<ReplOutcome> {
    const statements = [...this.statements, text];
    const attempt = this.program(this.declarations, statements);
    const analysis = this.analyze(attempt.source);
    if (!analysis.hir) return rejected(this.format(analysis.diagnostics, attempt));
    const warnings = this.warnings(analysis.diagnostics, attempt);
    if (!execute) {
      this.statements = statements;
      return { output: [], errors: [], warnings, accepted: true };
    }
    const run = await this.run(attempt);
    if (run.error) return rejected([run.error], run.lines.slice(this.shownLines));
    const output = run.lines.slice(this.shownLines);
    this.statements = statements;
    this.shownLines = run.lines.length;
    this.hostAnswers = run.hostAnswers;
    return { ...outputOf(output), errors: [], warnings, accepted: true };
  }

  private async evaluateExpression(text: string, execute: boolean): Promise<ReplOutcome> {
    const probe: Attempt = {
      ...this.program(this.declarations, [...this.statements, `${VALUE} := ${text}`]),
      prefix: `${VALUE} := `.length,
    };
    const analysis = this.analyze(probe.source);
    if (!analysis.hir) {
      // A void call, or a statement form such as `if` without `else`, is not a
      // value; run it as a statement. Its diagnostics are reported unless it
      // does not even parse as a statement.
      const statement = await this.evaluateStatement(text, execute);
      if (statement.accepted) return statement;
      const statementIsSyntax = statement.errors.every(isSyntaxMessage);
      const probeIsSyntax = analysis.diagnostics.every(({ code }) => isSyntaxCode(code));
      return statementIsSyntax && !probeIsSyntax
        ? rejected(this.format(analysis.diagnostics, probe))
        : statement;
    }
    const found = valueType(analysis.hir);
    // `void` is the empty tuple, so a void call binds; only the `()` literal
    // has a value to show, any other void input runs as a statement.
    if (found?.type === "void" && !found.unitLiteral) return this.evaluateStatement(text, execute);
    const shownType = displayType(found?.type ?? "void");
    if (!execute) {
      this.statements = [...this.statements, `_ := ${text}`];
      return {
        output: [],
        type: shownType,
        errors: [],
        warnings: this.warnings(analysis.diagnostics, probe),
        accepted: true,
      };
    }
    // The value shows as `dbg` prints it, with ` : TYPE` after it on its
    // last line (spec/cli/command-line.md#r-cli.repl.value).
    const width = ` : ${shownType}`.length;
    const shown: Attempt = {
      ...this.program(this.declarations, [
        ...this.statements,
        `${VALUE} := ${text}`,
        `println(${DBG_TEXT}(${VALUE}, ${width}))`,
      ]),
      inputLine: probe.inputLine,
      prefix: `${VALUE} := `.length,
    };
    const shownAnalysis = this.analyze(shown.source);
    if (!shownAnalysis.hir) return rejected(this.format(shownAnalysis.diagnostics, shown));
    const run = await this.run(shown);
    if (run.error) return rejected([run.error], run.lines.slice(this.shownLines));
    const output = run.lines.slice(this.shownLines, -1);
    this.statements = [...this.statements, `_ := ${text}`];
    this.shownLines = run.lines.length - 1;
    this.hostAnswers = run.hostAnswers;
    return {
      ...outputOf(output),
      value: run.lines.at(-1)?.text,
      type: shownType,
      errors: [],
      warnings: this.warnings(analysis.diagnostics, probe),
      accepted: true,
    };
  }

  private program(
    declarations: readonly string[],
    statements: readonly string[],
    input?: string,
  ): Attempt {
    const lines: string[] = [];
    let inputLine = 0;
    // The host's traits under names of the session's own, so the entry row
    // can name them whatever the inputs import (cli.repl.host.default-profile).
    const traits = this.host?.traits ?? [];
    for (const { module, name } of traits)
      lines.push(`use ${module}.{${name} as ${HOST_ALIAS}${name}}`);
    if (traits.length > 0) lines.push("");
    for (const declaration of declarations) {
      if (declaration === input) inputLine = lines.length;
      lines.push(...declaration.split("\n"), "");
    }
    const row = ["Console", ...traits.map(({ name }) => `${HOST_ALIAS}${name}`)];
    lines.push(
      traits.length === 0
        ? "pub fn main() -> void $ Console:"
        : `pub fn main!() -> void $ ${row.join(" + ")}:`,
    );
    const body = statements.length === 0 ? ["pass"] : statements;
    body.forEach((statement, index) => {
      if (input === undefined && index === body.length - 1) inputLine = lines.length;
      for (const line of statement.split("\n")) lines.push(`    ${line}`);
    });
    lines.push("");
    return { source: lines.join("\n"), inputLine, indent: input === undefined ? 4 : 0 };
  }

  private format(diagnostics: readonly Diagnostic[], attempt: Attempt): string[] {
    return diagnostics
      .filter((diagnostic) => diagnostic.severity !== "warning")
      .map((diagnostic) => formatReplDiagnostic(diagnostic, attempt));
  }

  private warnings(diagnostics: readonly Diagnostic[], attempt: Attempt): string[] {
    return diagnostics
      .filter(
        (diagnostic) =>
          diagnostic.severity === "warning" &&
          diagnostic.code !== "unused-local-binding" &&
          diagnostic.span.start.line > attempt.inputLine,
      )
      .map((diagnostic) => formatReplDiagnostic(diagnostic, attempt));
  }

  private async run(attempt: Attempt): Promise<RunResult> {
    const { source } = attempt;
    const lines: RunLine[] = [];
    // An accepted input's host calls are answered from the record on a
    // rerun, so their effects happen once (cli.repl.host.once).
    const replayed = this.hostAnswers;
    const hostAnswers: HostAnswer[] = [];
    const host = this.host;
    const prepared = this.prepare(source);
    if (prepared.failure)
      return { lines, error: "internal: the session does not link", hostAnswers: replayed };
    // Once a call differs from the record, as after a redeclared function,
    // every later call goes to the host.
    let diverged = false;
    const hostSuspensionInvoke = (call: HostSuspensionCall): HostSuspensionOutcome => {
      const method = `${call.standardName ?? call.providerKey}.${call.methodName}`;
      const kept = diverged ? undefined : replayed[hostAnswers.length];
      diverged ||= kept?.method !== method;
      const outcome = kept && !diverged ? kept.outcome : (host?.invoke(call) ?? { pending: false });
      hostAnswers.push({ method, outcome });
      return outcome;
    };
    try {
      const { instance, compilation } = await instantiate(prepared.source, {
        ...prepared.options,
        console: (text) => lines.push({ text }),
        consoleError: (text) => lines.push({ text }),
        // A `dbg` line prints before the input's value, and names its call as
        // a diagnostic would (spec/cli/command-line.md#r-cli.dbg.repl).
        debugOutput: (text) => lines.push({ text, debug: true }),
        debugLocation: (span: SourceSpan) =>
          replLocation(prepared.located({ code: "", message: "", span }), attempt),
        ...(host ? { hostSuspensionInvoke } : {}),
        ...(host?.needs ? { needs: host.needs } : {}),
      });
      this.lastModule = compilation.wat;
      this.modules += 1;
      const main = compilation.hir.functions.find(({ name }) => name === "main");
      const entry = instance.exports.main;
      if (!main || typeof entry !== "function")
        return { lines, error: "internal: no entry point", hostAnswers };
      entry(...main.requirements.map((requirement) => ({ requirement })));
      return { lines, hostAnswers };
    } catch (error) {
      const message =
        error instanceof RuntimePanicError
          ? replPanic(error)
          : error instanceof ReplRefusal
            ? error.message
            : `internal error: ${(error as Error).message}`;
      return { lines, error: message, hostAnswers };
    }
  }
}

function rejected(errors: readonly string[], output: readonly RunLine[] = []): ReplOutcome {
  return { ...outputOf(output), errors, warnings: [], accepted: false };
}

/** An outcome's output: each line's text, and which of them are `dbg` lines. */
function outputOf(lines: readonly RunLine[]): Pick<ReplOutcome, "output" | "debug"> {
  const debug = lines.flatMap((line, index) => (line.debug ? [index] : []));
  return { output: lines.map(({ text }) => text), ...(debug.length > 0 ? { debug } : {}) };
}

function isSyntaxCode(code: string): boolean {
  return (
    code === "syntax-error" ||
    code.startsWith("expected-") ||
    code.startsWith("unexpected-") ||
    code === "unclosed-delimiter" ||
    code === "unmatched-delimiter"
  );
}

function isSyntaxMessage(message: string): boolean {
  const code = /^(?:session:)?\d+:\d+: ([a-z0-9-]+):/.exec(message)?.[1];
  return code !== undefined && isSyntaxCode(code);
}

/** A REPL error or warning line, split back into its parts. */
export interface ReplMessage {
  /** A non-session source, such as a standard-library file. */
  readonly file?: string;
  /** Line and column in the input, when the message points into it. */
  readonly line?: number;
  readonly column?: number;
  /** A line of the session's program before the input, such as a declaration. */
  readonly sessionLine?: number;
  readonly severity: "error" | "warning";
  /** A diagnostic code, `runtime-panic`, or `internal-error`. */
  readonly code: string;
  /** The diagnostic message, or the panic code for `runtime-panic`. */
  readonly message: string;
  /** A panic's notes, such as the fix of a `usize` fallback. */
  readonly notes?: readonly string[];
}

/**
 * A panic's error text: `panic at LOCATION: CODE: detail`, where LOCATION is
 * where `replLocation` puts the operation that panicked, when it is known,
 * then a line for each note.
 */
function replPanic(error: RuntimePanicError): string {
  const at = error.location ? ` at ${error.location}` : "";
  const what = error.detail ? `${error.code}: ${error.detail}` : error.code;
  return `panic${at}: ${what}${error.notes.map((note) => `\n  note: ${note}`).join("")}`;
}

/** Parses a line of `ReplOutcome.errors` or `ReplOutcome.warnings`. */
export function parseReplMessage(text: string): ReplMessage {
  const panic = parseReplPanic(text);
  if (panic) return panic;
  const sourced = /^(.+):(\d+):(\d+): (warning: )?([a-z0-9-]+): ([\s\S]*)$/.exec(text);
  if (sourced && sourced[1] !== "session") {
    const [, file, line, column, warning, code, message] = sourced;
    return {
      file,
      line: Number(line),
      column: Number(column),
      severity: warning ? "warning" : "error",
      code: code!,
      message: message!,
    };
  }
  const located = /^(session:)?(\d+):(\d+): (warning: )?([a-z0-9-]+): ([\s\S]*)$/.exec(text);
  if (located) {
    const [, session, line, column, warning, code, message] = located;
    const position = session
      ? { sessionLine: Number(line) }
      : { line: Number(line), column: Number(column) };
    return { ...position, severity: warning ? "warning" : "error", code: code!, message: message! };
  }
  return { severity: "error", code: "internal-error", message: text };
}

/** A `replPanic` text: its position, when it names one, its code and detail, and its notes. */
function parseReplPanic(text: string): ReplMessage | undefined {
  const panic = /^panic(?: at (?:(\S+?):)?(\d+):(\d+))?: (.*)((?:\n {2}note: .*)*)$/.exec(text);
  if (!panic) return undefined;
  const [, file, line, column, message, notes] = panic;
  const position =
    line === undefined
      ? {}
      : file === "session"
        ? { sessionLine: Number(line) }
        : { ...(file ? { file } : {}), line: Number(line), column: Number(column) };
  return {
    ...position,
    severity: "error",
    code: "runtime-panic",
    message: message!,
    ...(notes ? { notes: notes.split("\n  note: ").slice(1) } : {}),
  };
}

function formatReplDiagnostic(diagnostic: Diagnostic, attempt: Attempt): string {
  const severity = diagnostic.severity === "warning" ? "warning: " : "";
  return `${replLocation(diagnostic, attempt)}: ${severity}${diagnostic.code}: ${diagnostic.message}`;
}

/**
 * Where a diagnostic points: `LINE:COLUMN` in the input, `session:LINE:COLUMN`
 * in an earlier part of the session, or `FILE:LINE:COLUMN` in another file.
 */
function replLocation(diagnostic: Diagnostic, attempt: Attempt): string {
  const document = sourceDocument(diagnostic.span);
  if (document) {
    const { line, column } = physicalSpan(diagnostic.span).start;
    return `${document.file}:${line}:${column}`;
  }
  const { line, column } = diagnostic.span.start;
  const relative = line - attempt.inputLine;
  const shift = attempt.indent + (relative === 1 ? (attempt.prefix ?? 0) : 0);
  return relative >= 1 ? `${relative}:${Math.max(1, column - shift)}` : `session:${line}:${column}`;
}

/** The type of the input's value, and whether it is the `()` literal. */
function valueType(
  hir: HirProgram,
): { readonly type: string; readonly unitLiteral?: boolean } | undefined {
  const main = hir.functions.find(({ name }) => name === "main");
  if (!main) return undefined;
  // `:=` always binds a readonly view, so the binding's own type loses `mut`.
  // Report the type of the expression the user wrote instead.
  const binding = findValueBinding(main.body);
  if (binding) {
    const value =
      binding.value.kind === "permission-weaken" ? binding.value.operand : binding.value;
    return { type: value.type, unitLiteral: value.kind === "tuple" };
  }
  const local = main.locals.findLast(({ name }) => name === VALUE);
  return local && { type: local.type };
}

interface BindingNode {
  readonly kind: "binding";
  readonly local: { readonly name: string };
  readonly value: HirExpression;
}

function findValueBinding(node: unknown): BindingNode | undefined {
  let found: BindingNode | undefined;
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    if (typeof value !== "object" || value === null) return;
    const record = value as Record<string, unknown>;
    if (
      record.kind === "binding" &&
      (record.local as { name?: string } | undefined)?.name === VALUE &&
      typeof record.value === "object"
    )
      found = record as unknown as BindingNode;
    for (const child of Object.values(record)) visit(child);
  };
  visit(node);
  return found;
}

const HELP = `Enter hd declarations, statements, or expressions.
A line ending in ':' starts a block; finish it with an empty line.
Expressions print their value and type. Declarations cannot see REPL bindings.

  :help          show this help
  :type EXPR     show the type of an expression
  :source        show the program the session stands for
  :reset         forget every declaration and binding
  :quit          leave (also Ctrl-D)`;

/** One line of a REPL reply, by what it shows. */
export interface ReplEntry {
  /**
   * `output`: console output; `debug`: a `dbg` line; `value`: an
   * expression's rendered value, with `type`; `code`: hd source or a type to
   * highlight; `info`: plain text.
   */
  readonly kind: "output" | "debug" | "value" | "code" | "info" | "warning" | "error";
  readonly text: string;
  readonly type?: string;
}

/** What one input or `:` command produced. */
export interface ReplReply {
  readonly entries: readonly ReplEntry[];
  /** True when the input was accepted and is now part of the session. */
  readonly kept: boolean;
  /** Set when the input was `:reset` or `:quit`. */
  readonly command?: "reset" | "quit";
}

/** Whether `text` is a `:` command rather than hd input. */
export function isReplCommand(text: string): boolean {
  return text.trim().startsWith(":");
}

/**
 * Answers one complete input or `:` command. Every REPL front end, the
 * terminal and the web page's worker, goes through this.
 */
export async function respond(session: ReplSession, input: string): Promise<ReplReply> {
  if (!isReplCommand(input)) {
    const outcome = await session.evaluate(input);
    return { entries: outcomeEntries(outcome), kept: outcome.accepted && input.trim() !== "" };
  }
  const trimmed = input.trim();
  const command = trimmed.split(/\s+/)[0]!;
  const argument = trimmed.slice(command.length).trim();
  const info = (text: string): ReplReply => ({ entries: [{ kind: "info", text }], kept: false });
  if (command === ":quit" || command === ":q" || command === ":exit")
    return { entries: [], kept: false, command: "quit" };
  if (command === ":help") return info(HELP);
  if (command === ":reset") {
    session.reset();
    return { ...info("session reset"), command: "reset" };
  }
  if (command === ":source")
    return { entries: [{ kind: "code", text: session.source().trimEnd() }], kept: false };
  if (command === ":type" && argument !== "") {
    const result = session.typeOf(argument);
    const entries: ReplEntry[] = result.errors.map((text) => ({ kind: "error", text }));
    if (result.type) entries.push({ kind: "code", text: result.type });
    return { entries, kept: false };
  }
  return info(`unknown command ${command}; type :help`);
}

/** The reply lines for an evaluated input, in the order the REPL shows them. */
function outcomeEntries(outcome: ReplOutcome): ReplEntry[] {
  const entries: ReplEntry[] = [
    ...outcome.output.map((text, index): ReplEntry => ({
      kind: outcome.debug?.includes(index) ? "debug" : "output",
      text,
    })),
    ...outcome.warnings.map((text): ReplEntry => ({ kind: "warning", text })),
    ...outcome.errors.map((text): ReplEntry => ({ kind: "error", text })),
  ];
  if (outcome.value !== undefined)
    entries.push({ kind: "value", text: outcome.value, type: outcome.type });
  return entries;
}
