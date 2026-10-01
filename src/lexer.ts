import type { Diagnostic, SourcePosition, SourceSpan } from "./diagnostics.ts";

export type TokenKind =
  | "identifier"
  | "integer"
  | "float"
  | "string"
  | "character"
  | "doc-comment"
  | "keyword"
  | "symbol"
  | "newline"
  | "indent"
  | "dedent"
  | "eof";

export type InterpolatedStringSegment =
  | { readonly kind: "text"; readonly value: string; readonly span: SourceSpan }
  | { readonly kind: "expression"; readonly source: string; readonly span: SourceSpan };

export interface InterpolatedStringValue {
  readonly kind: "interpolated-string";
  readonly segments: readonly InterpolatedStringSegment[];
}

export interface Token {
  readonly kind: TokenKind;
  readonly text: string;
  readonly value?: string | number | bigint | InterpolatedStringValue;
  readonly span: SourceSpan;
  // A backtick raw identifier: never a keyword or contextual word
  // (01-lexical-structure.md#raw-identifiers).
  readonly raw?: boolean;
  // The `.` or `|>` that starts a leading-dot or leading-pipe continuation line.
  readonly continuation?: boolean;
  // A numeric literal's suffix (01-lexical-structure.md#literal-suffixes),
  // as in `250ms`; `text` still holds the whole token.
  readonly suffix?: { readonly name: string; readonly span: SourceSpan };
  // A prefixed string's prefix (01-lexical-structure.md#prefixed-strings), as
  // in `sql"..."`; the value is always an `InterpolatedStringValue` of raw text.
  readonly prefix?: { readonly name: string; readonly span: SourceSpan };
}

export interface LexResult {
  readonly tokens: readonly Token[];
  readonly diagnostics: readonly Diagnostic[];
}

interface Delimiter {
  readonly text: string;
  readonly span: SourceSpan;
}

interface InterpolationScanResult {
  readonly source: string;
  readonly text: string;
  readonly end: SourcePosition;
  readonly terminated: boolean;
}

export const KEYWORDS = new Set([
  "Self",
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

const MULTI_SYMBOLS = [
  "...=",
  // Compound assignment (spec/01-lexical-structure.md#r-lex.op.compound-assign);
  // a member line of a derivation block also uses `+=`.
  "<<=",
  ">>=",
  "+=",
  "-=",
  "*=",
  "/=",
  "%=",
  "&=",
  "|=",
  "^=",
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
  "|>",
];
const SINGLE_SYMBOLS = new Set("+-*/%<>&|^~!?=.,:;()[]{}$@");
const OPEN_TO_CLOSE: Readonly<Record<string, string>> = { "(": ")", "[": "]", "{": "}" };
const CLOSE_TO_OPEN: Readonly<Record<string, string>> = { ")": "(", "]": "[", "}": "{" };

const isIdentifierStart = (value: string): boolean => /^(?:_|\p{ID_Start})$/u.test(value);
const isIdentifierContinue = (value: string): boolean => /^(?:_|\p{ID_Continue})$/u.test(value);
const isSuffixStart = (value: string): boolean => /^\p{XID_Start}$/u.test(value);
const isDigit = (value: string): boolean => value >= "0" && value <= "9";

class Scanner {
  private readonly source: string;
  private readonly tokens: Token[] = [];
  private readonly diagnostics: Diagnostic[] = [];
  private readonly indents = [0];
  private readonly delimiters: Delimiter[] = [];
  private offset = 0;
  private line = 1;
  private column = 1;
  private lineStart = true;
  private lineHasToken = false;
  private continuationDot = false;
  // The delimiter depths at which the current logical line has a `|>`.
  private readonly pipeDepths = new Set<number>();

  constructor(source: string) {
    this.source = source;
  }

  scan(): LexResult {
    if (this.source.startsWith("\uFEFF")) this.advance();
    while (!this.done()) {
      if (this.lineStart && this.delimiters.length === 0 && this.scanIndentation()) continue;
      const value = this.peek();
      if (value === " " || value === "\t") {
        if (value === "\t")
          this.report(
            "tab-whitespace",
            "tab characters are not permitted as whitespace",
            this.position(),
          );
        this.advance();
      } else if (value === "#") {
        if (this.peek(1) === "#" && !this.lineHasToken) this.scanDocComment();
        else this.skipComment();
      } else if (value === "\n") {
        this.scanNewline();
      } else if (value === "\r") {
        const start = this.position();
        if (this.peek(1) === "\n") {
          this.advance();
          this.scanNewline();
        } else {
          this.advance();
          this.report("bare-carriage-return", "a bare carriage return is not a line ending", start);
        }
      } else if (value === "\uFEFF") {
        const start = this.position();
        this.advance();
        this.report(
          "unexpected-bom",
          "a byte-order mark is only allowed at the start of a file",
          start,
        );
      } else if (value === '"' || value === "'") {
        this.scanQuoted();
      } else if (isDigit(value)) {
        this.scanNumber();
      } else if (isIdentifierStart(value)) {
        this.scanIdentifier();
      } else if (value === "`") {
        this.scanRawIdentifier();
      } else {
        this.scanSymbol();
      }
    }

    if (
      this.lineHasToken &&
      this.tokens.at(-1)?.kind !== "newline" &&
      this.delimiters.length === 0
    ) {
      this.emit("newline", "", this.position(), this.position());
    }
    for (const delimiter of this.delimiters) {
      this.diagnostics.push({
        code: "unclosed-delimiter",
        message: `unclosed '${delimiter.text}' delimiter`,
        span: delimiter.span,
      });
    }
    while (this.indents.length > 1) {
      this.indents.pop();
      this.emit("dedent", "", this.position(), this.position());
    }
    this.emit("eof", "", this.position(), this.position());
    return { tokens: this.tokens, diagnostics: this.diagnostics };
  }

  private scanIndentation(): boolean {
    const start = this.position();
    let width = 0;
    while (this.peek() === " ") {
      width += 1;
      this.advance();
    }
    if (this.peek() === "\t") {
      const tab = this.position();
      this.advance();
      this.report("tab-whitespace", "indentation must contain spaces only", tab);
      while (this.peek() === " " || this.peek() === "\t") this.advance();
    }
    if (
      (this.peek() === "#" && this.peek(1) !== "#") ||
      this.peek() === "\n" ||
      this.peek() === "\r" ||
      this.done()
    ) {
      return false;
    }

    const current = this.indents.at(-1)!;
    if (width > current) {
      this.indents.push(width);
      this.emit("indent", "", start, this.position());
    } else if (width < current) {
      while (width < this.indents.at(-1)!) {
        this.indents.pop();
        this.emit("dedent", "", start, this.position());
      }
      if (width !== this.indents.at(-1)) {
        this.report(
          "invalid-dedent",
          `column ${width + 1} is not an active indentation level`,
          start,
        );
      }
    }
    this.lineStart = false;
    return false;
  }

  private scanNewline(): void {
    const start = this.position();
    this.advanceNewline();
    if (this.delimiters.length === 0 && this.lineHasToken && this.continuesWithLeadingDot()) return;
    if (this.delimiters.length === 0 && this.lineHasToken) {
      this.emit("newline", "\n", start, this.position());
      this.pipeDepths.clear();
    }
    this.lineStart = true;
    this.lineHasToken = false;
  }

  // Leading-dot continuation (01-lexical-structure.md#physical-and-logical-lines):
  // a line starting with `.name`, indented farther than the logical line it
  // follows, continues it unless that line ends in `:` or `=>`. A line
  // starting with `|>` continues it the same way
  // (01-lexical-structure.md#leading-pipe-continuation). On success the
  // scanner is left at the `.` or `|>` with the logical line still open.
  private continuesWithLeadingDot(): boolean {
    const last = this.tokens.at(-1)?.text;
    if (last === ":" || last === "=>") return false;
    let offset = this.offset;
    let lineOffset = offset;
    for (;;) {
      let width = 0;
      while (this.source[offset] === " ") {
        width += 1;
        offset += 1;
      }
      const value = this.source[offset];
      if (value === "#" || value === "\n" || value === "\r") {
        while (offset < this.source.length && this.source[offset] !== "\n") offset += 1;
        if (offset >= this.source.length) return false;
        offset += 1;
        lineOffset = offset;
        continue;
      }
      const next = this.source[offset + 1] ?? "";
      const leadingDot = value === "." && isIdentifierStart(next);
      const leadingPipe = value === "|" && next === ">";
      if ((!leadingDot && !leadingPipe) || width <= this.indents.at(-1)!) return false;
      break;
    }
    while (this.offset < lineOffset) this.advance();
    while (this.peek() === " ") this.advance();
    this.lineStart = false;
    this.continuationDot = true;
    // A leading-dot line cannot continue a line with a `|>` at its depth
    // (01-lexical-structure.md#r-lex.pipe.no-dot-line).
    if (this.peek() === "." && this.pipeDepths.has(this.delimiters.length))
      this.report(
        "syntax-error",
        "a leading-dot line cannot continue a pipe chain; write the call as a step, such as '|> _.name()'",
        this.position(),
      );
    return true;
  }

  private skipComment(): void {
    while (!this.done() && this.peek() !== "\n" && this.peek() !== "\r") this.advance();
  }

  private scanDocComment(): void {
    const start = this.position();
    this.advance();
    this.advance();
    if (this.peek() === " ") this.advance();
    let text = "";
    while (!this.done() && this.peek() !== "\n" && this.peek() !== "\r") text += this.advance();
    this.emit("doc-comment", text, start, this.position(), text);
  }

  private scanIdentifier(): void {
    const start = this.position();
    let text = this.advance();
    while (!this.done() && isIdentifierContinue(this.peek())) text += this.advance();
    if (text.normalize("NFC") !== text) {
      this.report("identifier-not-nfc", `identifier '${text}' is not NFC-normalized`, start);
    }
    const kind: TokenKind = text === "_" ? "symbol" : KEYWORDS.has(text) ? "keyword" : "identifier";
    // An identifier directly before `"` is a string prefix
    // (01-lexical-structure.md#prefixed-strings). A reserved word there, as
    // in `return"done"`, is an error (lex.literal-fn.reserved-glued).
    if (kind === "keyword" && this.peek() === '"')
      this.report(
        "syntax-error",
        `'${text}' is a reserved word, so it cannot prefix a string: write '${text} "...'`,
        start,
      );
    if (kind === "identifier" && this.peek() === '"') {
      this.scanQuoted({ name: text, span: { start, end: this.position() } });
      return;
    }
    this.emit(kind, text, start, this.position(), text);
  }

  // A raw identifier (01-lexical-structure.md#raw-identifiers): an identifier
  // or reserved word between backticks is one identifier token.
  private scanRawIdentifier(): void {
    const start = this.position();
    let offset = this.offset + 1;
    let text = "";
    while (offset < this.source.length && isIdentifierContinue(this.source[offset]!)) {
      text += this.source[offset];
      offset += 1;
    }
    const valid =
      text !== "" &&
      text !== "_" &&
      isIdentifierStart(text[0]!) &&
      this.source[offset] === "`" &&
      text.normalize("NFC") === text;
    if (!valid) {
      this.advance();
      this.report("invalid-token", "a backtick must enclose an identifier or reserved word", start);
      return;
    }
    while (this.offset <= offset) this.advance();
    this.tokens.push({
      kind: "identifier",
      text,
      value: text,
      span: { start, end: this.position() },
      raw: true,
    });
    this.lineHasToken = true;
    this.lineStart = false;
  }

  private scanNumber(): void {
    const start = this.position();
    let text = "";
    const radixPrefix = this.peek() === "0" && /[bBoOxX]/.test(this.peek(1));
    if (radixPrefix) {
      text += this.advance() + this.advance();
      while (/[0-9a-fA-F_]/.test(this.peek())) text += this.advance();
      const clean = text.replaceAll("_", "");
      const digits = text.slice(2);
      const radix = text[1]!.toLowerCase();
      const validDigits =
        radix === "b"
          ? /^[01](?:_?[01])*$/
          : radix === "o"
            ? /^[0-7](?:_?[0-7])*$/
            : /^[0-9a-fA-F](?:_?[0-9a-fA-F])*$/;
      const validPrefixed = digits.startsWith("_")
        ? validDigits.test(digits.slice(1))
        : validDigits.test(digits);
      if (!validPrefixed || /[\p{ID_Continue}]/u.test(this.peek())) {
        while (/[\p{ID_Continue}]/u.test(this.peek())) text += this.advance();
        // Misplaced separators among valid digits form no token; any other
        // character splits the text into tokens the grammar rejects.
        const radixDigit = radix === "b" ? /[01_]/ : radix === "o" ? /[0-7_]/ : /[0-9a-fA-F_]/;
        const scanned = text.slice(2);
        const separatorsOnly =
          scanned.length > 0 && [...scanned].every((digit) => radixDigit.test(digit));
        this.report(
          separatorsOnly ? "invalid-token" : "syntax-error",
          `invalid integer literal '${text}'`,
          start,
        );
        return;
      }
      let value: bigint;
      try {
        value = BigInt(clean);
      } catch {
        this.report("syntax-error", `invalid integer literal '${text}'`, start);
        return;
      }
      // A radix literal takes no suffix (01-lexical-structure.md#r-lex.suffix.decimal),
      // so a following `'` begins a character literal.
      this.emitNumber("integer", text, start, value, undefined);
      return;
    }
    while (isDigit(this.peek()) || this.peek() === "_") text += this.advance();
    let floating = false;
    if (this.peek() === "." && isDigit(this.peek(1))) {
      floating = true;
      text += this.advance();
      while (isDigit(this.peek()) || this.peek() === "_") text += this.advance();
    }
    // An `e` starts an exponent only when digits follow it, after an optional
    // sign; otherwise it starts a suffix, as in `5em`. Separators stay in the
    // exponent so that `1e_5` is an invalid token.
    const sign = this.peek(1) === "+" || this.peek(1) === "-" ? 1 : 0;
    let exponentDigit = false;
    for (let distance = 1 + sign; ; distance += 1) {
      const next = this.peek(distance);
      if (isDigit(next)) exponentDigit = true;
      if (!isDigit(next) && next !== "_") break;
    }
    if (/[eE]/.test(this.peek()) && exponentDigit) {
      floating = true;
      text += this.advance();
      if (this.peek() === "+" || this.peek() === "-") text += this.advance();
      while (isDigit(this.peek()) || this.peek() === "_") text += this.advance();
    }
    if (
      text.startsWith("_") ||
      text.endsWith("_") ||
      text.includes("__") ||
      /_\.|\._|_[eE]|[eE]_|[+-]_/.test(text)
    ) {
      if (isSuffixStart(this.peek())) this.scanSuffix();
      this.report("invalid-token", `invalid numeric literal '${text}'`, start);
      return;
    }
    const clean = text.replaceAll("_", "");
    const suffix = isSuffixStart(this.peek()) ? this.scanSuffix() : undefined;
    // A reserved word glued to the digits, as in `5else`, is an error
    // (01-lexical-structure.md#r-lex.literal-fn.reserved-glued).
    if (suffix && KEYWORDS.has(suffix.name)) {
      this.report(
        "syntax-error",
        `'${suffix.name}' is a reserved word, so '${text}${suffix.name}' needs a space: write '${text} ${suffix.name}'`,
        start,
      );
      return;
    }
    this.emitNumber(
      floating ? "float" : "integer",
      text + (suffix?.text ?? ""),
      start,
      floating ? Number(clean) : BigInt(clean),
      suffix,
    );
  }

  // A literal suffix: identifier characters directly after decimal or float
  // digits (01-lexical-structure.md#literal-suffixes).
  private scanSuffix(): { text: string; name: string; span: SourceSpan } {
    const start = this.position();
    let name = "";
    while (!this.done() && isIdentifierContinue(this.peek())) name += this.advance();
    return { text: name, name, span: { start, end: this.position() } };
  }

  private emitNumber(
    kind: TokenKind,
    text: string,
    start: SourcePosition,
    value: number | bigint,
    suffix: { name: string; span: SourceSpan } | undefined,
  ): void {
    this.emit(kind, text, start, this.position(), value);
    if (suffix) {
      const token = this.tokens.pop()!;
      this.tokens.push({ ...token, suffix: { name: suffix.name, span: suffix.span } });
    }
  }

  // A quoted literal. A prefixed string (`prefix` set) keeps its text raw and
  // still interpolates; a `$` that begins no interpolation is text
  // (01-lexical-structure.md#prefixed-strings).
  private scanQuoted(prefix?: { name: string; span: SourceSpan }): void {
    const start = prefix ? prefix.span.start : this.position();
    const raw = prefix !== undefined;
    const quote = this.advance();
    const triple = quote === '"' && this.peek() === '"' && this.peek(1) === '"';
    if (triple) {
      this.advance();
      this.advance();
    }
    const terminator = triple ? quote.repeat(3) : quote;
    let text = (prefix?.name ?? "") + terminator;
    let value = "";
    const segments: InterpolatedStringSegment[] = [];
    let segmentStart = this.position();
    let terminated = false;
    while (!this.done()) {
      if (this.source.startsWith(terminator, this.offset)) {
        for (let index = 0; index < terminator.length; index += 1) text += this.advance();
        terminated = true;
        break;
      }
      if (!triple && (this.peek() === "\n" || this.peek() === "\r")) break;
      // A tab in a literal is written `\t` (01-lexical-structure.md#r-lex.tab.content).
      if (this.peek() === "\t")
        this.report(
          "tab-whitespace",
          "a tab inside a literal must be written as the '\\t' escape",
          this.position(),
        );
      const current = this.advance();
      text += current;
      if (current === "\n") {
        value += "\n";
        continue;
      }
      if (current === "\\" && raw) {
        // A raw backslash keeps the next character from ending the literal and
        // both stay in the value (01-lexical-structure.md#string-and-character-literals).
        value += current;
        const next = this.peek();
        if (!this.done() && (triple || (next !== "\n" && next !== "\r"))) {
          text += this.advance();
          value += next;
        }
      } else if (current === "\\" && !raw) {
        const escaped = this.advance();
        text += escaped;
        const escapes: Readonly<Record<string, string>> = {
          "0": "\0",
          n: "\n",
          r: "\r",
          t: "\t",
          "\\": "\\",
          '"': '"',
          "'": "'",
          $: "$",
        };
        if (escaped === "u" && this.peek() === "{") {
          text += this.advance();
          let digits = "";
          while (/[0-9a-fA-F]/.test(this.peek()) && digits.length < 7) {
            const digit = this.advance();
            digits += digit;
            text += digit;
          }
          if (this.peek() === "}") text += this.advance();
          const codePoint =
            digits.length >= 1 && digits.length <= 6 ? Number.parseInt(digits, 16) : -1;
          if (
            codePoint < 0 ||
            codePoint > 0x10ffff ||
            (codePoint >= 0xd800 && codePoint <= 0xdfff) ||
            !text.endsWith("}")
          ) {
            this.report("invalid-escape", "invalid Unicode escape sequence", start);
          } else {
            value += String.fromCodePoint(codePoint);
          }
        } else if (!(escaped in escapes)) {
          this.report("invalid-escape", `invalid escape sequence '\\${escaped}'`, start);
        } else {
          value += escapes[escaped];
        }
      } else if (
        current === "$" &&
        quote === '"' &&
        (!raw || isIdentifierStart(this.peek()) || this.peek() === "{")
      ) {
        if (value.length > 0)
          segments.push({
            kind: "text",
            value,
            span: {
              start: segmentStart,
              end: { ...this.position(), offset: this.offset - 1, column: this.column - 1 },
            },
          });
        value = "";
        if (isIdentifierStart(this.peek())) {
          const expressionStart = this.position();
          let source = this.advance();
          text += source;
          while (!this.done() && isIdentifierContinue(this.peek())) {
            const next = this.advance();
            source += next;
            text += next;
          }
          // `$name` takes an identifier or `self`; any other reserved word
          // leaves a bare `$` (01-lexical-structure.md#string-and-character-literals).
          if (KEYWORDS.has(source) && source !== "self")
            this.report(
              "syntax-error",
              "an unescaped '$' must be followed by an identifier, 'self', or '{'",
              start,
            );
          segments.push({
            kind: "expression",
            source,
            span: { start: expressionStart, end: this.position() },
          });
          segmentStart = this.position();
        } else if (this.peek() === "{") {
          text += this.advance();
          const expressionStart = this.position();
          const source = this.scanInterpolationExpression();
          text += source.text;
          if (source.terminated) {
            segments.push({
              kind: "expression",
              source: source.source,
              span: { start: expressionStart, end: source.end },
            });
            segmentStart = this.position();
          } else {
            this.report(
              "unterminated-string-interpolation",
              "unterminated '${...}' interpolation",
              expressionStart,
            );
            break;
          }
        } else {
          this.report(
            "syntax-error",
            "an unescaped '$' must be followed by an identifier or '{'",
            start,
          );
          segmentStart = this.position();
        }
      } else {
        value += current;
      }
    }
    if (!terminated) this.report("unterminated-string", "unterminated literal", start);
    // An unterminated character literal, as in `0xff'B`, reports only that.
    if (terminated && quote === "'" && [...value].length !== 1) {
      this.report(
        "invalid-character-literal",
        "a character literal must contain one Unicode scalar value",
        start,
      );
    }
    if (segments.length > 0 || prefix) {
      if (value.length > 0)
        segments.push({ kind: "text", value, span: { start: segmentStart, end: this.position() } });
      this.emit("string", text, start, this.position(), { kind: "interpolated-string", segments });
      if (prefix) {
        const token = this.tokens.pop()!;
        this.tokens.push({ ...token, prefix });
      }
    } else {
      this.emit(quote === "'" ? "character" : "string", text, start, this.position(), value);
    }
  }

  private scanInterpolationExpression(): InterpolationScanResult {
    const stack = ["}"];
    let source = "";
    let text = "";
    while (!this.done()) {
      const current = this.peek();
      if (current === '"' || current === "'") {
        const quoted = this.scanEmbeddedQuotedSource();
        source += quoted;
        text += quoted;
        continue;
      }
      if (current === "#") {
        while (!this.done() && this.peek() !== "\n" && this.peek() !== "\r") {
          const next = this.advance();
          source += next;
          text += next;
        }
        continue;
      }
      const close = OPEN_TO_CLOSE[current];
      if (close) {
        stack.push(close);
      } else if (current === stack.at(-1)) {
        if (stack.length === 1) {
          const end = this.position();
          text += this.advance();
          return { source, text, end, terminated: true };
        }
        stack.pop();
      }
      const next = this.advance();
      source += next;
      text += next;
    }
    return { source, text, end: this.position(), terminated: false };
  }

  private scanEmbeddedQuotedSource(): string {
    // A prefix before the quote was already copied as ordinary source.
    let text = "";
    const quote = this.advance();
    text += quote;
    const triple = quote === '"' && this.peek() === '"' && this.peek(1) === '"';
    const terminator = triple ? quote.repeat(3) : quote;
    if (triple) text += this.advance() + this.advance();
    while (!this.done()) {
      if (this.source.startsWith(terminator, this.offset)) {
        for (let index = 0; index < terminator.length; index += 1) text += this.advance();
        break;
      }
      const current = this.advance();
      text += current;
      if (current === "\\" && !this.done()) text += this.advance();
      if (!triple && (current === "\n" || current === "\r")) break;
    }
    return text;
  }

  private scanSymbol(): void {
    const start = this.position();
    const symbol =
      MULTI_SYMBOLS.find((candidate) => this.source.startsWith(candidate, this.offset)) ??
      this.peek();
    if (!SINGLE_SYMBOLS.has(symbol[0]!) && !MULTI_SYMBOLS.includes(symbol)) {
      this.advance();
      this.report("unexpected-character", `unexpected character '${symbol}'`, start);
      return;
    }
    for (let index = 0; index < symbol.length; index += 1) this.advance();
    if (symbol === ";")
      this.report(
        "reserved-semicolon",
        "semicolon is reserved and cannot separate statements",
        start,
      );
    if (symbol in OPEN_TO_CLOSE) {
      this.delimiters.push({ text: symbol, span: { start, end: this.position() } });
    } else if (symbol in CLOSE_TO_OPEN) {
      const expectedOpen = CLOSE_TO_OPEN[symbol];
      const open = this.delimiters.at(-1);
      if (!open || open.text !== expectedOpen) {
        this.report("mismatched-delimiter", `unexpected '${symbol}' delimiter`, start);
      } else {
        this.delimiters.pop();
      }
    }
    this.emit("symbol", symbol, start, this.position(), symbol);
  }

  private emit(
    kind: TokenKind,
    text: string,
    start: SourcePosition,
    end: SourcePosition,
    value?: Token["value"],
  ): void {
    const continuation =
      this.continuationDot && kind === "symbol" && (text === "." || text === "|>");
    if (kind === "symbol" && text === "|>") this.pipeDepths.add(this.delimiters.length);
    this.tokens.push({
      kind,
      text,
      value,
      span: { start, end },
      ...(continuation ? { continuation } : {}),
    });
    if (!(["newline", "indent", "dedent", "eof"] as TokenKind[]).includes(kind)) {
      this.continuationDot = false;
      this.lineHasToken = true;
      this.lineStart = false;
    }
  }

  private report(code: string, message: string, start: SourcePosition): void {
    this.diagnostics.push({ code, message, span: { start, end: this.position() } });
  }

  private position(): SourcePosition {
    return { offset: this.offset, line: this.line, column: this.column };
  }

  private peek(distance = 0): string {
    return this.source[this.offset + distance] ?? "";
  }

  private done(): boolean {
    return this.offset >= this.source.length;
  }

  private advance(): string {
    const value = this.peek();
    this.offset += value.length;
    if (value === "\n") {
      this.line += 1;
      this.column = 1;
    } else {
      this.column += 1;
    }
    return value;
  }

  private advanceNewline(): void {
    this.advance();
  }
}

export function lex(source: string): LexResult {
  return new Scanner(source).scan();
}
