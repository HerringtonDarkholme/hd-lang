import type { Expression, Pattern, Statement } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { ParseFailure } from "./base.ts";
import { DecoratorParser } from "./decorators.ts";

/** `let` statements and the `missing-let` check before `:=` (02-grammar.md#let-statements). */
export abstract class LetParser extends DecoratorParser {
  protected abstract parseRightSide(): Expression;
  protected abstract finishExpressionStatement(
    expression: Expression,
    topOrInline: boolean,
  ): SourceSpan["end"];

  /**
   * `:=` binds exactly one name, so a pattern before it, such as `(a, b)`,
   * `Point { x, y }`, or `.Some(v)`, is `missing-let` with a fix-it that
   * writes `let` and `=` (02-grammar.md#r-grammar.stmt.short-binding.let-only).
   */
  protected rejectPatternBeforeShortBinding(): void {
    const first = this.current();
    const opensPattern =
      first.text === "(" ||
      first.text === "." ||
      (first.kind === "identifier" && ["{", "(", "."].includes(this.peek(1).text));
    if (!opensPattern) return;
    // The pattern must reach `:=` outside any delimiter on this line.
    let depth = 0;
    let offset = 0;
    for (;;) {
      const token = this.peek(offset);
      if (token.kind === "eof" || token.kind === "newline" || token.kind === "indent") return;
      if (["(", "[", "{"].includes(token.text)) depth += 1;
      else if ([")", "]", "}"].includes(token.text)) depth -= 1;
      else if (depth === 0 && token.text === ":") return;
      else if (depth === 0 && token.text === ":=") break;
      if (depth < 0) return;
      offset += 1;
    }
    const mark = this.index;
    const diagnostics = this.diagnostics.length;
    try {
      this.parsePattern();
    } catch (error) {
      if (!(error instanceof ParseFailure)) throw error;
      this.index = mark;
      this.diagnostics.length = diagnostics;
      return;
    }
    const operator = this.current();
    if (operator.text !== ":=") {
      this.index = mark;
      return;
    }
    this.fail(
      "missing-let",
      "':=' binds one name; a pattern before it needs 'let', as in 'let (a, b) = pair'",
      { start: first.span.start, end: operator.span.end },
      {
        message: "write 'let' and '='",
        edits: [
          { span: { start: first.span.start, end: first.span.start }, replacement: "let " },
          { span: operator.span, replacement: "=" },
        ],
      },
    );
  }

  /**
   * `let pattern [: type] = value [else: suite]` (02-grammar.md#let-statements,
   * 02-grammar.md#let-else-statements). A name or a tuple of names and `_`
   * without an else block stays a `binding` or `tuple-binding`, and `let _ =
   * value` is a discard (06-control-flow.md#r-flow.unused.discard-forms);
   * any other form is a `pattern-binding` that the checker lowers to a match.
   */
  protected parseLetStatement(start: SourceSpan["start"], topOrInline: boolean): Statement {
    // `let (a) = ...` has no comma, so it is no tuple pattern
    // (02-grammar.md#r-grammar.stmt.let-pattern.parenthesized).
    const offset = this.peek(1).text === "mut" ? 2 : 1;
    if (
      this.atText("(") &&
      (this.peek(offset).kind === "identifier" || this.peek(offset).text === "_") &&
      this.peek(offset + 1).text === ")"
    )
      this.fail(
        "syntax-error",
        "a parenthesized let list needs at least two names; write 'let name = ...' for one",
        { start: this.current().span.start, end: this.peek(offset + 1).span.end },
      );
    const previous = this.letPattern;
    this.letPattern = true;
    let pattern: Pattern;
    try {
      pattern = this.parsePattern();
      this.rejectCommaClosingInlineSuite();
      if (this.atText(",")) {
        let last = pattern;
        while (this.matchText(",")) last = this.parsePattern();
        this.failBareNameList("let", pattern, last);
      }
    } finally {
      this.letPattern = previous;
    }
    const annotation = this.matchText(":") ? this.parseType() : undefined;
    this.expectText("=");
    const value = this.parseRightSide();
    // A let-else is never a same-line suite body: there the `else` belongs to
    // the enclosing statement (02-grammar.md#r-grammar.stmt.let-else.not-inline).
    const inline = this.inlineSuiteDepths.at(-1) === this.delimiterDepth(this.index);
    if (!inline && this.atText("else")) {
      const last = this.peek(-1);
      const parenthesized = last.text === ")" && last.span.end.offset > value.span.end.offset;
      if (
        !parenthesized &&
        ["if", "for", "while", "match", "closure", "provider-with"].includes(value.kind)
      )
        this.fail(
          "syntax-error",
          "a let-else initializer cannot end in a suite; put it in parentheses",
          value.span,
        );
      this.advance();
      const elseBody = this.parseSuite();
      return {
        kind: "pattern-binding",
        pattern,
        ...(annotation ? { annotation } : {}),
        value,
        elseBody,
        span: { start, end: elseBody.at(-1)!.span.end },
      };
    }
    const span = { start, end: this.finishExpressionStatement(value, topOrInline) };
    if (pattern.kind === "wildcard" && !annotation) return { kind: "discard", value, span };
    if (pattern.kind === "binding")
      return {
        kind: "binding",
        name: pattern.name,
        annotation,
        mutable: true,
        ...(pattern.mutableAccess ? { mutableAccess: true, mutSpan: pattern.mutSpan } : {}),
        value,
        span,
      };
    const elements = pattern.kind === "tuple" ? pattern.elements : [];
    if (
      pattern.kind === "tuple" &&
      elements.every((element) => element.kind === "binding" || element.kind === "wildcard")
    )
      return {
        kind: "tuple-binding",
        bindings: elements.map((element, index) => ({
          name: element.kind === "binding" ? element.name : "_",
          ...(element.kind === "binding" && element.mutableAccess
            ? { mutableAccess: true, mutSpan: element.mutSpan }
            : {}),
          ...(pattern.spread && index === elements.length - 1 ? { spread: true } : {}),
          span: element.span,
        })),
        annotation,
        mutable: true,
        value,
        span,
      };
    return { kind: "pattern-binding", pattern, ...(annotation ? { annotation } : {}), value, span };
  }
}
