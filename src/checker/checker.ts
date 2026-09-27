import type { Expression, FunctionDecl, Parameter } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, HirFunction, HirLocal, HirStatement, ValueType } from "../hir.ts";
import {
  contextType,
  functionType,
  functionParts,
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
} from "../types.ts";
import { CheckFailure, type Signature } from "./context.ts";
import { functionTypeMatchesRowPattern, matchTraitImplementation } from "./shared.ts";
import { STANDARD_FROM } from "./standard-traits.ts";

import { ExpressionControlChecker } from "./expression-control.ts";
export class FunctionChecker extends ExpressionControlChecker {
  protected checkExpression(expression: Expression, expected?: ValueType): HirExpression {
    const value = this.checkExpressionRaw(expression, expected);
    return this.coerce(value, expected, expression.span);
  }

  protected checkExpressionRaw(expression: Expression, expected?: ValueType): HirExpression {
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
      this.checkClosureExpression(expression, expected);
    if (checked !== undefined) return checked;
    throw new Error(`unsupported expression '${expression.kind}'`);
  }

  /**
   * 05 Propagation: an error of type `source` reaches the enclosing error type
   * `target` in one step, by one assignability rule or otherwise by one call of
   * `target`'s `std.convert.From[source]` implementation. The `?` is checked
   * as `match operand: Ok($ok) => $ok; Err($err) => return Err(conversion)`.
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
        `error type '${source}' is neither assignable to '${target}' nor converted by an implementation of From[${readonlyType(source)}] for '${target}'; implement it or map the error explicitly`,
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
                    callee: { kind: "name", name: "Err", span },
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

  /** Whether `target` implements the standard `From[source]` (spec/09-traits.md#conversion-trait). */
  private hasStandardFrom(source: ValueType, target: ValueType): boolean {
    const localName = [...this.imports].find(([, imported]) => imported === STANDARD_FROM)?.[0];
    const trait = localName === undefined ? undefined : this.traitTypes.get(localName);
    if (!trait) return false;
    const argument = readonlyType(source);
    return this.implementations.some((implementation) =>
      matchTraitImplementation(implementation, trait.index, readonlyType(target), [argument]),
    );
  }

  protected checkClosureExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "propagate": {
        const operand = this.checkExpression(expression.operand);
        const optional = optionalInner(operand.type);
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
            `? requires an optional or Result operand, found '${operand.type}'`,
            expression.span,
          );
        if (!target) {
          this.fail(
            "invalid-result-propagation",
            "Result propagation requires a function with a compatible Result error type",
            expression.span,
          );
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
      case "closure": {
        const expectedCallable = expected ? functionParts(expected) : undefined;
        const suspending = expression.suspending === true;
        if (expectedCallable && expectedCallable.suspending !== suspending) {
          this.fail(
            "type-mismatch",
            `expected ${expected}, found a ${suspending ? "suspending" : "non-suspending"} closure`,
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
          const inferred = expectedCallable?.parameters[index];
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
          : expectedCallable?.result;
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
          suspending,
          genericParameters: [],
          genericBounds: [],
          parameters,
          result: provisionalResultRef,
          requirements: expression.requirements ?? [],
          body: expression.body,
          span: expression.span,
        };
        let requirements = expression.requirements;
        if (requirements === undefined || inferResult) {
          const discoverySignature: Signature = {
            name: baseDeclaration.name,
            index: closureIndex,
            suspending,
            genericParameters: [],
            genericBounds: [],
            rowParameters: [],
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
            this.implementations,
            this.inherentMethods,
            true,
            false,
            this.closures,
            true,
            this.visibleCaptureSources(),
            closureIndex,
            this.visibleProviders(),
            requirements === undefined,
            inferResult,
            this.pendingRecursiveClosure,
            this.imports,
            this.globals,
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
          genericParameters: [],
          genericBounds: [],
          rowParameters: [],
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
          this.implementations,
          this.inherentMethods,
          true,
          false,
          this.closures,
          true,
          this.visibleCaptureSources(),
          closureIndex,
          this.visibleProviders(),
          false,
          false,
          this.pendingRecursiveClosure,
          this.imports,
          this.globals,
        ).check();
        if (!checked.function) {
          this.diagnostics.push(...checked.diagnostics);
          throw new CheckFailure("closure checking failed");
        }
        this.closures[closureIndex] = checked.function;
        const captures = checked.function.captures.map((capture) =>
          this.captureValue(capture.source, expression.span),
        );
        const type = functionType(parameterTypes, result, requirements, false, suspending);
        if (expected && expected !== type && !functionTypeMatchesRowPattern(expected, type)) {
          this.fail("type-mismatch", `expected ${expected}, found ${type}`, expression.span);
        }
        return { kind: "closure", closureIndex, captures, type, span: expression.span };
      }
      case "provider-use": {
        const key = this.canonicalProviderKey(expression.key, expression.span);
        const provider = this.resolveProvider(key, expression.span);
        if (!provider)
          this.fail(
            "missing-requirement",
            `provider '${key}' is not available in the current context`,
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
