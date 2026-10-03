import type { BindingName, ComprehensionClause, Expression, Pattern, Statement } from "../ast.ts";
import type {
  HirComprehensionClause,
  HirExpression,
  HirLocal,
  HirStatement,
  ValueType,
} from "../hir.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  typeSourceText,
  tupleParts,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import { patternsExhaustive } from "./exhaustiveness.ts";
import { ExpressionDataChecker } from "./expression-data.ts";
import { iterableInfo } from "./shared.ts";

/**
 * The hidden name that holds each yielded value of a `for` whose pattern is
 * not a name or a tuple of names; the pattern then matches it
 * (06-control-flow.md#r-flow.for.pattern).
 */
export const FOR_PATTERN_ITEM = "__for_pattern_item";

/** The hidden name that holds the value of a `let` pattern (06-control-flow.md#r-flow.let.match). */
const LET_PATTERN_ITEM = "__let_pattern_item";

/** The names a pattern binds, in source order, with a `let` pattern's `mut`. */
function patternNames(pattern: Pattern): BindingName[] {
  switch (pattern.kind) {
    case "binding":
      return [
        {
          name: pattern.name,
          ...(pattern.mutableAccess ? { mutableAccess: true, mutSpan: pattern.mutSpan } : {}),
          span: pattern.span,
        },
      ];
    case "tuple":
      return pattern.elements.flatMap(patternNames);
    case "data":
      return pattern.fields.flatMap((field) => patternNames(field.pattern));
    case "variant":
    case "result-variant":
      return pattern.payloadPatterns
        ? pattern.payloadPatterns.flatMap(patternNames)
        : pattern.bindings.flatMap((name) =>
            name === undefined ? [] : [{ name, span: pattern.span }],
          );
    default:
      return [];
  }
}

interface CheckedIterableInfo {
  readonly iterable: HirExpression;
  readonly iteratorFunctionIndex?: number;
  readonly iteratorKind: "iterator" | "list" | "map" | "trait";
  readonly yieldType: ValueType;
}

export abstract class ExpressionComprehensionChecker extends ExpressionDataChecker {
  protected checkComprehensionExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    if (expression.kind !== "list-comprehension" && expression.kind !== "map-comprehension")
      return undefined;
    // A bang call in a comprehension is valid where it is valid in the loops
    // the comprehension abbreviates (05-expressions.md#r-expr.comp.suspension).
    this.scopes.push(new Map());
    try {
      const clauses = expression.clauses.flatMap((clause) => this.checkClause(clause));
      return expression.kind === "list-comprehension"
        ? this.checkListComprehension(expression, clauses, expected)
        : this.checkMapComprehension(expression, clauses, expected);
    } finally {
      this.scopes.pop();
    }
  }

  private checkListComprehension(
    expression: Extract<Expression, { kind: "list-comprehension" }>,
    clauses: readonly HirComprehensionClause[],
    expected?: ValueType,
  ): HirExpression {
    const expectedNominal = expected ? nominalGenericParts(readonlyType(expected)) : undefined;
    const contextualElement =
      expectedNominal?.name === "List" && expectedNominal.arguments.length === 1
        ? expectedNominal.arguments[0]
        : undefined;
    const checkedValue = this.checkExpression(expression.value, contextualElement);
    const elementType = contextualElement ?? checkedValue.type;
    const value = this.requireCoercion(checkedValue, elementType, expression.value.span);
    const readonlyList = nominalGenericType("List", [elementType]);
    const type =
      (expected && mutableInner(expected) !== undefined) || expected === undefined
        ? mutableType(readonlyList)
        : readonlyList;
    return {
      kind: "list-comprehension",
      clauses,
      value,
      elementType,
      type,
      span: expression.span,
    };
  }

  private checkMapComprehension(
    expression: Extract<Expression, { kind: "map-comprehension" }>,
    clauses: readonly HirComprehensionClause[],
    expected?: ValueType,
  ): HirExpression {
    const expectedNominal = expected ? nominalGenericParts(readonlyType(expected)) : undefined;
    const contextualKey =
      expectedNominal?.name === "Map" && expectedNominal.arguments.length === 2
        ? expectedNominal.arguments[0]
        : undefined;
    const contextualValue =
      expectedNominal?.name === "Map" && expectedNominal.arguments.length === 2
        ? expectedNominal.arguments[1]
        : undefined;
    const checkedKey = this.checkExpression(expression.key, contextualKey);
    // An inferred key type is readonly, as a `mut` key type is invalid.
    const keyType = contextualKey ?? readonlyType(checkedKey.type);
    const key = this.requireCoercion(checkedKey, keyType, expression.key.span);
    const checkedValue = this.checkExpression(expression.value, contextualValue);
    const valueType = contextualValue ?? checkedValue.type;
    const value = this.requireCoercion(checkedValue, valueType, expression.value.span);
    const { keyKind, keyDispatch, keyDictionary } = this.mapKey(keyType, expression.key.span);
    const readonlyMap = nominalGenericType("Map", [keyType, valueType]);
    const type =
      (expected && mutableInner(expected) !== undefined) || expected === undefined
        ? mutableType(readonlyMap)
        : readonlyMap;
    return {
      kind: "map-comprehension",
      clauses,
      key,
      value,
      keyType,
      valueType,
      keyKind,
      ...(keyDispatch ? { keyDispatch } : {}),
      ...(keyDictionary ? { keyDictionary } : {}),
      type,
      span: expression.span,
    };
  }

  /** A `for` pattern must be irrefutable (06-control-flow.md#r-flow.for.pattern.irrefutable). */
  protected requireIrrefutableForPattern(pattern: Pattern, type: ValueType): void {
    if (!patternsExhaustive([pattern], type, { enums: this.enumTypes, data: this.dataTypes }))
      this.fail(
        "refutable-let-pattern",
        `a for pattern must match every value of '${typeSourceText(type)}'; this one may fail`,
        pattern.span,
      );
  }

  /**
   * `let P = value else: E` checks as a hidden item and a match that hands
   * the names of P to an ordinary `let`, so each name follows the `let` and
   * `let mut` rules (06-control-flow.md#r-flow.let.match):
   *
   *     item := value
   *     let (a, mut b) = match item: P => (a, b); _ => E
   *
   * Without an else block P must be irrefutable; with one, P must be
   * refutable and E must diverge. E runs without the names of P
   * (03-names-and-scopes.md#r-names.let-else.not-in-else).
   */
  protected checkPatternBinding(
    statement: Extract<Statement, { kind: "pattern-binding" }>,
  ): HirStatement[] {
    const { pattern, span } = statement;
    const annotation = statement.annotation ? this.resolveType(statement.annotation) : undefined;
    let value = this.checkExpression(statement.value, annotation);
    if (annotation) value = this.requireCoercion(value, annotation, statement.value.span);
    const type = annotation ?? value.type;
    if (type === "void")
      this.fail("void-binding", "a binding cannot store a void value", statement.value.span);
    const exhaustive = patternsExhaustive([pattern], type, {
      enums: this.enumTypes,
      data: this.dataTypes,
    });
    if (!statement.elseBody && !exhaustive)
      this.fail(
        "refutable-let-pattern",
        `this let pattern may not match every value of '${typeSourceText(type)}'; add an else block that leaves the enclosing block`,
        pattern.span,
      );
    // (06-control-flow.md#r-flow.let.else.unreachable)
    if (statement.elseBody && exhaustive)
      this.fail(
        "unreachable-match-arm",
        `this let pattern matches every value of '${typeSourceText(type)}', so its else block could never run; remove it`,
        span,
      );
    const item: HirLocal = {
      name: `${LET_PATTERN_ITEM}_${this.locals.length}`,
      type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span: statement.value.span,
    };
    this.locals.push(item);
    this.currentScope().set(item.name, item);
    const names = patternNames(pattern);
    const result: Statement =
      names.length === 0
        ? { kind: "pass", span: pattern.span }
        : {
            kind: "expression",
            expression:
              names.length === 1
                ? { kind: "name", name: names[0]!.name, span: names[0]!.span }
                : {
                    kind: "tuple",
                    elements: names.map((name) => ({
                      kind: "name",
                      name: name.name,
                      span: name.span,
                    })),
                    span: pattern.span,
                  },
            span: pattern.span,
          };
    // The else arm spans the statement, so its diagnostics point at the `let`.
    const elseSpan = statement.elseBody && span;
    const matched: Expression = {
      kind: "match",
      subject: { kind: "name", name: item.name, span: statement.value.span },
      arms: [
        { pattern, body: [result], span: pattern.span },
        ...(statement.elseBody && elseSpan
          ? [
              {
                pattern: { kind: "wildcard" as const, span: elseSpan },
                body: statement.elseBody,
                letElse: true,
                span: elseSpan,
              },
            ]
          : []),
      ],
      span,
    };
    const output: HirStatement[] = [{ kind: "binding", local: item, value, span }];
    if (names.length === 0)
      output.push(this.checkStatement({ kind: "expression", expression: matched, span }));
    else if (names.length === 1)
      output.push(
        this.checkStatement({
          kind: "binding",
          name: names[0]!.name,
          mutable: true,
          ...(names[0]!.mutableAccess ? { mutableAccess: true, mutSpan: names[0]!.mutSpan } : {}),
          value: matched,
          span,
        }),
      );
    else
      output.push(
        ...this.checkTupleBinding({
          kind: "tuple-binding",
          bindings: names,
          mutable: true,
          value: matched,
          span,
        }),
      );
    return output;
  }

  private checkClause(clause: ComprehensionClause): HirComprehensionClause[] {
    if (clause.kind === "if") {
      const condition = this.checkExpression(clause.condition);
      this.requireType(condition.type, "bool", condition.span);
      return [{ kind: "if", condition, span: clause.span }];
    }
    if (clause.pattern) return this.checkPatternClause(clause, clause.pattern);
    return [this.checkNameClause(clause)];
  }

  /**
   * `for P in xs` checks as `for item in xs for (a, b) in [match item: P => (a, b)]`,
   * where `a` and `b` are the names P binds: the one-element list binds them
   * per item, in clause order (06-control-flow.md#r-flow.for.pattern).
   */
  private checkPatternClause(
    clause: Extract<ComprehensionClause, { kind: "for" }>,
    pattern: Pattern,
  ): HirComprehensionClause[] {
    const span = pattern.span;
    const item = `${FOR_PATTERN_ITEM}_${this.locals.length}`;
    // The pattern binds readonly names, as a `let` pattern does without `mut`.
    const first = this.checkNameClause({ ...clause, bindings: [{ name: item, span }] }, true);
    this.requireIrrefutableForPattern(pattern, first.yieldType);
    const names = patternNames(pattern);
    if (names.length === 0) return [first];
    const value: Expression =
      names.length === 1
        ? { kind: "name", name: names[0]!.name, span }
        : {
            kind: "tuple",
            elements: names.map((name) => ({ kind: "name", name: name.name, span: name.span })),
            span,
          };
    const matched: Expression = {
      kind: "match",
      subject: { kind: "name", name: item, span },
      arms: [{ pattern, body: [{ kind: "expression", expression: value, span }], span }],
      span,
    };
    const second = this.checkNameClause({
      kind: "for",
      bindings: names,
      iterable: { kind: "list", elements: [matched], span },
      span: clause.span,
    });
    return [first, second];
  }

  private checkNameClause(
    clause: Extract<ComprehensionClause, { kind: "for" }>,
    readonlyItem = false,
  ): Extract<HirComprehensionClause, { kind: "for" }> {
    const info = this.checkIterable(clause.iterable);
    const bindingTypes =
      clause.bindings.length === 1
        ? [readonlyItem ? readonlyType(info.yieldType) : info.yieldType]
        : tupleParts(info.yieldType);
    if (!bindingTypes || bindingTypes.length !== clause.bindings.length)
      this.fail(
        "type-mismatch",
        `comprehension binding has ${clause.bindings.length} names but '${typeSourceText(info.yieldType)}' yields ${bindingTypes?.length ?? 1} value${bindingTypes?.length === 1 ? "" : "s"}`,
        clause.span,
      );
    const bindings = clause.bindings.map((binding, index) =>
      this.addComprehensionBinding(binding, bindingTypes[index]!),
    );
    return { kind: "for", ...info, bindings, span: clause.span };
  }

  private checkIterable(expression: Expression): CheckedIterableInfo {
    const value = this.checkExpression(expression);
    const iterable = this.iterableIterCall(value, expression) ?? value;
    const info = iterableInfo(iterable, this.iteratorNextFunction());
    if (info?.iteratorKind === "trait" && mutableInner(iterable.type) === undefined)
      this.fail(
        "mutable-receiver-required",
        "iteration requires mutable access to an Iterator implementation",
        expression.span,
      );
    if (info) return { iterable, ...info };
    this.fail(
      "unsatisfied-trait-bound",
      `type '${typeSourceText(iterable.type)}' does not implement Iterable, required by the comprehension's for clause`,
      expression.span,
    );
  }

  private addComprehensionBinding(binding: BindingName, type: ValueType): HirLocal {
    if (this.currentScope().has(binding.name))
      this.fail(
        "duplicate-binding",
        `comprehension binding '${binding.name}' appears more than once`,
        binding.span,
      );
    if (PRELUDE_NAMES.has(binding.name))
      this.fail(
        "prelude-name-shadow",
        `comprehension binding '${binding.name}' shadows a prelude name`,
        binding.span,
      );
    const local: HirLocal = {
      name: binding.name,
      type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span: binding.span,
    };
    this.locals.push(local);
    this.currentScope().set(binding.name, local);
    return local;
  }
}
