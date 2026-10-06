import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type {
  HirExpression,
  HirTrait,
  HirTraitMethod,
  HirTraitMethodFunction,
  ValueType,
} from "../hir.ts";
import { displayType, nominalGenericParts, nominalGenericType, readonlyType } from "../types.ts";
import { keepLiteralDefaults, speculate } from "./call-speculation.ts";
import {
  containsGenericType,
  genericTypeName,
  matchImplementationTarget,
  orderedTypeSubstitutions,
  substituteGenericType,
} from "./shared.ts";

import { CheckFailure, type Signature } from "./context.ts";
import { standardImportHint } from "./standard-uses.ts";
import { DebugPrintChecker } from "./debug-print-calls.ts";

export interface QualifiedCallExpression extends Extract<Expression, { kind: "call" }> {
  readonly callee: Extract<Expression, { kind: "qualified-name" }>;
}

/** An implementation that supplies an associated function for a type. */
type AssociatedCandidate = ReturnType<TraitCallChecker["associatedCandidates"]>[number];

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
   * call's arguments may not mention. Only a trait available at the
   * callee's span is a candidate (09-traits.md#r-trait.assoc-call.type.traits).
   */
  protected associatedCandidates(
    ownerType: ValueType,
    { name, span }: { readonly name: string; readonly span: SourceSpan },
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
      return candidateTrait && method && mapping && this.traitAvailable(candidateTrait.name, span)
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

  /**
   * What `Trait::name` says when only a supertrait of `trait` declares
   * `name`: the declaring trait, which a trait-qualified call or reference
   * must name (09-traits.md#r-trait.qualified.declaring). Empty otherwise.
   */
  protected declaringTraitHint(trait: HirTrait, name: string): string {
    const declaring = [
      ...this.findTraitMethods(trait, name),
      ...this.findTraitMethods(trait, name, [], new Set(), true),
    ].map(({ trait: owner }) => displayType(owner.name));
    const [first] = new Set(declaring);
    return first === undefined ? "" : `; it is declared by '${first}'; write ${first}::${name}`;
  }

  /**
   * `Type::f(args)` among the implementations `candidates` that supply the
   * associated function `f` for `ownerType` (09-traits.md#associated-function-calls).
   */
  protected checkTypeAssociatedCall(
    expression: QualifiedCallExpression,
    ownerType: ValueType,
    candidates: readonly AssociatedCandidate[],
    expected: ValueType | undefined,
  ): HirExpression {
    const associatedCandidates = [...candidates];
    const signatureOf = (candidate: AssociatedCandidate): Signature =>
      [...this.signatures.values()].find(
        (signature) => signature.index === candidate.mapping.functionIndex,
      )!;
    const callAssociated = (candidate: AssociatedCandidate): HirExpression =>
      this.checkDeclaredCall(
        {
          ...expression,
          callee: {
            kind: "name",
            name: signatureOf(candidate).name,
            span: expression.callee.span,
          },
        },
        expected,
        candidate.substitutions,
      );
    // 09 Conversion Trait and Method Resolution: among instantiations of one
    // generic trait, `Type::from(value)` selects the one whose parameters the
    // arguments fit.
    if (
      associatedCandidates.length > 1 &&
      associatedCandidates.every(
        (candidate) =>
          candidate.candidateTrait.index === associatedCandidates[0]!.candidateTrait.index,
      )
    ) {
      const trials = associatedCandidates.map((candidate) => {
        try {
          const call = speculate(this, () => {
            const call = callAssociated(candidate);
            if (expected) this.requireCoercion(call, expected, expression.span);
            return call;
          });
          return { candidate, call };
        } catch (error) {
          if (!(error instanceof CheckFailure)) throw error;
          return { candidate, call: undefined };
        }
      });
      let fitting = trials.filter((entry) => entry.call !== undefined);
      const kept = keepLiteralDefaults(
        fitting,
        expression,
        0,
        ({ candidate }) =>
          `${candidate.candidateTrait.name}[${candidate.traitArguments.map((argument) => displayType(substituteGenericType(argument, candidate.substitutions))).join(", ")}]`,
      );
      if (typeof kept === "string") this.fail("type-mismatch", kept, expression.span);
      fitting = kept;
      if (fitting.length === 0)
        this.fail(
          "type-mismatch",
          `the arguments of '${expression.callee.name}' fit no instantiation of trait '${displayType(associatedCandidates[0]!.candidateTrait.name)}' implemented by '${displayType(ownerType)}'; available: ${associatedCandidates.map((candidate) => `${displayType(candidate.candidateTrait.name)}[${candidate.traitArguments.map((argument) => displayType(substituteGenericType(argument, candidate.substitutions))).join(", ")}]`).join(", ")}`,
          expression.span,
        );
      if (fitting.length === 1)
        associatedCandidates.splice(0, associatedCandidates.length, fitting[0]!.candidate);
    }
    if (associatedCandidates.length > 1)
      this.fail(
        "ambiguous-method",
        `associated function '${expression.callee.name}' is supplied by multiple traits for '${displayType(ownerType)}'`,
        expression.callee.span,
      );
    const associated = associatedCandidates[0];
    if (!associated) {
      const ownerBase = nominalGenericParts(ownerType)?.name ?? ownerType;
      if (!this.dataTypes.has(ownerBase) && !this.enumTypes.has(ownerBase))
        this.fail(
          "unknown-type",
          `unknown associated-function owner '${displayType(ownerType)}'${standardImportHint(ownerBase, "type")}`,
          expression.callee.span,
        );
      this.fail(
        "unknown-method",
        `type '${displayType(ownerType)}' has no associated function '${expression.callee.name}'`,
        expression.callee.span,
      );
    }
    // The target's arguments solve the implementation's own parameters.
    return callAssociated(associated);
  }

  /**
   * `Trait::f(args)` for an associated function `f`: `Self` is inferred like
   * a generic argument of the call, from the arguments and the expected
   * type, and the call is `Self`'s implementation of `f` for that trait
   * (09-traits.md#r-trait.assoc-call.trait).
   */
  protected checkTraitAssociatedCall(
    expression: QualifiedCallExpression,
    trait: HirTrait,
    traitArguments: readonly ValueType[],
    method: HirTraitMethod,
    expected: ValueType | undefined,
  ): HirExpression {
    const name = `${trait.name}::${method.name}`;
    const substitutions = new Map(
      trait.genericParameters.map((parameter, index) => [parameter, traitArguments[index]!]),
    );
    const signature: Signature = {
      name,
      index: -1,
      suspending: method.suspending,
      // Explicit type arguments fill the method's own parameters first.
      genericParameters: [...method.genericParameters, "Self"],
      genericBounds: [],
      rowParameters: [],
      parameters: method.parameters.map((parameter) =>
        substituteGenericType(parameter, substitutions),
      ),
      parameterNames: method.parameterNames,
      defaultFunctionNames: method.parameters.map(() => undefined),
      variadic: method.variadic,
      result: substituteGenericType(method.result, substitutions),
      requirements: [],
      span: method.span,
    };
    let inferred: ValueType | undefined;
    try {
      inferred = speculate(this, () =>
        this.checkSignatureArguments(
          expression,
          signature,
          expected,
          `associated function '${displayType(name)}'`,
        ).substitutions.get("Self"),
      );
    } catch (error) {
      if (!(error instanceof CheckFailure)) throw error;
    }
    const self = inferred === undefined ? undefined : readonlyType(inferred);
    if (self === undefined || (containsGenericType(self) && !genericTypeName(self)))
      this.fail(
        "cannot-infer-type",
        `could not infer Self of '${displayType(name)}'; write Type::${method.name}() or T::${method.name}()`,
        expression.span,
      );
    const generic = genericTypeName(self);
    if (generic && this.signature.genericParameters.includes(generic))
      return this.checkBoundAssociatedCall(
        { ...expression, callee: { ...expression.callee, owner: generic } },
        generic,
        trait.index,
      );
    const candidates = this.associatedCandidates(self, expression.callee).filter(
      (candidate) =>
        candidate.candidateTrait.index === trait.index &&
        candidate.traitArguments.every(
          (argument, index) =>
            substituteGenericType(argument, candidate.substitutions) === traitArguments[index],
        ),
    );
    if (candidates.length === 0)
      this.fail(
        "unsatisfied-trait-bound",
        `type '${displayType(self)}' does not implement ${displayType(trait.name)}, required by '${displayType(name)}'`,
        expression.span,
      );
    return this.checkTypeAssociatedCall(expression, self, candidates, expected);
  }

  // `T::f(...)` on a type parameter calls the associated function through
  // the bound that supplies it (09-traits.md#associated-function-calls);
  // `Trait::f(...)` whose `Self` is `T` keeps only `onlyTrait`'s `f`.
  protected checkBoundAssociatedCall(
    expression: QualifiedCallExpression,
    owner: string,
    onlyTrait?: number,
  ): HirExpression {
    const name = expression.callee.name;
    const candidates = this.signature.genericBounds.flatMap((bound, boundIndex) => {
      if (bound.parameter !== owner) return [];
      const trait = this.traitTypes.get(bound.traitName)!;
      return this.findTraitMethods(trait, name, [], new Set(), true)
        .filter((selected) => onlyTrait === undefined || selected.trait.index === onlyTrait)
        .map((selected) => ({ bound, boundIndex, selected }));
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
        "unknown-method",
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
        this.failUnresolvedCall(unresolved, `${owner}::${name}`, expression.span, resolved);
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
