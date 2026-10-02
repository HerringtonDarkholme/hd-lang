import type { Expression, Pattern } from "../ast.ts";
import { ParserBase } from "./base.ts";

// `..` and `..=` bind more loosely than `||` and more tightly than `:=`
// (02-grammar.md#r-grammar.expr.range.precedence).
export const RANGE_PRECEDENCE = 0.5;

/** The token kinds and texts that can begin an operand. */
const OPERAND_KINDS = new Set(["identifier", "integer", "float", "string", "character"]);
const OPERAND_STARTS = new Set(
  "( [ { - + ! ~ . $ _ true false self if for while match fn".split(" "),
);

/** Range expressions and range patterns (02-grammar.md#range-syntax, #patterns). */
export abstract class RangeParser extends ParserBase {
  protected abstract parseExpression(minimumPrecedence?: number): Expression;

  protected atRangeOperator(): boolean {
    return this.atText("..") || this.atText("..=");
  }

  /**
   * A range expression at its `..` or `..=`, after the start bound if there
   * is one. A `..` has an end bound exactly when the next token can begin an
   * operand, a `..=` always has one, and a range is non-associative.
   */
  protected parseRange(start: Expression | undefined): Expression {
    const operator = this.advance();
    const inclusive = operator.text === "..=";
    let end: Expression | undefined;
    if (this.canBeginOperand()) end = this.parseExpression(RANGE_PRECEDENCE + 0.5);
    else if (inclusive)
      this.fail("syntax-error", "'..=' needs an end bound, as in 'a..=b'", this.current().span);
    if (this.atRangeOperator())
      this.fail(
        "syntax-error",
        "a range bound cannot be a range; parenthesize one of them",
        this.current().span,
      );
    return {
      kind: "range",
      ...(start ? { start } : {}),
      ...(end ? { end } : {}),
      inclusive,
      span: { start: (start ?? operator).span.start, end: (end ?? operator).span.end },
    };
  }

  /** Whether the current token can begin an operand (02-grammar.md#r-grammar.expr.range.open-end). */
  private canBeginOperand(): boolean {
    const token = this.current();
    return OPERAND_KINDS.has(token.kind) || OPERAND_STARTS.has(token.text);
  }

  /**
   * A range pattern `a..=b`, `a..b`, `a..`, `..=b`, or `..b` at the current
   * token, or undefined when none starts here. Each bound is an integer
   * literal that a `-` may precede (02-grammar.md#r-grammar.pattern.range).
   */
  protected parseRangePattern(): Pattern | undefined {
    const start = this.current().span.start;
    if (this.atText("..") || this.atText("..=")) {
      const inclusive = this.advance().text === "..=";
      const end = this.parseRangePatternBound();
      return { kind: "range", end, inclusive, span: { start, end: this.peek(-1).span.end } };
    }
    const offset = this.atText("-") ? 1 : 0;
    const operator = this.peek(offset + 1).text;
    if (operator !== ".." && operator !== "..=") return undefined;
    const low = this.parseRangePatternBound();
    const inclusive = this.advance().text === "..=";
    const high =
      inclusive || this.atText("-") || this.current().kind === "integer"
        ? this.parseRangePatternBound()
        : undefined;
    return {
      kind: "range",
      start: low,
      ...(high === undefined ? {} : { end: high }),
      inclusive,
      span: { start, end: this.peek(-1).span.end },
    };
  }

  /** A range pattern bound: an integer literal, which a `-` may precede (02-grammar.md#r-grammar.pattern.range.bound). */
  private parseRangePatternBound(): bigint {
    const negative = this.matchText("-");
    const literal = this.current();
    if (literal.kind !== "integer" || literal.suffix)
      this.fail("syntax-error", "a range pattern bound must be an integer literal", literal.span);
    this.advance();
    const value = literal.value as bigint;
    return negative ? -value : value;
  }
}
