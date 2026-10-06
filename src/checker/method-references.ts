import { arityCode } from "../diagnostics.ts";
import type { ClosureParameter, Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, HirLocal, ValueType } from "../hir.ts";
import {
  functionParts,
  functionType,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  PRIMITIVE_TYPES,
  readonlyType,
  displayType,
} from "../types.ts";
import type { MemberCallExpression } from "./expression-calls.ts";
import {
  genericTypeName,
  inferGenericType,
  matchGenericTypePattern,
  matchImplementationTarget,
  substituteGenericType,
} from "./shared.ts";
import { type QualifiedCallExpression, TraitCallChecker } from "./trait-calls.ts";

type Reference = Extract<Expression, { kind: "qualified-name" }>;

/** The member a method reference names, before its generic parameters are solved. */
interface ReferenceMember {
  /** The receiver parameter's type, for a method; undefined for an associated function. */
  readonly receiver?: ValueType;
  readonly parameters: readonly ValueType[];
  readonly result: ValueType;
  readonly requirements: readonly string[];
  readonly suspending: boolean;
  /** Every generic parameter the types above may name, such as a trait's `Self`. */
  readonly generics: readonly string[];
  /** The member's own generic parameters: the slots of explicit type arguments. */
  readonly ownGenerics: readonly string[];
  readonly substitutions: ReadonlyMap<string, ValueType>;
  /** A trait-owned method is called trait-qualified, as `Trait::name(receiver)`. */
  readonly traitQualified?: boolean;
}

/**
 * Method references (07-functions.md#method-references). A reference is
 * checked as the closure that calls its member: `Counter::read` as
 * `fn(receiver: Counter) -> i32: receiver.read()`, and `counter::read` as a
 * closure over the receiver, which is evaluated once when the reference is made.
 */
export abstract class MethodReferenceChecker extends TraitCallChecker {
  /** The canonical declaration spelling behind an imported type spelling. */
  protected canonicalTypeName(name: string): string {
    if (this.signature.genericParameters.includes(name)) return name;
    return (
      this.dataTypes.get(name)?.name ??
      this.enumTypes.get(name)?.name ??
      this.traitTypes.get(name)?.name ??
      name
    );
  }

  protected abstract checkImplementedMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
    expected?: ValueType,
  ): HirExpression;

  /**
   * `Owner::name(arguments)` as a member call (07-functions.md#r-fn.ref.call):
   * on the value `Owner` names when `firstArgument` is 0, or on the first
   * argument when it is 1.
   */
  protected receiverMemberCall(
    expression: QualifiedCallExpression,
    firstArgument: 0 | 1,
  ): MemberCallExpression {
    const span = expression.callee.span;
    const receiver: Expression =
      firstArgument === 1
        ? expression.arguments[0]!
        : { kind: "name", name: expression.callee.owner, span };
    return {
      ...expression,
      callee: { kind: "member", receiver, name: expression.callee.name, span },
      arguments: expression.arguments.slice(firstArgument),
      argumentNames: expression.argumentNames?.slice(firstArgument),
      argumentSpreads: expression.argumentSpreads?.slice(firstArgument),
    };
  }

  /** `Type::method(receiver, ...)` as the member call `receiver.method(...)`. */
  protected checkReceiverFirstCall(
    expression: QualifiedCallExpression,
    ownerType: ValueType,
    expected?: ValueType,
  ): HirExpression {
    if (expression.argumentNames?.[0] !== undefined || expression.argumentSpreads?.[0])
      this.fail(
        "argument-order",
        "a qualified call's receiver must be the first ordinary argument",
        expression.arguments[0]!.span,
      );
    const receiver = this.checkExpression(expression.arguments[0]!);
    const base = (type: ValueType): string => nominalGenericParts(type)?.name ?? type;
    if (base(readonlyType(receiver.type)) !== base(ownerType))
      this.fail(
        "type-mismatch",
        `expected a '${displayType(ownerType)}' receiver, found ${displayType(receiver.type)}`,
        expression.arguments[0]!.span,
      );
    return this.checkImplementedMemberCall(
      this.receiverMemberCall(expression, 1),
      receiver,
      expected,
    );
  }

  protected checkMethodReference(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    if (expression.kind !== "qualified-name") return undefined;
    if (this.namesReferenceValue(expression.owner))
      return this.checkBoundReference(expression, expected);
    if (expression.genericTypeOwner)
      expression = { ...expression, owner: expression.genericTypeOwner };
    const canonicalOwner = this.canonicalTypeName(expression.owner);
    if (canonicalOwner !== expression.owner) expression = { ...expression, owner: canonicalOwner };
    const member = this.unboundReferenceMember(expression);
    const { parameters, result, requirements } = this.solveReference(
      expression,
      member,
      expected,
      true,
    );
    const span = expression.span;
    const names = parameters.map((_, index) => `$reference${index}`);
    const values: Expression[] = names.map((name) => ({ kind: "name", name, span }));
    const qualified: Expression = {
      kind: "qualified-name",
      owner: expression.owner,
      ownerTypeArguments: expression.ownerTypeArguments,
      name: expression.name,
      span,
    };
    const byMember = member.receiver !== undefined && member.traitQualified !== true;
    const call = this.referenceCall(
      expression,
      member,
      byMember ? { kind: "member", receiver: values[0]!, name: expression.name, span } : qualified,
      byMember ? values.slice(1) : values,
    );
    return this.checkExpression(
      this.referenceClosure(names, member.suspending, call, span),
      functionType(parameters, result, requirements, false, member.suspending),
    );
  }

  /** Whether `Owner` in `Owner::name` names a value, which makes a bound reference. */
  protected namesReferenceValue(owner: string): boolean {
    return (
      !this.traitTypes.has(owner) &&
      Boolean(
        this.resolveLocal(owner) || this.availableCaptures.has(owner) || this.resolveGlobal(owner),
      )
    );
  }

  /**
   * `value::name` (07-functions.md#r-fn.ref.bound): the receiver is evaluated
   * now and bound for the closure, which calls the method on it.
   */
  private checkBoundReference(expression: Reference, expected?: ValueType): HirExpression {
    const span = expression.span;
    const value = this.checkExpression({ kind: "name", name: expression.owner, span });
    const member = this.referenceMember(readonlyType(value.type), expression.name, span);
    if (!member || member.receiver === undefined)
      this.fail(
        "unknown-method",
        member
          ? `'${expression.name}' is an associated function, which a bound reference cannot name; write '${displayType(readonlyType(value.type))}::${expression.name}'`
          : `type '${displayType(readonlyType(value.type))}' has no method '${expression.name}'`,
        span,
      );
    if (mutableInner(member.receiver) !== undefined && mutableInner(value.type) === undefined)
      this.fail(
        "mutable-receiver-required",
        `method '${expression.name}' takes mut self, so a bound reference needs mutable access to '${expression.owner}'`,
        span,
      );
    const { parameters, result, requirements } = this.solveReference(
      expression,
      member,
      expected,
      false,
    );
    const receiver: HirLocal = {
      name: "$receiver",
      type: value.type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span,
    };
    this.locals.push(receiver);
    this.scopes.push(new Map([[receiver.name, receiver]]));
    let closure: HirExpression;
    try {
      const names = parameters.map((_, index) => `$reference${index}`);
      const call = this.referenceCall(
        expression,
        member,
        {
          kind: "member",
          receiver: { kind: "name", name: receiver.name, span },
          name: expression.name,
          span,
        },
        names.map((name) => ({ kind: "name", name, span })),
      );
      closure = this.checkExpression(
        this.referenceClosure(names, member.suspending, call, span),
        functionType(parameters, result, requirements, false, member.suspending),
      );
    } finally {
      this.scopes.pop();
    }
    return {
      kind: "match",
      subject: value,
      representation: "scalar",
      arms: [
        {
          bindings: [{ local: receiver, fieldIndex: -1, type: value.type }],
          body: [{ kind: "expression", expression: closure, span }],
          span,
        },
      ],
      type: closure.type,
      span,
    };
  }

  private referenceCall(
    expression: Reference,
    member: ReferenceMember,
    callee: Expression,
    arguments_: Expression[],
  ): Expression {
    return {
      kind: member.suspending ? "suspend-call" : "call",
      callee,
      ...(expression.typeArguments ? { typeArguments: expression.typeArguments } : {}),
      arguments: arguments_,
      span: expression.span,
    };
  }

  private referenceClosure(
    names: readonly string[],
    suspending: boolean,
    call: Expression,
    span: SourceSpan,
  ): Expression {
    const parameters: ClosureParameter[] = names.map((name) => ({ name, span }));
    return {
      kind: "closure",
      ...(suspending ? { suspending: true } : {}),
      parameters,
      body: [{ kind: "expression", expression: call, span }],
      span,
    };
  }

  /**
   * The reference's function type: its generic parameters come from explicit
   * type arguments, then from the expected function type
   * (07-functions.md#r-fn.ref.generic.instantiate, #r-fn.ref.trait-self).
   */
  private solveReference(
    expression: Reference,
    member: ReferenceMember,
    expected: ValueType | undefined,
    withReceiver: boolean,
  ): { parameters: ValueType[]; result: ValueType; requirements: readonly string[] } {
    const substitutions = new Map(member.substitutions);
    const typeArguments = expression.typeArguments;
    if (typeArguments) {
      // Omitted trailing slots are inferred (types.generic.short-list).
      if (typeArguments.length > member.ownGenerics.length)
        this.fail(
          "argument-count",
          `'${expression.owner}::${expression.name}' expects ${member.ownGenerics.length} type arguments, received ${typeArguments.length}`,
          expression.span,
        );
      typeArguments.forEach((argument, index) => {
        if (argument.name !== "_")
          substitutions.set(member.ownGenerics[index]!, this.resolveType(argument));
      });
    }
    const formal = [
      ...(withReceiver && member.receiver !== undefined ? [member.receiver] : []),
      ...member.parameters,
    ];
    // An argument's expected type may still hold the call's unsolved
    // parameters; only its solved positions instantiate the reference.
    const pending = this.takePendingCallGenerics();
    const callable = expected ? functionParts(readonlyType(expected)) : undefined;
    if (callable && callable.parameters.length === formal.length) {
      formal.forEach((parameter, index) => {
        const actual = callable.parameters[index]!;
        if (!pending(actual)) inferGenericType(parameter, actual, substitutions);
      });
      if (!pending(callable.result))
        inferGenericType(member.result, callable.result, substitutions);
    }
    const unresolved = member.generics.filter((parameter) => !substitutions.has(parameter));
    if (unresolved.length > 0)
      this.fail(
        "cannot-infer-type",
        `'${expression.owner}::${expression.name}' needs ${unresolved.map((parameter) => `'${parameter}'`).join(", ")} from type arguments or an expected function type`,
        expression.span,
      );
    return {
      // A readonly parameter takes the expected `mut` type, which it accepts.
      parameters: formal.map((parameter, index) => {
        const solved = substituteGenericType(parameter, substitutions);
        const actual = callable?.parameters[index];
        return actual !== undefined && !pending(actual) && mutableInner(actual) === solved
          ? actual
          : solved;
      }),
      result: substituteGenericType(member.result, substitutions),
      requirements: member.requirements.map((key) => substituteGenericType(key, substitutions)),
    };
  }

  /** The member of `Owner::name` whose owner is a trait, a type parameter, or a type. */
  private unboundReferenceMember(expression: Reference): ReferenceMember {
    const { owner, name, span } = expression;
    const trait = this.traitTypes.get(owner);
    if (trait) {
      const traitArguments = (expression.ownerTypeArguments ?? []).map((argument) =>
        this.resolveType(argument),
      );
      if (traitArguments.length !== trait.genericParameters.length)
        this.fail(
          arityCode(traitArguments.length, trait.genericParameters.length),
          `trait '${displayType(trait.name)}' expects ${trait.genericParameters.length} type arguments`,
          span,
        );
      // A method or an associated function (07-functions.md#r-fn.ref.associated);
      // an associated one is called as `Trait::name(...)`, whose `Self` the
      // reference's expected type solves (07-functions.md#r-fn.ref.trait-self).
      const found = [
        ...this.findTraitMethods(trait, name),
        ...this.findTraitMethods(trait, name, [], new Set(), true),
      ];
      if (found.length === 0)
        this.fail(
          "unknown-method",
          `trait '${displayType(trait.name)}' has no method or associated function '${name}' to reference`,
          span,
        );
      if (found.length > 1)
        this.fail("ambiguous-method", `'${name}' is a member of several traits`, span);
      const { method, path, trait: owning } = found[0]!;
      const owningArguments = this.resolveTraitPath(trait, traitArguments, path).arguments;
      return {
        receiver: method.associated
          ? undefined
          : method.receiverMutable
            ? mutableType("generic:Self")
            : "generic:Self",
        parameters: method.parameters,
        result: method.result,
        requirements: method.requirements,
        suspending: method.suspending,
        generics: ["Self", ...method.genericParameters],
        ownGenerics: method.genericParameters,
        substitutions: new Map(
          owning.genericParameters.map((parameter, index) => [parameter, owningArguments[index]!]),
        ),
        traitQualified: true,
      };
    }
    const ownerType = this.signature.genericParameters.includes(owner)
      ? `generic:${owner}`
      : expression.ownerTypeArguments
        ? nominalGenericType(
            owner,
            expression.ownerTypeArguments.map((argument) => this.resolveType(argument)),
          )
        : owner;
    const member = this.referenceMember(ownerType, name, span);
    if (member) return member;
    if (
      !genericTypeName(ownerType) &&
      !this.dataTypes.has(owner) &&
      !this.enumTypes.has(owner) &&
      !this.inherentMethods.some((method) => this.inherentOwner(method.targetType) === owner)
    )
      this.fail("unknown-type", `unknown method-reference owner '${owner}'`, span);
    const field = this.dataTypes.get(owner)?.fields.some((candidate) => candidate.name === name);
    this.fail(
      "unknown-method",
      field
        ? `'${name}' is a field of '${owner}'; '::' names only methods and associated functions, so write a closure`
        : `type '${owner}' has no method or associated function '${name}'`,
      span,
    );
  }

  /** Whether `ownerType` has a method, not an associated function, named `name`. */
  protected hasReceiverMethod(ownerType: ValueType, name: string): boolean {
    const base = nominalGenericParts(ownerType)?.name ?? ownerType;
    if (
      this.inherentMethods.some(
        (method) =>
          method.name === name &&
          !method.associated &&
          this.inherentOwner(method.targetType) === base,
      )
    )
      return true;
    return this.implementations.some((implementation) => {
      if (!matchImplementationTarget(implementation, ownerType, new Map())) return false;
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      return trait?.methods.some((method) => method.name === name && !method.associated) === true;
    });
  }

  private inherentOwner(targetType: ValueType): string {
    return nominalGenericParts(targetType)?.name ?? targetType;
  }

  /**
   * The method or associated function `name` of `ownerType`, found as a
   * qualified call finds it: inherent members first, then the traits the type
   * implements, and a type parameter's members through its bounds
   * (07-functions.md#r-fn.ref.lookup).
   */
  protected referenceMember(
    ownerType: ValueType,
    name: string,
    span: SourceSpan,
  ): ReferenceMember | undefined {
    const generic = genericTypeName(ownerType);
    if (generic) {
      const candidates = this.signature.genericBounds.flatMap((bound) => {
        if (bound.parameter !== generic) return [];
        const trait = this.traitTypes.get(bound.traitName);
        if (!trait) return [];
        return [
          ...this.findTraitMethods(trait, name),
          ...this.findTraitMethods(trait, name, [], new Set(), true),
        ].map((selected) => ({ bound, trait, selected }));
      });
      if (candidates.length > 1)
        this.fail("ambiguous-method", `several bounds on '${generic}' supply '${name}'`, span);
      const candidate = candidates[0];
      if (!candidate) return undefined;
      const { method, path, trait: owning } = candidate.selected;
      const owningArguments = this.resolveTraitPath(
        candidate.trait,
        candidate.bound.traitArguments,
        path,
      ).arguments;
      const substitutions = new Map<string, ValueType>([
        ...owning.genericParameters.map(
          (parameter, index) => [parameter, owningArguments[index]!] as const,
        ),
        ["Self", ownerType],
      ]);
      return this.traitReferenceMember(method, ownerType, substitutions);
    }
    const base = nominalGenericParts(ownerType)?.name ?? ownerType;
    const inherent = this.inherentMethods.find(
      (method) => method.name === name && this.inherentOwner(method.targetType) === base,
    );
    if (inherent) {
      this.requireInherentVisible(inherent, span);
      const signature = this.signatures.get(inherent.functionName)!;
      const substitutions = new Map<string, ValueType>();
      matchGenericTypePattern(inherent.targetType, ownerType, substitutions);
      const targetParameters = inherent.targetGenericParameters ?? [];
      return {
        ...(inherent.associated
          ? {}
          : {
              receiver: inherent.receiverMutable
                ? mutableType(inherent.targetType)
                : inherent.targetType,
            }),
        parameters: inherent.associated ? signature.parameters : signature.parameters.slice(1),
        result: signature.result,
        requirements: signature.requirements,
        suspending: signature.suspending,
        generics: signature.genericParameters,
        ownGenerics: signature.genericParameters.filter(
          (parameter) => !targetParameters.includes(parameter),
        ),
        substitutions,
      };
    }
    const candidates = this.implementations.flatMap((implementation) => {
      const substitutions = new Map<string, ValueType>();
      if (!matchImplementationTarget(implementation, ownerType, substitutions)) return [];
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      const method = trait?.methods.find((candidate) => candidate.name === name);
      // Only an available trait's member is a candidate (07-functions.md#r-fn.ref.lookup).
      if (!trait || !method || !this.traitAvailable(trait.name, span)) return [];
      trait.genericParameters.forEach((parameter, index) =>
        substitutions.set(
          parameter,
          substituteGenericType(implementation.traitArguments[index]!, substitutions),
        ),
      );
      substitutions.set("Self", ownerType);
      return [{ method, substitutions }];
    });
    if (candidates.length > 1)
      this.fail(
        "ambiguous-method",
        `several traits give '${displayType(ownerType)}' a '${name}'`,
        span,
      );
    const candidate = candidates[0];
    return candidate
      ? this.traitReferenceMember(candidate.method, ownerType, candidate.substitutions)
      : undefined;
  }

  private traitReferenceMember(
    method: {
      readonly associated: boolean;
      readonly receiverMutable: boolean;
      readonly parameters: readonly ValueType[];
      readonly result: ValueType;
      readonly requirements: readonly string[];
      readonly suspending: boolean;
      readonly genericParameters: readonly string[];
    },
    ownerType: ValueType,
    substitutions: ReadonlyMap<string, ValueType>,
  ): ReferenceMember {
    return {
      ...(method.associated
        ? {}
        : {
            // A primitive `mut self` receiver is the plain type
            // (04-type-system.md#r-types.prim.no-mut.self-type).
            receiver:
              method.receiverMutable && !PRIMITIVE_TYPES.has(ownerType)
                ? mutableType(ownerType)
                : ownerType,
          }),
      parameters: method.parameters,
      result: method.result,
      requirements: method.requirements,
      suspending: method.suspending,
      generics: method.genericParameters,
      ownGenerics: method.genericParameters,
      substitutions,
    };
  }
}
