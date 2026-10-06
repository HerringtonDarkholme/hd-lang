import type { GenericBound } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import type {
  HirData,
  HirDeclaredBound,
  HirEnum,
  HirGenericBound,
  HirTrait,
  ValueType,
} from "../hir.ts";
import { genericTypeName } from "./shared.ts";
import {
  bindingParts,
  contextKeys,
  functionParts,
  inputsInner,
  mutableInner,
  nominalGenericParts,
  nonListRestElement,
  optionalInner,
  readonlyType,
  restInner,
  resultParts,
  rowArgumentKeys,
  tupleParts,
  displayType,
} from "../types.ts";
import { enclosingBoundProof } from "./trait-paths.ts";
import {
  ambiguousProjection,
  traitValueBindings,
  writtenBindingProblem,
} from "./associated-bindings.ts";
import { traitIsDynamicallySafe } from "./dynamic-safety.ts";
import { implementsTrait } from "./map-keys.ts";
import { TUPLE_TRAIT } from "./standard-traits.ts";

interface TypeProblem {
  readonly code: string;
  readonly message: string;
}

export function traitTypeName(type: ValueType): string | undefined {
  const readonly = readonlyType(type);
  if (!readonly.startsWith("trait:") || readonly.endsWith("?")) return undefined;
  const key = readonly.slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
}

/** The first trait value in `type` that cannot be used for dynamic dispatch. */
export function dynamicTraitProblemInType(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
): TypeProblem | undefined {
  const visit = (current: ValueType): TypeProblem | undefined => {
    const binding = bindingParts(current);
    if (binding) return visit(binding.type);
    const wrapper = restInner(current) ?? inputsInner(current) ?? mutableInner(current);
    if (wrapper !== undefined) return visit(wrapper);
    const optional = optionalInner(current);
    if (optional !== undefined) return visit(optional);

    const dynamicTraitName = traitTypeName(current);
    const dynamicTrait = dynamicTraitName && traitTypes.get(dynamicTraitName);
    if (
      dynamicTrait &&
      !traitIsDynamicallySafe(dynamicTrait, traitTypes, new Set(traitValueBindings(current).keys()))
    )
      return {
        code: "trait-not-dynamically-safe",
        message: `trait '${dynamicTrait.name}' cannot be used as a dynamic value`,
      };

    const tuple = tupleParts(current);
    if (tuple) {
      for (const element of tuple) {
        const problem = visit(element);
        if (problem) return problem;
      }
      return undefined;
    }
    const result = resultParts(current);
    if (result) return visit(result.ok) ?? visit(result.error);
    const callable = functionParts(current);
    if (callable) {
      for (const key of callable.requirements) {
        for (const argument of nominalGenericParts(key)?.arguments ?? []) {
          const problem = visit(argument);
          if (problem) return problem;
        }
      }
      for (const parameter of callable.parameters) {
        const problem = visit(parameter);
        if (problem) return problem;
      }
      return visit(callable.result);
    }
    const requirementKeys = contextKeys(current) ?? rowArgumentKeys(current);
    if (requirementKeys) {
      for (const key of requirementKeys) {
        for (const argument of nominalGenericParts(key)?.arguments ?? []) {
          const problem = visit(argument);
          if (problem) return problem;
        }
      }
      return undefined;
    }
    const nominal = nominalGenericParts(current);
    if (nominal) {
      for (const argument of nominal.arguments) {
        const problem = visit(argument);
        if (problem) return problem;
      }
    }
    return undefined;
  };
  return visit(type);
}

/** A rest element that is not a `List[T]` (04-type-system.md#r-types.tuple.rest.list). */
export function restElementProblem(type: ValueType): TypeProblem | undefined {
  const rest = nonListRestElement(type);
  return rest === undefined
    ? undefined
    : {
        code: "type-mismatch",
        message: `a rest element must be a List[T], as in 'List[${displayType(rest)}]...', not '${displayType(rest)}...'`,
      };
}

/** Structural problems shared by declaration and local written types. */
export function writtenTypeProblem(
  type: ValueType,
  bounds: readonly HirGenericBound[],
  traitTypes: ReadonlyMap<string, HirTrait>,
): TypeProblem | undefined {
  return (
    restElementProblem(type) ??
    writtenBindingProblem(type, traitTypes) ??
    ambiguousProjection(type, bounds, traitTypes)
  );
}
/** Value-category bounds take no implementation evidence, like the call checker. */
const EVIDENCE_FREE_BOUNDS = new Set(["Any", "AnyVal", "AnyRef"]);

/**
 * The key bound of the compiler-owned `Map`, which has no declaration
 * (spec/lang/04-type-system.md#map-key-types).
 */
const MAP_KEY_BOUNDS: readonly HirDeclaredBound[] = [{ parameter: "K", traits: ["Eq", "Hash"] }];

/** What checking a written type application's arguments against its declaration's bounds needs. */
export interface WrittenBoundScope {
  readonly dataTypes: ReadonlyMap<string, HirData>;
  readonly enumTypes: ReadonlyMap<string, HirEnum>;
  readonly traitTypes: ReadonlyMap<string, HirTrait>;
  /** Type parameters bounded by `Eq` and `Hash`, which may key a map (trait.hash.map-key). */
  readonly hashableParameters: ReadonlySet<string>;
  /** Whether an enclosing bound on `parameter` requires trait `traitName`. */
  readonly parameterImplied: (parameter: string, traitName: string) => boolean;
}

/**
 * The required trait's canonical name, when it names a trait checked with
 * implementation evidence. Value categories, the sealed `Tuple` marker, and
 * unknown traits need none here: categories pass no dictionary, and an
 * unknown trait is reported, if at all, where it is declared.
 */
function evidenceTraitName(
  source: string,
  traitTypes: ReadonlyMap<string, HirTrait>,
): string | undefined {
  const key = mutableInner(source) ?? source;
  // Required trait arguments are left for the call-site check, which
  // resolves them; comparing names only here never rejects valid code.
  const name = nominalGenericParts(key)?.name ?? key;
  if (EVIDENCE_FREE_BOUNDS.has(name)) return undefined;
  const trait = traitTypes.get(name);
  // `std.function.Tuple` is a sealed marker that every tuple type
  // implements; it passes no dictionary (fn.type.ctor.tuple-trait).
  if (!trait || trait.standardName === TUPLE_TRAIT) return undefined;
  return trait.name;
}

/**
 * Why the written application `name[args]` fails its declaration's bounds,
 * if it does: each argument must meet the bound on its parameter
 * (`trait.bound.no-implied`), at the place it is written.
 */
export function writtenApplicationBoundProblem(
  name: string,
  args: readonly ValueType[],
  scope: WrittenBoundScope,
): { readonly code: string; readonly message: string } | undefined {
  let parameters: readonly string[];
  let bounds: readonly HirDeclaredBound[];
  let rows: ReadonlySet<string>;
  if (name === "Map" && args.length >= 1) {
    const key = args[0]!;
    if (mutableInner(key) !== undefined)
      return {
        code: "invalid-map-key",
        message: `a map key type must not be mut, found '${displayType(key)}'`,
      };
    parameters = ["K", "V"];
    bounds = MAP_KEY_BOUNDS;
    rows = new Set();
  } else {
    const declaration = scope.dataTypes.get(name) ?? scope.enumTypes.get(name);
    if (!declaration?.declaredBounds || declaration.declaredBounds.length === 0) return undefined;
    parameters = declaration.genericParameters;
    bounds = declaration.declaredBounds;
    rows = new Set("rowParameters" in declaration ? (declaration.rowParameters ?? []) : []);
  }
  for (const bound of bounds) {
    if (rows.has(bound.parameter)) continue;
    const index = parameters.indexOf(bound.parameter);
    if (index < 0 || index >= args.length) continue;
    const checked = readonlyType(args[index]!);
    if (rowArgumentKeys(checked) !== undefined) continue;
    const missing: string[] = [];
    for (const source of bound.traits) {
      const traitName = evidenceTraitName(source, scope.traitTypes);
      if (traitName === undefined) continue;
      const generic = genericTypeName(checked);
      const implied =
        generic !== undefined
          ? name === "Map" && (traitName === "Eq" || traitName === "Hash")
            ? // A map key meets `Eq` and `Hash` through its own bounds (trait.hash.map-key).
              scope.hashableParameters.has(generic)
            : scope.parameterImplied(generic, traitName)
          : implementsTrait(scope.traitTypes, checked, traitName, scope.hashableParameters, 0);
      if (!implied) missing.push(traitName);
    }
    if (missing.length > 0)
      return {
        code: "unsatisfied-trait-bound",
        message: `type '${displayType(checked)}' does not implement ${missing.join(" and ")}, required by the bound on '${bound.parameter}' of '${name}'`,
      };
  }
  return undefined;
}

/** The first unsatisfied declared bound in a written type, nested applications included. */
export function writtenBoundProblem(
  type: ValueType,
  scope: WrittenBoundScope,
): { readonly code: string; readonly message: string } | undefined {
  const visit = (
    current: ValueType,
  ): { readonly code: string; readonly message: string } | undefined => {
    const binding = bindingParts(current);
    if (binding) return visit(binding.type);
    const wrapper = restInner(current) ?? inputsInner(current) ?? mutableInner(current);
    if (wrapper !== undefined) return visit(wrapper);
    const optional = optionalInner(current);
    if (optional !== undefined) return visit(optional);
    const tuple = tupleParts(current);
    if (tuple) {
      for (const element of tuple) {
        const problem = visit(element);
        if (problem) return problem;
      }
      return undefined;
    }
    const result = resultParts(current);
    if (result) return visit(result.ok) ?? visit(result.error);
    const callable = functionParts(current);
    if (callable) {
      for (const parameter of callable.parameters) {
        const problem = visit(parameter);
        if (problem) return problem;
      }
      return visit(callable.result);
    }
    const requirementKeys = contextKeys(current) ?? rowArgumentKeys(current);
    if (requirementKeys) {
      for (const key of requirementKeys) {
        for (const argument of nominalGenericParts(key)?.arguments ?? []) {
          const problem = visit(argument);
          if (problem) return problem;
        }
      }
      return undefined;
    }
    const nominal = nominalGenericParts(current);
    if (nominal) {
      const problem = writtenApplicationBoundProblem(nominal.name, nominal.arguments, scope);
      if (problem) return problem;
      for (const argument of nominal.arguments) {
        const nested = visit(argument);
        if (nested) return nested;
      }
    }
    return undefined;
  };
  return visit(type);
}

/**
 * Whether an enclosing bound written as `parameter < ...` requires the
 * trait: the name the declaration wrote, or a trait it extends, as
 * `T < Child` requires `Parent` (09-traits.md#r-trait.bound.supertraits).
 */
export function enclosingBoundImplies(
  bounds: readonly GenericBound[],
  traitTypes: ReadonlyMap<string, HirTrait>,
  parameter: string,
  traitName: string,
): boolean {
  if (
    bounds.some(
      (bound) =>
        bound.parameter === parameter &&
        bound.traits.some((source) => {
          const key = mutableInner(source) ?? source;
          return (nominalGenericParts(key)?.name ?? key) === traitName;
        }),
    )
  )
    return true;
  const required = traitTypes.get(traitName);
  if (required === undefined) return false;
  return (
    enclosingBoundProof(
      bounds.flatMap((bound) =>
        bound.parameter === parameter
          ? bound.traits.map((source) => {
              const key = mutableInner(source) ?? source;
              const nominal = nominalGenericParts(key);
              const name = nominal?.name ?? key;
              const trait = traitTypes.get(name);
              return {
                parameter,
                traitName: name,
                traitIndex: trait?.index ?? -1,
                traitArguments: nominal?.arguments ?? [],
                mutable: false,
              };
            })
          : [],
      ),
      traitTypes,
      parameter,
      required.index,
      [],
    ) !== undefined
  );
}

/**
 * The written-bound scope over a signature's bounds: an enclosing
 * parameter meets a required trait through the signature's bounds,
 * supertraits included, through the same lookup calls use. `boundBy`
 * answers the trait index the way the function checker does.
 */
export function signatureBoundScope(
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait>,
  genericParameters: readonly string[],
  boundBy: (parameter: string, traitIndex: number) => boolean,
): WrittenBoundScope {
  const implied = (parameter: string, traitName: string): boolean => {
    const trait = traitTypes.get(traitName);
    return trait !== undefined && boundBy(parameter, trait.index);
  };
  return {
    dataTypes,
    enumTypes,
    traitTypes,
    hashableParameters: new Set(
      genericParameters.filter(
        (parameter) => implied(parameter, "Eq") && implied(parameter, "Hash"),
      ),
    ),
    parameterImplied: implied,
  };
}

/** Report the first unsatisfied declared bound in a written type, if any. */
export function pushWrittenBoundProblem(
  diagnostics: Diagnostic[],
  span: SourceSpan,
  resolved: ValueType,
  scope: WrittenBoundScope,
): boolean {
  const problem = writtenBoundProblem(resolved, scope);
  if (!problem) return false;
  diagnostics.push({ ...problem, span });
  return true;
}
