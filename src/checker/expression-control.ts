import type { Expression, Statement } from "../ast.ts";
import type {
  HirData,
  HirEnum,
  HirExpression,
  HirLocal,
  HirMatchArm,
  HirPatternAccessStep,
  HirStatement,
  ValueType,
} from "../hir.ts";
import {
  type NominalGenericParts,
  type ResultParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  resultParts,
  tupleParts,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import {
  type BindingExpressionFlow,
  bindingExpressionFlow,
  genericTypeName,
  iterableInfo,
  substituteGenericType,
} from "./shared.ts";

import { ExpressionComprehensionChecker } from "./expression-comprehensions.ts";
type MatchExpression = Extract<Expression, { kind: "match" }>;
type MatchSourceArm = MatchExpression["arms"][number];
type MatchBinding = HirMatchArm["bindings"][number];
type MatchTest = NonNullable<HirMatchArm["tests"]>[number];

interface MatchContext {
  readonly subject: HirExpression;
  readonly subjectNominal?: NominalGenericParts;
  readonly declaration?: HirEnum;
  readonly dataDeclaration?: HirData;
  readonly optional?: ValueType;
  readonly result?: ResultParts;
  readonly boolean: boolean;
  readonly scalar: boolean;
  readonly expected?: ValueType;
  readonly covered: Set<number | string>;
  readonly arms: HirMatchArm[];
  catchAll: boolean;
  resultType?: ValueType;
}

export abstract class ExpressionControlChecker extends ExpressionComprehensionChecker {
  protected checkControlExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "if": {
        const condition = this.checkExpression(expression.condition);
        this.requireType(condition.type, "bool", condition.span);
        const bindingFlow = this.applyConditionBindingFlow(expression.condition);
        const thenBody = this.checkConditionalSuite(
          expression.thenBody,
          bindingFlow.whenTrue,
          expected,
        );
        const elseBody =
          expression.elseBody.length > 0
            ? this.checkConditionalSuite(expression.elseBody, bindingFlow.whenFalse, expected)
            : [];
        if (elseBody.length === 0) {
          return { kind: "if", condition, thenBody, elseBody, type: "void", span: expression.span };
        }
        const thenType = this.blockType(thenBody);
        const elseType = this.blockType(elseBody);
        if (thenType !== elseType && thenType !== "never" && elseType !== "never") {
          this.fail(
            "if-branch-type",
            `if branches have types ${thenType} and ${elseType}`,
            expression.span,
          );
        }
        const type = thenType === "never" ? elseType : thenType;
        return { kind: "if", condition, thenBody, elseBody, type, span: expression.span };
      }
      case "for": {
        const iterable = this.checkExpression(expression.iterable);
        const info = iterableInfo(iterable, this.implementations);
        if (info?.iteratorKind === "trait" && mutableInner(iterable.type) === undefined)
          this.fail(
            "mutable-receiver-required",
            "iteration requires mutable access to an Iterator implementation",
            expression.iterable.span,
          );
        if (!info) {
          this.fail(
            "unsatisfied-trait-bound",
            `type '${iterable.type}' does not implement Iterable, required by the for loop`,
            expression.iterable.span,
          );
        }
        const { iteratorKind, iteratorFunctionIndex, yieldType } = info;
        const bindingTypes = expression.bindings.length === 1 ? [yieldType] : tupleParts(yieldType);
        if (!bindingTypes || bindingTypes.length !== expression.bindings.length) {
          this.fail(
            "type-mismatch",
            `loop binding has ${expression.bindings.length} names but '${yieldType}' yields ${bindingTypes?.length ?? 1} value${bindingTypes?.length === 1 ? "" : "s"}`,
            expression.span,
          );
        }
        const elseBody =
          expression.elseBody.length > 0
            ? this.checkStatements(expression.elseBody, true, expected)
            : [];
        const result = elseBody.length > 0 ? this.blockType(elseBody) : undefined;
        this.loopResults.push(result);
        this.scopes.push(new Map());
        let bindings: HirLocal[];
        let body: readonly HirStatement[];
        try {
          const seen = new Set<string>();
          bindings = expression.bindings.map((binding, index) => {
            if (seen.has(binding.name))
              this.fail(
                "duplicate-binding",
                `loop binding '${binding.name}' appears more than once`,
                binding.span,
              );
            seen.add(binding.name);
            if (PRELUDE_NAMES.has(binding.name))
              this.fail(
                "prelude-name-shadow",
                `loop binding '${binding.name}' shadows a prelude name`,
                binding.span,
              );
            const local: HirLocal = {
              name: binding.name,
              type: bindingTypes[index]!,
              index: this.locals.length,
              mutable: false,
              parameter: false,
              span: binding.span,
            };
            this.locals.push(local);
            this.currentScope().set(binding.name, local);
            return local;
          });
          body = this.checkStatements(expression.body, false);
        } finally {
          this.scopes.pop();
          this.loopResults.pop();
        }
        return {
          kind: "for",
          iterable,
          iteratorKind,
          iteratorFunctionIndex,
          yieldType,
          bindings,
          body,
          elseBody,
          type: result ?? "void",
          span: expression.span,
        };
      }
      case "while": {
        const condition = this.checkExpression(expression.condition);
        this.requireType(condition.type, "bool", condition.span);
        const bindingFlow = this.applyConditionBindingFlow(expression.condition);
        const elseBody =
          expression.elseBody.length > 0
            ? this.checkConditionalSuite(expression.elseBody, bindingFlow.whenFalse, expected)
            : [];
        const result = elseBody.length > 0 ? this.blockType(elseBody) : undefined;
        this.loopResults.push(result);
        let body: readonly HirStatement[];
        try {
          body = this.checkConditionalSuite(expression.body, bindingFlow.whenTrue);
        } finally {
          this.loopResults.pop();
        }
        return {
          kind: "while",
          condition,
          body,
          elseBody,
          type: result ?? "void",
          span: expression.span,
        };
      }
      default:
        return undefined;
    }
  }

  private applyConditionBindingFlow(expression: Expression): BindingExpressionFlow {
    const flow = bindingExpressionFlow(expression);
    for (const name of flow.all) {
      const local = this.currentScope().get(name);
      if (!local) continue;
      if (flow.always.has(name)) this.unavailableBindingLocals.delete(local.index);
      else this.unavailableBindingLocals.add(local.index);
    }
    return flow;
  }

  private checkConditionalSuite(
    statements: readonly Statement[],
    availableNames: ReadonlySet<string>,
    expected?: ValueType,
  ): HirStatement[] {
    const previous = new Set(this.allowedConditionalBindingLocals);
    for (const name of availableNames) {
      const local = this.resolveLocal(name);
      if (local) this.allowedConditionalBindingLocals.add(local.index);
    }
    try {
      return this.checkStatements(statements, true, expected);
    } finally {
      this.allowedConditionalBindingLocals.clear();
      previous.forEach((index) => this.allowedConditionalBindingLocals.add(index));
    }
  }

  protected checkMatchExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    if (expression.kind !== "match") return undefined;
    return this.checkMatch(expression, expected);
  }

  private checkMatch(expression: MatchExpression, expected?: ValueType): HirExpression {
    const subject = this.checkExpression(expression.subject);
    const subjectNominal = nominalGenericParts(subject.type);
    const declaration = this.enumTypes.get(subjectNominal?.name ?? subject.type);
    const dataDeclaration = this.dataTypes.get(subject.type);
    const optional = optionalInner(subject.type);
    const result = resultParts(subject.type);
    const boolean = subject.type === "bool";
    const scalar = new Set<ValueType>(["bool", "i32", "f64", "char", "string"]).has(subject.type);
    if (!declaration && !dataDeclaration && optional === undefined && !result && !scalar) {
      this.fail(
        "unsupported-match-subject",
        `matching '${subject.type}' is not implemented in this MVP slice`,
        expression.subject.span,
      );
    }
    const context: MatchContext = {
      subject,
      subjectNominal,
      declaration,
      dataDeclaration,
      optional,
      result,
      boolean,
      scalar,
      expected,
      covered: new Set(),
      arms: [],
      catchAll: false,
    };
    for (const arm of expression.arms) this.checkMatchArm(arm, context);
    const requiredCases = context.declaration?.variants.length ?? 2;
    const finiteCoverage = Boolean(
      context.declaration || context.optional !== undefined || context.result || context.boolean,
    );
    if (!context.catchAll && (!finiteCoverage || context.covered.size !== requiredCases)) {
      const missing = context.declaration
        ? context.declaration.variants
            .filter((variant) => !context.covered.has(variant.tag))
            .map((variant) => variant.name)
        : context.optional !== undefined
          ? [!context.covered.has(0) && "nil", !context.covered.has(1) && "present"].filter(Boolean)
          : context.result
            ? [!context.covered.has(0) && "Ok", !context.covered.has(1) && "Err"].filter(Boolean)
            : context.boolean
              ? [
                  !context.covered.has("bool:false") && "false",
                  !context.covered.has("bool:true") && "true",
                ].filter(Boolean)
              : ["catch-all"];
      this.fail(
        "nonexhaustive-match",
        `match does not cover: ${missing.join(", ")}`,
        expression.span,
      );
    }
    return {
      kind: "match",
      subject: context.subject,
      representation: context.declaration
        ? "enum"
        : context.dataDeclaration
          ? "data"
          : context.scalar
            ? "scalar"
            : "erased-variant",
      enumIndex: context.declaration?.index,
      arms: context.arms,
      type: context.resultType ?? "void",
      span: expression.span,
    };
  }

  private checkMatchArm(arm: MatchSourceArm, context: MatchContext): void {
    if (context.catchAll)
      this.fail("unreachable-match-arm", "a match arm follows an unguarded catch-all", arm.span);
    this.scopes.push(new Map());
    const bindings: MatchBinding[] = [];
    const tests: MatchTest[] = [];
    let tag: number | undefined;
    let literal: HirExpression | undefined;
    const guarded = arm.guard !== undefined;
    try {
      if (context.dataDeclaration && arm.pattern.kind === "data") {
        const irrefutable = this.checkDataPattern(
          arm.pattern,
          context.dataDeclaration,
          [],
          bindings,
          tests,
        );
        if (irrefutable && !guarded) context.catchAll = true;
      } else if (
        context.scalar &&
        ["boolean", "integer", "float", "string", "character"].includes(arm.pattern.kind)
      ) {
        literal = this.checkExpression(
          arm.pattern as Extract<
            Expression,
            { kind: "boolean" | "integer" | "float" | "string" | "character" }
          >,
        );
        this.requireType(literal.type, context.subject.type, arm.pattern.span);
        const key =
          literal.kind === "string"
            ? `string:${literal.bytes.join(",")}`
            : `${literal.type}:${"value" in literal ? literal.value : ""}`;
        if (context.covered.has(key))
          this.fail(
            "unreachable-match-arm",
            "literal pattern is already covered",
            arm.pattern.span,
          );
        if (!guarded) context.covered.add(key);
      } else if (context.declaration && arm.pattern.kind === "variant") {
        const pattern = arm.pattern;
        if (pattern.enumName !== undefined && pattern.enumName !== context.declaration.name) {
          this.fail(
            "pattern-type-mismatch",
            `pattern names '${pattern.enumName}', expected '${context.declaration.name}'`,
            pattern.span,
          );
        }
        const variant = context.declaration.variants.find(
          (candidate) => candidate.name === pattern.variantName,
        );
        if (!variant)
          this.fail(
            "unknown-variant",
            `enum '${context.declaration.name}' has no variant '${pattern.variantName}'`,
            pattern.span,
          );
        if (context.covered.has(variant.tag))
          this.fail(
            "unreachable-match-arm",
            `variant '${variant.name}' is already covered`,
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
        const bindingNames = pattern.bindingNames ?? payloadPatterns.map(() => undefined);
        let nextPositional = 0;
        const seenFields = new Set<number>();
        const fieldIndices = bindingNames.map((fieldName) => {
          const fieldIndex =
            fieldName === undefined
              ? nextPositional++
              : variant.fields.findIndex((field) => field.name === fieldName);
          if (fieldIndex < 0)
            this.fail(
              "unknown-data-field",
              `variant '${variant.name}' has no payload field '${fieldName}'`,
              pattern.span,
            );
          if (seenFields.has(fieldIndex))
            this.fail(
              "duplicate-variant-pattern-field",
              `payload field '${variant.fields[fieldIndex]!.name}' appears more than once`,
              pattern.span,
            );
          seenFields.add(fieldIndex);
          return fieldIndex;
        });
        tag = variant.tag;
        let payloadRefutable = false;
        const substitutions = new Map<string, ValueType>();
        if (context.subjectNominal)
          context.declaration.genericParameters.forEach((parameter, parameterIndex) =>
            substitutions.set(parameter, context.subjectNominal!.arguments[parameterIndex]!),
          );
        payloadPatterns.forEach((payloadPattern, index) => {
          const fieldIndex = fieldIndices[index]!;
          const field = variant.fields[fieldIndex]!;
          const fieldType = substituteGenericType(field.type, substitutions);
          if (payloadPattern.kind === "binding") {
            const name = payloadPattern.name;
            if (
              bindingNames[index] === undefined &&
              name !== field.name &&
              variant.fields.some(
                (candidate, candidateIndex) =>
                  candidateIndex !== fieldIndex && candidate.name === name,
              )
            ) {
              this.diagnostics.push({
                code: "variant-binding-name-mismatch",
                message: `positional binding '${name}' occupies payload field '${field.name}'`,
                span: payloadPattern.span,
                severity: "warning",
              });
            }
          }
          const accessPath: HirPatternAccessStep[] = [
            {
              kind: "enum",
              typeIndex: context.declaration!.index,
              fieldIndex: field.index,
              erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
              valueType: fieldType,
            },
          ];
          payloadRefutable ||= !this.checkNestedPattern(
            payloadPattern,
            fieldType,
            accessPath,
            bindings,
            tests,
          );
        });
        if (!guarded && !payloadRefutable) context.covered.add(tag);
      } else if (context.optional !== undefined && arm.pattern.kind === "nil") {
        tag = 0;
        if (context.covered.has(tag))
          this.fail("unreachable-match-arm", "nil is already covered", arm.pattern.span);
        if (!guarded) context.covered.add(tag);
      } else if (context.optional !== undefined && arm.pattern.kind === "optional-present") {
        tag = 1;
        if (context.covered.has(tag))
          this.fail(
            "unreachable-match-arm",
            "the present optional case is already covered",
            arm.pattern.span,
          );
        if (!guarded) context.covered.add(tag);
        bindings.push({
          local: this.addPatternLocal(arm.pattern.name, context.optional, arm.pattern.span),
          fieldIndex: 0,
          type: context.optional,
        });
      } else if (context.result && arm.pattern.kind === "result-variant") {
        const ok = arm.pattern.variantName === "Ok";
        tag = ok ? 0 : 1;
        if (context.covered.has(tag))
          this.fail(
            "unreachable-match-arm",
            `${arm.pattern.variantName} is already covered`,
            arm.pattern.span,
          );
        const payloadType = ok ? context.result.ok : context.result.error;
        const payloadPatterns =
          arm.pattern.payloadPatterns ??
          arm.pattern.bindings.map((name) =>
            name
              ? { kind: "binding" as const, name, span: arm.pattern.span }
              : { kind: "wildcard" as const, span: arm.pattern.span },
          );
        const voidSuccess = ok && payloadType === "void";
        if (
          payloadPatterns.length !== (voidSuccess ? 0 : 1) &&
          !(voidSuccess && payloadPatterns.length === 1 && payloadPatterns[0]?.kind === "wildcard")
        ) {
          this.fail(
            "pattern-arity",
            `${arm.pattern.variantName} expects ${voidSuccess ? 0 : 1} payload patterns`,
            arm.pattern.span,
          );
        }
        let payloadRefutable = false;
        if (!voidSuccess) {
          const payloadPattern = payloadPatterns[0]!;
          if (payloadPattern.kind === "binding") {
            bindings.push({
              local: this.addPatternLocal(payloadPattern.name, payloadType, payloadPattern.span),
              fieldIndex: 0,
              type: payloadType,
            });
          } else if (payloadPattern.kind !== "wildcard") {
            const accessPath: HirPatternAccessStep[] = [
              {
                kind: "erased-variant",
                typeIndex: -1,
                fieldIndex: 0,
                valueType: payloadType,
              },
            ];
            payloadRefutable = !this.checkNestedPattern(
              payloadPattern,
              payloadType,
              accessPath,
              bindings,
              tests,
            );
          }
        }
        if (!guarded && !payloadRefutable) context.covered.add(tag);
      } else if (arm.pattern.kind === "wildcard" || arm.pattern.kind === "binding") {
        const bindingName = arm.pattern.kind === "binding" ? arm.pattern.name : undefined;
        if (
          context.declaration &&
          bindingName !== undefined &&
          context.declaration.variants.some((variant) => variant.name === bindingName)
        ) {
          this.fail(
            "bare-variant-pattern",
            `bare variant '${bindingName}' must be written as '.${bindingName}' or '${context.declaration.name}.${bindingName}'`,
            arm.pattern.span,
          );
        }
        if (!guarded) context.catchAll = true;
        if (arm.pattern.kind === "binding") {
          bindings.push({
            local: this.addPatternLocal(arm.pattern.name, context.subject.type, arm.pattern.span),
            fieldIndex: -1,
            type: context.subject.type,
          });
        }
      } else if (arm.pattern.kind === "optional-present") {
        this.fail(
          "optional-pattern-requires-optional",
          `the present pattern requires an optional subject, found '${context.subject.type}'`,
          arm.pattern.span,
        );
      } else {
        this.fail(
          "pattern-type-mismatch",
          `pattern is not valid for '${context.subject.type}'`,
          arm.pattern.span,
        );
      }
      const guard = arm.guard && this.checkExpression(arm.guard);
      if (guard) this.requireType(guard.type, "bool", arm.guard!.span);
      const body = this.checkStatements(arm.body, false, context.expected);
      const armType = this.blockType(body);
      if (context.resultType === undefined || context.resultType === "never")
        context.resultType = armType;
      else if (armType !== "never" && context.resultType !== armType) {
        this.fail(
          "match-arm-type",
          `match arms have types ${context.resultType} and ${armType}`,
          arm.span,
        );
      }
      context.arms.push({ tag, literal, guard, tests, bindings, body, span: arm.span });
    } finally {
      this.scopes.pop();
    }
  }
}
