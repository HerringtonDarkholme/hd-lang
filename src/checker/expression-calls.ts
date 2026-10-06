import { arityCode } from "../diagnostics.ts";
import { traitValueBindings } from "./associated-bindings.ts";
import type { Expression } from "../ast.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import { CheckFailure, type Signature } from "./context.ts";
import { CHECK_EQUAL } from "./standard-library.ts";
import { ENTRY_ERROR_REPORT, STD_ENTRY_REPORT } from "./entry-error.ts";
import { numericType } from "../numeric.ts";
import type { SourceSpan } from "../diagnostics.ts";
import {
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
  mutableOrPrimitive,
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  suspensionType,
  traitSuspensionParts,
  traitSuspensionType,
  tupleType,
  CURSOR_TYPE,
  splitTypeBindings,
  displayType,
} from "../types.ts";
import {
  containsGenericType,
  defaultCallFields,
  genericTypeName,
  matchImplementationTarget,
  matchTraitImplementation,
  orderedTypeSubstitutions,
  resultMisfit,
  substituteGenericType,
  traitTypeName,
} from "./shared.ts";
import { implementationsFor } from "./implementation-index.ts";
import type { QualifiedCallExpression } from "./trait-calls.ts";
import { IterationChecker } from "./iteration.ts";
import { loopNameHint } from "./cannot-infer.ts";
import { supertraitPathBindings } from "./trait-paths.ts";
import { keepLiteralDefaults, speculate } from "./call-speculation.ts";
import { isDowncastValImport } from "./inspectable.ts";
import { checkLiteralSuffixCall, checkStringPrefixCall } from "./literal-suffixes.ts";
import { TYPE_ID } from "./standard-traits.ts";
import { STRUCTURE_AS_DECLARED, STRUCTURE_MISMATCH } from "./typed-derivation.ts";
import { BUILT_IN_METHODS } from "./built-in-methods.ts";
type CallExpression = Extract<Expression, { kind: "call" }>;
export interface MemberCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "member" }>;
}
interface NamedCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "name" }>;
}

export abstract class ExpressionCallChecker extends IterationChecker {
  protected checkCallExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "call":
        return this.checkCall(expression, expected);
      default:
        return undefined;
    }
  }

  private checkCall(expression: CallExpression, expected?: ValueType): HirExpression {
    if (expression.callee.kind === "contextual-variant") {
      if (expected && optionalInner(expected) !== undefined)
        return this.checkOptionVariant(
          expression.callee.name,
          expression,
          expected,
          expression.span,
          false,
        );
      if (expected && resultParts(expected))
        return this.checkResultVariant(expression.callee.name, expression, expected);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "enum constructors have no variadic parameter",
          expression.span,
        );
      const nominal = expected ? nominalGenericParts(expected) : undefined;
      const declaration = expected && this.enumTypes.get(nominal?.name ?? expected);
      if (!declaration) {
        this.fail(
          "missing-contextual-enum-type",
          `variant '.${expression.callee.name}' requires an expected enum type`,
          expression.span,
        );
      }
      return this.checkEnumConstructor(declaration, expression.callee.name, expression, expected);
    }
    if (expression.callee.kind === "member" && !expression.callee.parenthesized) {
      return this.checkMemberCall(expression as MemberCallExpression, expected);
    }
    if (expression.callee.kind === "qualified-name") {
      return this.checkQualifiedCall(expression as QualifiedCallExpression, expected);
    }
    // A suffixed literal or prefixed string calls a function found in module
    // scope only (03-names-and-scopes.md#r-names.literal-fn.no-local).
    if (
      (expression.literalSuffix || expression.stringPrefix) &&
      expression.callee.kind === "name"
    ) {
      const name = expression.callee.name;
      return (expression.literalSuffix ? checkLiteralSuffixCall : checkStringPrefixCall)(
        name,
        this.visibleSignature(name),
        this.globals.has(name) || this.dataTypes.has(name) || this.enumTypes.has(name),
        (code, message) => this.fail(code, message, expression.span),
        () => this.checkDeclaredCall(expression as NamedCallExpression, expected),
      );
    }
    if (
      expression.callee.kind !== "name" ||
      this.resolveLocal(expression.callee.name) ||
      this.availableCaptures.has(expression.callee.name) ||
      this.resolveGlobal(expression.callee.name)
    ) {
      return this.checkFunctionValueCall(expression, expected);
    }
    const namedExpression = expression as NamedCallExpression;
    if (expression.typedFact) return this.checkTypedFactCall(namedExpression, expected);
    const intrinsic =
      this.checkNamedIntrinsicCall(namedExpression, expected) ??
      (expression.kind === "call" ? this.checkNewtypeCall(namedExpression, expected) : undefined);
    if (intrinsic) return intrinsic;
    return this.checkDeclaredCall(namedExpression, expected);
  }

  private checkMemberCall(expression: MemberCallExpression, expected?: ValueType): HirExpression {
    if (
      expression.callee.receiver.kind === "name" &&
      this.namesResultEnum(expression.callee.receiver.name)
    )
      return this.checkResultVariant(expression.callee.name, expression, expected);
    if (
      expression.callee.receiver.kind === "name" &&
      this.namesOptionEnum(expression.callee.receiver.name)
    )
      return this.checkOptionVariant(
        expression.callee.name,
        expression,
        expected,
        expression.span,
        true,
      );
    if (expression.callee.receiver.kind === "name") {
      const declaration = this.enumTypes.get(expression.callee.receiver.name);
      if (declaration) {
        if (expression.argumentSpreads?.some(Boolean))
          this.fail(
            "positional-spread-needs-vararg",
            "enum constructors have no variadic parameter",
            expression.span,
          );
        return this.checkEnumConstructor(declaration, expression.callee.name, expression, expected);
      }
    }
    const moduleReceiver = expression.callee.receiver;
    const standard =
      moduleReceiver.kind === "name" &&
      this.standardSubmoduleFunction(moduleReceiver.name, expression.callee.name, expression.span);
    if (standard)
      return this.checkCall(
        { ...expression, callee: { kind: "name", name: standard, span: expression.callee.span } },
        expected,
      );
    const receiver = this.checkExpression(expression.callee.receiver);
    const inspection = this.checkInspectMemberCall(expression, receiver, expected);
    if (inspection) return inspection;
    const builtin = this.checkBuiltInMemberCall(expression, receiver);
    if (builtin) return builtin;
    const dynamic = this.checkDynamicMemberCall(expression, receiver);
    if (dynamic) return dynamic;
    return this.withHandleWitness(expression, receiver, (call) =>
      this.checkImplementedMemberCall(call, receiver, expected),
    );
  }

  private checkBuiltInMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): HirExpression | undefined {
    const methodName = expression.callee.name;
    const suspension = suspensionParts(receiver.type);
    const traitSuspension = traitSuspensionParts(receiver.type);
    const storedSuspension = storedSuspensionParts(receiver.type);
    if ((suspension || traitSuspension || storedSuspension) && methodName === "cancel") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "Suspend.cancel expects no arguments", expression.span);
      if (storedSuspension && !storedSuspension.mutable) {
        this.fail(
          "mutable-receiver-required",
          "cancelling a stored suspension requires mut Suspend[T]",
          expression.callee.receiver.span,
        );
      }
      this.requireDrivableSuspension(expression.callee.receiver);
      if (storedSuspension) {
        return {
          kind: "suspension-cancel",
          suspension: receiver,
          type: "void",
          span: expression.span,
        };
      }
      return suspension
        ? {
            kind: "suspend-cancel",
            functionIndex: suspension.functionIndex,
            suspension: receiver,
            type: "void",
            span: expression.span,
          }
        : {
            kind: "trait-suspend-cancel",
            traitIndex: traitSuspension!.traitIndex,
            methodIndex: traitSuspension!.methodIndex,
            suspension: receiver,
            type: "void",
            span: expression.span,
          };
    }
    const receiverNominal = nominalGenericParts(readonlyType(receiver.type));
    const { List, Map } = BUILT_IN_METHODS;
    if (receiverNominal?.name === "List" && methodName === List.length) {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "list.len expects no arguments", expression.span);
      return { kind: "list-length", receiver, type: "usize", span: expression.span };
    }
    if (receiverNominal?.name === "List" && methodName === List.iterator) {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "list.iter expects no arguments", expression.span);
      const elementType = receiverNominal.arguments[0]!;
      return this.collectionIterator(
        {
          kind: "list-iterator",
          receiver,
          elementType,
          type: this.cursorType(elementType),
          span: expression.span,
        },
        elementType,
      );
    }
    if (receiverNominal?.name === CURSOR_TYPE && expression.callee.name === "next") {
      const elementType = receiverNominal.arguments[0]!;
      return {
        kind: "iterator-next",
        receiver,
        elementType,
        type: optionalType(elementType),
        span: expression.span,
      };
    }
    if (receiverNominal?.name === "List" && methodName === List.push) {
      if (mutableInner(receiver.type) === undefined)
        this.failReadonlyMethodReceiver("push", expression.callee.receiver, receiver);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "list.push has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length !== 1)
        this.fail("argument-count", "list.push expects one value", expression.span);
      this.resolveArgumentMapping(expression, ["value"], "list.push");
      const elementType = receiverNominal.arguments[0]!;
      const value = this.requireCoercion(
        this.checkExpression(expression.arguments[0]!, elementType),
        elementType,
        expression.arguments[0]!.span,
      );
      return {
        kind: "list-push",
        receiver,
        value,
        elementType,
        type: "void",
        span: expression.span,
      };
    }
    if (receiverNominal?.name === "Map" && methodName === Map.length) {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "map.len expects no arguments", expression.span);
      return { kind: "map-length", receiver, type: "usize", span: expression.span };
    }
    if (receiverNominal?.name === "Map" && methodName === Map.iterator) {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "map.iter expects no arguments", expression.span);
      const elementType = tupleType(receiverNominal.arguments);
      return this.collectionIterator(
        {
          kind: "map-iterator",
          receiver,
          elementType,
          type: this.cursorType(elementType),
          span: expression.span,
        },
        elementType,
      );
    }
    if (receiverNominal?.name === "Map" && (methodName === Map.get || methodName === Map.remove)) {
      const removing = methodName === Map.remove;
      if (removing && mutableInner(receiver.type) === undefined) {
        this.fail(
          "mutable-receiver-required",
          "map.remove requires mutable map access",
          expression.callee.receiver.span,
        );
      }
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          `map.${expression.callee.name} has no variadic parameter`,
          expression.span,
        );
      if (expression.arguments.length !== 1)
        this.fail(
          "argument-count",
          `map.${expression.callee.name} expects one key`,
          expression.span,
        );
      this.resolveArgumentMapping(expression, ["key"], `map.${expression.callee.name}`);
      const keyType = receiverNominal.arguments[0]!;
      const valueType = receiverNominal.arguments[1]!;
      const key = this.requireCoercion(
        this.checkExpression(expression.arguments[0]!, keyType),
        keyType,
        expression.arguments[0]!.span,
      );
      return {
        kind: removing ? "map-remove" : "map-index",
        receiver,
        key,
        keyType,
        valueType,
        type: optionalType(valueType),
        span: expression.span,
      };
    }
    return undefined;
  }

  protected checkDynamicMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): HirExpression | undefined {
    const methodName = expression.callee.name;
    const receiverGeneric = genericTypeName(readonlyType(receiver.type));
    const receiverBounds = receiverGeneric
      ? this.signature.genericBounds
          .map((bound, boundIndex) => ({
            bound,
            boundIndex,
            trait: this.traitTypes.get(bound.traitName)!,
          }))
          .filter(({ bound }) => bound.parameter === receiverGeneric)
      : [];
    const matchingBounds = receiverBounds.filter(
      ({ trait }) => this.findTraitMethods(trait, methodName).length > 0,
    );
    if (matchingBounds.length > 1) {
      this.fail(
        "ambiguous-method",
        `method '${methodName}' is supplied by multiple bounds on ${receiverGeneric}`,
        expression.callee.span,
      );
    }
    const receiverBound = matchingBounds[0];
    const receiverBoundTrait = receiverBound
      ? receiverBound.bound.traitArguments.length > 0
        ? nominalGenericType(receiverBound.bound.traitName, receiverBound.bound.traitArguments)
        : receiverBound.bound.traitName
      : undefined;
    const dispatchReceiver: HirExpression = receiverBound
      ? {
          kind: "trait-bound",
          value: receiver,
          traitIndex: receiverBound.bound.traitIndex,
          boundIndex: receiverBound.boundIndex,
          type:
            receiverBound.bound.mutable || this.hasMutableAccess(receiver.type)
              ? mutableType(`trait:${receiverBoundTrait}`)
              : `trait:${receiverBoundTrait}`,
          span: receiver.span,
        }
      : receiver;
    const dynamicTraitName = traitTypeName(dispatchReceiver.type);
    const dynamicTrait = dynamicTraitName && this.traitTypes.get(dynamicTraitName);
    if (dynamicTrait) {
      const methodCandidates = this.findTraitMethods(dynamicTrait, methodName);
      if (methodCandidates.length > 1)
        this.fail(
          "ambiguous-method",
          `method '${methodName}' is inherited through multiple supertraits of '${dynamicTrait.name}'`,
          expression.callee.span,
        );
      const selectedMethod = methodCandidates[0];
      if (!selectedMethod)
        this.fail(
          "unknown-method",
          `trait '${dynamicTrait.name}' has no method '${expression.callee.name}'`,
          expression.callee.span,
        );
      const { method } = selectedMethod;
      if (method.receiverMutable && mutableInner(dispatchReceiver.type) === undefined) {
        this.failReadonlyMethodReceiver(method.name, expression.callee.receiver, receiver);
      }
      const traitKey = readonlyType(dispatchReceiver.type).slice("trait:".length);
      const traitArguments = splitTypeBindings(
        nominalGenericParts(traitKey)?.arguments ?? [],
      ).positional;
      // Through a trait value that binds associated types, each projection
      // in a signature is its bound type (09-traits.md#r-trait.dyn.binding.signatures).
      const valueBindings = receiverBound
        ? new Map<string, ValueType>()
        : traitValueBindings(dispatchReceiver.type);
      const selfSubstitution = new Map(
        receiverBound && receiverGeneric ? [["Self", `generic:${receiverGeneric}`] as const] : [],
      );
      const selectedTraitArguments = this.resolveTraitPath(
        dynamicTrait,
        traitArguments,
        selectedMethod.path,
      ).arguments.map((argument) => substituteGenericType(argument, selfSubstitution));
      // A supertrait list may bind the selected trait's associated types
      // (09-traits.md#r-trait.binding.super.projection).
      const pathBindings = supertraitPathBindings(
        this.traitTypes,
        dynamicTrait,
        traitArguments,
        selectedMethod.path,
      ).map((binding) => ({
        ...binding,
        type: substituteGenericType(binding.type, selfSubstitution),
      }));
      const traitSubstitutions = new Map(
        selectedMethod.trait.genericParameters.map(
          (parameter, index) => [parameter, selectedTraitArguments[index]!] as const,
        ),
      );
      if (receiverBound && receiverGeneric) {
        traitSubstitutions.set("Self", `generic:${receiverGeneric}`);
        selectedMethod.trait.associatedTypes.forEach((associated) =>
          traitSubstitutions.set(
            `Self::${associated.name}`,
            (selectedMethod.path.length > 0 ? pathBindings : []).find(
              (binding) => binding.name === associated.name,
            )?.type ??
              // A bound's binding may name a supertrait's associated type
              // (09-traits.md#r-trait.binding.name-reach.meaning).
              (receiverBound.bound.associatedBindings ?? []).find(
                (binding) => binding.name === associated.name,
              )?.type ??
              `generic:${receiverGeneric}::${associated.name}`,
          ),
        );
      } else if (valueBindings.size > 0)
        selectedMethod.trait.associatedTypes.forEach((associated) => {
          const bound =
            pathBindings.find((binding) => binding.name === associated.name)?.type ??
            valueBindings.get(associated.name);
          if (bound !== undefined) traitSubstitutions.set(`Self::${associated.name}`, bound);
        });
      const methodParameters = method.parameters.map((parameter) =>
        substituteGenericType(parameter, traitSubstitutions),
      );
      let methodResult = substituteGenericType(method.result, traitSubstitutions);
      const methodRequirements = method.requirements.map((requirement) =>
        substituteGenericType(requirement, traitSubstitutions),
      );
      let checkedArguments: {
        readonly arguments: readonly HirExpression[];
        readonly parameterIndices?: readonly number[];
      };
      let bounds: HirExpression[] | undefined;
      const erasedSubstitutions = new Map(traitSubstitutions);
      if (method.genericParameters.length > 0) {
        // Method-level generics are inferred per call; their bounds other than
        // AnyVal and AnyRef travel as dictionary arguments, also through a trait value.
        const methodSignature: Signature = {
          name: method.name,
          index: -1,
          suspending: method.suspending,
          genericParameters: method.genericParameters,
          genericBounds: (method.genericBounds ?? []).map((bound) => ({
            ...bound,
            traitArguments: bound.traitArguments.map((argument) =>
              substituteGenericType(argument, traitSubstitutions),
            ),
          })),
          referenceParameters: method.referenceParameters,
          valueParameters: method.valueParameters,
          mutableParameters: method.mutableParameters,
          rowParameters: [],
          ...(method.genericDefaults
            ? {
                genericDefaults: new Map(
                  [...method.genericDefaults].map(([name, type]) => [
                    name,
                    substituteGenericType(type, traitSubstitutions),
                  ]),
                ),
              }
            : {}),
          parameters: methodParameters,
          parameterNames: method.parameterNames,
          defaultFunctionNames: method.parameters.map(() => undefined),
          variadic: method.variadic,
          result: methodResult,
          requirements: methodRequirements,
          span: method.span,
        };
        const checkedSignature = this.checkSignatureArguments(
          expression,
          methodSignature,
          undefined,
          `method '${method.name}'`,
        );
        const resolved = this.resolveAssociatedTypeSubstitutions(
          methodSignature,
          checkedSignature.substitutions,
          expression.span,
        );
        const unresolved = method.genericParameters.filter((parameter) => !resolved.has(parameter));
        if (unresolved.length > 0)
          this.failUnresolvedCall(unresolved, `.${method.name}`, expression.span, resolved);
        methodResult = substituteGenericType(methodResult, resolved);
        for (const [parameter, type] of resolved) erasedSubstitutions.set(parameter, type);
        bounds = this.resolveBoundDictionaries(
          methodSignature,
          checkedSignature.substitutions,
          expression.span,
        );
        checkedArguments = checkedSignature;
      } else {
        checkedArguments = this.checkConcreteArguments(
          expression,
          methodParameters,
          method.parameterNames,
          method.variadic,
          `method '${method.name}'`,
        );
      }
      const providers = methodRequirements.map((requirement) =>
        this.resolveProvider(requirement, expression.span),
      );
      const missing = methodRequirements.filter((_, index) => !providers[index]);
      if (missing.length > 0)
        this.fail(
          "missing-requirement",
          `method '${method.name}' requires ${missing.map(displayType).join(", ")}`,
          expression.span,
        );
      return method.suspending
        ? {
            kind: "trait-suspend-construct",
            receiver: dispatchReceiver,
            traitIndex: selectedMethod.trait.index,
            methodIndex: method.index,
            supertraitPath: selectedMethod.path.length > 0 ? selectedMethod.path : undefined,
            arguments: checkedArguments.arguments,
            argumentParameterIndices: checkedArguments.parameterIndices,
            bounds,
            providers: providers as HirExpression[],
            erasedParameterTypes: method.parameters.some(containsGenericType)
              ? method.parameters
              : undefined,
            erasedResultType: containsGenericType(method.result) ? method.result : undefined,
            erasedTypeSubstitutions: orderedTypeSubstitutions(
              [...erasedSubstitutions.keys()],
              erasedSubstitutions,
            ),
            type: traitSuspensionType(selectedMethod.trait.index, method.index, methodResult),
            span: expression.span,
          }
        : {
            kind: "trait-call",
            receiver: dispatchReceiver,
            traitIndex: selectedMethod.trait.index,
            methodIndex: method.index,
            supertraitPath: selectedMethod.path.length > 0 ? selectedMethod.path : undefined,
            arguments: checkedArguments.arguments,
            argumentParameterIndices: checkedArguments.parameterIndices,
            bounds,
            providers: providers as HirExpression[],
            erasedParameterTypes: method.parameters.some(containsGenericType)
              ? method.parameters
              : undefined,
            erasedTypeSubstitutions: orderedTypeSubstitutions(
              [...erasedSubstitutions.keys()],
              erasedSubstitutions,
            ),
            erasedResultType: containsGenericType(method.result) ? method.result : undefined,
            type: methodResult,
            span: expression.span,
          };
    }
    return undefined;
  }

  protected checkImplementedMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
    expected?: ValueType,
    qualifiedTraitIndex?: number,
    qualifiedTraitArguments?: readonly ValueType[],
  ): HirExpression {
    const methodName = expression.callee.name;
    const receiverImplementationType = readonlyType(receiver.type);
    if (qualifiedTraitIndex === undefined) {
      const selection = this.selectMethod(receiver.type, methodName, expression.callee.span);
      if (selection.kind === "inherent") {
        const owner = this.memberPath(receiver, selection.steps, expression.callee.receiver.span);
        return this.checkInherentMethodCall(expression, owner, selection.method, expected);
      }
    }
    const searched = implementationsFor(this.implementations, receiverImplementationType);
    const unavailable = new Set<string>(); // traits that supply the method, unavailable here
    const candidates = searched.flatMap((implementation) => {
      // An operator call names no trait arguments: any instance may apply (r-expr.op.left-dispatch).
      const anyInstantiation =
        qualifiedTraitIndex !== undefined && qualifiedTraitArguments === undefined;
      if (anyInstantiation && implementation.traitIndex !== qualifiedTraitIndex) return [];
      const substitutions =
        qualifiedTraitIndex === undefined || anyInstantiation
          ? new Map<string, ValueType>()
          : matchTraitImplementation(
              implementation,
              qualifiedTraitIndex,
              receiverImplementationType,
              qualifiedTraitArguments ?? [],
            );
      if (!substitutions) return [];
      if (
        (qualifiedTraitIndex === undefined || anyInstantiation) &&
        !matchImplementationTarget(implementation, receiverImplementationType, substitutions)
      )
        return [];
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      const method = trait?.methods.find(
        (candidate) => !candidate.associated && candidate.name === methodName,
      );
      const mapping =
        method &&
        implementation.methodFunctions.find((candidate) => candidate.methodIndex === method.index);
      // A dot call sees only available traits (09-traits.md#r-trait.avail.not-candidate).
      if (
        trait &&
        method &&
        qualifiedTraitIndex === undefined &&
        !this.traitAvailable(trait.name, expression.span)
      ) {
        unavailable.add(trait.name);
        return [];
      }
      return trait && method && mapping
        ? [{ trait, method, mapping, substitutions, implementation }]
        : [];
    });
    const callCandidate = (candidate: (typeof candidates)[number]): HirExpression => {
      // A primitive receiver needs no mutable access (04-type-system.md#r-types.prim.no-mut.self-call).
      if (candidate.method.receiverMutable && !mutableOrPrimitive(receiver.type)) {
        this.failReadonlyMethodReceiver(
          candidate.method.name,
          expression.callee.receiver,
          receiver,
        );
      }
      const signature = [...this.signatures.values()].find(
        (value) => value.index === candidate.mapping.functionIndex,
      )!;
      // A numeric family's parameter that the receiver solves is concrete in
      // the parameter types, as in each implementation the family stands for.
      const family = candidate.implementation.family;
      const callSignature = {
        ...signature,
        parameters: signature.parameters
          .slice(1)
          .map((parameter) =>
            family ? substituteGenericType(parameter, candidate.substitutions) : parameter,
          ),
        parameterNames: signature.parameterNames.slice(1),
        defaultFunctionNames: signature.defaultFunctionNames.slice(1),
      };
      const boundedParameters = new Set(signature.genericBounds.map((bound) => bound.parameter));
      const targetSubstitutions = new Map(
        [...candidate.substitutions].map(([parameter, type]) => [
          parameter,
          boundedParameters.has(parameter) ? readonlyType(type) : type,
        ]),
      );
      const checkedArguments = this.checkSignatureArguments(
        expression,
        callSignature,
        expected,
        `method '${candidate.method.name}'`,
        targetSubstitutions,
      );
      const { rowSubstitutions, substitutions: solved } = checkedArguments;
      const substitutions = this.resolveAssociatedTypeSubstitutions(
        signature,
        checkedArguments.substitutions,
        expression.span,
      );
      const unresolved = signature.genericParameters.filter(
        (parameter) => !substitutions.has(parameter),
      );
      if (unresolved.length > 0)
        this.failUnresolvedCall(unresolved, `.${candidate.method.name}`, expression.span, solved);
      const traitArguments = candidate.implementation.traitArguments.map((argument) =>
        substituteGenericType(argument, substitutions),
      );
      if (
        family &&
        !matchTraitImplementation(
          candidate.implementation,
          candidate.implementation.traitIndex,
          receiverImplementationType,
          traitArguments,
        )
      )
        this.fail(
          "type-mismatch",
          `the arguments of '${candidate.method.name}' fit no instantiation of trait '${displayType(candidate.trait.name)}' implemented by '${displayType(receiverImplementationType)}'`,
          expression.span,
        );
      const unresolvedRows = signature.rowParameters.filter(
        (parameter) => !rowSubstitutions.has(parameter),
      );
      if (unresolvedRows.length > 0)
        this.fail(
          "cannot-infer-type",
          `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`,
          expression.span,
        );
      const receiverParameter = substituteGenericType(
        signature.parameters[0]!,
        substitutions,
        rowSubstitutions,
      );
      const methodReceiver = this.requireCoercion(receiver, receiverParameter, receiver.span);
      const { providers, missing } = this.resolveCallProviders(
        signature.requirements,
        substitutions,
        rowSubstitutions,
        expression.span,
      );
      if (missing.length > 0)
        this.fail(
          "missing-requirement",
          `method '${candidate.method.name}' requires ${missing.map(displayType).join(", ")}`,
          expression.span,
        );
      const resultType = substituteGenericType(signature.result, substitutions, rowSubstitutions);
      const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
      // An intrinsic method has no code; the call is inline (09-traits.md#r-trait.impl.intrinsic.inline).
      if (candidate.implementation.intrinsic)
        return {
          kind: "intrinsic-call",
          method: candidate.method.name,
          arguments: [methodReceiver, ...checkedArguments.arguments],
          type: resultType,
          span: expression.span,
        };
      const implementationArgumentParameterIndices = checkedArguments.parameterIndices
        ? [0, ...checkedArguments.parameterIndices.map((parameterIndex) => parameterIndex + 1)]
        : undefined;
      // The receiver fills parameter 0, so a defaulted parameter's index counts it.
      const defaultFields = defaultCallFields(
        this.signatures,
        callSignature.defaultFunctionNames,
        checkedArguments.defaultParameterIndices,
        signature.parameters,
        1,
      );
      const callBase = {
        functionIndex: signature.index,
        functionName: signature.name,
        arguments: [methodReceiver, ...checkedArguments.arguments],
        argumentParameterIndices: implementationArgumentParameterIndices,
        ...defaultFields,
        bounds,
        providers,
        erasedParameterTypes:
          signature.genericParameters.length > 0 || signature.rowParameters.length > 0
            ? signature.parameters
            : undefined,
        erasedTypeSubstitutions: orderedTypeSubstitutions(
          signature.genericParameters,
          substitutions,
        ),
        erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
      };
      return candidate.method.suspending
        ? {
            kind: "suspend-construct",
            ...callBase,
            type: suspensionType(signature.index, resultType),
            span: expression.span,
          }
        : { kind: "call", ...callBase, type: resultType, span: expression.span };
    };
    if (candidates.length === 1) return callCandidate(candidates[0]!);
    if (candidates.length > 1) {
      const trait = candidates[0]!.trait;
      // 09 Method Resolution (TQ-4): among instantiations of one generic trait,
      // the call selects the one instantiation whose method fits.
      if (candidates.some((candidate) => candidate.trait !== trait))
        this.fail(
          "ambiguous-method",
          `method '${expression.callee.name}' is supplied by multiple traits`,
          expression.callee.span,
        );
      const trial = (candidate: (typeof candidates)[number]): HirExpression | undefined => {
        try {
          return speculate(this, () => {
            const call = callCandidate(candidate);
            if (expected) this.requireCoercion(call, expected, expression.span);
            return call;
          });
        } catch (error) {
          if (!(error instanceof CheckFailure)) throw error;
          return undefined;
        }
      };
      const trials = candidates.map((candidate) => ({ candidate, call: trial(candidate) }));
      let fitting = trials.filter((entry) => entry.call !== undefined);
      const kept = keepLiteralDefaults(
        fitting,
        expression,
        1,
        ({ candidate }) =>
          `${trait.name}[${candidate.implementation.traitArguments.map(displayType).join(", ")}]`,
      );
      if (typeof kept === "string") this.fail("type-mismatch", kept, expression.span);
      fitting = kept;
      if (fitting.length > 1)
        this.fail(
          "ambiguous-method",
          `method '${expression.callee.name}' fits ${fitting.length} instantiations of trait '${displayType(trait.name)}'; qualify the call as ${displayType(trait.name)}::[...]::${expression.callee.name}(value, ...)`,
          expression.callee.span,
        );
      if (fitting.length === 0) {
        const available = candidates
          .map(
            (candidate) =>
              `${displayType(trait.name)}[${candidate.implementation.traitArguments.map(displayType).join(", ")}]`,
          )
          .join(", ");
        this.fail(
          "type-mismatch",
          `the arguments of '${expression.callee.name}' fit no instantiation of trait '${displayType(trait.name)}' implemented by '${displayType(receiverImplementationType)}'; available: ${available}`,
          expression.span,
        );
      }
      return callCandidate(fitting[0]!.candidate);
    }
    this.failUnknownMethod(
      receiver.type,
      expression.callee.name,
      expression.callee.span,
      unavailable,
    );
  }

  private checkFunctionValueCall(
    expression: CallExpression,
    expected: ValueType | undefined,
  ): HirExpression {
    const callee = this.checkExpression(expression.callee);
    const callable = functionParts(readonlyType(callee.type));
    // Any other type is called through `Apply` (05-expressions.md#callable-values).
    if (!callable) return this.applyCall(expression, callee, expected);
    if (expression.argumentNames?.some((name) => name !== undefined)) {
      this.fail(
        "unknown-named-argument",
        "named arguments require a statically known function or method declaration",
        expression.span,
      );
    }
    const parameterNames = callable.parameters.map((_, index) => `$${index}`);
    const checkedArguments = this.checkConcreteArguments(
      expression,
      callable.parameters,
      parameterNames,
      callable.variadic,
      "function value",
    );
    const providers = callable.requirements.map((requirement) =>
      this.resolveProvider(requirement, expression.span),
    );
    const missing = callable.requirements.filter((_, index) => !providers[index]);
    if (missing.length > 0) {
      this.fail(
        "missing-requirement",
        `closure call requires ${missing.map(displayType).join(", ")}`,
        expression.span,
      );
    }
    return {
      kind: "closure-call",
      callee,
      arguments: checkedArguments.arguments,
      providers: providers as HirExpression[],
      type: callable.suspending
        ? mutableType(nominalGenericType("Suspend", [callable.result]))
        : callable.result,
      span: expression.span,
    };
  }

  private checkNamedIntrinsicCall(
    expression: NamedCallExpression,
    expected?: ValueType,
  ): HirExpression | undefined {
    const intrinsic = this.checkIntrinsicFunctionCall(expression, expected);
    if (intrinsic) return intrinsic;
    if (isDowncastValImport(this.imports, expression.callee.name)) {
      const inspection = this.checkInspectFunctionCall(expression, "downcast_val", expected);
      if (inspection) return inspection;
    }
    if (this.imports.get(expression.callee.name) === "std.task.block_on") {
      // The compiled module is the entry module, whose initialization may
      // drive (req.drive.block-on.forbidden-contexts names non-entry modules).
      if (this.deferDepth > 0) {
        this.fail(
          "suspension-forbidden-context",
          "block_on cannot start a suspension driver in this context",
          expression.span,
        );
      }
      if (expression.typeArguments?.length)
        this.fail("argument-count", "block_on infers its result type", expression.span);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "block_on has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length !== 1)
        this.fail("argument-count", "block_on expects one mutable suspension", expression.span);
      this.resolveArgumentMapping(expression, ["s"], "block_on");
      const source = expression.arguments[0]!;
      this.requireDrivableSuspension(source);
      const suspension = this.checkExpression(source);
      const storedParts = storedSuspensionParts(suspension.type);
      if (storedParts) {
        if (!storedParts.mutable)
          this.fail("mutable-receiver-required", "block_on requires mut Suspend[T]", source.span);
        return {
          kind: "suspension-drive",
          suspension,
          blockOn: true,
          type: storedParts.result,
          span: expression.span,
        };
      }
      const parts = suspensionParts(suspension.type);
      if (parts) {
        const signature = [...this.signatures.values()].find(
          (candidate) => candidate.index === parts.functionIndex,
        );
        return {
          kind: "suspend-drive",
          functionIndex: parts.functionIndex,
          suspension,
          erasedResultType: signature?.genericParameters.length ? signature.result : undefined,
          blockOn: true,
          type: parts.result,
          span: expression.span,
        };
      }
      const traitParts = traitSuspensionParts(suspension.type);
      if (traitParts) {
        return {
          kind: "trait-suspend-drive",
          traitIndex: traitParts.traitIndex,
          methodIndex: traitParts.methodIndex,
          suspension,
          blockOn: true,
          type: traitParts.result,
          span: expression.span,
        };
      }
      this.fail(
        "type-mismatch",
        `block_on expects mut Suspend[T], found ${displayType(suspension.type)}`,
        source.span,
      );
    }
    if (this.imports.get(expression.callee.name) === "std.testing.assert_equal") {
      if (expression.typeArguments?.length)
        this.fail("argument-count", "assert_equal infers its value type", expression.span);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "assert_equal has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length !== 3)
        this.fail(
          "argument-count",
          "assert_equal expects actual, expected, and reason",
          expression.span,
        );
      const mapping = this.resolveArgumentMapping(
        expression,
        ["actual", "expected", "reason"],
        "assert_equal",
      );
      const parameterIndex = (argumentIndex: number): number =>
        mapping?.[argumentIndex] ?? argumentIndex;
      const sourceIndex = (parameter: number): number =>
        expression.arguments.findIndex((_, index) => parameterIndex(index) === parameter);
      const actualIndex = sourceIndex(0);
      const expectedIndex = sourceIndex(1);
      const reasonIndex = sourceIndex(2);
      // `T` is the readonly view of `actual`'s type, so a `mut` actual
      // compares with a readonly expected value.
      const actualSource = expression.arguments[actualIndex]!;
      const hint = this.literalJoinExpected(actualSource, expression.arguments[expectedIndex]!);
      const checkedActual = this.checkExpression(actualSource, hint);
      const actual = { ...checkedActual, type: readonlyType(checkedActual.type) };
      const shownActual = displayType(actual.type);
      if (!this.equalityStrategy(actual.type))
        this.fail(
          "unsatisfied-trait-bound",
          `type '${shownActual}' does not implement Eq, required by assert_equal`,
          actual.span,
        );
      // spec/lang/10-modules.md#r-module.testing.assert-equal-debug
      const debugMessage = `type '${shownActual}' does not implement Debug, required by assert_equal`;
      if (!this.implementsTrait(actual.type, "Debug"))
        this.fail("unsatisfied-trait-bound", debugMessage, actual.span);
      const checkedByParameter = [
        actual,
        this.checkJoinedArgument("assert_equal", expression.arguments[expectedIndex]!, actual.type),
        this.requireCoercion(
          this.checkExpression(expression.arguments[reasonIndex]!, "string"),
          "string",
          expression.arguments[reasonIndex]!.span,
        ),
      ];
      return this.checkEqualCall(
        expression.arguments.map((_, index) => checkedByParameter[parameterIndex(index)]!),
        mapping,
        actual.type,
        expression.span,
      );
    }
    // `std.testing.snapshot(text, expect="")` compares text with a literal
    // expectation (spec/lang/10-modules.md#snapshots). The prototype checks it as an
    // `assert_equal` of two strings; it has no update run to rewrite `expect`.
    if (this.imports.get(expression.callee.name) === "std.testing.snapshot") {
      if (expression.typeArguments?.length)
        this.fail("argument-count", "snapshot has no type arguments", expression.span);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "snapshot has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length < 1 || expression.arguments.length > 2)
        this.fail(
          "argument-count",
          "snapshot expects text and an optional expect",
          expression.span,
        );
      const mapping = this.resolveArgumentMapping(expression, ["text", "expect"], "snapshot");
      const parameterIndex = (argumentIndex: number): number =>
        mapping?.[argumentIndex] ?? argumentIndex;
      const textIndex = expression.arguments.findIndex((_, index) => parameterIndex(index) === 0);
      if (textIndex < 0)
        this.fail("argument-count", "snapshot is missing argument text", expression.span);
      const expectIndex = expression.arguments.findIndex((_, index) => parameterIndex(index) === 1);
      const expect: Expression =
        expectIndex < 0
          ? { kind: "string", value: "", span: expression.span }
          : expression.arguments[expectIndex]!;
      // `expect` must be a literal a tool can rewrite
      // (spec/lang/10-modules.md#r-module.testing.snapshot.literal).
      if (expect.kind !== "string")
        this.fail(
          "non-literal-test-argument",
          "a snapshot expect must be a string literal without interpolation",
          expect.span,
        );
      const text = this.requireCoercion(
        this.checkExpression(expression.arguments[textIndex]!, "string"),
        "string",
        expression.arguments[textIndex]!.span,
      );
      return this.checkEqualCall(
        [
          text,
          this.checkExpression(expect, "string"),
          this.checkExpression(
            { kind: "string", value: "the snapshot matches", span: expression.span },
            "string",
          ),
        ],
        undefined,
        "string",
        expression.span,
      );
    }
    // Checker intrinsics that only typed derivation generates
    // (spec/lang/14-annotations.md#handles).
    if (expression.callee.name === STRUCTURE_MISMATCH) {
      this.hasPanicDetail = true;
      return {
        kind: "panic",
        message: this.checkExpression({
          kind: "string",
          value: "structure-variant-mismatch",
          span: expression.span,
        }),
        category: "structure-variant-mismatch",
        type: "never",
        span: expression.span,
      };
    }
    if (expression.callee.name === STRUCTURE_AS_DECLARED && expression.arguments.length === 1) {
      const value = this.checkExpression(expression.arguments[0]!);
      return { ...value, type: mutableType(readonlyType(value.type)) };
    }
    if (
      expression.callee.name === ENTRY_ERROR_REPORT &&
      this.declaration.compilerGenerated === true &&
      expression.arguments.length === 1
    )
      return this.checkEntryErrorReport(expression.arguments[0]!, expression.span);
    const structure = this.checkStructureFactIntrinsic(expression);
    if (structure) return structure;
    if (expression.callee.name === "panic") {
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "panic has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length !== 1)
        this.fail("argument-count", "panic expects one message argument", expression.span);
      this.resolveArgumentMapping(expression, ["message"], "panic");
      const message = this.checkExpression(expression.arguments[0]!);
      this.requireAssignable(message.type, "string", message.span);
      this.hasPanicDetail = true;
      return { kind: "panic", message, type: "never", span: expression.span };
    }

    if (
      expression.callee.name.startsWith("$enum-literal.") ||
      expression.callee.name.startsWith("$enum-template.")
    ) {
      return this.checkInternalEnumLiteral(expression, expression.callee.name, expected);
    }
    return undefined;
  }

  // The text the host writes for an entry point's `.Err(error)`
  // (checker/entry-error.ts): `std.error`'s report with its cause chain when
  // the error's type implements `Error`, else its `Display` text. An error
  // type with neither is already an entry-result error, so it renders as "".
  private checkEntryErrorReport(error: Expression, span: SourceSpan): HirExpression {
    const errorType = readonlyType(this.checkExpression(error).type);
    const chain = this.signatures.get(STD_ENTRY_REPORT);
    const chainParameter = chain?.parameters[0];
    const chainTrait = chainParameter === undefined ? undefined : traitTypeName(chainParameter);
    if (
      chain &&
      chainTrait !== undefined &&
      (errorType === readonlyType(chainParameter!) || this.implementsTrait(errorType, chainTrait))
    )
      return this.checkExpression({
        kind: "call",
        callee: { kind: "name", name: STD_ENTRY_REPORT, span },
        arguments: [error],
        span,
      });
    const display =
      ["bool", "char", "string"].includes(errorType) ||
      numericType(errorType) !== undefined ||
      this.implementsTrait(errorType, "Display");
    if (display)
      return this.checkExpression({
        kind: "call",
        callee: { kind: "member", receiver: error, name: "to_string", span },
        arguments: [],
        span,
      });
    return this.checkExpression({ kind: "string", value: "", span });
  }

  // A checked `assert_equal` or `snapshot` runs `std.testing`'s hd
  // `check_equal` with `T = valueType`, which the loader declares for a
  // program that imports either (checker/standard-library.ts).
  private checkEqualCall(
    arguments_: readonly HirExpression[],
    argumentParameterIndices: readonly number[] | undefined,
    valueType: ValueType,
    span: SourceSpan,
  ): HirExpression {
    const signature = this.signatures.get(CHECK_EQUAL);
    if (!signature) throw new Error("std.testing.check_equal is not declared");
    return {
      kind: "call",
      functionIndex: signature.index,
      functionName: signature.name,
      arguments: arguments_,
      argumentParameterIndices,
      bounds: this.resolveBoundDictionaries(signature, new Map([["T", valueType]]), span),
      providers: [],
      erasedParameterTypes: signature.parameters,
      erasedTypeSubstitutions: orderedTypeSubstitutions(
        signature.genericParameters,
        new Map([["T", valueType]]),
      ),
      erasedResultType: signature.result,
      type: "void",
      span,
    };
  }

  protected checkDeclaredCall(
    expression: NamedCallExpression,
    expected?: ValueType,
    initialSubstitutions?: ReadonlyMap<string, ValueType>,
  ): HirExpression {
    if (this.globals.has(expression.callee.name) && !this.resolveGlobal(expression.callee.name)) {
      this.fail(
        "binding-not-yet-visible",
        `module binding '${expression.callee.name}' is not visible before its binding point`,
        expression.callee.span,
      );
    }
    const signature = this.visibleSignature(expression.callee.name);
    if (!signature)
      this.failUnknownName(
        expression.callee.name,
        `unknown function '${expression.callee.name}'${loopNameHint(expression.callee.name)}`,
        expression.callee.span,
      );
    const checkedArguments = this.checkSignatureArguments(
      expression,
      signature,
      expected,
      undefined,
      initialSubstitutions,
    );
    const { rowSubstitutions } = checkedArguments;
    const substitutions = this.resolveAssociatedTypeSubstitutions(
      signature,
      checkedArguments.substitutions,
      expression.span,
    );
    const unresolved = signature.genericParameters.filter(
      (parameter) => !substitutions.has(parameter),
    );
    const misfit = unresolved.length > 0 ? resultMisfit(signature.result, expected) : undefined;
    if (misfit) this.fail("type-mismatch", misfit, expression.span);
    if (unresolved.length > 0)
      this.failUnresolvedCall(unresolved, expression.callee.name, expression.span, substitutions);
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
    if (missing.length > 0) {
      this.fail(
        "missing-requirement",
        `call to '${displayType(signature.name)}' requires ${missing.map(displayType).join(", ")}`,
        expression.span,
      );
    }
    const resultType = substituteGenericType(signature.result, substitutions, rowSubstitutions);
    const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
    const defaultFields = defaultCallFields(
      this.signatures,
      signature.defaultFunctionNames,
      checkedArguments.defaultParameterIndices,
      signature.parameters,
      0,
    );
    return signature.suspending
      ? {
          kind: "suspend-construct",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: checkedArguments.arguments,
          argumentParameterIndices: checkedArguments.parameterIndices,
          ...defaultFields,
          bounds,
          providers,
          erasedParameterTypes:
            signature.genericParameters.length > 0 || signature.rowParameters.length > 0
              ? signature.parameters
              : undefined,
          erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
          erasedTypeSubstitutions: orderedTypeSubstitutions(
            signature.genericParameters,
            substitutions,
          ),
          type: suspensionType(signature.index, resultType),
          span: expression.span,
        }
      : {
          kind: "call",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: checkedArguments.arguments,
          argumentParameterIndices: checkedArguments.parameterIndices,
          ...defaultFields,
          bounds,
          providers,
          erasedParameterTypes:
            signature.genericParameters.length > 0 || signature.rowParameters.length > 0
              ? signature.parameters
              : undefined,
          erasedTypeSubstitutions: orderedTypeSubstitutions(
            signature.genericParameters,
            substitutions,
          ),
          erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
          type: resultType,
          span: expression.span,
        };
  }

  private checkQualifiedCall(
    expression: QualifiedCallExpression,
    expected?: ValueType,
  ): HirExpression {
    let owner = this.canonicalTypeName(expression.callee.owner);
    if (owner === TYPE_ID && expression.callee.name === "of") {
      const inspection = this.checkInspectFunctionCall(expression, "of", expected);
      if (inspection) return inspection;
    }
    // A called bound reference `value::name(...)` is the method call
    // `value.name(...)` (07-functions.md#r-fn.ref.call).
    if (this.namesReferenceValue(expression.callee.owner))
      return this.checkMemberCall(this.receiverMemberCall(expression, 0), expected);
    if (expression.callee.genericTypeOwner) {
      owner = this.canonicalTypeName(expression.callee.genericTypeOwner);
      expression = { ...expression, callee: { ...expression.callee, owner } };
    }
    const trait = this.traitTypes.get(owner);
    if (trait) {
      const sourceArguments = expression.callee.ownerTypeArguments ?? [];
      if (sourceArguments.length !== trait.genericParameters.length)
        this.fail(
          arityCode(sourceArguments.length, trait.genericParameters.length),
          `trait '${displayType(trait.name)}' expects ${trait.genericParameters.length} type arguments`,
          expression.callee.span,
        );
      const traitArguments = sourceArguments.map((argument) => this.resolveType(argument));
      const traitMethod = trait.methods.find((method) => method.name === expression.callee.name);
      if (!traitMethod)
        this.fail(
          "unknown-method",
          `trait '${displayType(trait.name)}' has no method '${expression.callee.name}'${this.declaringTraitHint(trait, expression.callee.name)}`,
          expression.callee.span,
        );
      if (traitMethod.associated)
        return this.checkTraitAssociatedCall(
          expression,
          trait,
          traitArguments,
          traitMethod,
          expected,
        );
      if (expression.arguments.length === 0)
        this.fail(
          "argument-count",
          `qualified method '${displayType(trait.name)}.${expression.callee.name}' requires a receiver`,
          expression.span,
        );
      if (expression.argumentNames?.[0] !== undefined || expression.argumentSpreads?.[0])
        this.fail(
          "argument-order",
          "a trait-qualified receiver must be the first ordinary argument",
          expression.arguments[0]!.span,
        );
      const receiverSource = expression.arguments[0]!;
      const receiver = this.checkExpression(receiverSource);
      const memberExpression: MemberCallExpression = {
        ...expression,
        callee: {
          kind: "member",
          receiver: receiverSource,
          name: expression.callee.name,
          span: expression.callee.span,
        },
        arguments: expression.arguments.slice(1),
        argumentNames: expression.argumentNames?.slice(1),
        argumentSpreads: expression.argumentSpreads?.slice(1),
      };
      return this.checkImplementedMemberCall(
        memberExpression,
        receiver,
        expected,
        trait.index,
        traitArguments,
      );
    }
    if (this.signature.genericParameters.includes(owner))
      return this.checkBoundAssociatedCall(expression, owner);
    // A generic target's parameters, as `T` of `Iterator::from_fn`, are
    // inferred like the function's own.
    const member = this.inherentMethods.find(
      (method) =>
        method.associated &&
        (nominalGenericParts(method.targetType)?.name ?? method.targetType) === owner &&
        method.name === expression.callee.name,
    );
    if (member) this.requireInherentVisible(member, expression.callee.span);
    if (member)
      return this.checkDeclaredCall(
        {
          ...expression,
          callee: { kind: "name", name: member.functionName, span: expression.callee.span },
        },
        expected,
      );
    const ownerArguments = (expression.callee.ownerTypeArguments ?? []).map((argument) =>
      this.resolveType(argument),
    );
    const ownerType = ownerArguments.length > 0 ? nominalGenericType(owner, ownerArguments) : owner;
    // `Type::method(receiver, ...)` calls the unbound reference, receiver
    // first (07-functions.md#r-fn.ref.call).
    if (
      this.hasReceiverMethod(ownerType, expression.callee.name) &&
      expression.arguments.length > 0
    )
      return this.checkReceiverFirstCall(expression, ownerType, expected);
    return this.checkTypeAssociatedCall(
      expression,
      ownerType,
      this.associatedCandidates(ownerType, expression.callee),
      expected,
    );
  }
}
