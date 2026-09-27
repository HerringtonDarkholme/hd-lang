import type { Expression } from "../ast.ts";
import type { HirExpression, HirTrait, HirTraitMethod, ValueType } from "../hir.ts";
import { nominalGenericType } from "../types.ts";
import { containsGenericType, substituteGenericType } from "./shared.ts";

import { InspectChecker } from "./expression-inspect.ts";

export interface QualifiedCallExpression extends Extract<Expression, { kind: "call" }> {
  readonly callee: Extract<Expression, { kind: "qualified-name" }>;
}

export interface ResolvedTraitMethod {
  readonly method: HirTraitMethod;
  readonly path: readonly number[];
  readonly trait: HirTrait;
}

/** Trait method lookup through supertraits, and associated calls through a bound. */
export abstract class TraitCallChecker extends InspectChecker {
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
    if (selected.method.genericParameters.length > 0 || selected.method.suspending)
      this.fail(
        "unsupported-bound-associated-call",
        `the prototype calls only non-generic, non-suspending associated functions through a bound`,
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
    const result = substituteGenericType(selected.method.result, substitutions);
    const checkedArguments = this.checkConcreteArguments(
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
        `associated function '${name}' requires ${selected.method.requirements.join(", ")}`,
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
      providers: providers as HirExpression[],
      erasedParameterTypes: selected.method.parameters.some(containsGenericType)
        ? selected.method.parameters
        : undefined,
      erasedResultType: containsGenericType(selected.method.result)
        ? selected.method.result
        : undefined,
      type: result,
      span: expression.span,
    };
  }
}
