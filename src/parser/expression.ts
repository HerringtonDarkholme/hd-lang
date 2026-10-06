import {
  TEMPLATE_PLACEHOLDER,
  type BindingName,
  type ClosureParameter,
  type ComprehensionClause,
  type DataExpressionField,
  type Expression,
  type MapEntry,
  type MatchArm,
  type Pattern,
  type ProviderContextEntry,
  type Statement,
  type TypeRef,
} from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { InterpolatedStringValue, Token } from "../lexer.ts";
import { collectPipePlaceholders, isBarePipeStep } from "./pipe-steps.ts";
import { PatternParser } from "./patterns.ts";
import { RANGE_PRECEDENCE } from "./range.ts";

/**
 * The node with every span inside it set to `span`. An interpolated
 * expression is parsed from its own source, so its spans start at line 1.
 */
function withSpan<T>(node: T, span: SourceSpan): T {
  if (Array.isArray(node)) return node.map((item) => withSpan(item, span)) as T;
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(node))
    result[key] = key === "span" ? span : withSpan(child, span);
  return result as T;
}

/**
 * The dotted spelling `a.b.C` of a member chain over plain names, as a
 * `qualified_name` (02-grammar.md#types), or undefined for any other
 * expression. Only the outermost member may carry type arguments.
 */
function qualifiedPath(expression: Expression & { kind: "member" }): string | undefined {
  const receiver = (inner: Expression): string | undefined => {
    if (inner.kind === "name") return inner.typeArguments ? undefined : inner.name;
    if (inner.kind !== "member" || inner.typeArguments || inner.parenthesized) return undefined;
    const prefix = receiver(inner.receiver);
    return prefix === undefined ? undefined : `${prefix}.${inner.name}`;
  };
  const prefix = expression.parenthesized ? undefined : receiver(expression.receiver);
  return prefix === undefined ? undefined : `${prefix}.${expression.name}`;
}

const BINARY_PRECEDENCE: Readonly<Record<string, number>> = {
  "||": 1,
  "&&": 2,
  "==": 3,
  "!=": 3,
  is: 3,
  "<": 4,
  "<=": 4,
  ">": 4,
  ">=": 4,
  "|": 5,
  "^": 6,
  "&": 7,
  "<<": 8,
  ">>": 8,
  "+": 9,
  "-": 9,
  "*": 10,
  "/": 10,
  "%": 10,
  "**": 11,
};

// `|>` binds more loosely than `|` and more tightly than every comparison
// (05-expressions.md#r-expr.pipe.precedence).
const PIPE_PRECEDENCE = 4.5;

// The AST keeps the names "and", "or", and "not" for `&&`, `||`, and prefix `!`.
const LOGICAL_OPERATOR_NAMES: Readonly<Record<string, string>> = {
  "&&": "and",
  "||": "or",
  "!": "not",
};

type NameExpression = Extract<Expression, { kind: "name" }>;

export abstract class ExpressionParser extends PatternParser {
  protected parseExpression(minimumPrecedence = 0): Expression {
    if (
      minimumPrecedence === 0 &&
      this.current().kind === "identifier" &&
      this.peek(1).text === ":="
    ) {
      const name = this.advance();
      this.advance();
      const value = this.parseExpression();
      return {
        kind: "binding-expression",
        bindings: [{ name: name.text, span: name.span }],
        value,
        span: { start: name.span.start, end: value.span.end },
      };
    }
    // A range with no start bound: `..b`, `..=b`, or `..`
    // (02-grammar.md#r-grammar.expr.range.open-start).
    if (minimumPrecedence <= RANGE_PRECEDENCE && this.atRangeOperator())
      return this.parseRange(undefined);
    let left = this.parsePrefix();
    while (true) {
      // An expression that ended by closing an indented suite (a `for`,
      // `match`, or `if` body) is complete: the next line, such as `.None`,
      // starts a new statement.
      if (this.peek(-1).kind === "dedent") break;
      // A call, index, data-literal, or suspension suffix starts on its
      // operand's line (01-lexical-structure.md#physical-and-logical-lines).
      const suffixToken = ["(", "[", "{", "!"].includes(this.current().text);
      if (suffixToken && !this.onPreviousLine()) break;
      // In an expression, type arguments follow the marker `::`, and `[`
      // after an operand always indexes (02-grammar.md#type-arguments-in-expressions).
      if (
        this.atText("::") &&
        this.peek(1).text === "[" &&
        (left.kind === "name" || left.kind === "member" || left.kind === "qualified-name") &&
        left.typeArguments === undefined
      ) {
        const start = left.span.start;
        this.advance();
        this.advance();
        const typeArguments = this.parseTypeArgumentList();
        const close = this.expectText("]");
        // A bang call writes `!` on the name and the list after it
        // (02-grammar.md#r-grammar.primary.method-reference.no-bang).
        if (this.atText("!") && ["(", "::"].includes(this.peek(1).text))
          this.fail(
            "syntax-error",
            "a bang call writes '!' before its type arguments, as in 'fetch!::[T](...)'",
            this.current().span,
          );
        left = { ...left, typeArguments, span: { start, end: close.span.end } };
        continue;
      }
      if (
        this.atText("!") &&
        this.peek(1).text === "[" &&
        (left.kind === "name" || left.kind === "member" || left.kind === "qualified-name")
      )
        this.fail(
          "syntax-error",
          "a bang call's type arguments follow '!::', as in 'fetch!::[T](...)'",
          this.peek(1).span,
        );
      if (
        this.atText("!") &&
        this.peek(1).text === "::" &&
        this.peek(2).text === "[" &&
        (left.kind === "name" || left.kind === "member" || left.kind === "qualified-name")
      ) {
        if (12 < minimumPrecedence) break;
        const start = left.span.start;
        this.advance();
        this.advance();
        this.advance();
        const typeArguments = this.parseTypeArgumentList();
        const close = this.expectText("]");
        if (!this.atText("("))
          this.fail(
            "syntax-error",
            "expected '(' after a bang call's type arguments",
            this.current().span,
          );
        left = this.parseCall(
          { ...left, typeArguments, span: { start, end: close.span.end } },
          true,
        );
        continue;
      }
      if (this.atText("::") && left.kind === "name") {
        if (12 < minimumPrecedence) break;
        this.advance();
        const member = this.expectKind("identifier", "expected an associated function name");
        left = {
          kind: "qualified-name",
          owner: left.name,
          ownerTypeArguments: left.typeArguments,
          name: member.text,
          span: { start: left.span.start, end: member.span.end },
        };
        continue;
      }
      // A module namespace may qualify the type of an associated call or a
      // data expression, as in `text.StringBuilder::new()`: the type is a
      // `qualified_name` (02-grammar.md#primary-expressions).
      const qualified = left.kind === "member" ? qualifiedPath(left) : undefined;
      if (qualified !== undefined && left.kind === "member" && this.atText("::")) {
        if (12 < minimumPrecedence) break;
        this.advance();
        const member = this.expectKind("identifier", "expected an associated function name");
        left = {
          kind: "qualified-name",
          owner: qualified,
          ownerTypeArguments: left.typeArguments,
          name: member.text,
          span: { start: left.span.start, end: member.span.end },
        };
        continue;
      }
      if (this.atText("{") && left.kind === "name") {
        if (12 < minimumPrecedence) break;
        left = this.parseDataExpression(left);
        continue;
      }
      if (this.atText("{") && qualified !== undefined && left.kind === "member") {
        if (12 < minimumPrecedence) break;
        left = this.parseDataExpression({
          kind: "name",
          name: qualified,
          ...(left.typeArguments ? { typeArguments: left.typeArguments } : {}),
          span: left.span,
        });
        continue;
      }
      if (this.atText("(")) {
        if (12 < minimumPrecedence) break;
        left = this.parseCall(left);
        continue;
      }
      if (this.atText("!") && this.peek(1).text === "(") {
        if (12 < minimumPrecedence) break;
        this.advance();
        left = this.parseCall(left, true);
        continue;
      }
      if (this.atText(".")) {
        if (12 < minimumPrecedence) break;
        // A leading-dot line cannot continue a line whose same-line suite is
        // still open (01-lexical-structure.md#physical-and-logical-lines).
        if (this.current().continuation && this.inlineSuiteDepths.includes(0))
          this.fail(
            "syntax-error",
            "a leading-dot line cannot continue a line whose same-line suite is still open",
            this.current().span,
          );
        this.advance();
        const member = this.current();
        // A prefix is a bare name, never a member (02-grammar.md#r-grammar.primary.prefix-after-dot).
        if (member.prefix)
          this.fail(
            "qualified-string-prefix",
            `a string prefix is a bare name in scope: write '${member.prefix.name}"..."' without the qualifier`,
            member.span,
          );
        if (member.kind !== "identifier") {
          // Tuple members are identifiers such as `_0` (owner decision TUP-1).
          const numeric = /^[0-9]/.test(member.text);
          this.fail(
            "syntax-error",
            numeric
              ? `expected a member name after '.'; a tuple element is written '._${member.text.split(".")[0]}'`
              : "expected a member name after '.'",
            member.span,
          );
        }
        this.advance();
        left = {
          kind: "member",
          receiver: left,
          name: member.text,
          span: { start: left.span.start, end: member.span.end },
        };
        continue;
      }
      if (this.atText("[")) {
        if (12 < minimumPrecedence) break;
        this.advance();
        const index = this.parseExpression();
        const close = this.expectText("]");
        // `Box[i32] { ... }` and `Add[i32]::add(...)` index a name; a type
        // name's arguments need `::` (02-grammar.md#r-grammar.expr.type-arguments.unmarked).
        if (
          left.kind === "name" &&
          ((this.atText("{") && this.onPreviousLine()) || this.atText("::"))
        )
          this.fail(
            "syntax-error",
            `a type name's arguments in an expression follow '::', as in '${left.name}::[...]'`,
            this.current().span,
          );
        left = {
          kind: "index",
          receiver: left,
          index,
          span: { start: left.span.start, end: close.span.end },
        };
        continue;
      }
      if (this.atText("?")) {
        if (12 < minimumPrecedence) break;
        const suffix = this.advance();
        left = {
          kind: "propagate",
          operand: left,
          span: { start: left.span.start, end: suffix.span.end },
        };
        continue;
      }
      if (this.atText("|>")) {
        if (PIPE_PRECEDENCE < minimumPrecedence) break;
        left = this.parsePipe(left);
        continue;
      }
      if (this.atRangeOperator()) {
        if (RANGE_PRECEDENCE < minimumPrecedence) break;
        return this.parseRange(left);
      }
      const precedence = BINARY_PRECEDENCE[this.current().text];
      if (precedence === undefined || precedence < minimumPrecedence) break;
      const comparisons = new Set(["==", "!=", "<", "<=", ">", ">=", "is"]);
      if (
        comparisons.has(this.current().text) &&
        left.kind === "binary" &&
        comparisons.has(left.operator)
      ) {
        this.fail("comparison-chaining", "comparisons do not chain", this.current().span);
      }
      const operator = this.advance();
      // `**=` lexes as `**` and `=`, which no grammar rule accepts
      // (spec/lang/01-lexical-structure.md#r-lex.op.no-power-assign).
      if (operator.text === "**" && this.atText("="))
        this.fail("syntax-error", "there is no '**=' compound assignment", this.current().span);
      // An ordering comparison's right operand still takes a pipe.
      const right = this.parseExpression(
        operator.text === "**" ? precedence : precedence === 4 ? PIPE_PRECEDENCE : precedence + 1,
      );
      left = {
        kind: "binary",
        operator: LOGICAL_OPERATOR_NAMES[operator.text] ?? operator.text,
        left,
        right,
        span: { start: left.span.start, end: right.span.end },
      };
    }
    return left;
  }

  /** `value |> step`, at the `|>` (05-expressions.md#pipe-expressions). */
  private parsePipe(value: Expression): Expression {
    const operator = this.current();
    // A leading-`|>` line cannot continue a line whose same-line suite is
    // still open (01-lexical-structure.md#r-lex.pipe.open-suite).
    if (operator.continuation && this.inlineSuiteDepths.includes(0))
      this.fail(
        "syntax-error",
        "a leading-'|>' line cannot continue a line whose same-line suite is still open",
        operator.span,
      );
    this.advance();
    const first = this.index;
    const step = this.parseExpression(PIPE_PRECEDENCE + 0.5);
    if (this.tokens.slice(first, this.index).some((token) => token.kind === "indent"))
      this.fail(
        "multi-line-pipe-step",
        "a pipe step must fit on one line; bind a name first or extract a function",
        step.span,
      );
    const placeholders: Expression[] = [];
    collectPipePlaceholders(step, false, placeholders, (placeholder) =>
      this.fail(
        "pipe-placeholder-in-closure",
        "'_' inside a closure does not name the piped value; use the closure's own parameter",
        placeholder.span,
      ),
    );
    if (placeholders.length > 1)
      this.fail(
        "duplicate-pipe-placeholder",
        "a pipe step takes exactly one '_'",
        placeholders[1]!.span,
      );
    const bare = placeholders.length === 0;
    if (bare && !isBarePipeStep(step))
      this.fail(
        "pipe-step-needs-placeholder",
        "a pipe step other than a bare name or path needs '_' to mark the piped value, as in 'f(_, y)'",
        step.span,
      );
    return {
      kind: "pipe",
      value,
      step,
      bare,
      span: { start: value.span.start, end: step.span.end },
    };
  }

  private parseTypeArgumentList(): TypeRef[] {
    const typeArguments: TypeRef[] = [];
    if (!this.atText("]")) {
      do typeArguments.push(this.parseTypeArgument());
      while (this.matchText(",") && !this.atText("]"));
    }
    return typeArguments;
  }

  // A suffixed literal `Nx` is the call `x(N)` of the suffix function `x`
  // (05-expressions.md#r-expr.suffix.fn-call).
  private suffixedLiteral(token: Token, suffix: NonNullable<Token["suffix"]>): Expression {
    const numberSpan = { start: token.span.start, end: suffix.span.start };
    const literal: Expression =
      token.kind === "integer"
        ? { kind: "integer", value: token.value as bigint, span: numberSpan }
        : { kind: "float", value: token.value as number, span: numberSpan };
    return {
      kind: "call",
      callee: { kind: "name", name: suffix.name, span: token.span },
      arguments: [literal],
      literalSuffix: suffix.name,
      span: token.span,
    };
  }

  private interpolatedExpression(source: string, span: SourceSpan): Expression {
    const parsed = this.parseExpressionSource(source);
    const diagnostic = parsed.diagnostics[0];
    if (diagnostic || !parsed.expression) {
      this.fail(
        diagnostic?.code ?? "syntax-error",
        diagnostic?.message ?? "invalid interpolation expression",
        span,
      );
    }
    return parsed.expression;
  }

  // A prefixed string `x"a $b c"` is the call `x(t)` of the prefix function
  // `x`, where `t` is a `std.ops.Template` of the raw pieces `["a ", " c"]`
  // and the values `[b]` (05-expressions.md#r-expr.prefix.fn-call). Each value
  // keeps its segment's span, so a conversion error names its line.
  private prefixedString(token: Token, prefix: NonNullable<Token["prefix"]>): Expression {
    const value = token.value as InterpolatedStringValue;
    const pieces: Expression[] = [];
    const values: Expression[] = [];
    let text = "";
    for (const segment of value.segments) {
      if (segment.kind === "text") {
        text += segment.value;
        continue;
      }
      pieces.push({ kind: "string", value: text, span: token.span });
      text = "";
      values.push(
        withSpan(this.interpolatedExpression(segment.source, segment.span), segment.span),
      );
    }
    pieces.push({ kind: "string", value: text, span: token.span });
    const span = token.span;
    const template: Expression = {
      kind: "data",
      name: TEMPLATE_PLACEHOLDER,
      fields: [
        { name: "raw_parts", value: { kind: "list", elements: pieces, span }, span },
        { name: "values", value: { kind: "list", elements: values, span }, span },
      ],
      span,
    };
    return {
      kind: "call",
      callee: { kind: "name", name: prefix.name, span: prefix.span },
      arguments: [template],
      stringPrefix: prefix.name,
      span,
    };
  }

  protected parsePrefix(): Expression {
    const token = this.current();
    if (["+", "-", "~", "!"].includes(token.text)) {
      this.advance();
      // `-5s` is the ordinary negation `-(s(5))`, so it needs `Neg` on the
      // suffix function's result (05-expressions.md#r-expr.op.desugar).
      const operand = this.parseExpression(11);
      return {
        kind: "unary",
        operator: LOGICAL_OPERATOR_NAMES[token.text] ?? token.text,
        operand,
        span: { start: token.span.start, end: operand.span.end },
      };
    }
    if (this.matchText("if")) {
      // A same-line `if` cannot sit directly in another same-line suite
      // (02-grammar.md#statements).
      if (this.inlineSuiteDepths.at(-1) === this.delimiterDepth(this.index - 1))
        this.fail(
          "syntax-error",
          "a same-line if cannot sit directly in another same-line suite; parenthesize it",
          token.span,
        );
      return this.parseIf(token);
    }
    if (this.matchText("for")) return this.parseFor(token);
    if (this.matchText("while")) return this.parseWhile(token);
    if (this.matchText("match")) return this.parseMatch(token);
    if (this.matchText("fn")) return this.parseClosure(token);
    if (this.atText("mut") && this.peek(1).text === "fn")
      // `mut fn` closures were removed: every closure may mutate its captures
      // (07-functions.md#r-fn.type.no-mut.syntax).
      this.fail("syntax-error", "'mut' cannot precede 'fn'; closures need no 'mut'", token.span);
    if (this.matchText("$")) return this.parseProviderExpression(token);
    if (this.matchText(".")) {
      const variant = this.expectKind("identifier", "expected an enum variant name after '.'");
      return {
        kind: "contextual-variant",
        name: variant.text,
        span: { start: token.span.start, end: variant.span.end },
      };
    }
    if ((token.kind === "integer" || token.kind === "float") && token.suffix) {
      this.advance();
      return this.suffixedLiteral(token, token.suffix);
    }
    if (token.kind === "integer") {
      this.advance();
      return { kind: "integer", value: token.value as bigint, span: token.span };
    }
    if (token.kind === "float") {
      this.advance();
      return { kind: "float", value: token.value as number, span: token.span };
    }
    if (token.kind === "string" && token.prefix) {
      this.advance();
      return this.prefixedString(token, token.prefix);
    }
    if (token.kind === "string") {
      this.advance();
      if (typeof token.value === "string")
        return { kind: "string", value: token.value, span: token.span };
      const value = token.value as InterpolatedStringValue;
      const segments = value.segments.map((segment) => {
        if (segment.kind === "text") return segment;
        // The expression keeps its segment's span, so a diagnostic names its line.
        const expression = withSpan(
          this.interpolatedExpression(segment.source, segment.span),
          segment.span,
        );
        return { kind: "expression" as const, expression, span: segment.span };
      });
      return { kind: "interpolated-string", segments, span: token.span };
    }
    if (token.kind === "character") {
      this.advance();
      return { kind: "character", value: token.value as string, span: token.span };
    }
    if (token.kind === "keyword" && (token.text === "true" || token.text === "false")) {
      this.advance();
      return { kind: "boolean", value: token.text === "true", span: token.span };
    }
    if (this.matchText("[")) {
      if (this.atText("for") && !this.atForExpressionElement())
        return this.parseListComprehension(token);
      const elements: Expression[] = [];
      const spreads: boolean[] = [];
      if (!this.atText("]")) {
        do {
          if (this.atText("..."))
            this.fail(
              "syntax-error",
              "a list spread is written with a suffix '...', as in '[xs...]'",
              this.current().span,
            );
          // `[a, b := v]` holds `a` and the binding `b := v`
          // (02-grammar.md#r-grammar.expr.multi-binding.element).
          const element = this.parseExpression();
          this.rejectNameListBinding(element);
          if (this.atText(":"))
            this.fail(
              "trailing-block-position",
              "a trailing callback block is not valid inside brackets",
              this.current().span,
            );
          elements.push(element);
          // A suffix `...` spreads a list's elements (05-expressions.md#list-and-map-expressions).
          spreads.push(this.matchText("..."));
        } while (this.matchText(",") && !this.atText("]"));
      }
      const close = this.expectText("]");
      return {
        kind: "list",
        elements,
        ...(spreads.some(Boolean) ? { spreads } : {}),
        span: { start: token.span.start, end: close.span.end },
      };
    }
    if (this.matchText("{")) {
      if (this.atText("for")) return this.parseMapComprehension(token);
      const entries: MapEntry[] = [];
      if (!this.atText("}")) {
        do {
          const key = this.parseExpression();
          this.expectText(":");
          const value = this.parseExpression();
          entries.push({ key, value, span: { start: key.span.start, end: value.span.end } });
        } while (this.matchText(",") && !this.atText("}"));
      }
      const close = this.expectText("}");
      return { kind: "map", entries, span: { start: token.span.start, end: close.span.end } };
    }
    // `_` is a pipe step's placeholder; the checker rejects it anywhere else
    // (05-expressions.md#r-expr.pipe.placeholder-outside).
    if (token.text === "_") {
      this.advance();
      return { kind: "name", name: "_", span: token.span };
    }
    if (token.kind === "identifier" || token.text === "self") {
      this.advance();
      const name: NameExpression = { kind: "name", name: token.text, span: token.span };
      if (this.atText("{") && this.onPreviousLine()) return this.parseDataExpression(name);
      return name;
    }
    if (this.matchText("(")) {
      if (this.atText(")")) {
        const close = this.advance();
        return {
          kind: "tuple",
          elements: [],
          span: { start: token.span.start, end: close.span.end },
        };
      }
      const first = this.parseExpression();
      this.rejectNameListBinding(first);
      // The last element of a tuple expression may be a suffix spread
      // `xs...`; alone it keeps the trailing comma (02-grammar.md#r-grammar.primary.tuple-spread).
      const spreadAt = (): boolean => {
        if (!this.atText("...")) return false;
        const ellipsis = this.advance();
        const last =
          (this.atText(",") && this.peek(1).text === ")") || (this.atText(")") && !alone);
        if (!last)
          this.fail(
            "syntax-error",
            "a spread must be the last element of a tuple expression; alone it keeps the trailing comma, as in '(xs...,)'",
            ellipsis.span,
          );
        return true;
      };
      let alone = true;
      let spread = spreadAt();
      if (!spread && !this.matchText(",")) {
        this.expectText(")");
        return first.kind === "member" ? { ...first, parenthesized: true } : first;
      }
      if (spread) this.matchText(",");
      alone = false;
      const elements = [first];
      while (!spread && !this.atText(")")) {
        elements.push(this.parseExpression());
        this.rejectNameListBinding(elements.at(-1)!);
        spread = spreadAt();
        if (!this.matchText(",")) break;
      }
      const close = this.expectText(")");
      return {
        kind: "tuple",
        elements,
        ...(spread ? { spread: true } : {}),
        span: { start: token.span.start, end: close.span.end },
      };
    }
    // A deeper line that opens no suite and is not a leading-dot continuation
    // (01-lexical-structure.md#physical-and-logical-lines) is a syntax error.
    if (token.kind === "indent") this.fail("syntax-error", "unexpected indentation", token.span);
    // `tests` is reserved; a `tests:` block is only a top-level item
    // (spec/lang/02-grammar.md#r-grammar.tests.top-level).
    if (token.text === "tests")
      this.fail(
        "syntax-error",
        "'tests' is reserved: a tests: block may appear only at module top level",
        token.span,
      );
    this.fail("expected-expression", `expected an expression, found '${token.text}'`, token.span);
  }

  /**
   * A binding expression binds one name, so `:=` after any other operand,
   * as in `((a, b) := value)` or `[(a, b) := value]`, is an error; a `let`
   * statement destructures (02-grammar.md#r-grammar.expr.multi-binding.let-only).
   */
  private rejectNameListBinding(operand: Expression): void {
    if (!this.atText(":=")) return;
    const names = operand.kind === "tuple" ? "(a, b)" : "a pattern";
    this.fail(
      "syntax-error",
      `a binding expression binds one name; to destructure, write 'let ${names} = value' as a statement before this expression`,
      { start: operand.span.start, end: this.current().span.end },
    );
  }

  protected parseDataExpression(name: NameExpression): Expression {
    this.expectText("{");
    let spread: Expression | undefined;
    const fields: DataExpressionField[] = [];
    if (this.matchText("...")) {
      spread = this.parseExpression();
      if (!this.atText("}")) this.expectText(",");
    }
    if (!this.atText("}")) {
      do {
        if (this.atText("...")) {
          this.fail(
            "syntax-error",
            "a data spread must be the first and only spread in a data expression",
            this.current().span,
          );
        }
        const field = this.expectKind("identifier", "expected a data field name");
        if (this.atText(",") || this.atText("}")) {
          // Field shorthand: `x` means `x: x` (05-expressions.md#data-expressions).
          fields.push({
            name: field.text,
            value: { kind: "name", name: field.text, span: field.span },
            span: field.span,
          });
          continue;
        }
        this.expectText(":");
        // `Label: ...value` copies the value into an embedded field; the checker
        // decides whether the label names one (02-grammar.md#primary-expressions).
        const copy = this.matchText("...");
        const value = this.parseExpression();
        fields.push({
          name: field.text,
          value,
          ...(copy ? { copy } : {}),
          span: { start: field.span.start, end: value.span.end },
        });
      } while (this.matchText(",") && !this.atText("}"));
    }
    const close = this.expectText("}");
    return {
      kind: "data",
      name: name.name,
      typeArguments: name.typeArguments,
      spread,
      fields,
      span: { start: name.span.start, end: close.span.end },
    };
  }

  protected parseCall(callee: Expression, suspending = false): Expression {
    const typeArguments =
      callee.kind === "name" || callee.kind === "member" || callee.kind === "qualified-name"
        ? callee.typeArguments
        : undefined;
    if (typeArguments && callee.kind === "name")
      callee = { kind: "name", name: callee.name, span: callee.span };
    if (typeArguments && callee.kind === "member")
      callee = {
        kind: "member",
        receiver: callee.receiver,
        name: callee.name,
        span: callee.span,
      };
    if (typeArguments && callee.kind === "qualified-name")
      callee = {
        kind: "qualified-name",
        owner: callee.owner,
        ownerTypeArguments: callee.ownerTypeArguments,
        name: callee.name,
        span: callee.span,
      };
    this.expectText("(");
    const args: Expression[] = [];
    const argumentNames: Array<string | undefined> = [];
    const argumentSpreads: boolean[] = [];
    let sawNamed = false;
    if (!this.atText(")")) {
      do {
        if (this.current().kind === "identifier" && this.peek(1).text === "=") {
          sawNamed = true;
          const name = this.advance();
          this.advance();
          argumentNames.push(name.text);
          args.push(this.parseExpression());
          argumentSpreads.push(false);
        } else {
          if (sawNamed)
            this.fail(
              "argument-order",
              "positional arguments must precede named arguments",
              this.current().span,
            );
          argumentNames.push(undefined);
          args.push(this.parseExpression());
          const spread = this.matchText("...");
          argumentSpreads.push(spread);
          const followedByNamedArgument =
            this.atText(",") && this.peek(1).kind === "identifier" && this.peek(2).text === "=";
          if (spread && this.atText(",") && this.peek(1).text !== ")" && !followedByNamedArgument) {
            this.fail(
              "nonfinal-positional-spread",
              "a positional spread must be the final positional argument",
              this.peek(-1).span,
            );
          }
        }
      } while (this.matchText(",") && !this.atText(")"));
    }
    const close = this.expectText(")");
    return {
      kind: suspending ? "suspend-call" : "call",
      callee,
      typeArguments,
      arguments: args,
      argumentNames: sawNamed ? argumentNames : undefined,
      argumentSpreads: argumentSpreads.some(Boolean) ? argumentSpreads : undefined,
      span: { start: callee.span.start, end: close.span.end },
    };
  }

  protected parseIf(keyword: Token): Expression {
    const condition = this.parseExpression();
    this.rejectHeaderEndingInSuite(keyword, condition);
    const thenBody = this.parseSuite();
    let elseBody: readonly Statement[] = [];
    if (this.matchText("else")) {
      if (this.atText("if")) {
        const nestedKeyword = this.advance();
        const nested = this.parseIf(nestedKeyword);
        elseBody = [{ kind: "expression", expression: nested, span: nested.span }];
      } else {
        elseBody = this.parseSuite();
      }
    }
    return {
      kind: "if",
      condition,
      thenBody,
      elseBody,
      span: {
        start: keyword.span.start,
        end: elseBody.at(-1)?.span.end ?? thenBody.at(-1)!.span.end,
      },
    };
  }

  protected parseWhile(keyword: Token): Expression {
    const condition = this.parseExpression();
    this.rejectHeaderEndingInSuite(keyword, condition);
    const body = this.parseSuite();
    const elseBody = this.matchText("else") ? this.parseSuite() : [];
    return {
      kind: "while",
      condition,
      body,
      elseBody,
      span: { start: keyword.span.start, end: elseBody.at(-1)?.span.end ?? body.at(-1)!.span.end },
    };
  }

  /**
   * The pattern of a `for` loop or a comprehension `for` clause
   * (02-grammar.md#r-grammar.flow.for-pattern). A name, or a tuple of two or
   * more names, comes back as `bindings`; any other pattern as `pattern`. A
   * bare list `for a, b in m` is an error (02-grammar.md#r-grammar.flow.for-list.bare).
   */
  protected parseForPattern(form: "loop" | "comprehension"): {
    readonly bindings: readonly BindingName[];
    readonly pattern?: Pattern;
  } {
    const pattern = this.parsePattern();
    if (form === "loop") this.rejectCommaClosingInlineSuite();
    if (this.atText(",")) {
      let last = pattern;
      while (this.matchText(",")) last = this.parsePattern();
      this.failBareNameList(form === "loop" ? "for" : "comprehension", pattern, last);
    }
    if (pattern.kind === "binding")
      return { bindings: [{ name: pattern.name, span: pattern.span }] };
    if (
      pattern.kind === "tuple" &&
      !pattern.spread &&
      pattern.elements.length >= 2 &&
      pattern.elements.every((element) => element.kind === "binding")
    )
      return {
        bindings: pattern.elements.map((element) => ({
          name: (element as Extract<Pattern, { kind: "binding" }>).name,
          span: element.span,
        })),
      };
    return { bindings: [], pattern };
  }

  protected parseFor(keyword: Token): Expression {
    const { bindings, pattern } = this.parseForPattern("loop");
    this.expectText("in");
    const iterable = this.parseExpression();
    this.rejectHeaderEndingInSuite(keyword, iterable);
    const body = this.parseSuite();
    const elseBody = this.matchText("else") ? this.parseSuite() : [];
    return {
      kind: "for",
      bindings,
      ...(pattern ? { pattern } : {}),
      iterable,
      body,
      elseBody,
      span: { start: keyword.span.start, end: elseBody.at(-1)?.span.end ?? body.at(-1)!.span.end },
    };
  }

  /**
   * At `for` directly after `[`: true when the `for` header ends in `:`, so
   * the list holds a for expression, not a comprehension clause
   * (02-grammar.md#comprehensions).
   */
  private atForExpressionElement(): boolean {
    const depth = this.delimiterDepth(this.index);
    let afterIn = false;
    for (let index = this.index + 1; index < this.tokens.length; index += 1) {
      const token = this.tokens[index]!;
      if (token.kind === "eof" || this.delimiterDepth(index) < depth) return false;
      if (this.delimiterDepth(index) > depth || token.raw) continue;
      if (!afterIn) {
        afterIn = token.text === "in";
        continue;
      }
      if (token.text === ":") return true;
      if (["=>", "for", "if", "]"].includes(token.text)) return false;
    }
    return false;
  }

  private parseListComprehension(open: Token): Expression {
    const clauses = this.parseComprehensionClauses();
    this.expectText("=>");
    const value = this.parseExpression();
    const close = this.expectText("]");
    return {
      kind: "list-comprehension",
      clauses,
      value,
      span: { start: open.span.start, end: close.span.end },
    };
  }

  private parseMapComprehension(open: Token): Expression {
    const clauses = this.parseComprehensionClauses();
    this.expectText("=>");
    const key = this.parseExpression();
    this.expectText(":");
    const value = this.parseExpression();
    const close = this.expectText("}");
    return {
      kind: "map-comprehension",
      clauses,
      key,
      value,
      span: { start: open.span.start, end: close.span.end },
    };
  }

  private parseComprehensionClauses(): ComprehensionClause[] {
    const clauses: ComprehensionClause[] = [];
    while (!this.atText("=>")) {
      const keyword = this.current();
      if (this.matchText("for")) {
        const { bindings, pattern } = this.parseForPattern("comprehension");
        this.expectText("in");
        const iterable = this.parseExpression();
        clauses.push({
          kind: "for",
          bindings,
          ...(pattern ? { pattern } : {}),
          iterable,
          span: { start: keyword.span.start, end: iterable.span.end },
        });
        continue;
      }
      if (this.matchText("if")) {
        const condition = this.parseExpression();
        clauses.push({
          kind: "if",
          condition,
          span: { start: keyword.span.start, end: condition.span.end },
        });
        continue;
      }
      this.fail(
        "expected-comprehension-clause",
        "expected 'for', 'if', or '=>' in a comprehension",
        keyword.span,
      );
    }
    return clauses;
  }

  protected parseMatch(keyword: Token): Expression {
    const subject = this.parseExpression();
    this.rejectHeaderEndingInSuite(keyword, subject);
    const colonIndex = this.index;
    this.expectText(":");
    this.openNestedLayout(colonIndex, false);
    this.expectKind("newline", "expected a line ending after a match header");
    this.expectKind("indent", "expected indented match arms");
    const arms: MatchArm[] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const pattern = this.parsePattern();
      // A variant pattern is `Enum.Variant` or `.Variant`, with no type
      // arguments (06-control-flow.md#r-flow.match.variant.shorthand).
      if (pattern.kind === "binding" && this.atText("["))
        this.fail(
          "syntax-error",
          "expected '=>', found '['; a variant pattern takes no type arguments, as in 'Enum.Variant(...)'",
          this.current().span,
        );
      const guard = this.matchText("if") ? this.parseExpression() : undefined;
      this.expectText("=>");
      let body: readonly Statement[];
      if (this.matchKind("newline")) {
        this.expectKind("indent", "expected an indented match arm body");
        const statements: Statement[] = [];
        while (!this.atKind("dedent") && !this.atKind("eof")) {
          if (this.matchKind("newline")) continue;
          statements.push(this.parseStatement(false));
        }
        this.expectKind("dedent", "expected the end of the match arm body");
        body = statements;
      } else {
        body = [this.parseStatement(true)];
      }
      arms.push({
        pattern,
        guard,
        body,
        span: { start: pattern.span.start, end: body.at(-1)!.span.end },
      });
    }
    const close = this.expectKind("dedent", "expected the end of the match expression");
    if (arms.length === 0)
      this.fail("empty-match", "a match expression must contain an arm", keyword.span);
    return {
      kind: "match",
      subject,
      arms,
      span: { start: keyword.span.start, end: close.span.end },
    };
  }

  protected parseClosure(keyword: Token): Expression {
    const suspending = this.matchText("!");
    this.expectText("(");
    const parameters: ClosureParameter[] = [];
    if (!this.atText(")")) {
      do {
        const name = this.expectKind("identifier", "expected a closure parameter name");
        const type = this.matchText(":") ? this.parseType() : undefined;
        parameters.push({
          name: name.text,
          type,
          span: { start: name.span.start, end: type?.span.end ?? name.span.end },
        });
      } while (this.matchText(",") && !this.atText(")"));
    }
    this.expectText(")");
    const result = this.matchText("->") ? this.parseResultType() : undefined;
    const requirements = this.matchText("$") ? this.parseRequirements() : undefined;
    const body = this.parseClosureBody();
    return {
      kind: "closure",
      ...(suspending ? { suspending: true } : {}),
      parameters,
      result,
      requirements,
      body,
      span: { start: keyword.span.start, end: body.at(-1)!.span.end },
    };
  }

  // A closure body nested in brackets ends only at a line that starts with
  // `,` or a closing delimiter (01-lexical-structure.md#physical-and-logical-lines).
  protected parseClosureBody(): readonly Statement[] {
    return this.parseSuite(true);
  }

  protected parseLocalFunction(): Statement {
    const start = this.expectText("fn").span.start;
    const name = this.expectKind("identifier", "expected a local function name");
    const suspending = this.matchText("!");
    if (this.atText("["))
      this.fail(
        "unsupported-local-generic-function",
        "generic local functions are outside the current erased-closure slice",
        this.current().span,
      );
    this.expectText("(");
    const parameters: ClosureParameter[] = [];
    if (!this.atText(")")) {
      do {
        const parameter = this.expectKind("identifier", "expected a local function parameter name");
        this.expectText(":");
        const type = this.parseType();
        if (this.matchText("..."))
          this.fail(
            "unsupported-local-vararg",
            "variadic local functions are outside the current closure slice",
            this.peek(-1).span,
          );
        if (this.atText("="))
          this.fail(
            "unsupported-local-default",
            "local function parameter defaults are outside the current closure slice",
            this.current().span,
          );
        parameters.push({
          name: parameter.text,
          type,
          span: { start: parameter.span.start, end: type.span.end },
        });
      } while (this.matchText(",") && !this.atText(")"));
    }
    const close = this.expectText(")");
    // A local `fn` may omit its result type; it then lowers to an unannotated
    // closure whose result and row are inferred (07-functions.md#declarations).
    if (!this.atText("->")) {
      if (!this.atText("$") && !this.atText(":")) this.expectText("->");
      const requirements = this.matchText("$") ? this.parseRequirements() : undefined;
      const body = this.parseSuite();
      const end = body.at(-1)?.span.end ?? close.span.end;
      return {
        kind: "binding",
        name: name.text,
        mutable: false,
        localFunction: true,
        value: {
          kind: "closure",
          ...(suspending ? { suspending: true } : {}),
          parameters,
          ...(requirements ? { requirements } : {}),
          body,
          span: { start, end },
        },
        span: { start, end },
      };
    }
    this.expectText("->");
    const result = this.parseResultType();
    // Without a requirement clause the row is inferred, and a `$.with` block
    // around the declaration never satisfies it
    // (11-requirements-and-suspension.md#r-req.row.omitted.inferred-private).
    const requirements = this.matchText("$") ? this.parseRequirements() : undefined;
    const body = this.parseSuite();
    const end = body.at(-1)!.span.end;
    const closure: Expression = {
      kind: "closure",
      ...(suspending ? { suspending: true } : {}),
      parameters,
      result,
      ...(requirements ? { requirements } : {}),
      body,
      span: { start, end },
    };
    return {
      kind: "binding",
      name: name.text,
      mutable: false,
      ...(requirements
        ? {
            annotation: {
              name: `fn${suspending ? "!" : ""}(${parameters.map((parameter) => parameter.type!.name).join(",")})->${result.name}${requirements.length ? `$${requirements.join("+")}` : ""}`,
              span: { start: name.span.start, end: result.span.end },
            },
          }
        : {}),
      value: closure,
      span: { start, end },
    };
  }

  protected parseProviderExpression(namespace: Token): Expression {
    this.expectText(".");
    const operation = this.current();
    if (operation.raw || !new Set(["use", "with", "context"]).has(operation.text)) {
      this.fail("syntax-error", "expected a provider-context operation after '$.'", operation.span);
    }
    this.advance();
    if (operation.text === "use") {
      this.expectText("(");
      const keys = [this.parseRequirementKey()];
      while (this.matchText(",") && !this.atText(")")) keys.push(this.parseRequirementKey());
      const close = this.expectText(")");
      const span = { start: namespace.span.start, end: close.span.end };
      return keys.length === 1
        ? { kind: "provider-use", key: keys[0]!, span }
        : {
            kind: "tuple",
            elements: keys.map((key) => ({ kind: "provider-use", key, span })),
            span,
          };
    }
    if (operation.text === "with") {
      this.expectText("(");
      const entries = this.parseProviderEntries();
      this.expectText(")");
      const body = this.parseSuite();
      return {
        kind: "provider-with",
        entries,
        body,
        span: { start: namespace.span.start, end: body.at(-1)!.span.end },
      };
    }
    if (operation.text === "context") {
      this.expectText("(");
      const entries = this.parseProviderEntries();
      const close = this.expectText(")");
      return {
        kind: "provider-context",
        entries,
        span: { start: namespace.span.start, end: close.span.end },
      };
    }
    this.fail(
      "unsupported-context-operation",
      `provider-context operation '$.${operation.text}' is not implemented yet`,
      operation.span,
    );
  }

  /** Whether the next context entry is `[mut] Key[...] = value` rather than a spread. */
  private atProviderBinding(): boolean {
    let distance = this.peek(0).text === "mut" ? 1 : 0;
    if (this.peek(distance).kind !== "identifier") return false;
    distance += 1;
    if (this.peek(distance).text === "[") {
      let depth = 0;
      for (; this.peek(distance).kind !== "eof"; distance += 1) {
        const text = this.peek(distance).text;
        if (text === "[") depth += 1;
        else if (text === "]" && --depth === 0) break;
      }
      distance += 1;
    }
    return this.peek(distance).text === "=";
  }

  protected parseProviderEntries(): ProviderContextEntry[] {
    const entries: ProviderContextEntry[] = [];
    do {
      // A context spread is a suffix spread `ctx...` (02-grammar.md#primary-expressions);
      // a prefix `...` means copy and is a syntax error here.
      if (this.atText("..."))
        this.fail(
          "syntax-error",
          "a context spread is written with a suffix '...', as in 'ctx...'",
          this.current().span,
        );
      if (!this.atProviderBinding()) {
        const value = this.parseExpression();
        const marker = this.expectText("...");
        entries.push({
          kind: "spread",
          value,
          span: { start: value.span.start, end: marker.span.end },
        });
      } else {
        const keyStart = this.current().span.start;
        const key = this.parseRequirementKey();
        this.expectText("=");
        const value = this.parseExpression();
        entries.push({
          kind: "binding",
          key,
          value,
          span: { start: keyStart, end: value.span.end },
        });
      }
    } while (this.matchText(",") && !this.atText(")"));
    return entries;
  }
}
