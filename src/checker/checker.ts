import type {
  AssignmentStatement,
  Expression,
  FunctionDecl,
  Parameter,
  Statement,
} from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, HirFunction, HirLocal, HirStatement, ValueType } from "../hir.ts";
import {
  contextType,
  functionType,
  functionParts,
  nominalGenericParts,
  optionalInner,
  optionalType,
  readonlyType,
  resultParts,
  resultType,
  displayType,
} from "../types.ts";
import { CheckFailure, type FunctionCheckResult, type Signature } from "./context.ts";
import { functionTypeMatchesRowPattern, matchTraitImplementation } from "./shared.ts";
import { STANDARD_FROM } from "./standard-traits.ts";
import { mismatchMessage } from "./row-rules.ts";

import { ExpressionControlChecker } from "./expression-control.ts";
import { fallbackLiteralHint } from "./literal-join.ts";
import {
  chosenWidth,
  forceLiterals,
  type LiteralWidths,
  markState,
  namedWidths,
  restoreState,
  statementLiterals,
} from "./literal-retry.ts";
const USIZE = /\busize\b/;

export class FunctionChecker extends ExpressionControlChecker {
  private contextualHintDepth = 0;

  protected checkExpression(expression: Expression, expected?: ValueType): HirExpression {
    const value = this.checkExpressionRaw(expression, expected);
    return this.coerce(value, expected, expression.span);
  }

  /** Supplies missing type information without requiring its outer permission. */
  protected checkExpressionHint(expression: Expression, expected: ValueType): HirExpression {
    this.contextualHintDepth += 1;
    try {
      return this.checkExpressionRaw(expression, expected);
    } finally {
      this.contextualHintDepth -= 1;
    }
  }

  protected get expectedIsHint(): boolean {
    return this.contextualHintDepth > 0;
  }

  protected checkExpressionRaw(expression: Expression, expected?: ValueType): HirExpression {
    const prechecked = this.prechecked.get(expression);
    if (prechecked) return prechecked;
    return this.checkExpressionKind(expression, expected);
  }

  private literalTrialDepth = 0;

  /** The check, with the join model's literal hint added where no site gave one. */
  override check(): FunctionCheckResult {
    const result = super.check();
    // GADT arms' existential bounds read their evidence locals
    // (13-gadts.md#r-gadt.runtime.evidence.match).
    if (result.function && this.existentialBoundLocals.length > 0)
      return {
        ...result,
        function: { ...result.function, existentialBounds: this.existentialBoundLocals },
      };
    if (result.function || result.diagnostics.length === 0) return result;
    const scope = [...this.locals, ...this.availableCaptures.values()];
    const last = result.diagnostics.at(-1)!;
    const hinted = fallbackLiteralHint(last, this.declaration.body, scope);
    if (hinted === last) return result;
    return { ...result, diagnostics: [...result.diagnostics.slice(0, -1), hinted] };
  }

  /**
   * The join model's one retry per statement (types.literal.local.join): a
   * statement that fails with its literals at their default types is
   * checked again with them at each width the failure names, and the one
   * width that makes it check is taken. Nested statements never retry
   * inside a trial, so the work stays linear in the statement's size.
   */
  protected override checkStatement(
    statement: Statement,
    expected?: ValueType,
    valueContext?: boolean,
  ): HirStatement {
    return this.retryStatement(statement, () => {
      const checked = super.checkStatement(statement, expected, valueContext);
      // A final value whose literal default its expected type rejects is a
      // failure of this statement, so the retry sees it.
      const value = checked.kind === "expression" ? checked.expression : undefined;
      if (
        value &&
        expected &&
        expected !== "void" &&
        USIZE.test(value.type) &&
        !USIZE.test(expected)
      )
        this.requireCoercion(value, expected, checked.span);
      return checked;
    });
  }

  protected override checkDestructuring(
    statement: Extract<Statement, { kind: "tuple-binding" | "pattern-binding" }>,
  ): HirStatement[] {
    return this.retryStatement(statement, () => super.checkDestructuring(statement));
  }

  protected override checkCompoundAssignment(statement: AssignmentStatement): HirStatement[] {
    return this.retryStatement(statement, () => super.checkCompoundAssignment(statement));
  }

  /**
   * The join model's one retry per statement (types.literal.local.join): a
   * statement that fails with its literals at their default types is
   * checked again with them at each width the failure names, and the one
   * width that makes it check is taken. Nested statements never retry
   * inside a trial, so the work stays linear in the statement's size.
   */
  /** The literal hint for a failed statement, from any binding it names. */
  private hintStatement(statement: Statement, from: number): void {
    const last = this.diagnostics.length - 1;
    if (last < from) return;
    const scope = [...this.locals, ...this.availableCaptures.values()];
    this.diagnostics[last] = fallbackLiteralHint(this.diagnostics[last]!, statement, scope, true);
  }

  private retryStatement<T>(statement: Statement, check: () => T): T {
    if (this.literalTrialDepth > 0) return check();
    const mark = markState(
      [
        this.diagnostics,
        this.locals,
        this.closures,
        this.inferredRequirements,
        this.inferredReturns,
        this.inferredPropagations,
      ],
      [...this.scopes, this.captures, this.prechecked, this.globals],
    );
    try {
      return check();
    } catch (error) {
      if (!(error instanceof CheckFailure)) throw error;
      this.hintStatement(statement, mark.lengths[0]![1]);
      const failed = this.diagnostics.slice(mark.lengths[0]![1]);
      const found = statementLiterals(statement);
      const widths = namedWidths(found, failed);
      if (widths.length === 0) throw error;
      restoreState(mark);
      const fits: LiteralWidths[] = [];
      for (const width of widths) {
        const undo = forceLiterals(found, width);
        this.literalTrialDepth += 1;
        try {
          check();
          fits.push(width);
        } catch (trial) {
          if (!(trial instanceof CheckFailure)) throw trial;
        } finally {
          this.literalTrialDepth -= 1;
          undo();
          restoreState(mark);
        }
      }
      const chosen = chosenWidth(found, fits);
      if (chosen === undefined) {
        this.diagnostics.push(...failed);
        throw error;
      }
      const undo = forceLiterals(found, chosen);
      try {
        return check();
      } finally {
        undo();
      }
    }
  }

  private checkExpressionKind(expression: Expression, expected?: ValueType): HirExpression {
    const checked =
      this.checkLiteralExpression(expression, expected) ??
      this.checkOperatorExpression(expression, expected) ??
      this.checkCallExpression(expression, expected) ??
      this.checkSuspendingCallExpression(expression, expected) ??
      this.checkDataExpression(expression, expected) ??
      this.checkAccessExpression(expression, expected) ??
      this.checkComprehensionExpression(expression, expected) ??
      this.checkControlExpression(expression, expected) ??
      this.checkMatchExpression(expression, expected) ??
      this.checkPipeExpression(expression, expected) ??
      this.checkMethodReference(expression, expected) ??
      this.checkClosureExpression(expression, expected);
    if (checked !== undefined) return checked;
    throw new Error(`unsupported expression '${expression.kind}'`);
  }

  /**
   * 05 Propagation: an error of type `source` reaches the enclosing error type
   * `target` in one step, by one assignability rule or otherwise by one call of
   * `target`'s `std.convert.From[source]` implementation. The `?` is checked
   * as `match operand: .Ok($ok) => $ok; .Err($err) => return .Err(conversion)`.
   */
  private checkConvertingPropagation(
    expression: Extract<Expression, { kind: "propagate" }>,
    operand: HirExpression,
    source: ValueType,
    target: ValueType,
    expected?: ValueType,
  ): HirExpression {
    const span = expression.span;
    const errorName: Expression = { kind: "name", name: "$err", span };
    let conversion: Expression | undefined;
    if (this.assignableInOneStep(source, target, span)) conversion = errorName;
    else if (this.hasStandardFrom(source, target)) {
      const nominal = nominalGenericParts(target);
      conversion = {
        kind: "call",
        callee: {
          kind: "qualified-name",
          owner: nominal?.name ?? target,
          ownerTypeArguments: nominal?.arguments.map((argument) => ({ name: argument, span })),
          name: "from",
          span,
        },
        arguments: [errorName],
        span,
      };
    }
    if (!conversion)
      this.fail(
        "invalid-result-propagation",
        `error type '${displayType(source)}' is neither assignable to '${displayType(target)}' nor converted by From[${displayType(readonlyType(source))}] for '${displayType(target)}'; implement it or map the error explicitly`,
        span,
      );
    const subject: HirLocal = {
      name: "$propagated",
      type: operand.type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span,
    };
    this.locals.push(subject);
    this.scopes.push(new Map([[subject.name, subject]]));
    let checked: HirExpression;
    try {
      checked = this.checkExpression(
        {
          kind: "match",
          subject: { kind: "name", name: subject.name, span },
          arms: [
            {
              pattern: {
                kind: "result-variant",
                variantName: "Ok",
                bindings: ["$ok"],
                payloadPatterns: [{ kind: "binding", name: "$ok", span }],
                span,
              },
              body: [{ kind: "expression", expression: { kind: "name", name: "$ok", span }, span }],
              span,
            },
            {
              pattern: {
                kind: "result-variant",
                variantName: "Err",
                bindings: ["$err"],
                payloadPatterns: [{ kind: "binding", name: "$err", span }],
                span,
              },
              body: [
                {
                  kind: "return",
                  value: {
                    kind: "call",
                    callee: { kind: "contextual-variant", name: "Err", span },
                    arguments: [conversion],
                    span,
                  },
                  span,
                },
              ],
              span,
            },
          ],
          span,
        },
        expected,
      );
    } finally {
      this.scopes.pop();
    }
    if (checked.kind !== "match") throw new Error("converting propagation must check as a match");
    return { ...checked, subject: operand };
  }

  /** One rule of 04 Assignability And Coercion, never two (TQ-14). */
  private assignableInOneStep(source: ValueType, target: ValueType, span: SourceSpan): boolean {
    const inner = optionalInner(target);
    if (inner !== undefined && source !== inner && optionalInner(source) === undefined)
      return false;
    const probe: HirLocal = {
      name: "$probe",
      type: source,
      index: -1,
      mutable: false,
      parameter: false,
      span,
    };
    const coerced = this.coerce({ kind: "local", local: probe, type: source, span }, target, span);
    return coerced.type === target;
  }

  /** Whether `target` implements the standard `From[source]` (spec/lang/09-traits.md#conversion-trait). */
  private hasStandardFrom(source: ValueType, target: ValueType): boolean {
    // The prelude uses `std.convert`, so `From` is declared whether or not
    // the module imports it (spec/lang/10-modules.md#r-module.prelude.question-from).
    const trait = [...this.traitTypes.values()].find(
      (candidate) => candidate.standardName === STANDARD_FROM,
    );
    if (!trait) return false;
    const argument = readonlyType(source);
    return this.implementations.some((implementation) =>
      matchTraitImplementation(implementation, trait.index, readonlyType(target), [argument]),
    );
  }

  // While a result is being inferred, `?` converts nothing and its operand
  // joins the inferred result (05-expressions.md#r-expr.try.convert.inferred-closure).
  private checkInferredPropagation(expression: Expression, operand: HirExpression): HirExpression {
    const optional = optionalInner(operand.type);
    const parts = optional === undefined ? resultParts(operand.type) : undefined;
    if (optional === undefined && !parts)
      this.fail(
        "invalid-result-propagation",
        `? requires an optional or Result operand, found '${displayType(operand.type)}'`,
        expression.span,
      );
    this.inferredPropagations.push({
      ...(parts ? { error: parts.error } : {}),
      span: expression.span,
    });
    const payloadType = optional ?? parts!.ok;
    return {
      kind: "propagate",
      operand,
      payloadType,
      successTag: optional === undefined ? 0 : 1,
      returnType: operand.type,
      type: payloadType,
      span: expression.span,
    };
  }

  /**
   * The operand of `x?`. An expected type `T` for `x?` guides the operand's
   * inference as `Result[T, E]`, with the enclosing function's error type
   * `E`, or as `T?`, as in `let ports: List[i32] = items.collect()?`. It is
   * no coercion, so an error `From` converts still reaches the operand.
   */
  private checkPropagationOperand(operand: Expression, expected?: ValueType): HirExpression {
    const result = this.signature.result;
    const target = resultParts(result);
    const guide =
      expected === undefined || this.inferResult
        ? undefined
        : target
          ? resultType(expected, target.error)
          : optionalInner(result) !== undefined
            ? optionalType(expected)
            : undefined;
    return guide === undefined
      ? this.checkExpression(operand)
      : this.checkExpressionRaw(operand, guide);
  }

  /**
   * A closure sees the enclosing function's generic parameters and bounds,
   * so its body calls methods through those bounds
   * (07-functions.md#method-references), and its declared row is compared
   * with its own `$.with` keys
   * (11-requirements-and-suspension.md#r-req.with.collision.closure). The
   * prototype passes a suspending closure no bound dictionaries.
   */
  private enclosingGenerics(
    suspending: boolean,
  ): Pick<Signature, "genericParameters" | "genericBounds" | "rowParameters"> {
    return {
      genericParameters: this.signature.genericParameters,
      genericBounds: suspending ? [] : this.signature.genericBounds,
      rowParameters: this.signature.rowParameters,
    };
  }

  private checkPropagationExpression(
    expression: Extract<Expression, { kind: "propagate" }>,
    expected?: ValueType,
  ): HirExpression {
    if (this.deferDepth > 0)
      this.fail("defer-control-flow", "a defer suite cannot propagate with ?", expression.span);
    const operand = this.checkPropagationOperand(expression.operand, expected);
    const optional = optionalInner(operand.type);
    if (this.inferResult) return this.checkInferredPropagation(expression, operand);
    if (optional !== undefined) {
      if (optionalInner(this.signature.result) === undefined) {
        this.fail(
          "invalid-result-propagation",
          `optional propagation requires '${this.signature.name}' to return an optional type`,
          expression.span,
        );
      }
      return {
        kind: "propagate",
        operand,
        payloadType: optional,
        successTag: 1,
        returnType: this.signature.result,
        type: optional,
        span: expression.span,
      };
    }
    const parts = resultParts(operand.type);
    const target = resultParts(this.signature.result);
    if (!parts)
      this.fail(
        "invalid-result-propagation",
        `? requires an optional or Result operand, found '${displayType(operand.type)}'`,
        expression.span,
      );
    if (!target) {
      const error = displayType(parts.error);
      const result = `Result[${displayType(this.signature.result)}, ${error}]`;
      const note =
        error === "ConsoleError"
          ? `change the result type to '${result}' (the error type of 'write_line!'); for Console output without error handling, use 'println'`
          : `change the result type to '${result}' to propagate the '${error}' error`;
      const message = "Result propagation requires a function with a compatible Result error type";
      this.diagnostics.push({
        code: "invalid-result-propagation",
        message,
        notes: [note],
        span: expression.span,
      });
      throw new CheckFailure(message);
    }
    if (parts.error !== target.error)
      return this.checkConvertingPropagation(
        expression,
        operand,
        parts.error,
        target.error,
        expected,
      );
    return {
      kind: "propagate",
      operand,
      payloadType: parts.ok,
      successTag: 0,
      returnType: this.signature.result,
      type: parts.ok,
      span: expression.span,
    };
  }

  protected checkClosureExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "propagate":
        return this.checkPropagationExpression(expression, expected);
      case "closure": {
        // An optional function type checks the closure against its payload;
        // the caller's coercion then injects it into the optional.
        const optionalCallable = expected ? optionalInner(expected) : undefined;
        if (optionalCallable && functionParts(readonlyType(optionalCallable)))
          return this.checkClosureExpression(expression, optionalCallable);
        const expectedCallable = expected ? functionParts(readonlyType(expected)) : undefined;
        // Only solved positions of an argument's expected type type the closure.
        const solved = this.takeSolvedPositions();
        // A trailing block passed for an `fn!` parameter is a suspending
        // closure (spec/lang/07-functions.md#r-fn.trailing.suspending).
        const suspending =
          expression.suspending === true ||
          (expression.trailing === true && expectedCallable?.suspending === true);
        if (expectedCallable && expectedCallable.suspending !== suspending) {
          this.fail(
            "type-mismatch",
            `expected ${displayType(expected!)}, found a ${suspending ? "suspending" : "non-suspending"} closure`,
            expression.span,
          );
        }
        if (
          expectedCallable &&
          expectedCallable.parameters.length !== expression.parameters.length
        ) {
          this.fail(
            "argument-count",
            `expected a closure with ${expectedCallable.parameters.length} parameters, found ${expression.parameters.length}`,
            expression.span,
          );
        }
        const parameterTypes = expression.parameters.map((parameter, index) => {
          if (parameter.type) return this.resolveType(parameter.type);
          const inferred = solved(expectedCallable?.parameters[index]);
          if (!inferred)
            this.fail(
              "closure-parameter-needs-annotation",
              `closure parameter '${parameter.name}' needs a type annotation or an expected function type`,
              parameter.span,
            );
          return inferred;
        });
        let result = expression.result
          ? this.resolveType(expression.result)
          : solved(expectedCallable?.result);
        const inferResult = result === undefined;
        const provisionalResult = result ?? "void";
        const parameters: Parameter[] = expression.parameters.map((parameter, index) => ({
          name: parameter.name,
          type: parameter.type ?? { name: parameterTypes[index]!, span: parameter.span },
          span: parameter.span,
        }));
        const provisionalResultRef = expression.result ?? {
          name: provisionalResult,
          span: expression.span,
        };
        const closureIndex = this.closures.length;
        this.closures.push(undefined as unknown as HirFunction);
        const baseDeclaration: FunctionDecl = {
          kind: "function",
          name: `$closure${closureIndex}`,
          ...(this.declaration.testOnly ? { testOnly: true } : {}),
          ...(this.declaration.standard ? { standard: true } : {}),
          ...(this.declaration.compilerGenerated ? { compilerGenerated: true } : {}),
          suspending,
          genericParameters: [],
          genericBounds: [],
          parameters,
          result: provisionalResultRef,
          requirements: expression.requirements ?? [],
          body: expression.body,
          span: expression.span,
        };
        const enclosingGenerics = this.enclosingGenerics(suspending);
        let requirements: readonly string[] | undefined = expression.requirements?.map((key) =>
          this.canonicalProviderKey(key, expression.span),
        );
        if (requirements === undefined || inferResult) {
          const discoverySignature: Signature = {
            name: baseDeclaration.name,
            index: closureIndex,
            suspending,
            ...enclosingGenerics,
            parameters: parameterTypes,
            parameterNames: parameters.map((parameter) => parameter.name),
            defaultFunctionNames: parameters.map(() => undefined),
            variadic: false,
            result: provisionalResult,
            requirements: [],
            span: expression.span,
          };
          const discovery = new FunctionChecker(
            baseDeclaration,
            discoverySignature,
            this.signatures,
            this.dataTypes,
            this.enumTypes,
            this.traitTypes,
            this.allImplementations,
            this.allInherentMethods,
            true,
            false,
            this.closures,
            true,
            this.visibleCaptureSources(),
            closureIndex,
            new Map(),
            requirements === undefined,
            inferResult,
            this.pendingRecursiveClosure,
            this.imports,
            this.globals,
            this.visibleLocalImplementations(),
            this.integrationTest,
          ).check();
          this.closures.length = closureIndex;
          if (!discovery.function) {
            this.diagnostics.push(...discovery.diagnostics);
            throw new CheckFailure("closure requirement inference failed");
          }
          if (requirements === undefined) requirements = discovery.function.requirements;
          if (inferResult) result = discovery.function.result;
          this.closures.push(undefined as unknown as HirFunction);
        }
        if (!result)
          this.fail(
            "closure-result-needs-annotation",
            "a closure result could not be inferred",
            expression.span,
          );
        const declaration: FunctionDecl = {
          ...baseDeclaration,
          result: expression.result ?? { name: result, span: expression.span },
          requirements,
        };
        const signature: Signature = {
          name: declaration.name,
          index: closureIndex,
          suspending,
          ...enclosingGenerics,
          parameters: parameterTypes,
          parameterNames: parameters.map((parameter) => parameter.name),
          defaultFunctionNames: parameters.map(() => undefined),
          variadic: false,
          result,
          requirements,
          span: expression.span,
        };
        const checked = new FunctionChecker(
          declaration,
          signature,
          this.signatures,
          this.dataTypes,
          this.enumTypes,
          this.traitTypes,
          this.allImplementations,
          this.allInherentMethods,
          true,
          false,
          this.closures,
          true,
          this.visibleCaptureSources(),
          closureIndex,
          new Map(),
          false,
          false,
          this.pendingRecursiveClosure,
          this.imports,
          this.globals,
          this.visibleLocalImplementations(),
          this.integrationTest,
        ).check();
        if (!checked.function) {
          this.diagnostics.push(...checked.diagnostics);
          throw new CheckFailure("closure checking failed");
        }
        this.closures[closureIndex] = checked.function;
        if (checked.hasPanicDetail) this.hasPanicDetail = true;
        const captures = checked.function.captures.map((capture) =>
          this.captureValue(capture.source, expression.span),
        );
        const callableType = functionType(parameterTypes, result, requirements, false, suspending);
        const type = callableType;
        const expectedCallableType = expected && solved(readonlyType(expected));
        if (
          expectedCallableType &&
          expectedCallableType !== callableType &&
          !functionTypeMatchesRowPattern(expectedCallableType, callableType)
        ) {
          this.fail(
            "type-mismatch",
            `expected ${displayType(expected)}, found ${displayType(type)}`,
            expression.span,
          );
        }
        // A closure's row keeps each key its body uses, even inside a
        // `$.with` block, so an expected row without that key rejects the
        // closure itself (11-requirements-and-suspension.md#r-req.row.omitted.outer-scope).
        const expectedRow =
          expectedCallableType && functionParts(expectedCallableType)?.requirements;
        if (
          expectedRow &&
          !expectedRow.some((key) => key.startsWith("row:")) &&
          requirements.some((key) => !expectedRow.includes(key))
        )
          this.fail("type-mismatch", mismatchMessage(type, expected!), expression.span);
        return { kind: "closure", closureIndex, captures, type, span: expression.span };
      }
      case "provider-use": {
        const key = this.canonicalProviderKey(expression.key, expression.span);
        const provider = this.resolveProvider(key, expression.span);
        if (!provider)
          this.fail(
            "missing-requirement",
            `provider '${displayType(key)}' is not available in the current context`,
            expression.span,
          );
        return provider;
      }
      case "provider-context": {
        const { entries, providers } = this.checkProviderEntries(expression.entries);
        const keys = [...providers.keys()].sort();
        return {
          kind: "provider-context",
          keys,
          entries,
          type: contextType(keys),
          span: expression.span,
        };
      }
      case "provider-with": {
        if (this.declaration.defaultContext)
          this.fail(
            "requirement-in-default",
            "a default must be requirement-free: it cannot enter a provider scope",
            expression.span,
          );
        const { entries, providers } = this.checkProviderEntries(expression.entries);
        this.providerScopes.push(providers);
        let body: readonly HirStatement[];
        try {
          body = this.checkStatements(expression.body, true, expected, true);
        } finally {
          this.providerScopes.pop();
        }
        return {
          kind: "provider-with",
          entries,
          body,
          type: this.blockType(body),
          span: expression.span,
        };
      }
      default:
        return undefined;
    }
  }
}
