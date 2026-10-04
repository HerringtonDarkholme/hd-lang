import type { AssignmentStatement, Expression, Statement } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, HirLocal, HirStatement, HirTrait, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableInner,
  nominalGenericParts,
  readonlyType,
  displayType,
} from "../types.ts";
import { ExpressionCallChecker, type MemberCallExpression } from "./expression-calls.ts";
import { genericTypeName, matchImplementationTarget } from "./shared.ts";

// Operators and indexing on operands that are not primitive call the
// `std.ops` traits (spec/lang/05-expressions.md#operator-traits,
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
          `operator '${operator}' needs an implementation of std.ops.${traitName} for '${displayType(readonlyType(receiver.type))}'` +
            // A function-typed left operand gets no row subsumption
            // (05-expressions.md#r-expr.op.left-dispatch.exact-function.message).
            (functionParts(readonlyType(receiver.type))
              ? "; bind the function to a variable typed with the implementation's function type first"
              : ""),
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
          `an indexed place needs an implementation of std.ops.IndexSet for '${displayType(readonlyType(receiver.type))}'`,
          statement.target.span,
        ),
    );
  }

  /**
   * `v()` on a value whose type is not a function type: the call
   * `Apply::apply(v)`, which takes no argument (05-expressions.md#callable-values).
   */
  protected applyCall(
    expression: Extract<Expression, { kind: "call" }>,
    callee: HirExpression,
    expected: ValueType | undefined,
  ): HirExpression {
    const call = this.standardTraitCall(
      "Apply",
      "apply",
      expression.callee,
      callee,
      [],
      expression.span,
      expected,
      () =>
        this.fail(
          "not-callable",
          `type '${displayType(callee.type)}' is not callable: it is not a function type and does not implement std.ops.Apply`,
          expression.callee.span,
        ),
    );
    if (expression.arguments.length > 0)
      this.fail(
        "argument-count",
        `a callable value of type '${displayType(readonlyType(callee.type))}' takes no arguments`,
        expression.span,
      );
    return call;
  }

  /**
   * A call place's callee, through `once` in a compound assignment. A call
   * of a declared function or method is never a place
   * (05-expressions.md#r-expr.call.apply.place).
   */
  private callPlaceCallee(
    target: Extract<Expression, { kind: "call" }>,
    once: (source: Expression) => Expression = (source) => source,
  ): Expression {
    const callee = target.callee;
    const value =
      callee.kind === "name"
        ? this.resolveLocal(callee.name) !== undefined ||
          this.availableCaptures.has(callee.name) ||
          this.resolveGlobal(callee.name) !== undefined
        : callee.kind !== "qualified-name" &&
          callee.kind !== "contextual-variant" &&
          (callee.kind !== "member" || callee.parenthesized === true);
    if (!value) this.failNotCallPlace(target, undefined);
    return once(callee);
  }

  private failNotCallPlace(
    target: Extract<Expression, { kind: "call" }>,
    calleeType: ValueType | undefined,
  ): never {
    this.fail(
      "invalid-assignment-target",
      calleeType === undefined
        ? "a function or method call is not a place; only a callable value whose type implements std.ops.Update is"
        : `a call is a place only when its callee's type implements std.ops.Update, and '${displayType(readonlyType(calleeType))}' does not`,
      target.span,
    );
  }

  /**
   * `v() = x`: the call `Update::[V]::update(v, x)`, chosen by the type of
   * `v`, then of `x`. A store mutates `v`, so `v` needs mutable access
   * (05-expressions.md#r-expr.call.apply.write, .mut, .readonly).
   */
  protected updateCall(statement: Extract<Statement, { kind: "call-assignment" }>): HirExpression {
    if (statement.copy) this.failCopyIntoOrdinaryPlace(statement.span);
    const target = statement.target;
    const calleeSource = this.callPlaceCallee(target);
    const callee = this.checkExpression(calleeSource);
    if (functionParts(readonlyType(callee.type))) this.failNotCallPlace(target, callee.type);
    if (!this.standardTraitImplemented("Update", "update", callee))
      this.failNotCallPlace(target, callee.type);
    if (target.arguments.length > 0)
      this.fail(
        "argument-count",
        `a callable value of type '${displayType(readonlyType(callee.type))}' takes no arguments`,
        target.span,
      );
    if (mutableInner(callee.type) === undefined && genericTypeName(callee.type) === undefined) {
      let root: Expression = calleeSource;
      while (root.kind === "member") root = root.receiver;
      const binding =
        root.kind === "name"
          ? (this.resolveLocal(root.name) ?? this.resolveGlobal(root.name))
          : undefined;
      const edge = root !== calleeSource && binding && mutableInner(binding.type) !== undefined;
      this.fail(
        edge ? "readonly-edge" : "readonly-root",
        `a store through '${displayType(readonlyType(callee.type))}' needs mutable access to it`,
        calleeSource.span,
      );
    }
    return this.standardTraitCall(
      "Update",
      "update",
      calleeSource,
      callee,
      [statement.value],
      statement.span,
      undefined,
      () => this.failNotCallPlace(target, callee.type),
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
    const trait = this.standardOpsTrait(traitName);
    if (!trait || !this.standardTraitImplemented(traitName, methodName, receiver)) return missing();
    const call: MemberCallExpression = {
      kind: "call",
      callee: { kind: "member", receiver: receiverSource, name: methodName, span },
      arguments: arguments_,
      span,
    };
    if (genericTypeName(readonlyType(receiver.type)))
      return this.checkDynamicMemberCall(call, receiver) ?? missing();
    return this.checkImplementedMemberCall(call, receiver, expected, trait.index);
  }

  private standardOpsTrait(traitName: string): HirTrait | undefined {
    return [...this.traitTypes.values()].find(
      (candidate) => candidate.standardName === `std.ops.${traitName}`,
    );
  }

  /**
   * Whether the receiver's type supplies the `std.ops` trait: through its
   * bounds for a type parameter, otherwise through an implementation.
   */
  private standardTraitImplemented(
    traitName: string,
    methodName: string,
    receiver: HirExpression,
  ): boolean {
    const trait = this.standardOpsTrait(traitName);
    if (!trait) return false;
    const receiverType = readonlyType(receiver.type);
    const generic = genericTypeName(receiverType);
    if (generic)
      return this.signature.genericBounds.some((bound) => {
        if (bound.parameter !== generic) return false;
        const boundTrait = this.traitTypes.get(bound.traitName);
        return (
          boundTrait !== undefined &&
          this.findTraitMethods(boundTrait, methodName).some(
            (selected) => selected.trait.index === trait.index,
          )
        );
      });
    return this.implementations.some(
      (implementation) =>
        implementation.traitIndex === trait.index &&
        matchImplementationTarget(implementation, receiverType, new Map()),
    );
  }

  /**
   * `place op= value` (05-expressions.md#compound-assignment). The place's
   * receiver and index are evaluated once, into hidden locals when they are
   * not plain names, and the place stores `place op value`.
   */
  protected checkCompoundAssignment(statement: AssignmentStatement): HirStatement[] {
    const operator = statement.compound!;
    const output: HirStatement[] = [];
    const once = (source: Expression, expected?: ValueType): Expression => {
      if (source.kind === "name" || source.kind === "integer" || source.kind === "string")
        return source;
      const value = this.checkExpression(source, expected);
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
          : statement.kind === "call-assignment"
            ? // The callee is evaluated once; the read is `v()` and the store
              // `v() = x` (05-expressions.md#r-expr.assign.compound.call-read-write).
              { ...statement.target, callee: this.callPlaceCallee(statement.target, once) }
            : (() => {
                const receiver = once(statement.target.receiver);
                const checkedReceiver = this.checkExpression(receiver);
                const nominal = nominalGenericParts(readonlyType(checkedReceiver.type));
                const builtIn =
                  nominal?.name === "List" || readonlyType(checkedReceiver.type) === "string";
                if (builtIn && statement.target.index.kind === "range")
                  this.fail(
                    "invalid-assignment-target",
                    "a list or string slice is a new value, not a place; assign each element instead",
                    statement.target.span,
                  );
                return {
                  ...statement.target,
                  receiver,
                  index: once(statement.target.index, builtIn ? "u32" : undefined),
                  required: true,
                };
              })();
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
          : plain.kind === "call-assignment"
            ? { ...plain, target: place as typeof plain.target, value }
            : { ...plain, target: place as typeof plain.target, value };
    output.push(this.checkStatement(store));
    return output;
  }
}
