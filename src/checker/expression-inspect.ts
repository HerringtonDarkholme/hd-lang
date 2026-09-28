import type { Expression, TypeRef } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, HirTrait, ValueType } from "../hir.ts";
import {
  mutableInner,
  nominalGenericParts,
  optionalInner,
  optionalType,
  readonlyType,
} from "../types.ts";
import { extendsInspectable, usesStandardInspect } from "./inspectable.ts";
import { MemberLookupChecker } from "./member-lookup.ts";
import { containsGenericType, genericTypeName, traitTypeName } from "./shared.ts";
import { INSPECTABLE, TYPE_ID } from "./standard-traits.ts";
import {
  metadataMethodName,
  SHAPE_METADATA_TYPES,
  shapeBuilderName,
  shapeOfBuilderName,
  shapeOfCall,
  specializedShapeBase,
} from "./shapes.ts";

type CallExpression = Extract<Expression, { kind: "call" }>;
interface MemberCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "member" }>;
}

// Runtime type identity intrinsics (spec/09-traits.md#runtime-type-identity):
// the `downcast` default methods, `downcast_val`, `TypeId::of`, and
// `runtime_type` on a concrete receiver.
export abstract class InspectChecker extends MemberLookupChecker {
  /** The standard Inspectable trait, when the module imported `std.inspect` or `std.error`. */
  protected standardInspectable(): HirTrait | undefined {
    const trait = this.traitTypes.get(INSPECTABLE);
    return trait && usesStandardInspect(this.imports) ? trait : undefined;
  }

  /**
   * The Inspectable dictionary of a downcast or TypeId target. A trait value
   * type that extends Inspectable satisfies the bound by its supertrait
   * (spec/09-traits.md#dynamic-trait-values); its TypeId never equals a
   * recorded type, so such a downcast yields `.None`.
   */
  protected inspectTargetDictionary(
    trait: HirTrait,
    target: ValueType,
    span: SourceSpan,
  ): HirExpression {
    const type = readonlyType(target);
    const generic = genericTypeName(type);
    if (generic) {
      const boundIndex = this.signature.genericBounds.findIndex(
        (bound) => bound.parameter === generic && bound.traitIndex === trait.index,
      );
      if (boundIndex < 0)
        this.fail(
          "unsatisfied-trait-bound",
          `generic parameter '${generic}' does not implement Inspectable`,
          span,
        );
      return {
        kind: "trait-bound-dictionary",
        traitIndex: trait.index,
        boundIndex,
        type: `trait:${INSPECTABLE}`,
        span,
      };
    }
    const traitValue = traitTypeName(type);
    const plan =
      traitValue && extendsInspectable(this.traitTypes, traitValue)
        ? {
            bounds: [],
            implementationIndex: -1,
            supertraits: [],
            builtin: {
              kind: "inspectable" as const,
              traitIndex: trait.index,
              targetType: type,
              key: [type.slice("trait:".length)],
            },
          }
        : this.builtinTraitDictionaryPlan(trait.index, type, [], span);
    if (!plan)
      this.fail("unsatisfied-trait-bound", `type '${type}' does not implement Inspectable`, span);
    return {
      kind: "trait-dictionary",
      traitIndex: trait.index,
      dictionary: plan,
      type: `trait:${INSPECTABLE}`,
      span,
    };
  }

  protected inspectTarget(
    typeArguments: readonly TypeRef[] | undefined,
    expected: ValueType | undefined,
    name: string,
    span: SourceSpan,
  ): ValueType {
    if (typeArguments && typeArguments.length !== 1)
      this.fail("generic-arity", `${name} expects one type argument`, span);
    if (typeArguments) return this.resolveType(typeArguments[0]!);
    const inferred = expected === undefined ? undefined : optionalInner(expected);
    if (inferred === undefined)
      this.fail(
        "unresolved-generic-placeholder",
        `could not infer the target type of ${name}`,
        span,
      );
    return inferred;
  }

  // `value.downcast[T]()`, `value.downcast_mut[T]()`, and `value.runtime_type()`
  // on a concrete receiver (spec/09-traits.md#recovering-a-concrete-type). The
  // prototype lowers the default methods as intrinsics.
  protected checkInspectMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
    expected?: ValueType,
  ): HirExpression | undefined {
    const name = expression.callee.name;
    if (name !== "downcast" && name !== "downcast_mut" && name !== "runtime_type") return undefined;
    const trait = this.standardInspectable();
    if (!trait) return undefined;
    const receiverType = readonlyType(receiver.type);
    const traitValue = traitTypeName(receiverType);
    const generic = genericTypeName(receiverType);
    if (traitValue && !extendsInspectable(this.traitTypes, traitValue)) return undefined;
    // An inherent method of the same name wins ordinary lookup on the concrete
    // type (spec/09-traits.md#sealed-traits).
    const nominal = nominalGenericParts(receiverType)?.name ?? receiverType;
    if (
      this.inherentMethods.some(
        (method) =>
          method.name === name &&
          (nominalGenericParts(method.targetType)?.name ?? method.targetType) === nominal,
      )
    )
      return undefined;
    if (name === "runtime_type") {
      if (traitValue || generic) return undefined;
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "runtime_type takes no arguments", expression.span);
      const erased = this.coerce(receiver, `trait:${INSPECTABLE}`, expression.span);
      if (erased.type !== `trait:${INSPECTABLE}`) return undefined;
      return {
        kind: "trait-call",
        receiver: erased,
        traitIndex: trait.index,
        methodIndex: 0,
        arguments: [],
        providers: [],
        type: TYPE_ID,
        span: expression.span,
      };
    }
    if (expression.arguments.length !== 0)
      this.fail("argument-count", `${name} takes no value arguments`, expression.span);
    const mutable = name === "downcast_mut";
    if (mutable && mutableInner(receiver.type) === undefined)
      this.fail(
        "mutable-receiver-required",
        "downcast_mut requires a mutable receiver",
        expression.callee.receiver.span,
      );
    const erasedType = mutable ? `mut:trait:${INSPECTABLE}` : `trait:${INSPECTABLE}`;
    const erased = this.coerce(
      mutable ? receiver : this.coerce(receiver, receiverType, expression.span),
      erasedType,
      expression.span,
    );
    if (erased.type !== erasedType)
      this.fail(
        "unsatisfied-trait-bound",
        `type '${receiver.type}' does not implement Inspectable`,
        expression.span,
      );
    const target = readonlyType(
      this.inspectTarget(expression.typeArguments, expected, name, expression.span),
    );
    const targetGeneric = genericTypeName(target);
    // `T < AnyRef + Inspectable`: value types use downcast_val.
    if (
      targetGeneric
        ? !(this.signature.referenceParameters ?? []).includes(targetGeneric)
        : !this.isIdentityType(target)
    )
      this.fail(
        "unsatisfied-trait-bound",
        `type '${target}' does not implement AnyRef, required by the bound on '${name}'; recover a value type with downcast_val`,
        expression.span,
      );
    const dictionary = this.inspectTargetDictionary(trait, target, expression.span);
    return {
      kind: "inspect-downcast",
      traitIndex: trait.index,
      value: erased,
      dictionary,
      type: optionalType(mutable ? `mut:${target}` : target),
      span: expression.span,
    };
  }

  // `std.inspect.downcast_val[T](value)` and `TypeId::of[T]()`.
  protected checkInspectFunctionCall(
    expression: CallExpression,
    kind: "downcast_val" | "of",
    expected?: ValueType,
  ): HirExpression | undefined {
    const trait = this.standardInspectable();
    if (!trait) return undefined;
    const target = readonlyType(
      this.inspectTarget(expression.typeArguments, expected, kind, expression.span),
    );
    const dictionary = this.inspectTargetDictionary(trait, target, expression.span);
    if (kind === "of") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "TypeId::of takes no value arguments", expression.span);
      return {
        kind: "inspect-type-id",
        traitIndex: trait.index,
        dictionary,
        type: TYPE_ID,
        span: expression.span,
      };
    }
    if (expression.arguments.length !== 1)
      this.fail("argument-count", "downcast_val expects one value argument", expression.span);
    const source = this.checkExpression(expression.arguments[0]!, `trait:${INSPECTABLE}`);
    const value = this.coerce(source, `trait:${INSPECTABLE}`, source.span);
    this.requireAssignable(value.type, `trait:${INSPECTABLE}`, source.span);
    return {
      kind: "inspect-downcast",
      traitIndex: trait.index,
      value,
      dictionary,
      type: optionalType(target),
      span: expression.span,
    };
  }

  /**
   * `shape[T]()` and `shape_of(f)` (spec/14-annotations.md#shape-intrinsics):
   * the call of the builder that `shapes.ts` generated for the target.
   */
  protected shapeIntrinsicCall(expression: CallExpression): Expression {
    const name = expression.callee.kind === "name" ? expression.callee.name : "shape";
    const call = (callee: string): Expression => ({
      kind: "call",
      callee: { kind: "name", name: callee, span: expression.callee.span },
      arguments: [],
      span: expression.span,
    });
    if (expression.argumentSpreads?.some(Boolean))
      this.fail(
        "positional-spread-needs-vararg",
        `${name} has no variadic parameter`,
        expression.span,
      );
    if (name === "shape") {
      const typeArguments = expression.typeArguments ?? [];
      if (typeArguments.length !== 1 || expression.arguments.length !== 0)
        this.fail(
          "argument-count",
          "shape expects one type argument and no values",
          expression.span,
        );
      const target = typeArguments[0]!;
      const builder = shapeBuilderName(target.name);
      if (!this.signatures.has(builder)) this.failReifiedShape(target, "unknown-shape-target");
      return call(builder);
    }
    if (expression.typeArguments?.length)
      this.fail("unexpected-type-arguments", "shape_of takes no type arguments", expression.span);
    if (expression.arguments.length !== 1)
      this.fail("argument-count", "shape_of expects one function name", expression.span);
    const argument = expression.arguments[0]!;
    const local =
      argument.kind !== "name" ||
      this.resolveLocal(argument.name) !== undefined ||
      this.availableCaptures.has(argument.name) ||
      this.resolveGlobal(argument.name) !== undefined;
    const signature = local
      ? undefined
      : this.visibleSignature((argument as { name: string }).name);
    if (!signature || !this.signatures.has(shapeOfBuilderName(signature.name)))
      this.fail(
        "unknown-shape-target",
        "shape_of expects the name of a module-level function declaration",
        argument.span,
      );
    const types = {
      newtypeBase: (type: string): string | undefined => {
        const declaration = this.dataTypes.get(type);
        return declaration?.newtype ? declaration.fields[0]?.type : undefined;
      },
      isTrait: (type: string): boolean => this.traitTypes.has(type),
    };
    const { result, requirements } = signature;
    return shapeOfCall(types, signature.name, result, requirements, expression.span);
  }

  /**
   * The generated `hd__metadata_M` method that `shape.metadata[M]()` calls on
   * a concrete shape type (spec/14-annotations.md#common-shape-representation).
   */
  protected shapeMetadataMethod(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): string | undefined {
    const type = readonlyType(receiver.type);
    if (expression.callee.name !== "metadata") return undefined;
    if (!SHAPE_METADATA_TYPES.has(type) && !specializedShapeBase(type)) return undefined;
    for (const argument of expression.arguments) this.checkExpression(argument);
    const typeArguments = expression.typeArguments ?? [];
    if (typeArguments.length !== 1 || expression.arguments.length !== 0)
      this.fail(
        "argument-count",
        "metadata expects one type argument and no values",
        expression.span,
      );
    const target = typeArguments[0]!;
    this.failReifiedShape(target);
    return metadataMethodName(target.name);
  }

  /**
   * A type parameter's shape needs its runtime descriptor, which the
   * prototype does not pass; otherwise `fallback`, if any, is reported.
   */
  private failReifiedShape(target: TypeRef, fallback?: string): void {
    if (containsGenericType(this.resolveType(target)))
      this.fail(
        "unsupported-reified-shape",
        `the prototype does not pass the runtime descriptor of '${target.name}'`,
        target.span,
      );
    if (fallback) this.fail(fallback, `shape has no target '${target.name}'`, target.span);
  }
}
