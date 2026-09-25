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
}

export interface LexResult {
  readonly tokens: readonly Token[];
  readonly diagnostics: readonly Diagnostic[];
}

const KEYWORDS = new Set([
  "Self", "and", "annotate", "as", "break", "continue", "data", "defer",
  "else", "enum", "false", "fn", "for", "if", "impl", "in", "is", "let",
  "match", "mut", "nil", "not", "or", "pass", "pub", "reified", "return",
  "self", "shape", "super", "trait", "true", "type", "use", "where", "while",
]);

const MULTI_SYMBOLS = ["...", ":=", "->", "=>", "::", "==", "!=", "<=", ">=", "<<", ">>", "**"];
const SINGLE_SYMBOLS = new Set("+-*/%<>&|^~!?=.,:;()[]{}$@");
const OPEN_TO_CLOSE: Readonly<Record<string, string>> = { "(": ")", "[": "]", "{": "}" };
const CLOSE_TO_OPEN: Readonly<Record<string, string>> = { ")": "(", "]": "[", "}": "{" };

const isIdentifierStart = (value: string): boolean => /^(?:_|\p{ID_Start})$/u.test(value);
const isIdentifierContinue = (value: string): boolean => /^(?:_|\p{ID_Continue})$/u.test(value);
const isDigit = (value: string): boolean => value >= "0" && value <= "9";

class Scanner {
  private readonly source: string;
  private readonly tokens: Token[] = [];
  private readonly diagnostics: Diagnostic[] = [];
  private readonly indents = [0];
  private readonly delimiters: Array<{ text: string; span: SourceSpan }> = [];
  private offset = 0;
  private line = 1;
  private column = 1;
  private lineStart = true;
  private lineHasToken = false;

  constructor(source: string) {
    this.source = source;
  }

  scan(): LexResult {
    if (this.source.startsWith("\uFEFF")) this.advance();
    while (!this.done()) {
      if (this.lineStart && this.delimiters.length === 0 && this.scanIndentation()) continue;
      const value = this.peek();
      if (value === " " || value === "\t") {
      if (value === "\t") this.report("tab-whitespace", "tab characters are not permitted as whitespace", this.position());
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
        this.report("unexpected-bom", "a byte-order mark is only allowed at the start of a file", start);
      } else if (value === '"' || value === "'" || (value === "r" && this.peek(1) === '"')) {
        this.scanQuoted();
      } else if (isDigit(value)) {
        this.scanNumber();
      } else if (isIdentifierStart(value)) {
        this.scanIdentifier();
      } else {
        this.scanSymbol();
      }
    }

    if (this.lineHasToken && this.tokens.at(-1)?.kind !== "newline" && this.delimiters.length === 0) {
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
    if ((this.peek() === "#" && this.peek(1) !== "#") || this.peek() === "\n" || this.peek() === "\r" || this.done()) {
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
        this.report("inconsistent-dedent", `column ${width + 1} is not an active indentation level`, start);
      }
    }
    this.lineStart = false;
    return false;
  }

  private scanNewline(): void {
    const start = this.position();
    this.advanceNewline();
    if (this.delimiters.length === 0 && this.lineHasToken) {
      this.emit("newline", "\n", start, this.position());
    }
    this.lineStart = true;
    this.lineHasToken = false;
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
    this.emit(kind, text, start, this.position(), text);
  }

  private scanNumber(): void {
    const start = this.position();
    let text = "";
    const previous = this.tokens.at(-1);
    const numericSelector = previous?.text === "." && previous.span.end.offset === start.offset;
    if (numericSelector) {
      while (isDigit(this.peek())) text += this.advance();
      this.emit("integer", text, start, this.position(), BigInt(text));
      return;
    }
    const radixPrefix = this.peek() === "0" && /[bBoOxX]/.test(this.peek(1));
    if (radixPrefix) {
      text += this.advance() + this.advance();
      while (/[0-9a-fA-F_]/.test(this.peek())) text += this.advance();
      const clean = text.replaceAll("_", "");
      const digits = text.slice(2);
      const radix = text[1]!.toLowerCase();
      const validDigits = radix === "b" ? /^[01](?:_?[01])*$/ : radix === "o" ? /^[0-7](?:_?[0-7])*$/ : /^[0-9a-fA-F](?:_?[0-9a-fA-F])*$/;
      const validPrefixed = digits.startsWith("_")
        ? validDigits.test(digits.slice(1))
        : validDigits.test(digits);
      if (!validPrefixed || /[\p{ID_Continue}]/u.test(this.peek())) {
        while (/[\p{ID_Continue}]/u.test(this.peek())) text += this.advance();
        this.report("invalid-integer-literal", `invalid integer literal '${text}'`, start);
        return;
      }
      try {
        this.emit("integer", text, start, this.position(), BigInt(clean));
      } catch {
        this.report("invalid-integer-literal", `invalid integer literal '${text}'`, start);
      }
      return;
    }
    while (isDigit(this.peek()) || this.peek() === "_") text += this.advance();
    let floating = false;
    if (this.peek() === "." && isDigit(this.peek(1))) {
      floating = true;
      text += this.advance();
      while (isDigit(this.peek()) || this.peek() === "_") text += this.advance();
    }
    if (/[eE]/.test(this.peek())) {
      floating = true;
      text += this.advance();
      if (this.peek() === "+" || this.peek() === "-") text += this.advance();
      while (isDigit(this.peek()) || this.peek() === "_") text += this.advance();
    }
    if (text.startsWith("_") || text.endsWith("_") || text.includes("__") || /_\.|\._|_[eE]|[eE]_|[+-]_/.test(text)) {
      this.report(floating ? "invalid-float-literal" : "invalid-integer-literal", `invalid numeric literal '${text}'`, start);
      return;
    }
    const clean = text.replaceAll("_", "");
    this.emit(floating ? "float" : "integer", text, start, this.position(), floating ? Number(clean) : BigInt(clean));
  }

  private scanQuoted(): void {
    const start = this.position();
    const raw = this.peek() === "r";
    if (raw) this.advance();
    const quote = this.advance();
    const triple = quote === '"' && this.peek() === '"' && this.peek(1) === '"';
    if (triple) {
      this.advance();
      this.advance();
    }
    const terminator = triple ? quote.repeat(3) : quote;
    let text = raw ? "r" + terminator : terminator;
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
      const current = this.advance();
      text += current;
      if (current === "\n") {
        value += "\n";
        continue;
      }
      if (current === "\\" && !raw) {
        const escaped = this.advance();
        text += escaped;
        const escapes: Readonly<Record<string, string>> = { "0": "\0", n: "\n", r: "\r", t: "\t", "\\": "\\", '"': '"', "'": "'", "$": "$" };
        if (escaped === "u" && this.peek() === "{") {
          text += this.advance();
          let digits = "";
          while (/[0-9a-fA-F]/.test(this.peek()) && digits.length < 7) {
            const digit = this.advance();
            digits += digit;
            text += digit;
          }
          if (this.peek() === "}") text += this.advance();
          const codePoint = digits.length >= 1 && digits.length <= 6 ? Number.parseInt(digits, 16) : -1;
          if (codePoint < 0 || codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff) || !text.endsWith("}")) {
            this.report("invalid-escape", "invalid Unicode escape sequence", start);
          } else {
            value += String.fromCodePoint(codePoint);
          }
        } else if (!(escaped in escapes)) {
          this.report("invalid-escape", `invalid escape sequence '\\${escaped}'`, start);
        } else {
          value += escapes[escaped];
        }
      } else if (current === "$" && !raw && quote === '"') {
        if (value.length > 0) segments.push({ kind: "text", value, span: { start: segmentStart, end: { ...this.position(), offset: this.offset - 1, column: this.column - 1 } } });
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
          segments.push({ kind: "expression", source, span: { start: expressionStart, end: this.position() } });
          segmentStart = this.position();
        } else if (this.peek() === "{") {
          text += this.advance();
          const expressionStart = this.position();
          const source = this.scanInterpolationExpression();
          text += source.text;
          if (source.terminated) {
            segments.push({ kind: "expression", source: source.source, span: { start: expressionStart, end: source.end } });
            segmentStart = this.position();
          } else {
            this.report("unterminated-string-interpolation", "unterminated '${...}' interpolation", expressionStart);
            break;
          }
        } else {
          this.report("invalid-string-interpolation", "an unescaped '$' must be followed by an identifier or '{'", start);
          segmentStart = this.position();
        }
      } else {
        value += current;
      }
    }
    if (!terminated) this.report("unterminated-string", "unterminated literal", start);
    if (quote === "'" && [...value].length !== 1) {
      this.report("invalid-character-literal", "a character literal must contain one Unicode scalar value", start);
    }
    if (segments.length > 0) {
      if (value.length > 0) segments.push({ kind: "text", value, span: { start: segmentStart, end: this.position() } });
      this.emit("string", text, start, this.position(), { kind: "interpolated-string", segments });
    } else {
      this.emit(quote === "'" ? "character" : "string", text, start, this.position(), value);
    }
  }

  private scanInterpolationExpression(): { readonly source: string; readonly text: string; readonly end: SourcePosition; readonly terminated: boolean } {
    const stack = ["}"];
    let source = "";
    let text = "";
    while (!this.done()) {
      const current = this.peek();
      if (current === '"' || current === "'" || (current === "r" && this.peek(1) === '"')) {
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
    let text = "";
    const raw = this.peek() === "r";
    if (raw) text += this.advance();
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
    const symbol = MULTI_SYMBOLS.find((candidate) => this.source.startsWith(candidate, this.offset)) ?? this.peek();
    if (!SINGLE_SYMBOLS.has(symbol[0]!) && !MULTI_SYMBOLS.includes(symbol)) {
      this.advance();
      this.report("unexpected-character", `unexpected character '${symbol}'`, start);
      return;
    }
    for (let index = 0; index < symbol.length; index += 1) this.advance();
    if (symbol === ";") this.report("reserved-semicolon", "semicolon is reserved and cannot separate statements", start);
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

  private emit(kind: TokenKind, text: string, start: SourcePosition, end: SourcePosition, value?: Token["value"]): void {
    this.tokens.push({ kind, text, value, span: { start, end } });
    if (!(["newline", "indent", "dedent", "eof"] as TokenKind[]).includes(kind)) {
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
