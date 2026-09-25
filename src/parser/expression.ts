import type {
  ClosureParameter,
  ComprehensionClause,
  DataExpressionField,
  DataPatternField,
  Expression,
  MapEntry,
  MatchArm,
  Pattern,
  ProviderContextEntry,
  Statement,
  TypeRef,
} from "../ast.ts";
import type { InterpolatedStringValue, Token } from "../lexer.ts";
import { ParserBase } from "./base.ts";

const BINARY_PRECEDENCE: Readonly<Record<string, number>> = {
  or: 1,
  and: 2,
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
      if (
        this.atText("[") &&
        (left.kind === "name" || left.kind === "member" || left.kind === "qualified-name") &&
        this.typeArgumentsFollowedBySuffix()
      ) {
        const start = left.span.start;
        this.advance();
        const typeArguments: TypeRef[] = [];
        if (!this.atText("]")) {
          do typeArguments.push(this.parseType());
          while (this.matchText(",") && !this.atText("]"));
        }
        const close = this.expectText("]");
        left = { ...left, typeArguments, span: { start, end: close.span.end } };
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
        this.advance();
        const member = this.current();
        if (
          member.kind !== "identifier" &&
          !(member.kind === "integer" && /^[0-9]+$/.test(member.text))
        ) {
          this.fail("expected-token", "expected a member name after '.'", member.span);
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
      const right = this.parseExpression(precedence + (operator.text === "**" ? 0 : 1));
      left = {
        kind: "binary",
        operator: operator.text,
        left,
        right,
        span: { start: left.span.start, end: right.span.end },
      };
    }
    return left;
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

  protected parsePrefix(): Expression {
    const token = this.current();
    if (["+", "-", "~", "not"].includes(token.text)) {
      this.advance();
      const operand = this.parseExpression(11);
      return {
        kind: "unary",
        operator: token.text,
        operand,
        span: { start: token.span.start, end: operand.span.end },
      };
    }
    if (this.matchText("if")) return this.parseIf(token);
    if (this.matchText("for")) return this.parseFor(token);
    if (this.matchText("while")) return this.parseWhile(token);
    if (this.matchText("match")) return this.parseMatch(token);
    if (this.matchText("fn")) return this.parseClosure(token);
    if (this.matchText("$")) return this.parseProviderExpression(token);
    if (this.matchText(".")) {
      const variant = this.expectKind("identifier", "expected an enum variant name after '.'");
      return {
        kind: "contextual-variant",
        name: variant.text,
        span: { start: token.span.start, end: variant.span.end },
      };
    }
    if (token.kind === "integer") {
      this.advance();
      return { kind: "integer", value: token.value as bigint, span: token.span };
    }
    if (token.kind === "float") {
      this.advance();
      return { kind: "float", value: token.value as number, span: token.span };
    }
    if (token.kind === "string") {
      this.advance();
      if (typeof token.value === "string")
        return { kind: "string", value: token.value, span: token.span };
      const value = token.value as InterpolatedStringValue;
      const segments = value.segments.map((segment) => {
        if (segment.kind === "text") return segment;
        const parsed = this.parseExpressionSource(segment.source);
        const diagnostic = parsed.diagnostics[0];
        if (diagnostic || !parsed.expression) {
          this.fail(
            diagnostic?.code ?? "invalid-string-interpolation",
            diagnostic?.message ?? "invalid interpolation expression",
            segment.span,
          );
        }
        return { kind: "expression" as const, expression: parsed.expression, span: segment.span };
      });
      return { kind: "interpolated-string", segments, span: token.span };
    }
    if (token.kind === "character") {
      this.advance();
      return { kind: "character", value: token.value as string, span: token.span };
    }
    if (token.text === "true" || token.text === "false") {
      this.advance();
      return { kind: "boolean", value: token.text === "true", span: token.span };
    }
    if (token.text === "nil") {
      this.advance();
      return { kind: "nil", span: token.span };
    }
    if (this.matchText("[")) {
      if (this.atText("for")) return this.parseListComprehension(token);
      const elements: Expression[] = [];
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
        } while (this.matchText(",") && !this.atText("]"));
      }
      const close = this.expectText("]");
      return { kind: "list", elements, span: { start: token.span.start, end: close.span.end } };
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
      this.advance();
      const name: NameExpression = { kind: "name", name: token.text, span: token.span };
      if (this.atText("{")) return this.parseDataExpression(name);
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
        return first;
      }
      const elements = [first];
      while (!this.atText(")")) {
        elements.push(this.parseExpression());
        if (!this.matchText(",")) break;
      }
      const close = this.expectText(")");
      return { kind: "tuple", elements, span: { start: token.span.start, end: close.span.end } };
    }
    this.fail("expected-expression", `expected an expression, found '${token.text}'`, token.span);
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
            "data-spread-position",
            "a data spread must be the first and only spread in a data expression",
            this.current().span,
          );
        }
        const field = this.expectKind("identifier", "expected a data field name");
        this.expectText(":");
        const value = this.parseExpression();
        fields.push({
          name: field.text,
          value,
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
    while (this.matchText(","))
      names.push(this.expectKind("identifier", "expected a loop binding name after ','"));
    this.expectText("in");
    const iterable = this.parseExpression();
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
    this.expectText(":");
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
    const result = this.matchText("->") ? this.parseType() : undefined;
    const requirements = this.matchText("$") ? this.parseRequirements() : undefined;
    const body = this.parseSuite();
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
    this.expectText(")");
    this.expectText("->");
    const result = this.parseType();
    const requirements = this.matchText("$") ? this.parseRequirements() : [];
    const body = this.parseSuite();
    const end = body.at(-1)!.span.end;
    const closure: Expression = {
      kind: "closure",
      ...(suspending ? { suspending: true } : {}),
      parameters,
      result,
      requirements,
      body,
      span: { start, end },
    };
    return {
      kind: "binding",
      name: name.text,
      mutable: false,
      annotation: {
        name: `fn${suspending ? "!" : ""}(${parameters.map((parameter) => parameter.type!.name).join(",")})->${result.name}${requirements.length ? `$${requirements.join("+")}` : ""}`,
        span: { start: name.span.start, end: result.span.end },
      },
      value: closure,
      span: { start, end },
    };
  }

  protected parseProviderExpression(namespace: Token): Expression {
    this.expectText(".");
    const operation = this.current();
    if (!new Set(["use", "with", "context"]).has(operation.text)) {
      this.fail(
        "expected-token",
        "expected a provider-context operation after '$.'",
        operation.span,
      );
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

  protected parseProviderEntries(): ProviderContextEntry[] {
    const entries: ProviderContextEntry[] = [];
    do {
      if (this.matchText("...")) {
        const start = this.peek(-1).span.start;
        const value = this.parseExpression();
        entries.push({ kind: "spread", value, span: { start, end: value.span.end } });
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
    if (this.matchText("nil")) return { kind: "nil", span: { start, end: this.peek(-1).span.end } };
    const negative = this.matchText("-");
    const literal = this.current();
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
          const pattern = this.matchText("=")
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
    if (this.matchText("?")) {
      return {
        kind: "optional-present",
        name: first.text,
        span: { start, end: this.peek(-1).span.end },
      };
    }
    if ((first.text === "Ok" || first.text === "Err") && this.atText("(")) {
      const { bindings, patterns } = this.parsePatternBindings();
      return {
        kind: "result-variant",
        variantName: first.text,
        bindings,
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
