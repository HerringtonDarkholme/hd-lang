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
  mutableInner,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType as renderOptionalType,
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleLayout,
  displayType,
} from "../types.ts";
import { isIntegerType, numericType } from "../numeric.ts";
import { PRELUDE_NAMES, TEST_CASE_FUNCTIONS } from "./context.ts";
import { standardSubmoduleFunctionIdentity } from "./standard-library.ts";
import {
  containsGenericType,
  genericTypeName,
  inferGenericType,
  orderedTypeSubstitutions,
  substituteGenericType,
} from "./shared.ts";
import { spelledApplication, spelledType } from "./spelling.ts";

import { CallChecker } from "./calls.ts";
export abstract class PatternChecker extends CallChecker {
  /**
   * A std submodule member, unless a value binding owns the receiver name.
   * A test registration function reached through `std.testing`, as
   * `testing.it_each`, is misplaced here, at `span`: it registers only as a
   * direct call in test position (spec/lang/10-modules.md#r-module.testing.reg.identity).
   */
  protected standardSubmoduleFunction(
    receiver: string,
    member: string,
    span: SourceSpan,
  ): string | undefined {
    if (
      this.resolveLocal(receiver) ||
      this.availableCaptures.has(receiver) ||
      this.globals.has(receiver) ||
      this.signatures.has(receiver)
    )
      return undefined;
    const origin = this.imports.get(receiver);
    if (origin === "std.testing" && TEST_CASE_FUNCTIONS.has(`${origin}.${member}`))
      this.fail(
        "misplaced-test-case",
        `${receiver}.${member}(...) registers a test case only as a direct call in test position`,
        span,
      );
    const identity = standardSubmoduleFunctionIdentity(origin, member);
    return identity === undefined ? undefined : this.signatures.get(identity)?.name;
  }

  protected isIdentityType(type: ValueType): boolean {
    // Access permission does not change the category (04 types.sealed.permission).
    if (readonlyType(type) !== type) return this.isIdentityType(readonlyType(type));
    // A newtype has its base type's category (04 types.sealed.newtype).
    const newtype = this.dataTypes.get(type);
    if (newtype?.newtype && newtype.fields[0]) return this.isIdentityType(newtype.fields[0].type);
    const generic = genericTypeName(type);
    if (generic) return (this.signature.referenceParameters ?? []).includes(generic);
    if (type.startsWith("trait:")) return true;
    // An optional is an ordinary enum value (04 Optional Types): `.None` is
    // canonical and each `.Some` construction has its own identity.
    if (optionalInner(type) !== undefined) return true;
    if (
      functionParts(type) ||
      storedSuspensionParts(type) ||
      suspensionParts(type) ||
      traitSuspensionParts(type) ||
      contextKeys(type)
    )
      return true;
    const nominal = nominalGenericParts(type);
    if (nominal?.name === "List" || nominal?.name === "Map") return true;
    if (nominal && (this.dataTypes.has(nominal.name) || this.enumTypes.has(nominal.name)))
      return true;
    return this.dataTypes.has(type) || this.enumTypes.has(type);
  }

  /**
   * `T?` is the prelude enum `Option[T]`. Its variants construct the existing
   * erased optional representation: `.None` is the absent variant and
   * `.Some(value)` wraps exactly one layer.
   */
  protected checkOptionVariant(
    variantName: string,
    call: Extract<Expression, { kind: "call" }> | undefined,
    expected: ValueType | undefined,
    span: SourceSpan,
    qualified: boolean,
  ): HirExpression {
    const expectedOptional = expected ? optionalInner(expected) : undefined;
    const optionalType = expectedOptional !== undefined ? expected : undefined;
    if (variantName !== "Some" && variantName !== "None")
      this.fail("unknown-variant", `enum 'Option' has no variant '${variantName}'`, span);
    if (call?.argumentSpreads?.some(Boolean))
      this.fail(
        "positional-spread-needs-vararg",
        "enum constructors have no variadic parameter",
        span,
      );
    if (optionalType === undefined && !qualified)
      this.fail(
        "missing-contextual-enum-type",
        `variant '.${variantName}' requires an expected enum type`,
        span,
      );
    if (variantName === "None") {
      if (call && call.arguments.length > 0)
        this.fail("argument-count", "variant 'None' takes no arguments", span);
      if (optionalType === undefined) this.failUnresolvedType(["T"], "T?", span);
      return { kind: "variant-wrap", variant: "optional-absent", type: optionalType, span };
    }
    if (!call)
      this.fail("unsaturated-enum-constructor", "variant 'Some' requires 1 argument", span);
    if (call.arguments.length !== 1)
      this.fail(
        "argument-count",
        `variant 'Some' takes 1 argument, found ${call.arguments.length}`,
        span,
      );
    const name = call.argumentNames?.[0];
    if (name !== undefined && name !== "value")
      this.fail("unknown-data-field", `variant 'Some' has no payload field '${name}'`, span);
    const argument = call.arguments[0]!;
    if (expectedOptional === undefined) {
      const payload = this.checkExpression(argument);
      return {
        kind: "variant-wrap",
        variant: "optional-present",
        payload,
        payloadType: payload.type,
        type: renderOptionalType(payload.type),
        span,
      };
    }
    const payload = this.requireCoercion(
      this.checkExpression(argument, expectedOptional),
      expectedOptional,
      argument.span,
    );
    return {
      kind: "variant-wrap",
      variant: "optional-present",
      payload,
      payloadType: expectedOptional,
      type: optionalType!,
      span,
    };
  }

  /** `Option` names the prelude enum unless a local or global binding shadows it. */
  protected namesOptionEnum(name: string): boolean {
    return (
      name === "Option" &&
      !this.resolveLocal(name) &&
      !this.availableCaptures.has(name) &&
      !this.resolveGlobal(name)
    );
  }

  /** `Result` names the prelude enum unless a local, capture, or global shadows it. */
  protected namesResultEnum(name: string): boolean {
    return (
      name === "Result" &&
      !this.resolveLocal(name) &&
      !this.availableCaptures.has(name) &&
      !this.resolveGlobal(name)
    );
  }

  /**
   * Checks `.Ok(value)`, `.Err(error)`, and their `Result.`-qualified forms
   * (04-type-system.md#result-types). A `void` success takes the one
   * argument `()` (04-type-system.md#r-types.result.unit-ok); the prototype
   * accepts only the literal `()` there, since it still keeps `void` apart
   * from the empty tuple.
   */
  protected checkResultVariant(
    variantName: string,
    expression: Extract<Expression, { kind: "call" }>,
    expected: ValueType | undefined,
  ): HirExpression {
    if (variantName !== "Ok" && variantName !== "Err")
      this.fail(
        "unknown-variant",
        `enum 'Result' has no variant '${variantName}'`,
        expression.span,
      );
    if (expression.argumentSpreads?.some(Boolean))
      this.fail(
        "positional-spread-needs-vararg",
        "enum constructors have no variadic parameter",
        expression.span,
      );
    const parts = expected && resultParts(expected);
    const ok = variantName === "Ok";
    if (!parts) {
      // Name what the payload leaves unsolved: `E` for `.Ok`, `T` for `.Err`.
      const argument = expression.arguments.length === 1 ? expression.arguments[0]! : undefined;
      const payload = argument && spelledType(this.checkExpression(argument));
      const [success, error] = ok ? [payload ?? "T", "E"] : ["T", payload ?? "E"];
      this.failUnresolvedType(
        payload === undefined ? ["T", "E"] : [ok ? "E" : "T"],
        nominalGenericType("Result", [success, error]),
        expression.span,
      );
    }
    const payloadType = ok ? parts.ok : parts.error;
    const unitSuccess = ok && payloadType === "void";
    if (expression.arguments.length !== 1) {
      this.fail(
        "argument-count",
        `variant '${variantName}' takes 1 argument, found ${expression.arguments.length}`,
        expression.span,
      );
    }
    this.resolveArgumentMapping(expression, [ok ? "value" : "error"], `Result.${variantName}`);
    if (unitSuccess) {
      const argument = expression.arguments[0]!;
      if (argument.kind !== "tuple" || argument.elements.length > 0)
        this.fail("type-mismatch", "a void success takes the value '()'", argument.span);
    }
    const payload = !unitSuccess
      ? this.requireCoercion(
          this.checkExpression(expression.arguments[0]!, payloadType),
          payloadType,
          expression.arguments[0]!.span,
        )
      : undefined;
    return {
      kind: "variant-wrap",
      variant: ok ? "result-ok" : "result-error",
      payload,
      payloadType,
      type: expected,
      span: expression.span,
    };
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
      new Set(),
      "unknown-data-field",
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
      if (conflict) this.fail("type-mismatch", conflict, argument.span);
      return this.requireCoercion(
        checked,
        substituteGenericType(field.type, substitutions),
        argument.span,
      );
    });
    const unresolved = declaration.genericParameters.filter(
      (parameter) => !substitutions.has(parameter),
    );
    if (unresolved.length > 0)
      this.failUnresolvedType(
        unresolved,
        spelledApplication(
          nominalGenericType(
            declaration.name,
            declaration.genericParameters.map(
              (parameter) => substitutions.get(parameter) ?? parameter,
            ),
          ),
          fields,
        ),
        span,
      );
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
      erasedTypeSubstitutions: orderedTypeSubstitutions(
        declaration.genericParameters,
        substitutions,
      ),
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
    // `$enum-template` builds a variant's shared data alone; its payload slots
    // keep their default values and are never read.
    const sourceFields = internalName.startsWith("$enum-template.")
      ? declaration.sharedFields
      : [...declaration.sharedFields, ...variant.fields];
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
    const substitutions = new Map<string, ValueType>();
    const nominal = nominalGenericParts(readonlyType(type));
    if (nominal?.name === declaration.name)
      declaration.genericParameters.forEach((parameter, index) =>
        substitutions.set(parameter, nominal.arguments[index]!),
      );
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
      erasedTypeSubstitutions: orderedTypeSubstitutions(
        declaration.genericParameters,
        substitutions,
      ),
      type,
      span: expression.span,
    };
  }

  /** `Name(...)` without `.` or a qualifier never names a variant (06-control-flow.md#match-expressions). */
  protected rejectBareCallPattern(pattern: Pattern): void {
    if (pattern.kind === "variant" && pattern.bare)
      this.fail(
        "bare-variant-pattern",
        `bare variant '${pattern.variantName}(...)' must be written as '.${pattern.variantName}(...)' or qualified by its enum`,
        pattern.span,
      );
  }

  /** The unit pattern `()` matches only `void`, the type of `()` (06-control-flow.md#r-flow.match.unit.type). */
  protected requireUnitPattern(type: ValueType, span: SourceSpan): void {
    const plain = readonlyType(type);
    if (plain !== "void" && plain !== "()")
      this.fail(
        "type-mismatch",
        `the pattern '()' matches only void, not '${displayType(type)}'`,
        span,
      );
  }

  /** A void success's payload `()` matches `_` or `()` (04-type-system.md#r-types.result.unit-pattern); any other pattern passes. */
  protected requireUnitPayload(pattern: Pattern | undefined, type: ValueType): void {
    if (pattern?.kind === "wildcard" && pattern.unit) this.requireUnitPattern(type, pattern.span);
  }

  protected checkNestedPattern(
    pattern: Pattern,
    type: ValueType,
    accessPath: readonly HirPatternAccessStep[],
    bindings: Array<HirMatchArm["bindings"][number]>,
    tests: Array<NonNullable<HirMatchArm["tests"]>[number]>,
  ): boolean {
    this.rejectBareCallPattern(pattern);
    if (pattern.kind === "wildcard") {
      if (pattern.unit) this.requireUnitPattern(type, pattern.span);
      return true;
    }
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
    if (pattern.kind === "range") {
      const local = this.checkRangePattern(pattern, type);
      bindings.push({ local, fieldIndex: -1, type: local.type, accessPath });
      return false;
    }
    const optional = optionalInner(readonlyType(type));
    if (
      optional !== undefined &&
      pattern.kind === "variant" &&
      (pattern.enumName === undefined || pattern.enumName === "Option")
    ) {
      const payloadPatterns =
        pattern.payloadPatterns ??
        pattern.bindings.map((name) =>
          name
            ? { kind: "binding" as const, name, span: pattern.span }
            : { kind: "wildcard" as const, span: pattern.span },
        );
      if (pattern.variantName !== "Some" && pattern.variantName !== "None")
        this.fail(
          "unknown-variant",
          `enum 'Option' has no variant '${pattern.variantName}'`,
          pattern.span,
        );
      const some = pattern.variantName === "Some";
      if (payloadPatterns.length !== (some ? 1 : 0))
        this.fail(
          "pattern-arity",
          `variant '${pattern.variantName}' expects ${some ? 1 : 0} payload patterns`,
          pattern.span,
        );
      tests.push({ accessPath, tag: some ? 1 : 0, tagEnumIndex: -1 });
      if (some)
        this.checkNestedPattern(
          payloadPatterns[0]!,
          optional,
          [
            ...accessPath,
            { kind: "erased-variant", typeIndex: -1, fieldIndex: 0, valueType: optional },
          ],
          bindings,
          tests,
        );
      return false;
    }
    if (pattern.kind === "tuple")
      return this.checkTuplePattern(pattern, type, bindings, tests, accessPath);
    const nominal = nominalGenericParts(type);
    if (pattern.kind === "data") {
      const declaration = this.dataTypes.get(nominal?.name ?? type);
      if (!declaration || pattern.typeName !== declaration.name) {
        this.fail(
          "pattern-type-mismatch",
          `pattern names '${pattern.typeName}', expected '${displayType(type)}'`,
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
            erasedFieldType: containsGenericType(field.type) ? field.type : undefined,
            erasedTypeSubstitutions: orderedTypeSubstitutions(
              declaration.genericParameters,
              substitutions,
            ),
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
      if (!declaration && pattern.enumName === undefined)
        this.fail(
          "missing-contextual-enum-type",
          `variant pattern '.${pattern.variantName}' requires an enum type, found '${displayType(type)}'`,
          pattern.span,
        );
      if (
        !declaration ||
        (pattern.enumName !== undefined && pattern.enumName !== declaration.name)
      ) {
        this.fail(
          "pattern-type-mismatch",
          `variant pattern does not match '${displayType(type)}'`,
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
            "unknown-data-field",
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
            erasedFieldType: containsGenericType(field.type) ? field.type : undefined,
            erasedTypeSubstitutions: orderedTypeSubstitutions(
              declaration.genericParameters,
              substitutions,
            ),
            valueType: fieldType,
          },
        ];
        this.checkNestedPattern(payloadPattern, fieldType, nextPath, bindings, tests);
      });
      return false;
    }
    this.fail(
      "unsupported-nested-variant-pattern",
      `pattern '${pattern.kind}' is not supported for nested type '${displayType(type)}'`,
      pattern.span,
    );
  }

  /** A tuple pattern; irrefutable when every element pattern is. */
  protected checkTuplePattern(
    pattern: Extract<Pattern, { kind: "tuple" }>,
    type: ValueType,
    bindings: Array<HirMatchArm["bindings"][number]>,
    tests: Array<NonNullable<HirMatchArm["tests"]>[number]>,
    accessPath: readonly HirPatternAccessStep[] = [],
  ): boolean {
    const elements = tupleLayout(readonlyType(type));
    if (!elements)
      this.fail(
        "pattern-type-mismatch",
        `a tuple pattern does not match '${displayType(type)}'`,
        pattern.span,
      );
    this.checkSpreadArity(
      pattern.elements.map(
        (_, index) => pattern.spread === true && index === pattern.elements.length - 1,
      ),
      type,
      pattern.span,
    );
    if (elements.length !== pattern.elements.length)
      this.fail(
        "pattern-arity",
        `a ${pattern.elements.length}-element tuple pattern does not match '${displayType(type)}'`,
        pattern.span,
      );
    let irrefutable = true;
    pattern.elements.forEach((element, index) => {
      const elementType = elements[index]!;
      const nextPath: HirPatternAccessStep[] = [
        ...accessPath,
        { kind: "tuple", typeIndex: -1, fieldIndex: index, valueType: elementType },
      ];
      irrefutable =
        this.checkNestedPattern(element, elementType, nextPath, bindings, tests) && irrefutable;
    });
    return irrefutable;
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
        // A direct `mut U` field of a readonly subject binds as `U`
        // (06-control-flow.md#r-flow.match.data.readonly-mut).
        const view =
          this.matchSubjectReadonly && mutableInner(field.type) !== undefined
            ? readonlyType(field.type)
            : field.type;
        bindings.push({
          local: this.addPatternLocal(nested.name, view, nested.span),
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
            `field '${entry.name}' has non-data type '${displayType(field.type)}'`,
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
          field.type,
        );
        this.requireType(literal.type, field.type, nested.span);
        tests.push({ path: fieldPath, literal });
        irrefutable = false;
        continue;
      }
      this.fail(
        "pattern-type-mismatch",
        `pattern is not valid for field '${entry.name}' of type '${displayType(field.type)}'`,
        nested.span,
      );
    }
    return irrefutable;
  }

  /** True while the arms of a match on a readonly subject are checked. */
  protected matchSubjectReadonly = false;

  /**
   * The tests of the range patterns in the arm being checked, which the arm
   * adds to its guard. Each reads the hidden local its pattern binds.
   */
  protected rangePatternConditions: Expression[] = [];

  /**
   * A range pattern (06-control-flow.md#range-patterns) binds no name and
   * builds no range: the matched value goes to a hidden local, and the arm's
   * guard tests it against the bounds. Coverage is checked from the pattern
   * itself (exhaustiveness.ts), so the guard does not weaken it.
   */
  protected checkRangePattern(
    pattern: Extract<Pattern, { kind: "range" }>,
    type: ValueType,
  ): HirLocal {
    const view = readonlyType(type);
    const numeric = numericType(view);
    if (!numeric || !isIntegerType(view))
      this.fail(
        "type-mismatch",
        `a range pattern needs an integer subject, found '${displayType(type)}'`,
        pattern.span,
      );
    for (const bound of [pattern.start, pattern.end])
      if (bound !== undefined && (bound < numeric.minimum! || bound > numeric.maximum!))
        this.fail(
          "integer-literal-range",
          `range pattern bound ${bound} is outside the range of '${view}'`,
          pattern.span,
        );
    const span = pattern.span;
    const local = this.addPatternLocal(`$range${this.locals.length}`, view, span);
    const value: Expression = { kind: "name", name: local.name, span };
    const literal = (bound: bigint): Expression =>
      bound < 0n
        ? {
            kind: "unary",
            operator: "-",
            operand: { kind: "integer", value: -bound, span },
            span,
          }
        : { kind: "integer", value: bound, span };
    if (pattern.start !== undefined)
      this.rangePatternConditions.push({
        kind: "binary",
        operator: ">=",
        left: value,
        right: literal(pattern.start),
        span,
      });
    if (pattern.end !== undefined)
      this.rangePatternConditions.push({
        kind: "binary",
        operator: pattern.inclusive ? "<=" : "<",
        left: value,
        right: literal(pattern.end),
        span,
      });
    return local;
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
