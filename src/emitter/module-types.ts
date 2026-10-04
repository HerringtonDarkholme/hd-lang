import type { HirProgram, ValueType } from "../hir.ts";
import {
  contextKeys,
  functionParts,
  functionType,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleLayout,
} from "../types.ts";
import { cellInner } from "../checker/captured-cells.ts";

/** The closure signatures and provider contexts a module declares types for. */
interface CollectedModuleTypes {
  readonly signatureNames: Map<ValueType, number>;
  readonly contextNames: Map<ValueType, number>;
}

/**
 * The closure signature a function type shares with the types it converts to
 * by declared variance (07-functions.md#r-fn.type.variance-repr): `mut` is
 * erased at run time, so a parameter's or the result's outer `mut` does not
 * select a different closure struct.
 */
function signatureKey(type: ValueType): ValueType {
  const callable = functionParts(type);
  if (!callable) return type;
  const { parameters, result, requirements, variadic, suspending } = callable;
  if (functionType(parameters, result, requirements, variadic, suspending) !== type) return type;
  return functionType(
    parameters.map(readonlyType),
    readonlyType(result),
    requirements,
    variadic,
    suspending,
  );
}

/** Closure signature indexes keyed by `signatureKey`. */
class SignatureMap extends Map<ValueType, number> {
  override get(type: ValueType): number | undefined {
    return super.get(signatureKey(type));
  }

  override has(type: ValueType): boolean {
    return super.has(signatureKey(type));
  }

  override set(type: ValueType, index: number): this {
    return super.set(signatureKey(type), index);
  }
}

export function collectModuleTypes(
  program: HirProgram,
  methodIsLive: (traitIndex: number, methodIndex: number) => boolean,
): CollectedModuleTypes {
  const signatureNames = new SignatureMap();
  const contextNames = new Map<ValueType, number>();
  const collectType = (type: ValueType): void => {
    if (type.startsWith("trait:")) return;
    const cell = cellInner(type);
    if (cell !== undefined) return collectType(cell);
    const mutable = mutableInner(type);
    if (mutable !== undefined) return collectType(mutable);
    const tuple = tupleLayout(type);
    if (tuple !== undefined) {
      tuple.forEach(collectType);
      return;
    }
    const nominal = nominalGenericParts(type);
    if (storedSuspensionParts(type)) {
      collectType(storedSuspensionParts(type)!.result);
      return;
    }
    if (nominal?.name === "List" && nominal.arguments.length === 1) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (nominal?.name === "Map" && nominal.arguments.length === 2) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (nominal && program.data.some((declaration) => declaration.name === nominal.name)) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (nominal && program.enums.some((declaration) => declaration.name === nominal.name)) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (contextKeys(type)) {
      if (!contextNames.has(type)) contextNames.set(type, contextNames.size);
      return;
    }
    const suspension = suspensionParts(type);
    if (suspension) {
      collectType(suspension.result);
      return;
    }
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension) {
      collectType(traitSuspension.result);
      return;
    }
    const callable = functionParts(type);
    if (callable) {
      if (!signatureNames.has(type)) signatureNames.set(type, signatureNames.size);
      callable.parameters.forEach(collectType);
      collectType(callable.result);
      return;
    }
    const optional = optionalInner(type);
    if (optional !== undefined) collectType(optional);
    const result = resultParts(type);
    if (result) {
      collectType(result.ok);
      collectType(result.error);
    }
  };
  for (const declaration of [...program.functions, ...program.closures]) {
    declaration.parameters.forEach((parameter) => collectType(parameter.type));
    declaration.locals.forEach((local) => collectType(local.type));
    collectType(declaration.result);
    if (declaration.closure || declaration.genericParameters.length === 0) {
      collectType(
        functionType(
          declaration.parameters.map((parameter) => parameter.type),
          declaration.result,
          declaration.requirements,
          declaration.variadic,
          declaration.suspending,
        ),
      );
    }
  }
  program.globals.forEach((global) => collectType(global.type));
  program.data.forEach((declaration) =>
    declaration.fields.forEach((field) => collectType(field.type)),
  );
  program.enums.forEach((declaration) =>
    declaration.fields.forEach((field) => collectType(field.type)),
  );
  program.traits.forEach((trait) =>
    trait.methods
      .filter((method) => methodIsLive(trait.index, method.index))
      .forEach((method) => {
        method.parameters.forEach(collectType);
        collectType(method.result);
      }),
  );
  return { signatureNames, contextNames };
}
