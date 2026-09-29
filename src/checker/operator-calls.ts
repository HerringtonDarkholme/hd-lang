import type { Expression, Statement } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, HirLocal, HirStatement, ValueType } from "../hir.ts";
import { readonlyType } from "../types.ts";
import { ExpressionCallChecker, type MemberCallExpression } from "./expression-calls.ts";
import { genericTypeName, matchGenericTypePattern } from "./shared.ts";

// Operators and indexing on operands that are not primitive call the
// `std.ops` traits (spec/05-expressions.md#operator-traits,
// #index-traits, #compound-assignment). The traits are found by their
// qualified names, so the calls need no `use`.
export abstract class OperatorCallChecker extends ExpressionCallChecker {
  protected operatorTraitCall(
    [traitName, methodName]: readonly [string, string],
    operator: string,
    receiverSource: Expression,
    receiver: HirExpression,
    argument: Expression | undefined,
    span: SourceSpan,
    expected: ValueType | undefined,
  ): HirExpression {
    return this.standardTraitCall(
      traitName,
      methodName,
      receiverSource,
      receiver,
      argument ? [argument] : [],
      span,
      expected,
      () =>
        this.fail(
          "type-mismatch",
          `operator '${operator}' needs an implementation of std.ops.${traitName} for '${readonlyType(receiver.type)}'`,
          span,
        ),
    );
  }

  protected indexSetCall(
    statement: Extract<Statement, { kind: "index-assignment" }>,
    receiver: HirExpression,
  ): HirExpression {
    if (statement.copy) this.failCopyIntoOrdinaryPlace(statement.span);
    return this.standardTraitCall(
      "IndexSet",
      "index_set",
      statement.target.receiver,
      receiver,
      [statement.target.index, statement.value],
      statement.span,
      undefined,
      () =>
        this.fail(
          "invalid-assignment-target",
          `an indexed place needs an implementation of std.ops.IndexSet for '${readonlyType(receiver.type)}'`,
          statement.target.span,
        ),
    );
  }

  /**
   * The call `Trait::method(receiver, arguments...)` of a `std.ops` trait,
   * which needs no `use`: through the receiver's bounds for a type parameter
   * (r-expr.op.generic), otherwise through its implementations, choosing
   * among instantiations by the arguments (r-expr.op.left-dispatch).
   */
  private standardTraitCall(
    traitName: string,
    methodName: string,
    receiverSource: Expression,
    receiver: HirExpression,
    arguments_: readonly Expression[],
    span: SourceSpan,
    expected: ValueType | undefined,
    missing: () => never,
  ): HirExpression {
    const trait = [...this.traitTypes.values()].find(
      (candidate) => candidate.standardName === `std.ops.${traitName}`,
    );
    if (!trait) return missing();
    const receiverType = readonlyType(receiver.type);
    const call: MemberCallExpression = {
      kind: "call",
      callee: { kind: "member", receiver: receiverSource, name: methodName, span },
      arguments: arguments_,
      span,
    };
    const generic = genericTypeName(receiverType);
    if (generic) {
      const supplied = this.signature.genericBounds.some((bound) => {
        if (bound.parameter !== generic) return false;
        const boundTrait = this.traitTypes.get(bound.traitName);
        return (
          boundTrait !== undefined &&
          this.findTraitMethods(boundTrait, methodName).some(
            (selected) => selected.trait.index === trait.index,
          )
        );
      });
      if (!supplied) return missing();
      return this.checkDynamicMemberCall(call, receiver) ?? missing();
    }
    const implemented = this.implementations.some(
      (implementation) =>
        implementation.traitIndex === trait.index &&
        matchGenericTypePattern(implementation.targetType, receiverType, new Map()),
    );
    if (!implemented) return missing();
    return this.checkImplementedMemberCall(call, receiver, expected, trait.index);
  }

  /**
   * `place op= value` (05-expressions.md#compound-assignment). The place's
   * receiver and index are evaluated once, into hidden locals when they are
   * not plain names, and the place stores `place op value`.
   */
  protected checkCompoundAssignment(
    statement: Extract<Statement, { kind: "assignment" | "field-assignment" | "index-assignment" }>,
  ): HirStatement[] {
    const operator = statement.compound!;
    const output: HirStatement[] = [];
    const once = (source: Expression): Expression => {
      if (source.kind === "name" || source.kind === "integer" || source.kind === "string")
        return source;
      const value = this.checkExpression(source);
      const local: HirLocal = {
        name: `$compound${this.locals.length}`,
        type: value.type,
        index: this.locals.length,
        mutable: false,
        parameter: false,
        span: source.span,
      };
      this.locals.push(local);
      this.currentScope().set(local.name, local);
      output.push({ kind: "binding", local, value, span: source.span });
      return { kind: "name", name: local.name, span: source.span };
    };
    const place: Expression =
      statement.kind === "assignment"
        ? { kind: "name", name: statement.name, span: statement.span }
        : statement.kind === "field-assignment"
          ? { ...statement.target, receiver: once(statement.target.receiver) }
          : {
              ...statement.target,
              receiver: once(statement.target.receiver),
              index: once(statement.target.index),
            };
    // `p op= e` is `p = p op e` for every type: the operator follows
    // Operator Traits and the store follows assignment
    // (05-expressions.md#r-expr.assign.compound.meaning).
    const value: Expression = {
      kind: "binary",
      operator,
      left: place,
      right: statement.value,
      span: statement.span,
    };
    const { compound: _compound, ...plain } = statement;
    const store: Statement =
      plain.kind === "assignment"
        ? { ...plain, value }
        : plain.kind === "field-assignment"
          ? { ...plain, target: place as typeof plain.target, value }
          : { ...plain, target: place as typeof plain.target, value };
    output.push(this.checkStatement(store));
    return output;
  }
}
