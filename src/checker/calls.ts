import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type {
  HirExpression,
  HirGenericBound,
  HirLocal,
  HirProviderContextEntry,
  ValueType,
} from "../hir.ts";
import { forwardingPlan } from "./assignability.ts";
import { inferTypesThroughBounds } from "./bound-inference.ts";
import { DERIVED_IMPLEMENTATION_SPANS } from "./derive-intrinsics.ts";
import { standardSubmoduleFunctionIdentity } from "./standard-library.ts";
import {
  contextKeys,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  rowArgumentKeys,
  storedSuspensionParts,
  suspensionType,
  tupleParts,
  tupleLayout,
  tupleRest,
  tupleType,
  inputsInner,
  displayType,
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
  mentionsUnsolved,
  genericTypeName,
  inferGenericType,
  lacksOnlyPatternKeys,
  matchGenericTypePattern,
  matchTraitImplementation,
  MAX_BOUND_DEPTH,
  normalizeBoundProjections,
  orderedTypeSubstitutions,
  requirementKeysMayCollide,
  resolveGenericType,
  resolveTraitType,
  rowParameterName,
  substituteGenericType,
  argumentOwnType,
  defaultCallFields,
  traitKeyName,
  traitTypeName,
} from "./shared.ts";
import { spelledCall } from "./spelling.ts";
import { checkLiteralArgumentsLast } from "./literal-arguments.ts";
import { requirementKeyDiagnostics, resolveRequirementKeyTypes } from "./requirement-keys.ts";
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
  /** A std submodule member, unless a value binding owns the receiver name. */
  protected standardSubmoduleFunction(receiver: string, member: string): string | undefined {
    if (
      this.resolveLocal(receiver) ||
      this.availableCaptures.has(receiver) ||
      this.globals.has(receiver) ||
      this.signatures.has(receiver)
    )
      return undefined;
    const identity = standardSubmoduleFunctionIdentity(this.imports.get(receiver), member);
    return identity === undefined ? undefined : this.signatures.get(identity)?.name;
  }

  /** Arguments a call rewrite has already checked, such as a spread tuple's elements. */
  // Enumerable so argument trials can roll back context-dependent entries.
  // Its lifetime is the function checker, not a process-wide AST cache.
  protected readonly prechecked = new Map<Expression, HirExpression>();

  /**
   * Whether `type` implements the trait `traitName`: a type parameter through
   * its bounds, any other type through an implementation, std's included,
   * whose bounds its type arguments meet, or through the language's own
   * dictionary, as a primitive's `Eq`.
   */
  protected implementsTrait(type: ValueType, traitName: string, depth = 0): boolean {
    const trait = this.traitTypes.get(traitName);
    if (!trait) return false;
    const target = readonlyType(type);
    const generic = genericTypeName(target);
    if (generic)
      return this.signature.genericBounds.some(
        (bound) => bound.parameter === generic && bound.traitIndex === trait.index,
      );
    if (depth > MAX_BOUND_DEPTH) return true;
    if (this.builtinTraitDictionaryPlan(trait.index, target, [], this.signature.span)) return true;
    return this.implementations.some((implementation) => {
      const substitutions = matchTraitImplementation(implementation, trait.index, target, []);
      return (
        substitutions !== undefined &&
        implementation.genericBounds.every((bound) =>
          this.implementsTrait(
            substitutions.get(bound.parameter) ?? bound.parameter,
            bound.traitName,
            depth + 1,
          ),
        )
      );
    });
  }

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
        substitution.map((key) =>
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

  protected requireDrivableSuspension(expression: Expression): void {
    if (expression.kind !== "name") return;
    const local = this.resolveLocal(expression.name);
    const binding = local ?? this.resolveGlobal(expression.name);
    const stored = binding && storedSuspensionParts(binding.type);
    if (stored?.mutable) return;
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
    unknownNameCode = "unknown-named-argument",
  ): readonly PlannedArgument[] {
    const fixedCount = variadic ? parameterNames.length - 1 : parameterNames.length;
    const names = expression.argumentNames ?? expression.arguments.map(() => undefined);
    const spreads = expression.argumentSpreads ?? expression.arguments.map(() => false);
    const entries: PlannedArgument[] = [];
    const assigned = new Set<number>();
    let positionalIndex = 0;
    let varargElements: number[] = [];

    const rejectDuplicate = (parameterIndex: number, argumentIndex: number): void => {
      if (assigned.has(parameterIndex))
        this.fail(
          "duplicate-argument",
          `parameter '${parameterNames[parameterIndex]}' is supplied more than once`,
          expression.arguments[argumentIndex]!.span,
        );
    };
    const supply = (parameterIndex: number, argumentIndex: number): void => {
      rejectDuplicate(parameterIndex, argumentIndex);
      assigned.add(parameterIndex);
      entries.push({ parameterIndex, argumentIndices: [argumentIndex], kind: "single" });
    };
    const flushVarargElements = (): void => {
      if (varargElements.length === 0) return;
      const parameterIndex = parameterNames.length - 1;
      rejectDuplicate(parameterIndex, varargElements[0]!);
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
          this.fail(unknownNameCode, `${callable} has no parameter named '${name}'`, argument.span);
        }
        supply(parameterIndex, argumentIndex);
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
        supply(parameterNames.length - 1, argumentIndex);
        return;
      }
      // A trailing block always supplies the final parameter, so defaulted
      // parameters before it may be omitted (07-functions.md#r-fn.default.final-function).
      if (
        argument.kind === "closure" &&
        argument.trailing === true &&
        !variadic &&
        argumentIndex === expression.arguments.length - 1 &&
        parameterNames.length > 0
      ) {
        supply(parameterNames.length - 1, argumentIndex);
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
      rejectDuplicate(parameterNames.length - 1, argumentIndex);
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
    expected?: ValueType,
  ): HirExpression {
    if (method.receiverMutable && mutableInner(receiver.type) === undefined) {
      this.failReadonlyMethodReceiver(
        method.name,
        expression.callee.kind === "member" ? expression.callee.receiver : expression.callee,
        receiver,
      );
    }
    // A generic target such as `Box[T]` fixes the implementation's
    // parameters from the receiver (09-traits.md#inherent-member-names).
    const targetSubstitutions = new Map<string, ValueType>();
    if (method.targetGenericParameters)
      matchGenericTypePattern(method.targetType, readonlyType(receiver.type), targetSubstitutions);
    const targetType = substituteGenericType(method.targetType, targetSubstitutions);
    const receiverParameterType = method.receiverMutable ? mutableType(targetType) : targetType;
    const methodReceiver = this.requireCoercion(receiver, receiverParameterType, receiver.span);
    const signature = this.signatures.get(method.functionName)!;
    const callSignature: Signature = {
      ...signature,
      // Explicit type arguments name the method's own parameters.
      genericParameters: signature.genericParameters.filter(
        (parameter) => !targetSubstitutions.has(parameter),
      ),
      parameters: signature.parameters.slice(1),
      parameterNames: signature.parameterNames.slice(1),
      defaultFunctionNames: signature.defaultFunctionNames.slice(1),
    };
    const checkedArguments = this.checkSignatureArguments(
      expression,
      callSignature,
      expected,
      `method '${method.name}'`,
      targetSubstitutions,
    );
    const { rowSubstitutions } = checkedArguments;
    // The receiver fills parameter 0, so a defaulted parameter's index counts it.
    const defaultFields = defaultCallFields(
      this.signatures,
      callSignature.defaultFunctionNames,
      checkedArguments.defaultParameterIndices,
      signature.parameters,
      1,
    );
    const substitutions = this.resolveAssociatedTypeSubstitutions(
      signature,
      checkedArguments.substitutions,
      expression.span,
    );
    const unresolved = signature.genericParameters.filter(
      (parameter) => !substitutions.has(parameter),
    );
    if (unresolved.length > 0)
      this.failUnresolvedCall(unresolved, `.${method.name}`, expression.span);
    const unresolvedRows = signature.rowParameters.filter(
      (parameter) => !rowSubstitutions.has(parameter),
    );
    if (unresolvedRows.length > 0)
      this.fail(
        "cannot-infer-type",
        `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`,
        expression.span,
      );
    const { providers, missing } = this.resolveCallProviders(
      signature.requirements,
      substitutions,
      rowSubstitutions,
      expression.span,
    );
    if (missing.length > 0)
      this.fail(
        "missing-requirement",
        `method '${method.name}' requires ${missing.map(displayType).join(", ")}`,
        expression.span,
      );
    const resultType = substituteGenericType(signature.result, substitutions, rowSubstitutions);
    const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
    const argumentParameterIndices = checkedArguments.parameterIndices
      ? [0, ...checkedArguments.parameterIndices.map((parameterIndex) => parameterIndex + 1)]
      : undefined;
    const callBase = {
      functionIndex: signature.index,
      functionName: signature.name,
      arguments: [methodReceiver, ...checkedArguments.arguments],
      argumentParameterIndices,
      ...defaultFields,
      bounds,
      providers,
      erasedParameterTypes:
        signature.genericParameters.length > 0 || signature.rowParameters.length > 0
          ? signature.parameters
          : undefined,
      erasedTypeSubstitutions: orderedTypeSubstitutions(signature.genericParameters, substitutions),
      erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
    };
    return method.suspending
      ? {
          kind: "suspend-construct",
          ...callBase,
          type: suspensionType(signature.index, resultType),
          span: expression.span,
        }
      : spelledCall(signature, {
          kind: "call",
          ...callBase,
          type: resultType,
          span: expression.span,
        });
  }

  protected checkConcreteArguments(
    written: Extract<Expression, { kind: "call" | "suspend-call" }>,
    inputTypes: readonly ValueType[],
    parameterNames: readonly string[],
    variadic: boolean,
    callable: string,
  ): CheckedArguments {
    const expression = this.spreadIntoInputs(written, inputTypes, parameterNames, variadic);
    // `*Args` takes the inputs tuple as one value of type `Args`.
    const parameterTypes = inputTypes.map((type) => inputsInner(type) ?? type);
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
            `readonly argument '${displayType(checked.type)}' cannot satisfy mutable parameter '${displayType(formal)}'`,
            source.span,
          );
        }
        return this.requireCoercion(checked, formal, source.span);
      }
      const nominal = nominalGenericParts(formal);
      const elementType = nominal?.name === "List" ? nominal.arguments[0]! : "void";
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

  /** Explain a rejected `mut self` call in terms of the source receiver. */
  protected failReadonlyMethodReceiver(
    methodName: string,
    source: Expression,
    receiver: HirExpression,
  ): never {
    const bindingName = source.kind === "name" ? source.name : undefined;
    const receiverType = displayType(genericTypeName(receiver.type) ?? receiver.type);
    const binding =
      bindingName !== undefined
        ? (this.resolveLocal(bindingName) ??
          this.availableCaptures.get(bindingName) ??
          this.resolveGlobal(bindingName))
        : undefined;
    if (bindingName !== undefined && binding && mutableInner(binding.type) === undefined) {
      this.fail(
        "mutable-receiver-required",
        `method '${methodName}' takes mut self, but binding '${bindingName}' has readonly type '${receiverType}'`,
        source.span,
      );
    }
    this.fail(
      "mutable-receiver-required",
      `method '${methodName}' takes mut self, but its receiver has readonly type '${receiverType}'`,
      source.span,
    );
  }

  /**
   * Takes the pending call generics as a filter that keeps a type only when it
   * mentions none of them (07-functions.md#generic-function-values).
   */
  protected takeSolvedPositions(): (type: ValueType | undefined) => ValueType | undefined {
    const pending = this.takePendingCallGenerics();
    return (type) => (type !== undefined && !pending(type) ? type : undefined);
  }

  /**
   * Whether an argument names a generic function or a generic single-payload
   * variant constructor. Such a value receives the partly solved formal
   * parameter type, so its type arguments are solved at the use site
   * (07-functions.md#generic-function-values).
   */
  private isGenericFunctionValue(source: Expression): boolean {
    // A method reference may take its generic parameters, such as a trait
    // reference's `Self`, from the expected type (07-functions.md#r-fn.ref.trait-self).
    if (source.kind === "qualified-name") return true;
    if (source.kind === "name") {
      if (source.typeArguments) return false;
      if (this.resolveLocal(source.name) || this.availableCaptures.has(source.name)) return false;
      if (this.resolveGlobal(source.name)) return false;
      const signature = this.signatures.get(source.name);
      return signature !== undefined && signature.genericParameters.length > 0;
    }
    if (source.kind === "member" && source.receiver.kind === "name") {
      const enumType = this.enumTypes.get(source.receiver.name);
      const variant = enumType?.variants.find((candidate) => candidate.name === source.name);
      return (
        enumType !== undefined &&
        variant?.fields.length === 1 &&
        (enumType.genericParameters.length > 0 || containsGenericType(variant.fields[0]!.type))
      );
    }
    return false;
  }

  /**
   * A call of a function with a tuple-typed or `Tuple`-bounded vararg passes
   * its trailing positional arguments as the tuple expression of them
   * (07-functions.md#r-fn.vararg.collect.tuple-expr): `call(g, 1, 2, xs...)`
   * passes `(1, 2, xs...)`. One spread alone passes its operand
   * (05-expressions.md#r-expr.call.spread.at-vararg).
   */
  protected collectTupleVararg<T extends Extract<Expression, { kind: "call" | "suspend-call" }>>(
    expression: T,
    signature: Signature,
  ): T {
    if (!signature.tupleVararg) return expression;
    const varargName = signature.parameterNames.at(-1);
    const names = expression.argumentNames ?? expression.arguments.map(() => undefined);
    if (names.includes(varargName)) return expression;
    const spreads = expression.argumentSpreads ?? expression.arguments.map(() => false);
    const fixedCount = signature.parameters.length - 1;
    const positional = names.flatMap((name, index) => (name === undefined ? [index] : []));
    const collected = positional.slice(fixedCount);
    if (collected.length === 1 && spreads[collected[0]!])
      return {
        ...expression,
        argumentSpreads: spreads.map((spread, index) => spread && index !== collected[0]),
      };
    const elements = collected.map((index) => expression.arguments[index]!);
    const spread = collected.length > 0 && spreads[collected.at(-1)!] === true;
    const span =
      elements.length > 0
        ? { start: elements[0]!.span.start, end: elements.at(-1)!.span.end }
        : expression.span;
    const tuple: Expression = { kind: "tuple", elements, ...(spread ? { spread } : {}), span };
    const kept = expression.arguments.flatMap((_, index) =>
      collected.includes(index) ? [] : [index],
    );
    const at =
      collected.length > 0 ? collected[0]! : positional.length > 0 ? positional.at(-1)! + 1 : 0;
    const order = [
      ...kept.filter((index) => index < at),
      -1,
      ...kept.filter((index) => index >= at),
    ];
    return {
      ...expression,
      arguments: order.map((index) => (index < 0 ? tuple : expression.arguments[index]!)),
      ...(expression.argumentNames
        ? { argumentNames: order.map((index) => (index < 0 ? undefined : names[index])) }
        : {}),
      argumentSpreads: order.map((index) => (index < 0 ? false : spreads[index]!)),
    };
  }

  /**
   * A positional spread before fixed parameters (05-expressions.md#r-expr.call.spread.inputs):
   * its operand is evaluated once and must have the same type as the tuple of
   * the remaining inputs, rest element included. Each fixed element becomes
   * one argument, and a rest element's list a spread at the vararg. A spread
   * into the one input `*Args` of `Fn[Args, O, $ R]` passes its operand whole.
   */
  protected spreadIntoInputs<T extends Extract<Expression, { kind: "call" | "suspend-call" }>>(
    expression: T,
    allTypes: readonly ValueType[],
    parameterNames: readonly string[],
    variadic: boolean,
  ): T {
    const spreads = expression.argumentSpreads;
    const at = spreads?.findIndex(Boolean) ?? -1;
    const names = expression.argumentNames ?? expression.arguments.map(() => undefined);
    if (at < 0 || names[at] !== undefined) return expression;
    // The parameters left for positional arguments, after the named ones.
    const free = allTypes.flatMap((_, index) =>
      names.includes(parameterNames[index]) ? [] : [index],
    );
    const slots = free.slice(names.slice(0, at).filter((name) => name === undefined).length);
    const last = allTypes.length - 1;
    if (slots.length === 0 || (variadic && slots[0] === last)) return expression;
    const parameterTypes = slots.map((index) => allTypes[index]!);
    const operandSource = expression.arguments[at]!;
    const inputs = parameterTypes.length === 1 ? inputsInner(parameterTypes[0]!) : undefined;
    const remaining =
      inputs ??
      tupleType(
        parameterTypes.map((type, index) =>
          variadic && slots[index] === last ? `${readonlyType(type)}...` : readonlyType(type),
        ),
      );
    const operand = this.checkExpression(operandSource);
    const actual = readonlyType(operand.type);
    if (nominalGenericParts(actual)?.name === "List")
      this.fail(
        "positional-spread-needs-vararg",
        `a List spread needs a vararg as the next positional parameter, which takes '${displayType(readonlyType(parameterTypes[0]!))}'`,
        operandSource.span,
      );
    const expectedTuple = tupleRest(remaining);
    const actualTuple = tupleRest(actual);
    const same =
      actual === remaining ||
      (actualTuple !== undefined &&
        expectedTuple !== undefined &&
        (containsGenericType(remaining)
          ? actualTuple.fixed.length === expectedTuple.fixed.length &&
            (actualTuple.rest === undefined) === (expectedTuple.rest === undefined)
          : tupleType(tupleParts(actual)!.map(readonlyType)) === remaining));
    if (!same)
      this.fail(
        "type-mismatch",
        `a spread before fixed parameters needs the tuple of the remaining inputs '${displayType(remaining)}', found '${displayType(operand.type)}'`,
        operandSource.span,
      );
    if (inputs !== undefined) {
      this.prechecked.set(operandSource, operand);
      return { ...expression, argumentSpreads: spreads!.map(() => false) };
    }
    const layout = tupleLayout(actual)!;
    const bindings = layout.map((type, index): HirLocal => ({
      name: `$spread${index}`,
      type: readonlyType(type),
      index: this.locals.length + index,
      mutable: false,
      parameter: false,
      span: operandSource.span,
    }));
    this.locals.push(...bindings);
    const span = operandSource.span;
    const bound: HirExpression = {
      kind: "binding-expression",
      bindings,
      value: operand,
      elementTypes: layout,
      type: operand.type,
      span,
    };
    const elements = bindings.map((local, index): Expression => {
      const placeholder: Expression = { kind: "name", name: local.name, span };
      this.prechecked.set(
        placeholder,
        index === 0
          ? {
              kind: "tuple-index",
              receiver: bound,
              index: 0,
              elementType: layout[0]!,
              type: local.type,
              span,
            }
          : { kind: "local", local, type: local.type, span },
      );
      this.readLocals.add(local);
      return placeholder;
    });
    const restSpread = actualTuple!.rest !== undefined;
    return {
      ...expression,
      arguments: [
        ...expression.arguments.slice(0, at),
        ...elements,
        ...expression.arguments.slice(at + 1),
      ],
      ...(expression.argumentNames
        ? {
            argumentNames: [
              ...names.slice(0, at),
              ...elements.map(() => undefined),
              ...names.slice(at + 1),
            ],
          }
        : {}),
      argumentSpreads: [
        ...spreads!.slice(0, at),
        ...elements.map((_, index) => restSpread && index === elements.length - 1),
        ...spreads!.slice(at + 1),
      ],
    };
  }

  protected checkSignatureArguments(
    written: Extract<Expression, { kind: "call" | "suspend-call" }>,
    signature: Signature,
    expected?: ValueType,
    callable = `function '${displayType(signature.name)}'`,
    initialSubstitutions: ReadonlyMap<string, ValueType> = new Map(),
  ): CheckedSignatureArguments {
    const expression = this.spreadIntoInputs(
      this.collectTupleVararg(written, signature),
      signature.parameters,
      signature.parameterNames,
      signature.variadic,
    );
    const substitutions = new Map(initialSubstitutions);
    const rowSubstitutions = new Map<string, readonly string[]>();
    // A row parameter that a function-type target matched, as `R` of
    // `impl[...] T for Fn[Args, O, $ R]`, is solved already.
    for (const parameter of signature.rowParameters) {
      const keys = rowArgumentKeys(initialSubstitutions.get(parameter));
      if (keys) rowSubstitutions.set(parameter, keys);
    }
    if (expression.typeArguments) {
      const slots = signature.typeArgumentOrder ?? signature.genericParameters;
      // A short list leaves its omitted trailing slots to inference and
      // defaults, as `_` does (types.generic.short-list); a long one is an
      // error (types.generic.too-long).
      if (expression.typeArguments.length > slots.length)
        this.fail(
          "argument-count",
          `${callable} expects ${slots.length} type arguments, received ${expression.typeArguments.length}`,
          expression.span,
        );
      expression.typeArguments.forEach((argument, index) => {
        if (argument.name === "_") return;
        const parameter = slots[index]!;
        const row = rowArgumentKeys(argument.name);
        // A row parameter takes a row written after `$`
        // (07-functions.md#r-fn.generic.explicit.row-dollar).
        if (signature.rowParameters.includes(parameter)) {
          if (!row)
            this.fail(
              "generic-kind-mismatch",
              `${callable} takes a requirement row for '${displayType(parameter)}', written after '$', as in '$ ${displayType(argument.name)}'`,
              argument.span,
            );
          rowSubstitutions.set(
            parameter,
            row.map((key) =>
              key.startsWith("row:") ? key : this.canonicalProviderKey(key, argument.span),
            ),
          );
          return;
        }
        if (row)
          this.fail(
            "generic-kind-mismatch",
            `${callable} takes a type, not the requirement row '${displayType(argument.name)}', for '${displayType(parameter)}'`,
            argument.span,
          );
        substitutions.set(parameter, this.resolveType(argument));
      });
    }
    const inferredBeforeExpected = new Set(substitutions.keys());
    // The own type of the first argument that solved each parameter; a
    // later argument of that parameter must have the same type, up to `mut`
    // (04-type-system.md#inference-from-several-arguments).
    const joined = new Map<string, ValueType>();
    if (expected) inferGenericType(signature.result, expected, substitutions, rowSubstitutions);
    const inferredFromExpected = new Set(
      [...substitutions.keys()].filter((name) => !inferredBeforeExpected.has(name)),
    );
    const defaultParameters = new Set(
      signature.defaultFunctionNames.flatMap((name, index) => (name ? [index] : [])),
    );
    const plan = this.planArguments(
      expression,
      signature.parameterNames,
      signature.variadic,
      callable,
      defaultParameters,
    );
    const checkEntry = (entry: PlannedArgument): HirExpression => {
      const formal = signature.parameters[entry.parameterIndex]!;
      if (entry.kind === "single") {
        const source = expression.arguments[entry.argumentIndices[0]!]!;
        this.inferTypesThroughBounds(signature, substitutions);
        const formalGeneric = genericTypeName(formal);
        const inferredFormal = substituteGenericType(formal, substitutions, rowSubstitutions);
        let checked: HirExpression;
        // A formal that mentions only the caller's own type parameters is a
        // known expected type; only the callee's unsolved ones leave it open.
        if (!mentionsUnsolved(inferredFormal, signature, substitutions, rowSubstitutions)) {
          checked = this.checkExpression(source, inferredFormal);
        } else if (source.kind === "closure" || this.isGenericFunctionValue(source)) {
          this.pendingCallGenerics = new Set(
            signature.genericParameters.filter((parameter) => !substitutions.has(parameter)),
          );
          try {
            checked = this.checkExpression(source, inferredFormal);
          } finally {
            this.pendingCallGenerics = undefined;
          }
        } else {
          checked = this.checkExpression(source);
        }
        const own = argumentOwnType(source, checked);
        if (formalGeneric && own !== undefined && !inferredBeforeExpected.has(formalGeneric)) {
          const earlier = joined.get(formalGeneric);
          const current = readonlyType(own);
          if (earlier === undefined) joined.set(formalGeneric, current);
          else if (earlier !== current)
            this.failArgumentJoin(signature.name, formalGeneric, earlier, current, source.span);
        }
        if (
          formalGeneric &&
          signature.genericBounds.some(
            (bound) => bound.parameter === formalGeneric && bound.mutable,
          ) &&
          mutableInner(checked.type) === undefined
        )
          this.fail(
            "unsatisfied-trait-bound",
            `readonly type '${displayType(checked.type)}' does not satisfy the mut bound on '${displayType(formalGeneric)}' of '${displayType(signature.name)}'`,
            source.span,
          );
        const boundedParameters = new Set(signature.genericBounds.map((bound) => bound.parameter));
        const inferredActual = weakenBoundedGenericActual(formal, checked.type, boundedParameters);
        let conflict = inferGenericType(formal, inferredActual, substitutions, rowSubstitutions);
        // An earlier `mut T` argument and this readonly `T` meet at `T`
        // (types.generic.infer.join.outer-permission); an expected type never
        // gives the `mut`.
        if (
          conflict &&
          formalGeneric &&
          !inferredFromExpected.has(formalGeneric) &&
          substitutions.get(formalGeneric) === mutableType(inferredActual)
        ) {
          substitutions.set(formalGeneric, inferredActual);
          conflict = undefined;
        }
        if (conflict) {
          // A readonly argument never infers a mutable type from the expected result.
          const fromArgument = new Map<string, ValueType>();
          inferGenericType(formal, inferredActual, fromArgument);
          const upgraded = [...fromArgument].find(
            ([name, type]) =>
              inferredFromExpected.has(name) && substitutions.get(name) === mutableType(type),
          );
          if (upgraded)
            this.fail(
              "mutable-upgrade",
              `readonly argument '${displayType(checked.type)}' cannot infer '${upgraded[0]}' as mutable '${displayType(substitutions.get(upgraded[0])!)}'`,
              source.span,
            );
          this.fail("type-mismatch", conflict, source.span);
        }
        const instantiatedFormal = substituteGenericType(formal, substitutions, rowSubstitutions);
        if (mutableInner(instantiatedFormal) === checked.type) {
          this.fail(
            "readonly-argument-to-mutable-parameter",
            `readonly argument '${displayType(checked.type)}' cannot satisfy mutable parameter '${displayType(instantiatedFormal)}'`,
            source.span,
          );
        }
        // A callback lacking a discharged key keeps its own type; the emitter
        // adapts it to the pattern (11-requirements-and-suspension.md#r-req.poly.absent-matches).
        if (lacksOnlyPatternKeys(formal, instantiatedFormal, checked.type)) return checked;
        return this.requireCoercion(checked, instantiatedFormal, source.span);
      }
      const nominal = nominalGenericParts(formal);
      const elementFormal = nominal?.name === "List" ? nominal.arguments[0]! : "void";
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
        if (conflict) this.fail("type-mismatch", conflict, source.span);
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
        type: nominalGenericType("List", [elementType]),
        span: expression.span,
      };
    };
    const arguments_ = checkLiteralArgumentsLast(
      plan,
      expression.arguments,
      signature,
      substitutions,
      (source) => this.isGenericFunctionValue(source),
      checkEntry,
    );
    const missingBound = this.inferTypesThroughBounds(signature, substitutions);
    if (missingBound) {
      const actual = substitutions.get(missingBound.parameter)!;
      this.fail(
        "unsatisfied-trait-bound",
        `type '${displayType(actual)}' does not implement ${displayType(missingBound.traitName)}, required by the bound on '${displayType(missingBound.parameter)}' of '${displayType(signature.name)}'`,
        expression.span,
      );
    }
    this.applyGenericDefaults(signature, substitutions, rowSubstitutions);
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

  private inferTypesThroughBounds(
    signature: Signature,
    substitutions: Map<string, ValueType>,
  ): HirGenericBound | undefined {
    return inferTypesThroughBounds(
      signature,
      substitutions,
      this.signature.genericBounds,
      this.implementations,
      this.traitTypes,
      (traitIndex, target, traitArguments) =>
        this.builtinTraitDictionaryPlan(traitIndex, target, traitArguments, signature.span) !==
        undefined,
    );
  }

  /** `v()` on a callable value, through `Apply`; see `operator-calls.ts`. */
  protected abstract applyCall(
    expression: Extract<Expression, { kind: "call" }>,
    callee: HirExpression,
    expected: ValueType | undefined,
  ): HirExpression;

  /**
   * A later argument of a type parameter that an earlier one solved as
   * `solved`, as `assert_equal`'s `expected` after `actual`: it converts to
   * `solved` only by permission weakening (types.generic.infer.join).
   */
  protected checkJoinedArgument(
    name: string,
    source: Expression,
    solved: ValueType,
  ): HirExpression {
    const checked = this.checkExpression(source, solved);
    const own = argumentOwnType(source, checked);
    if (own !== undefined && readonlyType(own) !== readonlyType(solved))
      this.failArgumentJoin(name, "T", readonlyType(solved), readonlyType(own), source.span);
    return this.requireCoercion(checked, solved, source.span);
  }

  /**
   * Two arguments that solve one type parameter have different types. The
   * join converts only `mut X` to `X`: never by numeric widening, and never
   * to a trait value (types.num.no-implicit, types.generic.infer.join.no-trait-value).
   */
  protected failArgumentJoin(
    name: string,
    parameter: string,
    earlier: ValueType,
    current: ValueType,
    span: SourceSpan,
  ): never {
    if (traitTypeName(earlier) !== undefined || traitTypeName(current) !== undefined)
      this.fail(
        "no-common-type",
        `arguments of types '${displayType(earlier)}' and '${displayType(current)}' both solve '${displayType(parameter)}' of '${name}', and inference never converts to a trait value; write '${name}::[${displayType(traitTypeName(earlier) ?? traitTypeName(current)!)}](...)'`,
        span,
      );
    this.fail(
      "type-mismatch",
      `arguments of types '${displayType(earlier)}' and '${displayType(current)}' both solve '${displayType(parameter)}' of '${name}', and inference never widens a number; convert one argument to the other's type`,
      span,
    );
  }

  /**
   * Gives each parameter that inference left unsolved its default, in
   * declaration order with the earlier solutions substituted
   * (types.generic.default.after-inference, types.generic.default.fill).
   */
  protected applyGenericDefaults(
    signature: Pick<Signature, "genericDefaults" | "rowParameters">,
    substitutions: Map<string, ValueType>,
    rowSubstitutions: Map<string, readonly string[]>,
  ): void {
    for (const [parameter, fallback] of signature.genericDefaults ?? []) {
      if (signature.rowParameters.includes(parameter)) {
        if (!rowSubstitutions.has(parameter))
          rowSubstitutions.set(parameter, rowArgumentKeys(fallback) ?? [fallback]);
      } else if (!substitutions.has(parameter))
        substitutions.set(
          parameter,
          substituteGenericType(fallback, substitutions, rowSubstitutions),
        );
    }
  }

  protected checkProviderEntries(
    entries: Extract<Expression, { kind: "provider-context" | "provider-with" }>["entries"],
  ): CheckedProviderEntries {
    const checked: HirProviderContextEntry[] = [];
    const providers = new Map<string, HirLocal>();
    for (const entry of entries) {
      if (entry.kind === "binding") {
        const key = this.canonicalProviderKey(entry.key, entry.span);
        this.rejectProviderKeyCollision(key, providers.keys(), entry.span);
        const trait = this.traitTypes.get(traitKeyName(key));
        // A mutable requirement trait needs a `mut` value (req.mut.install-mutable).
        const providerType = trait ? this.providerValueType(key) : undefined;
        let value = this.checkExpression(entry.value, providerType);
        if (
          providerType &&
          mutableInner(providerType) !== undefined &&
          mutableInner(value.type) === undefined
        )
          this.fail(
            "mutable-upgrade",
            `readonly value '${displayType(value.type)}' cannot provide the mutable requirement trait '${displayType(key)}'`,
            entry.value.span,
          );
        if (providerType) value = this.requireCoercion(value, providerType, entry.value.span);
        else if (!value.type.startsWith("provider:")) {
          this.fail(
            "provider-type-mismatch",
            `provider binding '${displayType(key)}' requires an opaque provider value`,
            entry.value.span,
          );
        }
        const local = this.addProviderLocal(key, entry.span);
        checked.push({ kind: "binding", key, local, value });
        providers.set(key, local);
        continue;
      }
      const value = this.checkExpression(entry.value);
      const keys = contextKeys(value.type);
      if (!keys)
        this.fail(
          "type-mismatch",
          `context spread requires a $.Context value, found '${displayType(value.type)}'`,
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

  /**
   * The provider value type for a key. A mutable requirement trait, one that
   * declares or inherits a `mut self` method, is always provided with mutable
   * access (11-requirements-and-suspension.md#mutable-providers).
   */
  protected providerValueType(key: string): ValueType {
    if (!this.traitTypes.has(traitKeyName(key))) return `provider:${key}`;
    return this.isMutableRequirementTrait(traitKeyName(key))
      ? mutableType(`trait:${key}`)
      : `trait:${key}`;
  }

  private isMutableRequirementTrait(name: string, seen = new Set<string>()): boolean {
    if (seen.has(name)) return false;
    seen.add(name);
    const trait = this.traitTypes.get(name);
    if (!trait) return false;
    return (
      trait.methods.some((method) => method.receiverMutable) ||
      trait.supertraits.some((supertrait) =>
        this.isMutableRequirementTrait(supertrait.traitName, seen),
      )
    );
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
        `provider keys '${displayType(collision)}' and '${displayType(key)}' can become identical after generic substitution`,
        span,
      );
    }
  }

  protected canonicalProviderKey(key: string, span: SourceSpan): string {
    const resolved = resolveRequirementKeyTypes(
      resolveGenericType(
        key,
        new Set(this.signature.genericParameters),
        new Set(this.signature.rowParameters),
      ),
      (type) => resolveTraitType(type, this.traitTypes),
    );
    const diagnostic = requirementKeyDiagnostics([resolved], this.traitTypes, span, (type) =>
      isKnownType(
        resolveTraitType(type, this.traitTypes),
        this.dataTypes,
        this.enumTypes,
        this.traitTypes,
      ),
    )[0];
    if (diagnostic) this.fail(diagnostic.code, diagnostic.message, span);
    return resolved;
  }

  /** `cannot-infer-type` for a generic parameter inference left unsolved. */
  protected failUninferredGenericParameter(parameter: string, span: SourceSpan): never {
    this.fail(
      "cannot-infer-type",
      `could not infer generic parameter ${displayType(parameter)}`,
      span,
    );
  }

  protected resolveBoundDictionaries(
    signature: Signature,
    substitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan,
  ): HirExpression[] {
    for (const parameter of signature.referenceParameters ?? []) {
      const actual = substitutions.get(parameter);
      if (!actual) this.failUninferredGenericParameter(parameter, span);
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        if (!(this.signature.referenceParameters ?? []).includes(forwarded)) {
          this.fail(
            "unsatisfied-trait-bound",
            `generic parameter '${displayType(forwarded)}' does not implement AnyRef, required by the bound on '${displayType(parameter)}' of '${displayType(signature.name)}'`,
            span,
          );
        }
      } else if (!this.isIdentityType(actual)) {
        this.fail(
          "unsatisfied-trait-bound",
          `type '${displayType(actual)}' does not implement AnyRef, required by the bound on '${displayType(parameter)}' of '${displayType(signature.name)}'`,
          span,
        );
      }
    }
    // AnyVal and AnyRef partition the value types
    // (04-type-system.md#trait-values-and-any).
    for (const parameter of signature.valueParameters ?? []) {
      const actual = substitutions.get(parameter);
      if (!actual) this.failUninferredGenericParameter(parameter, span);
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        if (!(this.signature.valueParameters ?? []).includes(forwarded)) {
          this.fail(
            "unsatisfied-trait-bound",
            `generic parameter '${displayType(forwarded)}' does not implement AnyVal, required by the bound on '${displayType(parameter)}' of '${displayType(signature.name)}'`,
            span,
          );
        }
      } else if (this.isIdentityType(actual)) {
        this.fail(
          "unsatisfied-trait-bound",
          `type '${displayType(actual)}' does not implement AnyVal, required by the bound on '${displayType(parameter)}' of '${displayType(signature.name)}'`,
          span,
        );
      }
    }
    // Every tuple type implements the sealed `Tuple` (fn.type.ctor.tuple-trait).
    for (const parameter of signature.tupleParameters ?? []) {
      const actual = substitutions.get(parameter);
      const forwarded = actual && genericTypeName(actual);
      if (
        actual &&
        (forwarded
          ? !(this.signature.tupleParameters ?? []).includes(forwarded)
          : tupleParts(readonlyType(actual)) === undefined)
      )
        this.fail(
          "unsatisfied-trait-bound",
          `type '${displayType(actual)}' does not implement Tuple, required by the bound on '${displayType(parameter)}' of '${displayType(signature.name)}'`,
          span,
        );
    }
    // An unmet bound of an intrinsically derived implementation's method
    // (spec/lang/09-traits.md#r-trait.derive.bound-unmet).
    const boundCode = DERIVED_IMPLEMENTATION_SPANS.has(signature.span)
      ? "missing-derived-bound"
      : "unsatisfied-trait-bound";
    return signature.genericBounds.map((bound) => {
      const actual = substitutions.get(bound.parameter);
      if (!actual) this.failUninferredGenericParameter(bound.parameter, span);
      const traitArguments = bound.traitArguments.map((argument) =>
        substituteGenericType(argument, substitutions),
      );
      const traitKey =
        traitArguments.length > 0
          ? nominalGenericType(bound.traitName, traitArguments)
          : bound.traitName;
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        const boundIndex = this.signature.genericBounds.findIndex(
          (candidate) =>
            candidate.parameter === forwarded &&
            candidate.traitIndex === bound.traitIndex &&
            candidate.traitArguments.length === traitArguments.length &&
            candidate.traitArguments.every((argument, index) => argument === traitArguments[index]),
        );
        if (boundIndex < 0) {
          this.fail(
            boundCode,
            `generic parameter '${displayType(forwarded)}' does not implement ${displayType(bound.traitName)}, required by the bound on '${displayType(bound.parameter)}' of '${displayType(signature.name)}'`,
            span,
          );
        }
        return {
          kind: "trait-bound-dictionary",
          traitIndex: bound.traitIndex,
          boundIndex,
          type: `trait:${traitKey}`,
          span,
        };
      }
      // A parameter instantiated with `mut U` uses the implementation for
      // `U`; only the Inspectable evidence records the inner `mut`.
      const implementationType = readonlyType(actual);
      const implementation = this.implementations.find((candidate) =>
        Boolean(
          matchTraitImplementation(candidate, bound.traitIndex, implementationType, traitArguments),
        ),
      );
      if (!implementation) {
        const builtin =
          this.builtinTraitDictionaryPlan(bound.traitIndex, actual, traitArguments, span) ??
          forwardingPlan(this.traitTypes, actual, bound.traitIndex, traitArguments);
        if (builtin)
          return {
            kind: "trait-dictionary",
            traitIndex: bound.traitIndex,
            dictionary: builtin,
            type: `trait:${traitKey}`,
            span,
          };
        this.fail(
          boundCode,
          `type '${displayType(actual)}' does not implement ${displayType(bound.traitName)}, required by the bound on '${displayType(bound.parameter)}' of '${displayType(signature.name)}'`,
          span,
        );
      }
      return {
        kind: "trait-dictionary",
        traitIndex: bound.traitIndex,
        dictionary: this.traitDictionaryPlan(
          implementation,
          implementationType,
          traitArguments,
          span,
        ),
        type: `trait:${traitKey}`,
        span,
      };
    });
  }

  protected resolveAssociatedTypeSubstitutions(
    signature: Signature,
    sourceSubstitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan = signature.span,
  ): Map<string, ValueType> {
    const substitutions = new Map(sourceSubstitutions);
    for (const bound of signature.genericBounds) {
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === bound.traitIndex,
      );
      if (!trait) continue;
      // The trait's own associated types, and a supertrait's that a binding
      // names (09-traits.md#r-trait.binding.name-reach.meaning).
      const names = [
        ...new Set([
          ...trait.associatedTypes.map((associated) => associated.name),
          ...(bound.associatedBindings ?? []).map((binding) => binding.name),
        ]),
      ];
      if (names.length === 0) continue;
      const actual = substitutions.get(bound.parameter);
      if (!actual) continue;
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        for (const name of names) {
          const projection = normalizeBoundProjections(
            `generic:${forwarded}::${name}`,
            this.signature.genericBounds,
          );
          substitutions.set(`${bound.parameter}::${name}`, projection);
          this.bindAssociatedType(signature, bound, name, projection, substitutions, span);
        }
        continue;
      }
      const traitArguments = bound.traitArguments.map((argument) =>
        substituteGenericType(argument, substitutions),
      );
      // A trait value type's bindings answer its projections
      // (09-traits.md#r-trait.dyn.bound.projection).
      for (const name of names) {
        const resolved = this.implementationAssociatedType(actual, trait, traitArguments, name);
        if (resolved === undefined) continue;
        const key = `${bound.parameter}::${name}`;
        const inferred = substitutions.get(key);
        // A `mut X` projection weakens to an `X` that an expected type
        // inferred (04-type-system.md#r-types.mut.weaken).
        if (inferred !== undefined && inferred !== resolved && mutableInner(resolved) !== inferred)
          this.fail(
            "associated-type-mismatch",
            `projection '${bound.parameter}::${name}' resolves to '${displayType(resolved)}', not '${displayType(inferred)}'`,
            signature.span,
          );
        substitutions.set(key, resolved);
        this.bindAssociatedType(signature, bound, name, resolved, substitutions, span);
      }
    }
    return substitutions;
  }

  /** Applies a `Name = type` binding: infers a free parameter or checks equality. */
  private bindAssociatedType(
    signature: Signature,
    bound: HirGenericBound,
    name: string,
    resolved: ValueType,
    substitutions: Map<string, ValueType>,
    span: SourceSpan,
  ): void {
    const binding = bound.associatedBindings?.find((candidate) => candidate.name === name);
    if (!binding) return;
    const expected = substituteGenericType(binding.type, substitutions);
    const free = genericTypeName(expected);
    if (free && signature.genericParameters.includes(free) && !substitutions.has(free)) {
      substitutions.set(free, resolved);
      return;
    }
    if (expected !== resolved)
      this.fail(
        "unsatisfied-trait-bound",
        `'${bound.parameter}::${name}' is '${displayType(resolved)}', but the bound on '${bound.parameter}' of '${signature.name}' requires '${displayType(expected)}'`,
        span,
      );
  }
}
