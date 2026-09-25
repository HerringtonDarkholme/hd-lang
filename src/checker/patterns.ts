import type { Expression, Pattern } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type {
  HirExpression,
  HirData,
  HirEnum,
  HirLocal,
  HirMatchArm,
  HirMatchBinding,
  HirMatchTest,
  HirPatternAccessStep,
  HirPatternPathStep,
  ValueType,
} from "../hir.ts";
import {
  contextKeys,
  functionParts,
  nominalGenericParts,
  nominalGenericType,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import {
  containsGenericType,
  genericTypeName,
  inferGenericType,
  substituteGenericType,
} from "./shared.ts";

import { CallChecker } from "./calls.ts";
export abstract class PatternChecker extends CallChecker {
  protected isIdentityType(type: ValueType): boolean {
    const generic = genericTypeName(type);
    if (generic) return (this.signature.referenceParameters ?? []).includes(generic);
    if (type.startsWith("trait:")) return true;
    if (
      functionParts(type) ||
      storedSuspensionParts(type) ||
      suspensionParts(type) ||
      traitSuspensionParts(type) ||
      contextKeys(type)
    )
      return true;
    const nominal = nominalGenericParts(type);
    if (nominal?.name === "list" || nominal?.name === "map") return true;
    if (nominal && (this.dataTypes.has(nominal.name) || this.enumTypes.has(nominal.name)))
      return true;
    return this.dataTypes.has(type) || this.enumTypes.has(type);
  }

  protected checkEnumConstructor(
    declaration: HirEnum,
    variantName: string,
    expression: Extract<Expression, { kind: "call" }>,
    expected?: ValueType,
  ): HirExpression {
    const span = expression.span;
    const variant = declaration.variants.find((candidate) => candidate.name === variantName);
    if (!variant)
      this.fail(
        "unknown-variant",
        `enum '${declaration.name}' has no variant '${variantName}'`,
        span,
      );
    if (variant.factoryFunctionName) {
      return this.checkExpression(
        {
          ...expression,
          callee: { kind: "name", name: variant.factoryFunctionName, span: expression.callee.span },
        },
        expected,
      );
    }
    const plan = this.planArguments(
      expression,
      variant.fields.map((field) => field.name),
      false,
      `variant '${variantName}'`,
    );
    const substitutions = new Map<string, ValueType>();
    const expectedNominal = expected ? nominalGenericParts(expected) : undefined;
    if (
      expectedNominal?.name === declaration.name &&
      expectedNominal.arguments.length === declaration.genericParameters.length
    ) {
      declaration.genericParameters.forEach((parameter, index) =>
        substitutions.set(parameter, expectedNominal.arguments[index]!),
      );
    }
    const fields = plan.map((entry) => {
      const argument = expression.arguments[entry.argumentIndices[0]!]!;
      const field = variant.fields[entry.parameterIndex]!;
      const inferredField = substituteGenericType(field.type, substitutions);
      const checked = this.checkExpression(
        argument,
        containsGenericType(inferredField) ? undefined : inferredField,
      );
      const conflict = inferGenericType(field.type, checked.type, substitutions);
      if (conflict) this.fail("generic-type-mismatch", conflict, argument.span);
      return this.requireCoercion(
        checked,
        substituteGenericType(field.type, substitutions),
        argument.span,
      );
    });
    const unresolved = declaration.genericParameters.filter(
      (parameter) => !substitutions.has(parameter),
    );
    if (unresolved.length > 0) {
      this.fail(
        "generic-enum-needs-context",
        `could not infer generic enum parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`,
        span,
      );
    }
    const type =
      declaration.genericParameters.length > 0
        ? nominalGenericType(
            declaration.name,
            declaration.genericParameters.map((parameter) => substitutions.get(parameter)!),
          )
        : declaration.name;
    return {
      kind: "enum",
      enumIndex: declaration.index,
      tag: variant.tag,
      fields,
      fieldIndices: plan.map((entry) => variant.fields[entry.parameterIndex]!.index),
      fieldTypes: declaration.fields.map((field) => field.type),
      erasedFieldTypes:
        declaration.genericParameters.length > 0
          ? declaration.fields.map((field) => field.type)
          : undefined,
      type,
      span,
    };
  }

  protected checkInternalEnumLiteral(
    expression: Extract<Expression, { kind: "call" }>,
    internalName: string,
    expected?: ValueType,
  ): HirExpression {
    const [, enumName, variantName] = internalName.split(".");
    const declaration = enumName ? this.enumTypes.get(enumName) : undefined;
    const variant = declaration?.variants.find((candidate) => candidate.name === variantName);
    if (!declaration || !variant)
      this.fail(
        "internal-enum-literal",
        `invalid internal enum literal '${internalName}'`,
        expression.span,
      );
    const sourceFields = [...declaration.sharedFields, ...variant.fields];
    if (expression.arguments.length !== sourceFields.length) {
      this.fail(
        "internal-enum-literal",
        `internal enum literal '${internalName}' has the wrong field count`,
        expression.span,
      );
    }
    const fields = expression.arguments.map((argument, index) =>
      this.requireCoercion(
        this.checkExpression(argument, sourceFields[index]!.type),
        sourceFields[index]!.type,
        argument.span,
      ),
    );
    const expectedNominal = expected ? nominalGenericParts(expected) : undefined;
    const type =
      expectedNominal?.name === declaration.name
        ? expected!
        : declaration.genericParameters.length > 0
          ? nominalGenericType(
              declaration.name,
              declaration.genericParameters.map((parameter) => `generic:${parameter}`),
            )
          : declaration.name;
    if (expected) this.requireAssignable(type, expected, expression.span);
    return {
      kind: "enum",
      enumIndex: declaration.index,
      tag: variant.tag,
      fields,
      fieldIndices: sourceFields.map((field) => field.index),
      fieldTypes: declaration.fields.map((field) => field.type),
      erasedFieldTypes:
        declaration.genericParameters.length > 0
          ? declaration.fields.map((field) => field.type)
          : undefined,
      type,
      span: expression.span,
    };
  }

  protected checkNestedPattern(
    pattern: Pattern,
    type: ValueType,
    accessPath: readonly HirPatternAccessStep[],
    bindings: Array<HirMatchArm["bindings"][number]>,
    tests: Array<NonNullable<HirMatchArm["tests"]>[number]>,
  ): boolean {
    if (pattern.kind === "wildcard") return true;
    if (pattern.kind === "binding") {
      bindings.push({
        local: this.addPatternLocal(pattern.name, type, pattern.span),
        fieldIndex: -1,
        type,
        accessPath,
      });
      return true;
    }
    if (["boolean", "integer", "float", "string", "character"].includes(pattern.kind)) {
      const literal = this.checkExpression(
        pattern as Extract<
          Expression,
          { kind: "boolean" | "integer" | "float" | "string" | "character" }
        >,
        type,
      );
      this.requireType(literal.type, type, pattern.span);
      tests.push({ accessPath, literal });
      return false;
    }
    const nominal = nominalGenericParts(type);
    if (pattern.kind === "data") {
      const declaration = this.dataTypes.get(nominal?.name ?? type);
      if (!declaration || pattern.typeName !== declaration.name) {
        this.fail(
          "pattern-type-mismatch",
          `pattern names '${pattern.typeName}', expected '${type}'`,
          pattern.span,
        );
      }
      const substitutions = new Map<string, ValueType>();
      if (nominal)
        declaration.genericParameters.forEach((parameter, index) =>
          substitutions.set(parameter, nominal.arguments[index]!),
        );
      const seen = new Set<string>();
      let irrefutable = true;
      for (const entry of pattern.fields) {
        if (seen.has(entry.name))
          this.fail(
            "duplicate-data-pattern-field",
            `field '${entry.name}' appears more than once`,
            entry.span,
          );
        seen.add(entry.name);
        const field = declaration.fields.find((candidate) => candidate.name === entry.name);
        if (!field)
          this.fail(
            "unknown-data-field",
            `type '${declaration.name}' has no field '${entry.name}'`,
            entry.span,
          );
        const fieldType = substituteGenericType(field.type, substitutions);
        const nextPath: HirPatternAccessStep[] = [
          ...accessPath,
          {
            kind: "data",
            typeIndex: declaration.index,
            fieldIndex: field.index,
            erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
            valueType: fieldType,
          },
        ];
        irrefutable =
          this.checkNestedPattern(entry.pattern, fieldType, nextPath, bindings, tests) &&
          irrefutable;
      }
      return irrefutable;
    }
    if (pattern.kind === "variant") {
      const declaration = this.enumTypes.get(nominal?.name ?? type);
      if (
        !declaration ||
        (pattern.enumName !== undefined && pattern.enumName !== declaration.name)
      ) {
        this.fail(
          "pattern-type-mismatch",
          `variant pattern does not match '${type}'`,
          pattern.span,
        );
      }
      const variant = declaration.variants.find(
        (candidate) => candidate.name === pattern.variantName,
      );
      if (!variant)
        this.fail(
          "unknown-variant",
          `enum '${declaration.name}' has no variant '${pattern.variantName}'`,
          pattern.span,
        );
      const payloadPatterns =
        pattern.payloadPatterns ??
        pattern.bindings.map((name) =>
          name
            ? { kind: "binding" as const, name, span: pattern.span }
            : { kind: "wildcard" as const, span: pattern.span },
        );
      if (payloadPatterns.length !== variant.fields.length) {
        this.fail(
          "pattern-arity",
          `variant '${variant.name}' expects ${variant.fields.length} payload patterns`,
          pattern.span,
        );
      }
      const names = pattern.bindingNames ?? payloadPatterns.map(() => undefined);
      let nextPositional = 0;
      const seen = new Set<number>();
      const fieldIndices = names.map((name) => {
        const index =
          name === undefined
            ? nextPositional++
            : variant.fields.findIndex((field) => field.name === name);
        if (index < 0)
          this.fail(
            "unknown-variant-pattern-field",
            `variant '${variant.name}' has no payload field '${name}'`,
            pattern.span,
          );
        if (seen.has(index))
          this.fail(
            "duplicate-variant-pattern-field",
            `payload field '${variant.fields[index]!.name}' appears more than once`,
            pattern.span,
          );
        seen.add(index);
        return index;
      });
      tests.push({ accessPath, tag: variant.tag, tagEnumIndex: declaration.index });
      const substitutions = new Map<string, ValueType>();
      if (nominal)
        declaration.genericParameters.forEach((parameter, index) =>
          substitutions.set(parameter, nominal.arguments[index]!),
        );
      payloadPatterns.forEach((payloadPattern, sourceIndex) => {
        const fieldIndex = fieldIndices[sourceIndex]!;
        const field = variant.fields[fieldIndex]!;
        if (
          payloadPattern.kind === "binding" &&
          names[sourceIndex] === undefined &&
          payloadPattern.name !== field.name &&
          variant.fields.some(
            (candidate, candidateIndex) =>
              candidateIndex !== fieldIndex && candidate.name === payloadPattern.name,
          )
        ) {
          this.diagnostics.push({
            code: "variant-binding-name-mismatch",
            message: `positional binding '${payloadPattern.name}' occupies payload field '${field.name}'`,
            span: payloadPattern.span,
            severity: "warning",
          });
        }
        const fieldType = substituteGenericType(field.type, substitutions);
        const nextPath: HirPatternAccessStep[] = [
          ...accessPath,
          {
            kind: "enum",
            typeIndex: declaration.index,
            fieldIndex: field.index,
            erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
            valueType: fieldType,
          },
        ];
        this.checkNestedPattern(payloadPattern, fieldType, nextPath, bindings, tests);
      });
      return false;
    }
    this.fail(
      "unsupported-nested-variant-pattern",
      `pattern '${pattern.kind}' is not supported for nested type '${type}'`,
      pattern.span,
    );
  }

  protected checkDataPattern(
    pattern: Extract<Pattern, { kind: "data" }>,
    declaration: HirData,
    path: readonly HirPatternPathStep[],
    bindings: HirMatchBinding[],
    tests: HirMatchTest[],
  ): boolean {
    if (pattern.typeName !== declaration.name) {
      this.fail(
        "pattern-type-mismatch",
        `pattern names '${pattern.typeName}', expected '${declaration.name}'`,
        pattern.span,
      );
    }
    const seen = new Set<string>();
    let irrefutable = true;
    for (const entry of pattern.fields) {
      if (seen.has(entry.name))
        this.fail(
          "duplicate-data-pattern-field",
          `field '${entry.name}' appears more than once`,
          entry.span,
        );
      seen.add(entry.name);
      const field = declaration.fields.find((candidate) => candidate.name === entry.name);
      if (!field)
        this.fail(
          "unknown-data-field",
          `type '${declaration.name}' has no field '${entry.name}'`,
          entry.span,
        );
      const fieldPath = [...path, { dataIndex: declaration.index, fieldIndex: field.index }];
      const nested = entry.pattern;
      if (nested.kind === "wildcard") continue;
      if (nested.kind === "binding") {
        bindings.push({
          local: this.addPatternLocal(nested.name, field.type, nested.span),
          fieldIndex: -1,
          type: field.type,
          path: fieldPath,
        });
        continue;
      }
      if (nested.kind === "data") {
        const nestedDeclaration = this.dataTypes.get(field.type);
        if (!nestedDeclaration)
          this.fail(
            "pattern-type-mismatch",
            `field '${entry.name}' has non-data type '${field.type}'`,
            nested.span,
          );
        irrefutable =
          this.checkDataPattern(nested, nestedDeclaration, fieldPath, bindings, tests) &&
          irrefutable;
        continue;
      }
      if (["boolean", "integer", "float", "string", "character"].includes(nested.kind)) {
        const literal = this.checkExpression(
          nested as Extract<
            Expression,
            { kind: "boolean" | "integer" | "float" | "string" | "character" }
          >,
        );
        this.requireType(literal.type, field.type, nested.span);
        tests.push({ path: fieldPath, literal });
        irrefutable = false;
        continue;
      }
      this.fail(
        "pattern-type-mismatch",
        `pattern is not valid for field '${entry.name}' of type '${field.type}'`,
        nested.span,
      );
    }
    return irrefutable;
  }

  protected addPatternLocal(name: string, type: ValueType, span: SourceSpan): HirLocal {
    if (PRELUDE_NAMES.has(name))
      this.fail("prelude-name-shadow", `pattern binding '${name}' shadows a prelude name`, span);
    if (this.currentScope().has(name))
      this.fail("duplicate-binding", `pattern binding '${name}' appears more than once`, span);
    const local: HirLocal = {
      name,
      type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span,
    };
    this.locals.push(local);
    this.currentScope().set(name, local);
    return local;
  }
}
