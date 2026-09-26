import type { Diagnostic, GrammarToken, LexResult } from "./types.ts";

const reserved = new Set([
  "Self",
  "and",
  "annotate",
  "as",
  "break",
  "continue",
  "data",
  "defer",
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
  "nil",
  "not",
  "or",
  "pass",
  "pub",
  "reified",
  "return",
  "self",
  "shape",
  "super",
  "trait",
  "true",
  "type",
  "use",
  "where",
  "while",
]);
export const openToClose = new Map([
  ["(", ")"],
  ["[", "]"],
  ["{", "}"],
]);
export const closeToOpen = new Map([...openToClose].map(([open, close]) => [close, open]));
const multiOperators = ["...", ":=", "->", "=>", "::", "==", "!=", "<=", ">=", "<<", ">>", "**"];
const simpleEscapes = new Set(`\\"'nrt0$`);

interface StringScan {
  readonly diagnostics: readonly Diagnostic[];
  readonly end: number;
  readonly line: number;
  readonly quote: string;
}

interface NumberScan {
  readonly end: number;
  readonly floating: boolean;
}

interface DepthEntry {
  readonly depth: number;
  readonly text: string;
}

// An indentation level of a suite nested inside delimiters. `saved` holds the
// headers still waiting for their `:` after the suite that this level opened.
interface NestedLevel {
  readonly delimiters: number;
  readonly indent: number;
  saved?: DepthEntry[];
}

function diagnostic(code: string, line: number): Diagnostic {
  return { code, line };
}

function token(kinds: string | ReadonlySet<string>, line: number, text: string): GrammarToken {
  return { kinds: typeof kinds === "string" ? new Set([kinds]) : kinds, line, text };
}

function isLetter(character: string): boolean {
  return /^\p{L}$/u.test(character);
}

function isLetterOrNumber(character: string): boolean {
  return /^[\p{L}\p{N}]$/u.test(character);
}

function isDigit(character: string): boolean {
  return /^[0-9]$/.test(character);
}

function startsInterpolatedName(source: string, index: number): boolean {
  const first = source[index] ?? "";
  if (isLetter(first)) return true;
  const second = source[index + 1] ?? "";
  return first === "_" && (second === "_" || isLetterOrNumber(second));
}

function escapeEnd(source: string, index: number): number | undefined {
  const next = source[index + 1] ?? "";
  if (simpleEscapes.has(next)) return index + 2;
  if (next !== "u") return undefined;
  const unicode = /^u\{([0-9A-Fa-f]{1,6})\}/.exec(source.slice(index + 1, index + 11));
  if (!unicode) return undefined;
  const value = Number.parseInt(unicode[1]!, 16);
  if (value > 0x10ffff || (value >= 0xd800 && value <= 0xdfff)) return undefined;
  return index + 1 + unicode[0].length;
}

function startsString(source: string, index: number): boolean {
  const character = source[index]!;
  if (character === '"' || character === "'") return true;
  if (character !== "r" || (source[index + 1] !== '"' && source[index + 1] !== "'")) return false;
  const previous = source[index - 1] ?? "";
  return previous !== "_" && !isLetterOrNumber(previous);
}

interface InterpolationScan {
  readonly diagnostics: readonly Diagnostic[];
  readonly end: number;
  readonly line: number;
  readonly terminated: boolean;
}

function scanInterpolation(
  source: string,
  start: number,
  initialLine: number,
  triple: boolean,
): InterpolationScan {
  const diagnostics: Diagnostic[] = [];
  const stack: string[] = [];
  let index = start;
  let line = initialLine;
  while (index < source.length) {
    const character = source[index]!;
    if (character === "\n") {
      if (!triple) return { diagnostics, end: index, line, terminated: false };
      line += 1;
      index += 1;
      continue;
    }
    if (startsString(source, index)) {
      const nested = scanString(source, index, line);
      diagnostics.push(...nested.diagnostics);
      index = nested.end;
      line = nested.line;
      continue;
    }
    if (openToClose.has(character)) stack.push(character);
    else if (closeToOpen.has(character)) {
      if (stack.length === 0 && character === "}")
        return { diagnostics, end: index + 1, line, terminated: true };
      if (stack.at(-1) === closeToOpen.get(character)) stack.pop();
      else diagnostics.push(diagnostic("unmatched-delimiter", line));
    }
    index += 1;
  }
  return { diagnostics, end: index, line, terminated: false };
}

function scanString(source: string, start: number, initialLine: number): StringScan {
  const diagnostics: Diagnostic[] = [];
  const raw = source[start] === "r";
  const quoteAt = raw ? start + 1 : start;
  const quote = source[quoteAt]!;
  const triple = source.startsWith(quote.repeat(3), quoteAt);
  const terminator = quote.repeat(triple ? 3 : 1);
  const interpolates = !raw && quote === '"';
  let index = quoteAt + terminator.length;
  let line = initialLine;
  while (index < source.length) {
    if (source.startsWith(terminator, index))
      return { diagnostics, end: index + terminator.length, line, quote };
    const character = source[index]!;
    if (character === "\n") {
      if (!triple) {
        diagnostics.push(diagnostic("unterminated-string", initialLine));
        return { diagnostics, end: index, line, quote };
      }
      line += 1;
      index += 1;
      continue;
    }
    if (character === "\\" && raw) {
      // A backslash keeps the next quote from terminating a raw literal, and
      // both characters remain content.
      index += source[index + 1] === "\n" || index + 1 >= source.length ? 1 : 2;
      continue;
    }
    if (character === "\\") {
      if (index + 1 >= source.length || source[index + 1] === "\n") {
        diagnostics.push(diagnostic("invalid-escape", line));
        index += 1;
        continue;
      }
      const end = escapeEnd(source, index);
      if (end === undefined) {
        diagnostics.push(diagnostic("invalid-escape", line));
        index += 2;
      } else index = end;
      continue;
    }
    if (character === "$" && interpolates) {
      if (source[index + 1] === "{") {
        const found = scanInterpolation(source, index + 2, line, triple);
        diagnostics.push(...found.diagnostics);
        line = found.line;
        index = found.end;
        if (!found.terminated) {
          diagnostics.push(diagnostic("unterminated-string", initialLine));
          return { diagnostics, end: index, line, quote };
        }
        continue;
      }
      if (!startsInterpolatedName(source, index + 1))
        diagnostics.push(diagnostic("syntax-error", line));
      index += 1;
      continue;
    }
    index += 1;
  }
  diagnostics.push(diagnostic("unterminated-string", initialLine));
  return { diagnostics, end: index, line, quote };
}

/** Replace each string or character literal with empty quotes and drop comments, keeping line breaks. */
export function maskLiterals(source: string): string {
  let result = "";
  let index = 0;
  while (index < source.length) {
    const character = source[index]!;
    if (character === "#") {
      while (index < source.length && source[index] !== "\n") index += 1;
      continue;
    }
    if (startsString(source, index)) {
      const found = scanString(source, index, 1);
      const quote = found.quote;
      result += quote + quote + "\n".repeat(found.line - 1);
      index = Math.max(found.end, index + 1);
      continue;
    }
    result += character;
    index += 1;
  }
  return result;
}

const digits = (digit: string): string => `${digit}(?:_?${digit})*`;
const validNumberPattern = new RegExp(
  `^(?:0[xX]_?${digits("[0-9A-Fa-f]")}|0[bB]_?${digits("[01]")}|0[oO]_?${digits("[0-7]")}` +
    `|${digits("[0-9]")}(?:\\.${digits("[0-9]")})?(?:[eE][+-]?${digits("[0-9]")})?)$`,
);

/** Chapter 01 separator rule: an underscore only between digits, or once after a radix prefix. */
function validNumber(text: string): boolean {
  return validNumberPattern.test(text);
}

function numberEnd(source: string, start: number, afterDot: boolean): NumberScan {
  const rest = source.slice(start);
  const based = /^(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+)/.exec(rest);
  if (based) return { end: start + based[0].length, floating: false };
  const integer = /^[0-9][0-9_]*/.exec(rest)!;
  let end = start + integer[0].length;
  let floating = false;
  if (!afterDot && source[end] === "." && isDigit(source[end + 1] ?? "")) {
    floating = true;
    end += 1;
    end += /^[0-9][0-9_]*/.exec(source.slice(end))![0].length;
  }
  if (/[eE]/.test(source[end] ?? "")) {
    const exponent = /^[eE][+-]?[0-9][0-9_]*/.exec(source.slice(end));
    if (exponent) {
      floating = true;
      end += exponent[0].length;
    }
  }
  return { end, floating };
}

// True when the token before the just-pushed `fn` is `->` (or `->` then `mut`).
function typeResultStart(tokens: readonly GrammarToken[]): boolean {
  const previous = tokens.at(-2)?.text;
  return previous === "->" || (previous === "mut" && tokens.at(-3)?.text === "->");
}

export function lexSource(source: string): LexResult {
  const tokens: GrammarToken[] = [];
  const diagnostics: Diagnostic[] = [];
  const indents = [0];
  const delimiters: DepthEntry[] = [];
  let index = 0;
  let line = 1;
  let atLineStart = true;
  let lineHasToken = false;
  let previousText = "";
  let pendingHeaders: DepthEntry[] = [];
  const inlineSuites: number[] = [];
  let pendingForcedSuite: NestedLevel | undefined;
  const forcedIndents: NestedLevel[] = [];

  while (index < source.length) {
    if (atLineStart) {
      const start = index;
      let indent = 0;
      while (index < source.length && " \t".includes(source[index]!)) {
        if (source[index] === "\t") {
          diagnostics.push(diagnostic("tab-whitespace", line));
          indent += 4;
        } else indent += 1;
        index += 1;
      }
      if (index >= source.length) break;
      if (source[index] === "#" || source[index] === "\n") {
        while (index < source.length && source[index] !== "\n") index += 1;
        if (index < source.length) {
          index += 1;
          line += 1;
        }
        atLineStart = true;
        continue;
      }
      if (delimiters.length > 0) {
        if (pendingForcedSuite) {
          if (indent <= pendingForcedSuite.indent)
            diagnostics.push(diagnostic("unexpected-indentation", line));
          forcedIndents.push({ ...pendingForcedSuite, indent });
          tokens.push(token("INDENT", line, "<indent>"));
          pendingForcedSuite = undefined;
        } else {
          while (forcedIndents.length > 0 && indent < forcedIndents.at(-1)!.indent) {
            const closed = forcedIndents.pop()!;
            tokens.push(token("DEDENT", line, "<dedent>"));
            if (closed.saved) pendingHeaders.push(...closed.saved);
          }
          // A deeper line inside a nested suite body opens an ordinary nested block.
          const top = forcedIndents.at(-1);
          if (top && top.delimiters === delimiters.length && indent > top.indent) {
            forcedIndents.push({ delimiters: top.delimiters, indent });
            tokens.push(token("INDENT", line, "<indent>"));
          }
        }
      } else if (indent > indents.at(-1)!) {
        indents.push(indent);
        tokens.push(token("INDENT", line, "<indent>"));
      } else if (indent < indents.at(-1)!) {
        while (indents.length > 1 && indent < indents.at(-1)!) {
          indents.pop();
          tokens.push(token("DEDENT", line, "<dedent>"));
        }
        if (indent !== indents.at(-1)) diagnostics.push(diagnostic("invalid-dedent", line));
      }
      atLineStart = false;
      if (index === start && source[index] === "\ufeff" && index === 0) {
        index += 1;
        continue;
      }
    }

    if (index >= source.length) break;
    const character = source[index]!;
    if (character === "\ufeff") {
      if (index !== 0) diagnostics.push(diagnostic("unexpected-bom", line));
      index += 1;
      continue;
    }
    if (" \r\t".includes(character)) {
      index += 1;
      continue;
    }
    if (character === "#") {
      while (index < source.length && source[index] !== "\n") index += 1;
      continue;
    }
    if (character === "\n") {
      // Layout is active at delimiter depth zero and at the depth of the
      // innermost nested suite; deeper line breaks are implicit continuation.
      const layoutDepth = forcedIndents.at(-1)?.delimiters ?? 0;
      if (delimiters.length > layoutDepth && !pendingForcedSuite) {
        line += 1;
        index += 1;
        atLineStart = false;
        continue;
      }
      // Same-line suites and headers opened at a shallower delimiter depth end
      // at their own comma or closing delimiter, not at this line break.
      const depth = delimiters.length;
      const closing = inlineSuites.filter((entry) => entry >= depth).length;
      if (closing > 0) {
        for (let count = 0; count < closing; count += 1)
          tokens.push(token("SUITE_END", line, "<suite-end>"));
        inlineSuites.length -= closing;
      } else if (lineHasToken)
        tokens.push(token(new Set(["NEWLINE", "SUITE_END"]), line, "<newline>"));
      line += 1;
      index += 1;
      atLineStart = true;
      lineHasToken = false;
      previousText = "";
      // Headers at this depth that enclose a nested suite resume after it.
      if (pendingForcedSuite)
        pendingForcedSuite.saved = pendingHeaders.filter((entry) => entry.depth === depth);
      pendingHeaders = pendingHeaders.filter((entry) => entry.depth < depth);
      continue;
    }

    const rawString = character === "r" && (source[index + 1] === '"' || source[index + 1] === "'");
    if (character === '"' || character === "'" || rawString) {
      const found = scanString(source, index, line);
      diagnostics.push(...found.diagnostics);
      const text = source.slice(index, found.end);
      const kind = found.quote === "'" && !rawString ? "char_literal" : "string_literal";
      tokens.push(token(kind, line, text));
      line = found.line;
      index = found.end;
      lineHasToken = true;
      previousText = text;
      continue;
    }

    if (
      character === "_" &&
      (index + 1 === source.length ||
        (!isLetterOrNumber(source[index + 1]!) && source[index + 1] !== "_"))
    ) {
      tokens.push(token("_", line, character));
      index += 1;
      lineHasToken = true;
      previousText = character;
      continue;
    }
    if (character === "_" || isLetter(character)) {
      let end = index + 1;
      while (end < source.length && (source[end] === "_" || isLetterOrNumber(source[end]!)))
        end += 1;
      const word = source.slice(index, end);
      const depth = delimiters.length;
      if (word === "else" && inlineSuites.at(-1) === depth) {
        inlineSuites.pop();
        tokens.push(token("SUITE_END", line, "<suite-end>"));
      }
      let kinds: ReadonlySet<string>;
      if (word === "true" || word === "false") kinds = new Set([word, "boolean_literal"]);
      else if (word === "nil") kinds = new Set([word, "nil_literal"]);
      else if (reserved.has(word)) kinds = new Set([word]);
      else kinds = new Set([word, "identifier"]);
      tokens.push(token(kinds, line, word));
      let suiteWord = new Set([
        "defer",
        "fn",
        "if",
        "while",
        "match",
        "else",
        "data",
        "test",
        "annotate",
        "with",
      ]).has(word);
      // A function type in a closure's result position never takes a `:`.
      if (word === "fn" && typeResultStart(tokens)) suiteWord = false;
      // A `for` loop expression follows an operator, an opening delimiter, or
      // a header word. A comprehension clause `for` follows a complete
      // expression; a leading one after `[` or `{` loses its header at `=>`.
      if (word === "for")
        suiteWord = new Set([
          "",
          ":=",
          "=",
          "return",
          "break",
          ":",
          "=>",
          "(",
          "[",
          "{",
          ",",
          "...",
          "@",
          "if",
          "while",
          "in",
          "match",
        ]).has(previousText);
      if (suiteWord) pendingHeaders.push({ depth, text: word });
      if (word === "in") {
        const header = pendingHeaders.findLastIndex(
          (entry) => entry.depth === depth && entry.text === "for",
        );
        if (header >= 0) pendingHeaders[header] = { depth, text: "for-in" };
      }
      index = end;
      lineHasToken = true;
      previousText = word;
      continue;
    }
    if (isDigit(character)) {
      const found = numberEnd(source, index, previousText === ".");
      const text = source.slice(index, found.end);
      if (!validNumber(text)) diagnostics.push(diagnostic("invalid-token", line));
      tokens.push(token(found.floating ? "float_literal" : "integer_literal", line, text));
      index = found.end;
      lineHasToken = true;
      previousText = text;
      continue;
    }

    const operator = multiOperators.find((value) => source.startsWith(value, index));
    const text = operator ?? character;
    if (!operator && !"()[]{}.,:+-*/%~!?&|^<>=@$;".includes(character)) {
      diagnostics.push(diagnostic("invalid-token", line));
      index += 1;
      continue;
    }
    const depth = delimiters.length;
    if (new Set([",", ")", "]", "}"]).has(text)) {
      let closedSuite = false;
      while (inlineSuites.length > 0 && inlineSuites.at(-1)! >= depth) {
        inlineSuites.pop();
        tokens.push(token("SUITE_END", line, "<suite-end>"));
        closedSuite = true;
      }
      // A comma between the names of a `for` binding keeps the headers open.
      const innermost = pendingHeaders.findLast((entry) => entry.depth === depth);
      const inForBinding = text === "," && !closedSuite && innermost?.text === "for";
      if (!inForBinding) pendingHeaders = pendingHeaders.filter((entry) => entry.depth < depth);
    }
    tokens.push(token(text, line, text));
    index += text.length;
    lineHasToken = true;
    previousText = text;
    if (openToClose.has(text)) delimiters.push({ depth: line, text });
    else if (closeToOpen.has(text)) {
      if (delimiters.at(-1)?.text !== closeToOpen.get(text))
        diagnostics.push(diagnostic("unmatched-delimiter", line));
      else delimiters.pop();
    }
    if (text === "=>") pendingHeaders = pendingHeaders.filter((entry) => entry.depth !== depth);
    else if (text === ":") {
      let match = -1;
      for (let position = pendingHeaders.length - 1; position >= 0; position -= 1) {
        if (pendingHeaders[position]!.depth === depth) {
          match = position;
          break;
        }
      }
      let look = index;
      while (look < source.length && " \t\r".includes(source[look]!)) look += 1;
      if (match >= 0) {
        if (look < source.length && !"\n#".includes(source[look]!)) inlineSuites.push(depth);
        else if (delimiters.length > 0) {
          const lineStart = source.lastIndexOf("\n", index - 1) + 1;
          const headerIndent = /^ */.exec(source.slice(lineStart))![0].length;
          pendingForcedSuite = { delimiters: depth, indent: headerIndent };
        }
        pendingHeaders.splice(match);
      }
    }
  }

  if (inlineSuites.length > 0) {
    for (const unused of inlineSuites) {
      void unused;
      tokens.push(token("SUITE_END", line, "<suite-end>"));
    }
  } else if (lineHasToken) tokens.push(token(new Set(["NEWLINE", "SUITE_END"]), line, "<newline>"));
  diagnostics.push(...delimiters.map((entry) => diagnostic("unclosed-delimiter", entry.depth)));
  while (forcedIndents.pop()) tokens.push(token("DEDENT", line + 1, "<dedent>"));
  while (indents.length > 1) {
    indents.pop();
    tokens.push(token("DEDENT", line + 1, "<dedent>"));
  }
  tokens.push(token("EOF", line + 1, "<eof>"));
  return { diagnostics, tokens };
}
