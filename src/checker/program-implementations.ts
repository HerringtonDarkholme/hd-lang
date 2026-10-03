import { extendsInspectable, inspectKey, usesStandardInspect } from "./inspectable.ts";
import { INSPECTABLE, INSPECTABLE_MEMBERS, TUPLE_TRAIT } from "./standard-traits.ts";
import type { Diagnostic } from "../diagnostics.ts";
import {
  listVararg,
  type Expression,
  type FunctionDecl,
  type GenericBound,
  type ImplDecl,
  type MethodDecl,
  type Parameter,
  type TypeRef,
} from "../ast.ts";
import { traitDefaultDeclarations } from "./member-lookup.ts";
import type { HirSupertrait, HirTrait, NumericFamily, ValueType } from "../hir.ts";
import { numericType } from "../numeric.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  PRIMITIVE_TYPES,
  readonlyType,
  typeSourceText,
  tupleParts,
  tupleType,
} from "../types.ts";
import {
  collectRowParameterReferences,
  genericTypeName,
  isKnownType,
  matchTraitImplementation,
  normalizedRequirements,
  requirementKeysMayCollide,
  resolveGenericRequirement,
  resolveGenericType,
  resolveTraitType,
  rowParameterName,
  sameRequirements,
  substituteGenericType,
  typeName,
} from "./shared.ts";
import { resolveRequirementKeyTypes } from "./requirement-keys.ts";
import {
  traitImpliesValueCategory,
  typeSatisfiesValueCategory,
  type ValueCategory,
} from "./value-categories.ts";

import type {
  ImplementationMethodPreparation,
  ImplementationPreparation,
  ProgramCheckContext,
} from "./program-context.ts";

/** The sealed `std.num` traits (09-traits.md#numeric-traits). */
const SEALED_NUMERIC_TRAITS: ReadonlySet<string> = new Set([
  "std.num.Num",
  "std.num.Integer",
  "std.num.Float",
]);

interface TraitSpecialization {
  readonly arguments: readonly string[];
  readonly substitutions: ReadonlyMap<string, string>;
}

function parameterKinds(
  implementation: ImplDecl,
  method?: MethodDecl,
): { readonly types: Set<string>; readonly rows: Set<string> } {
  const rows = new Set([...(implementation.rowParameters ?? []), ...(method?.rowParameters ?? [])]);
  return {
    types: new Set(
      [...implementation.genericParameters, ...(method?.genericParameters ?? [])].filter(
        (parameter) => !rows.has(parameter),
      ),
    ),
    rows,
  };
}

function methodRequirements(
  method: MethodDecl,
  kinds: ReturnType<typeof parameterKinds>,
  traitTypes: ReadonlyMap<string, HirTrait>,
): readonly string[] {
  return normalizedRequirements(
    method.requirements
      .flatMap((requirement) => resolveGenericRequirement(requirement, kinds.rows))
      .map((requirement) =>
        rowParameterName(requirement)
          ? requirement
          : resolveRequirementKeyTypes(
              resolveGenericType(requirement, kinds.types, kinds.rows),
              (type) => resolveTraitType(type, traitTypes),
            ),
      ),
  );
}

/** Whether `candidate` is in scope at the declaration point of `source`. */
export function implementationVisibleFrom(candidate: ImplDecl, source: ImplDecl): boolean {
  return (
    candidate.localImplementation === undefined ||
    source.localImplementations?.includes(candidate.localImplementation) === true
  );
}

interface RegisteredImplementationTarget {
  readonly genericParameters: readonly string[];
  readonly traitArguments: readonly string[];
  readonly traitIndex: number;
  readonly targetType: string;
  readonly family?: NumericFamily;
}

/**
 * The types each sealed numeric trait lists, by its name in the program:
 * the targets of std's `impl Num for i8` and the like
 * (09-traits.md#sealed-traits).
 */
function sealedNumericTypes(context: ProgramCheckContext): Map<string, ValueType[]> {
  const types = new Map<string, ValueType[]>();
  for (const implementation of context.program.implementations) {
    const trait = implementation.traitName && context.traitTypes.get(implementation.traitName);
    if (
      !trait ||
      !implementation.standard ||
      implementation.genericParameters.length > 0 ||
      !SEALED_NUMERIC_TRAITS.has(trait.standardName ?? "")
    )
      continue;
    types.set(trait.name, [...(types.get(trait.name) ?? []), implementation.targetName]);
  }
  return types;
}

/**
 * The family of a standard-library implementation whose target is a bare
 * parameter and whose every parameter is bounded by exactly one sealed
 * numeric trait, as `impl[N < Integer, C < Integer] Shl[C] for N`
 * (09-traits.md#r-trait.target.numeric-family).
 */
function numericFamily(
  implementation: ImplDecl,
  sealed: ReadonlyMap<string, readonly ValueType[]>,
): NumericFamily | undefined {
  if (
    !implementation.standard ||
    !implementation.genericParameters.includes(implementation.targetName)
  )
    return undefined;
  const family: Record<string, readonly ValueType[]> = {};
  for (const parameter of implementation.genericParameters) {
    const bounds = implementation.genericBounds.filter((bound) => bound.parameter === parameter);
    const only = bounds.length === 1 ? bounds[0]! : undefined;
    const types =
      only && only.traits.length === 1 && !only.bindings?.length
        ? sealed.get(only.traits[0]!)
        : undefined;
    if (!types) return undefined;
    family[parameter] = types;
  }
  return family;
}

/** Each way of giving every parameter of `family` one of its types. */
function familyInstances(family: NumericFamily): Map<string, ValueType>[] {
  let instances: Map<string, ValueType>[] = [new Map()];
  for (const [parameter, types] of Object.entries(family))
    instances = instances.flatMap((partial) =>
      types.map((type) => new Map([...partial, [parameter, type]])),
    );
  return instances;
}

/** The trait arguments and target of each implementation a family implementation stands for. */
function implementationHeads(
  traitArguments: readonly ValueType[],
  targetType: ValueType,
  family: NumericFamily | undefined,
): ValueType[][] {
  const head = [...traitArguments, readonlyType(targetType)];
  if (!family) return [head];
  return familyInstances(family).map((instance) =>
    head.map((type) => substituteGenericType(type, instance)),
  );
}

// 09 Overlap (TQ-28): two implementations of one trait overlap when one
// substitution unifies their trait arguments and full targets together.
function implementationHeadsMayUnify(
  left: readonly string[],
  right: readonly string[],
  rightGenerics: readonly string[],
): boolean {
  // Rename the other implementation's parameters apart before unifying, with
  // no `$`, which would start a function type's row.
  const renamed = new Map(
    rightGenerics.map((parameter) => [parameter, `generic:%other.${parameter}`] as const),
  );
  const leftKey = tupleType(left);
  const rightKey = tupleType(right.map((argument) => substituteGenericType(argument, renamed)));
  return leftKey === rightKey || requirementKeysMayCollide(leftKey, rightKey);
}

function specializeTrait(
  implementation: ImplDecl,
  trait: HirTrait,
  context: ProgramCheckContext,
): TraitSpecialization | undefined {
  const application = nominalGenericParts(implementation.traitName!);
  if (trait.genericParameters.length !== (application?.arguments.length ?? 0)) {
    context.diagnostics.push({
      code: "generic-arity",
      message: `trait '${trait.name}' expects ${trait.genericParameters.length} type arguments`,
      span: implementation.span,
    });
    return undefined;
  }
  const kinds = parameterKinds(implementation);
  const arguments_ =
    application?.arguments.map(
      (argument) =>
        typeName(
          { name: argument, span: implementation.span },
          context.dataTypes,
          context.enumTypes,
          context.traitTypes,
          context.diagnostics,
          kinds.types,
          kinds.rows,
        ) ?? "void",
    ) ?? [];
  return {
    arguments: arguments_,
    substitutions: new Map(
      trait.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
    ),
  };
}

// A numeric-family implementation overlaps another when one of the
// implementations it stands for does (09-traits.md#r-trait.overlap.numeric-family).
function registerImplementationPair(
  implementation: ImplDecl,
  trait: HirTrait,
  traitArguments: readonly string[],
  targetType: string,
  family: NumericFamily | undefined,
  targets: RegisteredImplementationTarget[],
  diagnostics: Diagnostic[],
): boolean {
  const heads = implementationHeads(traitArguments, targetType, family);
  const conflict = targets.find(
    (candidate) =>
      candidate.traitIndex === trait.index &&
      heads.some((head) =>
        implementationHeads(candidate.traitArguments, candidate.targetType, candidate.family).some(
          (other) => implementationHeadsMayUnify(head, other, candidate.genericParameters),
        ),
      ),
  );
  if (conflict) {
    diagnostics.push({
      code: "overlapping-impl",
      message: `${typeSourceText(implementation.targetName)} overlaps the ${trait.name} implementation for '${typeSourceText(readonlyType(conflict.targetType).replaceAll("generic:", ""))}': their heads unify`,
      span: implementation.span,
    });
    return false;
  }
  targets.push({
    genericParameters: implementation.genericParameters,
    traitArguments,
    traitIndex: trait.index,
    targetType,
    ...(family ? { family } : {}),
  });
  return true;
}

// 09 Implementation Targets: no outer `mut`, and no bare type parameter
// except a numeric family's.
function checkImplementationTarget(
  implementation: ImplDecl,
  diagnostics: Diagnostic[],
  family?: NumericFamily,
): boolean {
  if (mutableInner(implementation.targetName) !== undefined) {
    diagnostics.push({
      code: "mutable-impl-target",
      message: `implementation target '${typeSourceText(readonlyType(implementation.targetName))}' cannot be written with mut; permission belongs to receivers and bounds`,
      span: implementation.span,
    });
    return false;
  }
  if (!family && implementation.genericParameters.includes(implementation.targetName)) {
    diagnostics.push({
      code: "bare-parameter-impl-target",
      message: `implementation target '${typeSourceText(implementation.targetName)}' is a bare type parameter; a target must start with a type constructor`,
      span: implementation.span,
    });
    return false;
  }
  return true;
}

function substituteSelfType(type: TypeRef, targetName: string): TypeRef {
  const generic = resolveGenericType(type.name, new Set(["Self"]), new Set());
  return {
    name: substituteGenericType(generic, new Map([["Self", targetName]])),
    span: type.span,
  };
}

// The AST keys whose values are written types (`TypeRef`s).
const WRITTEN_TYPE_KEYS = new Set(["type", "annotation", "result", "typeArguments"]);

/**
 * A trait's default method as one implementation's own: each of the trait's
 * type parameters becomes the implementation's written trait argument, in
 * the signature and in the types the body writes, as in `List[T]` becoming
 * `List[i32]` for `impl Iterator[i32] for Countdown`.
 */
function instantiateDefault(
  method: MethodDecl,
  implementation: ImplDecl,
  trait: HirTrait,
): MethodDecl {
  if (trait.genericParameters.length === 0) return method;
  const written = nominalGenericParts(implementation.traitName!)?.arguments ?? [];
  const generics = new Set(trait.genericParameters);
  const substitutions = new Map(
    trait.genericParameters.map((parameter, index) => [parameter, written[index] ?? parameter]),
  );
  const instantiate = (type: TypeRef): TypeRef => ({
    ...type,
    name: substituteGenericType(resolveGenericType(type.name, generics), substitutions),
  });
  const visit = (node: unknown, writtenType: boolean): unknown => {
    if (Array.isArray(node)) return node.map((item) => visit(item, writtenType));
    if (!node || typeof node !== "object") return node;
    const record = node as Record<string, unknown>;
    if (writtenType && typeof record.name === "string" && !("kind" in record))
      return instantiate(record as unknown as TypeRef);
    const result: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(record))
      result[key] = key === "span" ? child : visit(child, WRITTEN_TYPE_KEYS.has(key));
    return result;
  };
  return visit(method, false) as MethodDecl;
}

function substituteSelfParameter(parameter: Parameter, targetName: string): Parameter {
  // A primitive has no `mut` form, so `mut self` is the plain `Self`
  // (04-type-system.md#r-types.prim.no-mut.self-type).
  if (primitiveMutSelf(parameter, targetName))
    return { ...parameter, type: { name: targetName, span: parameter.type.span } };
  return {
    ...parameter,
    type: substituteSelfType(parameter.type, targetName),
  };
}

function primitiveMutSelf(parameter: Parameter | undefined, targetName: string): boolean {
  return (
    parameter?.name === "self" &&
    parameter.type.name === "mut:Self" &&
    PRIMITIVE_TYPES.has(targetName)
  );
}

interface RegisteredInherentMember {
  readonly name: string;
  readonly targetType: ValueType;
  readonly genericParameters: readonly string[];
}

/**
 * Why an inherent implementation outside `std` cannot have this target: a
 * tuple, a trait value, or a type this package does not own
 * (09-traits.md#r-trait.own.inherent).
 */
function invalidInherentTarget(
  implementation: ImplDecl,
  targetBase: string,
  context: ProgramCheckContext,
): Diagnostic {
  const { dataTypes, enumTypes, traitTypes } = context;
  const span = implementation.span;
  const target = implementation.targetName;
  const kinds = parameterKinds(implementation);
  if (tupleParts(target) !== undefined)
    return {
      code: "invalid-impl-target",
      message: `an inherent implementation cannot target the tuple type '${typeSourceText(target)}'; tuples get only trait implementations`,
      span,
    };
  if (traitTypes.has(targetBase))
    return {
      code: "trait-value-impl-target",
      message: `an inherent implementation cannot target the trait value type '${typeSourceText(target)}'; add a provided method to the trait instead`,
      span,
    };
  const known = typeName(
    { name: target, span },
    dataTypes,
    enumTypes,
    traitTypes,
    [],
    kinds.types,
    kinds.rows,
  );
  if (known !== undefined)
    return {
      code: "orphan-impl",
      message: `an inherent implementation of '${typeSourceText(target)}' must be declared in the package that owns it`,
      span,
    };
  return {
    code: "unknown-type",
    message: `unknown inherent implementation target '${typeSourceText(target)}'`,
    span,
  };
}

function prepareInherentImplementation(
  implementation: ImplDecl,
  implementationIndex: number,
  members: RegisteredInherentMember[],
  context: ProgramCheckContext,
): void {
  const { diagnostics, dataTypes, enumTypes, traitTypes, inherentMethods, inherentDeclarations } =
    context;
  const targetBase =
    nominalGenericParts(implementation.targetName)?.name ?? implementation.targetName;
  const target = dataTypes.get(targetBase) ?? enumTypes.get(targetBase);
  const implementationKinds = parameterKinds(implementation);
  const resolveTarget = (): ValueType | undefined =>
    typeName(
      { name: implementation.targetName, span: implementation.span },
      dataTypes,
      enumTypes,
      traitTypes,
      diagnostics,
      implementationKinds.types,
      implementationKinds.rows,
      hashableParameters(implementation),
    );
  let targetType: ValueType | undefined;
  if (!target && implementation.standard) {
    // `std` declares inherent methods on built-in types such as `string`,
    // `T?`, and `List[T]` (09-traits.md#r-trait.own.inherent.std).
    targetType = resolveTarget();
  } else if (!target) {
    diagnostics.push(invalidInherentTarget(implementation, targetBase, context));
    return;
  } else if (target.genericParameters.length > 0 && targetBase === implementation.targetName) {
    diagnostics.push({
      code: "unsupported-generic-impl",
      message: `inherent implementation of generic type '${typeSourceText(implementation.targetName)}' requires explicit generic parameters`,
      span: implementation.span,
    });
    return;
  } else {
    targetType = targetBase === implementation.targetName ? targetBase : resolveTarget();
  }
  if (targetType === undefined) return;
  for (const method of implementation.methods) {
    // 09 Inherent Member Names: two members with one name clash only when
    // their targets unify.
    const clash = members.some(
      (member) =>
        member.name === method.name &&
        implementationHeadsMayUnify([targetType], [member.targetType], member.genericParameters),
    );
    if (clash) {
      diagnostics.push({
        code: "duplicate-inherent-member",
        message: `inherent method '${typeSourceText(implementation.targetName)}.${method.name}' is declared more than once for unifying targets`,
        span: method.span,
      });
      continue;
    }
    members.push({
      name: method.name,
      targetType,
      genericParameters: implementation.genericParameters,
    });
    const associated = method.parameters[0]?.name !== "self";
    const sourceParameters = associated ? method.parameters : method.parameters.slice(1);
    const kinds = parameterKinds(implementation, method);
    sourceParameters.forEach((parameter, parameterIndex) => {
      if (parameter.variadic && parameterIndex !== sourceParameters.length - 1) {
        diagnostics.push({
          code: "nonfinal-vararg",
          message: `variadic parameter '${parameter.name}' must be the final parameter`,
          span: parameter.span,
        });
      }
    });
    const parameters = sourceParameters.map((parameter) => {
      const resolved =
        typeName(
          substituteSelfType(parameter.type, implementation.targetName),
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          kinds.types,
          kinds.rows,
          hashableParameters(implementation, method),
        ) ?? "void";
      return resolved;
    });
    const result =
      typeName(
        substituteSelfType(method.result, implementation.targetName),
        dataTypes,
        enumTypes,
        traitTypes,
        diagnostics,
        kinds.types,
        kinds.rows,
        hashableParameters(implementation, method),
      ) ?? "void";
    const functionName = `$inherent${implementationIndex}.${method.name}`;
    inherentMethods.push({
      sourceMethod: method,
      targetType,
      ...(implementation.genericParameters.length > 0
        ? { targetGenericParameters: implementation.genericParameters }
        : {}),
      name: method.name,
      public: method.public === true,
      associated,
      receiverMutable:
        !associated &&
        method.parameters[0]!.type.name === "mut:Self" &&
        !primitiveMutSelf(method.parameters[0], implementation.targetName),
      parameters,
      parameterNames: sourceParameters.map((parameter) => parameter.name),
      variadic: listVararg(sourceParameters.at(-1)),
      suspending: method.suspending,
      result,
      requirements: methodRequirements(method, kinds, traitTypes),
      functionName,
      span: method.span,
      ...(implementation.localImplementation !== undefined
        ? { localImplementation: implementation.localImplementation }
        : {}),
    });
    inherentDeclarations.push({
      kind: "function",
      name: functionName,
      suspending: method.suspending,
      genericParameters: [...implementation.genericParameters, ...method.genericParameters],
      rowParameters: [...(implementation.rowParameters ?? []), ...(method.rowParameters ?? [])],
      genericBounds: [...implementation.genericBounds, ...method.genericBounds],
      parameters: method.parameters.map((parameter) =>
        substituteSelfParameter(parameter, implementation.targetName),
      ),
      result: substituteSelfType(method.result, implementation.targetName),
      requirements: method.requirements,
      ...(method.resultOmitted ? { resultOmitted: true } : {}),
      // A public method without a clause has the empty row
      // (11-requirements-and-suspension.md#r-req.row.omitted.empty-pub).
      ...(method.requirementsOmitted && !method.public ? { requirementsOmitted: true } : {}),
      ...(implementation.standard ? { standard: true } : {}),
      ...(implementation.localImplementations
        ? { localImplementations: implementation.localImplementations }
        : {}),
      ...selfDefaults(method, implementation.targetName),
      body: method.body ?? [],
      span: method.span,
    });
  }
}

/** A method's type-argument defaults, with `Self` read as the implementation's target. */
function selfDefaults(
  method: MethodDecl,
  targetName: string,
): Pick<FunctionDecl, "genericDefaults"> {
  if (!method.genericDefaults) return {};
  return {
    genericDefaults: Object.fromEntries(
      Object.entries(method.genericDefaults).map(([name, type]) => [
        name,
        substituteSelfType(type, targetName),
      ]),
    ),
  };
}

/**
 * Whether an implementation method repeats the trait method's defaults, slot
 * by slot (09-traits.md#r-trait.impl.generics.default).
 */
function sameMethodDefaults(
  required: MethodDecl | undefined,
  method: MethodDecl,
  renaming: ReadonlyMap<string, ValueType>,
  traitGenerics: ReadonlySet<string>,
  methodGenerics: ReadonlySet<string>,
): boolean {
  const requiredParameters = required?.genericParameters ?? [];
  return requiredParameters.every((name, index) => {
    const renamed = method.genericParameters[index];
    const expected = required?.genericDefaults?.[name];
    const actual = renamed === undefined ? undefined : method.genericDefaults?.[renamed];
    if (expected === undefined || actual === undefined) return expected === actual;
    return (
      substituteGenericType(resolveGenericType(expected.name, traitGenerics), renaming) ===
      resolveGenericType(actual.name, methodGenerics)
    );
  });
}

/**
 * The type parameters of an implementation, and of one of its methods, that
 * may key a map: those bounded by `Eq` and `Hash` (09-traits.md#r-trait.hash.map-key).
 */
function hashableParameters(
  implementation: ImplDecl,
  method?: { readonly genericBounds?: readonly GenericBound[] },
): Set<string> {
  const bounds = [...implementation.genericBounds, ...(method?.genericBounds ?? [])];
  const bounded = (name: string, trait: string): boolean =>
    bounds.some((bound) => bound.parameter === name && bound.traits.includes(trait));
  return new Set(
    bounds
      .map((bound) => bound.parameter)
      .filter((name) => bounded(name, "Eq") && bounded(name, "Hash")),
  );
}

function resolveImplementationTarget(
  implementation: ImplDecl,
  context: ProgramCheckContext,
): string | undefined {
  const { diagnostics, dataTypes, enumTypes, traitTypes } = context;
  // A parameter in a function type's row position, as `R` in
  // `Fn[Args, O, $ R]`, is a row parameter (07-functions.md#r-fn.type.ctor.row).
  const rowParameters = new Set<string>();
  collectRowParameterReferences(
    implementation.targetName,
    new Set(implementation.genericParameters),
    rowParameters,
  );
  for (const parameter of implementation.rowParameters ?? []) rowParameters.add(parameter);
  const typeParameters = new Set(
    implementation.genericParameters.filter((parameter) => !rowParameters.has(parameter)),
  );
  const targetType =
    typeName(
      { name: implementation.targetName, span: implementation.span },
      dataTypes,
      enumTypes,
      traitTypes,
      diagnostics,
      typeParameters,
      rowParameters,
      // A std target may be a map over an unbounded key (lib/std/iter.hd).
      implementation.standard
        ? new Set(implementation.genericParameters)
        : hashableParameters(implementation),
    ) ?? "void";
  if (isKnownType(targetType, dataTypes, enumTypes, traitTypes)) return targetType;
  diagnostics.push({
    code: "unknown-type",
    message: `unknown implementation target '${typeSourceText(implementation.targetName)}'`,
    span: implementation.span,
  });
  return undefined;
}

// 09 Implementation Ownership: the package must own the trait, the target's
// constructor, or the outer constructor of a trait argument (the last never
// for a bare-parameter target).
function checkImplementationOwnership(
  implementation: ImplDecl,
  trait: HirTrait,
  traitArguments: readonly string[],
  targetType: string,
  context: ProgramCheckContext,
): boolean {
  const { program, dataTypes, enumTypes } = context;
  const constructorOf = (type: string): string =>
    nominalGenericParts(readonlyType(type))?.name ?? readonlyType(type);
  const isLocalConstructor = (type: string): boolean => {
    const constructor = constructorOf(type);
    const data = dataTypes.get(constructor);
    if (data) return data.standardName === undefined;
    const enumType = enumTypes.get(constructor);
    return enumType !== undefined && enumType.standardName === undefined;
  };
  // A std trait joined into the program, such as `Iterator`, stays foreign.
  const traitIsLocal = program.traits.some(
    (declaration) => declaration.name === trait.name && declaration.standardName === undefined,
  );
  const argumentIsLocal =
    genericTypeName(readonlyType(targetType)) === undefined &&
    traitArguments.some(isLocalConstructor);
  // `std` declares the prelude traits it implements, such as `Debug`.
  if (implementation.standard) return true;
  if (traitIsLocal || isLocalConstructor(targetType) || argumentIsLocal) return true;
  context.diagnostics.push({
    code: "orphan-impl",
    message: `implementation of nonlocal trait '${trait.name}' for nonlocal type '${typeSourceText(constructorOf(targetType))}' is not allowed`,
    span: implementation.span,
  });
  return false;
}

function prepareAssociatedTypes(
  implementation: ImplDecl,
  trait: HirTrait,
  context: ProgramCheckContext,
): ValueType[] {
  const { dataTypes, diagnostics, enumTypes, traitTypes } = context;
  const bindings = new Map<string, (typeof implementation.associatedTypes)[number]>();
  for (const binding of implementation.associatedTypes) {
    if (bindings.has(binding.name))
      diagnostics.push({
        code: "duplicate-impl-member",
        message: `implementation member '${binding.name}' is declared more than once`,
        span: binding.span,
      });
    bindings.set(binding.name, binding);
    if (!trait.associatedTypes.some((associated) => associated.name === binding.name))
      diagnostics.push({
        code: "extra-trait-member",
        message: `associated type '${binding.name}' is not declared by trait ${trait.name}`,
        span: binding.span,
      });
  }
  const kinds = parameterKinds(implementation);
  return trait.associatedTypes.map((associated) => {
    const binding = bindings.get(associated.name);
    if (!binding?.value) {
      diagnostics.push({
        code: "missing-associated-type",
        message: `${typeSourceText(implementation.targetName)} does not bind ${trait.name}.${associated.name}`,
        span: implementation.span,
      });
      return "void";
    }
    return (
      typeName(
        binding.value,
        dataTypes,
        enumTypes,
        traitTypes,
        diagnostics,
        kinds.types,
        kinds.rows,
      ) ?? "void"
    );
  });
}

export function prepareImplementations(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes, implementationPreparations } =
    context;
  const inherentMembers: RegisteredInherentMember[] = [];
  const implementationTargets: RegisteredImplementationTarget[] = [];
  const delegations: Delegation[] = [];
  const orderedImplementationEntries = [...program.implementations.entries()].sort(
    (left, right) =>
      Number(left[1].traitName !== undefined) - Number(right[1].traitName !== undefined),
  );
  const sealed = sealedNumericTypes(context);
  for (const [implementationIndex, implementation] of orderedImplementationEntries) {
    const family = numericFamily(implementation, sealed);
    if (!checkImplementationTarget(implementation, diagnostics, family)) continue;
    if (implementation.traitName === undefined) {
      prepareInherentImplementation(implementation, implementationIndex, inherentMembers, context);
      continue;
    }
    const traitApplication = nominalGenericParts(implementation.traitName);
    const traitName = traitApplication?.name ?? implementation.traitName;
    if (traitName === INSPECTABLE && usesStandardInspect(context.imports)) {
      diagnostics.push({
        code: "sealed-trait-implementation",
        message:
          "Inspectable is sealed: the compiler supplies its implementation for every inspectable type",
        span: implementation.span,
      });
      continue;
    }
    if (traitName === "Suspend") {
      diagnostics.push({
        code: "sealed-trait-implementation",
        message: "Suspend[T] is a sealed runtime protocol and cannot be implemented by user code",
        span: implementation.span,
      });
      continue;
    }
    if (traitName === "AnyVal" || traitName === "AnyRef") {
      diagnostics.push({
        code: "sealed-trait-implementation",
        message: `${traitName} is a sealed value-category trait and cannot be implemented by user code`,
        span: implementation.span,
      });
      continue;
    }
    const trait = traitTypes.get(traitName);
    if (!trait) {
      diagnostics.push({
        code: "unknown-trait",
        message: `unknown trait '${implementation.traitName}'`,
        span: implementation.span,
      });
      continue;
    }
    if (trait.standardName === TUPLE_TRAIT) {
      diagnostics.push({
        code: "sealed-trait-implementation",
        message: `${trait.name} is sealed: the compiler implements it for every tuple type`,
        span: implementation.span,
      });
      continue;
    }
    // The numeric traits stand for the primitive number types, and only
    // the standard library implements them (09-traits.md#r-trait.num.sealed).
    if (
      trait.standardName &&
      SEALED_NUMERIC_TRAITS.has(trait.standardName) &&
      !implementation.standard
    ) {
      diagnostics.push({
        code: "sealed-trait-implementation",
        message: `${trait.name} is sealed: only the standard library implements it, for the primitive number types`,
        span: implementation.span,
      });
      continue;
    }
    // 09 Implementation Targets: a trait value type is never a target.
    const targetBase = nominalGenericParts(implementation.targetName)?.name;
    if (traitTypes.has(targetBase ?? implementation.targetName)) {
      diagnostics.push({
        code: "trait-value-impl-target",
        message: `implementation target '${typeSourceText(implementation.targetName)}' is a trait value type; implement the trait for concrete types instead`,
        span: implementation.span,
      });
      continue;
    }
    const specialization = specializeTrait(implementation, trait, context);
    if (!specialization) continue;
    const traitArguments = specialization.arguments;
    const traitSubstitutions = specialization.substitutions;
    const targetType = resolveImplementationTarget(implementation, context);
    if (targetType === undefined) continue;
    if (!checkImplementationOwnership(implementation, trait, traitArguments, targetType, context))
      continue;
    if (
      !registerImplementationPair(
        implementation,
        trait,
        traitArguments,
        targetType,
        family,
        implementationTargets,
        diagnostics,
      )
    )
      continue;
    const delegation = implementation.delegate
      ? prepareDelegation(implementation, trait, context)
      : undefined;
    if (implementation.delegate && !delegation) continue;
    if (delegation) delegations.push({ ...delegation, trait, traitArguments, implementation });
    const associatedTypes = prepareAssociatedTypes(
      delegation
        ? { ...implementation, associatedTypes: delegation.associatedTypes }
        : implementation,
      trait,
      context,
    );
    const memberSubstitutions = new Map(traitSubstitutions);
    trait.associatedTypes.forEach((associated, index) =>
      memberSubstitutions.set(`Self::${associated.name}`, associatedTypes[index]!),
    );
    const supplied = new Map<string, (typeof implementation.methods)[number]>();
    for (const method of implementation.methods) {
      if (supplied.has(method.name))
        diagnostics.push({
          code: "duplicate-impl-member",
          message: `implementation member '${method.name}' is declared more than once`,
          span: method.span,
        });
      supplied.set(method.name, method);
    }
    // 09 Trait Delegation: every instance method the body does not write
    // forwards to the delegated part.
    for (const method of delegation?.methods ?? [])
      if (!supplied.has(method.name)) supplied.set(method.name, method);
    const sealedMembers =
      usesStandardInspect(context.imports) && extendsInspectable(context.traitTypes, trait.name)
        ? INSPECTABLE_MEMBERS
        : new Set<string>();
    for (const method of implementation.methods) {
      if (sealedMembers.has(method.name)) {
        diagnostics.push({
          code: "sealed-trait-implementation",
          message: `'${method.name}' belongs to the sealed Inspectable, whose implementation the compiler supplies`,
          span: method.span,
        });
        continue;
      }
      if (!trait.methods.some((required) => required.name === method.name)) {
        diagnostics.push({
          code: "extra-trait-method",
          message: `method '${method.name}' is not declared by trait ${trait.name}`,
          span: method.span,
        });
      }
    }
    const methods: ImplementationMethodPreparation[] = [];
    for (const required of trait.methods) {
      const suppliedMethod = supplied.get(required.name);
      const defaultMethod = program.traits[trait.index]?.methods[required.index];
      // E5: only a written method or a trait default fills a trait method; a
      // method promoted from an embedded field never does.
      const method =
        suppliedMethod ??
        (defaultMethod?.body
          ? instantiateDefault(defaultMethod, implementation, trait)
          : undefined);
      if (!method) {
        diagnostics.push({
          code: "missing-trait-method",
          message: `${typeSourceText(implementation.targetName)} does not implement ${trait.name}.${required.name}`,
          span: implementation.span,
        });
        continue;
      }
      if (suppliedMethod) {
        const methodKinds = parameterKinds(implementation, method);
        const methodGenerics = methodKinds.types;
        const requirements = methodRequirements(method, methodKinds, traitTypes);
        // 09 Implementation Declarations: method-level generic parameters
        // correspond by position, so the trait's names are replaced by the
        // implementation's before parameter, result, and bound comparison.
        const renaming = new Map(memberSubstitutions);
        required.genericParameters.forEach((name, index) => {
          const renamed = method.genericParameters[index];
          if (renamed !== undefined) renaming.set(name, `generic:${renamed}`);
        });
        const traitGenerics = new Set([...trait.genericParameters, ...required.genericParameters]);
        const requiredBounds = defaultMethod?.genericBounds ?? [];
        // 14 Walkers, Describers, And Sources: an implementation may
        // strengthen the bound on `member[F]` (annot.walker.strengthen-member).
        const strengthenable =
          program.traits[trait.index]?.strengthenableMembers?.includes(required.name) === true;
        const boundsMatch = required.genericParameters.every((name, index) => {
          const renamed = method.genericParameters[index];
          if (renamed === undefined) return false;
          if (strengthenable) return true;
          const expected = methodBoundKeys(requiredBounds, name, traitGenerics, renaming);
          const actual = methodBoundKeys(method.genericBounds, renamed, methodGenerics, new Map());
          return (
            expected.length === actual.length &&
            expected.every((key, position) => key === actual[position])
          );
        });
        const parameterTypes = method.parameters.map((parameter) => {
          if (parameter.name === "self") {
            return parameter.type.name === "mut:Self" ? mutableType(targetType) : targetType;
          }
          if (parameter.type.name === "Self") return targetType;
          const type =
            typeName(
              parameter.type,
              dataTypes,
              enumTypes,
              traitTypes,
              diagnostics,
              methodGenerics,
              methodKinds.rows,
              hashableParameters(implementation, method),
            ) ?? "void";
          return type;
        });
        const expectedParameters = [
          ...(required.associated
            ? []
            : [required.receiverMutable ? mutableType(targetType) : targetType]),
          ...required.parameters
            .map((parameter) => substituteGenericType(parameter, renaming))
            // `Self` anywhere in a parameter type, as in `mut Walker[Self]`.
            .map((parameter) => substituteGenericType(parameter, new Map([["Self", targetType]]))),
        ];
        const renamedResult = substituteGenericType(required.result, renaming);
        // A result such as `mut Self` names the target (09 Implementation Declarations).
        const result =
          typeName(
            substituteSelfType(method.result, implementation.targetName),
            dataTypes,
            enumTypes,
            traitTypes,
            diagnostics,
            methodGenerics,
            methodKinds.rows,
            hashableParameters(implementation, method),
          ) ?? "void";
        if (
          (method.parameters[0]?.name !== "self") !== required.associated ||
          method.genericParameters.length !== required.genericParameters.length ||
          !boundsMatch ||
          !sameMethodDefaults(defaultMethod, method, renaming, traitGenerics, methodGenerics) ||
          parameterTypes.length !== expectedParameters.length ||
          parameterTypes.some((parameter, index) => parameter !== expectedParameters[index]) ||
          listVararg(method.parameters.at(-1)) !== required.variadic ||
          result !== substituteGenericType(renamedResult, new Map([["Self", targetType]])) ||
          method.suspending !== required.suspending ||
          !sameRequirements(requirements, required.requirements)
        ) {
          diagnostics.push({
            code: "trait-method-signature",
            message: `method '${method.name}' does not match ${trait.name}.${required.name}`,
            span: method.span,
          });
        }
      }
      const declaration: FunctionDecl = {
        kind: "function",
        name: `$impl${implementationIndex}.${method.name}`,
        suspending: method.suspending,
        genericParameters: [...implementation.genericParameters, ...method.genericParameters],
        rowParameters: [...(implementation.rowParameters ?? []), ...(method.rowParameters ?? [])],
        genericBounds: [...implementation.genericBounds, ...method.genericBounds],
        parameters: method.parameters.map((parameter) =>
          substituteSelfParameter(parameter, implementation.targetName),
        ),
        result: substituteSelfType(method.result, implementation.targetName),
        requirements: method.requirements,
        ...(implementation.standard ? { standard: true } : {}),
        ...(() => {
          const localImplementations = new Set([
            ...(trait.localImplementations ?? []),
            ...(suppliedMethod
              ? (implementation.localImplementations ?? [])
              : implementation.localImplementation === undefined
                ? []
                : [implementation.localImplementation]),
          ]);
          return localImplementations.size > 0
            ? { localImplementations: [...localImplementations] }
            : {};
        })(),
        ...selfDefaults(method, implementation.targetName),
        body: method.body ?? [],
        ...(suppliedMethod && !method.body ? { bodiless: true } : {}),
        span: method.span,
      };
      if (!suppliedMethod) traitDefaultDeclarations.set(declaration, trait.index);
      methods.push({ methodIndex: required.index, declaration });
    }
    implementationPreparations.push({
      declaration: implementation,
      trait,
      traitArguments,
      targetType,
      associatedTypes,
      methods,
      ...(family ? { family } : {}),
    });
  }
  validateSupertraitImplementations(context);
  validateDelegations(delegations, context);
}

/**
 * The bounds written for one method-level generic parameter, as comparable
 * keys in written order: each bound trait with its resolved arguments (and
 * `mut`), then each associated type binding. `substitutions` instantiates the
 * trait's parameters and renames its method parameters to the
 * implementation's.
 */
function methodBoundKeys(
  bounds: readonly GenericBound[],
  parameter: string,
  generics: ReadonlySet<string>,
  substitutions: ReadonlyMap<string, ValueType>,
): string[] {
  const resolve = (type: string): string =>
    substituteGenericType(resolveGenericType(type, generics), substitutions);
  const traitKey = (written: string): string => {
    const inner = mutableInner(written);
    const application = nominalGenericParts(inner ?? written);
    const key = application
      ? nominalGenericType(application.name, application.arguments.map(resolve))
      : (inner ?? written);
    return inner === undefined ? key : `mut ${key}`;
  };
  return bounds
    .filter((bound) => bound.parameter === parameter)
    .flatMap((bound) => [
      ...bound.traits.map(traitKey),
      ...(bound.bindings ?? []).map(
        (binding) => `${traitKey(binding.trait)}::${binding.name}=${resolve(binding.type.name)}`,
      ),
    ]);
}

interface Delegation {
  readonly implementation: ImplDecl;
  readonly trait: HirTrait;
  readonly traitArguments: readonly ValueType[];
  readonly partType: ValueType;
  readonly associatedTypes: ImplDecl["associatedTypes"];
  readonly methods: readonly MethodDecl[];
}

/**
 * 09 Trait Delegation: `impl Trait for C by E` requires `E` to be an embedded
 * field of the data type `C`. Each instance method of the trait is generated
 * as `Trait::m(self.E, arguments...)`; associated functions are not
 * forwarded. Associated types take the part's bindings, so the body may not
 * bind them. The prototype supports delegation of non-generic traits only.
 */
function prepareDelegation(
  implementation: ImplDecl,
  trait: HirTrait,
  context: ProgramCheckContext,
): Omit<Delegation, "trait" | "traitArguments" | "implementation"> | undefined {
  const { program, dataTypes, diagnostics } = context;
  const delegate = implementation.delegate!;
  const fail = (message: string, span = delegate.span): undefined => {
    diagnostics.push({ code: "invalid-delegation", message, span });
    return undefined;
  };
  const target = dataTypes.get(
    nominalGenericParts(implementation.targetName)?.name ?? implementation.targetName,
  );
  const field = target?.fields.find((candidate) => candidate.name === delegate.name);
  if (!field?.embedded)
    return fail(
      `'${delegate.name}' is not an embedded field of '${typeSourceText(implementation.targetName)}'`,
    );
  if (implementation.associatedTypes.length > 0)
    return fail(
      `a delegating implementation takes its associated types from '${delegate.name}'`,
      implementation.associatedTypes[0]!.span,
    );
  const traitName =
    nominalGenericParts(implementation.traitName!)?.name ?? implementation.traitName!;
  const partImplementation = program.implementations.find(
    (candidate) =>
      implementationVisibleFrom(candidate, implementation) &&
      candidate.traitName !== undefined &&
      (nominalGenericParts(candidate.traitName)?.name ?? candidate.traitName) === traitName &&
      candidate.targetName === field.type,
  );
  const declaration = program.traits[trait.index];
  const span = delegate.span;
  const methods = (declaration?.methods ?? [])
    .filter((method) => method.parameters[0]?.name === "self")
    .map((method): MethodDecl => {
      const parameters = method.parameters.slice(1);
      const call: Expression = {
        kind: method.suspending ? "suspend-call" : "call",
        callee: { kind: "qualified-name", owner: traitName, name: method.name, span },
        arguments: [
          {
            kind: "member",
            receiver: { kind: "name", name: "self", span },
            name: delegate.name,
            span,
          },
          ...parameters.map((parameter): Expression => ({
            kind: "name",
            name: parameter.name,
            span,
          })),
        ],
        argumentSpreads: [false, ...parameters.map((parameter) => parameter.variadic === true)],
        span,
      };
      // Associated types take the part's bindings.
      const bound = (type: TypeRef): TypeRef =>
        partImplementation?.associatedTypes.find(
          (associated) => type.name === `Self::${associated.name}`,
        )?.value ?? type;
      return {
        ...method,
        parameters: method.parameters.map((parameter) => ({
          ...parameter,
          type: bound(parameter.type),
        })),
        result: bound(method.result),
        body: [{ kind: "expression", expression: call, span }],
        span,
      };
    });
  return {
    partType: field.type,
    associatedTypes: partImplementation?.associatedTypes ?? [],
    methods,
  };
}

function validateDelegations(
  delegations: readonly Delegation[],
  context: ProgramCheckContext,
): void {
  for (const delegation of delegations) {
    const implemented = context.implementationPreparations.some((candidate) =>
      Boolean(
        matchTraitImplementation(
          candidate,
          delegation.trait.index,
          delegation.partType,
          delegation.traitArguments,
        ),
      ),
    );
    if (!implemented)
      context.diagnostics.push({
        code: "invalid-delegation",
        message: `'${delegation.implementation.delegate!.name}' has type '${typeSourceText(delegation.partType)}', which does not implement ${delegation.implementation.traitName}`,
        span: delegation.implementation.delegate!.span,
      });
  }
}

/**
 * The standard library's implementations that have no source `impl`, as far
 * as a supertrait check of a primitive number type needs them: `Eq`,
 * `PartialOrd`, and `Display` for every number type, and `Ord` for integers
 * (05-expressions.md#equality, #ordering).
 */
function builtInSupertraitHolds(traitName: string, targetType: ValueType): boolean {
  const numeric = numericType(targetType);
  if (!numeric) return false;
  if (traitName === "Ord") return numeric.family !== "float";
  return traitName === "Eq" || traitName === "PartialOrd" || traitName === "Display";
}

/**
 * Whether `candidate`, matched with `matched`, binds each associated type
 * the supertrait binds, after `Self` is the target.
 */
function supertraitBindingsHold(
  supertrait: HirSupertrait,
  candidate: ImplementationPreparation,
  matched: ReadonlyMap<string, ValueType>,
  traitTypes: ProgramCheckContext["traitTypes"],
  substitutions: ReadonlyMap<string, ValueType>,
): boolean {
  const trait = [...traitTypes.values()].find((item) => item.index === supertrait.traitIndex);
  return (supertrait.associatedBindings ?? []).every((binding) => {
    const index = trait?.associatedTypes.findIndex(
      (associated) => associated.name === binding.name,
    );
    return (
      index === undefined ||
      index < 0 ||
      substituteGenericType(candidate.associatedTypes[index]!, matched) ===
        substituteGenericType(binding.type, substitutions)
    );
  });
}

function validateSupertraitImplementations(context: ProgramCheckContext): void {
  for (const declared of context.implementationPreparations) {
    // Each implementation a numeric family stands for needs its supertraits.
    const instances = declared.family ? familyInstances(declared.family) : [new Map()];
    for (const instance of instances) {
      const implementation = {
        ...declared,
        targetType: substituteGenericType(declared.targetType, instance),
        traitArguments: declared.traitArguments.map((argument) =>
          substituteGenericType(argument, instance),
        ),
      };
      validateSupertraits(implementation, context);
    }
  }
}

function validateSupertraits(
  implementation: ImplementationPreparation,
  context: ProgramCheckContext,
): void {
  const traitSubstitutions = new Map(
    implementation.trait.genericParameters.map(
      (parameter, index) => [parameter, implementation.traitArguments[index]!] as const,
    ),
  );
  traitSubstitutions.set("Self", implementation.targetType);
  for (const category of implementation.trait.categorySupertraits ?? []) {
    const parameters = implementationCategoryParameters(
      implementation.declaration,
      context.traitTypes,
      category,
    );
    if (
      !typeSatisfiesValueCategory(implementation.targetType, category, {
        dataTypes: context.dataTypes,
        enumTypes: context.enumTypes,
        ...(category === "AnyRef"
          ? { referenceParameters: parameters }
          : { valueParameters: parameters }),
      })
    )
      context.diagnostics.push({
        code: "missing-supertrait-implementation",
        message: `${typeSourceText(implementation.declaration.targetName)} must implement ${category} before ${implementation.trait.name}`,
        span: implementation.declaration.span,
      });
  }
  for (const supertrait of implementation.trait.supertraits) {
    const expectedArguments = supertrait.traitArguments.map((argument) =>
      substituteGenericType(argument, traitSubstitutions),
    );
    if (
      supertrait.traitName === INSPECTABLE &&
      usesStandardInspect(context.imports) &&
      inspectKey(implementation.targetType, {
        nominal: (name) =>
          [context.dataTypes.get(name), context.enumTypes.get(name)].some(
            (declaration) => declaration !== undefined && !declaration.local,
          ),
        inspectableParameter: () => true,
      })
    )
      continue;
    if (
      expectedArguments.length === 0 &&
      builtInSupertraitHolds(supertrait.traitName, implementation.targetType)
    )
      continue;
    const found = context.implementationPreparations.some((candidate) => {
      if (!implementationVisibleFrom(candidate.declaration, implementation.declaration))
        return false;
      const matched = matchTraitImplementation(
        candidate,
        supertrait.traitIndex,
        implementation.targetType,
        expectedArguments,
      );
      return (
        matched !== undefined &&
        supertraitBindingsHold(
          supertrait,
          candidate,
          matched,
          context.traitTypes,
          traitSubstitutions,
        )
      );
    });
    if (!found)
      context.diagnostics.push({
        code: "missing-supertrait-implementation",
        message: `${typeSourceText(implementation.declaration.targetName)} must implement ${supertrait.traitName} before ${implementation.trait.name}`,
        span: implementation.declaration.span,
      });
  }
}

/** Generic implementation parameters whose written bounds prove one category. */
function implementationCategoryParameters(
  implementation: ImplDecl,
  traitTypes: ReadonlyMap<string, HirTrait>,
  category: ValueCategory,
): Set<string> {
  return new Set(
    implementation.genericParameters.filter((parameter) =>
      implementation.genericBounds.some(
        (bound) =>
          bound.parameter === parameter &&
          bound.traits.some((written) => {
            const key = mutableInner(written) ?? written;
            const name = nominalGenericParts(key)?.name ?? key;
            if (name === category) return true;
            const trait = traitTypes.get(name);
            return trait !== undefined && traitImpliesValueCategory(trait, traitTypes, category);
          }),
      ),
    ),
  );
}
