import { standardImportHint } from "./standard-uses.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import type { HirTrait, ValueType } from "../hir.ts";
import {
  bindingParts,
  contextKeys,
  functionParts,
  inputsInner,
  mutableInner,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  restInner,
  resultParts,
  rowArgumentKeys,
  tupleParts,
  displayType,
} from "../types.ts";
import { associatedNames, bindingNameProblem, traitKeyParts } from "./associated-bindings.ts";
import { traitIsDynamicallySafe } from "./dynamic-safety.ts";
import { extendsInspectable } from "./inspectable.ts";
import { rowParameterName } from "./requirement-rows.ts";
import { STANDARD_INSPECTABLE } from "./standard-traits.ts";

type KnownType = (type: ValueType) => boolean;

/** Resolve the type arguments of a key without turning its outer trait into a value type. */
export function resolveRequirementKeyTypes(
  requirement: string,
  resolveType: (type: ValueType) => ValueType,
): string {
  const nominal = nominalGenericParts(requirement);
  return nominal
    ? nominalGenericType(nominal.name, nominal.arguments.map(resolveType))
    : requirement;
}

/**
 * Validate normalized requirement keys at any checker entry point. Keeping
 * this independent of declarations prevents callable types, row arguments,
 * contexts, and provider expressions from acquiring weaker rules.
 */
export function requirementKeyDiagnostics(
  requirements: readonly string[],
  traitTypes: ReadonlyMap<string, HirTrait>,
  span: SourceSpan,
  knownType?: KnownType,
): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const hasStandardInspectable = [...traitTypes.values()].some(
    (trait) => trait.standardName === STANDARD_INSPECTABLE,
  );
  for (const requirement of requirements) {
    if (rowParameterName(requirement)) continue;
    const key = traitKeyParts(requirement);
    const trait = traitTypes.get(key.name);
    if (!trait) {
      diagnostics.push({
        code: "unknown-trait",
        message: `unknown trait '${displayType(key.name)}' in requirement key '${displayType(requirement)}'${standardImportHint(key.name, "type")}`,
        span,
      });
      continue;
    }
    if (hasStandardInspectable && extendsInspectable(traitTypes, trait.name)) {
      diagnostics.push({
        code: "inspectable-requirement",
        message: `'${displayType(trait.name)}' extends Inspectable, so it cannot be a requirement key`,
        span,
      });
      continue;
    }
    if (trait.genericParameters.length !== key.positional.length) {
      diagnostics.push({
        code: "generic-arity",
        message: `trait '${displayType(trait.name)}' expects ${trait.genericParameters.length} type arguments`,
        span,
      });
      continue;
    }
    const duplicate = key.bindings.find(
      (binding, index) => key.bindings.findIndex((other) => other.name === binding.name) !== index,
    );
    if (duplicate) {
      diagnostics.push({
        code: "duplicate-associated-binding",
        message: `associated type '${duplicate.name}' of '${displayType(trait.name)}' is bound more than once`,
        span,
      });
      continue;
    }
    const problem = key.bindings
      .map((binding) => bindingNameProblem(trait, binding.name, traitTypes))
      .find((candidate) => candidate !== undefined);
    if (problem) {
      diagnostics.push({ ...problem, span });
      continue;
    }
    const arguments_ = [...key.positional, ...key.bindings.map((binding) => binding.type)];
    if (knownType && !arguments_.every(knownType)) {
      diagnostics.push({
        code: "unknown-type",
        message: `requirement key '${displayType(requirement)}' contains an unknown type`,
        span,
      });
      continue;
    }
    const bound = new Set(key.bindings.map((binding) => binding.name));
    const unbound = [...associatedNames(trait, traitTypes)].filter((name) => !bound.has(name));
    if (unbound.length > 0) {
      diagnostics.push({
        code: "trait-not-dynamically-safe",
        message: `requirement key '${displayType(requirement)}' leaves the associated type${unbound.length === 1 ? "" : "s"} ${unbound.join(", ")} of '${displayType(trait.name)}' unbound; write '${displayType(trait.name)}[${unbound.map((name) => `${name} = ...`).join(", ")}]'`,
        span,
      });
      continue;
    }
    if (!traitIsDynamicallySafe(trait, traitTypes, bound)) {
      diagnostics.push({
        code: "trait-not-dynamically-safe",
        message: `trait '${displayType(trait.name)}' cannot be used as requirement key '${displayType(requirement)}' because it is not dynamically safe`,
        span,
      });
      continue;
    }
    for (const argument of arguments_)
      diagnostics.push(...requirementKeyDiagnosticsInType(argument, traitTypes, span, knownType));
  }
  return diagnostics;
}

/** Every requirement row nested in a resolved type. */
export function requirementKeyDiagnosticsInType(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
  span: SourceSpan,
  knownType?: KnownType,
): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const visitKeys = (keys: readonly string[]): void => {
    diagnostics.push(...requirementKeyDiagnostics(keys, traitTypes, span, knownType));
  };
  const visit = (current: ValueType): void => {
    const binding = bindingParts(current);
    if (binding) return visit(binding.type);
    const wrapper = restInner(current) ?? inputsInner(current) ?? mutableInner(current);
    if (wrapper !== undefined) return visit(wrapper);
    const tuple = tupleParts(current);
    if (tuple) return tuple.forEach(visit);
    const optional = optionalInner(current);
    if (optional !== undefined) return visit(optional);
    const result = resultParts(current);
    if (result) {
      visit(result.ok);
      visit(result.error);
      return;
    }
    const callable = functionParts(current);
    if (callable) {
      visitKeys(callable.requirements);
      callable.parameters.forEach(visit);
      visit(callable.result);
      return;
    }
    const context = contextKeys(current);
    if (context) return visitKeys(context);
    const row = rowArgumentKeys(current);
    if (row) return visitKeys(row);
    nominalGenericParts(current)?.arguments.forEach(visit);
  };
  visit(type);
  return diagnostics;
}
