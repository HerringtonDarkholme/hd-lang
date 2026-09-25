import type { Expression } from "../ast.ts";
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
import { mapKeyKind } from "./context.ts";

import { PatternChecker } from "./patterns.ts";
export abstract class ExpressionLiteralChecker extends PatternChecker {
  protected checkLiteralExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "integer": {
        if (expression.value < -2_147_483_648n || expression.value > 2_147_483_647n) {
          this.fail(
            "integer-literal-range",
            "integer literal is outside the i32 range",
            expression.span,
          );
        }
        return {
          kind: "integer",
          value: Number(expression.value),
          type: "i32",
          span: expression.span,
        };
      }
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
      case "nil": {
        if (!expected || optionalInner(expected) === undefined) {
          this.fail(
            "nil-needs-optional-type",
            "nil requires an expected optional type",
            expression.span,
          );
        }
        return {
          kind: "variant-wrap",
          variant: "optional-absent",
          type: expected,
          span: expression.span,
        };
      }
      case "list": {
        const expectedDataType = expected ? readonlyType(expected) : undefined;
        const expectedNominal = expectedDataType
          ? nominalGenericParts(expectedDataType)
          : undefined;
        const contextualElement =
          expectedNominal?.name === "list" && expectedNominal.arguments.length === 1
            ? expectedNominal.arguments[0]
            : undefined;
        if (expression.elements.length === 0 && !contextualElement) {
          this.fail(
            "empty-list-needs-context",
            "an empty list requires an expected list type",
            expression.span,
          );
        }
        let elementType = contextualElement;
        const elements = expression.elements.map((element) => {
          const checked = this.checkExpression(element, contextualElement);
          if (!elementType) elementType = checked.type;
          if (!contextualElement && checked.type !== elementType) {
            this.fail(
              "no-common-type",
              `list elements have no common type: ${elementType} and ${checked.type}`,
              element.span,
            );
          }
          return this.requireCoercion(checked, elementType!, element.span);
        });
        const readonlyList = nominalGenericType("list", [elementType!]);
        const type =
          (expected && mutableInner(expected) !== undefined) || expected === undefined
            ? mutableType(readonlyList)
            : readonlyList;
        return { kind: "list", elements, elementType: elementType!, type, span: expression.span };
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
          expectedNominal?.name === "map" && expectedNominal.arguments.length === 2
            ? expectedNominal.arguments[0]
            : undefined;
        const contextualValue =
          expectedNominal?.name === "map" && expectedNominal.arguments.length === 2
            ? expectedNominal.arguments[1]
            : undefined;
        if (expression.entries.length === 0 && (!contextualKey || !contextualValue)) {
          this.fail(
            "empty-map-needs-context",
            "an empty map requires an expected map type",
            expression.span,
          );
        }
        let keyType = contextualKey;
        let valueType = contextualValue;
        const entries = expression.entries.map((entry) => {
          const checkedKey = this.checkExpression(entry.key, keyType);
          if (!keyType) keyType = checkedKey.type;
          const key = this.requireCoercion(checkedKey, keyType!, entry.key.span);
          const checkedValue = this.checkExpression(entry.value, valueType);
          if (!valueType) valueType = checkedValue.type;
          const value = this.requireCoercion(checkedValue, valueType!, entry.value.span);
          return { key, value };
        });
        const keyKind = mapKeyKind(keyType!);
        if (keyKind === undefined) {
          this.fail(
            "unsupported-map-key",
            `type '${keyType}' does not have the MVP's built-in Eq and Hash support`,
            expression.span,
          );
        }
        const readonlyMap = nominalGenericType("map", [keyType!, valueType!]);
        const type =
          (expected && mutableInner(expected) !== undefined) || expected === undefined
            ? mutableType(readonlyMap)
            : readonlyMap;
        return {
          kind: "map",
          entries,
          keyType: keyType!,
          valueType: valueType!,
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
