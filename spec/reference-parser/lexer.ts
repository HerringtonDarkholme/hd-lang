import type { Diagnostic, GrammarToken, LexResult } from "./types.ts";

const reserved = new Set([
  "Self",
  "annotate",
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
  "pass",
  "pub",
  "return",
  "self",
  "tests",
  "trait",
  "true",
  "type",
  "while",
]);
export const openToClose = new Map([
  ["(", ")"],
  ["[", "]"],
  ["{", "}"],
]);
export const closeToOpen = new Map([...openToClose].map(([open, close]) => [close, open]));
const multiOperators = [
  "...=",
  "...",
  ":=",
  "->",
  "=>",
  "::",
  "==",
  "!=",
  "<=",
  ">=",
  "<<",
  ">>",
  "**",
  "&&",
  "||",
  "+=",
];
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
  /** Where the literal suffix begins. */
  readonly suffix?: number;
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
  // Set on a closure body: the indentation of the line holding its header.
  readonly closureReference?: number;
  // The indentation of the first physical line of the logical line holding
  // the header; the body must also be deeper than it.
  readonly logicalIndent?: number;
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

function interpolatedName(source: string, index: number): boolean {
  let end = index;
  while (end < source.length && (source[end] === "_" || isLetterOrNumber(source[end]!))) end += 1;
  const word = source.slice(index, end);
  return word === "self" || !reserved.has(word);
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
  // Raw literals are `r"..."` and `r"""..."""` only; there is no raw character literal.
  if (character !== "r" || source[index + 1] !== '"') return false;
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
      // `$name` takes an identifier or `self`; any other reserved word, like
      // a character that cannot start an identifier, leaves a bare `$`.
      if (!startsInterpolatedName(source, index + 1) || !interpolatedName(source, index + 1))
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

// Chapter 01 and 02 (owner decision TUP-1): digits after `.` are an ordinary
// number, so `t.0.1` lexes `0.1` as one floating-point token; tuple members
// are identifiers such as `_0`.
// Chapter 01 literal suffixes: a letter directly after decimal or float
// digits starts a suffix of identifier characters; radix literals take none.
function suffixEnd(source: string, start: number): number {
  let end = start;
  while (end < source.length && /^[\p{XID_Continue}_]$/u.test(source[end]!)) end += 1;
  return end;
}

function startsSuffix(character: string | undefined): boolean {
  return character !== undefined && /^\p{XID_Start}$/u.test(character);
}

function numberEnd(source: string, start: number): NumberScan {
  const scan = unsuffixedNumberEnd(source, start);
  const radix = /^0[xXbBoO]/.test(source.slice(start, start + 2));
  if (!radix && startsSuffix(source[scan.end]))
    return { ...scan, end: suffixEnd(source, scan.end), suffix: scan.end };
  return scan;
}

function numberKind(found: NumberScan): string {
  if (found.suffix !== undefined) return "suffixed_literal";
  return found.floating ? "float_literal" : "integer_literal";
}

function unsuffixedNumberEnd(source: string, start: number): NumberScan {
  const rest = source.slice(start);
  const based = /^(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+)/.exec(rest);
  if (based) return { end: start + based[0].length, floating: false };
  const integer = /^[0-9][0-9_]*/.exec(rest)!;
  let end = start + integer[0].length;
  let floating = false;
  if (source[end] === "." && isDigit(source[end + 1] ?? "")) {
    floating = true;
    end += 1;
    end += /^[0-9][0-9_]*/.exec(source.slice(end))![0].length;
  }
  if (/[eE]/.test(source[end] ?? "")) {
    // Separators after the marker or sign stay in the literal, so a misplaced
    // one, as in `1e_5`, is an invalid token rather than `1` then `e_5`.
    const exponent = /^[eE][+-]?(?=[0-9_]*[0-9])[0-9_]+/.exec(source.slice(end));
    if (exponent) {
      floating = true;
      end += exponent[0].length;
    }
  }
  return { end, floating };
}

// Chapter 01 leading-dot continuation: unless the line ends in `:` or `=>`,
// which open an indented block, when the next non-blank, non-comment line
// starts with `.` and an identifier and is indented farther than
// `lineIndent`, returns the index of that `.` and the lines skipped.
function leadingDotContinuation(
  source: string,
  newline: number,
  lineIndent: number,
  previousText: string,
): { readonly index: number; readonly lines: number } | undefined {
  if (previousText === ":" || previousText === "=>") return undefined;
  let index = newline + 1;
  let lines = 1;
  while (index < source.length) {
    let indent = 0;
    while (source[index] === " ") {
      indent += 1;
      index += 1;
    }
    if (source[index] === "\r") index += 1;
    if (source[index] === "\n" || source[index] === "#") {
      while (index < source.length && source[index] !== "\n") index += 1;
      index += 1;
      lines += 1;
      continue;
    }
    const next = source[index + 1] ?? "";
    const identifierStart =
      isLetter(next) || (next === "_" && startsInterpolatedName(source, index + 1));
    return source[index] === "." && identifierStart && indent > lineIndent
      ? { index, lines }
      : undefined;
  }
  return undefined;
}

// A nested suite level. Only a closure written directly inside the brackets
// records its header line; a closure statement in a nested suite body ends
// like any statement.
function nestedLevel(
  delimiters: number,
  indent: number,
  closure: boolean,
  logicalIndent: number,
): NestedLevel {
  return closure
    ? { closureReference: indent, delimiters, indent, logicalIndent }
    : { delimiters, indent, logicalIndent };
}

// A closure body nested in brackets ends only at a line no deeper than its
// header line that starts with `,` or a closing delimiter.
function badClosureEnd(level: NestedLevel, indent: number, next: string | undefined): boolean {
  const reference = level.closureReference;
  return reference !== undefined && (indent > reference || !",)]}".includes(next ?? ""));
}

// A token that can end an operand, so that a following `(`, `[`, `{`, or `!`
// on the same line would be a suffix.
function endsOperand(previous: GrammarToken | undefined): boolean {
  if (!previous) return false;
  const operandKinds = [
    "identifier",
    "integer_literal",
    "float_literal",
    "suffixed_literal",
    "string_literal",
    "char_literal",
    "boolean_literal",
  ];
  return (
    operandKinds.some((kind) => previous.kinds.has(kind)) ||
    [")", "]", "}", "?", "self", "Self", "pass"].includes(previous.text)
  );
}

// Words whose next suite-opening `:` layout must recognize.
const suiteWords = new Set([
  "defer",
  "fn",
  "if",
  "while",
  "match",
  "else",
  "data",
  "tests",
  "annotate",
  "with",
]);

// Tokens after which `for` starts a loop expression rather than a
// comprehension clause.
const loopExpressionFollows = new Set([
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
]);

function wordKinds(word: string, source: string, end: number): ReadonlySet<string> {
  if (word === "true" || word === "false") return new Set([word, "boolean_literal"]);
  if (reserved.has(word)) return new Set([word]);
  // `pack.map(` and `pack.map_list(` always form the pack operation, even when
  // a local named `pack` is in scope.
  if (word === "pack" && /^\s*\.\s*(?:map|map_list)\s*\(/u.test(source.slice(end, end + 64)))
    return new Set([word]);
  return new Set([word, "identifier"]);
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
  // Indentation of the first physical line of the current logical line.
  let lineIndent = 0;
  // The physical line on which the previous token ended.
  let lastTokenLine = 0;

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
          // The first body line must be deeper than the header's line and the
          // logical line that contains the header.
          const reference = Math.max(
            pendingForcedSuite.indent,
            pendingForcedSuite.logicalIndent ?? 0,
          );
          if (indent <= reference) diagnostics.push(diagnostic("unexpected-indentation", line));
          forcedIndents.push({ ...pendingForcedSuite, indent });
          tokens.push(token("INDENT", line, "<indent>"));
          pendingForcedSuite = undefined;
        } else {
          while (forcedIndents.length > 0 && indent < forcedIndents.at(-1)!.indent) {
            const closed = forcedIndents.pop()!;
            tokens.push(token("DEDENT", line, "<dedent>"));
            if (closed.saved) pendingHeaders.push(...closed.saved);
            if (badClosureEnd(closed, indent, source[index]))
              diagnostics.push(diagnostic("syntax-error", line));
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
      lineIndent = indent;
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
      const continuation =
        lineHasToken && !pendingForcedSuite
          ? leadingDotContinuation(source, index, lineIndent, previousText)
          : undefined;
      // A leading-dot line cannot continue a line whose same-line suite is
      // still open: the chain would silently join that suite's body.
      if (continuation && closing > 0)
        diagnostics.push(diagnostic("syntax-error", line + continuation.lines));
      else if (continuation) {
        line += continuation.lines;
        index = continuation.index;
        continue;
      }
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

    const rawString = character === "r" && source[index + 1] === '"';
    if (character === '"' || character === "'" || rawString) {
      const found = scanString(source, index, line);
      diagnostics.push(...found.diagnostics);
      const text = source.slice(index, found.end);
      const kind = found.quote === "'" && !rawString ? "char_literal" : "string_literal";
      tokens.push(token(kind, line, text));
      line = found.line;
      index = found.end;
      lineHasToken = true;
      lastTokenLine = line;
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
      lastTokenLine = line;
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
      tokens.push(token(wordKinds(word, source, end), line, word));
      let suiteWord = suiteWords.has(word);
      // A function type in a closure's result position never takes a `:`.
      if (word === "fn" && typeResultStart(tokens)) suiteWord = false;
      // A `for` loop expression follows an operator, an opening delimiter, or
      // a header word. A comprehension clause `for` follows a complete
      // expression; a leading one after `[` or `{` loses its header at `=>`.
      if (word === "for") suiteWord = loopExpressionFollows.has(previousText);
      // A closure `fn` is followed by `(` or `!`; a declaration `fn` by a name.
      const closure = word === "fn" && /^[ \t]*[(!]/.test(source.slice(end, end + 40));
      if (suiteWord) pendingHeaders.push({ depth, text: closure ? "closure" : word });
      if (word === "in") {
        const header = pendingHeaders.findLastIndex(
          (entry) => entry.depth === depth && entry.text === "for",
        );
        if (header >= 0) pendingHeaders[header] = { depth, text: "for-in" };
      }
      index = end;
      lineHasToken = true;
      lastTokenLine = line;
      previousText = word;
      continue;
    }
    if (isDigit(character)) {
      const found = numberEnd(source, index);
      const text = source.slice(index, found.end);
      // A reserved word as a suffix, as in `5else`, forms no token
      // (chapter 01 `lex.suffix.reserved`).
      const suffix = found.suffix === undefined ? "" : source.slice(found.suffix, found.end);
      if (!validNumber(source.slice(index, found.suffix ?? found.end)) || reserved.has(suffix))
        diagnostics.push(diagnostic("invalid-token", line));
      tokens.push(token(numberKind(found), line, text));
      index = found.end;
      lineHasToken = true;
      lastTokenLine = line;
      previousText = text;
      continue;
    }

    // A raw identifier: any identifier or reserved word between backticks is
    // one identifier token that never acts as a keyword or contextual word.
    if (character === "`") {
      const raw = /^`([\p{L}_][\p{L}\p{N}_]*)`/u.exec(source.slice(index, index + 256));
      const word = raw?.[1];
      if (!raw || word === "_") {
        diagnostics.push(diagnostic("invalid-token", line));
        index += 1;
        continue;
      }
      tokens.push(token("identifier", line, word!));
      index += raw[0].length;
      lineHasToken = true;
      lastTokenLine = line;
      previousText = raw[0];
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
      // A closing delimiter at a nested suite's delimiter depth ends that
      // suite's last body line: NEWLINE, then every pending DEDENT.
      if (text !== "," && forcedIndents.at(-1)?.delimiters === depth) {
        if (!closedSuite && lineHasToken)
          tokens.push(token(new Set(["NEWLINE", "SUITE_END"]), line, "<newline>"));
        while (forcedIndents.at(-1)?.delimiters === depth) {
          const closed = forcedIndents.pop()!;
          tokens.push(token("DEDENT", line, "<dedent>"));
          if (closed.saved) pendingHeaders.push(...closed.saved);
          // A closure body ends at a line starting with `,` or a closing
          // delimiter, never at a closing delimiter on a body line.
          if (closed.closureReference !== undefined)
            diagnostics.push(diagnostic("syntax-error", line));
        }
      }
      // A comma between the names of a `for` binding keeps the headers open.
      const innermost = pendingHeaders.findLast((entry) => entry.depth === depth);
      const inForBinding = text === "," && !closedSuite && innermost?.text === "for";
      if (!inForBinding) pendingHeaders = pendingHeaders.filter((entry) => entry.depth < depth);
    }
    // A call, index, data-literal, or suspension suffix starts on its operand's
    // line: inside delimiters, a line starting with one begins a new operand.
    const layoutDepth = forcedIndents.at(-1)?.delimiters ?? 0;
    if (
      ["(", "[", "{", "!"].includes(text) &&
      lastTokenLine < line &&
      delimiters.length > layoutDepth &&
      endsOperand(tokens.at(-1))
    )
      diagnostics.push(diagnostic("syntax-error", line));
    tokens.push(token(text, line, text));
    index += text.length;
    lineHasToken = true;
    lastTokenLine = line;
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
          const closure = pendingHeaders[match]!.text === "closure";
          const direct = depth > (forcedIndents.at(-1)?.delimiters ?? 0);
          pendingForcedSuite = nestedLevel(depth, headerIndent, closure && direct, lineIndent);
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
