import type { Expression } from "../ast.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  tupleParts,
} from "../types.ts";
import {
  containsGenericType,
  genericTypeName,
  inferGenericType,
  substituteGenericType,
} from "./shared.ts";

import { ExpressionSuspensionChecker } from "./expression-suspensions.ts";
export abstract class ExpressionDataChecker extends ExpressionSuspensionChecker {
  protected checkDataExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "data": {
        const declaration = this.dataTypes.get(expression.name);
        if (!declaration)
          this.fail("unknown-type", `unknown data type '${expression.name}'`, expression.span);
        const outerMutableExpected = expected !== undefined && mutableInner(expected) !== undefined;
        const supplied = new Map<string, Expression>();
        for (const field of expression.fields) {
          if (supplied.has(field.name))
            this.fail(
              "duplicate-field",
              `field '${field.name}' is supplied more than once`,
              field.span,
            );
          if (!declaration.fields.some((candidate) => candidate.name === field.name)) {
            this.fail(
              "unknown-data-field",
              `type '${declaration.name}' has no field '${field.name}'`,
              field.span,
            );
          }
          supplied.set(field.name, field.value);
        }
        const substitutions = new Map<string, ValueType>();
        if (expression.typeArguments) {
          if (expression.typeArguments.length !== declaration.genericParameters.length) {
            const code =
              expression.typeArguments.length < declaration.genericParameters.length
                ? "partial-generic-arguments"
                : "generic-argument-count";
            this.fail(
              code,
              `data type '${declaration.name}' expects ${declaration.genericParameters.length} type arguments, received ${expression.typeArguments.length}`,
              expression.span,
            );
          }
          expression.typeArguments.forEach((argument, index) => {
            substitutions.set(declaration.genericParameters[index]!, this.resolveType(argument));
          });
        }
        const expectedNominal = expected ? nominalGenericParts(expected) : undefined;
        if (
          expectedNominal?.name === declaration.name &&
          expectedNominal.arguments.length === declaration.genericParameters.length
        ) {
          const conflict = inferGenericType(
            nominalGenericType(declaration.name, declaration.genericParameters),
            expectedNominal.name === declaration.name ? expected : declaration.name,
            substitutions,
          );
          if (conflict) this.fail("generic-type-mismatch", conflict, expression.span);
        }
        const spread = expression.spread ? this.checkExpression(expression.spread) : undefined;
        if (spread) {
          if (declaration.genericParameters.length === 0) {
            this.requireAssignable(spread.type, declaration.name, expression.spread!.span);
          } else {
            const spreadNominal = nominalGenericParts(spread.type);
            if (
              spreadNominal?.name !== declaration.name ||
              spreadNominal.arguments.length !== declaration.genericParameters.length
            ) {
              this.fail(
                "type-mismatch",
                `copy-update for '${declaration.name}' requires the same data type, found ${spread.type}`,
                expression.spread!.span,
              );
            }
            const conflict = inferGenericType(
              nominalGenericType(declaration.name, declaration.genericParameters),
              spread.type,
              substitutions,
            );
            if (conflict) this.fail("generic-type-mismatch", conflict, expression.spread!.span);
          }
        }
        const missingFields = declaration.fields.filter((field) => !supplied.has(field.name));
        const missingRequired = expression.spread
          ? undefined
          : missingFields.find((field) => !field.defaultFunctionName);
        if (missingRequired)
          this.fail(
            "missing-required-field",
            `missing required field '${missingRequired.name}'`,
            expression.span,
          );
        const initiallyChecked = expression.fields.map((entry) => {
          const field = declaration.fields.find((candidate) => candidate.name === entry.name)!;
          const value = entry.value;
          const inferredField = substituteGenericType(field.type, substitutions);
          const directMutable = mutableInner(field.type) !== undefined;
          const contextualField =
            directMutable && !outerMutableExpected
              ? expected === undefined
                ? undefined
                : mutableInner(inferredField)
              : inferredField;
          const checked = this.checkExpression(
            value,
            contextualField && !containsGenericType(contextualField) ? contextualField : undefined,
          );
          const conflict = inferGenericType(field.type, checked.type, substitutions);
          if (conflict) {
            if (mutableInner(inferredField) === checked.type) {
              this.fail(
                "mutable-upgrade",
                `readonly type '${checked.type}' cannot initialize generic field '${inferredField}'`,
                value.span,
              );
            }
            this.fail("generic-type-mismatch", conflict, value.span);
          }
          return checked;
        });
        const unresolved = declaration.genericParameters.filter(
          (parameter) => !substitutions.has(parameter),
        );
        if (unresolved.length > 0)
          this.fail(
            "unresolved-generic-placeholder",
            `could not infer data parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`,
            expression.span,
          );
        const explicitFieldIndices = expression.fields.map(
          (entry) => declaration.fields.find((field) => field.name === entry.name)!.index,
        );
        const explicitFields = initiallyChecked.map((field, sourceIndex) => {
          const declarationField = declaration.fields[explicitFieldIndices[sourceIndex]!]!;
          const fieldType = substituteGenericType(declarationField.type, substitutions);
          const directMutable = mutableInner(declarationField.type) !== undefined;
          if (directMutable && !outerMutableExpected && field.type === mutableInner(fieldType))
            return field;
          return this.requireCoercion(field, fieldType, field.span);
        });
        const defaultFields = (expression.spread ? [] : missingFields).map((field) => {
          const expectedField = substituteGenericType(field.type, substitutions);
          const call: Expression = {
            kind: "call",
            callee: { kind: "name", name: field.defaultFunctionName!, span: expression.span },
            arguments: [],
            span: expression.span,
          };
          return this.requireCoercion(
            this.checkExpression(call, expectedField),
            expectedField,
            expression.span,
          );
        });
        const fields = [...explicitFields, ...defaultFields];
        const fieldIndices = [
          ...explicitFieldIndices,
          ...(expression.spread ? [] : missingFields.map((field) => field.index)),
        ];
        const readonlyResult =
          declaration.genericParameters.length > 0
            ? nominalGenericType(
                declaration.name,
                declaration.genericParameters.map((parameter) => substitutions.get(parameter)!),
              )
            : declaration.name;
        const mutableDirectFields = declaration.fields.filter(
          (field) => mutableInner(field.type) !== undefined,
        );
        const explicitByIndex = new Map(
          explicitFieldIndices.map(
            (fieldIndex, sourceIndex) => [fieldIndex, explicitFields[sourceIndex]!] as const,
          ),
        );
        const canProduceMutable = mutableDirectFields.every((field) => {
          const explicit = explicitByIndex.get(field.index);
          if (explicit) return explicit.type === substituteGenericType(field.type, substitutions);
          if (expression.spread) return mutableInner(spread!.type) !== undefined;
          return true;
        });
        if (outerMutableExpected && !canProduceMutable) {
          this.fail(
            "mutable-upgrade",
            `construction of '${readonlyResult}' does not retain mutable access for every direct mutable field`,
            expression.span,
          );
        }
        const type =
          outerMutableExpected || (expected === undefined && canProduceMutable)
            ? mutableType(readonlyResult)
            : readonlyResult;
        return {
          kind: "data",
          dataIndex: declaration.index,
          spread,
          fields,
          fieldIndices,
          erasedFieldTypes:
            declaration.genericParameters.length > 0
              ? declaration.fields.map((field) => field.type)
              : undefined,
          type,
          span: expression.span,
        };
      }
      default:
        return undefined;
    }
  }

  protected checkAccessExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "member": {
        if (expression.receiver.kind === "name") {
          const enumType = this.enumTypes.get(expression.receiver.name);
          if (enumType) {
            const variant = enumType.variants.find(
              (candidate) => candidate.name === expression.name,
            );
            if (variant && variant.fields.length > 0) {
              this.fail(
                "unsaturated-enum-constructor",
                `variant '${variant.name}' requires ${variant.fields.length} argument${variant.fields.length === 1 ? "" : "s"}`,
                expression.span,
              );
            }
            return this.checkEnumConstructor(
              enumType,
              expression.name,
              { kind: "call", callee: expression, arguments: [], span: expression.span },
              expected,
            );
          }
        }
        const receiver = this.checkExpression(expression.receiver);
        const receiverReadonly = readonlyType(receiver.type);
        const tuple = tupleParts(receiverReadonly);
        if (tuple) {
          if (!/^[0-9]+$/.test(expression.name)) {
            this.fail(
              "unknown-tuple-member",
              `tuple type '${receiver.type}' has no member '${expression.name}'`,
              expression.span,
            );
          }
          const index = Number(expression.name);
          if (!Number.isSafeInteger(index) || index >= tuple.length) {
            this.fail(
              "tuple-index-range",
              `tuple index ${expression.name} is outside a ${tuple.length}-element tuple`,
              expression.span,
            );
          }
          return {
            kind: "tuple-index",
            receiver,
            index,
            elementType: tuple[index]!,
            type: tuple[index]!,
            span: expression.span,
          };
        }
        const nominal = nominalGenericParts(receiverReadonly);
        const typeName = nominal?.name ?? receiverReadonly;
        const dataDeclaration = this.dataTypes.get(typeName);
        if (dataDeclaration) {
          const field = dataDeclaration.fields.find(
            (candidate) => candidate.name === expression.name,
          );
          if (!field)
            this.fail(
              "unknown-data-field",
              `type '${dataDeclaration.name}' has no field '${expression.name}'`,
              expression.span,
            );
          const substitutions = new Map<string, ValueType>();
          if (nominal)
            dataDeclaration.genericParameters.forEach((parameter, index) =>
              substitutions.set(parameter, nominal.arguments[index]!),
            );
          const declaredType = substituteGenericType(field.type, substitutions);
          const type =
            mutableInner(receiver.type) !== undefined || genericTypeName(field.type)
              ? declaredType
              : readonlyType(declaredType);
          return {
            kind: "member",
            receiver,
            dataIndex: dataDeclaration.index,
            fieldIndex: field.index,
            erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
            type,
            span: expression.span,
          };
        }
        const enumDeclaration = this.enumTypes.get(typeName);
        if (enumDeclaration) {
          const field = enumDeclaration.sharedFields.find(
            (candidate) => candidate.name === expression.name,
          );
          if (!field)
            this.fail(
              "unknown-data-field",
              `enum '${enumDeclaration.name}' has no shared field '${expression.name}'`,
              expression.span,
            );
          const substitutions = new Map<string, ValueType>();
          if (nominal)
            enumDeclaration.genericParameters.forEach((parameter, index) =>
              substitutions.set(parameter, nominal.arguments[index]!),
            );
          const type = substituteGenericType(field.type, substitutions);
          return {
            kind: "enum-member",
            receiver,
            enumIndex: enumDeclaration.index,
            fieldIndex: field.index,
            erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
            type,
            span: expression.span,
          };
        }
        this.fail(
          "member-on-non-data",
          `type '${receiver.type}' has no data fields`,
          expression.receiver.span,
        );
      }
      case "contextual-variant": {
        const nominal = expected ? nominalGenericParts(expected) : undefined;
        const declaration = expected && this.enumTypes.get(nominal?.name ?? expected);
        if (!declaration) {
          this.fail(
            "missing-contextual-enum-type",
            `variant '.${expression.name}' requires an expected enum type`,
            expression.span,
          );
        }
        const variant = declaration.variants.find(
          (candidate) => candidate.name === expression.name,
        );
        if (variant && variant.fields.length > 0) {
          this.fail(
            "unsaturated-enum-constructor",
            `variant '${variant.name}' requires ${variant.fields.length} argument${variant.fields.length === 1 ? "" : "s"}`,
            expression.span,
          );
        }
        return this.checkEnumConstructor(
          declaration,
          expression.name,
          { kind: "call", callee: expression, arguments: [], span: expression.span },
          expected,
        );
      }
      case "index": {
        const receiver = this.checkExpression(expression.receiver);
        const nominal = nominalGenericParts(readonlyType(receiver.type));
        if (nominal?.name === "list" && nominal.arguments.length === 1) {
          const index = this.checkExpression(expression.index, "i32");
          this.requireAssignable(index.type, "i32", expression.index.span);
          return {
            kind: "list-index",
            receiver,
            index,
            elementType: nominal.arguments[0]!,
            type: nominal.arguments[0]!,
            span: expression.span,
          };
        }
        if (nominal?.name === "map" && nominal.arguments.length === 2) {
          const key = this.checkExpression(expression.index, nominal.arguments[0]);
          this.requireAssignable(key.type, nominal.arguments[0]!, expression.index.span);
          return {
            kind: "map-index",
            receiver,
            key,
            keyType: nominal.arguments[0]!,
            valueType: nominal.arguments[1]!,
            type: `${nominal.arguments[1]}?`,
            span: expression.span,
          };
        }
        if (receiver.type === "string") {
          this.fail(
            "unsupported-string-indexing",
            "strings are not indexable; iterate Unicode scalars explicitly",
            expression.span,
          );
        }
        this.fail(
          "not-indexable",
          `type '${receiver.type}' does not support indexing`,
          expression.receiver.span,
        );
      }
      default:
        return undefined;
    }
  }
}
