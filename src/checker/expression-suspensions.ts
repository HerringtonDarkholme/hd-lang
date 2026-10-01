import type { Expression } from "../ast.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import {
  functionParts,
  storedSuspensionParts,
  suspensionParts,
  suspensionType,
  traitSuspensionParts,
  tupleType,
} from "../types.ts";
import { ALL_COMBINATOR, RACE_INTRINSIC } from "./standard-traits.ts";
import { substituteGenericType } from "./shared.ts";

import { OperatorCallChecker } from "./operator-calls.ts";
export abstract class ExpressionSuspensionChecker extends OperatorCallChecker {
  /**
   * `all!(e_1, ..., e_n)`, where each `e_i` is a `mut Suspend[X_i]`, has type
   * `(X_1, ..., X_n)` (11-requirements-and-suspension.md#typing-all). Its
   * polling body is a compiler intrinsic that the prototype does not emit.
   */
  private checkAllCombinator(
    expression: Extract<Expression, { kind: "suspend-call" }>,
  ): HirExpression {
    if (expression.typeArguments)
      this.fail("type-mismatch", "all! has no type parameters", expression.span);
    if (expression.argumentNames?.some((name) => name !== undefined))
      this.fail("type-mismatch", "all! takes positional arguments only", expression.span);
    if (expression.argumentSpreads?.some(Boolean))
      this.fail(
        "type-mismatch",
        "all! takes direct positional arguments, not a spread",
        expression.span,
      );
    const results = expression.arguments.map((argument) => {
      const child = this.checkExpression(argument);
      const stored = storedSuspensionParts(child.type);
      const result =
        suspensionParts(child.type)?.result ??
        traitSuspensionParts(child.type)?.result ??
        (stored?.mutable ? stored.result : undefined);
      if (result === undefined)
        this.fail(
          "type-mismatch",
          `an argument of all! must be a mut Suspend[T], such as a cold call, found '${child.type}'`,
          argument.span,
        );
      return result;
    });
    const message = "std.task.all! has no run time in the prototype yet";
    return {
      kind: "panic",
      message: this.checkExpression({ kind: "string", value: message, span: expression.span }),
      unsupported: { code: "unsupported-task-combinator", message },
      type: tupleType(results),
      span: expression.span,
    };
  }

  protected checkSuspendingCallExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "suspend-call": {
        if (this.deferDepth > 0) {
          this.fail("suspending-defer", "a defer suite cannot make a bang call", expression.span);
        }
        if (!this.declaration.suspending) {
          this.fail(
            "bang-call-outside-suspension",
            "a bang call requires a suspending driver context",
            expression.span,
          );
        }
        if (
          (expression.callee.kind === "member" && !expression.callee.parenthesized) ||
          expression.callee.kind === "qualified-name"
        ) {
          const suspension = this.checkExpression({
            kind: "call",
            callee: expression.callee,
            typeArguments: expression.typeArguments,
            arguments: expression.arguments,
            argumentNames: expression.argumentNames,
            argumentSpreads: expression.argumentSpreads,
            span: expression.span,
          });
          if (suspension.kind === "suspend-construct") {
            const result = suspensionParts(suspension.type)!.result;
            const signature = [...this.signatures.values()].find(
              (candidate) => candidate.index === suspension.functionIndex,
            );
            return {
              kind: "suspend-drive",
              functionIndex: suspension.functionIndex,
              suspension,
              erasedResultType: signature?.genericParameters.length ? signature.result : undefined,
              type: result,
              span: expression.span,
            };
          }
          if (suspension.kind === "trait-suspend-construct") {
            const parts = traitSuspensionParts(suspension.type)!;
            return {
              kind: "trait-suspend-drive",
              traitIndex: parts.traitIndex,
              methodIndex: parts.methodIndex,
              suspension,
              type: parts.result,
              span: expression.span,
            };
          }
          this.fail(
            "not-suspending",
            `method '${expression.callee.name}' is not suspending`,
            expression.span,
          );
        }
        if (
          expression.callee.kind === "name" &&
          !this.resolveLocal(expression.callee.name) &&
          !this.availableCaptures.has(expression.callee.name) &&
          !this.resolveGlobal(expression.callee.name)
        ) {
          if (this.globals.has(expression.callee.name)) {
            this.fail(
              "binding-not-yet-visible",
              `module binding '${expression.callee.name}' is not visible before its binding point`,
              expression.callee.span,
            );
          }
          const signature = this.visibleSignature(expression.callee.name);
          if (!signature && this.imports.get(expression.callee.name) === ALL_COMBINATOR)
            return this.checkAllCombinator(expression);
          if (!signature)
            this.failUnknownName(
              expression.callee.name,
              `unknown function '${expression.callee.name}'`,
              expression.callee.span,
            );
          if (!signature.suspending)
            this.fail(
              "not-suspending",
              `function '${signature.name}' is not suspending`,
              expression.span,
            );
          const checkedArguments = this.checkSignatureArguments(expression, signature, expected);
          const { substitutions, rowSubstitutions } = checkedArguments;
          const unresolved = signature.genericParameters.filter(
            (parameter) => !substitutions.has(parameter),
          );
          if (unresolved.length > 0)
            this.fail(
              "unresolved-generic-placeholder",
              `could not infer generic parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`,
              expression.span,
            );
          const unresolvedRows = signature.rowParameters.filter(
            (parameter) => !rowSubstitutions.has(parameter),
          );
          if (unresolvedRows.length > 0)
            this.fail(
              "unresolved-generic-placeholder",
              `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`,
              expression.span,
            );
          const { providers, missing } = this.resolveCallProviders(
            signature.requirements,
            substitutions,
            rowSubstitutions,
            expression.span,
          );
          if (missing.length > 0)
            this.fail(
              "missing-requirement",
              `call to '${signature.name}' requires ${missing.join(", ")}`,
              expression.span,
            );
          const resultType = substituteGenericType(
            signature.result,
            substitutions,
            rowSubstitutions,
          );
          const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
          const defaultArguments = checkedArguments.defaultParameterIndices.map(
            (parameterIndex) => ({
              parameterIndex,
              functionIndex: this.signatures.get(signature.defaultFunctionNames[parameterIndex]!)!
                .index,
            }),
          );
          const suspension: HirExpression = {
            kind: "suspend-construct",
            functionIndex: signature.index,
            functionName: signature.name,
            arguments: checkedArguments.arguments,
            argumentParameterIndices: checkedArguments.parameterIndices,
            defaultArguments: defaultArguments.length > 0 ? defaultArguments : undefined,
            parameterTypes: defaultArguments.length > 0 ? signature.parameters : undefined,
            bounds,
            providers,
            erasedParameterTypes:
              signature.genericParameters.length > 0 || signature.rowParameters.length > 0
                ? signature.parameters
                : undefined,
            type: suspensionType(signature.index, resultType),
            span: expression.span,
          };
          // `race!`'s declaration types the call; its polling body is a
          // compiler intrinsic that the prototype does not emit yet.
          if (signature.intrinsic === RACE_INTRINSIC) {
            const message = "std.task.race! has no run time in the prototype yet";
            return {
              kind: "panic",
              message: this.checkExpression({
                kind: "string",
                value: message,
                span: expression.span,
              }),
              unsupported: { code: "unsupported-task-combinator", message },
              type: resultType,
              span: expression.span,
            };
          }
          return {
            kind: "suspend-drive",
            functionIndex: signature.index,
            suspension,
            erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
            type: resultType,
            span: expression.span,
          };
        }
        const callable = functionParts(this.checkExpression(expression.callee).type);
        if (callable) {
          if (!callable.suspending)
            this.fail("not-suspending", "function value is not suspending", expression.span);
          const suspension = this.checkExpression({
            kind: "call",
            callee: expression.callee,
            arguments: expression.arguments,
            argumentNames: expression.argumentNames,
            argumentSpreads: expression.argumentSpreads,
            span: expression.span,
          });
          return {
            kind: "suspension-drive",
            suspension,
            type: callable.result,
            span: expression.span,
          };
        }
        if (expression.arguments.length !== 0)
          this.fail(
            "argument-count",
            "driving a stored suspension takes no arguments",
            expression.span,
          );
        this.requireDrivableSuspension(expression.callee);
        const suspension = this.checkExpression(expression.callee);
        const storedParts = storedSuspensionParts(suspension.type);
        if (storedParts) {
          if (!storedParts.mutable) {
            this.fail(
              "mutable-receiver-required",
              "driving a stored suspension requires mut Suspend[T]",
              expression.callee.span,
            );
          }
          return {
            kind: "suspension-drive",
            suspension,
            type: storedParts.result,
            span: expression.span,
          };
        }
        const parts = suspensionParts(suspension.type);
        if (parts) {
          const signature = [...this.signatures.values()].find(
            (candidate) => candidate.index === parts.functionIndex,
          );
          return {
            kind: "suspend-drive",
            functionIndex: parts.functionIndex,
            suspension,
            erasedResultType: signature?.genericParameters.length ? signature.result : undefined,
            type: parts.result,
            span: expression.span,
          };
        }
        const traitParts = traitSuspensionParts(suspension.type);
        if (traitParts) {
          return {
            kind: "trait-suspend-drive",
            traitIndex: traitParts.traitIndex,
            methodIndex: traitParts.methodIndex,
            suspension,
            type: traitParts.result,
            span: expression.span,
          };
        }
        this.fail(
          "not-suspending",
          `type '${suspension.type}' is not a suspension`,
          expression.span,
        );
      }
      default:
        return undefined;
    }
  }
}
