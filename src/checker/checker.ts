import type { Expression, FunctionDecl, Parameter } from "../ast.ts";
import type { HirExpression, HirFunction, HirStatement, ValueType } from "../hir.ts";
import { contextType, functionType, functionParts, optionalInner, resultParts } from "../types.ts";
import { CheckFailure, type Signature } from "./context.ts";
import { functionTypeMatchesRowPattern } from "./shared.ts";

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
              "invalid-optional-propagation",
              `function '${this.signature.name}' must return an optional type`,
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
        if (!parts || !target || parts.error !== target.error) {
          this.fail(
            "invalid-result-propagation",
            "Result propagation requires a function with a compatible Result error type",
            expression.span,
          );
        }
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
