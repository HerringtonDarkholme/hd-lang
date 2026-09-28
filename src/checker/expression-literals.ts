import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  readonlyType,
  tupleParts,
  tupleType,
} from "../types.ts";
import { leastCommonType } from "./assignability.ts";
import { mapKeyKind } from "./context.ts";

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
  ): ValueType {
    const least = leastCommonType(types);
    if ("type" in least) return least.type;
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
    const ranges: Readonly<Record<string, readonly [bigint, bigint, string?]>> = {
      i64: [-(2n ** 63n), 2n ** 63n - 1n],
      u8: [0n, 255n, "u16"],
      i32: [-2_147_483_648n, 2_147_483_647n, "i64"],
    };
    const [minimum, maximum, wider] = ranges[target]!;
    if (value < minimum || value > maximum)
      this.fail(
        "integer-literal-range",
        `integer literal is outside the ${target} range ${minimum}..${maximum}` +
          (wider ? `; declare it ${wider} for a wider range` : ""),
        span,
      );
    return target === "i64"
      ? { kind: "integer", value: Number(value), wide: value.toString(), type: "i64", span }
      : { kind: "integer", value: Number(value), type: target, span };
  }

  protected checkLiteralExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "integer":
        return this.integerLiteral(expression.value, expected, expression.span);
      case "float":
        if (!Number.isFinite(expression.value))
          this.fail("float-literal-range", "floating-point literal is not finite", expression.span);
        return { ...expression, type: "f64" };
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
          this.inferLeastCommonType(partTypes, "list elements", element.span);
          return checked;
        });
        const elementType =
          contextualElement ??
          this.inferLeastCommonType(partTypes, "list elements", expression.span);
        const elements = contextualElement
          ? checkedElements
          : checkedElements.map((checked, index) =>
              this.requireCoercion(checked, elementType, expression.elements[index]!.span),
            );
        const readonlyList = nominalGenericType("List", [elementType]);
        const type =
          (expected && mutableInner(expected) !== undefined) || expected === undefined
            ? mutableType(readonlyList)
            : readonlyList;
        return { kind: "list", elements, elementType, type, span: expression.span };
      }
      case "tuple": {
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
        const keyType =
          contextualKey ?? this.inferLeastCommonType(keyTypes, "map keys", expression.span);
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
        const keyKind = mapKeyKind(keyType);
        if (keyKind === undefined) {
          this.fail(
            "invalid-map-key",
            `type '${keyType}' does not have the MVP's built-in Eq and Hash support`,
            expression.span,
          );
        }
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
export function integerLiteralTarget(expected: ValueType | undefined): "i64" | "u8" | undefined {
  if (!expected) return undefined;
  const type = readonlyType(expected);
  return integerTarget(type) ?? integerTarget(optionalInner(type));
}

/** `i64` or `u8`: the integer types other than the default `i32`. */
export function integerTarget(type: ValueType | undefined): "i64" | "u8" | undefined {
  return type === "i64" || type === "u8" ? type : undefined;
}
