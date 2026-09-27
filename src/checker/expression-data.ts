import type { Expression, Statement } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirEnum, HirExpression, HirStatement, ValueType } from "../hir.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
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
          const declared = declaration.fields.find((candidate) => candidate.name === field.name);
          if (!declared) {
            this.fail(
              "unknown-data-field",
              `type '${declaration.name}' has no field '${field.name}'`,
              field.span,
            );
          }
          if (declared.embedded && !field.copy)
            this.fail(
              "embedded-copy-required",
              `embedded field '${field.name}' receives a copy; write '${field.name}: ...value'`,
              field.span,
            );
          if (!declared.embedded && field.copy)
            this.fail(
              "copy-into-ordinary-field",
              `'...' copies into an embedded field, and '${field.name}' is an ordinary field; write '${field.name}: value'`,
              field.span,
            );
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
          declaration.genericParameters.forEach((parameter, index) => {
            const conflict = inferGenericType(
              `generic:${parameter}`,
              expectedNominal.arguments[index]!,
              substitutions,
            );
            if (conflict) this.fail("type-mismatch", conflict, expression.span);
          });
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
            if (conflict) this.fail("type-mismatch", conflict, expression.spread!.span);
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
          if (field.embedded) {
            // The part is filled with a copy of any view of the value; a `mut`
            // outer literal asks for a mutable source (08 Data Embedding).
            const hint = outerMutableExpected
              ? mutableType(readonlyType(inferredField))
              : expected === undefined
                ? undefined
                : readonlyType(inferredField);
            const checked = this.checkExpression(
              value,
              hint && !containsGenericType(hint) ? hint : undefined,
            );
            const conflict = inferGenericType(
              field.type,
              readonlyType(checked.type),
              substitutions,
            );
            if (conflict) this.fail("type-mismatch", conflict, value.span);
            return checked;
          }
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
            this.fail("type-mismatch", conflict, value.span);
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
          if (declarationField.embedded) return this.embeddedCopy(field, fieldType, field.span);
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
        const directFieldsMutable = mutableDirectFields.every((field) => {
          const explicit = explicitByIndex.get(field.index);
          if (explicit) return explicit.type === substituteGenericType(field.type, substitutions);
          if (expression.spread) return mutableInner(spread!.type) !== undefined;
          return true;
        });
        // Every embedded copy must have mutable access too; a part copied from
        // the spread is read through the spread source's view (VE-A).
        const readonlyCopy = declaration.fields.find((field) => {
          if (!field.embedded) return false;
          const explicit = explicitByIndex.get(field.index);
          if (explicit) return mutableInner(explicit.type) === undefined;
          return (
            mutableInner(spread!.type) === undefined &&
            this.hasMutableEdges(substituteGenericType(field.type, substitutions))
          );
        });
        const canProduceMutable = directFieldsMutable && readonlyCopy === undefined;
        if (outerMutableExpected && !directFieldsMutable) {
          this.fail(
            "mutable-upgrade",
            `construction of '${readonlyResult}' does not retain mutable access for every direct mutable field`,
            expression.span,
          );
        }
        if (outerMutableExpected && readonlyCopy) {
          const explicit = expression.fields.find((field) => field.name === readonlyCopy.name);
          this.fail(
            "mutable-upgrade",
            `the copy for embedded field '${readonlyCopy.name}' is readonly: it is made from a readonly value whose type has a direct 'mut' field, so '${readonlyResult}' cannot be mutable`,
            explicit?.value.span ?? expression.span,
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

  private readonly mutableEdges = new Map<string, boolean>();

  /**
   * VE-A: a data type has mutable edges when it declares a direct `mut U`
   * field or embeds a type that has mutable edges. Generic fields keep their
   * substituted type and never count.
   */
  protected hasMutableEdges(type: ValueType): boolean {
    const readonly = readonlyType(type);
    const name = nominalGenericParts(readonly)?.name ?? readonly;
    const cached = this.mutableEdges.get(name);
    if (cached !== undefined) return cached;
    const declaration = this.dataTypes.get(name);
    // Guards a self-embedding type, which the declaration check rejects.
    this.mutableEdges.set(name, false);
    const result =
      declaration?.fields.some(
        (field) =>
          mutableInner(field.type) !== undefined ||
          (field.embedded === true && this.hasMutableEdges(field.type)),
      ) ?? false;
    this.mutableEdges.set(name, result);
    return result;
  }

  /**
   * The copy of `value` for an embedded field of type `partType`
   * (08 Data Embedding). It has type `mut E` when `value` is `mut E` or `E`
   * has no mutable edges, and readonly `E` otherwise.
   */
  protected embeddedCopy(
    value: HirExpression,
    partType: ValueType,
    span: SourceSpan,
  ): HirExpression {
    const part = readonlyType(partType);
    this.requireAssignable(readonlyType(value.type), part, span);
    const declaration = this.dataTypes.get(nominalGenericParts(part)?.name ?? part)!;
    const mutable = mutableInner(value.type) !== undefined || !this.hasMutableEdges(part);
    return {
      kind: "embedded-copy",
      value,
      dataIndex: declaration.index,
      type: mutable ? mutableType(part) : part,
      span,
    };
  }

  /** Whether `expression` reads an embedded field (`x.Part`). */
  private readsEmbeddedField(expression: HirExpression): boolean {
    if (expression.kind !== "member") return false;
    const declaration = [...this.dataTypes.values()].find(
      (candidate) => candidate.index === expression.dataIndex,
    );
    return declaration?.fields[expression.fieldIndex]?.embedded === true;
  }

  /**
   * `place.field = value` and the copy assignment `place.Part ...= value`.
   * The field may be promoted; it is reached through its embedded fields, each
   * of which follows its container's access (04 Mutable Paths).
   */
  protected checkFieldAssignment(
    statement: Extract<Statement, { kind: "field-assignment" }>,
  ): HirStatement {
    const root = this.checkExpression(statement.target.receiver);
    if (tupleParts(readonlyType(root.type)) !== undefined)
      this.fail(
        "invalid-assignment-target",
        `tuple element '${statement.target.name}' is not assignable; tuples are immutable`,
        statement.target.span,
      );
    const rootReadonly = readonlyType(root.type);
    const selection = this.dataTypes.has(nominalGenericParts(rootReadonly)?.name ?? rootReadonly)
      ? this.selectField(root.type, statement.target.name, statement.target.span)
      : undefined;
    const selected = selection?.kind === "field" ? selection : undefined;
    const receiver = selected
      ? this.memberPath(root, selected.steps, statement.target.receiver.span)
      : root;
    const field = selected?.final.field;
    if (field?.embedded && !statement.copy)
      this.fail(
        "embedded-copy-required",
        `embedded field '${field.name}' stores a copy; write 'place.${field.name} ...= value'`,
        statement.span,
      );
    if (field && !field.embedded && statement.copy)
      this.fail(
        "copy-into-ordinary-field",
        `the copy assignment '...=' stores into an embedded field, and '${field.name}' is an ordinary field; use '='`,
        statement.span,
      );
    const mutableReceiver = mutableInner(receiver.type);
    if (mutableReceiver === undefined) {
      let rootExpression: Expression = statement.target.receiver;
      while (rootExpression.kind === "member") rootExpression = rootExpression.receiver;
      const rootBinding =
        rootExpression.kind === "name"
          ? (this.resolveLocal(rootExpression.name) ?? this.resolveGlobal(rootExpression.name))
          : undefined;
      // An embedded field read through a readonly value is never a readonly edge.
      const code =
        !this.readsEmbeddedField(receiver) &&
        rootBinding &&
        mutableInner(rootBinding.type) !== undefined
          ? "readonly-edge"
          : "readonly-root";
      this.fail(
        code,
        `field '${statement.target.name}' cannot be assigned through readonly type '${receiver.type}'`,
        statement.target.span,
      );
    }
    if (!selection)
      this.fail(
        "member-on-non-data",
        `type '${mutableReceiver}' has no assignable data fields`,
        statement.target.receiver.span,
      );
    if (!selected || !field)
      this.fail(
        "unknown-data-field",
        `type '${rootReadonly}' has no field '${statement.target.name}'`,
        statement.target.span,
      );
    const { declaration, substitutions } = selected.final;
    const fieldType = substituteGenericType(field.type, substitutions);
    let value: HirExpression;
    if (field.embedded) {
      const part = readonlyType(fieldType);
      value = this.embeddedCopy(
        this.checkExpression(statement.value, mutableType(part)),
        part,
        statement.value.span,
      );
      if (mutableInner(value.type) === undefined)
        this.fail(
          "mutable-upgrade",
          `the copy stored into embedded field '${field.name}' is readonly: it is made from a readonly value whose type has a direct 'mut' field`,
          statement.value.span,
        );
    } else {
      value = this.requireCoercion(
        this.checkExpression(statement.value, fieldType),
        fieldType,
        statement.value.span,
      );
    }
    const expression: HirExpression = {
      kind: "field-set",
      receiver,
      value,
      dataIndex: declaration.index,
      fieldIndex: field.index,
      erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
      type: "void",
      span: statement.span,
    };
    return { kind: "expression", expression, span: statement.span };
  }

  /**
   * Checks `Enum.Variant` for a single-payload variant as the function value
   * `fn(payload: P) -> Enum: Enum.Variant(payload)`. A generic enum takes its
   * types from the expected function type.
   */
  private checkVariantFunctionValue(
    enumType: HirEnum,
    payloadType: ValueType,
    expression: Extract<Expression, { kind: "member" }>,
    expected?: ValueType,
  ): HirExpression {
    const span = expression.span;
    const generic = enumType.genericParameters.length > 0 || containsGenericType(payloadType);
    return this.checkExpression(
      {
        kind: "closure",
        parameters: [
          { name: "$payload", type: generic ? undefined : { name: payloadType, span }, span },
        ],
        result: generic ? undefined : { name: enumType.name, span },
        body: [
          {
            kind: "expression",
            expression: {
              kind: "call",
              callee: expression,
              arguments: [{ kind: "name", name: "$payload", span }],
              span,
            },
            span,
          },
        ],
        span,
      },
      expected,
    );
  }

  protected checkAccessExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "member": {
        if (expression.receiver.kind === "name" && this.namesOptionEnum(expression.receiver.name))
          return this.checkOptionVariant(
            expression.name,
            undefined,
            expected,
            expression.span,
            true,
          );
        if (expression.receiver.kind === "name") {
          const enumType = this.enumTypes.get(expression.receiver.name);
          if (enumType) {
            const variant = enumType.variants.find(
              (candidate) => candidate.name === expression.name,
            );
            // 08 Enum Declarations: a constructor with exactly one payload
            // field is a function value; with two or more it must be called.
            if (variant && variant.fields.length === 1)
              return this.checkVariantFunctionValue(
                enumType,
                variant.fields[0]!.type,
                expression,
                expected,
              );
            if (variant && variant.fields.length > 1) {
              this.fail(
                "unsaturated-enum-constructor",
                `variant '${variant.name}' requires ${variant.fields.length} arguments; only a single-payload constructor is a function value`,
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
              "unknown-method",
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
          const selection = this.selectField(receiver.type, expression.name, expression.span);
          if (selection.kind !== "field")
            this.fail(
              "unknown-data-field",
              `type '${dataDeclaration.name}' has no field '${expression.name}'`,
              expression.span,
            );
          const { declaration, field, substitutions } = selection.final;
          return this.dataMember(
            this.memberPath(receiver, selection.steps, expression.span),
            declaration,
            field,
            substitutions,
            expression.span,
          );
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
        if (expected && optionalInner(expected) !== undefined)
          return this.checkOptionVariant(
            expression.name,
            undefined,
            expected,
            expression.span,
            false,
          );
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
        if (nominal?.name === "List" && nominal.arguments.length === 1) {
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
        if (nominal?.name === "Map" && nominal.arguments.length === 2) {
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
