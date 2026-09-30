import {
  TEMPLATE_PLACEHOLDER,
  type ClosureParameter,
  type ComprehensionClause,
  type DataExpressionField,
  type DataPatternField,
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
import { ParserBase } from "./base.ts";

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

// The AST keeps the names "and", "or", and "not" for `&&`, `||`, and prefix `!`.
const LOGICAL_OPERATOR_NAMES: Readonly<Record<string, string>> = {
  "&&": "and",
  "||": "or",
  "!": "not",
};

interface PatternBindings {
  readonly bindings: readonly (string | undefined)[];
  readonly names: readonly (string | undefined)[];
  readonly patterns: readonly Pattern[];
}

type NameExpression = Extract<Expression, { kind: "name" }>;

export abstract class ExpressionParser extends ParserBase {
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
      if (
        this.atText("[") &&
        (left.kind === "name" || left.kind === "member" || left.kind === "qualified-name") &&
        (this.typeArgumentsFollowedBySuffix() || this.bracketHoldsTypeArgumentList())
      ) {
        const start = left.span.start;
        this.advance();
        const typeArguments = this.parseTypeArgumentList();
        const close = this.expectText("]");
        // A bang call writes its type arguments after the `!`
        // (02-grammar.md#primary-expressions).
        if (left.kind === "qualified-name" && this.atText("!"))
          this.fail(
            "syntax-error",
            "a qualified bang call writes its type arguments after '!', as in 'Type::name![T](...)'",
            this.current().span,
          );
        left = { ...left, typeArguments, span: { start, end: close.span.end } };
        if (left.kind === "qualified-name") this.rejectMethodValue(left.span);
        continue;
      }
      if (
        this.atText("!") &&
        this.peek(1).text === "[" &&
        (left.kind === "name" || left.kind === "member" || left.kind === "qualified-name")
      ) {
        if (12 < minimumPrecedence) break;
        const start = left.span.start;
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
        if (!this.atText("[")) this.rejectMethodValue(left.span);
        continue;
      }
      if (this.atText("{") && left.kind === "name") {
        if (12 < minimumPrecedence) break;
        left = this.parseDataExpression(left);
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
      // (spec/01-lexical-structure.md#r-lex.op.no-power-assign).
      if (operator.text === "**" && this.atText("="))
        this.fail("syntax-error", "there is no '**=' compound assignment", this.current().span);
      const right = this.parseExpression(precedence + (operator.text === "**" ? 0 : 1));
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

  private parseTypeArgumentList(): TypeRef[] {
    const typeArguments: TypeRef[] = [];
    if (!this.atText("]")) {
      do typeArguments.push(this.parseTypeArgument());
      while (this.matchText(",") && !this.atText("]"));
    }
    return typeArguments;
  }

  /**
   * At `[`: true when the brackets hold several entries or a `_` placeholder,
   * which only a type-argument list can, as in `pair[i32, string]` or
   * `first[_, bool]` (07-functions.md#function-types-and-values).
   */
  private bracketHoldsTypeArgumentList(): boolean {
    let depth = 0;
    for (let distance = 0; ; distance += 1) {
      const token = this.peek(distance);
      if (token.kind === "eof") return false;
      if (["(", "[", "{"].includes(token.text)) depth += 1;
      else if ([")", "]", "}"].includes(token.text)) {
        depth -= 1;
        if (depth === 0) return false;
      } else if (depth === 1 && token.text === ",") return true;
      else if (
        depth === 1 &&
        token.text === "_" &&
        [",", "["].includes(this.peek(distance - 1).text)
      )
        return true;
    }
  }

  protected typeArgumentsFollowedBySuffix(): boolean {
    let depth = 0;
    for (let distance = 0; ; distance += 1) {
      const token = this.peek(distance);
      if (token.kind === "eof" || token.kind === "newline") return false;
      if (token.text === "[") depth += 1;
      else if (token.text === "]") {
        depth -= 1;
        if (depth === 0) {
          const next = this.peek(distance + 1);
          return (
            next.text === "{" ||
            next.text === "(" ||
            next.text === "::" ||
            (next.text === "!" && this.peek(distance + 2).text === "(")
          );
        }
      }
    }
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
      const operand = this.parseExpression(11);
      // `-5s` negates the literal before the suffix applies: it is
      // `s(-5)` (04-type-system.md#suffixed-literals).
      if (token.text === "-" && operand.kind === "call" && operand.literalSuffix) {
        const literal = operand.arguments[0]!;
        const span = { start: token.span.start, end: operand.span.end };
        return {
          ...operand,
          arguments: [{ kind: "unary", operator: "-", operand: literal, span }],
          span,
        };
      }
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
        const expression = this.interpolatedExpression(segment.source, segment.span);
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
          const multiBinding = this.unparenthesizedMultiBindingOperator();
          if (multiBinding)
            this.fail(
              "multi-binding-needs-parentheses",
              "a multi-name binding inside delimiters must be parenthesized",
              multiBinding.span,
            );
          const element = this.parseExpression();
          if (this.atText(":="))
            this.fail(
              "multi-binding-needs-parentheses",
              "a multi-name binding inside delimiters must be parenthesized",
              this.current().span,
            );
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
    if (token.kind === "identifier" || token.text === "self") {
      if (this.atPackOperation()) this.checkPackOperationArguments();
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
      const groupedBinding = this.parseGroupedBindingExpression(token);
      if (groupedBinding) return groupedBinding;
      const first = this.parseExpression();
      if (!this.matchText(",")) {
        this.expectText(")");
        return first.kind === "member" ? { ...first, parenthesized: true } : first;
      }
      const elements = [first];
      while (!this.atText(")")) {
        elements.push(this.parseExpression());
        if (!this.matchText(",")) break;
      }
      const close = this.expectText(")");
      return { kind: "tuple", elements, span: { start: token.span.start, end: close.span.end } };
    }
    // A deeper line that opens no suite and is not a leading-dot continuation
    // (01-lexical-structure.md#physical-and-logical-lines) is a syntax error.
    if (token.kind === "indent") this.fail("syntax-error", "unexpected indentation", token.span);
    // `tests` is reserved; a `tests:` block is only a top-level item
    // (spec/02-grammar.md#r-grammar.tests.top-level).
    if (token.text === "tests")
      this.fail(
        "syntax-error",
        "'tests' is reserved: a tests: block may appear only at module top level",
        token.span,
      );
    this.fail("expected-expression", `expected an expression, found '${token.text}'`, token.span);
  }

  /** `pack.map(` and `pack.map_list(` always form the pack operation (01-lexical-structure.md). */
  private atPackOperation(): boolean {
    const token = this.current();
    return (
      token.text === "pack" &&
      !token.raw &&
      this.peek(1).text === "." &&
      ["map", "map_list"].includes(this.peek(2).text) &&
      !this.peek(2).raw &&
      this.peek(3).text === "("
    );
  }

  /** The pack operation takes an expression, then a mapper name (02-grammar.md#primary-expressions). */
  private checkPackOperationArguments(): void {
    let distance = 4;
    let depth = 0;
    while (this.peek(distance).kind !== "eof") {
      const text = this.peek(distance).text;
      if (["(", "[", "{"].includes(text)) depth += 1;
      else if ([")", "]", "}"].includes(text)) {
        if (depth === 0) break;
        depth -= 1;
      } else if (text === "," && depth === 0) {
        if (this.peek(distance + 1).kind === "identifier") return;
        break;
      }
      distance += 1;
    }
    this.fail(
      "syntax-error",
      "pack.map( takes a tuple expression and a mapper name",
      this.peek(distance).span,
    );
  }

  private parseGroupedBindingExpression(open: Token): Expression | undefined {
    if (this.current().kind !== "identifier" || this.peek(1).text !== ",") return undefined;
    let distance = 2;
    while (this.peek(distance).kind === "identifier" && this.peek(distance + 1).text === ",")
      distance += 2;
    if (this.peek(distance).kind !== "identifier" || this.peek(distance + 1).text !== ":=")
      return undefined;
    const names = [this.advance()];
    while (this.matchText(","))
      names.push(this.expectKind("identifier", "expected a binding name after ','"));
    this.expectText(":=");
    const value = this.parseExpression();
    const close = this.expectText(")");
    return {
      kind: "binding-expression",
      bindings: names.map((name) => ({ name: name.text, span: name.span })),
      value,
      span: { start: open.span.start, end: close.span.end },
    };
  }

  private unparenthesizedMultiBindingOperator(): Token | undefined {
    if (this.current().kind !== "identifier" || this.peek(1).text !== ",") return undefined;
    let distance = 2;
    while (this.peek(distance).kind === "identifier") {
      if (this.peek(distance + 1).text === ":=") return this.peek(distance + 1);
      if (this.peek(distance + 1).text !== ",") return undefined;
      distance += 2;
    }
    return undefined;
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

  protected parseFor(keyword: Token): Expression {
    const names = [this.expectKind("identifier", "expected a loop binding name")];
    this.rejectCommaClosingInlineSuite();
    while (this.matchText(","))
      names.push(this.expectKind("identifier", "expected a loop binding name after ','"));
    this.expectText("in");
    const iterable = this.parseExpression();
    this.rejectHeaderEndingInSuite(keyword, iterable);
    const body = this.parseSuite();
    const elseBody = this.matchText("else") ? this.parseSuite() : [];
    return {
      kind: "for",
      bindings: names.map((name) => ({ name: name.text, span: name.span })),
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
        const names = [this.expectKind("identifier", "expected a comprehension binding name")];
        while (this.matchText(","))
          names.push(
            this.expectKind("identifier", "expected a comprehension binding name after ','"),
          );
        this.expectText("in");
        const iterable = this.parseExpression();
        clauses.push({
          kind: "for",
          bindings: names.map((name) => ({ name: name.text, span: name.span })),
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

  /**
   * `Type::name` and `x::name` without a call are reserved for future method
   * values (the unbound method function and the bound method value).
   */
  private rejectMethodValue(span: SourceSpan): void {
    if (this.atText("(") || (this.atText("!") && ["(", "["].includes(this.peek(1).text))) return;
    this.fail(
      "deferred-method-value",
      "method values are deferred: 'Type::name' and 'x::name' must be called",
      span,
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

  protected parsePattern(): Pattern {
    const start = this.current().span.start;
    if (this.matchText("_"))
      return { kind: "wildcard", span: { start, end: this.peek(-1).span.end } };
    if (this.matchText("true"))
      return { kind: "boolean", value: true, span: { start, end: this.peek(-1).span.end } };
    if (this.matchText("false"))
      return { kind: "boolean", value: false, span: { start, end: this.peek(-1).span.end } };
    const negative = this.matchText("-");
    const literal = this.current();
    // A suffixed literal is a call, not a pattern
    // (02-grammar.md#r-grammar.pattern.no-suffixed-literal).
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
    // (02-grammar.md#r-grammar.pattern.no-prefixed-string).
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
      // `tuple_pattern` needs a comma: `(p,)` or `(p, q)` (02-grammar.md#patterns).
      const elements = [this.parsePattern()];
      this.expectText(",");
      while (!this.atText(")")) {
        elements.push(this.parsePattern());
        if (!this.matchText(",")) break;
      }
      const close = this.expectText(")");
      return { kind: "tuple", elements, span: { start, end: close.span.end } };
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
    if (this.matchText("{")) {
      const fields: DataPatternField[] = [];
      if (!this.atText("}")) {
        do {
          const field = this.expectKind("identifier", "expected a data pattern field");
          if (this.atText("="))
            this.fail(
              "syntax-error",
              "a data pattern labels a field with ':', as in 'Point { x: 0 }'",
              this.current().span,
            );
          const pattern = this.matchText(":")
            ? this.parsePattern()
            : { kind: "binding" as const, name: field.text, span: field.span };
          fields.push({
            name: field.text,
            pattern,
            span: { start: field.span.start, end: pattern.span.end },
          });
        } while (this.matchText(",") && !this.atText("}"));
      }
      const close = this.expectText("}");
      return { kind: "data", typeName: first.text, fields, span: { start, end: close.span.end } };
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
    if (!this.matchText(".")) return { kind: "binding", name: first.text, span: first.span };
    const variant = this.expectKind("identifier", "expected a variant name after '.'");
    const { bindings, names, patterns } = this.parsePatternBindings();
    return {
      kind: "variant",
      enumName: first.text,
      variantName: variant.text,
      bindings,
      bindingNames: names,
      payloadPatterns: patterns,
      span: { start, end: this.peek(-1).span.end },
    };
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
