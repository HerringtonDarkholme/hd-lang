import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import { speculationSafeArguments } from "./call-speculation.ts";
import { isIntegerType, numericType, type NumericType, widerIntegerName } from "../numeric.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  readonlyType,
  tupleParts,
  tupleRest,
  tupleType,
} from "../types.ts";
import { leastCommonType, rowUnionType } from "./assignability.ts";

import { PatternChecker } from "./patterns.ts";

type ListExpression = Extract<Expression, { kind: "list" }>;

const SPREAD_PART = "__list_spread_part";
const SPREAD_ITEM = "__list_spread_item";

// `[a, xs..., b]` checks as `[for part in [[a], xs, [b]] for item in part => item]`,
// so each spread is evaluated once, in element order
// (05-expressions.md#list-and-map-expressions).
function listSpreadComprehension(expression: ListExpression): Expression {
  const span = expression.span;
  const parts: Expression[] = expression.elements.map((element, index) =>
    expression.spreads?.[index]
      ? element
      : { kind: "list", elements: [element], span: element.span },
  );
  const partList: ListExpression & { readonly spreadOperands: readonly boolean[] } = {
    kind: "list",
    elements: parts,
    spreadOperands: expression.spreads ?? [],
    span,
  };
  return {
    kind: "list-comprehension",
    clauses: [
      { kind: "for", bindings: [{ name: SPREAD_PART, span }], iterable: partList, span },
      {
        kind: "for",
        bindings: [{ name: SPREAD_ITEM, span }],
        iterable: { kind: "name", name: SPREAD_PART, span },
        span,
      },
    ],
    value: { kind: "name", name: SPREAD_ITEM, span },
    span,
  };
}

export abstract class ExpressionLiteralChecker extends PatternChecker {
  /**
   * The least common type of the values checked so far
   * (04-type-system.md#least-common-type); `what` names them in the message.
   */
  protected inferLeastCommonType(
    types: readonly ValueType[],
    what: string,
    span: SourceSpan,
    spreadParts = false,
  ): ValueType {
    const least = leastCommonType(types);
    if ("type" in least) return least.type;
    // Function values in a list or map literal take the union of their rows
    // (11-requirements-and-suspension.md#r-req.row.union.literal).
    const union = rowUnionType(types);
    if (union !== undefined) return union;
    // A spread part is a list, and contributes its elements' rows
    // (r-req.row.union.literal.spread).
    const elements = spreadParts
      ? types.map((type) => {
          const nominal = nominalGenericParts(type);
          return nominal?.name === "List" ? nominal.arguments[0] : undefined;
        })
      : [];
    const elementUnion = elements.every((element) => element !== undefined)
      ? rowUnionType(elements as ValueType[])
      : undefined;
    if (elementUnion !== undefined) return nominalGenericType("List", [elementUnion]);
    const listed = [...new Set(types)].join(", ");
    this.fail(
      least.code,
      least.code === "no-common-type"
        ? `${what} have no common type: ${listed}`
        : `${what} have no unique least common type: ${listed}; add an expected type`,
      span,
    );
  }

  /**
   * A part of a desugared spread literal whose element type took the union
   * of the parts' rows (r-req.row.union.literal.spread). A function value
   * with a smaller row needs its own conversion, so the part is checked again
   * against the union: a spread operand `xs` as `[for x in xs => x]`, and a
   * plain `[e]` as itself. The first check had no lasting effect, because
   * the part is speculation-safe.
   */
  private recheckSpreadPart(part: Expression, spread: boolean, type: ValueType): HirExpression {
    if (!speculationSafeArguments(part))
      this.fail(
        "no-common-type",
        `list elements have no common type with '${type}'; add an expected type`,
        part.span,
      );
    const span = part.span;
    const source: Expression = spread
      ? {
          kind: "list-comprehension",
          clauses: [{ kind: "for", bindings: [{ name: SPREAD_ITEM, span }], iterable: part, span }],
          value: { kind: "name", name: SPREAD_ITEM, span },
          span,
        }
      : part;
    return this.requireCoercion(this.checkExpression(source, type), type, span);
  }

  /**
   * A tuple expression that fills a rest element: against an expected rest
   * tuple it collects its trailing elements into the rest's list, and a final
   * spread `xs...` supplies that list (05-expressions.md#tuple-rest-elements).
   */
  protected checkRestTuple(
    expression: Extract<Expression, { kind: "tuple" }>,
    expectedTuple: { readonly fixed: readonly ValueType[]; readonly rest?: ValueType } | undefined,
  ): HirExpression {
    const rest = expectedTuple?.rest;
    const written = expression.spread ? expression.elements.slice(0, -1) : expression.elements;
    const fixedCount = rest !== undefined ? expectedTuple!.fixed.length : written.length;
    if (
      rest !== undefined &&
      (expression.spread ? written.length !== fixedCount : written.length < fixedCount)
    )
      this.fail(
        "type-mismatch",
        `expected a tuple of type '${tupleType([...expectedTuple!.fixed, `${rest}...`])}' with ${fixedCount} fixed element${fixedCount === 1 ? "" : "s"}, found ${written.length}`,
        expression.span,
      );
    const fixed = written.slice(0, fixedCount).map((element, index) => {
      const expectedElement = rest !== undefined ? expectedTuple!.fixed[index] : undefined;
      const checked = this.checkExpression(element, expectedElement);
      return expectedElement
        ? this.requireCoercion(checked, expectedElement, element.span)
        : checked;
    });
    let list: HirExpression;
    if (expression.spread) {
      const operand = expression.elements.at(-1)!;
      const checked = this.checkExpression(operand, rest);
      if (nominalGenericParts(readonlyType(checked.type))?.name !== "List")
        this.fail(
          "type-mismatch",
          `a tuple spread supplies the rest element and needs a List[T], found '${checked.type}'`,
          operand.span,
        );
      list = rest !== undefined ? this.requireCoercion(checked, rest, operand.span) : checked;
    } else {
      const trailing = written.slice(fixedCount);
      const span = trailing.length > 0 ? trailing[0]!.span : expression.span;
      list = this.requireCoercion(
        this.checkExpression({ kind: "list", elements: trailing, span }, rest),
        rest!,
        span,
      );
    }
    const listType = rest ?? readonlyType(list.type);
    const elementTypes = [...fixed.map((element) => element.type), listType];
    return {
      kind: "tuple",
      elements: [...fixed, list],
      elementTypes,
      type: tupleType([...elementTypes.slice(0, -1), `${listType}...`]),
      span: expression.span,
    };
  }

  /**
   * An integer literal, typed `i64` or `u8` under that expected type (or its
   * optional) and `i32` otherwise, and range-checked against that type
   * (04-type-system.md#integer-literals). `value` is the mathematical value,
   * already negated for a negated literal.
   */
  protected integerLiteral(
    value: bigint,
    expected: ValueType | undefined,
    span: SourceSpan,
  ): HirExpression {
    const target = integerLiteralTarget(expected) ?? "i32";
    const { minimum, maximum, bits } = numericType(target)! as Required<NumericType>;
    const wider = widerIntegerName(target);
    if (value < minimum || value > maximum)
      this.fail(
        "integer-literal-range",
        `integer literal is outside the ${target} range ${minimum}..${maximum}` +
          (wider ? `; declare it ${wider} for a wider range` : ""),
        span,
      );
    return bits === 64
      ? { kind: "integer", value: Number(value), wide: value.toString(), type: target, span }
      : { kind: "integer", value: Number(value), type: target, span };
  }

  protected checkLiteralExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "integer":
        return this.integerLiteral(expression.value, expected, expression.span);
      case "float": {
        // An expected `f32` converts the literal directly (04 Floating-Point Literals).
        const single = floatLiteralTarget(expected) === "f32";
        const value = single ? Math.fround(expression.value) : expression.value;
        if (!Number.isFinite(value))
          this.fail(
            "float-literal-range",
            `floating-point literal is not finite as ${single ? "f32" : "f64"}`,
            expression.span,
          );
        return { ...expression, value, type: single ? "f32" : "f64" };
      }
      case "string":
        return {
          kind: "string",
          bytes: [...new TextEncoder().encode(expression.value)],
          type: "string",
          span: expression.span,
        };
      case "interpolated-string": {
        const segments = expression.segments.map((segment): HirExpression => {
          if (segment.kind === "text") {
            return {
              kind: "string",
              bytes: [...new TextEncoder().encode(segment.value)],
              type: "string",
              span: segment.span,
            };
          }
          const operand = this.checkExpression(segment.expression);
          return this.displayValue(operand, segment.span);
        });
        return { kind: "string-build", segments, type: "string", span: expression.span };
      }
      case "character":
        return {
          kind: "character",
          value: expression.value.codePointAt(0)!,
          type: "char",
          span: expression.span,
        };
      case "boolean":
        return { ...expression, type: "bool" };
      case "list": {
        if (expression.spreads?.some(Boolean))
          return this.checkExpression(listSpreadComprehension(expression), expected);
        const spreadOperands = (expression as { readonly spreadOperands?: readonly boolean[] })
          .spreadOperands;
        const expectedDataType = expected ? readonlyType(expected) : undefined;
        const expectedNominal = expectedDataType
          ? nominalGenericParts(expectedDataType)
          : undefined;
        const contextualElement =
          expectedNominal?.name === "List" && expectedNominal.arguments.length === 1
            ? expectedNominal.arguments[0]
            : undefined;
        if (expression.elements.length === 0 && !contextualElement) {
          this.fail(
            "unresolved-generic-placeholder",
            "an empty list requires an expected list type",
            expression.span,
          );
        }
        const partTypes: ValueType[] = [];
        const checkedElements = expression.elements.map((element, index) => {
          const checked = this.checkExpression(element, contextualElement);
          if (
            spreadOperands?.[index] &&
            nominalGenericParts(readonlyType(checked.type))?.name !== "List"
          )
            this.fail(
              "type-mismatch",
              `a list spread needs a list, found '${checked.type}'`,
              element.span,
            );
          // Spread parts compare as readonly lists: `[0]` is a fresh mutable list.
          partTypes.push(spreadOperands ? readonlyType(checked.type) : checked.type);
          if (contextualElement)
            return this.requireCoercion(checked, contextualElement, element.span);
          this.inferLeastCommonType(
            partTypes,
            "list elements",
            element.span,
            spreadOperands !== undefined,
          );
          return checked;
        });
        const elementType =
          contextualElement ??
          this.inferLeastCommonType(
            partTypes,
            "list elements",
            expression.span,
            spreadOperands !== undefined,
          );
        const elements = contextualElement
          ? checkedElements
          : checkedElements.map((checked, index) =>
              spreadOperands !== undefined && readonlyType(checked.type) !== elementType
                ? this.recheckSpreadPart(
                    expression.elements[index]!,
                    spreadOperands[index] === true,
                    elementType,
                  )
                : this.requireCoercion(checked, elementType, expression.elements[index]!.span),
            );
        const readonlyList = nominalGenericType("List", [elementType]);
        const type =
          (expected && mutableInner(expected) !== undefined) || expected === undefined
            ? mutableType(readonlyList)
            : readonlyList;
        return { kind: "list", elements, elementType, type, span: expression.span };
      }
      case "tuple": {
        const expectedTuple = expected ? tupleRest(readonlyType(expected)) : undefined;
        if (expression.spread || expectedTuple?.rest !== undefined)
          return this.checkRestTuple(expression, expectedTuple);
        const contextual = expected ? tupleParts(expected) : undefined;
        if (contextual && contextual.length !== expression.elements.length) {
          this.fail(
            "tuple-arity",
            `expected a ${contextual.length}-element tuple, found ${expression.elements.length} elements`,
            expression.span,
          );
        }
        const elements = expression.elements.map((element, index) => {
          const expectedElement = contextual?.[index];
          const checked = this.checkExpression(element, expectedElement);
          return expectedElement
            ? this.requireCoercion(checked, expectedElement, element.span)
            : checked;
        });
        const elementTypes = elements.map((element) => element.type);
        return {
          kind: "tuple",
          elements,
          elementTypes,
          type: tupleType(elementTypes),
          span: expression.span,
        };
      }
      case "map": {
        const expectedNominal = expected ? nominalGenericParts(readonlyType(expected)) : undefined;
        const contextualKey =
          expectedNominal?.name === "Map" && expectedNominal.arguments.length === 2
            ? expectedNominal.arguments[0]
            : undefined;
        const contextualValue =
          expectedNominal?.name === "Map" && expectedNominal.arguments.length === 2
            ? expectedNominal.arguments[1]
            : undefined;
        if (expression.entries.length === 0 && (!contextualKey || !contextualValue)) {
          this.fail(
            "unresolved-generic-placeholder",
            "an empty map requires an expected map type",
            expression.span,
          );
        }
        const keyTypes: ValueType[] = [];
        const valueTypes: ValueType[] = [];
        const checkedEntries = expression.entries.map((entry) => {
          let key = this.checkExpression(entry.key, contextualKey);
          keyTypes.push(key.type);
          if (contextualKey) key = this.requireCoercion(key, contextualKey, entry.key.span);
          else this.inferLeastCommonType(keyTypes, "map keys", entry.key.span);
          let value = this.checkExpression(entry.value, contextualValue);
          valueTypes.push(value.type);
          if (contextualValue)
            value = this.requireCoercion(value, contextualValue, entry.value.span);
          else this.inferLeastCommonType(valueTypes, "map values", entry.value.span);
          return { key, value };
        });
        // An inferred key type is readonly, as a `mut` key type is invalid.
        const keyType =
          contextualKey ??
          readonlyType(this.inferLeastCommonType(keyTypes, "map keys", expression.span));
        const valueType =
          contextualValue ?? this.inferLeastCommonType(valueTypes, "map values", expression.span);
        const entries = checkedEntries.map((entry, index) => ({
          key: this.requireCoercion(entry.key, keyType, expression.entries[index]!.key.span),
          value: this.requireCoercion(
            entry.value,
            valueType,
            expression.entries[index]!.value.span,
          ),
        }));
        const { keyKind, keyDispatch } = this.mapKey(keyType, expression.span);
        const readonlyMap = nominalGenericType("Map", [keyType, valueType]);
        const type =
          (expected && mutableInner(expected) !== undefined) || expected === undefined
            ? mutableType(readonlyMap)
            : readonlyMap;
        return {
          kind: "map",
          entries,
          keyType,
          valueType,
          keyKind,
          ...(keyDispatch ? { keyDispatch } : {}),
          type,
          span: expression.span,
        };
      }
      default:
        return undefined;
    }
  }
}

/** The integer type an expected type asks an unsuffixed integer literal to take. */
export function integerLiteralTarget(expected: ValueType | undefined): ValueType | undefined {
  if (!expected) return undefined;
  const type = readonlyType(expected);
  return integerTarget(type) ?? integerTarget(optionalInner(type));
}

/** An integer type other than the default `i32`. */
export function integerTarget(type: ValueType | undefined): ValueType | undefined {
  return type !== "i32" && isIntegerType(type) ? type : undefined;
}

/** The float type an expected type asks a floating-point literal to take. */
export function floatLiteralTarget(expected: ValueType | undefined): ValueType | undefined {
  if (!expected) return undefined;
  const type = readonlyType(expected);
  const inner = optionalInner(type);
  return type === "f32" || inner === "f32" ? "f32" : undefined;
}
