import type { Expression } from "../ast.ts";
import type { HirExpression, HirTrait, HirTraitMethod, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  suspensionType,
  traitSuspensionParts,
  traitSuspensionType,
  tupleType,
} from "../types.ts";
import {
  containsGenericType,
  genericTypeName,
  matchGenericTypePattern,
  matchTraitImplementation,
  substituteGenericType,
  traitTypeName,
} from "./shared.ts";

import { ExpressionOperatorChecker } from "./expression-operators.ts";
type CallExpression = Extract<Expression, { kind: "call" }>;
interface MemberCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "member" }>;
}

interface NamedCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "name" }>;
}

interface QualifiedCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "qualified-name" }>;
}

interface ResolvedTraitMethod {
  readonly method: HirTraitMethod;
  readonly path: readonly number[];
  readonly trait: HirTrait;
}

export abstract class ExpressionCallChecker extends ExpressionOperatorChecker {
  private findTraitMethods(
    trait: HirTrait,
    name: string,
    path: readonly number[] = [],
    seen: ReadonlySet<number> = new Set(),
  ): ResolvedTraitMethod[] {
    if (seen.has(trait.index)) return [];
    const nextSeen = new Set([...seen, trait.index]);
    const direct = trait.methods
      .filter((method) => !method.associated && method.name === name)
      .map((method) => ({ method, path, trait }));
    const inherited = trait.supertraits.flatMap((supertrait, fieldIndex) => {
      const parent = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === supertrait.traitIndex,
      );
      return parent ? this.findTraitMethods(parent, name, [...path, fieldIndex], nextSeen) : [];
    });
    return [...direct, ...inherited];
  }

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
    if (expression.callee.kind === "member") {
      return this.checkMemberCall(expression as MemberCallExpression, expected);
    }
    if (expression.callee.kind === "qualified-name") {
      return this.checkQualifiedCall(expression as QualifiedCallExpression, expected);
    }
    if (
      expression.callee.kind !== "name" ||
      this.resolveLocal(expression.callee.name) ||
      this.availableCaptures.has(expression.callee.name) ||
      this.resolveGlobal(expression.callee.name)
    ) {
      return this.checkFunctionValueCall(expression);
    }
    const namedExpression = expression as NamedCallExpression;
    const intrinsic = this.checkNamedIntrinsicCall(namedExpression, expected);
    if (intrinsic) return intrinsic;
    return this.checkDeclaredCall(namedExpression, expected);
  }

  private checkMemberCall(expression: MemberCallExpression, expected?: ValueType): HirExpression {
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
    const receiver = this.checkExpression(expression.callee.receiver);
    const builtin = this.checkBuiltInMemberCall(expression, receiver);
    if (builtin) return builtin;
    const dynamic = this.checkDynamicMemberCall(expression, receiver);
    if (dynamic) return dynamic;
    return this.checkImplementedMemberCall(expression, receiver, expected);
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
    const fieldReceiverType = readonlyType(receiver.type);
    const fieldReceiverNominal = nominalGenericParts(fieldReceiverType);
    const fieldDeclaration = this.dataTypes.get(fieldReceiverNominal?.name ?? fieldReceiverType);
    const callableField = fieldDeclaration?.fields.find((field) => field.name === methodName);
    if (fieldDeclaration && callableField) {
      const substitutions = new Map<string, ValueType>();
      if (fieldReceiverNominal)
        fieldDeclaration.genericParameters.forEach((parameter, index) =>
          substitutions.set(parameter, fieldReceiverNominal.arguments[index]!),
        );
      const callableType = substituteGenericType(callableField.type, substitutions);
      const callable = functionParts(callableType);
      if (callable) {
        if (expression.argumentNames?.some((name) => name !== undefined)) {
          this.fail(
            "named-argument-needs-declaration",
            "named arguments are unavailable through a stored function field",
            expression.span,
          );
        }
        const fieldCallee: HirExpression = {
          kind: "member",
          receiver,
          dataIndex: fieldDeclaration.index,
          fieldIndex: callableField.index,
          erasedFieldType: genericTypeName(callableField.type) ? callableField.type : undefined,
          type: callableType,
          span: expression.callee.span,
        };
        const parameterNames = callable.parameters.map((_, index) => `$${index}`);
        const checkedArguments = this.checkConcreteArguments(
          expression,
          callable.parameters,
          parameterNames,
          callable.variadic,
          "function field",
        );
        const providers = callable.requirements.map((requirement) =>
          this.resolveProvider(requirement, expression.span),
        );
        const missing = callable.requirements.filter((_, index) => !providers[index]);
        if (missing.length > 0)
          this.fail(
            "missing-requirement",
            `function field requires ${missing.join(" + ")}`,
            expression.span,
          );
        return {
          kind: "closure-call",
          callee: fieldCallee,
          arguments: checkedArguments.arguments,
          providers: providers as HirExpression[],
          type: callable.suspending
            ? mutableType(nominalGenericType("Suspend", [callable.result]))
            : callable.result,
          span: expression.span,
        };
      }
    }
    const stringCall = this.checkStringMemberCall(expression, receiver);
    if (stringCall) return stringCall;
    const receiverNominal = nominalGenericParts(readonlyType(receiver.type));
    if (receiverNominal?.name === "list" && expression.callee.name === "len") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "list.len expects no arguments", expression.span);
      return { kind: "list-length", receiver, type: "i32", span: expression.span };
    }
    if (receiverNominal?.name === "list" && expression.callee.name === "iter") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "list.iter expects no arguments", expression.span);
      const elementType = receiverNominal.arguments[0]!;
      return {
        kind: "list-iterator",
        receiver,
        elementType,
        type: mutableType(nominalGenericType("Iterator", [elementType])),
        span: expression.span,
      };
    }
    if (receiverNominal?.name === "Iterator" && expression.callee.name === "next") {
      if (mutableInner(receiver.type) === undefined)
        this.fail(
          "mutable-receiver-required",
          "Iterator.next requires mutable iterator access",
          expression.callee.receiver.span,
        );
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "Iterator.next expects no arguments", expression.span);
      const elementType = receiverNominal.arguments[0]!;
      return {
        kind: "iterator-next",
        receiver,
        elementType,
        type: `${elementType}?`,
        span: expression.span,
      };
    }
    if (receiverNominal?.name === "list" && expression.callee.name === "append") {
      if (mutableInner(receiver.type) === undefined)
        this.fail(
          "mutable-receiver-required",
          "list.append requires mutable list access",
          expression.callee.receiver.span,
        );
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "list.append has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length !== 1)
        this.fail("argument-count", "list.append expects one value", expression.span);
      this.resolveArgumentMapping(expression, ["value"], "list.append");
      const elementType = receiverNominal.arguments[0]!;
      const value = this.requireCoercion(
        this.checkExpression(expression.arguments[0]!, elementType),
        elementType,
        expression.arguments[0]!.span,
      );
      return {
        kind: "list-append",
        receiver,
        value,
        elementType,
        type: "void",
        span: expression.span,
      };
    }
    if (receiverNominal?.name === "map" && expression.callee.name === "len") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "map.len expects no arguments", expression.span);
      return { kind: "map-length", receiver, type: "i32", span: expression.span };
    }
    if (receiverNominal?.name === "map" && expression.callee.name === "iter") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "map.iter expects no arguments", expression.span);
      const elementType = tupleType(receiverNominal.arguments);
      return {
        kind: "map-iterator",
        receiver,
        elementType,
        type: mutableType(nominalGenericType("Iterator", [elementType])),
        span: expression.span,
      };
    }
    if (
      receiverNominal?.name === "map" &&
      (expression.callee.name === "get" || expression.callee.name === "remove")
    ) {
      const removing = expression.callee.name === "remove";
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
        type: `${valueType}?`,
        span: expression.span,
      };
    }
    return undefined;
  }

  private checkStringMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): HirExpression | undefined {
    if (receiver.type !== "string") return undefined;
    const method = expression.callee.name;
    if (method === "len") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "string.len expects no arguments", expression.span);
      return { kind: "string-length", receiver, type: "i32", span: expression.span };
    }
    if (method === "trim" || method === "lower") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", `string.${method} expects no arguments`, expression.span);
      return {
        kind: "string-transform",
        operation: method,
        receiver,
        type: "string",
        span: expression.span,
      };
    }
    if (method !== "split" && method !== "starts_with") return undefined;
    if (expression.arguments.length !== 1)
      this.fail("argument-count", `string.${method} expects one argument`, expression.span);
    if (expression.argumentSpreads?.some(Boolean))
      this.fail(
        "positional-spread-needs-vararg",
        `string.${method} has no variadic parameter`,
        expression.span,
      );
    const parameterName = method === "split" ? "separator" : "prefix";
    const argumentName = expression.argumentNames?.[0];
    if (argumentName && argumentName !== parameterName)
      this.fail(
        "unknown-named-argument",
        `string.${method} has no parameter named '${argumentName}'`,
        expression.arguments[0]!.span,
      );
    const argument = this.requireCoercion(
      this.checkExpression(expression.arguments[0]!, "string"),
      "string",
      expression.arguments[0]!.span,
    );
    return method === "split"
      ? {
          kind: "string-split",
          receiver,
          separator: argument,
          type: nominalGenericType("list", ["string"]),
          span: expression.span,
        }
      : {
          kind: "string-starts-with",
          receiver,
          prefix: argument,
          type: "bool",
          span: expression.span,
        };
  }

  private checkDynamicMemberCall(
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
        "ambiguous-bound-method",
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
            receiverBound.bound.mutable || mutableInner(receiver.type) !== undefined
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
        this.fail(
          "mutable-receiver-required",
          `method '${method.name}' requires mutable access to ${dynamicTrait.name}`,
          expression.callee.receiver.span,
        );
      }
      const traitKey = readonlyType(dispatchReceiver.type).slice("trait:".length);
      const traitArguments = nominalGenericParts(traitKey)?.arguments ?? [];
      const selectedTraitArguments = this.resolveTraitPath(
        dynamicTrait,
        traitArguments,
        selectedMethod.path,
      ).arguments;
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
            `generic:${receiverGeneric}::${associated.name}`,
          ),
        );
      }
      const methodParameters = method.parameters.map((parameter) =>
        substituteGenericType(parameter, traitSubstitutions),
      );
      const methodResult = substituteGenericType(method.result, traitSubstitutions);
      const methodRequirements = method.requirements.map((requirement) =>
        substituteGenericType(requirement, traitSubstitutions),
      );
      const checkedArguments = this.checkConcreteArguments(
        expression,
        methodParameters,
        method.parameterNames,
        method.variadic,
        `method '${method.name}'`,
      );
      const providers = methodRequirements.map((requirement) =>
        this.resolveProvider(requirement, expression.span),
      );
      const missing = methodRequirements.filter((_, index) => !providers[index]);
      if (missing.length > 0)
        this.fail(
          "missing-requirement",
          `method '${method.name}' requires ${missing.join(" + ")}`,
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
            providers: providers as HirExpression[],
            erasedParameterTypes: method.parameters.some(containsGenericType)
              ? method.parameters
              : undefined,
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
            providers: providers as HirExpression[],
            erasedParameterTypes: method.parameters.some(containsGenericType)
              ? method.parameters
              : undefined,
            erasedResultType: containsGenericType(method.result) ? method.result : undefined,
            type: methodResult,
            span: expression.span,
          };
    }
    return undefined;
  }

  private checkImplementedMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
    expected?: ValueType,
    qualifiedTraitIndex?: number,
    qualifiedTraitArguments: readonly ValueType[] = [],
  ): HirExpression {
    const methodName = expression.callee.name;
    const receiverImplementationType = readonlyType(receiver.type);
    const inherent =
      qualifiedTraitIndex === undefined &&
      this.inherentMethods.find(
        (method) =>
          !method.associated &&
          method.targetType === receiverImplementationType &&
          method.name === methodName,
      );
    if (inherent) return this.checkInherentMethodCall(expression, receiver, inherent, expected);
    const receiverData = this.dataTypes.get(receiverImplementationType);
    const promoted =
      qualifiedTraitIndex !== undefined
        ? []
        : (receiverData?.fields.flatMap((field) => {
            if (!field.embedded) return [];
            const fieldType = readonlyType(field.type);
            return this.inherentMethods
              .filter(
                (method) =>
                  !method.associated &&
                  method.targetType === fieldType &&
                  method.name === methodName,
              )
              .map((method) => ({ field, fieldType, method }));
          }) ?? []);
    if (promoted.length > 1)
      this.fail(
        "ambiguous-method",
        `method '${methodName}' is promoted by multiple embedded fields`,
        expression.callee.span,
      );
    if (promoted.length === 1) {
      const selected = promoted[0]!;
      const promotedReceiver: HirExpression = {
        kind: "member",
        receiver,
        dataIndex: receiverData!.index,
        fieldIndex: selected.field.index,
        erasedFieldType: genericTypeName(selected.field.type) ? selected.field.type : undefined,
        type: selected.fieldType,
        span: expression.callee.receiver.span,
      };
      return this.checkInherentMethodCall(expression, promotedReceiver, selected.method, expected);
    }
    const candidates = this.implementations.flatMap((implementation) => {
      const substitutions =
        qualifiedTraitIndex === undefined
          ? new Map<string, ValueType>()
          : matchTraitImplementation(
              implementation,
              qualifiedTraitIndex,
              receiverImplementationType,
              qualifiedTraitArguments,
            );
      if (!substitutions) return [];
      if (
        qualifiedTraitIndex === undefined &&
        !matchGenericTypePattern(
          implementation.targetType,
          receiverImplementationType,
          substitutions,
        )
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
      return trait && method && mapping ? [{ trait, method, mapping, substitutions }] : [];
    });
    if (candidates.length > 1)
      this.fail(
        "ambiguous-method",
        `method '${expression.callee.name}' is supplied by multiple traits`,
        expression.callee.span,
      );
    const candidate = candidates[0];
    if (candidate) {
      if (candidate.method.receiverMutable && mutableInner(receiver.type) === undefined) {
        this.fail(
          "mutable-receiver-required",
          `method '${candidate.method.name}' requires mutable access to ${receiverImplementationType}`,
          expression.callee.receiver.span,
        );
      }
      const signature = [...this.signatures.values()].find(
        (value) => value.index === candidate.mapping.functionIndex,
      )!;
      const callSignature = {
        ...signature,
        parameters: signature.parameters.slice(1),
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
      const { substitutions, rowSubstitutions } = checkedArguments;
      const unresolved = signature.genericParameters.filter(
        (parameter) => !substitutions.has(parameter),
      );
      if (unresolved.length > 0)
        this.fail(
          "unresolved-generic-placeholder",
          `could not infer generic parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`,
          expression.span,
        );
      const unresolvedRows = signature.rowParameters.filter(
        (parameter) => !rowSubstitutions.has(parameter),
      );
      if (unresolvedRows.length > 0)
        this.fail(
          "unresolved-generic-placeholder",
          `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`,
          expression.span,
        );
      const receiverParameter = substituteGenericType(
        signature.parameters[0]!,
        substitutions,
        rowSubstitutions,
      );
      const methodReceiver = this.requireCoercion(receiver, receiverParameter, receiver.span);
      this.warnAbsentRowSubtractions(signature.requirements, rowSubstitutions, expression.span);
      const { providers, missing } = this.resolveCallProviders(
        signature.requirements,
        substitutions,
        rowSubstitutions,
        expression.span,
      );
      if (missing.length > 0)
        this.fail(
          "missing-requirement",
          `method '${candidate.method.name}' requires ${missing.join(" + ")}`,
          expression.span,
        );
      const resultType = substituteGenericType(signature.result, substitutions, rowSubstitutions);
      const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
      const implementationArgumentParameterIndices = checkedArguments.parameterIndices
        ? [0, ...checkedArguments.parameterIndices.map((parameterIndex) => parameterIndex + 1)]
        : undefined;
      return candidate.method.suspending
        ? {
            kind: "suspend-construct",
            functionIndex: signature.index,
            functionName: signature.name,
            arguments: [methodReceiver, ...checkedArguments.arguments],
            argumentParameterIndices: implementationArgumentParameterIndices,
            bounds,
            providers,
            erasedParameterTypes:
              signature.genericParameters.length > 0 || signature.rowParameters.length > 0
                ? signature.parameters
                : undefined,
            type: suspensionType(signature.index, resultType),
            span: expression.span,
          }
        : {
            kind: "call",
            functionIndex: signature.index,
            functionName: signature.name,
            arguments: [methodReceiver, ...checkedArguments.arguments],
            argumentParameterIndices: implementationArgumentParameterIndices,
            bounds,
            providers,
            erasedParameterTypes:
              signature.genericParameters.length > 0 || signature.rowParameters.length > 0
                ? signature.parameters
                : undefined,
            erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
            type: resultType,
            span: expression.span,
          };
    }
    this.fail(
      "unknown-method",
      `type '${receiver.type}' has no supported method '${expression.callee.name}'`,
      expression.callee.span,
    );
  }

  private checkFunctionValueCall(expression: CallExpression): HirExpression {
    if (expression.argumentNames?.some((name) => name !== undefined)) {
      this.fail(
        "named-argument-needs-declaration",
        "named arguments require a statically known function or method declaration",
        expression.span,
      );
    }
    const callee = this.checkExpression(expression.callee);
    const callable = functionParts(callee.type);
    if (!callable)
      this.fail("not-callable", `type '${callee.type}' is not callable`, expression.callee.span);
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
        `closure call requires ${missing.join(" + ")}`,
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
    if (this.imports.get(expression.callee.name) === "std.task.block_on") {
      if (this.deferDepth > 0 || this.moduleBody) {
        this.fail(
          "suspension-forbidden-context",
          "block_on cannot start a suspension driver in this context",
          expression.span,
        );
      }
      if (expression.typeArguments?.length)
        this.fail("unexpected-type-arguments", "block_on infers its result type", expression.span);
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
        `block_on expects mut Suspend[T], found ${suspension.type}`,
        source.span,
      );
    }
    if (this.imports.get(expression.callee.name) === "std.testing.assert") {
      if (expression.typeArguments?.length)
        this.fail("unexpected-type-arguments", "assert has no type arguments", expression.span);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "assert has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length !== 2)
        this.fail("argument-count", "assert expects condition and reason", expression.span);
      const mapping = this.resolveArgumentMapping(expression, ["condition", "reason"], "assert");
      const parameterIndex = (argumentIndex: number): number =>
        mapping?.[argumentIndex] ?? argumentIndex;
      const sourceIndex = (parameter: number): number =>
        expression.arguments.findIndex((_, index) => parameterIndex(index) === parameter);
      const checkedByParameter = [
        this.requireCoercion(
          this.checkExpression(expression.arguments[sourceIndex(0)]!, "bool"),
          "bool",
          expression.arguments[sourceIndex(0)]!.span,
        ),
        this.requireCoercion(
          this.checkExpression(expression.arguments[sourceIndex(1)]!, "string"),
          "string",
          expression.arguments[sourceIndex(1)]!.span,
        ),
      ];
      return {
        kind: "assert",
        arguments: expression.arguments.map(
          (_, index) => checkedByParameter[parameterIndex(index)]!,
        ),
        argumentParameterIndices: mapping,
        type: "void",
        span: expression.span,
      };
    }
    if (this.imports.get(expression.callee.name) === "std.testing.assert_equal") {
      if (expression.typeArguments?.length)
        this.fail(
          "unexpected-type-arguments",
          "assert_equal infers its value type",
          expression.span,
        );
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
      const actual = this.checkExpression(expression.arguments[actualIndex]!);
      const strategy = this.equalityStrategy(actual.type);
      if (!strategy) {
        this.fail(
          "missing-partial-eq",
          `type '${actual.type}' does not implement PartialEq`,
          actual.span,
        );
      }
      const checkedByParameter = [
        actual,
        this.requireCoercion(
          this.checkExpression(expression.arguments[expectedIndex]!, actual.type),
          actual.type,
          expression.arguments[expectedIndex]!.span,
        ),
        this.requireCoercion(
          this.checkExpression(expression.arguments[reasonIndex]!, "string"),
          "string",
          expression.arguments[reasonIndex]!.span,
        ),
      ];
      const arguments_ = expression.arguments.map(
        (_, index) => checkedByParameter[parameterIndex(index)]!,
      );
      return {
        kind: "assert-equal",
        arguments: arguments_,
        argumentParameterIndices: mapping,
        valueType: actual.type,
        strategy,
        type: "void",
        span: expression.span,
      };
    }
    if (expression.callee.name === "Ok" || expression.callee.name === "Err") {
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          `${expression.callee.name} has no variadic parameter`,
          expression.span,
        );
      const parts = expected && resultParts(expected);
      if (!parts)
        this.fail(
          "result-constructor-needs-context",
          `${expression.callee.name} requires an expected Result type`,
          expression.span,
        );
      const ok = expression.callee.name === "Ok";
      const payloadType = ok ? parts.ok : parts.error;
      const expectedCount = ok && payloadType === "void" ? 0 : 1;
      if (expression.arguments.length !== expectedCount) {
        this.fail(
          "argument-count",
          `${expression.callee.name} expects ${expectedCount} argument${expectedCount === 1 ? "" : "s"}`,
          expression.span,
        );
      }
      this.resolveArgumentMapping(
        expression,
        expectedCount === 0 ? [] : [ok ? "value" : "error"],
        expression.callee.name,
      );
      const payload =
        expectedCount === 1
          ? this.requireCoercion(
              this.checkExpression(expression.arguments[0]!, payloadType),
              payloadType,
              expression.arguments[0]!.span,
            )
          : undefined;
      return {
        kind: "variant-wrap",
        variant: ok ? "result-ok" : "result-error",
        payload,
        payloadType,
        type: expected,
        span: expression.span,
      };
    }
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
      return { kind: "panic", message, type: "never", span: expression.span };
    }
    if (expression.callee.name === "println") {
      if (expression.typeArguments)
        this.fail("unexpected-type-arguments", "println infers its Display type", expression.span);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "println has no variadic parameter",
          expression.span,
        );
      if (expression.arguments.length !== 1)
        this.fail("argument-count", "println expects one value argument", expression.span);
      this.resolveArgumentMapping(expression, ["value"], "println");
      const operand = this.checkExpression(expression.arguments[0]!);
      const value = this.displayValue(operand, operand.span);
      const provider = this.resolveProvider("Console", expression.span);
      if (!provider) this.fail("missing-requirement", "println requires Console", expression.span);
      return { kind: "console-print", provider, value, type: "void", span: expression.span };
    }
    if (expression.callee.name.startsWith("$enum-literal.")) {
      return this.checkInternalEnumLiteral(expression, expression.callee.name, expected);
    }
    return undefined;
  }

  private checkDeclaredCall(expression: NamedCallExpression, expected?: ValueType): HirExpression {
    if (this.globals.has(expression.callee.name) && !this.resolveGlobal(expression.callee.name)) {
      this.fail(
        "binding-not-yet-visible",
        `module binding '${expression.callee.name}' is not visible before its binding point`,
        expression.callee.span,
      );
    }
    const signature = this.signatures.get(expression.callee.name);
    if (!signature)
      this.failUnknownName(
        expression.callee.name,
        `unknown function '${expression.callee.name}'`,
        expression.callee.span,
      );
    const checkedArguments = this.checkSignatureArguments(expression, signature, expected);
    const { rowSubstitutions } = checkedArguments;
    const substitutions = this.resolveAssociatedTypeSubstitutions(
      signature,
      checkedArguments.substitutions,
    );
    const unresolved = signature.genericParameters.filter(
      (parameter) => !substitutions.has(parameter),
    );
    if (unresolved.length > 0)
      this.fail(
        "unresolved-generic-placeholder",
        `could not infer generic parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`,
        expression.span,
      );
    const unresolvedRows = signature.rowParameters.filter(
      (parameter) => !rowSubstitutions.has(parameter),
    );
    if (unresolvedRows.length > 0)
      this.fail(
        "unresolved-generic-placeholder",
        `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`,
        expression.span,
      );
    this.warnAbsentRowSubtractions(signature.requirements, rowSubstitutions, expression.span);
    const { providers, missing } = this.resolveCallProviders(
      signature.requirements,
      substitutions,
      rowSubstitutions,
      expression.span,
    );
    if (missing.length > 0) {
      this.fail(
        "missing-requirement",
        `call to '${signature.name}' requires ${missing.join(" + ")}`,
        expression.span,
      );
    }
    const resultType = substituteGenericType(signature.result, substitutions, rowSubstitutions);
    const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
    const defaultArguments = checkedArguments.defaultParameterIndices.map((parameterIndex) => ({
      parameterIndex,
      functionIndex: this.signatures.get(signature.defaultFunctionNames[parameterIndex]!)!.index,
    }));
    return signature.suspending
      ? {
          kind: "suspend-construct",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: checkedArguments.arguments,
          argumentParameterIndices: checkedArguments.parameterIndices,
          defaultArguments: defaultArguments.length > 0 ? defaultArguments : undefined,
          parameterTypes: defaultArguments.length > 0 ? signature.parameters : undefined,
          bounds,
          providers,
          erasedParameterTypes:
            signature.genericParameters.length > 0 || signature.rowParameters.length > 0
              ? signature.parameters
              : undefined,
          type: suspensionType(signature.index, resultType),
          span: expression.span,
        }
      : {
          kind: "call",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: checkedArguments.arguments,
          argumentParameterIndices: checkedArguments.parameterIndices,
          defaultArguments: defaultArguments.length > 0 ? defaultArguments : undefined,
          parameterTypes: defaultArguments.length > 0 ? signature.parameters : undefined,
          bounds,
          providers,
          erasedParameterTypes:
            signature.genericParameters.length > 0 || signature.rowParameters.length > 0
              ? signature.parameters
              : undefined,
          erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
          type: resultType,
          span: expression.span,
        };
  }

  private checkQualifiedCall(
    expression: QualifiedCallExpression,
    expected?: ValueType,
  ): HirExpression {
    const owner = expression.callee.owner;
    const trait = this.traitTypes.get(owner);
    if (trait) {
      const sourceArguments = expression.callee.ownerTypeArguments ?? [];
      if (sourceArguments.length !== trait.genericParameters.length)
        this.fail(
          "generic-arity",
          `trait '${trait.name}' expects ${trait.genericParameters.length} type arguments`,
          expression.callee.span,
        );
      const traitArguments = sourceArguments.map((argument) => this.resolveType(argument));
      const traitMethod = trait.methods.find((method) => method.name === expression.callee.name);
      if (!traitMethod)
        this.fail(
          "unknown-method",
          `trait '${trait.name}' has no method '${expression.callee.name}'`,
          expression.callee.span,
        );
      if (traitMethod.associated)
        this.fail(
          "associated-function-needs-target",
          `associated function '${trait.name}.${traitMethod.name}' must be qualified by an implementing type`,
          expression.callee.span,
        );
      if (expression.arguments.length === 0)
        this.fail(
          "argument-count",
          `qualified method '${trait.name}.${expression.callee.name}' requires a receiver`,
          expression.span,
        );
      if (expression.argumentNames?.[0] !== undefined || expression.argumentSpreads?.[0])
        this.fail(
          "qualified-receiver-position",
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
    const member = this.inherentMethods.find(
      (method) =>
        method.associated && method.targetType === owner && method.name === expression.callee.name,
    );
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
    const associatedCandidates = this.implementations.flatMap((implementation) => {
      const substitutions = new Map<string, ValueType>();
      if (!matchGenericTypePattern(implementation.targetType, ownerType, substitutions)) return [];
      const candidateTrait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      const method = candidateTrait?.methods.find(
        (candidate) => candidate.associated && candidate.name === expression.callee.name,
      );
      const mapping =
        method &&
        implementation.methodFunctions.find((candidate) => candidate.methodIndex === method.index);
      return candidateTrait && method && mapping ? [{ candidateTrait, method, mapping }] : [];
    });
    if (associatedCandidates.length > 1)
      this.fail(
        "ambiguous-associated-function",
        `associated function '${expression.callee.name}' is supplied by multiple traits for '${ownerType}'`,
        expression.callee.span,
      );
    const associated = associatedCandidates[0];
    if (!associated) {
      const ownerBase = nominalGenericParts(ownerType)?.name ?? ownerType;
      if (!this.dataTypes.has(ownerBase) && !this.enumTypes.has(ownerBase))
        this.fail(
          "unknown-type",
          `unknown associated-function owner '${ownerType}'`,
          expression.callee.span,
        );
      this.fail(
        "unknown-associated-function",
        `type '${ownerType}' has no associated function '${expression.callee.name}'`,
        expression.callee.span,
      );
    }
    const associatedSignature = [...this.signatures.values()].find(
      (signature) => signature.index === associated.mapping.functionIndex,
    )!;
    return this.checkDeclaredCall(
      {
        ...expression,
        callee: {
          kind: "name",
          name: associatedSignature.name,
          span: expression.callee.span,
        },
      },
      expected,
    );
  }
}
