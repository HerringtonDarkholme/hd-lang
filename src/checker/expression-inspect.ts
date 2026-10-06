import type { Expression, TypeRef } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirEnum, HirExpression, HirTrait, ValueType } from "../hir.ts";
import {
  mutableInner,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
  readonlyType,
  displayType,
} from "../types.ts";
import { matchFactPattern } from "./fact-patterns.ts";
import {
  extendsInspectable,
  HANDLE_TYPE,
  inspectableBuiltin,
  inspectKey,
  usesStandardInspect,
} from "./inspectable.ts";
import { MemberLookupChecker } from "./member-lookup.ts";
import { genericTypeName, traitTypeName } from "./shared.ts";
import { INSPECTABLE, TYPE_ID } from "./standard-traits.ts";
import { factsOfBuilderName, STRUCTURE_FACT } from "./function-facts.ts";
import { STRUCTURE_WITNESS, STRUCTURE_WITNESS_FIELD } from "./typed-derivation.ts";

type CallExpression = Extract<Expression, { kind: "call" }>;
interface MemberCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "member" }>;
}
interface NamedCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "name" }>;
}

// Runtime type identity intrinsics (spec/lang/09-traits.md#runtime-type-identity):
// the `downcast` default methods, `downcast_val`, `TypeId::of`, and
// `runtime_type` on a concrete receiver; and typed-fact patterns, at a
// typed fact's checking call and at `h.fact::[D]()` (fact-patterns.ts).
export abstract class InspectChecker extends MemberLookupChecker {
  protected abstract checkDeclaredCall(
    expression: NamedCallExpression,
    expected?: ValueType,
    initialSubstitutions?: ReadonlyMap<string, ValueType>,
  ): HirExpression;

  /** The standard Inspectable trait, when the module imported `std.inspect` or `std.error`. */
  protected standardInspectable(): HirTrait | undefined {
    const trait = this.traitTypes.get(INSPECTABLE);
    return trait && usesStandardInspect(this.imports) ? trait : undefined;
  }

  /**
   * The Inspectable dictionary of a downcast or TypeId target. A trait value
   * type that extends Inspectable satisfies the bound by its supertrait
   * (spec/lang/09-traits.md#dynamic-trait-values); its TypeId never equals a
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
      const dictionary = this.inspectableBound(generic, trait.index, span);
      if (!dictionary)
        this.fail(
          "unsatisfied-trait-bound",
          `generic parameter '${generic}' does not implement Inspectable`,
          span,
        );
      return dictionary;
    }
    const traitValue = traitTypeName(type);
    // A trait value type's key spells its trait as any other declaration's
    // (r-trait.identity.trait-value, r-trait.typeid.name.qualified); one whose
    // arguments have no key keeps its written spelling.
    const parts =
      traitValue && extendsInspectable(this.traitTypes, traitValue)
        ? (inspectKey(type, this.inspectEnvironment(), true) ?? [type.slice("trait:".length)])
        : undefined;
    const dictionary =
      parts &&
      inspectableBuiltin(parts, trait.index, type, (generic) =>
        generic === HANDLE_TYPE
          ? this.handleWitness!.dictionary
          : this.inspectableBound(generic, trait.index, span)!,
      );
    const plan = dictionary
      ? {
          bounds: dictionary.bounds,
          implementationIndex: -1,
          supertraits: [],
          builtin: dictionary.builtin,
        }
      : this.builtinTraitDictionaryPlan(trait.index, type, [], span);
    if (!plan)
      this.fail(
        "unsatisfied-trait-bound",
        `type '${displayType(type)}' does not implement Inspectable`,
        span,
      );
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
      this.fail("cannot-infer-type", `could not infer the target type of ${name}`, span);
    return inferred;
  }

  // `value.downcast::[T]()`, `value.downcast_mut::[T]()`, and `value.runtime_type()`
  // on a concrete receiver (spec/lang/09-traits.md#recovering-a-concrete-type). The
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
    // type (spec/lang/09-traits.md#sealed-traits).
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
        `type '${displayType(receiver.type)}' does not implement Inspectable`,
        expression.span,
      );
    const target = readonlyType(
      this.inspectTarget(expression.typeArguments, expected, name, expression.span),
    );
    const targetGeneric = genericTypeName(target);
    // `T < AnyRef & Inspectable`: value types use downcast_val.
    if (
      targetGeneric
        ? !(this.signature.referenceParameters ?? []).includes(targetGeneric)
        : !this.isIdentityType(target)
    )
      this.fail(
        "unsatisfied-trait-bound",
        `type '${displayType(target)}' does not implement AnyRef, required by the bound on '${name}'; recover a value type with downcast_val`,
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

  // `std.inspect.downcast_val::[T](value)` and `TypeId::of::[T]()`.
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

  /** Runs `check` with every type inspectable, for a fact value or a handle witness. */
  private withAnyTypeInspectable<T>(check: () => T): T {
    const saved = this.anyTypeInspectable;
    this.anyTypeInspectable = true;
    try {
      return check();
    } finally {
      this.anyTypeInspectable = saved;
    }
  }

  /**
   * The checker intrinsics of typed derivation's facts: `hd__structure_fact(v)`
   * erases a fact value of any type to `Inspectable`, and
   * `hd__structure_witness::[X]()` is an `Inspectable` value, with no
   * payload, whose dictionary names `X`, whatever `X` is.
   */
  protected checkStructureFactIntrinsic(expression: CallExpression): HirExpression | undefined {
    const name = expression.callee.kind === "name" ? expression.callee.name : undefined;
    if (name === STRUCTURE_FACT && expression.arguments.length === 1) {
      const value = this.checkExpression(expression.arguments[0]!);
      return this.withAnyTypeInspectable(() =>
        this.requireCoercion(value, `trait:${INSPECTABLE}`, value.span),
      );
    }
    if (name !== STRUCTURE_WITNESS || expression.typeArguments?.length !== 1) return undefined;
    const target = this.resolveType(expression.typeArguments[0]!);
    const trait = this.traitTypes.get(INSPECTABLE)!;
    const plan = this.withAnyTypeInspectable(() =>
      this.builtinTraitDictionaryPlan(trait.index, target, [], expression.span),
    );
    if (!plan) throw new Error(`no handle witness for '${target}'`);
    return {
      kind: "trait-dictionary",
      traitIndex: trait.index,
      dictionary: plan,
      type: `trait:${INSPECTABLE}`,
      span: expression.span,
    };
  }

  /**
   * The arguments that a typed fact type's pattern takes from `target`
   * (annot.typed-fact.infer). A target that does not fit the pattern is an
   * error at `span` (annot.typed-fact.infer.mismatch,
   * annot.handle.fact.pattern.mismatch).
   */
  protected factPatternArguments(
    factType: HirData | HirEnum,
    pattern: ValueType,
    target: ValueType,
    span: SourceSpan,
  ): ReadonlyMap<string, ValueType> {
    const bounded = new Set((factType.declaredBounds ?? []).map((bound) => bound.parameter));
    const solved = matchFactPattern(pattern, target, bounded, {
      data: this.dataTypes,
      enums: this.enumTypes,
    });
    if (!solved)
      this.fail(
        "type-mismatch",
        `'${displayType(target)}' does not fit the pattern '${displayType(pattern)}' of the typed fact type '${displayType(factType.name)}'`,
        span,
      );
    return solved;
  }

  /**
   * A typed fact's checking call `hd__typed_fact_N(v)` (typed-facts.ts): the
   * fact type's pattern, matched against the target's type, gives the call
   * the arguments that the pattern mentions (annot.typed-fact.infer). The
   * ordinary call then checks `v` and the bounds (annot.typed-fact.check).
   */
  protected checkTypedFactCall(
    expression: NamedCallExpression,
    expected?: ValueType,
  ): HirExpression {
    const { target, factType } = expression.typedFact!;
    const typed = this.typedFactType(factType);
    const helper = this.visibleSignature(expression.callee.name);
    if (!typed || !helper) return this.checkDeclaredCall(expression, expected);
    const { declaration, pattern } = typed;
    const targetType = this.resolveType({ name: target, span: expression.span });
    const solved = this.factPatternArguments(declaration, pattern, targetType, expression.span);
    // The helper's parameters are the fact type's, renamed in order.
    const initial = new Map<string, ValueType>();
    declaration.genericParameters.forEach((parameter, index) => {
      const type = solved.get(parameter);
      if (type !== undefined) initial.set(helper.genericParameters[index]!, type);
    });
    return this.checkDeclaredCall(expression, expected, initial);
  }

  /** The typed fact type that `type` names, with its pattern. */
  protected typedFactType(
    type: ValueType,
  ): { readonly declaration: HirData | HirEnum; readonly pattern: ValueType } | undefined {
    const name = nominalGenericParts(type)?.name ?? type;
    const declaration = this.dataTypes.get(name) ?? this.enumTypes.get(name);
    const pattern = declaration?.factPattern;
    return declaration && pattern !== undefined ? { declaration, pattern } : undefined;
  }

  /**
   * `h.fact::[D]()` on a `std.structure` handle `Field[S, F]`, where `D` is a
   * typed fact type: `D`'s pattern is matched against `F` as the
   * attach-time check matches it (annot.handle.fact.pattern), and a bare `D`
   * becomes `D[A]` with the inferred arguments. Written arguments must be
   * those (annot.handle.fact.pattern.written).
   */
  protected withHandleFactPattern(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): MemberCallExpression {
    const written =
      expression.typeArguments?.length === 1 ? expression.typeArguments[0] : undefined;
    const handle = nominalGenericParts(readonlyType(receiver.type));
    const memberType = handle?.arguments[1];
    if (
      expression.callee.name !== "fact" ||
      !written ||
      memberType === undefined ||
      !this.dataTypes
        .get(handle!.name)
        ?.fields.some((field) => field.name === STRUCTURE_WITNESS_FIELD)
    )
      return expression;
    const bare = !written.name.includes("[");
    const typed = this.typedFactType(bare ? written.name : this.resolveType(written));
    if (!typed) return expression;
    const { declaration, pattern } = typed;
    const solved = this.factPatternArguments(declaration, pattern, memberType, expression.span);
    if (!bare) {
      const writtenArguments = nominalGenericParts(this.resolveType(written))?.arguments ?? [];
      declaration.genericParameters.forEach((parameter, index) => {
        const inferred = solved.get(parameter);
        if (inferred !== undefined && writtenArguments[index] !== inferred)
          this.fail(
            "type-mismatch",
            `'${displayType(written.name)}' does not match the handle's member type '${displayType(memberType)}': the pattern '${displayType(pattern)}' gives '${displayType(parameter)}' = '${displayType(inferred)}'`,
            written.span,
          );
      });
      return expression;
    }
    if (declaration.genericParameters.length === 0) return expression;
    const unsolved = declaration.genericParameters.filter((parameter) => !solved.has(parameter));
    if (unsolved.length > 0)
      this.fail(
        "cannot-infer-type",
        `the pattern '${displayType(pattern)}' of '${displayType(declaration.name)}' does not mention ${unsolved.map((name) => `'${displayType(name)}'`).join(", ")}; write the type arguments: \`fact::[${displayType(declaration.name)}[...]]\``,
        written.span,
      );
    const type = nominalGenericType(
      declaration.name,
      declaration.genericParameters.map((parameter) => solved.get(parameter)!),
    );
    return { ...expression, typeArguments: [{ name: type, span: written.span }] };
  }

  /**
   * Checks a member call with `h.fact::[M]()`'s exemption
   * (annot.handle.fact.key): when `h` is a `std.structure` handle
   * `Field[S, F]` whose `F` has no runtime identity of its own, the handle's
   * witness is `F`'s Inspectable dictionary while the call is checked. The
   * witness is read from `h` again, so `h` must be a name; any other
   * receiver keeps the `Inspectable` bound on `F`.
   */
  protected withHandleWitness(
    written: MemberCallExpression,
    receiver: HirExpression,
    check: (expression: MemberCallExpression) => HirExpression,
  ): HirExpression {
    const expression = this.withHandleFactPattern(written, receiver);
    const handle = nominalGenericParts(readonlyType(receiver.type));
    const type = handle?.arguments[1];
    if (
      expression.callee.name !== "fact" ||
      expression.callee.receiver.kind !== "name" ||
      type === undefined ||
      !this.dataTypes
        .get(handle!.name)
        ?.fields.some((field) => field.name === STRUCTURE_WITNESS_FIELD) ||
      inspectKey(type, this.inspectEnvironment())
    )
      return check(expression);
    const privateMember = this.compilerPrivateMember;
    this.compilerPrivateMember = true;
    let dictionary: HirExpression;
    try {
      dictionary = this.checkExpression({
        kind: "member",
        receiver: expression.callee.receiver,
        name: STRUCTURE_WITNESS_FIELD,
        span: expression.callee.receiver.span,
      });
    } finally {
      this.compilerPrivateMember = privateMember;
    }
    const saved = this.handleWitness;
    this.handleWitness = { type, dictionary };
    try {
      return check(expression);
    } finally {
      this.handleWitness = saved;
    }
  }

  /**
   * `facts_of(f)` (spec/lang/14-annotations.md#function-facts): the call of the
   * `Facts` builder that `function-facts.ts` generated for the module-level
   * function `f`. The argument names `f` and is never evaluated.
   */
  protected factsOfCall(expression: CallExpression): Expression {
    if (expression.typeArguments?.length)
      this.fail("unexpected-type-arguments", "facts_of takes no type arguments", expression.span);
    if (expression.arguments.length !== 1 || expression.argumentSpreads?.some(Boolean))
      this.fail("argument-count", "facts_of expects one function name", expression.span);
    const argument = expression.arguments[0]!;
    const signature =
      argument.kind === "name" &&
      this.resolveLocal(argument.name) === undefined &&
      !this.availableCaptures.has(argument.name) &&
      this.resolveGlobal(argument.name) === undefined
        ? this.visibleSignature(argument.name)
        : undefined;
    const builder = signature && factsOfBuilderName(signature.name);
    if (!builder || !this.signatures.has(builder))
      this.fail(
        "invalid-facts-of-target",
        "facts_of expects the name of a module-level function declaration",
        argument.span,
      );
    return {
      kind: "call",
      callee: { kind: "name", name: builder, span: expression.callee.span },
      arguments: [],
      span: expression.span,
    };
  }
}
