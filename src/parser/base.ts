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

  protected abstract parseType(): TypeRef;
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

  protected atText(text: string): boolean {
    return this.current().text === text;
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
