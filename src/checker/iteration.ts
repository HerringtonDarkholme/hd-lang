import type { Expression } from "../ast.ts";
import type { HirExpression, HirLocal, ValueType } from "../hir.ts";
import {
  CURSOR_TYPE,
  functionType,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalType,
  readonlyType,
} from "../types.ts";
import type { MemberCallExpression } from "./expression-calls.ts";
import { MethodReferenceChecker } from "./method-references.ts";
import { genericTypeName, matchImplementationTarget } from "./shared.ts";

/**
 * The prelude `Iterator[T]` is a `std.iter` data type with a `step` closure
 * (06-control-flow.md#iteration-protocols). A list or map iterates through the
 * built-in cursor, which checks iterator invalidation.
 */
export abstract class IterationChecker extends MethodReferenceChecker {
  protected abstract checkDynamicMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): HirExpression | undefined;

  /**
   * `value.iter()` when a loop or comprehension iterates a value whose type
   * implements `Iterable`, or a type parameter bounded by it
   * (06-control-flow.md#for-loops). Undefined otherwise, and for a list, a
   * map, or an iterator, which a loop advances directly.
   */
  protected iterableIterCall(value: HirExpression, source: Expression): HirExpression | undefined {
    const iterable = this.traitTypes.get("Iterable");
    if (!iterable) return undefined;
    const type = readonlyType(value.type);
    if (["List", "Map", "Iterator"].includes(nominalGenericParts(type)?.name ?? ""))
      return undefined;
    const generic = genericTypeName(type);
    const bounded =
      generic !== undefined &&
      this.signature.genericBounds.some(
        (bound) => bound.parameter === generic && bound.traitIndex === iterable.index,
      );
    const implemented =
      generic === undefined &&
      this.implementations.some(
        (implementation) =>
          implementation.traitIndex === iterable.index &&
          matchImplementationTarget(implementation, type, new Map()),
      );
    if (!bounded && !implemented) return undefined;
    const call: MemberCallExpression = {
      kind: "call",
      callee: { kind: "member", receiver: source, name: "iter", span: source.span },
      arguments: [],
      span: source.span,
    };
    return bounded
      ? this.checkDynamicMemberCall(call, value)
      : this.checkImplementedMemberCall(call, value);
  }

  /**
   * `list.iter()` or `map.iter()`: the `Iterator` whose `step` advances the
   * built-in `cursor` that the collection just made.
   */
  protected collectionIterator(cursor: HirExpression, elementType: ValueType): HirExpression {
    const iterator = this.dataTypes.get("Iterator");
    if (!iterator) throw new Error("std.iter declares Iterator for a program that calls iter()");
    const span = cursor.span;
    const local: HirLocal = {
      name: "$cursor",
      type: cursor.type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span,
    };
    this.locals.push(local);
    this.scopes.push(new Map([[local.name, local]]));
    let step: HirExpression;
    try {
      step = this.checkExpression(
        {
          kind: "closure",
          parameters: [],
          body: [
            {
              kind: "expression",
              expression: {
                kind: "call",
                callee: {
                  kind: "member",
                  receiver: { kind: "name", name: local.name, span },
                  name: "next",
                  span,
                },
                arguments: [],
                span,
              },
              span,
            },
          ],
          span,
        },
        functionType([], optionalType(elementType)),
      );
    } finally {
      this.scopes.pop();
    }
    const value: HirExpression = {
      kind: "data",
      dataIndex: iterator.index,
      fields: [step],
      fieldIndices: [0],
      erasedFieldTypes: iterator.fields.map((field) => field.type),
      type: mutableType(nominalGenericType("Iterator", [elementType])),
      span,
    };
    return {
      kind: "match",
      subject: cursor,
      representation: "scalar",
      arms: [
        {
          bindings: [{ local, fieldIndex: -1, type: cursor.type }],
          body: [{ kind: "expression", expression: value, span }],
          span,
        },
      ],
      type: value.type,
      span,
    };
  }

  /** The built-in cursor type over `elementType`. */
  protected cursorType(elementType: ValueType): ValueType {
    return mutableType(nominalGenericType(CURSOR_TYPE, [elementType]));
  }

  /** The function of `Iterator.next`, which a loop over an iterator calls. */
  protected iteratorNextFunction(): number | undefined {
    const method = this.inherentMethods.find(
      (candidate) =>
        candidate.name === "next" &&
        !candidate.associated &&
        nominalGenericParts(candidate.targetType)?.name === "Iterator",
    );
    return method ? this.signatures.get(method.functionName)?.index : undefined;
  }
}
