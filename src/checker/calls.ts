import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirExpression, HirLocal, HirProviderContextEntry, ValueType } from "../hir.ts";
import {
  contextKeys,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  suspensionType,
} from "../types.ts";
import {
  isKnownType,
  weakenBoundedGenericActual,
  type InherentMethod,
  type PlannedArgument,
  type Signature,
} from "./context.ts";
import {
  containsGenericType,
  genericTypeName,
  inferGenericType,
  instantiateRowRequirement,
  requirementExclusions,
  requirementKeysMayCollide,
  resolveGenericType,
  rowParameterName,
  substituteGenericType,
  traitKeyName,
} from "./shared.ts";

import { StatementChecker } from "./statements.ts";

interface ResolvedCallProviders {
  readonly providers: readonly HirExpression[];
  readonly missing: readonly string[];
}

interface CheckedArguments {
  readonly arguments: readonly HirExpression[];
  readonly parameterIndices?: readonly number[];
}

interface CheckedSignatureArguments extends CheckedArguments {
  readonly defaultParameterIndices: readonly number[];
  readonly substitutions: ReadonlyMap<string, ValueType>;
  readonly rowSubstitutions: ReadonlyMap<string, readonly string[]>;
}

interface CheckedProviderEntries {
  readonly entries: readonly HirProviderContextEntry[];
  readonly providers: Map<string, HirLocal>;
}

export abstract class CallChecker extends StatementChecker {
  protected resolveProvider(key: string, span: SourceSpan): HirExpression | undefined {
    for (let index = this.providerScopes.length - 1; index >= 0; index -= 1) {
      const local = this.providerScopes[index]!.get(key);
      if (local) return this.referenceLocal(local, span);
    }
    const providerIndex = this.signature.requirements.indexOf(key);
    if (providerIndex >= 0)
      return {
        kind: "provider-use",
        providerIndex,
        key,
        type: rowParameterName(key)
          ? `provider-row:${rowParameterName(key)}`
          : this.providerValueType(key),
        span,
      };
    const requestedRow = rowParameterName(key);
    if (requestedRow) {
      const availableIndex = this.signature.requirements.findIndex(
        (requirement) => rowParameterName(requirement) === requestedRow,
      );
      if (availableIndex >= 0) {
        const available = this.signature.requirements[availableIndex]!;
        const requestedExcluded = new Set(requirementExclusions(key));
        const restoredKeys = requirementExclusions(available).filter(
          (excluded) => !requestedExcluded.has(excluded),
        );
        const restoredProviders = restoredKeys.map((restored) =>
          this.resolveProvider(restored, span),
        );
        if (restoredProviders.every((provider) => provider !== undefined)) {
          const base: HirExpression = {
            kind: "provider-use",
            providerIndex: availableIndex,
            key: available,
            type: `provider-row:${requestedRow}`,
            span,
          };
          return restoredKeys.length === 0
            ? base
            : {
                kind: "provider-pack",
                keys: restoredKeys,
                providers: restoredProviders as HirExpression[],
                bases: [base],
                type: `provider-row:${requestedRow}`,
                span,
              };
        }
      }
    }
    if (!this.inferRequirements) return undefined;
    let inferredIndex = this.inferredRequirements.indexOf(key);
    if (inferredIndex < 0) {
      this.inferredRequirements.push(key);
      inferredIndex = this.inferredRequirements.length - 1;
    }
    return {
      kind: "provider-use",
      providerIndex: inferredIndex,
      key,
      type: this.providerValueType(key),
      span,
    };
  }

  protected resolveCallProviders(
    requirements: readonly string[],
    substitutions: ReadonlyMap<string, ValueType>,
    rowSubstitutions: ReadonlyMap<string, readonly string[]>,
    span: SourceSpan,
  ): ResolvedCallProviders {
    const providers: HirExpression[] = [];
    const missing: string[] = [];
    for (const sourceRequirement of requirements) {
      const requirement = rowParameterName(sourceRequirement)
        ? sourceRequirement
        : substituteGenericType(sourceRequirement, substitutions, rowSubstitutions);
      const row = rowParameterName(requirement);
      if (!row) {
        const provider = this.resolveProvider(requirement, span);
        if (provider) providers.push(provider);
        else missing.push(requirement);
        continue;
      }
      const substitution = rowSubstitutions.get(row);
      const keys =
        substitution &&
        instantiateRowRequirement(requirement, substitution).map((key) =>
          rowParameterName(key) ? key : substituteGenericType(key, substitutions, rowSubstitutions),
        );
      if (!keys) {
        missing.push(requirement);
        continue;
      }
      const symbolicKeys = keys.filter((key) => rowParameterName(key));
      const concreteKeys = keys.filter((key) => !rowParameterName(key));
      if (symbolicKeys.length === 1 && concreteKeys.length === 0) {
        const provider = this.resolveProvider(symbolicKeys[0]!, span);
        if (provider) providers.push(provider);
        else missing.push(symbolicKeys[0]!);
        continue;
      }
      const bases = symbolicKeys.map((key) => this.resolveProvider(key, span));
      symbolicKeys.forEach((key, index) => {
        if (!bases[index]) missing.push(key);
      });
      const rowProviders = concreteKeys.map((key) => this.resolveProvider(key, span));
      concreteKeys.forEach((key, index) => {
        if (!rowProviders[index]) missing.push(key);
      });
      providers.push({
        kind: "provider-pack",
        keys: concreteKeys,
        providers: rowProviders.filter(
          (provider): provider is HirExpression => provider !== undefined,
        ),
        bases: bases.filter((provider): provider is HirExpression => provider !== undefined),
        type: `provider-row:${row}`,
        span,
      });
    }
    return { providers, missing };
  }

  protected warnAbsentRowSubtractions(
    requirements: readonly string[],
    rowSubstitutions: ReadonlyMap<string, readonly string[]>,
    span: SourceSpan,
  ): void {
    for (const requirement of requirements) {
      const row = rowParameterName(requirement);
      if (!row) continue;
      const substitution = rowSubstitutions.get(row);
      if (!substitution || substitution.some((entry) => rowParameterName(entry))) continue;
      for (const excluded of requirementExclusions(requirement)) {
        if (!substitution.includes(excluded)) {
          this.diagnostics.push({
            code: "requirement-subtract-absent",
            message: `requirement '${excluded}' is absent from inferred row ${row}`,
            span,
            severity: "warning",
          });
        }
      }
    }
  }

  protected requireDrivableSuspension(expression: Expression): void {
    if (expression.kind !== "name") return;
    const local = this.resolveLocal(expression.name);
    const binding = local ?? this.resolveGlobal(expression.name);
    if (binding && !binding.drivable) {
      this.fail(
        "mutable-receiver-required",
        "a stored suspension must have an explicit mut Suspend[T] binding to be driven or cancelled",
        expression.span,
      );
    }
  }

  protected resolveArgumentMapping(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    parameterNames: readonly string[],
    callable: string,
  ): readonly number[] | undefined {
    if (!expression.argumentNames) return undefined;
    const mapping: number[] = [];
    const assigned = new Set<number>();
    let positionalIndex = 0;
    expression.argumentNames.forEach((name, argumentIndex) => {
      const parameterIndex = name === undefined ? positionalIndex++ : parameterNames.indexOf(name);
      if (parameterIndex < 0) {
        this.fail(
          "unknown-named-argument",
          `${callable} has no parameter named '${name}'`,
          expression.arguments[argumentIndex]!.span,
        );
      }
      if (assigned.has(parameterIndex)) {
        this.fail(
          "duplicate-argument",
          `parameter '${parameterNames[parameterIndex]}' is supplied more than once`,
          expression.arguments[argumentIndex]!.span,
        );
      }
      assigned.add(parameterIndex);
      mapping.push(parameterIndex);
    });
    return mapping.every((parameterIndex, argumentIndex) => parameterIndex === argumentIndex)
      ? undefined
      : mapping;
  }

  protected planArguments(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    parameterNames: readonly string[],
    variadic: boolean,
    callable: string,
    defaultParameterIndices: ReadonlySet<number> = new Set(),
  ): readonly PlannedArgument[] {
    const fixedCount = variadic ? parameterNames.length - 1 : parameterNames.length;
    const names = expression.argumentNames ?? expression.arguments.map(() => undefined);
    const spreads = expression.argumentSpreads ?? expression.arguments.map(() => false);
    const entries: PlannedArgument[] = [];
    const assigned = new Set<number>();
    let positionalIndex = 0;
    let varargElements: number[] = [];

    const flushVarargElements = (): void => {
      if (varargElements.length === 0) return;
      const parameterIndex = parameterNames.length - 1;
      if (assigned.has(parameterIndex)) {
        this.fail(
          "duplicate-argument",
          `parameter '${parameterNames[parameterIndex]}' is supplied more than once`,
          expression.arguments[varargElements[0]!]!.span,
        );
      }
      assigned.add(parameterIndex);
      entries.push({ parameterIndex, argumentIndices: varargElements, kind: "vararg-elements" });
      varargElements = [];
    };

    expression.arguments.forEach((argument, argumentIndex) => {
      const name = names[argumentIndex];
      const spread = spreads[argumentIndex] ?? false;
      if (name !== undefined) {
        flushVarargElements();
        const parameterIndex = parameterNames.indexOf(name);
        if (parameterIndex < 0) {
          this.fail(
            "unknown-named-argument",
            `${callable} has no parameter named '${name}'`,
            argument.span,
          );
        }
        if (assigned.has(parameterIndex)) {
          this.fail(
            "duplicate-argument",
            `parameter '${parameterNames[parameterIndex]}' is supplied more than once`,
            argument.span,
          );
        }
        assigned.add(parameterIndex);
        entries.push({ parameterIndex, argumentIndices: [argumentIndex], kind: "single" });
        return;
      }
      if (spread) {
        if (!variadic) {
          this.fail(
            "positional-spread-needs-vararg",
            `${callable} has no variadic parameter for this positional spread`,
            argument.span,
          );
        }
        flushVarargElements();
        const parameterIndex = parameterNames.length - 1;
        if (assigned.has(parameterIndex)) {
          this.fail(
            "duplicate-argument",
            `parameter '${parameterNames[parameterIndex]}' is supplied more than once`,
            argument.span,
          );
        }
        assigned.add(parameterIndex);
        entries.push({ parameterIndex, argumentIndices: [argumentIndex], kind: "single" });
        return;
      }
      if (positionalIndex < fixedCount) {
        const parameterIndex = positionalIndex++;
        assigned.add(parameterIndex);
        entries.push({ parameterIndex, argumentIndices: [argumentIndex], kind: "single" });
        return;
      }
      if (!variadic) {
        this.fail(
          "argument-count",
          `${callable} expects ${parameterNames.length} arguments, received ${expression.arguments.length}`,
          argument.span,
        );
      }
      const parameterIndex = parameterNames.length - 1;
      if (assigned.has(parameterIndex)) {
        this.fail(
          "duplicate-argument",
          `parameter '${parameterNames[parameterIndex]}' is supplied more than once`,
          argument.span,
        );
      }
      varargElements.push(argumentIndex);
    });
    flushVarargElements();

    const missing = parameterNames
      .slice(0, fixedCount)
      .filter((_, index) => !assigned.has(index) && !defaultParameterIndices.has(index));
    if (missing.length > 0) {
      this.fail(
        "argument-count",
        `${callable} is missing argument${missing.length === 1 ? "" : "s"} ${missing.join(", ")}`,
        expression.span,
      );
    }
    if (variadic && !assigned.has(parameterNames.length - 1)) {
      entries.push({
        parameterIndex: parameterNames.length - 1,
        argumentIndices: [],
        kind: "vararg-elements",
      });
    }
    return entries;
  }

  protected checkInherentMethodCall(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    receiver: HirExpression,
    method: InherentMethod,
  ): HirExpression {
    if (method.receiverMutable && mutableInner(receiver.type) === undefined) {
      this.fail(
        "mutable-receiver-required",
        `method '${method.name}' requires mutable access to ${method.targetType}`,
        expression.callee.span,
      );
    }
    const receiverParameterType = method.receiverMutable
      ? mutableType(method.targetType)
      : method.targetType;
    const methodReceiver = this.requireCoercion(receiver, receiverParameterType, receiver.span);
    const checkedArguments = this.checkConcreteArguments(
      expression,
      method.parameters,
      method.parameterNames,
      method.variadic,
      `method '${method.name}'`,
    );
    const signature = this.signatures.get(method.functionName)!;
    const providers = signature.requirements.map((requirement) =>
      this.resolveProvider(requirement, expression.span),
    );
    const missing = signature.requirements.filter((_, index) => !providers[index]);
    if (missing.length > 0)
      this.fail(
        "missing-requirement",
        `method '${method.name}' requires ${missing.join(" + ")}`,
        expression.span,
      );
    const argumentParameterIndices = checkedArguments.parameterIndices
      ? [0, ...checkedArguments.parameterIndices.map((parameterIndex) => parameterIndex + 1)]
      : undefined;
    return method.suspending
      ? {
          kind: "suspend-construct",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: [methodReceiver, ...checkedArguments.arguments],
          argumentParameterIndices,
          providers: providers as HirExpression[],
          type: suspensionType(signature.index, signature.result),
          span: expression.span,
        }
      : {
          kind: "call",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: [methodReceiver, ...checkedArguments.arguments],
          argumentParameterIndices,
          providers: providers as HirExpression[],
          type: signature.result,
          span: expression.span,
        };
  }

  protected checkConcreteArguments(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    parameterTypes: readonly ValueType[],
    parameterNames: readonly string[],
    variadic: boolean,
    callable: string,
  ): CheckedArguments {
    if (expression.typeArguments)
      this.fail("unexpected-type-arguments", `${callable} is not generic`, expression.span);
    const plan = this.planArguments(expression, parameterNames, variadic, callable);
    const arguments_ = plan.map((entry): HirExpression => {
      const formal = parameterTypes[entry.parameterIndex]!;
      if (entry.kind === "single") {
        const source = expression.arguments[entry.argumentIndices[0]!]!;
        const checked = this.checkExpression(source, formal);
        if (mutableInner(formal) === checked.type) {
          this.fail(
            "readonly-argument-to-mutable-parameter",
            `readonly argument '${checked.type}' cannot satisfy mutable parameter '${formal}'`,
            source.span,
          );
        }
        return this.requireCoercion(checked, formal, source.span);
      }
      const nominal = nominalGenericParts(formal);
      const elementType = nominal?.name === "list" ? nominal.arguments[0]! : "void";
      const elements = entry.argumentIndices.map((argumentIndex) => {
        const source = expression.arguments[argumentIndex]!;
        return this.requireCoercion(
          this.checkExpression(source, elementType),
          elementType,
          source.span,
        );
      });
      return { kind: "list", elements, elementType, type: formal, span: expression.span };
    });
    const mapping = plan.map((entry) => entry.parameterIndex);
    return {
      arguments: arguments_,
      parameterIndices: mapping.every(
        (parameterIndex, argumentIndex) => parameterIndex === argumentIndex,
      )
        ? undefined
        : mapping,
    };
  }

  protected checkSignatureArguments(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    signature: Signature,
    expected?: ValueType,
  ): CheckedSignatureArguments {
    const substitutions = new Map<string, ValueType>();
    const rowSubstitutions = new Map<string, readonly string[]>();
    if (expression.typeArguments) {
      if (expression.typeArguments.length !== signature.genericParameters.length) {
        const code =
          expression.typeArguments.length < signature.genericParameters.length
            ? "partial-generic-arguments"
            : "generic-argument-count";
        this.fail(
          code,
          `function '${signature.name}' expects ${signature.genericParameters.length} type arguments, received ${expression.typeArguments.length}`,
          expression.span,
        );
      }
      expression.typeArguments.forEach((argument, index) => {
        if (argument.name === "_") return;
        substitutions.set(signature.genericParameters[index]!, this.resolveType(argument));
      });
    }
    if (expected) inferGenericType(signature.result, expected, substitutions, rowSubstitutions);
    const defaultParameters = new Set(
      signature.defaultFunctionNames.flatMap((name, index) => (name ? [index] : [])),
    );
    const plan = this.planArguments(
      expression,
      signature.parameterNames,
      signature.variadic,
      `function '${signature.name}'`,
      defaultParameters,
    );
    const arguments_ = plan.map((entry): HirExpression => {
      const formal = signature.parameters[entry.parameterIndex]!;
      if (entry.kind === "single") {
        const source = expression.arguments[entry.argumentIndices[0]!]!;
        const inferredFormal = substituteGenericType(formal, substitutions, rowSubstitutions);
        const checked = this.checkExpression(
          source,
          containsGenericType(inferredFormal) ? undefined : inferredFormal,
        );
        const boundedParameters = new Set(signature.genericBounds.map((bound) => bound.parameter));
        const inferredActual = weakenBoundedGenericActual(formal, checked.type, boundedParameters);
        const conflict = inferGenericType(formal, inferredActual, substitutions, rowSubstitutions);
        if (conflict) this.fail("generic-type-mismatch", conflict, source.span);
        const instantiatedFormal = substituteGenericType(formal, substitutions, rowSubstitutions);
        if (mutableInner(instantiatedFormal) === checked.type) {
          this.fail(
            "readonly-argument-to-mutable-parameter",
            `readonly argument '${checked.type}' cannot satisfy mutable parameter '${instantiatedFormal}'`,
            source.span,
          );
        }
        return this.requireCoercion(checked, instantiatedFormal, source.span);
      }
      const nominal = nominalGenericParts(formal);
      const elementFormal = nominal?.name === "list" ? nominal.arguments[0]! : "void";
      const elements = entry.argumentIndices.map((argumentIndex) => {
        const source = expression.arguments[argumentIndex]!;
        const inferredElement = substituteGenericType(
          elementFormal,
          substitutions,
          rowSubstitutions,
        );
        const checked = this.checkExpression(
          source,
          containsGenericType(inferredElement) ? undefined : inferredElement,
        );
        const conflict = inferGenericType(
          elementFormal,
          checked.type,
          substitutions,
          rowSubstitutions,
        );
        if (conflict) this.fail("generic-type-mismatch", conflict, source.span);
        return this.requireCoercion(
          checked,
          substituteGenericType(elementFormal, substitutions, rowSubstitutions),
          source.span,
        );
      });
      const elementType = substituteGenericType(elementFormal, substitutions, rowSubstitutions);
      return {
        kind: "list",
        elements,
        elementType,
        type: nominalGenericType("list", [elementType]),
        span: expression.span,
      };
    });
    const mapping = plan.map((entry) => entry.parameterIndex);
    const supplied = new Set(mapping);
    return {
      arguments: arguments_,
      parameterIndices: mapping.every(
        (parameterIndex, argumentIndex) => parameterIndex === argumentIndex,
      )
        ? undefined
        : mapping,
      defaultParameterIndices: [...defaultParameters].filter(
        (parameterIndex) => !supplied.has(parameterIndex),
      ),
      substitutions,
      rowSubstitutions,
    };
  }

  protected checkProviderEntries(
    entries: Extract<Expression, { kind: "provider-context" | "provider-with" }>["entries"],
  ): CheckedProviderEntries {
    const checked: HirProviderContextEntry[] = [];
    const providers = new Map<string, HirLocal>();
    for (const entry of entries) {
      let value = this.checkExpression(entry.value);
      if (entry.kind === "binding") {
        const key = this.canonicalProviderKey(entry.key, entry.span);
        this.rejectProviderKeyCollision(key, providers.keys(), entry.span);
        const trait = this.traitTypes.get(traitKeyName(key));
        if (trait) value = this.requireCoercion(value, `trait:${key}`, entry.value.span);
        else if (!value.type.startsWith("provider:")) {
          this.fail(
            "provider-type-mismatch",
            `provider binding '${key}' requires an opaque provider value`,
            entry.value.span,
          );
        }
        const local = this.addProviderLocal(key, entry.span);
        checked.push({ kind: "binding", key, local, value });
        providers.set(key, local);
        continue;
      }
      const keys = contextKeys(value.type);
      if (!keys)
        this.fail(
          "context-spread-type",
          `context spread requires a $.Context value, found '${value.type}'`,
          entry.value.span,
        );
      const contextLocal: HirLocal = {
        name: "$context-spread",
        type: value.type,
        index: this.locals.length,
        mutable: false,
        parameter: false,
        span: entry.span,
      };
      this.locals.push(contextLocal);
      const spreadProviders = keys.map((key, fieldIndex) => {
        this.rejectProviderKeyCollision(key, providers.keys(), entry.span);
        const local = this.addProviderLocal(key, entry.span);
        providers.set(key, local);
        return { key, local, fieldIndex };
      });
      checked.push({ kind: "spread", contextLocal, value, providers: spreadProviders });
    }
    return { entries: checked, providers };
  }

  protected addProviderLocal(key: string, span: SourceSpan): HirLocal {
    const local: HirLocal = {
      name: `$provider-${key}`,
      type: this.providerValueType(key),
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span,
    };
    this.locals.push(local);
    return local;
  }

  protected providerValueType(key: string): ValueType {
    return this.traitTypes.has(traitKeyName(key)) ? `trait:${key}` : `provider:${key}`;
  }

  protected rejectProviderKeyCollision(
    key: string,
    currentKeys: Iterable<string>,
    span: SourceSpan,
  ): void {
    const visibleKeys = [
      ...this.signature.requirements.filter((requirement) => !rowParameterName(requirement)),
      ...this.providerScopes.flatMap((scope) => [...scope.keys()]),
      ...currentKeys,
    ];
    const collision = visibleKeys.find((visible) => requirementKeysMayCollide(visible, key));
    if (collision) {
      this.fail(
        "generic-requirement-key-collision",
        `provider keys '${collision}' and '${key}' can become identical after generic substitution`,
        span,
      );
    }
  }

  protected canonicalProviderKey(key: string, span: SourceSpan): string {
    const resolved = resolveGenericType(
      key,
      new Set(this.signature.genericParameters),
      new Set(this.signature.rowParameters),
    );
    const nominal = nominalGenericParts(resolved);
    if (!nominal) return resolved;
    const trait = this.traitTypes.get(nominal.name);
    if (!trait) this.fail("unknown-requirement", `unknown generic requirement key '${key}'`, span);
    if (trait.genericParameters.length !== nominal.arguments.length) {
      this.fail(
        "generic-arity",
        `trait '${trait.name}' expects ${trait.genericParameters.length} type arguments`,
        span,
      );
    }
    if (
      !nominal.arguments.every((argument) =>
        isKnownType(argument, this.dataTypes, this.enumTypes, this.traitTypes),
      )
    ) {
      this.fail("unknown-type", `requirement key '${key}' contains an unknown type`, span);
    }
    return resolved;
  }

  protected resolveBoundDictionaries(
    signature: Signature,
    substitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan,
  ): HirExpression[] {
    for (const parameter of signature.referenceParameters ?? []) {
      const actual = substitutions.get(parameter);
      if (!actual)
        this.fail(
          "unresolved-generic-placeholder",
          `could not infer generic parameter ${parameter}`,
          span,
        );
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        if (!(this.signature.referenceParameters ?? []).includes(forwarded)) {
          this.fail(
            "missing-trait-implementation",
            `generic parameter '${forwarded}' does not satisfy Reference`,
            span,
          );
        }
      } else if (!this.isIdentityType(actual)) {
        this.fail(
          "missing-trait-implementation",
          `type '${actual}' does not implement Reference`,
          span,
        );
      }
    }
    return signature.genericBounds.map((bound) => {
      const actual = substitutions.get(bound.parameter);
      if (!actual)
        this.fail(
          "unresolved-generic-placeholder",
          `could not infer generic parameter ${bound.parameter}`,
          span,
        );
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        const boundIndex = this.signature.genericBounds.findIndex(
          (candidate) =>
            candidate.parameter === forwarded && candidate.traitIndex === bound.traitIndex,
        );
        if (boundIndex < 0) {
          this.fail(
            "missing-trait-implementation",
            `generic parameter '${forwarded}' does not satisfy ${bound.traitName}`,
            span,
          );
        }
        return {
          kind: "trait-bound-dictionary",
          traitIndex: bound.traitIndex,
          boundIndex,
          type: `trait:${bound.traitName}`,
          span,
        };
      }
      const implementation = this.implementations.find(
        (candidate) => candidate.traitIndex === bound.traitIndex && candidate.targetType === actual,
      );
      if (!implementation) {
        this.fail(
          "missing-trait-implementation",
          `type '${actual}' does not implement ${bound.traitName}`,
          span,
        );
      }
      return {
        kind: "trait-dictionary",
        traitIndex: bound.traitIndex,
        implementationIndex: implementation.index,
        type: `trait:${bound.traitName}`,
        span,
      };
    });
  }
}
