import type { Expression, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import type { Token } from "../lexer.ts";

export class ParseFailure extends Error {}

export interface ExpressionParseResult {
  readonly expression?: Expression;
  readonly diagnostics: readonly Diagnostic[];
}

export abstract class ParserBase {
  protected readonly tokens: readonly Token[];
  protected index = 0;
  protected readonly diagnostics: Diagnostic[] = [];
  protected activeGenericParameters: ReadonlySet<string> = new Set();

  constructor(tokens: readonly Token[]) {
    this.tokens = tokens;
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

  protected abstract parseSuite(): readonly Statement[];
  protected abstract parseStatement(topOrInline: boolean): Statement;
  protected abstract parseRequirements(): readonly string[];
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

  protected fail(code: string, message: string, span: SourceSpan): never {
    this.diagnostics.push({ code, message, span });
    throw new ParseFailure(message);
  }
}
