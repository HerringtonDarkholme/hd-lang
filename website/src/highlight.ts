// Syntax coloring for one line of hd source: `classify` gives the token
// classes the playground editor, the website's code blocks and the REPL use,
// and `highlight` renders them with ANSI colors for a terminal. The scanner
// is tolerant of partial input (an unterminated string or interpolation
// colors to the end of the line), because the REPL colors the line while it
// is being typed.

/**
 * The reserved words, as the Rust lexer (`keyword` in
 * crates/hd_syntax/src/lexer.rs) lexes them; a test checks the two lists match.
 */
export const KEYWORDS: ReadonlySet<string> = new Set([
  "Self",
  "break",
  "continue",
  "data",
  "dyn",
  "else",
  "enum",
  "false",
  "fn",
  "for",
  "if",
  "impl",
  "in",
  "is",
  "let",
  "match",
  "mut",
  "pass",
  "pub",
  "return",
  "self",
  "struct",
  "tests",
  "trait",
  "true",
  "type",
  "while",
]);

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

const LITERAL_WORDS = new Set(["true", "false", "self", "Self"]);
const PRIMITIVE_TYPES = new Set([
  "i8",
  "i16",
  "i32",
  "i64",
  "u8",
  "u16",
  "u32",
  "u64",
  "usize",
  "f32",
  "f64",
  "bool",
  "char",
  "string",
  "void",
  "never",
]);
// Contextual words directly after `$.`: the provider operations and the
// context type (01-lexical-structure.md#contextual-words).
const PROVIDER_WORDS = new Set(["use", "with", "context", "Context"]);
// A use declaration: `use` or `pub use` followed by a use root.
const USE_DECLARATION = /^\s*(?:pub\s+)?use\s+(?:pkg|std|dep|self|super)\b/;

interface Span {
  readonly text: string;
  readonly kind: TokenClass;
}

const isIdentifierStart = (character: string): boolean => /^[\p{ID_Start}_]$/u.test(character);
const isIdentifierPart = (character: string): boolean => /^[\p{ID_Continue}_!]$/u.test(character);

function scanNumber(line: string, index: number): string {
  return /^(?:0[xob][0-9a-fA-F_]+|[0-9][0-9_]*(?:\.[0-9][0-9_]*)?(?:[eE][+-]?[0-9]+)?(?:\p{XID_Start}[\p{XID_Continue}_]*)?)/u.exec(
    line.slice(index),
  )![0];
}

function identifierEnd(line: string, index: number): number {
  let end = index + 1;
  while (
    end < line.length &&
    isIdentifierPart(line[end]!) &&
    !(line[end] === "!" && line[end + 1] === "=")
  ) {
    end += 1;
  }
  return end;
}

function whitespaceEnd(line: string, index: number): number {
  let end = index + 1;
  while (end < line.length && /\s/.test(line[end]!)) {
    end += 1;
  }
  return end;
}

function rawIdentifier(line: string, index: number): { end: number; kind: TokenClass } {
  const raw = /^`[\p{ID_Continue}_]+`/u.exec(line.slice(index));
  return raw ? { end: index + raw[0].length, kind: "plain" } : { end: index + 1, kind: "operator" };
}

/** Splits one line into classified spans whose texts concatenate to `line`. */
export function classify(line: string): Span[] {
  const spans: Span[] = [];
  const push = (text: string, kind: TokenClass): void => {
    if (text === "") {
      return;
    }
    const last = spans.at(-1);
    if (last && last.kind === kind) {
      spans[spans.length - 1] = { text: last.text + text, kind };
    } else {
      spans.push({ text, kind });
    }
  };
  let index = 0;
  while (index < line.length) {
    const character = line[index]!;
    if (character === "#") {
      push(line.slice(index), "comment");
      break;
    }
    if (character === '"') {
      index = scanString(line, index, 0, push);
      continue;
    }
    if (character === "'") {
      // A name with no closing quote is a lifetime (01-lexical-structure.md#lifetimes).
      const lifetime = /^'[\p{ID_Start}_][\p{ID_Continue}_]*(?!['\p{ID_Continue}_])/u.exec(
        line.slice(index),
      );
      if (lifetime) {
        push(lifetime[0], "plain");
        index += lifetime[0].length;
        continue;
      }
      const end = scanChar(line, index);
      push(line.slice(index, end), "string");
      index = end;
      continue;
    }
    if (/[0-9]/.test(character)) {
      // A literal suffix such as `250ms` stays part of the number; radix
      // literals take none (01-lexical-structure.md#literal-suffixes).
      const number = scanNumber(line, index);
      push(number, "number");
      index += number.length;
      continue;
    }
    if (character === "`") {
      // A raw identifier is always a plain name, whatever word it encloses.
      const { end, kind } = rawIdentifier(line, index);
      push(line.slice(index, end), kind);
      index = end;
      continue;
    }
    if (isIdentifierStart(character)) {
      const end = identifierEnd(line, index);
      const word = line.slice(index, end);
      // A prefixed string such as `sql"..."` is one string, prefix included
      // (01-lexical-structure.md#prefixed-strings).
      if (line[end] === '"' && !KEYWORDS.has(word) && !LITERAL_WORDS.has(word) && word !== "_") {
        index = scanString(line, index, end - index, push);
        continue;
      }
      push(word, wordClass(word, line, index, end));
      index = end;
      continue;
    }
    if (/\s/.test(character)) {
      const end = whitespaceEnd(line, index);
      push(line.slice(index, end), "plain");
      index = end;
      continue;
    }
    push(character, "()[]{},.".includes(character) ? "plain" : "operator");
    index += 1;
  }
  return spans;
}

function wordClass(word: string, line: string, start: number, end: number): TokenClass {
  if (LITERAL_WORDS.has(word)) {
    return "literal";
  }
  if (KEYWORDS.has(word)) {
    return "keyword";
  }
  if (contextualKeyword(word, line, start, end)) {
    return "keyword";
  }
  if (PRIMITIVE_TYPES.has(word) || /^\p{Lu}/u.test(word)) {
    return "type";
  }
  if (/^\s*(?:\(|\[[^\]]*\]\s*\()/.test(line.slice(end))) {
    return "function";
  }
  return "plain";
}

// `use`, `super`, and `as` in a use declaration, a provider word after `$.`,
// `derive` after `@`, and `by` after the target of an `impl` header (with or
// without a trait) are keywords
// (01-lexical-structure.md#contextual-words).
function contextualKeyword(word: string, line: string, start: number, end: number): boolean {
  if (word === "using" || word === "using!") {
    return /^\s*$/.test(line.slice(0, start)) && /^\s+\p{ID_Start}/u.test(line.slice(end));
  }
  if (PROVIDER_WORDS.has(word) && line.slice(0, start).endsWith("$.")) {
    return true;
  }
  if (word === "derive") {
    return line[start - 1] === "@";
  }
  if (word === "by") {
    return /^\s*impl\b.*\S\s+$/.test(line.slice(0, start)) && /^\s+\S/.test(line.slice(end));
  }
  if (word === "use") {
    return USE_DECLARATION.test(line) && /^\s*(?:pub\s+)?$/.test(line.slice(0, start));
  }
  if (word === "super" || word === "as") {
    return USE_DECLARATION.test(line);
  }
  return false;
}

function scanChar(line: string, start: number): number {
  let index = start + 1;
  while (index < line.length) {
    if (line[index] === "\\") {
      index += 2;
    } else if (line[index] === "'") {
      return index + 1;
    } else {
      index += 1;
    }
  }
  return line.length;
}

// A string from its prefix (`prefixLength` characters, none for an
// interpreted string) or opening quote. A prefixed string's backslash is
// text, but it still keeps the next quote or `$` from acting, so both forms
// scan a backslash pair as string text.
function scanString(
  line: string,
  start: number,
  prefixLength: number,
  push: (text: string, kind: TokenClass) => void,
): number {
  let index = start + prefixLength + 1;
  push(line.slice(start, index), "string");
  while (index < line.length) {
    const character = line[index]!;
    if (character === "\\") {
      push(line.slice(index, index + 2), "string");
      index += 2;
      continue;
    }
    if (character === '"') {
      push('"', "string");
      return index + 1;
    }
    if (character === "$" && line[index + 1] === "{") {
      const end = interpolationEnd(line, index + 2);
      push("${", "interpolation");
      for (const span of classify(line.slice(index + 2, end))) {
        push(span.text, span.kind);
      }
      if (line[end] === "}") {
        push("}", "interpolation");
      }
      index = Math.min(line.length, end + 1);
      continue;
    }
    if (character === "$" && isIdentifierStart(line[index + 1] ?? "")) {
      let end = index + 2;
      while (end < line.length && /[\p{ID_Continue}_]/u.test(line[end]!)) {
        end += 1;
      }
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
      while (index < line.length && line[index] !== '"') {
        index += line[index] === "\\" ? 2 : 1;
      }
    } else if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      if (depth === 0) {
        return index;
      }
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
