import type { Diagnostic } from "../diagnostics.ts";
import type { Expression, ImplDecl, Parameter, TypeRef } from "../ast.ts";
import type { HirTrait, ValueType } from "../hir.ts";
import { mutableType, nominalGenericParts, nominalGenericType, readonlyType } from "../types.ts";
import {
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
  readonly traitIndex: number;
  readonly targetType: string;
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
  targetType: string,
  pairs: Set<string>,
  normalizedPairs: Set<string>,
  targets: RegisteredImplementationTarget[],
  diagnostics: Diagnostic[],
): boolean {
  const pair = `${trait.index}:${targetType}`;
  const normalizedPair = `${trait.index}:${readonlyType(targetType)}`;
  if (pairs.has(pair)) {
    diagnostics.push({
      code: "duplicate-trait-impl",
      message: `${implementation.targetName} implements ${trait.name} more than once`,
      span: implementation.span,
    });
    return false;
  }
  if (normalizedPairs.has(normalizedPair)) {
    diagnostics.push({
      code: "overlapping-impl",
      message: `${implementation.targetName} overlaps another ${trait.name} implementation after permission normalization`,
      span: implementation.span,
    });
    return false;
  }
  const normalizedTarget = readonlyType(targetType);
  if (
    targets.some(
      (candidate) =>
        candidate.traitIndex === trait.index &&
        requirementKeysMayCollide(readonlyType(candidate.targetType), normalizedTarget),
    )
  ) {
    diagnostics.push({
      code: "overlapping-impl",
      message: `${implementation.targetName} can overlap another ${trait.name} implementation after generic substitution`,
      span: implementation.span,
    });
    return false;
  }
  pairs.add(pair);
  normalizedPairs.add(normalizedPair);
  targets.push({ traitIndex: trait.index, targetType });
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
  const {
    program,
    diagnostics,
    dataTypes,
    enumTypes,
    traitTypes,
    implementationPreparations,
    inherentMethods,
  } = context;
  const inherentMethodKeys = new Set<string>();
  const implementationPairs = new Set<string>();
  const normalizedImplementationPairs = new Set<string>();
  const implementationTargets: RegisteredImplementationTarget[] = [];
  const orderedImplementationEntries = [...program.implementations.entries()].sort(
    (left, right) =>
      Number(left[1].traitName !== undefined) - Number(right[1].traitName !== undefined),
  );
  for (const [implementationIndex, implementation] of orderedImplementationEntries) {
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
    const specialization = specializeTrait(implementation, trait, context);
    if (!specialization) continue;
    const traitArguments = specialization.arguments;
    const traitSubstitutions = specialization.substitutions;
    const implementationGenerics = new Set(implementation.genericParameters);
    const targetType =
      typeName(
        { name: implementation.targetName, span: implementation.span },
        dataTypes,
        enumTypes,
        traitTypes,
        diagnostics,
        implementationGenerics,
      ) ?? "void";
    if (!isKnownType(targetType, dataTypes, enumTypes, traitTypes)) {
      diagnostics.push({
        code: "unknown-type",
        message: `unknown implementation target '${implementation.targetName}'`,
        span: implementation.span,
      });
      continue;
    }
    const targetHead =
      nominalGenericParts(readonlyType(targetType))?.name ?? readonlyType(targetType);
    const traitIsLocal = program.traits.some((declaration) => declaration.name === trait.name);
    const targetIsLocal = dataTypes.has(targetHead) || enumTypes.has(targetHead);
    if (!traitIsLocal && !targetIsLocal) {
      diagnostics.push({
        code: "orphan-impl",
        message: `implementation of nonlocal trait '${trait.name}' for nonlocal type '${targetHead}' is not allowed`,
        span: implementation.span,
      });
      continue;
    }
    if (
      !registerImplementationPair(
        implementation,
        trait,
        targetType,
        implementationPairs,
        normalizedImplementationPairs,
        implementationTargets,
        diagnostics,
      )
    )
      continue;
    const associatedTypes = prepareAssociatedTypes(implementation, trait, context);
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
      const requiredParameters = required.parameters.map((parameter) =>
        substituteGenericType(parameter, memberSubstitutions),
      );
      const requiredResult = substituteGenericType(required.result, memberSubstitutions);
      const suppliedMethod = supplied.get(required.name);
      const defaultMethod = program.traits[trait.index]?.methods[required.index];
      let method = suppliedMethod ?? (defaultMethod?.body ? defaultMethod : undefined);
      if (!method && !required.associated) {
        const target = dataTypes.get(implementation.targetName);
        const promoted =
          target?.fields.flatMap((field) => {
            if (!field.embedded) return [];
            const embeddedType = readonlyType(field.type);
            return inherentMethods
              .filter(
                (candidate) =>
                  !candidate.associated &&
                  candidate.targetType === embeddedType &&
                  candidate.name === required.name,
              )
              .map((candidate) => ({ field, candidate }));
          }) ?? [];
        if (required.receiverMutable && promoted.length > 0) {
          diagnostics.push({
            code: "promoted-mutable-requirement",
            message: `mutable requirement '${trait.name}.${required.name}' cannot be filled through an embedded readonly edge`,
            span: implementation.span,
          });
          continue;
        }
        const compatible = promoted.filter(
          ({ candidate }) =>
            !candidate.receiverMutable &&
            candidate.suspending === required.suspending &&
            candidate.parameters.length === requiredParameters.length &&
            candidate.parameters.every(
              (parameter, index) => parameter === requiredParameters[index],
            ) &&
            candidate.result === requiredResult &&
            sameRequirements(candidate.requirements, required.requirements),
        );
        if (compatible.length === 1 && defaultMethod) {
          const promotedMethod = compatible[0]!;
          const receiver: Expression = {
            kind: "member",
            receiver: { kind: "name", name: "self", span: defaultMethod.span },
            name: promotedMethod.field.name,
            span: defaultMethod.span,
          };
          const call: Expression = {
            kind: required.suspending ? "suspend-call" : "call",
            callee: { kind: "member", receiver, name: required.name, span: defaultMethod.span },
            arguments: defaultMethod.parameters.slice(1).map((parameter) => ({
              kind: "name" as const,
              name: parameter.name,
              span: parameter.span,
            })),
            span: defaultMethod.span,
          };
          method = {
            ...defaultMethod,
            body: [{ kind: "expression", expression: call, span: defaultMethod.span }],
          };
        }
      }
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
          ...requiredParameters.map((parameter) =>
            parameter === "generic:Self" ? targetType : parameter,
          ),
        ];
        const result =
          typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics, methodGenerics) ??
          "void";
        if (
          (method.parameters[0]?.name !== "self") !== required.associated ||
          method.genericParameters.length !== required.genericParameters.length ||
          parameterTypes.length !== expectedParameters.length ||
          parameterTypes.some((parameter, index) => parameter !== expectedParameters[index]) ||
          (method.parameters.at(-1)?.variadic === true) !== required.variadic ||
          result !== (requiredResult === "generic:Self" ? targetType : requiredResult) ||
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
      methods.push({
        methodIndex: required.index,
        declaration: {
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
        },
      });
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
