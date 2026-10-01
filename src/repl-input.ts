// How the REPL reads its input: which kind of input a text is, when a line
// needs more lines, and how a whole file or snippet divides into inputs.
// This module imports nothing from the compiler, so a web page can use it to
// read input while the compiler itself loads in a worker.

export type InputKind = "declaration" | "statement" | "expression";

const DECLARATION_WORDS = new Set([
  "fn",
  "pub",
  "data",
  "enum",
  "trait",
  "impl",
  "use",
  "type",
  "tests",
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

/** Classifies one complete input by its leading words and top-level tokens. */
export function classifyInput(text: string): InputKind {
  const first = text.trimStart();
  const word = /^[A-Za-z_][A-Za-z0-9_]*/.exec(first)?.[0] ?? "";
  if (first.startsWith("@")) return "declaration";
  if (word === "fn") return /^fn!?\s*\(/.test(first) ? "expression" : "declaration";
  // `use` is contextual: it begins a declaration only before a use root.
  if (word === "use" && !/^use\s+(?:pkg|std|dep|self|super)\b/.test(first))
    return hasTopLevelBinding(first.split("\n")[0]!) ? "statement" : "expression";
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

/**
 * Scans one line outside strings and comments: the bracket depth after it,
 * starting from `depth`, and its code without the comment.
 */
function scanLine(line: string, depth: number): { readonly depth: number; readonly code: string } {
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
  return { depth, code };
}

/** One indentation level, as the formatter and the guide write it. */
export const INDENT_UNIT = "    ";

/**
 * True when a line's code (from `scanLine`) ends where the parser expects an
 * indented suite on the next line: after `:`, or after a match arm's `=>`.
 * The lexer emits an indent for any deeper line, so the opener is this token.
 */
function opensSuite(code: string): boolean {
  return /(?::|=>)\s*$/.test(code);
}

/** True when more lines are needed before the input can be evaluated. */
export function needsMoreInput(lines: readonly string[]): boolean {
  let depth = 0;
  let block = false;
  for (const line of lines) {
    const scanned = scanLine(line, depth);
    depth = scanned.depth;
    if (depth === 0 && opensSuite(scanned.code)) block = true;
  }
  if (depth > 0) return true;
  // A block ends with an empty line, as in Python's interactive mode.
  return block && lines.at(-1)?.trim() !== "";
}

/**
 * The indentation a new line after `line` starts with: the line's leading
 * spaces, plus one level when the line opens an indented suite. A suite can
 * open inside brackets too, as a closure body does.
 */
export function continuationIndent(line: string): string {
  const indent = /^ */.exec(line)![0];
  return opensSuite(scanLine(line, 0).code) ? indent + INDENT_UNIT : indent;
}

/**
 * The spaces Backspace deletes when the text before the cursor is
 * `before`: back to the previous indentation level inside the leading
 * indentation, otherwise one character.
 */
export function backspaceWidth(before: string): number {
  if (before.length === 0) return 0;
  if (!/^ +$/.test(before)) return 1;
  return before.length % INDENT_UNIT.length || INDENT_UNIT.length;
}

/** One input of a file or snippet, with the 1-based line it starts on. */
export interface SourceInput {
  readonly text: string;
  readonly line: number;
}

/**
 * Divides a source text into the inputs a REPL would read, in order. An input
 * starts at a line that begins in column 1 and takes every following line
 * that is indented, blank, a comment, inside open brackets, a closing
 * bracket, or an `else`/`elif` clause. Annotation lines join the declaration
 * after them. Blank lines and comments between inputs are dropped.
 */
export function splitInputs(source: string): SourceInput[] {
  const inputs: SourceInput[] = [];
  let current: { line: number; lines: string[]; depth: number } | undefined;
  const flush = (): void => {
    if (!current) return;
    const lines = current.lines;
    while (lines.length > 0 && /^\s*(?:#.*)?$/.test(lines.at(-1)!)) lines.pop();
    if (lines.length > 0) inputs.push({ text: lines.join("\n"), line: current.line });
    current = undefined;
  };
  source.split("\n").forEach((raw, index) => {
    const line = raw.replace(/\r$/, "");
    const quiet = /^\s*(?:#.*)?$/.test(line);
    const continues =
      current !== undefined &&
      (quiet ||
        current.depth > 0 ||
        /^\s/.test(line) ||
        /^(?:[)\]}]|(?:else|elif)\b)/.test(line) ||
        current.lines.every((text) => text.startsWith("@")));
    if (continues) current!.lines.push(line);
    else if (!quiet) {
      flush();
      current = { line: index + 1, lines: [line], depth: 0 };
    }
    if (current && !quiet) current.depth = Math.max(0, scanLine(line, current.depth).depth);
  });
  flush();
  return inputs;
}
