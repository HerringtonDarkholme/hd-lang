import type { Expression } from "../ast.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  readonlyType,
  resultParts,
  suspensionParts,
  suspensionType,
  traitSuspensionParts,
  traitSuspensionType,
} from "../types.ts";
import { supportsMvpEquality } from "./context.ts";
import { genericTypeName, substituteGenericType, traitTypeName } from "./shared.ts";

import { ExpressionOperatorChecker } from "./expression-operators.ts";
type CallExpression = Extract<Expression, { kind: "call" }>;
interface MemberCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "member" }>;
}

interface NamedCallExpression extends CallExpression {
  readonly callee: Extract<Expression, { kind: "name" }>;
}

export abstract class ExpressionCallChecker extends ExpressionOperatorChecker {
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
    return this.checkImplementedMemberCall(expression, receiver);
  }

  private checkBuiltInMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): HirExpression | undefined {
    const methodName = expression.callee.name;
    const suspension = suspensionParts(receiver.type);
    const traitSuspension = traitSuspensionParts(receiver.type);
    if ((suspension || traitSuspension) && expression.callee.name === "cancel") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "Suspend.cancel expects no arguments", expression.span);
      this.requireDrivableSuspension(expression.callee.receiver);
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
          type: callable.result,
          span: expression.span,
        };
      }
    }
    if (receiver.type === "string" && expression.callee.name === "len") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "string.len expects no arguments", expression.span);
      return { kind: "string-length", receiver, type: "i32", span: expression.span };
    }
    if (receiver.type === "string" && expression.callee.name === "starts_with") {
      if (expression.arguments.length !== 1)
        this.fail("argument-count", "string.starts_with expects one argument", expression.span);
      if (expression.argumentSpreads?.some(Boolean))
        this.fail(
          "positional-spread-needs-vararg",
          "string.starts_with has no variadic parameter",
          expression.span,
        );
      const argumentName = expression.argumentNames?.[0];
      if (argumentName && argumentName !== "prefix")
        this.fail(
          "unknown-named-argument",
          `string.starts_with has no parameter named '${argumentName}'`,
          expression.arguments[0]!.span,
        );
      const prefix = this.requireCoercion(
        this.checkExpression(expression.arguments[0]!, "string"),
        "string",
        expression.arguments[0]!.span,
      );
      return {
        kind: "string-starts-with",
        receiver,
        prefix,
        type: "bool",
        span: expression.span,
      };
    }
    const receiverNominal = nominalGenericParts(readonlyType(receiver.type));
    if (receiverNominal?.name === "list" && expression.callee.name === "len") {
      if (expression.arguments.length !== 0)
        this.fail("argument-count", "list.len expects no arguments", expression.span);
      return { kind: "list-length", receiver, type: "i32", span: expression.span };
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
    const matchingBounds = receiverBounds.filter(({ trait }) =>
      trait.methods.some((method) => method.name === methodName),
    );
    if (matchingBounds.length > 1) {
      this.fail(
        "ambiguous-bound-method",
        `method '${methodName}' is supplied by multiple bounds on ${receiverGeneric}`,
        expression.callee.span,
      );
    }
    const receiverBound = matchingBounds[0];
    const dispatchReceiver: HirExpression = receiverBound
      ? {
          kind: "trait-bound",
          value: receiver,
          traitIndex: receiverBound.bound.traitIndex,
          boundIndex: receiverBound.boundIndex,
          type:
            mutableInner(receiver.type) !== undefined
              ? mutableType(`trait:${receiverBound.bound.traitName}`)
              : `trait:${receiverBound.bound.traitName}`,
          span: receiver.span,
        }
      : receiver;
    const dynamicTraitName = traitTypeName(dispatchReceiver.type);
    const dynamicTrait = dynamicTraitName && this.traitTypes.get(dynamicTraitName);
    if (dynamicTrait) {
      const method = dynamicTrait.methods.find((candidate) => candidate.name === methodName);
      if (!method)
        this.fail(
          "unknown-method",
          `trait '${dynamicTrait.name}' has no method '${expression.callee.name}'`,
          expression.callee.span,
        );
      if (method.receiverMutable && mutableInner(dispatchReceiver.type) === undefined) {
        this.fail(
          "mutable-receiver-required",
          `method '${method.name}' requires mutable access to ${dynamicTrait.name}`,
          expression.callee.receiver.span,
        );
      }
      const checkedArguments = this.checkConcreteArguments(
        expression,
        method.parameters,
        method.parameterNames,
        method.variadic,
        `method '${method.name}'`,
      );
      const providers = method.requirements.map((requirement) =>
        this.resolveProvider(requirement, expression.span),
      );
      const missing = method.requirements.filter((_, index) => !providers[index]);
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
            traitIndex: dynamicTrait.index,
            methodIndex: method.index,
            arguments: checkedArguments.arguments,
            argumentParameterIndices: checkedArguments.parameterIndices,
            providers: providers as HirExpression[],
            type: traitSuspensionType(dynamicTrait.index, method.index, method.result),
            span: expression.span,
          }
        : {
            kind: "trait-call",
            receiver: dispatchReceiver,
            traitIndex: dynamicTrait.index,
            methodIndex: method.index,
            arguments: checkedArguments.arguments,
            argumentParameterIndices: checkedArguments.parameterIndices,
            providers: providers as HirExpression[],
            type: method.result,
            span: expression.span,
          };
    }
    return undefined;
  }

  private checkImplementedMemberCall(
    expression: MemberCallExpression,
    receiver: HirExpression,
  ): HirExpression {
    const methodName = expression.callee.name;
    const receiverImplementationType = readonlyType(receiver.type);
    const inherent = this.inherentMethods.find(
      (method) => method.targetType === receiverImplementationType && method.name === methodName,
    );
    if (inherent) return this.checkInherentMethodCall(expression, receiver, inherent);
    const receiverData = this.dataTypes.get(receiverImplementationType);
    const promoted =
      receiverData?.fields.flatMap((field) => {
        if (!field.embedded) return [];
        const fieldType = readonlyType(field.type);
        return this.inherentMethods
          .filter((method) => method.targetType === fieldType && method.name === methodName)
          .map((method) => ({ field, fieldType, method }));
      }) ?? [];
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
      return this.checkInherentMethodCall(expression, promotedReceiver, selected.method);
    }
    const candidates = this.implementations.flatMap((implementation) => {
      if (implementation.targetType !== receiverImplementationType) return [];
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      const method = trait?.methods.find((candidate) => candidate.name === methodName);
      const mapping =
        method &&
        implementation.methodFunctions.find((candidate) => candidate.methodIndex === method.index);
      return trait && method && mapping ? [{ trait, method, mapping }] : [];
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
      const receiverParameterType = candidate.method.receiverMutable
        ? mutableType(receiverImplementationType)
        : receiverImplementationType;
      const methodReceiver = this.requireCoercion(receiver, receiverParameterType, receiver.span);
      const checkedArguments = this.checkConcreteArguments(
        expression,
        candidate.method.parameters,
        candidate.method.parameterNames,
        candidate.method.variadic,
        `method '${candidate.method.name}'`,
      );
      const signature = [...this.signatures.values()].find(
        (value) => value.index === candidate.mapping.functionIndex,
      )!;
      const providers = signature.requirements.map((requirement) =>
        this.resolveProvider(requirement, expression.span),
      );
      const missing = signature.requirements.filter((_, index) => !providers[index]);
      if (missing.length > 0)
        this.fail(
          "missing-requirement",
          `method '${candidate.method.name}' requires ${missing.join(" + ")}`,
          expression.span,
        );
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
            providers: providers as HirExpression[],
            type: suspensionType(signature.index, signature.result),
            span: expression.span,
          }
        : {
            kind: "call",
            functionIndex: signature.index,
            functionName: signature.name,
            arguments: [methodReceiver, ...checkedArguments.arguments],
            argumentParameterIndices: implementationArgumentParameterIndices,
            providers: providers as HirExpression[],
            type: signature.result,
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
      type: callable.result,
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
      if (!supportsMvpEquality(actual.type)) {
        this.fail(
          "missing-partial-eq",
          `type '${actual.type}' does not implement PartialEq in the executable MVP`,
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
          ? this.checkExpression(expression.arguments[0]!, payloadType)
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
      this.fail(
        "unknown-name",
        `unknown function '${expression.callee.name}'`,
        expression.callee.span,
      );
    const checkedArguments = this.checkSignatureArguments(expression, signature, expected);
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
}
