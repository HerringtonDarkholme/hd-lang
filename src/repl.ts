import { clearLine, createInterface, cursorTo } from "node:readline";

import { analyze, instantiate, type CompileOptions } from "./compiler.ts";
import type { Diagnostic } from "./diagnostics.ts";
import { highlight, highlightLines } from "./highlight.ts";
import type { HirData, HirEnum, HirProgram } from "./hir.ts";
import { RuntimePanicError } from "./runtime-panic.ts";

// The REPL keeps a session as ordinary hd source: accepted declarations at the
// top level, and accepted statements in the body of a synthesized entry point.
// Each input is checked and run as a whole program. Programs are
// deterministic, so console output already shown is skipped on reruns.

const VALUE = "__repl_value";
const SHOW = "__repl_show";
const DECLARATION_WORDS = new Set([
  "fn",
  "pub",
  "data",
  "enum",
  "trait",
  "impl",
  "use",
  "type",
  "annotate",
  "test",
]);
const STATEMENT_WORDS = new Set([
  "let",
  "for",
  "while",
  "return",
  "defer",
  "break",
  "continue",
  "pass",
]);
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

export interface ReplOutcome {
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

interface RunResult {
  readonly lines: readonly string[];
  readonly error?: string;
}

export type InputKind = "declaration" | "statement" | "expression";

export class ReplSession {
  private declarations: string[] = [];
  private statements: string[] = [];
  private shownLines = 0;
  private readonly options: CompileOptions;

  constructor(options: CompileOptions = {}) {
    this.options = options;
  }

  reset(): void {
    this.declarations = [];
    this.statements = [];
    this.shownLines = 0;
  }

  /** The complete program the session currently stands for. */
  source(): string {
    return this.program(this.declarations, this.statements).source;
  }

  async evaluate(input: string): Promise<ReplOutcome> {
    const text = input.replace(/\s+$/, "");
    if (text.trim() === "") return { output: [], errors: [], warnings: [], accepted: true };
    const kind = classifyInput(text);
    if (kind === "declaration") return this.evaluateDeclaration(text);
    if (kind === "statement") return this.evaluateStatement(text);
    return this.evaluateExpression(text);
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

  private async evaluateDeclaration(text: string): Promise<ReplOutcome> {
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

  private async evaluateStatement(text: string): Promise<ReplOutcome> {
    const statements = [...this.statements, text];
    const attempt = this.program(this.declarations, statements);
    const analysis = analyze(attempt.source, this.options);
    if (!analysis.hir) return rejected(this.format(analysis.diagnostics, attempt));
    const run = await this.run(attempt.source);
    if (run.error) return rejected([run.error], run.lines.slice(this.shownLines));
    const output = run.lines.slice(this.shownLines);
    this.statements = statements;
    this.shownLines = run.lines.length;
    return {
      output,
      errors: [],
      warnings: this.warnings(analysis.diagnostics, attempt),
      accepted: true,
    };
  }

  private async evaluateExpression(text: string): Promise<ReplOutcome> {
    const probe: Attempt = {
      ...this.program(this.declarations, [...this.statements, `${VALUE} := ${text}`]),
      prefix: `${VALUE} := `.length,
    };
    const analysis = analyze(probe.source, this.options);
    if (!analysis.hir) {
      // A void call, or a statement form such as `if` without `else`, is not a
      // value; run it as a statement. Its diagnostics are reported unless it
      // does not even parse as a statement.
      const statement = await this.evaluateStatement(text);
      if (statement.accepted) return statement;
      const statementIsSyntax = statement.errors.every(isSyntaxMessage);
      const probeIsSyntax = analysis.diagnostics.every(({ code }) => isSyntaxCode(code));
      return statementIsSyntax && !probeIsSyntax
        ? rejected(this.format(analysis.diagnostics, probe))
        : statement;
    }
    const type = valueType(analysis.hir) ?? "void";
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
      const main = compilation.hir.functions.find(({ name }) => name === "main");
      const entry = instance.exports.main;
      if (!main || typeof entry !== "function") return { lines, error: "internal: no entry point" };
      entry(...main.requirements.map((requirement) => ({ requirement })));
      return { lines };
    } catch (error) {
      if (error instanceof RuntimePanicError) return { lines, error: `panic: ${error.code}` };
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

function formatReplDiagnostic(diagnostic: Diagnostic, attempt: Attempt): string {
  const { line, column } = diagnostic.span.start;
  const relative = line - attempt.inputLine;
  const shift = attempt.indent + (relative === 1 ? (attempt.prefix ?? 0) : 0);
  const where =
    relative >= 1 ? `${relative}:${Math.max(1, column - shift)}` : `session:${line}:${column}`;
  const severity = diagnostic.severity === "warning" ? "warning: " : "";
  return `${where}: ${severity}${diagnostic.code}: ${diagnostic.message}`;
}

/** Classifies one complete input by its leading words and top-level tokens. */
export function classifyInput(text: string): InputKind {
  const first = text.trimStart();
  const word = /^[A-Za-z_][A-Za-z0-9_]*/.exec(first)?.[0] ?? "";
  if (first.startsWith("@")) return "declaration";
  if (word === "fn") return /^fn!?\s*\(/.test(first) ? "expression" : "declaration";
  if (DECLARATION_WORDS.has(word)) return "declaration";
  if (STATEMENT_WORDS.has(word)) return "statement";
  return hasTopLevelBinding(first.split("\n")[0]!) ? "statement" : "expression";
}

function hasTopLevelBinding(line: string): boolean {
  let depth = 0;
  let quote: string | undefined;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index]!;
    if (quote) {
      if (character === "\\") index += 1;
      else if (character === quote) quote = undefined;
      continue;
    }
    if (character === '"' || character === "'") quote = character;
    else if (character === "#") return false;
    else if ("([{".includes(character)) depth += 1;
    else if (")]}".includes(character)) depth -= 1;
    else if (depth === 0 && character === ":" && line[index + 1] === "=") return true;
    else if (depth === 0 && character === ":") return false;
    else if (
      depth === 0 &&
      character === "=" &&
      line[index + 1] !== "=" &&
      line[index + 1] !== ">" &&
      !"=!<>".includes(line[index - 1] ?? "")
    )
      return true;
  }
  return false;
}

/** True when more lines are needed before the input can be evaluated. */
export function needsMoreInput(lines: readonly string[]): boolean {
  let depth = 0;
  let block = false;
  for (const line of lines) {
    let quote: string | undefined;
    let code = "";
    for (let index = 0; index < line.length; index += 1) {
      const character = line[index]!;
      if (quote) {
        if (character === "\\") index += 1;
        else if (character === quote) quote = undefined;
        continue;
      }
      if (character === "#") break;
      if (character === '"' || character === "'") quote = character;
      else if ("([{".includes(character)) depth += 1;
      else if (")]}".includes(character)) depth -= 1;
      code += character;
    }
    if (depth === 0 && /:\s*$/.test(code)) block = true;
  }
  if (depth > 0) return true;
  // A block ends with an empty line, as in Python's interactive mode.
  return block && lines.at(-1)?.trim() !== "";
}

function valueType(hir: HirProgram): string | undefined {
  const main = hir.functions.find(({ name }) => name === "main");
  return main?.locals.findLast(({ name }) => name === VALUE)?.type;
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
    return ["match value:", `    present? => ${inner}(present)`, '    nil => "nil"'];
  }
  const generic = splitGeneric(type);
  if (generic?.name === "list" && generic.arguments.length === 1) {
    const element = nameFor(generic.arguments[0]!);
    return [
      'let text: string = "["',
      "let first: bool = true",
      "for item in value:",
      "    if not first:",
      '        text = text + ", "',
      "    first = false",
      `    text = text + ${element}(item)`,
      'text + "]"',
    ];
  }
  if (generic?.name === "map" && generic.arguments.length === 2) {
    const key = nameFor(generic.arguments[0]!);
    const entry = nameFor(generic.arguments[1]!);
    return [
      'let text: string = "{"',
      "let first: bool = true",
      "for key, entry in value:",
      "    if not first:",
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
      `    Ok(success) => "Ok(" + ${ok}(success) + ")"`,
      `    Err(failure) => "Err(" + ${error}(failure) + ")"`,
    ];
  }
  if (type.startsWith("(") && type.endsWith(")")) {
    const elements = splitTopLevel(type.slice(1, -1));
    const parts = elements.map((element, index) => `${nameFor(element)}(value.${index})`);
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

export interface ReplIo {
  readonly input: NodeJS.ReadableStream;
  readonly output: NodeJS.WritableStream;
  readonly terminal?: boolean;
  /** Syntax coloring; defaults to on for a terminal unless NO_COLOR is set. */
  readonly color?: boolean;
}

const RED = "\u001b[31m";
const YELLOW = "\u001b[33m";
const DIM = "\u001b[2m";
const RESET = "\u001b[0m";

function colorEnabled(terminal: boolean): boolean {
  return terminal && process.env.NO_COLOR === undefined && process.env.TERM !== "dumb";
}

/** Runs an interactive session until end of input or `:quit`. */
export async function runRepl(
  io: ReplIo = { input: process.stdin, output: process.stdout },
  options: CompileOptions = {},
): Promise<number> {
  const session = new ReplSession(options);
  const terminal = io.terminal ?? Boolean((io.output as { isTTY?: boolean }).isTTY);
  const reader = createInterface({ input: io.input, output: io.output, terminal });
  const write = (text: string): void => {
    io.output.write(`${text}\n`);
  };
  let closed = false;
  reader.on("close", () => {
    closed = true;
  });
  let pending: string[] = [];
  const promptText = (): string => (pending.length === 0 ? "hd> " : "... ");
  const prompt = (): void => {
    if (!terminal || closed) return;
    reader.setPrompt(promptText());
    reader.prompt();
  };
  const color = io.color ?? colorEnabled(terminal);
  const paint = (code: string, text: string): string => (color ? `${code}${text}${RESET}` : text);
  if (color) {
    // Readline echoes plain text; after it handles a key, redraw the edited
    // line in color. The return key is handled first, so the submitted line
    // stays colored after readline moves to the next line.
    const redraw = (): void => {
      if (closed) return;
      const position = reader.getCursorPos();
      const columns = (io.output as { columns?: number }).columns || 80;
      if (position.rows > 0 || promptText().length + reader.line.length >= columns) return;
      cursorTo(io.output, 0);
      const line = reader.line.trimStart().startsWith(":") ? reader.line : highlight(reader.line);
      io.output.write(promptText() + line);
      clearLine(io.output, 1);
      cursorTo(io.output, position.cols);
    };
    const isReturn = (key?: { name?: string }): boolean =>
      key?.name === "return" || key?.name === "enter";
    io.input.prependListener("keypress", (_text: string, key?: { name?: string }) => {
      if (isReturn(key)) redraw();
    });
    io.input.on("keypress", (_text: string, key?: { name?: string }) => {
      if (!isReturn(key)) redraw();
    });
  }
  const report = (outcome: ReplOutcome): void => {
    for (const text of outcome.output) write(text);
    for (const warning of outcome.warnings) write(paint(YELLOW, warning));
    for (const error of outcome.errors) write(paint(RED, error));
    if (outcome.value !== undefined)
      write(
        color
          ? `${highlight(outcome.value)}${paint(DIM, ` : ${outcome.type}`)}`
          : `${outcome.value} : ${outcome.type}`,
      );
  };
  if (terminal) write("hd repl. Type :help for commands, :quit to leave.");
  prompt();
  for await (const line of reader) {
    if (pending.length === 0 && line.trim().startsWith(":")) {
      const trimmed = line.trim();
      const command = trimmed.split(/\s+/)[0]!;
      const argument = trimmed.slice(command.length).trim();
      if (command === ":quit" || command === ":q" || command === ":exit") break;
      if (command === ":help") write(HELP);
      else if (command === ":reset") {
        session.reset();
        write("session reset");
      } else if (command === ":source") {
        const source = session.source().trimEnd();
        write(color ? highlightLines(source) : source);
      } else if (command === ":type" && argument !== "") {
        const result = session.typeOf(argument);
        for (const error of result.errors) write(paint(RED, error));
        if (result.type) write(color ? highlight(result.type) : result.type);
      } else write(`unknown command ${command}; type :help`);
      prompt();
      continue;
    }
    pending.push(line);
    if (needsMoreInput(pending)) {
      prompt();
      continue;
    }
    const input = pending.join("\n");
    pending = [];
    report(await session.evaluate(input));
    prompt();
  }
  if (pending.length > 0) report(await session.evaluate(pending.join("\n")));
  reader.close();
  if (terminal) write("");
  return 0;
}
