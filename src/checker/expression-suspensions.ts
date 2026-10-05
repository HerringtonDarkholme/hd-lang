import type { Expression } from "../ast.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableType,
  nominalGenericType,
  storedSuspensionParts,
  suspensionParts,
  suspensionType,
  traitSuspensionParts,
  tupleType,
  displayType,
} from "../types.ts";
import { ALL_COMBINATOR, ALL_FRAME_INTRINSIC, HD_RUN, RACE_COMBINATOR } from "./standard-traits.ts";
import { defaultCallFields, orderedTypeSubstitutions, substituteGenericType } from "./shared.ts";

import { OperatorCallChecker } from "./operator-calls.ts";
export abstract class ExpressionSuspensionChecker extends OperatorCallChecker {
  /**
   * `all!(e_1, ..., e_n)`, where each `e_i` is a `mut Suspend[X_i]`, has type
   * `(X_1, ..., X_n)` (11-requirements-and-suspension.md#typing-all). It
   * drives the polling frame of `all_frame`, a `lib/std/task.hd` intrinsic.
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
    const checked: HirExpression[] = [];
    const results = expression.arguments.map((argument) => {
      const child = this.checkExpression(argument);
      checked.push(child);
      const stored = storedSuspensionParts(child.type);
      const result =
        suspensionParts(child.type)?.result ??
        traitSuspensionParts(child.type)?.result ??
        (stored?.mutable ? stored.result : undefined);
      if (result === undefined)
        this.fail(
          "type-mismatch",
          `an argument of all! must be a mut Suspend[T], such as a cold call, found '${displayType(child.type)}'`,
          argument.span,
        );
      return result;
    });
    // The call drives the frame that `lib/std/task.hd`'s `all_frame` builds
    // over the children, each held as a stored suspension.
    const frame = [...this.signatures.values()].find(
      (signature) => signature.intrinsic === ALL_FRAME_INTRINSIC,
    );
    if (!frame) throw new Error("std.task's all_frame is not declared");
    const type = tupleType(results);
    const stored = (result: ValueType): ValueType =>
      mutableType(nominalGenericType("Suspend", [result]));
    const children = results.map((result, index) =>
      this.coerce(checked[index]!, stored(result), expression.arguments[index]!.span),
    );
    const call: HirExpression = {
      kind: "call",
      functionIndex: frame.index,
      functionName: frame.name,
      arguments: [
        {
          kind: "list",
          elements: children,
          elementType: stored(type),
          type: nominalGenericType("List", [stored(type)]),
          span: expression.span,
        },
      ],
      bounds: [],
      providers: [],
      erasedParameterTypes: frame.parameters,
      erasedResultType: frame.result,
      type: stored(type),
      span: expression.span,
    };
    return { kind: "suspension-drive", suspension: call, type, span: expression.span };
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
              `function '${displayType(signature.name)}' is not suspending`,
              expression.span,
            );
          // `race!` has no first result without a task
          // (11-requirements-and-suspension.md#r-req.combinator.race-empty).
          if (
            this.imports.get(expression.callee.name) === RACE_COMBINATOR &&
            expression.arguments.length === 0
          )
            this.fail("argument-count", "race! expects at least one task", expression.span);
          // An empty list literal by name is the same error
          // (11-requirements-and-suspension.md#r-req.combinator.race-empty-literal).
          const [tasks] = expression.arguments;
          if (
            this.imports.get(expression.callee.name) === RACE_COMBINATOR &&
            expression.arguments.length === 1 &&
            expression.argumentNames?.[0] === "tasks" &&
            !expression.argumentSpreads?.[0] &&
            tasks?.kind === "list" &&
            tasks.elements.length === 0
          )
            this.fail("argument-count", "race! expects at least one task", expression.span);
          const checkedArguments = this.checkSignatureArguments(expression, signature, expected);
          const { substitutions, rowSubstitutions } = checkedArguments;
          const unresolved = signature.genericParameters.filter(
            (parameter) => !substitutions.has(parameter),
          );
          if (unresolved.length > 0)
            this.failUnresolvedCall(unresolved, expression.callee.name, expression.span);
          const unresolvedRows = signature.rowParameters.filter(
            (parameter) => !rowSubstitutions.has(parameter),
          );
          if (unresolvedRows.length > 0)
            this.fail(
              "cannot-infer-type",
              `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`,
              expression.span,
            );
          // `hd_run!` runs an executable of the package, so only an
          // integration test module may call it
          // (spec/std/testing.md#r-std-testing.hd-run.integration-only).
          if (this.imports.get(expression.callee.name) === HD_RUN && !this.integrationTest)
            this.fail(
              "test-only-use",
              "hd_run! runs an executable of the package, so only an integration test module, a file under tests/, may call it",
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
              `call to '${displayType(signature.name)}' requires ${missing.map(displayType).join(", ")}`,
              expression.span,
            );
          const resultType = substituteGenericType(
            signature.result,
            substitutions,
            rowSubstitutions,
          );
          const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
          const defaultFields = defaultCallFields(
            this.signatures,
            signature.defaultFunctionNames,
            checkedArguments.defaultParameterIndices,
            signature.parameters,
            0,
          );
          const suspension: HirExpression = {
            kind: "suspend-construct",
            functionIndex: signature.index,
            functionName: signature.name,
            arguments: checkedArguments.arguments,
            argumentParameterIndices: checkedArguments.parameterIndices,
            ...defaultFields,
            bounds,
            providers,
            erasedParameterTypes:
              signature.genericParameters.length > 0 || signature.rowParameters.length > 0
                ? signature.parameters
                : undefined,
            erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
            erasedTypeSubstitutions: orderedTypeSubstitutions(
              signature.genericParameters,
              substitutions,
            ),
            type: suspensionType(signature.index, resultType),
            span: expression.span,
          };
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
          `type '${displayType(suspension.type)}' is not a suspension`,
          expression.span,
        );
      }
      default:
        return undefined;
    }
  }
}
