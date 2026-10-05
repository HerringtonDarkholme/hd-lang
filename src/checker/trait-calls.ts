import type { Expression } from "../ast.ts";
import type {
  HirExpression,
  HirTrait,
  HirTraitMethod,
  HirTraitMethodFunction,
  ValueType,
} from "../hir.ts";
import { nominalGenericType, displayType } from "../types.ts";
import {
  containsGenericType,
  matchImplementationTarget,
  orderedTypeSubstitutions,
  substituteGenericType,
} from "./shared.ts";

import type { Signature } from "./context.ts";
import { DebugPrintChecker } from "./debug-print-calls.ts";

export interface QualifiedCallExpression extends Extract<Expression, { kind: "call" }> {
  readonly callee: Extract<Expression, { kind: "qualified-name" }>;
}

interface ResolvedTraitMethod {
  readonly method: HirTraitMethod;
  readonly path: readonly number[];
  readonly trait: HirTrait;
}

/** Trait method lookup through supertraits, and associated calls through a bound. */
export abstract class TraitCallChecker extends DebugPrintChecker {
  /**
   * The implementations whose trait supplies the associated function `name`
   * for `ownerType`, with the implementation's parameters that the target
   * solves, as `T = Point` for `Box::[Point]::name()`, which a receiverless
   * call's arguments may not mention.
   */
  protected associatedCandidates(
    ownerType: ValueType,
    name: string,
  ): {
    readonly candidateTrait: HirTrait;
    readonly method: HirTraitMethod;
    readonly mapping: HirTraitMethodFunction;
    readonly traitArguments: readonly ValueType[];
    readonly substitutions: ReadonlyMap<string, ValueType>;
  }[] {
    return this.implementations.flatMap((implementation) => {
      const substitutions = new Map<string, ValueType>();
      if (!matchImplementationTarget(implementation, ownerType, substitutions)) return [];
      const candidateTrait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      const method = candidateTrait?.methods.find(
        (candidate) => candidate.associated && candidate.name === name,
      );
      const mapping =
        method &&
        implementation.methodFunctions.find((candidate) => candidate.methodIndex === method.index);
      return candidateTrait && method && mapping
        ? [
            {
              candidateTrait,
              method,
              mapping,
              substitutions,
              traitArguments: implementation.traitArguments,
            },
          ]
        : [];
    });
  }

  protected findTraitMethods(
    trait: HirTrait,
    name: string,
    path: readonly number[] = [],
    seen: ReadonlySet<number> = new Set(),
    associated = false,
  ): ResolvedTraitMethod[] {
    if (seen.has(trait.index)) return [];
    const nextSeen = new Set([...seen, trait.index]);
    const direct = trait.methods
      .filter((method) => method.associated === associated && method.name === name)
      .map((method) => ({ method, path, trait }));
    const inherited = trait.supertraits.flatMap((supertrait, fieldIndex) => {
      const parent = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === supertrait.traitIndex,
      );
      return parent
        ? this.findTraitMethods(parent, name, [...path, fieldIndex], nextSeen, associated)
        : [];
    });
    return [...direct, ...inherited];
  }

  // `T::f(...)` on a type parameter calls the associated function through
  // the bound that supplies it (09-traits.md#associated-function-calls).
  protected checkBoundAssociatedCall(
    expression: QualifiedCallExpression,
    owner: string,
  ): HirExpression {
    const name = expression.callee.name;
    const candidates = this.signature.genericBounds.flatMap((bound, boundIndex) => {
      if (bound.parameter !== owner) return [];
      const trait = this.traitTypes.get(bound.traitName)!;
      return this.findTraitMethods(trait, name, [], new Set(), true).map((selected) => ({
        bound,
        boundIndex,
        selected,
      }));
    });
    if (candidates.length > 1)
      this.fail(
        "ambiguous-method",
        `associated function '${name}' is supplied by multiple bounds on ${owner}`,
        expression.callee.span,
      );
    const candidate = candidates[0];
    if (!candidate)
      this.fail(
        "unknown-associated-function",
        `no bound on '${owner}' supplies an associated function '${name}'`,
        expression.callee.span,
      );
    const { bound, boundIndex, selected } = candidate;
    if (selected.method.suspending)
      this.fail(
        "unsupported-bound-associated-call",
        `the prototype calls only non-suspending associated functions through a bound`,
        expression.callee.span,
      );
    const boundTrait =
      bound.traitArguments.length > 0
        ? nominalGenericType(bound.traitName, bound.traitArguments)
        : bound.traitName;
    const traitArguments = this.resolveTraitPath(
      this.traitTypes.get(bound.traitName)!,
      bound.traitArguments,
      selected.path,
    ).arguments;
    const substitutions = new Map<string, ValueType>([
      ...selected.trait.genericParameters.map(
        (parameter, index) => [parameter, traitArguments[index]!] as const,
      ),
      ["Self", `generic:${owner}`],
    ]);
    const parameters = selected.method.parameters.map((parameter) =>
      substituteGenericType(parameter, substitutions),
    );
    let result = substituteGenericType(selected.method.result, substitutions);
    let checkedArguments: {
      readonly arguments: readonly HirExpression[];
      readonly parameterIndices?: readonly number[];
    };
    let bounds: HirExpression[] | undefined;
    const method = selected.method;
    if (method.genericParameters.length > 0) {
      // Method-level generics are inferred per call, and their bounds
      // travel as dictionary arguments, as for a method call on a receiver.
      const methodSignature: Signature = {
        name,
        index: -1,
        suspending: false,
        genericParameters: method.genericParameters,
        genericBounds: (method.genericBounds ?? []).map((item) => ({
          ...item,
          traitArguments: item.traitArguments.map((argument) =>
            substituteGenericType(argument, substitutions),
          ),
        })),
        referenceParameters: method.referenceParameters,
        valueParameters: method.valueParameters,
        rowParameters: [],
        parameters,
        parameterNames: method.parameterNames,
        defaultFunctionNames: method.parameters.map(() => undefined),
        variadic: method.variadic,
        result,
        requirements: method.requirements.map((requirement) =>
          substituteGenericType(requirement, substitutions),
        ),
        span: method.span,
      };
      const checkedSignature = this.checkSignatureArguments(
        expression,
        methodSignature,
        undefined,
        `associated function '${name}'`,
      );
      const resolved = this.resolveAssociatedTypeSubstitutions(
        methodSignature,
        checkedSignature.substitutions,
        expression.span,
      );
      const unresolved = method.genericParameters.filter((parameter) => !resolved.has(parameter));
      if (unresolved.length > 0)
        this.failUnresolvedCall(unresolved, `${owner}::${name}`, expression.span);
      result = substituteGenericType(result, resolved);
      for (const [parameter, type] of resolved) substitutions.set(parameter, type);
      bounds = this.resolveBoundDictionaries(
        methodSignature,
        checkedSignature.substitutions,
        expression.span,
      );
      checkedArguments = checkedSignature;
    } else
      checkedArguments = this.checkConcreteArguments(
        expression,
        parameters,
        selected.method.parameterNames,
        selected.method.variadic,
        `associated function '${name}'`,
      );
    const providers = selected.method.requirements.map((requirement) =>
      this.resolveProvider(substituteGenericType(requirement, substitutions), expression.span),
    );
    if (providers.some((provider) => !provider))
      this.fail(
        "missing-requirement",
        `associated function '${name}' requires ${selected.method.requirements.map(displayType).join(", ")}`,
        expression.span,
      );
    // The bound dictionary stands in for the receiver, which an associated
    // function never reads.
    return {
      kind: "trait-call",
      receiver: {
        kind: "trait-bound-dictionary",
        traitIndex: bound.traitIndex,
        boundIndex,
        type: `trait:${boundTrait}`,
        span: expression.callee.span,
      },
      traitIndex: selected.trait.index,
      methodIndex: selected.method.index,
      supertraitPath: selected.path.length > 0 ? selected.path : undefined,
      arguments: checkedArguments.arguments,
      argumentParameterIndices: checkedArguments.parameterIndices,
      ...(bounds ? { bounds } : {}),
      providers: providers as HirExpression[],
      erasedParameterTypes: selected.method.parameters.some(containsGenericType)
        ? selected.method.parameters
        : undefined,
      erasedTypeSubstitutions: orderedTypeSubstitutions([...substitutions.keys()], substitutions),
      erasedResultType: containsGenericType(selected.method.result)
        ? selected.method.result
        : undefined,
      type: result,
      span: expression.span,
    };
  }
}
