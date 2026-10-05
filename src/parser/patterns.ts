import type { DataPatternField, Pattern } from "../ast.ts";
import type { SourcePosition } from "../diagnostics.ts";
import { RangeParser } from "./range.ts";

interface PatternBindings {
  readonly bindings: readonly (string | undefined)[];
  readonly names: readonly (string | undefined)[];
  readonly patterns: readonly Pattern[];
}

/** Patterns (02-grammar.md#patterns), below the expression parser. */
export abstract class PatternParser extends RangeParser {
  /**
   * A `...` after a tuple pattern's element makes it a spread pattern: a
   * name or `_`, last, and alone it keeps the trailing comma
   * (02-grammar.md#r-grammar.pattern.tuple-spread).
   */
  private matchSpreadPattern(element: Pattern, alone: boolean): boolean {
    if (!this.atText("...")) return false;
    const ellipsis = this.advance();
    const last = (this.atText(",") && this.peek(1).text === ")") || (this.atText(")") && !alone);
    if (!last || (element.kind !== "binding" && element.kind !== "wildcard"))
      this.fail(
        "syntax-error",
        "a spread pattern is a name or '_' that ends a tuple pattern; alone it keeps the trailing comma, as in '(xs...,)'",
        ellipsis.span,
      );
    return true;
  }

  /**
   * True while a `let` pattern is parsed: `mut` may then precede each name
   * the pattern binds (02-grammar.md#r-grammar.stmt.let-pattern.mut).
   */
  protected letPattern = false;

  /** `mut name` in a `let` pattern, or undefined when `mut` does not follow. */
  private parseMutBindingPattern(): Pattern | undefined {
    if (!this.letPattern || !this.atText("mut")) return undefined;
    const mut = this.advance();
    const name = this.expectKind(
      "identifier",
      "'mut' in a let pattern precedes a name the pattern binds, as in 'let (mut log, db) = ...'",
    );
    if (["{", "(", "."].includes(this.current().text))
      this.fail(
        "syntax-error",
        "'mut' precedes a name the pattern binds, not a whole pattern",
        mut.span,
      );
    return {
      kind: "binding",
      name: name.text,
      mutableAccess: true,
      mutSpan: { start: mut.span.start, end: name.span.start },
      span: name.span,
    };
  }

  protected parsePattern(): Pattern {
    const start = this.current().span.start;
    const mutBinding = this.parseMutBindingPattern();
    if (mutBinding) return mutBinding;
    if (this.matchText("_"))
      return { kind: "wildcard", span: { start, end: this.peek(-1).span.end } };
    if (this.matchText("true"))
      return { kind: "boolean", value: true, span: { start, end: this.peek(-1).span.end } };
    if (this.matchText("false"))
      return { kind: "boolean", value: false, span: { start, end: this.peek(-1).span.end } };
    const range = this.parseRangePattern();
    if (range) return range;
    const negative = this.matchText("-");
    const literal = this.current();
    // A suffixed literal is a call, not a pattern
    // (02-grammar.md#r-grammar.pattern.no-literal-call).
    if (literal.suffix)
      this.fail("syntax-error", "a suffixed literal cannot be a pattern", literal.span);
    if (literal.kind === "integer") {
      this.advance();
      const value = literal.value as bigint;
      return {
        kind: "integer",
        value: negative ? -value : value,
        span: { start, end: literal.span.end },
      };
    }
    if (literal.kind === "float") {
      this.advance();
      const value = literal.value as number;
      return {
        kind: "float",
        value: negative ? -value : value,
        span: { start, end: literal.span.end },
      };
    }
    if (negative)
      this.fail(
        "expected-pattern",
        "'-' in a pattern must precede a numeric literal",
        literal.span,
      );
    // A prefixed string is a call, never a pattern
    // (02-grammar.md#r-grammar.pattern.no-literal-call).
    if (literal.kind === "string" && literal.prefix)
      this.fail("syntax-error", "a prefixed string cannot be a pattern", literal.span);
    if (literal.kind === "string") {
      this.advance();
      if (typeof literal.value !== "string")
        this.fail(
          "interpolated-pattern",
          "string patterns must be constant and cannot contain interpolation",
          literal.span,
        );
      return { kind: "string", value: literal.value, span: literal.span };
    }
    if (literal.kind === "character") {
      this.advance();
      return { kind: "character", value: literal.value as string, span: literal.span };
    }
    if (this.matchText("(")) {
      // `()` is the unit pattern, irrefutable for `void` (02-grammar.md#r-grammar.pattern.unit).
      if (this.matchText(")"))
        return { kind: "wildcard", unit: true, span: { start, end: this.peek(-1).span.end } };
      // `tuple_pattern` needs a comma: `(p,)` or `(p, q)`; its last element
      // may be a spread pattern `xs...` or `_...` (02-grammar.md#patterns).
      const elements = [this.parsePattern()];
      let spread = this.matchSpreadPattern(elements[0]!, true);
      this.expectText(",");
      while (!spread && !this.atText(")")) {
        elements.push(this.parsePattern());
        spread = this.matchSpreadPattern(elements.at(-1)!, false);
        if (!this.matchText(",")) break;
      }
      const close = this.expectText(")");
      return {
        kind: "tuple",
        elements,
        ...(spread ? { spread: true } : {}),
        span: { start, end: close.span.end },
      };
    }
    if (this.matchText(".")) {
      const variant = this.expectKind("identifier", "expected a variant name after '.'");
      const { bindings, names, patterns } = this.parsePatternBindings();
      return {
        kind: "variant",
        variantName: variant.text,
        bindings,
        bindingNames: names,
        payloadPatterns: patterns,
        span: { start, end: this.peek(-1).span.end },
      };
    }
    const first = this.expectKind("identifier", "expected a supported match pattern");
    // A qualified data or variant pattern may start with module segments, as
    // in `cmp.Ordering.Less` (02-grammar.md#r-grammar.pattern.variant): the
    // last segment of a variant pattern is the variant, and the segments
    // before it name the enum. Name resolution decides what each names.
    const segments = [first.text];
    while (this.matchText(".")) {
      segments.push(this.expectKind("identifier", "expected a variant name after '.'").text);
    }
    if (this.atText("{")) return this.parseDataPattern(segments.join("."), start);
    if (segments.length > 1) {
      const { bindings, names, patterns } = this.parsePatternBindings();
      return {
        kind: "variant",
        enumName: segments.slice(0, -1).join("."),
        variantName: segments.at(-1)!,
        bindings,
        bindingNames: names,
        payloadPatterns: patterns,
        span: { start, end: this.peek(-1).span.end },
      };
    }
    if (this.atText("?"))
      this.fail(
        "syntax-error",
        `'${first.text}?' is not a pattern; match an optional with '.Some(${first.text})'`,
        this.current().span,
      );
    if (this.atText("(")) {
      const { bindings, names, patterns } = this.parsePatternBindings();
      return {
        kind: "variant",
        variantName: first.text,
        bare: true,
        bindings,
        bindingNames: names,
        payloadPatterns: patterns,
        span: { start, end: this.peek(-1).span.end },
      };
    }
    return { kind: "binding", name: first.text, span: first.span };
  }

  /** A data pattern `Type { field: pattern, ... }` from its `{` (02-grammar.md#patterns). */
  private parseDataPattern(typeName: string, start: SourcePosition): Pattern {
    this.expectText("{");
    const fields: DataPatternField[] = [];
    if (!this.atText("}")) {
      do {
        // `Point { mut tags }` binds the field mutably in a `let` pattern.
        const mut = this.letPattern && this.atText("mut") ? this.advance() : undefined;
        const field = this.expectKind("identifier", "expected a data pattern field");
        if (mut && this.atText(":"))
          this.fail(
            "syntax-error",
            "'mut' precedes the name a field binds, as in 'Point { x: mut name }'",
            mut.span,
          );
        if (this.atText("="))
          this.fail(
            "syntax-error",
            "a data pattern labels a field with ':', as in 'Point { x: 0 }'",
            this.current().span,
          );
        const pattern = this.matchText(":")
          ? this.parsePattern()
          : {
              kind: "binding" as const,
              name: field.text,
              ...(mut
                ? {
                    mutableAccess: true,
                    mutSpan: { start: mut.span.start, end: field.span.start },
                  }
                : {}),
              span: field.span,
            };
        fields.push({
          name: field.text,
          pattern,
          span: { start: field.span.start, end: pattern.span.end },
        });
      } while (this.matchText(",") && !this.atText("}"));
    }
    const close = this.expectText("}");
    return { kind: "data", typeName, fields, span: { start, end: close.span.end } };
  }

  protected parsePatternBindings(): PatternBindings {
    const bindings: Array<string | undefined> = [];
    const names: Array<string | undefined> = [];
    const patterns: Pattern[] = [];
    let sawNamed = false;
    if (this.matchText("(")) {
      if (!this.atText(")")) {
        do {
          if (this.current().kind === "identifier" && this.peek(1).text === "=") {
            sawNamed = true;
            const name = this.advance();
            this.advance();
            const pattern = this.parsePattern();
            names.push(name.text);
            patterns.push(pattern);
            bindings.push(pattern.kind === "binding" ? pattern.name : undefined);
          } else {
            if (sawNamed)
              this.fail(
                "pattern-order",
                "positional patterns must precede named patterns",
                this.current().span,
              );
            names.push(undefined);
            const pattern = this.parsePattern();
            patterns.push(pattern);
            bindings.push(pattern.kind === "binding" ? pattern.name : undefined);
          }
        } while (this.matchText(",") && !this.atText(")"));
      }
      this.expectText(")");
    }
    return { bindings, names, patterns };
  }
}
