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
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleElements,
  tupleParts,
  displayType,
} from "../types.ts";
import { numericType } from "../numeric.ts";
import { PRELUDE_NAMES } from "./context.ts";
import {
  type BindingExpressionFlow,
  bindingExpressionFlow,
  erasedFieldType,
  iterableInfo,
  orderedTypeSubstitutions,
  substituteGenericType,
} from "./shared.ts";

import {
  integerPatternInterval,
  patternsExhaustive,
  type IntegerInterval,
} from "./exhaustiveness.ts";
import {
  firstBareLiteral,
  isDefaultedLiteral,
  joinedLeastCommonType,
  literalText,
  recordDefaultedLocal,
} from "./literal-join.ts";
import { spellBinding, uniqueSpelledTypes } from "./spelling.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { ExpressionComprehensionChecker, FOR_PATTERN_ITEM } from "./expression-comprehensions.ts";
import { STATEMENT_IFS } from "./statements.ts";
import { variantShape } from "./gadt.ts";
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
  readonly tuple: boolean;
  readonly boolean: boolean;
  readonly scalar: boolean;
  readonly expected?: ValueType;
  readonly covered: Set<number | string>;
  /** The integers that earlier unguarded literal and range arms match. */
  readonly intervals: IntegerInterval[];
  readonly arms: HirMatchArm[];
  /** Let-else arms already reported as falling through; their value is not coerced. */
  readonly fallsThrough: Set<HirMatchArm>;
  catchAll: boolean;
  readonly resultTypes: ValueType[];
}

function enumPatternAccess(
  declaration: HirEnum,
  field: HirEnum["fields"][number],
  variables: readonly string[],
  substitutions: ReadonlyMap<string, ValueType>,
  valueType: ValueType,
): HirPatternAccessStep {
  return {
    kind: "enum",
    typeIndex: declaration.index,
    fieldIndex: field.index,
    erasedFieldType: erasedFieldType(field.type),
    erasedTypeSubstitutions: orderedTypeSubstitutions(variables, substitutions),
    valueType,
  };
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
        // With an else suite each branch's final statement is the `if`'s
        // value, so the must-use check applies to the `if` instead.
        const valued = expression.elseBody.length > 0;
        if (!valued && !STATEMENT_IFS.has(expression))
          this.fail(
            "type-mismatch",
            "an `if` used as a value needs an `else`, for the value when the condition is false",
            expression.span,
          );
        const thenBody = this.checkConditionalSuite(
          expression.thenBody,
          bindingFlow.whenTrue,
          expected,
          valued,
        );
        const elseBody = valued
          ? this.checkConditionalSuite(expression.elseBody, bindingFlow.whenFalse, expected, true)
          : [];
        this.joinPastDivergingBranch(thenBody, elseBody, bindingFlow);
        if (elseBody.length === 0) {
          return { kind: "if", condition, thenBody, elseBody, type: "void", span: expression.span };
        }
        const thenType = this.blockType(thenBody);
        const elseType = this.blockType(elseBody);
        const type = this.joinMemberType(
          [finalValue(thenBody), finalValue(elseBody)],
          [thenType, elseType],
          "if branches",
          expression.span,
        );
        return {
          kind: "if",
          condition,
          thenBody: this.coerceBlockResult(thenBody, type),
          elseBody: this.coerceBlockResult(elseBody, type),
          type,
          span: expression.span,
        };
      }
      case "for": {
        const value = this.checkExpression(expression.iterable);
        const iterable = this.iterableIterCall(value, expression.iterable) ?? value;
        const info = iterableInfo(iterable, this.iteratorNextFunction());
        if (info?.iteratorKind === "trait" && mutableInner(iterable.type) === undefined)
          this.fail(
            "mutable-receiver-required",
            "iteration requires mutable access to an Iterator implementation",
            expression.iterable.span,
          );
        if (!info) {
          this.fail(
            "unsatisfied-trait-bound",
            `type '${displayType(iterable.type)}' does not implement Iterable, required by the for loop`,
            expression.iterable.span,
          );
        }
        const { iteratorKind, iteratorFunctionIndex, yieldType } = info;
        let sourceBindings = expression.bindings;
        let sourceBody = expression.body;
        if (expression.pattern) {
          // `for P in xs: body` checks as `for item in xs: match item: P => body`
          // once P is known to be irrefutable (06-control-flow.md#r-flow.for.pattern).
          this.requireIrrefutableForPattern(expression.pattern, yieldType);
          const span = expression.pattern.span;
          sourceBindings = [{ name: FOR_PATTERN_ITEM, span }];
          sourceBody = [
            {
              kind: "expression",
              expression: {
                kind: "match",
                subject: { kind: "name", name: FOR_PATTERN_ITEM, span },
                arms: [
                  { pattern: expression.pattern, body: expression.body, span: expression.span },
                ],
                span: expression.span,
              },
              span: expression.span,
            },
          ];
        }
        // The pattern binds readonly names, as a `let` pattern does without `mut`.
        const bindingTypes = expression.pattern
          ? [readonlyType(yieldType)]
          : sourceBindings.length === 1
            ? [yieldType]
            : tupleParts(yieldType);
        if (!bindingTypes || bindingTypes.length !== sourceBindings.length) {
          this.fail(
            "type-mismatch",
            `loop binding has ${sourceBindings.length} names but '${displayType(yieldType)}' yields ${bindingTypes?.length ?? 1} value${bindingTypes?.length === 1 ? "" : "s"}`,
            expression.span,
          );
        }
        const elseBody =
          expression.elseBody.length > 0
            ? this.checkStatements(expression.elseBody, true, expected)
            : [];
        const result = elseBody.length > 0 ? this.blockType(elseBody) : undefined;
        const breaks = isDefaultedLiteral(finalValue(elseBody)) ? [] : undefined;
        this.loopResults.push(result);
        this.loopJoins.push(breaks);
        this.loopBroken.push(false);
        this.scopes.push(new Map());
        let bindings: HirLocal[];
        let body: readonly HirStatement[];
        try {
          const seen = new Set<string>();
          bindings = sourceBindings.map((binding, index) => {
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
            // One binding prints its element the way the iterable prints it.
            if (sourceBindings.length === 1) spellBinding(local, value);
            return local;
          });
          // A loop over bare literals gives its bindings the literals' fix hint.
          const looped = firstBareLiteral(expression.iterable);
          for (const local of looped ? bindings : [])
            if (local.type === "u32")
              recordDefaultedLocal(local, {
                name: local.name,
                literal: literalText(looped!),
                span: looped!.span,
                kind: "loop",
              });
          body = this.checkStatements(sourceBody, false);
        } finally {
          this.scopes.pop();
          this.loopResults.pop();
          this.loopJoins.pop();
          this.loopBroken.pop();
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
          type: this.loopValueType(elseBody, result, breaks, expression.span) ?? "void",
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
        const breaks = isDefaultedLiteral(finalValue(elseBody)) ? [] : undefined;
        this.loopResults.push(result);
        this.loopJoins.push(breaks);
        this.loopBroken.push(false);
        let body: readonly HirStatement[];
        let broken: boolean;
        try {
          body = this.checkConditionalSuite(expression.body, bindingFlow.whenTrue);
        } finally {
          this.loopResults.pop();
          this.loopJoins.pop();
          broken = this.loopBroken.pop()!;
        }
        // Only the literal `true` makes an infinite loop, which completes
        // only through a `break` that targets it (06-control-flow.md#r-flow.while.infinite).
        const infinite =
          expression.condition.kind === "boolean" && expression.condition.value && !broken;
        const type = this.loopValueType(elseBody, result, breaks, expression.span) ?? "void";
        return {
          kind: "while",
          condition,
          body,
          elseBody,
          type: infinite ? "never" : type,
          span: expression.span,
        };
      }
      default:
        return undefined;
    }
  }

  /** A loop's value type: its `else` value joined with its `break` values. */
  private loopValueType(
    elseBody: readonly HirStatement[],
    result: ValueType | undefined,
    breaks: readonly HirExpression[] | undefined,
    span: SourceSpan,
  ): ValueType | undefined {
    if (!breaks || result === undefined) return result;
    const members = [
      { type: result, value: finalValue(elseBody) },
      ...breaks.map((value) => ({ type: value.type, value })),
    ];
    const joined = joinedLeastCommonType(members, { data: this.dataTypes, enums: this.enumTypes });
    if ("type" in joined) return joined.type;
    const listed = uniqueSpelledTypes(members).map(displayType).join(", ");
    this.fail(joined.code, `loop values have no common type: ${listed}`, span);
  }

  /**
   * A branch that diverges is not an incoming path at the merge after the
   * `if` (03-names-and-scopes.md#r-names.definite.diverging), so the names
   * the other branch's condition outcome bound are initialized after it.
   */
  private joinPastDivergingBranch(
    thenBody: readonly HirStatement[],
    elseBody: readonly HirStatement[],
    flow: BindingExpressionFlow,
  ): void {
    const thenDiverges = this.blockType(thenBody) === "never";
    const elseDiverges = elseBody.length > 0 && this.blockType(elseBody) === "never";
    if (thenDiverges === elseDiverges) return;
    for (const name of thenDiverges ? flow.whenFalse : flow.whenTrue) {
      const local = this.currentScope().get(name);
      if (local) this.unavailableBindingLocals.delete(local.index);
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
    valued = false,
  ): HirStatement[] {
    const previous = new Set(this.allowedConditionalBindingLocals);
    for (const name of availableNames) {
      const local = this.resolveLocal(name);
      if (local) this.allowedConditionalBindingLocals.add(local.index);
    }
    try {
      return this.checkStatements(statements, true, expected, valued);
    } finally {
      this.allowedConditionalBindingLocals.clear();
      previous.forEach((index) => this.allowedConditionalBindingLocals.add(index));
    }
  }

  /**
   * `value |> step` (05-expressions.md#pipe-expressions). The value is
   * evaluated first and bound to `_` for the step; a bare step `path` is the
   * call `path(_)`. It lowers to a one-arm match on the value.
   */
  protected checkPipeExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    if (expression.kind !== "pipe") return undefined;
    const value = this.checkExpression(expression.value);
    const local: HirLocal = {
      name: "_",
      type: value.type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span: expression.value.span,
    };
    this.locals.push(local);
    this.scopes.push(new Map([["_", local]]));
    let step: HirExpression;
    try {
      if (expression.bare) {
        const call = this.checkExpression({
          kind: "call",
          callee: expression.step,
          arguments: [{ kind: "name", name: "_", span: expression.value.span }],
          span: expression.step.span,
        });
        // A bare step never suspends (05-expressions.md#r-expr.pipe.bare.no-suspend).
        if (
          suspensionParts(call.type) ||
          traitSuspensionParts(call.type) ||
          storedSuspensionParts(call.type)
        )
          this.fail(
            "suspending-pipe-step",
            "a bare pipe step cannot call a suspending function; write a substitution step such as 'x |> load!(_)'",
            expression.step.span,
          );
        step = this.coerce(call, expected, expression.step.span);
      } else step = this.checkExpression(expression.step, expected);
    } finally {
      this.scopes.pop();
    }
    return {
      kind: "match",
      subject: value,
      representation: "scalar",
      arms: [
        {
          bindings: [{ local, fieldIndex: -1, type: value.type }],
          body: [{ kind: "expression", expression: step, span: step.span }],
          span: expression.span,
        },
      ],
      type: step.type,
      span: expression.span,
    };
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
    // A `mut` data subject matches as its data type; its fields keep their
    // own access (06-control-flow.md#r-flow.match.data.readonly-mut).
    // A generic instantiation matches as its declaration, as enums do above.
    const readonlySubject = readonlyType(subject.type);
    const dataDeclaration = this.dataTypes.get(
      nominalGenericParts(readonlySubject)?.name ?? readonlySubject,
    );
    const optional = optionalInner(subject.type);
    const result = resultParts(subject.type);
    const tuple = tupleElements(subject.type) !== undefined;
    const boolean = subject.type === "bool";
    const scalar =
      numericType(subject.type) !== undefined ||
      new Set<ValueType>(["bool", "char", "string"]).has(subject.type);
    if (
      !declaration &&
      !dataDeclaration &&
      optional === undefined &&
      !result &&
      !scalar &&
      !tuple
    ) {
      this.fail(
        "unsupported-match-subject",
        `matching '${displayType(subject.type)}' is not implemented in this MVP slice`,
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
      tuple,
      boolean,
      scalar,
      expected,
      covered: new Set(),
      intervals: [],
      arms: [],
      fallsThrough: new Set(),
      resultTypes: [],
      catchAll: false,
    };
    // Exhaustiveness counts only the variants that can inhabit the subject's
    // type (13-gadts.md#r-gadt.refine.exhaustive).
    for (const variant of declaration?.variants ?? [])
      if (!this.variantInhabits(declaration!, variant, subject.type))
        context.covered.add(variant.tag);
    const previousReadonly = this.matchSubjectReadonly;
    this.matchSubjectReadonly = mutableInner(subject.type) === undefined;
    try {
      for (const arm of expression.arms) this.checkMatchArm(arm, context);
    } finally {
      this.matchSubjectReadonly = previousReadonly;
    }
    const requiredCases = context.declaration?.variants.length ?? 2;
    const finiteCoverage = Boolean(
      context.declaration || context.optional !== undefined || context.result || context.boolean,
    );
    const coveredCases =
      context.optional !== undefined
        ? [0, 1].filter((tag) => context.covered.has(tag)).length
        : context.covered.size;
    if (
      !context.catchAll &&
      (!finiteCoverage || coveredCases !== requiredCases) &&
      !patternsExhaustive(
        expression.arms.filter((arm) => !arm.guard).map((arm) => arm.pattern),
        subject.type,
        { enums: this.enumTypes, data: this.dataTypes },
      )
    ) {
      const missing = context.declaration
        ? context.declaration.variants
            .filter((variant) => !context.covered.has(variant.tag))
            .map((variant) => variant.name)
        : context.optional !== undefined
          ? [!context.covered.has(0) && ".None", !context.covered.has(1) && ".Some"].filter(Boolean)
          : context.result
            ? [!context.covered.has(0) && ".Ok", !context.covered.has(1) && ".Err"].filter(Boolean)
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
    const resultType =
      context.resultTypes.length === 0
        ? "void"
        : this.joinMemberType(
            context.arms.map((arm) => finalValue(arm.body)),
            context.resultTypes,
            "match arms",
            expression.span,
          );
    return {
      kind: "match",
      subject: context.subject,
      representation: context.declaration
        ? "enum"
        : context.dataDeclaration || context.tuple
          ? "data"
          : context.scalar
            ? "scalar"
            : "erased-variant",
      enumIndex: context.declaration?.index,
      // Each arm's value fits a union-row result by row subsumption.
      arms: context.arms.map((arm) =>
        resultType === "never" || context.fallsThrough.has(arm)
          ? arm
          : { ...arm, body: this.coerceBlockResult(arm.body, resultType) },
      ),
      type: resultType,
      span: expression.span,
    };
  }

  /** Checks a `.Some(pattern)`, `.None`, or `Option.`-qualified arm on an optional subject. */
  private checkOptionalArm(
    pattern: Extract<MatchSourceArm["pattern"], { kind: "variant" }>,
    context: MatchContext,
    guarded: boolean,
    bindings: MatchBinding[],
    tests: MatchTest[],
  ): number {
    const optional = context.optional!;
    if (pattern.variantName !== "Some" && pattern.variantName !== "None")
      this.fail(
        "unknown-variant",
        `enum 'Option' has no variant '${pattern.variantName}'`,
        pattern.span,
      );
    const payloadPatterns =
      pattern.payloadPatterns ??
      pattern.bindings.map((name) =>
        name
          ? { kind: "binding" as const, name, span: pattern.span }
          : { kind: "wildcard" as const, span: pattern.span },
      );
    const some = pattern.variantName === "Some";
    if (payloadPatterns.length !== (some ? 1 : 0))
      this.fail(
        "pattern-arity",
        `variant '${pattern.variantName}' expects ${some ? 1 : 0} payload patterns`,
        pattern.span,
      );
    const fieldName = pattern.bindingNames?.[0];
    if (some && fieldName !== undefined && fieldName !== "value")
      this.fail(
        "unknown-data-field",
        `variant 'Some' has no payload field '${fieldName}'`,
        pattern.span,
      );
    const tag = some ? 1 : 0;
    if (context.covered.has(tag))
      this.fail(
        "unreachable-match-arm",
        `variant '${pattern.variantName}' is already covered`,
        pattern.span,
      );
    let payloadRefutable = false;
    if (some) {
      const payloadPattern = payloadPatterns[0]!;
      if (payloadPattern.kind === "binding") {
        bindings.push({
          local: this.addPatternLocal(payloadPattern.name, optional, payloadPattern.span),
          fieldIndex: 0,
          type: optional,
        });
      } else if (payloadPattern.kind !== "wildcard") {
        payloadRefutable = !this.checkNestedPattern(
          payloadPattern,
          optional,
          [
            {
              kind: "erased-variant",
              typeIndex: -1,
              fieldIndex: 0,
              valueType: optional,
            },
          ],
          bindings,
          tests,
        );
        // `.Some(.None)` plus `.Some(.Some(_))` covers the present case of
        // a nested optional; deeper payload coverage is not tracked.
        const nested = payloadPattern.kind === "variant" ? payloadPattern : undefined;
        const inner = nested ? (nested.payloadPatterns ?? [])[0] : undefined;
        if (
          !guarded &&
          nested &&
          optionalInner(optional) !== undefined &&
          (nested.variantName === "None" ||
            inner === undefined ||
            inner.kind === "binding" ||
            inner.kind === "wildcard")
        ) {
          context.covered.add(`some:${nested.variantName}`);
          if (context.covered.has("some:None") && context.covered.has("some:Some"))
            context.covered.add(1);
        }
      }
    }
    if (!guarded && !payloadRefutable) context.covered.add(tag);
    return tag;
  }

  /** An arm whose tuple pattern is irrefutable covers the rest of a tuple subject. */
  private checkTupleArm(
    pattern: Extract<MatchSourceArm["pattern"], { kind: "tuple" }>,
    context: MatchContext,
    guarded: boolean,
    bindings: MatchBinding[],
    tests: MatchTest[],
  ): void {
    const irrefutable = this.checkTuplePattern(pattern, context.subject.type, bindings, tests);
    if (irrefutable && !guarded) context.catchAll = true;
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
    this.rangePatternConditions = [];
    const outerRefinement = this.beginArmRefinement();
    try {
      this.rejectBareCallPattern(arm.pattern);
      if (context.dataDeclaration && arm.pattern.kind === "data") {
        const irrefutable = this.checkDataPattern(
          arm.pattern,
          context.dataDeclaration,
          context.subject.type,
          [],
          bindings,
          tests,
        );
        if (irrefutable && !guarded) context.catchAll = true;
      } else if (context.tuple && arm.pattern.kind === "tuple") {
        this.checkTupleArm(arm.pattern, context, guarded, bindings, tests);
      } else if (
        context.scalar &&
        ["boolean", "integer", "float", "string", "character"].includes(arm.pattern.kind)
      ) {
        literal = this.checkExpression(
          arm.pattern as Extract<
            Expression,
            { kind: "boolean" | "integer" | "float" | "string" | "character" }
          >,
          context.subject.type,
        );
        this.requireType(literal.type, context.subject.type, arm.pattern.span);
        const key =
          literal.kind === "string"
            ? `string:${literal.bytes.join(",")}`
            : literal.kind === "integer" && literal.wide !== undefined
              ? `${literal.type}:${literal.wide}`
              : `${literal.type}:${"value" in literal ? literal.value : ""}`;
        const interval = integerPatternInterval(arm.pattern, context.subject.type);
        if (context.covered.has(key) || (interval && this.intervalCovered(interval, context)))
          this.fail(
            "unreachable-match-arm",
            "literal pattern is already covered",
            arm.pattern.span,
          );
        if (!guarded) context.covered.add(key);
        if (!guarded && interval) context.intervals.push(interval);
      } else if (arm.pattern.kind === "range") {
        this.checkRangeArm(arm.pattern, context, guarded, bindings);
      } else if (context.declaration && arm.pattern.kind === "variant") {
        tag = this.checkEnumVariantArm(arm.pattern, context, guarded, bindings, tests);
      } else if (
        context.optional !== undefined &&
        arm.pattern.kind === "variant" &&
        (arm.pattern.enumName === undefined || arm.pattern.enumName === "Option")
      ) {
        tag = this.checkOptionalArm(arm.pattern, context, guarded, bindings, tests);
      } else if (
        context.result &&
        (arm.pattern.kind === "result-variant" ||
          (arm.pattern.kind === "variant" &&
            (arm.pattern.enumName === undefined || arm.pattern.enumName === "Result")))
      ) {
        if (arm.pattern.variantName !== "Ok" && arm.pattern.variantName !== "Err")
          this.fail(
            "unknown-variant",
            `enum 'Result' has no variant '${arm.pattern.variantName}'`,
            arm.pattern.span,
          );
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
        // A `void` success has one payload, `()`, which the unit pattern `()`
        // matches as any pattern of `void` does (04-type-system.md#r-types.result.unit-pattern).
        this.requireUnitPayload(payloadPatterns[0], payloadType);
        if (payloadPatterns.length !== 1) {
          this.fail(
            "pattern-arity",
            `${arm.pattern.variantName} expects 1 payload pattern`,
            arm.pattern.span,
          );
        }
        let payloadRefutable = false;
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
        if (!guarded && !payloadRefutable) context.covered.add(tag);
      } else if (arm.pattern.kind === "wildcard" || arm.pattern.kind === "binding") {
        // A top-level `()` arm needs a `void` subject (06-control-flow.md#r-flow.match.unit.type).
        this.requireUnitPayload(arm.pattern, context.subject.type);
        const bindingName = arm.pattern.kind === "binding" ? arm.pattern.name : undefined;
        if (context.optional !== undefined && (bindingName === "Some" || bindingName === "None")) {
          this.fail(
            "bare-variant-pattern",
            `bare variant '${bindingName}' must be written as '.${bindingName}' or 'Option.${bindingName}'`,
            arm.pattern.span,
          );
        }
        if (context.result && (bindingName === "Ok" || bindingName === "Err")) {
          this.fail(
            "bare-variant-pattern",
            `bare variant '${bindingName}' must be written as '.${bindingName}(...)' or 'Result.${bindingName}(...)'`,
            arm.pattern.span,
          );
        }
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
      } else if (
        arm.pattern.kind === "variant" &&
        arm.pattern.enumName === undefined &&
        !context.declaration &&
        !context.result
      ) {
        this.fail(
          "missing-contextual-enum-type",
          `variant pattern '.${arm.pattern.variantName}' requires an enum subject, found '${displayType(context.subject.type)}'`,
          arm.pattern.span,
        );
      } else {
        this.fail(
          "pattern-type-mismatch",
          `pattern is not valid for '${displayType(context.subject.type)}'`,
          arm.pattern.span,
        );
      }
      // The guard and body check under the arm's GADT refinement; the body
      // checks against the refined expected type, and its value converts back
      // (13-gadts.md#r-gadt.refine.scope).
      const { guard, body } = this.withArmRefinement(outerRefinement, () => {
        const guard = this.checkArmGuard(arm);
        const expected =
          context.expected === undefined ? undefined : this.refineArmType(context.expected);
        // An arm's final expression is the value of the enclosing match, even
        // before its common result type has been inferred.
        const body = this.checkStatements(arm.body, false, expected, true);
        if (expected === context.expected || this.blockType(body) === "never")
          return { guard, body };
        return {
          guard,
          body: this.coerceBlockResult(this.coerceBlockResult(body, expected!), context.expected!),
        };
      });
      const checkedArm = { tag, literal, guard, tests, bindings, body, span: arm.span };
      const armType = this.letElseArmType(arm, checkedArm, context);
      context.resultTypes.push(armType);
      context.arms.push(checkedArm);
    } finally {
      this.pendingRefinement = outerRefinement;
      this.scopes.pop();
    }
  }

  /** A variant arm on an enum subject; a GADT variant refines the arm (13-gadts.md#pattern-refinement). */
  private checkEnumVariantArm(
    pattern: Extract<MatchSourceArm["pattern"], { kind: "variant" }>,
    context: MatchContext,
    guarded: boolean,
    bindings: MatchBinding[],
    tests: MatchTest[],
  ): number {
    const declaration = context.declaration!;
    if (pattern.enumName !== undefined && pattern.enumName !== declaration.name) {
      this.fail(
        "pattern-type-mismatch",
        `pattern names '${pattern.enumName}', expected '${declaration.name}'`,
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
    // An impossible GADT variant is reported before coverage, which already
    // counts it (13-gadts.md#r-gadt.refine.impossible).
    const substitutions = this.variantPatternSubstitutions(
      declaration,
      variant,
      context.subject.type,
      [],
      bindings,
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
    const tag = variant.tag;
    let payloadRefutable = false;
    const variables = variantShape(declaration, variant).variables;
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
            (candidate, candidateIndex) => candidateIndex !== fieldIndex && candidate.name === name,
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
      const accessPath = [
        enumPatternAccess(declaration, field, variables, substitutions, fieldType),
      ];
      if (!this.checkNestedPattern(payloadPattern, fieldType, accessPath, bindings, tests))
        payloadRefutable = true;
    });
    if (!guarded && !payloadRefutable) context.covered.add(tag);
    return tag;
  }

  /** A top-level range arm; an empty or already covered range is unreachable (06 Range Patterns). */
  private checkRangeArm(
    pattern: Extract<MatchSourceArm["pattern"], { kind: "range" }>,
    context: MatchContext,
    guarded: boolean,
    bindings: MatchBinding[],
  ): void {
    const local = this.checkRangePattern(pattern, context.subject.type);
    bindings.push({ local, fieldIndex: -1, type: local.type });
    const interval = integerPatternInterval(pattern, context.subject.type)!;
    if (interval.low > interval.high)
      this.fail("unreachable-match-arm", "range pattern matches no value", pattern.span);
    if (this.intervalCovered(interval, context))
      this.fail(
        "unreachable-match-arm",
        "earlier arms already cover every value of this range pattern",
        pattern.span,
      );
    if (!guarded) context.intervals.push(interval);
  }

  /** The arm's guard, after the tests of its range patterns. */
  private checkArmGuard(arm: MatchSourceArm): HirExpression | undefined {
    const source = [...this.rangePatternConditions, ...(arm.guard ? [arm.guard] : [])].reduce<
      Expression | undefined
    >(
      (left, right) =>
        left ? { kind: "binary", operator: "and", left, right, span: right.span } : right,
      undefined,
    );
    if (!source) return undefined;
    const guard = this.checkExpression(source);
    this.requireType(guard.type, "bool", source.span);
    return guard;
  }

  /** Whether earlier unguarded literal and range arms match every integer of `interval`. */
  private intervalCovered(interval: IntegerInterval, context: MatchContext): boolean {
    let next = interval.low;
    const sorted = [...context.intervals].sort((left, right) =>
      left.low < right.low ? -1 : left.low > right.low ? 1 : 0,
    );
    for (const covering of sorted) {
      if (covering.low > next) break;
      if (covering.high >= next) next = covering.high + 1n;
    }
    return next > interval.high;
  }

  /**
   * An arm's type. A let-else block must diverge; one that may complete is
   * reported without stopping, so the row inference of the enclosing
   * function still sees the whole body, and then counts as `never`
   * (06-control-flow.md#r-flow.let.else.falls-through).
   */
  private letElseArmType(
    arm: MatchSourceArm,
    checked: HirMatchArm,
    context: MatchContext,
  ): ValueType {
    const type = this.blockType(checked.body);
    if (!arm.letElse || type === "never") return type;
    context.fallsThrough.add(checked);
    this.diagnostics.push({
      code: "let-else-falls-through",
      message:
        "a let-else block must leave the enclosing block, with return, break, continue, or a call that never returns",
      span: arm.span,
    });
    return "never";
  }

  /** A suite whose final value is coerced to `type`, as a union-row site needs. */
  protected coerceBlockResult(
    body: readonly HirStatement[],
    type: ValueType,
  ): readonly HirStatement[] {
    const last = body.at(-1);
    if (last?.kind !== "expression" || last.expression.type === type) return body;
    return [
      ...body.slice(0, -1),
      { ...last, expression: this.requireCoercion(last.expression, type, last.span) },
    ];
  }
}

/** The value a block ends with, a member of the join of its `if` or `match`. */
function finalValue(body: readonly HirStatement[]): HirExpression | undefined {
  const last = body.at(-1);
  return last?.kind === "expression" ? last.expression : undefined;
}
