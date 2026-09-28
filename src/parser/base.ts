import type { Expression, Statement, TypeRef, UseDecl, UseName, VarianceMarker } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import type { Token, TokenKind } from "../lexer.ts";
import { rowArgumentType } from "../types.ts";

export class ParseFailure extends Error {}

export interface ExpressionParseResult {
  readonly expression?: Expression;
  readonly diagnostics: readonly Diagnostic[];
}

export abstract class ParserBase {
  // Mutable: a suite nested inside brackets gets its layout tokens spliced in
  // when the parser reaches its header (see `openNestedLayout`).
  protected readonly tokens: Token[];
  protected index = 0;
  protected readonly diagnostics: Diagnostic[] = [];
  protected activeGenericParameters: ReadonlySet<string> = new Set();

  constructor(tokens: readonly Token[]) {
    this.tokens = [...tokens];
  }

  // Set while parsing a declaration or closure result (02-grammar.md#types):
  // function types along the result chain carry no row, so a trailing
  // requirement clause belongs to the declaration.
  protected rowlessResult = false;

  protected abstract parseType(): TypeRef;

  protected parseResultType(): TypeRef {
    this.rowlessResult = true;
    try {
      return this.parseType();
    } finally {
      this.rowlessResult = false;
    }
  }

  protected abstract parseSuite(closureBody?: boolean): readonly Statement[];
  protected abstract parseStatement(topOrInline: boolean): Statement;
  protected abstract finishSimpleStatement(topOrInline: boolean): SourceSpan["end"];
  protected abstract parseRequirements(header?: boolean): readonly string[];
  protected abstract parseRequirementKey(): string;
  protected abstract parseExpressionSource(source: string): ExpressionParseResult;

  protected parseDocComments(): string | undefined {
    if (!this.atKind("doc-comment")) return undefined;
    const lines: string[] = [];
    const first = this.current();
    let previous = first;
    while (this.atKind("doc-comment")) {
      const comment = this.advance();
      if (
        lines.length > 0 &&
        (comment.span.start.line !== previous.span.end.line + 1 ||
          comment.span.start.column !== first.span.start.column)
      ) {
        this.fail(
          "doc-comment-without-target",
          "documentation comment lines must be consecutive and share an indentation",
          comment.span,
        );
      }
      lines.push(String(comment.value ?? comment.text));
      previous = comment;
      this.matchKind("newline");
    }
    const target = this.current();
    if (
      target.kind === "eof" ||
      target.kind === "dedent" ||
      target.span.start.line !== previous.span.end.line + 1 ||
      target.span.start.column !== first.span.start.column
    ) {
      this.fail(
        "doc-comment-without-target",
        "documentation comments must immediately precede a declaration or member at the same indentation",
        first.span,
      );
    }
    return lines.join("\n");
  }

  private depths?: number[];

  // Delimiter depths of the same-line suites being parsed, innermost last.
  protected readonly inlineSuiteDepths: number[] = [];

  /** The indentation of the physical line holding the token at `index`. */
  protected lineIndentAt(index: number): number {
    let first = index;
    const line = this.tokens[index]!.span.start.line;
    const layout = ["newline", "indent", "dedent"];
    while (
      first > 0 &&
      this.tokens[first - 1]!.span.start.line === line &&
      !layout.includes(this.tokens[first - 1]!.kind)
    )
      first -= 1;
    return this.tokens[first]!.span.start.column - 1;
  }

  /** The indentation of the first physical line of the logical line holding `index`. */
  protected logicalLineIndentAt(index: number): number {
    let first = index;
    while (first > 0 && !["newline", "indent", "dedent"].includes(this.tokens[first - 1]!.kind))
      first -= 1;
    return this.lineIndentAt(first);
  }

  /**
   * An indented suite nested inside brackets must start deeper than both its
   * header's line and the logical line containing the header
   * (01-lexical-structure.md#physical-and-logical-lines).
   */
  protected checkNestedSuiteIndent(colonIndex: number, first: Token): void {
    const reference = Math.max(this.lineIndentAt(colonIndex), this.logicalLineIndentAt(colonIndex));
    if (first.span.start.column - 1 <= reference)
      this.fail(
        "unexpected-indentation",
        "a nested suite's body must be deeper than the statement that contains its header",
        first.span,
      );
  }

  /**
   * A comma at the delimiter depth where a same-line suite opened closes that
   * suite (01-lexical-structure.md#physical-and-logical-lines), so a
   * multi-name binding or loop written directly in the suite is a syntax
   * error. Call this at such a comma.
   */
  protected rejectCommaClosingInlineSuite(): void {
    if (this.atText(",") && this.inlineSuiteDepths.at(-1) === this.delimiterDepth(this.index))
      this.fail(
        "syntax-error",
        "a comma ends a same-line suite, so several names need an indented body or parentheses",
        this.current().span,
      );
  }

  /**
   * Layout for an indented suite nested inside brackets
   * (01-lexical-structure.md#physical-and-logical-lines). The lexer emits no
   * layout inside brackets, so when the parser reaches a suite header whose
   * `:` (at `colonIndex`) ends its line, this splices in the `NEWLINE`,
   * `INDENT`, and `DEDENT` tokens that layout processing emits for the body:
   * its lines at the suite's delimiter depth, including the suites nested in
   * it at that depth, are laid out as at depth zero. The body ends at a
   * closing delimiter at that depth or at a line indented less than the body.
   * A closure body ends only at a line indented no farther than its header's
   * line that starts with `,` or a closing delimiter at that depth. Returns
   * false when the suite does not start on a later line inside brackets.
   */
  protected openNestedLayout(colonIndex: number, closureBody: boolean): boolean {
    const first = this.index;
    const firstToken = this.tokens[first]!;
    const colon = this.tokens[colonIndex]!;
    if (
      ["newline", "indent", "dedent", "eof"].includes(firstToken.kind) ||
      firstToken.span.start.line <= colon.span.end.line
    )
      return false;
    this.checkNestedSuiteIndent(colonIndex, firstToken);
    const depth = this.delimiterDepth(first);
    const headerIndent = this.lineIndentAt(colonIndex);
    const bodyIndent = firstToken.span.start.column - 1;
    const closers = new Set([")", "]", "}"]);
    const inserts: { readonly at: number; readonly kinds: readonly TokenKind[] }[] = [
      { at: first, kinds: ["newline", "indent"] },
    ];
    const indents = [bodyIndent];
    let previous = firstToken;
    let end = first + 1;
    for (; end < this.tokens.length; end += 1) {
      const token = this.tokens[end]!;
      if (token.kind === "eof") break;
      if (["newline", "indent", "dedent"].includes(token.kind)) continue;
      if (this.delimiterDepth(end) > depth) {
        previous = token;
        continue;
      }
      const closer = closers.has(token.text);
      const startsLine = token.span.start.line > previous.span.end.line;
      if (!startsLine) {
        if (!closer) {
          previous = token;
          continue;
        }
        if (closureBody)
          this.fail(
            "syntax-error",
            "a closing delimiter on a body line does not end an indented closure body; start the line with it",
            token.span,
          );
        break;
      }
      const indent = token.span.start.column - 1;
      if (indent > indents.at(-1)!) {
        const leadingDot =
          token.text === "." &&
          this.tokens[end + 1]?.kind === "identifier" &&
          previous.text !== ":" &&
          previous.text !== "=>";
        if (!leadingDot) {
          inserts.push({ at: end, kinds: ["newline", "indent"] });
          indents.push(indent);
        }
      } else if (indent >= bodyIndent) {
        const kinds: TokenKind[] = ["newline"];
        while (indent < indents.at(-1)!) {
          indents.pop();
          kinds.push("dedent");
        }
        if (indent !== indents.at(-1))
          this.fail(
            "invalid-dedent",
            `column ${indent + 1} is not an active indentation level`,
            token.span,
          );
        inserts.push({ at: end, kinds });
      } else {
        if (closureBody && indent > headerIndent)
          this.fail(
            "syntax-error",
            "a line after an indented closure body must be indented no farther than the closure header",
            token.span,
          );
        if (closureBody && !closer && token.text !== ",")
          this.fail(
            "syntax-error",
            "after an indented closure body inside brackets, the next line must start with ',' or a closing delimiter",
            token.span,
          );
        break;
      }
      previous = token;
    }
    inserts.push({ at: end, kinds: ["newline", ...indents.map((): TokenKind => "dedent")] });
    for (const insert of inserts.reverse()) {
      const position = this.tokens[insert.at]!.span.start;
      const span = { start: position, end: position };
      this.tokens.splice(
        insert.at,
        0,
        ...insert.kinds.map((kind) => ({ kind, text: kind === "newline" ? "\n" : "", span })),
      );
    }
    this.depths = undefined;
    return true;
  }

  // `+T` and `-T` declare variance (04-type-system.md#variance).
  protected parseVarianceMarker(): VarianceMarker {
    if (this.matchText("+")) return "+";
    if (this.matchText("-")) return "-";
    return undefined;
  }

  // After `return` or `break`: the statement ends without a value, at a line
  // ending or, in a same-line suite, where that suite ends.
  protected atValuelessEnd(topOrInline: boolean): boolean {
    if (this.atKind("newline") || this.atKind("dedent") || this.atKind("eof")) return true;
    return topOrInline && [")", ",", "]", "}", "else"].some((text) => this.atText(text));
  }

  // A type argument may be a row, `$()` or `$(A, B)`, for a row-kinded
  // parameter (02-grammar.md#types); a single key reads as a type.
  protected parseTypeArgument(): TypeRef {
    const start = this.current().span.start;
    if (this.atText("$") && this.peek(1).text === "(") {
      this.advance();
      const keys = this.parseRequirements(false);
      return { name: rowArgumentType(keys), span: { start, end: this.peek(-1).span.end } };
    }
    const argument = this.parseType();
    this.rejectOldRowOperator();
    return argument;
  }

  // `$ A + B` and `$ (R - K)` are the removed row union and subtraction.
  protected rejectOldRowOperator(): void {
    const operator = this.current();
    if (operator.text !== "+" && operator.text !== "-") return;
    this.fail(
      "old-row-operator",
      operator.text === "+"
        ? "requirement rows are comma lists: write '$ A, B' before a header's ':' or '$(A, B)' inside a type"
        : "row subtraction was removed: take the callback as 'fn(...) -> T $(R, K)' and declare '$ R' to remove K",
      operator.span,
    );
  }

  /** True when the current token starts on the line where the previous token ends. */
  protected onPreviousLine(): boolean {
    return this.current().span.start.line === this.peek(-1).span.end.line;
  }

  /** The number of open `(`, `[`, and `{` before the token at `index`. */
  protected delimiterDepth(index: number): number {
    if (!this.depths) {
      const depths: number[] = [];
      let depth = 0;
      for (const token of this.tokens) {
        depths.push(depth);
        if (token.text === "(" || token.text === "[" || token.text === "{") depth += 1;
        else if (token.text === ")" || token.text === "]" || token.text === "}")
          depth = Math.max(0, depth - 1);
      }
      this.depths = depths;
    }
    return this.depths[index] ?? 0;
  }

  /**
   * Outside brackets, a control-flow header cannot end in a suite
   * (02-grammar.md#statements): layout would carry that suite over the `:`.
   */
  protected rejectHeaderEndingInSuite(keyword: Token, header: Expression): void {
    if (this.peek(-1).kind === "dedent" && this.delimiterDepth(this.tokens.indexOf(keyword)) === 0)
      this.fail(
        "syntax-error",
        "a control-flow header outside brackets cannot end in an indented suite",
        header.span,
      );
  }

  protected expectText(text: string): Token {
    if (!this.atText(text))
      this.fail(
        "syntax-error",
        `expected '${text}', found '${this.current().text}'`,
        this.current().span,
      );
    return this.advance();
  }

  protected expectKind(kind: Token["kind"], message: string): Token {
    if (!this.atKind(kind)) this.fail("syntax-error", message, this.current().span);
    return this.advance();
  }

  protected matchText(text: string): boolean {
    if (!this.atText(text)) return false;
    this.advance();
    return true;
  }

  protected matchKind(kind: Token["kind"]): boolean {
    if (!this.atKind(kind)) return false;
    this.advance();
    return true;
  }

  // A raw identifier never matches a keyword or contextual word.
  protected atText(text: string): boolean {
    const token = this.current();
    return token.text === text && !token.raw;
  }

  protected atKind(kind: Token["kind"]): boolean {
    return this.current().kind === kind;
  }

  protected current(): Token {
    return this.tokens[Math.min(this.index, this.tokens.length - 1)]!;
  }

  protected peek(distance: number): Token {
    return this.tokens[Math.max(0, Math.min(this.index + distance, this.tokens.length - 1))]!;
  }

  protected advance(): Token {
    const token = this.current();
    if (token.kind !== "eof") this.index += 1;
    return token;
  }

  // The prototype parses no decorators, but the grammar lets derive lines
  // precede only data, enum, and newtype declarations
  // (02-grammar.md#r-grammar.annot.newtype-derive.error). Looking past
  // `@derive(...)` lines, this returns the span of a transparent alias's
  // `type` keyword, which is then a syntax error.
  protected aliasAfterDeriveLines(): SourceSpan | undefined {
    let offset = 0;
    while (this.peek(offset).text === "@" && this.peek(offset + 1).text === "derive") {
      if (this.peek(offset + 2).text !== "(") return undefined;
      offset += 3;
      let depth = 1;
      while (depth > 0) {
        const token = this.peek(offset);
        if (token.kind === "eof" || token.kind === "newline") return undefined;
        if (token.text === "(") depth += 1;
        else if (token.text === ")") depth -= 1;
        offset += 1;
      }
      if (this.peek(offset).kind !== "newline") return undefined;
      offset += 1;
    }
    if (offset === 0) return undefined;
    if (this.peek(offset).text === "pub") offset += 1;
    const keyword = this.peek(offset);
    if (keyword.text !== "type" || this.peek(offset + 1).kind !== "identifier") return undefined;
    offset += 2;
    if (this.peek(offset).text === "[") {
      let depth = 0;
      do {
        const token = this.peek(offset);
        if (token.kind === "eof" || token.kind === "newline") return undefined;
        if (token.text === "[") depth += 1;
        else if (token.text === "]") depth -= 1;
        offset += 1;
      } while (depth > 0);
    }
    return this.peek(offset).text === "=" ? keyword.span : undefined;
  }

  protected fail(code: string, message: string, span: SourceSpan): never {
    this.diagnostics.push({ code, message, span });
    throw new ParseFailure(message);
  }

  // `use` begins a use declaration only before a use root
  // (01-lexical-structure.md#keywords-and-reserved-words).
  protected atUseDeclaration(): boolean {
    const offset = this.atText("pub") ? 1 : 0;
    const word = this.peek(offset);
    const root = this.peek(offset + 1);
    return (
      word.text === "use" &&
      !word.raw &&
      !root.raw &&
      ["pkg", "std", "dep", "self", "super", "tests"].includes(root.text)
    );
  }

  protected parseUse(): UseDecl {
    const public_ = this.matchText("pub");
    const start = this.expectText("use").span.start;
    // The `tests` root names integration test modules. The prototype compiles
    // one ordinary module, never one under `tests/`, so every such use is
    // test-only-use (10-modules.md#r-module.test.tests-root-elsewhere).
    if (this.atText("tests"))
      this.fail(
        "test-only-use",
        "the tests use root is available only in an integration test module under tests/",
        this.current().span,
      );
    // A use root: `pkg`, `std`, `dep`, `super`, or the reserved word `self`.
    const parts = [
      this.atText("self")
        ? this.advance().text
        : this.expectKind("identifier", "expected a module path after use").text,
    ];
    let grouped = false;
    while (this.matchText(".")) {
      if (this.matchText("{")) {
        grouped = true;
        break;
      }
      parts.push(this.expectKind("identifier", "expected a module path component").text);
    }
    const names: UseName[] = [];
    let module: string;
    if (grouped) {
      module = parts.join(".");
      if (!this.atText("}")) {
        do {
          const name = this.expectKind("identifier", "expected an imported declaration name").text;
          if (this.atText("."))
            this.fail(
              "direct-variant-use",
              "enum variants cannot be imported directly",
              this.current().span,
            );
          const alias = this.matchText("as")
            ? this.expectKind("identifier", "expected an import alias").text
            : undefined;
          names.push({ name, ...(alias ? { alias } : {}) });
        } while (this.matchText(",") && !this.atText("}"));
      }
      this.expectText("}");
    } else {
      // Only the grouped form accepts a `pub` prefix (10 Use Forms).
      if (public_)
        this.fail(
          "syntax-error",
          "only the grouped use form accepts 'pub'; write 'pub use module.{Name}'",
          { start, end: this.peek(-1).span.end },
        );
      const name = parts.pop()!;
      module = parts.join(".");
      const alias = this.matchText("as")
        ? this.expectKind("identifier", "expected an import alias").text
        : undefined;
      names.push({ name, ...(alias ? { alias } : {}) });
    }
    const end = this.finishSimpleStatement(false);
    return {
      kind: "use",
      module,
      names,
      ...(public_ ? { public: true } : {}),
      span: { start, end },
    };
  }
}
