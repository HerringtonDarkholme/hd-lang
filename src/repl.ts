import { analyze, instantiate, type CompileOptions } from "./compiler.ts";
import type { Diagnostic } from "./diagnostics.ts";
import type { HirData, HirEnum, HirProgram } from "./hir.ts";
import { RuntimePanicError } from "./runtime-panic.ts";
import { classifyInput } from "./repl-input.ts";

export {
  backspaceWidth,
  classifyInput,
  continuationIndent,
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
const SHOW = "__repl_show";
const DISPLAY_PRIMITIVES = new Set([
  "i8",
  "i16",
  "i32",
  "i64",
  "u8",
  "u16",
  "u32",
  "u64",
  "f32",
  "f64",
  "bool",
]);

interface ReplOutcome {
  /** Console output produced by this input only. */
  readonly output: readonly string[];
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

interface RunResult {
  readonly lines: readonly string[];
  readonly error?: string;
}

export class ReplSession {
  private declarations: string[] = [];
  private statements: string[] = [];
  private shownLines = 0;
  private lastModule: string | undefined;
  private modules = 0;
  private readonly options: CompileOptions;

  constructor(options: CompileOptions = {}) {
    this.options = options;
  }

  reset(): void {
    this.declarations = [];
    this.statements = [];
    this.shownLines = 0;
    this.lastModule = undefined;
    this.modules = 0;
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
    const analysis = analyze(attempt.source, this.options);
    if (!analysis.hir)
      return {
        errors: this.format(analysis.diagnostics, { ...attempt, prefix: `${VALUE} := `.length }),
      };
    const found = valueType(analysis.hir);
    return found === undefined
      ? { errors: ["expression has no value"] }
      : { type: displayType(found), errors: [] };
  }

  /** Adds `text` as top-level declarations, whatever its leading word. */
  async declare(text: string): Promise<ReplOutcome> {
    const declarations = [...this.declarations, text];
    const attempt = this.program(declarations, this.statements, text);
    const analysis = analyze(attempt.source, this.options);
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
    const analysis = analyze(attempt.source, this.options);
    if (!analysis.hir) return rejected(this.format(analysis.diagnostics, attempt));
    const warnings = this.warnings(analysis.diagnostics, attempt);
    if (!execute) {
      this.statements = statements;
      return { output: [], errors: [], warnings, accepted: true };
    }
    const run = await this.run(attempt.source);
    if (run.error) return rejected([run.error], run.lines.slice(this.shownLines));
    const output = run.lines.slice(this.shownLines);
    this.statements = statements;
    this.shownLines = run.lines.length;
    return { output, errors: [], warnings, accepted: true };
  }

  private async evaluateExpression(text: string, execute: boolean): Promise<ReplOutcome> {
    const probe: Attempt = {
      ...this.program(this.declarations, [...this.statements, `${VALUE} := ${text}`]),
      prefix: `${VALUE} := `.length,
    };
    const analysis = analyze(probe.source, this.options);
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
    const type = valueType(analysis.hir) ?? "void";
    if (!execute) {
      this.statements = [...this.statements, `_ := ${text}`];
      return {
        output: [],
        type: displayType(type),
        errors: [],
        warnings: this.warnings(analysis.diagnostics, probe),
        accepted: true,
      };
    }
    const helpers = renderers(type, analysis.hir);
    const shown = this.program(
      [...this.declarations, ...helpers.declarations],
      [...this.statements, `${VALUE} := ${text}`, `println(${helpers.call(VALUE)})`],
    );
    const shownAnalysis = analyze(shown.source, this.options);
    if (!shownAnalysis.hir) return rejected(this.format(shownAnalysis.diagnostics, shown));
    const run = await this.run(shown.source);
    if (run.error) return rejected([run.error], run.lines.slice(this.shownLines));
    const output = run.lines.slice(this.shownLines, -1);
    this.statements = [...this.statements, `_ := ${text}`];
    this.shownLines = run.lines.length - 1;
    return {
      output,
      value: run.lines.at(-1),
      type: displayType(type),
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
    for (const declaration of declarations) {
      if (declaration === input) inputLine = lines.length;
      lines.push(...declaration.split("\n"), "");
    }
    lines.push("pub fn main() -> void $ Console:");
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

  private async run(source: string): Promise<RunResult> {
    const lines: string[] = [];
    try {
      const { instance, compilation } = await instantiate(source, {
        ...this.options,
        console: (text) => lines.push(text),
      });
      this.lastModule = compilation.wat;
      this.modules += 1;
      const main = compilation.hir.functions.find(({ name }) => name === "main");
      const entry = instance.exports.main;
      if (!main || typeof entry !== "function") return { lines, error: "internal: no entry point" };
      entry(...main.requirements.map((requirement) => ({ requirement })));
      return { lines };
    } catch (error) {
      if (error instanceof RuntimePanicError)
        return { lines, error: `panic: ${error.detail ? error.message : error.code}` };
      return { lines, error: `internal error: ${(error as Error).message}` };
    }
  }
}

function rejected(errors: readonly string[], output: readonly string[] = []): ReplOutcome {
  return { output, errors, warnings: [], accepted: false };
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
}

/** Parses a line of `ReplOutcome.errors` or `ReplOutcome.warnings`. */
export function parseReplMessage(text: string): ReplMessage {
  const located = /^(session:)?(\d+):(\d+): (warning: )?([a-z0-9-]+): ([\s\S]*)$/.exec(text);
  if (located) {
    const [, session, line, column, warning, code, message] = located;
    const position = session
      ? { sessionLine: Number(line) }
      : { line: Number(line), column: Number(column) };
    return { ...position, severity: warning ? "warning" : "error", code: code!, message: message! };
  }
  const panic = /^panic: (.*)$/.exec(text);
  if (panic) return { severity: "error", code: "runtime-panic", message: panic[1]! };
  return { severity: "error", code: "internal-error", message: text };
}

function formatReplDiagnostic(diagnostic: Diagnostic, attempt: Attempt): string {
  const { line, column } = diagnostic.span.start;
  const relative = line - attempt.inputLine;
  const shift = attempt.indent + (relative === 1 ? (attempt.prefix ?? 0) : 0);
  const where =
    relative >= 1 ? `${relative}:${Math.max(1, column - shift)}` : `session:${line}:${column}`;
  const severity = diagnostic.severity === "warning" ? "warning: " : "";
  return `${where}: ${severity}${diagnostic.code}: ${diagnostic.message}`;
}

function valueType(hir: HirProgram): string | undefined {
  const main = hir.functions.find(({ name }) => name === "main");
  if (!main) return undefined;
  // `:=` always binds a readonly view, so the binding's own type loses `mut`.
  // Report the type of the expression the user wrote instead.
  const binding = findValueBinding(main.body);
  if (binding) {
    const value = binding.value;
    return value.kind === "permission-weaken" && value.operand ? value.operand.type : value.type;
  }
  return main.locals.findLast(({ name }) => name === VALUE)?.type;
}

interface BindingNode {
  readonly kind: "binding";
  readonly local: { readonly name: string };
  readonly value: {
    readonly kind: string;
    readonly type: string;
    readonly operand?: { readonly type: string };
  };
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

function displayType(type: string): string {
  return type.replaceAll("mut:", "mut ").replace(/,(?! )/g, ", ");
}

interface Renderers {
  readonly declarations: string[];
  call(argument: string): string;
}

/** Generates hd functions that render a value of `type` as source-like text. */
function renderers(type: string, hir: HirProgram): Renderers {
  const names = new Map<string, string>();
  const declarations: string[] = [];
  const nameFor = (valueType: string): string => {
    const key = valueType.replaceAll("mut:", "");
    const existing = names.get(key);
    if (existing) return existing;
    const name = `${SHOW}_${names.size}`;
    names.set(key, name);
    const body = rendererBody(key, hir, nameFor);
    declarations.push(
      [`fn ${name}(value: ${key}) -> string:`, ...body.map((line) => `    ${line}`)].join("\n"),
    );
    return name;
  };
  const top = nameFor(type);
  return { declarations, call: (argument) => `${top}(${argument})` };
}

function rendererBody(type: string, hir: HirProgram, nameFor: (type: string) => string): string[] {
  if (DISPLAY_PRIMITIVES.has(type)) return ['"$value"'];
  if (type === "string") return ['"\\"" + value + "\\""'];
  if (type === "char") return ["\"'$value'\""];
  if (type.endsWith("?")) {
    const inner = nameFor(type.slice(0, -1));
    return [
      "match value:",
      `    .Some(present) => ".Some(" + ${inner}(present) + ")"`,
      '    .None => ".None"',
    ];
  }
  const generic = splitGeneric(type);
  if (generic?.name === "List" && generic.arguments.length === 1) {
    const element = nameFor(generic.arguments[0]!);
    return [
      'let text: string = "["',
      "let first: bool = true",
      "for item in value:",
      "    if !first:",
      '        text = text + ", "',
      "    first = false",
      `    text = text + ${element}(item)`,
      'text + "]"',
    ];
  }
  if (generic?.name === "Map" && generic.arguments.length === 2) {
    const key = nameFor(generic.arguments[0]!);
    const entry = nameFor(generic.arguments[1]!);
    return [
      'let text: string = "{"',
      "let first: bool = true",
      "for (key, entry) in value:",
      "    if !first:",
      '        text = text + ", "',
      "    first = false",
      `    text = text + ${key}(key) + ": " + ${entry}(entry)`,
      'text + "}"',
    ];
  }
  if (generic?.name === "Result" && generic.arguments.length === 2) {
    const ok = nameFor(generic.arguments[0]!);
    const error = nameFor(generic.arguments[1]!);
    return [
      "match value:",
      `    .Ok(success) => ".Ok(" + ${ok}(success) + ")"`,
      `    .Err(failure) => ".Err(" + ${error}(failure) + ")"`,
    ];
  }
  if (type.startsWith("(") && type.endsWith(")")) {
    const elements = splitTopLevel(type.slice(1, -1));
    const parts = elements.map((element, index) => `${nameFor(element)}(value._${index})`);
    return [`"(" + ${parts.join(' + ", " + ')} + ")"`];
  }
  const data = hir.data.find(({ name }) => name === (generic?.name ?? type));
  if (data) return dataBody(data, type, generic?.arguments ?? [], nameFor);
  const enumeration = hir.enums.find(({ name }) => name === (generic?.name ?? type));
  if (enumeration) return enumBody(enumeration, generic?.arguments ?? [], nameFor);
  return [`"<${displayType(type).replaceAll('"', "'")}>"`];
}

function dataBody(
  data: HirData,
  type: string,
  typeArguments: readonly string[],
  nameFor: (type: string) => string,
): string[] {
  const substitute = substitution(data.genericParameters, typeArguments);
  if (data.fields.length === 0) return [`"${type}"`];
  const fields = data.fields.map(
    (field) => `"${field.name}: " + ${nameFor(substitute(field.type))}(value.${field.name})`,
  );
  return [`"${data.name} { " + ${fields.join(' + ", " + ')} + " }"`];
}

function enumBody(
  enumeration: HirEnum,
  typeArguments: readonly string[],
  nameFor: (type: string) => string,
): string[] {
  const substitute = substitution(enumeration.genericParameters, typeArguments);
  const arms = enumeration.variants.map((variant) => {
    const label = `${enumeration.name}.${variant.name}`;
    if (variant.fields.length === 0) return `    .${variant.name} => "${label}"`;
    const names = variant.fields.map((field) => field.name);
    const parts = variant.fields.map(
      (field) => `"${field.name}: " + ${nameFor(substitute(field.type))}(${field.name})`,
    );
    return `    .${variant.name}(${names.join(", ")}) => "${label}(" + ${parts.join(' + ", " + ')} + ")"`;
  });
  return ["match value:", ...arms];
}

function substitution(
  parameters: readonly string[],
  typeArguments: readonly string[],
): (type: string) => string {
  return (type) => {
    let result = type;
    parameters.forEach((parameter, index) => {
      const argument = typeArguments[index];
      if (argument !== undefined)
        result = result.replace(new RegExp(`\\b${parameter}\\b`, "g"), argument);
    });
    return result;
  };
}

function splitGeneric(type: string): { name: string; arguments: string[] } | undefined {
  const match = /^([A-Za-z_][A-Za-z0-9_.]*)\[(.*)\]$/.exec(type);
  if (!match) return undefined;
  return { name: match[1]!, arguments: splitTopLevel(match[2]!) };
}

function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const character of text) {
    if ("([{".includes(character)) depth += 1;
    else if (")]}".includes(character)) depth -= 1;
    if (character === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
    } else current += character;
  }
  if (current.trim() !== "") parts.push(current.trim());
  return parts;
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
   * `output`: console output; `value`: an expression's rendered value, with
   * `type`; `code`: hd source or a type to highlight; `info`: plain text.
   */
  readonly kind: "output" | "value" | "code" | "info" | "warning" | "error";
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
    ...outcome.output.map((text): ReplEntry => ({ kind: "output", text })),
    ...outcome.warnings.map((text): ReplEntry => ({ kind: "warning", text })),
    ...outcome.errors.map((text): ReplEntry => ({ kind: "error", text })),
  ];
  if (outcome.value !== undefined)
    entries.push({ kind: "value", text: outcome.value, type: outcome.type });
  return entries;
}
