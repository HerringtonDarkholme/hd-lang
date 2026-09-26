import type { Diagnostic } from "../diagnostics.ts";
import type {
  Expression,
  FunctionDecl,
  GenericBound,
  ImplDecl,
  MethodDecl,
  Parameter,
  TypeRef,
} from "../ast.ts";
import { traitDefaultDeclarations } from "./member-lookup.ts";
import type { HirTrait, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  tupleType,
} from "../types.ts";
import {
  genericTypeName,
  isKnownType,
  matchTraitImplementation,
  requirementKeysMayCollide,
  resolveGenericType,
  sameRequirements,
  substituteGenericType,
  typeName,
} from "./shared.ts";

import type { ImplementationMethodPreparation, ProgramCheckContext } from "./program-context.ts";

interface TraitSpecialization {
  readonly arguments: readonly string[];
  readonly substitutions: ReadonlyMap<string, string>;
}

interface RegisteredImplementationTarget {
  readonly genericParameters: readonly string[];
  readonly traitArguments: readonly string[];
  readonly traitIndex: number;
  readonly targetType: string;
}

// 09 Overlap (TQ-28): two implementations of one trait overlap when one
// substitution unifies their trait arguments and full targets together.
function implementationHeadsMayUnify(
  left: readonly string[],
  right: readonly string[],
  rightGenerics: readonly string[],
): boolean {
  // Rename the other implementation's parameters apart before unifying.
  const renamed = new Map(
    rightGenerics.map((parameter) => [parameter, `generic:$other.${parameter}`] as const),
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
  const arguments_ =
    application?.arguments.map(
      (argument) =>
        typeName(
          { name: argument, span: implementation.span },
          context.dataTypes,
          context.enumTypes,
          context.traitTypes,
          context.diagnostics,
          new Set(implementation.genericParameters),
        ) ?? "void",
    ) ?? [];
  return {
    arguments: arguments_,
    substitutions: new Map(
      trait.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
    ),
  };
}

function registerImplementationPair(
  implementation: ImplDecl,
  trait: HirTrait,
  traitArguments: readonly string[],
  targetType: string,
  targets: RegisteredImplementationTarget[],
  diagnostics: Diagnostic[],
): boolean {
  const conflict = targets.find(
    (candidate) =>
      candidate.traitIndex === trait.index &&
      implementationHeadsMayUnify(
        [...traitArguments, readonlyType(targetType)],
        [...candidate.traitArguments, readonlyType(candidate.targetType)],
        candidate.genericParameters,
      ),
  );
  if (conflict) {
    diagnostics.push({
      code: "overlapping-impl",
      message: `${implementation.targetName} overlaps the ${trait.name} implementation for '${readonlyType(conflict.targetType).replaceAll("generic:", "")}': their heads unify`,
      span: implementation.span,
    });
    return false;
  }
  targets.push({
    genericParameters: implementation.genericParameters,
    traitArguments,
    traitIndex: trait.index,
    targetType,
  });
  return true;
}

// 09 Implementation Targets: no outer `mut`, and no bare type parameter.
function checkImplementationTarget(implementation: ImplDecl, diagnostics: Diagnostic[]): boolean {
  if (mutableInner(implementation.targetName) !== undefined) {
    diagnostics.push({
      code: "mutable-impl-target",
      message: `implementation target '${readonlyType(implementation.targetName)}' cannot be written with mut; permission belongs to receivers and bounds`,
      span: implementation.span,
    });
    return false;
  }
  if (functionParts(implementation.targetName)) {
    diagnostics.push({
      code: "function-impl-target",
      message: `implementation target '${implementation.targetName}' is a function type; function types are never implementation targets`,
      span: implementation.span,
    });
    return false;
  }
  if (implementation.genericParameters.includes(implementation.targetName)) {
    diagnostics.push({
      code: "bare-parameter-impl-target",
      message: `implementation target '${implementation.targetName}' is a bare type parameter; a target must start with a type constructor`,
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

function substituteSelfParameter(parameter: Parameter, targetName: string): Parameter {
  return {
    ...parameter,
    type: substituteSelfType(parameter.type, targetName),
  };
}

function prepareInherentImplementation(
  implementation: ImplDecl,
  implementationIndex: number,
  methodKeys: Set<string>,
  context: ProgramCheckContext,
): void {
  const { diagnostics, dataTypes, enumTypes, traitTypes, inherentMethods, inherentDeclarations } =
    context;
  const target =
    dataTypes.get(implementation.targetName) ?? enumTypes.get(implementation.targetName);
  if (!target) {
    diagnostics.push({
      code: "unknown-type",
      message: `unknown inherent implementation target '${implementation.targetName}'`,
      span: implementation.span,
    });
    return;
  }
  if (target.genericParameters.length > 0) {
    diagnostics.push({
      code: "unsupported-generic-impl",
      message: `inherent implementation of generic type '${implementation.targetName}' requires explicit generic parameters`,
      span: implementation.span,
    });
    return;
  }
  for (const method of implementation.methods) {
    const key = `${implementation.targetName}.${method.name}`;
    if (methodKeys.has(key)) {
      diagnostics.push({
        code: "duplicate-inherent-member",
        message: `inherent method '${key}' is declared more than once`,
        span: method.span,
      });
      continue;
    }
    methodKeys.add(key);
    const associated = method.parameters[0]?.name !== "self";
    const sourceParameters = associated ? method.parameters : method.parameters.slice(1);
    const genericParameters = new Set(method.genericParameters);
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
          genericParameters,
        ) ?? "void";
      return parameter.variadic ? nominalGenericType("list", [resolved]) : resolved;
    });
    const result =
      typeName(
        substituteSelfType(method.result, implementation.targetName),
        dataTypes,
        enumTypes,
        traitTypes,
        diagnostics,
        genericParameters,
      ) ?? "void";
    const functionName = `$inherent${implementationIndex}.${method.name}`;
    inherentMethods.push({
      targetType: implementation.targetName,
      name: method.name,
      public: method.public === true,
      associated,
      receiverMutable: !associated && method.parameters[0]!.type.name === "mut:Self",
      parameters,
      parameterNames: sourceParameters.map((parameter) => parameter.name),
      variadic: sourceParameters.at(-1)?.variadic === true,
      suspending: method.suspending,
      result,
      requirements: method.requirements,
      functionName,
      span: method.span,
    });
    inherentDeclarations.push({
      kind: "function",
      name: functionName,
      suspending: method.suspending,
      genericParameters: method.genericParameters,
      genericBounds: method.genericBounds,
      parameters: method.parameters.map((parameter) =>
        substituteSelfParameter(parameter, implementation.targetName),
      ),
      result: substituteSelfType(method.result, implementation.targetName),
      requirements: method.requirements,
      ...(method.resultOmitted ? { resultOmitted: true } : {}),
      ...(method.requirementsOmitted ? { requirementsOmitted: true } : {}),
      body: method.body ?? [],
      span: method.span,
    });
  }
}

function resolveImplementationTarget(
  implementation: ImplDecl,
  context: ProgramCheckContext,
): string | undefined {
  const { diagnostics, dataTypes, enumTypes, traitTypes } = context;
  const targetType =
    typeName(
      { name: implementation.targetName, span: implementation.span },
      dataTypes,
      enumTypes,
      traitTypes,
      diagnostics,
      new Set(implementation.genericParameters),
    ) ?? "void";
  if (isKnownType(targetType, dataTypes, enumTypes, traitTypes)) return targetType;
  diagnostics.push({
    code: "unknown-type",
    message: `unknown implementation target '${implementation.targetName}'`,
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
  const isLocalConstructor = (type: string): boolean =>
    dataTypes.has(constructorOf(type)) || enumTypes.has(constructorOf(type));
  const traitIsLocal = program.traits.some((declaration) => declaration.name === trait.name);
  const argumentIsLocal =
    genericTypeName(readonlyType(targetType)) === undefined &&
    traitArguments.some(isLocalConstructor);
  if (traitIsLocal || isLocalConstructor(targetType) || argumentIsLocal) return true;
  context.diagnostics.push({
    code: "orphan-impl",
    message: `implementation of nonlocal trait '${trait.name}' for nonlocal type '${constructorOf(targetType)}' is not allowed`,
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
  const genericParameters = new Set(implementation.genericParameters);
  return trait.associatedTypes.map((associated) => {
    const binding = bindings.get(associated.name);
    if (!binding?.value) {
      diagnostics.push({
        code: "missing-associated-type",
        message: `${implementation.targetName} does not bind ${trait.name}.${associated.name}`,
        span: implementation.span,
      });
      return "void";
    }
    return (
      typeName(binding.value, dataTypes, enumTypes, traitTypes, diagnostics, genericParameters) ??
      "void"
    );
  });
}

export function prepareImplementations(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes, implementationPreparations } =
    context;
  const inherentMethodKeys = new Set<string>();
  const implementationTargets: RegisteredImplementationTarget[] = [];
  const delegations: Delegation[] = [];
  const orderedImplementationEntries = [...program.implementations.entries()].sort(
    (left, right) =>
      Number(left[1].traitName !== undefined) - Number(right[1].traitName !== undefined),
  );
  for (const [implementationIndex, implementation] of orderedImplementationEntries) {
    if (!checkImplementationTarget(implementation, diagnostics)) continue;
    if (implementation.traitName === undefined) {
      prepareInherentImplementation(
        implementation,
        implementationIndex,
        inherentMethodKeys,
        context,
      );
      continue;
    }
    const traitApplication = nominalGenericParts(implementation.traitName);
    const traitName = traitApplication?.name ?? implementation.traitName;
    if (traitName === "Suspend") {
      diagnostics.push({
        code: "sealed-trait-implementation",
        message: "Suspend[T] is a sealed runtime protocol and cannot be implemented by user code",
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
    // 09 Implementation Targets: a trait value type is never a target.
    const targetBase = nominalGenericParts(implementation.targetName)?.name;
    if (traitTypes.has(targetBase ?? implementation.targetName)) {
      diagnostics.push({
        code: "trait-value-impl-target",
        message: `implementation target '${implementation.targetName}' is a trait value type; implement the trait for concrete types instead`,
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
    for (const method of implementation.methods) {
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
      const method = suppliedMethod ?? (defaultMethod?.body ? defaultMethod : undefined);
      if (!method) {
        diagnostics.push({
          code: "missing-trait-method",
          message: `${implementation.targetName} does not implement ${trait.name}.${required.name}`,
          span: implementation.span,
        });
        continue;
      }
      if (suppliedMethod) {
        const methodGenerics = new Set([
          ...implementation.genericParameters,
          ...method.genericParameters,
        ]);
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
        const boundsMatch = required.genericParameters.every((name, index) => {
          const renamed = method.genericParameters[index];
          if (renamed === undefined) return false;
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
            ) ?? "void";
          return parameter.variadic ? nominalGenericType("list", [type]) : type;
        });
        const expectedParameters = [
          ...(required.associated
            ? []
            : [required.receiverMutable ? mutableType(targetType) : targetType]),
          ...required.parameters
            .map((parameter) => substituteGenericType(parameter, renaming))
            .map((parameter) => (parameter === "generic:Self" ? targetType : parameter)),
        ];
        const renamedResult = substituteGenericType(required.result, renaming);
        const result =
          typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics, methodGenerics) ??
          "void";
        if (
          (method.parameters[0]?.name !== "self") !== required.associated ||
          method.genericParameters.length !== required.genericParameters.length ||
          !boundsMatch ||
          parameterTypes.length !== expectedParameters.length ||
          parameterTypes.some((parameter, index) => parameter !== expectedParameters[index]) ||
          (method.parameters.at(-1)?.variadic === true) !== required.variadic ||
          result !== (renamedResult === "generic:Self" ? targetType : renamedResult) ||
          method.suspending !== required.suspending ||
          !sameRequirements(method.requirements, required.requirements)
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
        genericBounds: [...implementation.genericBounds, ...method.genericBounds],
        parameters: method.parameters.map((parameter) =>
          substituteSelfParameter(parameter, implementation.targetName),
        ),
        result: method.result,
        requirements: method.requirements,
        body: method.body ?? [],
        span: method.span,
      };
      if (!suppliedMethod) traitDefaultDeclarations.add(declaration);
      methods.push({ methodIndex: required.index, declaration });
    }
    implementationPreparations.push({
      declaration: implementation,
      trait,
      traitArguments,
      targetType,
      associatedTypes,
      methods,
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
    return fail(`'${delegate.name}' is not an embedded field of '${implementation.targetName}'`);
  if (implementation.associatedTypes.length > 0)
    return fail(
      `a delegating implementation takes its associated types from '${delegate.name}'`,
      implementation.associatedTypes[0]!.span,
    );
  const traitName =
    nominalGenericParts(implementation.traitName!)?.name ?? implementation.traitName!;
  const partImplementation = program.implementations.find(
    (candidate) =>
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
        message: `'${delegation.implementation.delegate!.name}' has type '${delegation.partType}', which does not implement ${delegation.implementation.traitName}`,
        span: delegation.implementation.delegate!.span,
      });
  }
}

function validateSupertraitImplementations(context: ProgramCheckContext): void {
  for (const implementation of context.implementationPreparations) {
    const traitSubstitutions = new Map(
      implementation.trait.genericParameters.map(
        (parameter, index) => [parameter, implementation.traitArguments[index]!] as const,
      ),
    );
    for (const supertrait of implementation.trait.supertraits) {
      const expectedArguments = supertrait.traitArguments.map((argument) =>
        substituteGenericType(argument, traitSubstitutions),
      );
      const found = context.implementationPreparations.some((candidate) => {
        return Boolean(
          matchTraitImplementation(
            candidate,
            supertrait.traitIndex,
            implementation.targetType,
            expectedArguments,
          ),
        );
      });
      if (!found)
        context.diagnostics.push({
          code: "missing-supertrait-implementation",
          message: `${implementation.declaration.targetName} must implement ${supertrait.traitName} before ${implementation.trait.name}`,
          span: implementation.declaration.span,
        });
    }
  }
}
