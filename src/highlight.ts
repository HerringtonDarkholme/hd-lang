import { KEYWORDS } from "./lexer.ts";

// ANSI syntax coloring for one line of hd source. The scanner is tolerant of
// partial input (an unterminated string or interpolation colors to the end of
// the line), because the REPL colors the line while it is being typed.

export type TokenClass =
  | "keyword"
  | "literal"
  | "type"
  | "function"
  | "number"
  | "string"
  | "interpolation"
  | "comment"
  | "operator"
  | "plain";

const COLORS: Readonly<Record<TokenClass, string>> = {
  keyword: "\u001b[35m",
  literal: "\u001b[33m",
  type: "\u001b[36m",
  function: "\u001b[34m",
  number: "\u001b[33m",
  string: "\u001b[32m",
  interpolation: "\u001b[1;32m",
  comment: "\u001b[90m",
  operator: "\u001b[37m",
  plain: "",
};
const RESET = "\u001b[0m";

const LITERAL_WORDS = new Set(["true", "false", "nil", "self", "Self"]);
const PRIMITIVE_TYPES = new Set([
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
  "char",
  "string",
  "void",
  "never",
  "list",
  "map",
]);
// Contextual words that are keywords only in declaration or statement heads.
const CONTEXTUAL_KEYWORDS = new Set(["test", "with"]);

export interface Span {
  readonly text: string;
  readonly kind: TokenClass;
}

const isIdentifierStart = (character: string): boolean => /^[\p{ID_Start}_]$/u.test(character);
const isIdentifierPart = (character: string): boolean => /^[\p{ID_Continue}_!]$/u.test(character);

/** Splits one line into classified spans whose texts concatenate to `line`. */
export function classify(line: string): Span[] {
  const spans: Span[] = [];
  const push = (text: string, kind: TokenClass): void => {
    if (text === "") return;
    const last = spans.at(-1);
    if (last && last.kind === kind) spans[spans.length - 1] = { text: last.text + text, kind };
    else spans.push({ text, kind });
  };
  let index = 0;
  while (index < line.length) {
    const character = line[index]!;
    if (character === "#") {
      push(line.slice(index), "comment");
      break;
    }
    if (character === '"' || (character === "r" && line[index + 1] === '"')) {
      index = scanString(line, index, push);
      continue;
    }
    if (character === "'") {
      const end = scanChar(line, index);
      push(line.slice(index, end), "string");
      index = end;
      continue;
    }
    if (/[0-9]/.test(character)) {
      const match =
        /^(?:0[xob][0-9a-fA-F_]+|[0-9][0-9_]*(?:\.[0-9][0-9_]*)?(?:[eE][+-]?[0-9]+)?)(?:[iuf](?:8|16|32|64))?/.exec(
          line.slice(index),
        )!;
      push(match[0], "number");
      index += match[0].length;
      continue;
    }
    if (isIdentifierStart(character)) {
      let end = index + 1;
      while (end < line.length && isIdentifierPart(line[end]!)) end += 1;
      const word = line.slice(index, end);
      push(word, wordClass(word, line, end));
      index = end;
      continue;
    }
    if (/\s/.test(character)) {
      let end = index + 1;
      while (end < line.length && /\s/.test(line[end]!)) end += 1;
      push(line.slice(index, end), "plain");
      index = end;
      continue;
    }
    push(character, "()[]{},.".includes(character) ? "plain" : "operator");
    index += 1;
  }
  return spans;
}

function wordClass(word: string, line: string, end: number): TokenClass {
  if (LITERAL_WORDS.has(word)) return "literal";
  if (KEYWORDS.has(word)) return "keyword";
  if (CONTEXTUAL_KEYWORDS.has(word) && /^\s*(?:"|:)/.test(line.slice(end))) return "keyword";
  if (PRIMITIVE_TYPES.has(word) || /^\p{Lu}/u.test(word)) return "type";
  if (/^\s*(?:\(|\[[^\]]*\]\s*\()/.test(line.slice(end))) return "function";
  return "plain";
}

function scanChar(line: string, start: number): number {
  let index = start + 1;
  while (index < line.length) {
    if (line[index] === "\\") index += 2;
    else if (line[index] === "'") return index + 1;
    else index += 1;
  }
  return line.length;
}

function scanString(
  line: string,
  start: number,
  push: (text: string, kind: TokenClass) => void,
): number {
  const raw = line[start] === "r";
  let index = start + (raw ? 2 : 1);
  push(line.slice(start, index), "string");
  while (index < line.length) {
    const character = line[index]!;
    if (!raw && character === "\\") {
      push(line.slice(index, index + 2), "string");
      index += 2;
      continue;
    }
    if (character === '"') {
      push('"', "string");
      return index + 1;
    }
    if (!raw && character === "$" && line[index + 1] === "{") {
      const end = interpolationEnd(line, index + 2);
      push("${", "interpolation");
      for (const span of classify(line.slice(index + 2, end))) push(span.text, span.kind);
      if (line[end] === "}") push("}", "interpolation");
      index = Math.min(line.length, end + 1);
      continue;
    }
    if (!raw && character === "$" && isIdentifierStart(line[index + 1] ?? "")) {
      let end = index + 2;
      while (end < line.length && /[\p{ID_Continue}_]/u.test(line[end]!)) end += 1;
      push(line.slice(index, end), "interpolation");
      index = end;
      continue;
    }
    push(character, "string");
    index += 1;
  }
  return index;
}

function interpolationEnd(line: string, start: number): number {
  let depth = 0;
  let index = start;
  while (index < line.length) {
    const character = line[index]!;
    if (character === '"') {
      index += 1;
      while (index < line.length && line[index] !== '"') index += line[index] === "\\" ? 2 : 1;
    } else if (character === "{") depth += 1;
    else if (character === "}") {
      if (depth === 0) return index;
      depth -= 1;
    }
    index += 1;
  }
  return line.length;
}

/** Returns `line` with ANSI color codes; the visible text is unchanged. */
export function highlight(line: string): string {
  return classify(line)
    .map(({ text, kind }) => (COLORS[kind] === "" ? text : `${COLORS[kind]}${text}${RESET}`))
    .join("");
}

/** Colors each line of a multi-line hd fragment. */
export function highlightLines(text: string): string {
  return text.split("\n").map(highlight).join("\n");
}
