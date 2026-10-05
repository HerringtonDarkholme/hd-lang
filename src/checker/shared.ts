import { writtenBindingProblem } from "./associated-bindings.ts";
import { markAmbiguous } from "./ambiguous-solutions.ts";
import {
  dynamicTraitProblemInType,
  enclosingBoundImplies,
  pushWrittenBoundProblem,
  restElementProblem,
} from "./written-type-validation.ts";
import { requirementKeyDiagnosticsInType, resolveRequirementKeyTypes } from "./requirement-keys.ts";
import type { Expression, GenericBound, Program, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { numericType, widensTo } from "../numeric.ts";
import type {
  HirData,
  HirDefaultArgument,
  HirEnum,
  HirExpression,
  HirGenericBound,
  HirTrait,
  HirTypeSubstitution,
  NumericFamily,
  ValueType,
} from "../hir.ts";
import type { Signature } from "./context-types.ts";
import { PRELUDE_NAMES } from "./prelude-names.ts";
import { matchGenericTypePattern } from "./generic-patterns.ts";
import { pureLiteralKind } from "./literal-join.ts";
import { familyHolds } from "./numeric-family.ts";
import { normalizedRequirements, rowParameterName, sameRequirements } from "./requirement-rows.ts";

export { matchGenericTypePattern } from "./generic-patterns.ts";
export { normalizedRequirements, rowParameterName, sameRequirements } from "./requirement-rows.ts";
import {
  contextKeys,
  contextType,
  functionParts,
  functionType,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
  readonlyType,
  resultParts,
  rowArgumentKeys,
  rowArgumentType,
  tupleParts,
  tupleType,
  CURSOR_TYPE,
  bindingParts,
  bindingType,
  splitTypeBindings,
  restInner,
  inputsInner,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  functionInputsTuple,
  type FunctionParts,
  displayType,
} from "../types.ts";

interface NamedParameter {
  readonly name: string;
}

interface TypeChildren {
  readonly head: string;
  readonly values: readonly ValueType[];
}

export interface BindingExpressionFlow {
  readonly all: ReadonlySet<string>;
  readonly always: ReadonlySet<string>;
  readonly whenFalse: ReadonlySet<string>;
  readonly whenTrue: ReadonlySet<string>;
}

interface IterableInfo {
  readonly iteratorFunctionIndex?: number;
  readonly iteratorKind: "iterator" | "list" | "map" | "trait";
  readonly yieldType: ValueType;
}

/**
 * How a loop advances `iterable`: a list or map through the built-in cursor,
 * and the prelude `Iterator` by calling its `next`, `iteratorNext`.
 */
export function iterableInfo(
  iterable: HirExpression,
  iteratorNext: number | undefined,
): IterableInfo | undefined {
  const nominal = nominalGenericParts(readonlyType(iterable.type));
  if (nominal?.name === "List" && nominal.arguments.length === 1)
    return { iteratorKind: "list", yieldType: nominal.arguments[0]! };
  if (nominal?.name === "Map" && nominal.arguments.length === 2)
    return { iteratorKind: "map", yieldType: tupleType(nominal.arguments) };
  if (nominal?.name === CURSOR_TYPE && nominal.arguments.length === 1)
    return { iteratorKind: "iterator", yieldType: nominal.arguments[0]! };
  if (nominal?.name !== "Iterator" || nominal.arguments.length !== 1 || iteratorNext === undefined)
    return undefined;
  return {
    iteratorKind: "trait",
    iteratorFunctionIndex: iteratorNext,
    yieldType: nominal.arguments[0]!,
  };
}

function unionNames(...sets: readonly ReadonlySet<string>[]): Set<string> {
  return new Set(sets.flatMap((set) => [...set]));
}

function intersectNames(left: ReadonlySet<string>, right: ReadonlySet<string>): Set<string> {
  return new Set([...left].filter((name) => right.has(name)));
}

function sequentialBindingFlow(expressions: readonly Expression[]): BindingExpressionFlow {
  const flows = expressions.map(bindingExpressionFlow);
  const all = unionNames(...flows.map((flow) => flow.all));
  const always = unionNames(...flows.map((flow) => flow.always));
  return { all, always, whenFalse: always, whenTrue: always };
}

export function bindingExpressionFlow(expression: Expression): BindingExpressionFlow {
  if (expression.kind === "binding-expression") {
    const nested = bindingExpressionFlow(expression.value);
    const names = new Set(expression.bindings.map((binding) => binding.name));
    return {
      all: unionNames(nested.all, names),
      always: unionNames(nested.always, names),
      whenFalse: unionNames(nested.whenFalse, names),
      whenTrue: unionNames(nested.whenTrue, names),
    };
  }
  if (expression.kind === "unary" && expression.operator === "not") {
    const operand = bindingExpressionFlow(expression.operand);
    return { ...operand, whenFalse: operand.whenTrue, whenTrue: operand.whenFalse };
  }
  if (expression.kind === "binary" && expression.operator === "and") {
    const left = bindingExpressionFlow(expression.left);
    const right = bindingExpressionFlow(expression.right);
    return {
      all: unionNames(left.all, right.all),
      always: intersectNames(left.whenFalse, unionNames(left.whenTrue, right.always)),
      whenFalse: intersectNames(left.whenFalse, unionNames(left.whenTrue, right.whenFalse)),
      whenTrue: unionNames(left.whenTrue, right.whenTrue),
    };
  }
  if (expression.kind === "binary" && expression.operator === "or") {
    const left = bindingExpressionFlow(expression.left);
    const right = bindingExpressionFlow(expression.right);
    return {
      all: unionNames(left.all, right.all),
      always: intersectNames(left.whenTrue, unionNames(left.whenFalse, right.always)),
      whenFalse: unionNames(left.whenFalse, right.whenFalse),
      whenTrue: intersectNames(left.whenTrue, unionNames(left.whenFalse, right.whenTrue)),
    };
  }
  return sequentialBindingFlow(eagerExpressionChildren(expression));
}

function eagerExpressionChildren(expression: Expression): readonly Expression[] {
  switch (expression.kind) {
    case "interpolated-string":
      return expression.segments.flatMap((segment) =>
        segment.kind === "expression" ? [segment.expression] : [],
      );
    case "list":
    case "tuple":
      return expression.elements;
    case "map":
      return expression.entries.flatMap((entry) => [entry.key, entry.value]);
    case "unary":
    case "propagate":
      return [expression.operand];
    case "binary":
      return [expression.left, expression.right];
    case "call":
    case "suspend-call":
      return [expression.callee, ...expression.arguments];
    case "data":
      return [
        ...(expression.spread ? [expression.spread] : []),
        ...expression.fields.map((field) => field.value),
      ];
    case "member":
      return [expression.receiver];
    case "index":
      return [expression.receiver, expression.index];
    case "provider-context":
      return expression.entries.map((entry) => entry.value);
    case "provider-with":
      return expression.entries.map((entry) => entry.value);
    case "if":
    case "while":
      return [expression.condition];
    case "for":
      return [expression.iterable];
    case "match":
      return [expression.subject];
    case "pipe":
      return [expression.value];
    case "range":
      return [
        ...(expression.start ? [expression.start] : []),
        ...(expression.end ? [expression.end] : []),
      ];
    case "integer":
    case "float":
    case "string":
    case "character":
    case "boolean":
    case "name":
    case "qualified-name":
    case "contextual-variant":
    case "closure":
    case "list-comprehension":
    case "map-comprehension":
    case "binding-expression":
    case "provider-use":
      return [];
  }
}

const TYPE_NAMES = new Set<ValueType>([
  "i8",
  "i16",
  "u16",
  "u32",
  "u64",
  "f32",
  "i32",
  "i64",
  "u8",
  "bool",
  "f64",
  "char",
  "string",
  "void",
  "never",
]);

export function isKnownType(
  type: ValueType,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait> = new Map(),
): boolean {
  const binding = bindingParts(type);
  if (binding) return isKnownType(binding.type, dataTypes, enumTypes, traitTypes);
  const rest = restInner(type) ?? inputsInner(type);
  if (rest !== undefined) return isKnownType(rest, dataTypes, enumTypes, traitTypes);
  const mutable = mutableInner(type);
  if (mutable !== undefined)
    return mutable !== "void" && isKnownType(mutable, dataTypes, enumTypes, traitTypes);
  if (genericTypeName(type) || rowArgumentKeys(type)) return true;
  if (TYPE_NAMES.has(type)) return true;
  const plainData = dataTypes.get(type);
  if (plainData) return plainData.genericParameters.length === 0;
  const plainEnum = enumTypes.get(type);
  if (plainEnum) return plainEnum.genericParameters.length === 0;
  if (type.startsWith("trait:") && !type.endsWith("?")) {
    const key = type.slice("trait:".length);
    const nominalTrait = nominalGenericParts(key);
    const trait = traitTypes.get(nominalTrait?.name ?? key);
    if (!trait) return false;
    if (!nominalTrait) return trait.genericParameters.length === 0;
    return (
      trait.genericParameters.length ===
        splitTypeBindings(nominalTrait.arguments).positional.length &&
      nominalTrait.arguments.every((argument) =>
        isKnownType(argument, dataTypes, enumTypes, traitTypes),
      )
    );
  }
  if (type.startsWith("provider:")) return true;
  if (contextKeys(type)) return true;
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tuple.every((element) => isKnownType(element, dataTypes, enumTypes, traitTypes));
  const optional = optionalInner(type);
  if (optional !== undefined) return isKnownType(optional, dataTypes, enumTypes, traitTypes);
  const result = resultParts(type);
  if (result)
    return (
      isKnownType(result.ok, dataTypes, enumTypes, traitTypes) &&
      isKnownType(result.error, dataTypes, enumTypes, traitTypes)
    );
  const nominal = nominalGenericParts(type);
  if (nominal) {
    if (nominal.name === "Suspend") {
      return (
        nominal.arguments.length === 1 &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    if (nominal.name === CURSOR_TYPE) {
      return (
        nominal.arguments.length === 1 &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    if (nominal.name === "List") {
      return (
        nominal.arguments.length === 1 &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    if (nominal.name === "Map") {
      return (
        nominal.arguments.length === 2 &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes) &&
        isKnownType(nominal.arguments[1]!, dataTypes, enumTypes, traitTypes)
      );
    }
    const declaration = dataTypes.get(nominal.name);
    if (declaration) {
      return (
        declaration.genericParameters.length === nominal.arguments.length &&
        nominal.arguments.every((argument) =>
          isKnownType(argument, dataTypes, enumTypes, traitTypes),
        )
      );
    }
    const enumDeclaration = enumTypes.get(nominal.name);
    return Boolean(
      enumDeclaration &&
      enumDeclaration.genericParameters.length === nominal.arguments.length &&
      nominal.arguments.every((argument) =>
        isKnownType(argument, dataTypes, enumTypes, traitTypes),
      ),
    );
  }
  const callable = functionParts(type);
  return Boolean(
    callable &&
    callable.parameters.every((parameter) =>
      isKnownType(parameter, dataTypes, enumTypes, traitTypes),
    ) &&
    isKnownType(callable.result, dataTypes, enumTypes, traitTypes),
  );
}

export function statementsReferenceName(statements: readonly Statement[], name: string): boolean {
  return statements.some((statement) => nodeReferencesName(statement, name));
}

function nodeReferencesName(value: unknown, name: string): boolean {
  if (Array.isArray(value)) return value.some((item) => nodeReferencesName(item, name));
  if (!value || typeof value !== "object") return false;
  const node = value as Record<string, unknown>;
  if (node.kind === "name" && node.name === name) return true;
  if (node.kind === "closure") {
    const parameters = node.parameters as readonly NamedParameter[];
    if (parameters.some((parameter) => parameter.name === name)) return false;
  }
  return Object.entries(node).some(
    ([key, child]) => key !== "span" && nodeReferencesName(child, name),
  );
}

export function substituteGenericType(
  type: ValueType,
  substitutions: ReadonlyMap<string, ValueType>,
  rowSubstitutions: ReadonlyMap<string, readonly string[]> = new Map(),
): ValueType {
  const binding = bindingParts(type);
  if (binding)
    return bindingType(
      binding,
      substituteGenericType(binding.type, substitutions, rowSubstitutions),
    );
  const inputs = inputsInner(type);
  if (inputs !== undefined)
    return `*${substituteGenericType(inputs, substitutions, rowSubstitutions)}`;
  const rest = restInner(type);
  if (rest !== undefined)
    return `${substituteGenericType(rest, substitutions, rowSubstitutions)}...`;
  const mutable = mutableInner(type);
  if (mutable !== undefined)
    return mutableType(substituteGenericType(mutable, substitutions, rowSubstitutions));
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tupleType(
      tuple.map((element) => substituteGenericType(element, substitutions, rowSubstitutions)),
    );
  const optional = optionalInner(type);
  if (optional !== undefined)
    return optionalType(substituteGenericType(optional, substitutions, rowSubstitutions));
  const result = resultParts(type);
  if (result)
    return `Result[${substituteGenericType(result.ok, substitutions, rowSubstitutions)},${substituteGenericType(result.error, substitutions, rowSubstitutions)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal)
    return nominalGenericType(
      nominal.name,
      nominal.arguments.map((argument) =>
        substituteGenericType(argument, substitutions, rowSubstitutions),
      ),
    );
  const callable = functionParts(type);
  if (callable) {
    return functionType(
      callable.parameters.map((parameter) =>
        substituteGenericType(parameter, substitutions, rowSubstitutions),
      ),
      substituteGenericType(callable.result, substitutions, rowSubstitutions),
      callable.requirements.flatMap((requirement) => {
        const row = rowParameterName(requirement);
        return row
          ? (rowSubstitutions.get(row) ?? rowArgumentKeys(substitutions.get(row)) ?? [requirement])
          : [substituteGenericType(requirement, substitutions, rowSubstitutions)];
      }),
      callable.variadic,
      callable.suspending,
    );
  }
  const generic = genericTypeName(type);
  if (generic) return substitutions.get(generic) ?? type;
  return type;
}

/** Preserve a declaration's binder identities at an erased storage boundary. */
export function orderedTypeSubstitutions(
  parameters: readonly string[],
  substitutions: ReadonlyMap<string, ValueType>,
): readonly HirTypeSubstitution[] {
  return parameters.flatMap((parameter) => {
    const type = substitutions.get(parameter);
    return type === undefined ? [] : [{ parameter, type }];
  });
}

/** The `Args` of a function type `Fn[Args, O, $ R]` whose inputs are a type parameter. */
function tupleInputs(callable: FunctionParts): ValueType | undefined {
  return callable.parameters.length === 1 ? inputsInner(callable.parameters[0]!) : undefined;
}

export function genericTypeName(type: ValueType): string | undefined {
  if (!type.startsWith("generic:")) return undefined;
  const match = /^generic:([^?[\](),]+)$/.exec(type);
  return match?.[1];
}

/**
 * The type an argument has before its expected type converted it: the
 * operand of a numeric widening or of a trait-value conversion. A numeric
 * literal takes its expected type directly, so it has no type of its own
 * here (04-type-system.md#inference-from-several-arguments).
 */
/**
 * An unsuffixed numeric literal argument, possibly negated: its type comes
 * from the parameter it fills, so it never solves a type parameter that
 * another argument solves (types.literal.local.form.argument).
 */
export function isNumericLiteralArgument(source: Expression): boolean {
  return pureLiteralKind(source) !== undefined;
}

export function argumentOwnType(source: Expression, checked: HirExpression): ValueType | undefined {
  if (isNumericLiteralArgument(source)) return undefined;
  if (checked.type === "never") return undefined;
  if (checked.kind === "unary" && checked.operator === "widen") return checked.operand.type;
  if (checked.kind === "trait-wrap" || checked.kind === "trait-bound") return checked.value.type;
  return checked.type;
}

export { traitTypeName } from "./written-type-validation.ts";

export function traitKeyName(key: string): string {
  return nominalGenericParts(key)?.name ?? key;
}

export function requirementKeysMayCollide(left: string, right: string): boolean {
  if (left === right) return false;
  const substitutions = new Map<string, ValueType>();
  const resolve = (type: ValueType): ValueType => {
    const generic = genericTypeName(type);
    const substitution = generic && substitutions.get(generic);
    return substitution ? resolve(substitution) : type;
  };
  const children = (type: ValueType): TypeChildren => {
    // A key's bindings take part as its type arguments do (req.with.collision.bindings).
    const binding = bindingParts(type);
    if (binding) return { head: `binding:${binding.name}`, values: [binding.type] };
    const rest = restInner(type);
    if (rest !== undefined) return { head: "rest", values: [rest] };
    const mutable = mutableInner(type);
    if (mutable !== undefined) return { head: "mutable", values: [mutable] };
    const tuple = tupleParts(type);
    if (tuple !== undefined) return { head: `tuple:${tuple.length}`, values: tuple };
    const optional = optionalInner(type);
    if (optional !== undefined) return { head: "optional", values: [optional] };
    const result = resultParts(type);
    if (result) return { head: "Result", values: [result.ok, result.error] };
    const nominal = nominalGenericParts(type);
    if (nominal) return { head: `nominal:${nominal.name}`, values: nominal.arguments };
    const callable = functionParts(type);
    // The inputs take part as one tuple, so `Fn[Args, O, $ R]` unifies with
    // any arity; a lone row parameter unifies with any row.
    if (callable) {
      const row = callable.requirements.length === 1 && rowParameterName(callable.requirements[0]!);
      return {
        head: `function:${callable.suspending}`,
        values: [
          functionInputsTuple(callable),
          callable.result,
          row ? `generic:${row}` : rowArgumentType(callable.requirements),
        ],
      };
    }
    return { head: `plain:${type}`, values: [] };
  };
  const occurs = (name: string, type: ValueType): boolean => {
    const resolved = resolve(type);
    if (genericTypeName(resolved) === name) return true;
    return children(resolved).values.some((child) => occurs(name, child));
  };
  const unifyTypes = (firstType: ValueType, secondType: ValueType): boolean => {
    const first = resolve(firstType);
    const second = resolve(secondType);
    if (first === second) return true;
    // A rest element unifies only with a rest element (types.tuple.rest.same).
    if ((restInner(first) === undefined) !== (restInner(second) === undefined)) return false;
    const firstGeneric = genericTypeName(first);
    if (firstGeneric) {
      if (occurs(firstGeneric, second)) return false;
      substitutions.set(firstGeneric, second);
      return true;
    }
    const secondGeneric = genericTypeName(second);
    if (secondGeneric) {
      if (occurs(secondGeneric, first)) return false;
      substitutions.set(secondGeneric, first);
      return true;
    }
    const firstChildren = children(first);
    const secondChildren = children(second);
    return (
      firstChildren.head === secondChildren.head &&
      firstChildren.values.length === secondChildren.values.length &&
      firstChildren.values.every((child, index) => unifyTypes(child, secondChildren.values[index]!))
    );
  };
  return unifyTypes(left, right);
}

/**
 * Why a generic call's result, whose type arguments the expected type could
 * not solve, does not fit that expected type: it has the same constructor,
 * so its arguments differ, as `NumSuffix[fn(N) -> R]` and
 * `NumSuffix[fn(i64, i64) -> i64]` do.
 */
export function resultMisfit(
  result: ValueType,
  expected: ValueType | undefined,
): string | undefined {
  const head = nominalGenericParts(readonlyType(result))?.name;
  if (expected === undefined || head === undefined) return undefined;
  if (head !== nominalGenericParts(readonlyType(expected))?.name) return undefined;
  return `expected '${displayType(expected)}', but the call returns '${displayType(result)}'`;
}

/**
 * Whether `type` still names one of a callee's unsolved type or row
 * parameters. A `generic:` or `row:` name of the caller is a known type.
 */
export function mentionsUnsolved(
  type: ValueType,
  callee: {
    readonly genericParameters: readonly string[];
    readonly rowParameters: readonly string[];
  },
  substitutions: ReadonlyMap<string, ValueType>,
  rowSubstitutions: ReadonlyMap<string, readonly string[]>,
): boolean {
  for (const match of type.matchAll(/\b(generic|row):([A-Za-z_]\w*)/g)) {
    const name = match[2]!;
    const unsolved =
      match[1] === "generic"
        ? callee.genericParameters.includes(name) && !substitutions.has(name)
        : callee.rowParameters.includes(name) && !rowSubstitutions.has(name);
    if (unsolved) return true;
  }
  return false;
}

export function containsGenericType(type: ValueType): boolean {
  if (genericTypeName(type)) return true;
  const inputs = inputsInner(type) ?? restInner(type);
  if (inputs !== undefined) return containsGenericType(inputs);
  const binding = bindingParts(type);
  if (binding) return containsGenericType(binding.type);
  const mutable = mutableInner(type);
  if (mutable !== undefined) return containsGenericType(mutable);
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tuple.some(containsGenericType);
  const optional = optionalInner(type);
  if (optional !== undefined) return containsGenericType(optional);
  const result = resultParts(type);
  if (result) return containsGenericType(result.ok) || containsGenericType(result.error);
  const nominal = nominalGenericParts(type);
  if (nominal) return nominal.arguments.some(containsGenericType);
  const callable = functionParts(type);
  return Boolean(
    callable &&
    (callable.parameters.some(containsGenericType) ||
      containsGenericType(callable.result) ||
      callable.requirements.some(
        (requirement) => !rowParameterName(requirement) && containsGenericType(requirement),
      )),
  );
}

/**
 * Every generic parameter name `type` mentions, in one structural walk.
 * Collecting the names up front turns per-parameter scans into one pass
 * with set lookups.
 */
export function mentionedGenericParameters(type: ValueType): Set<string> {
  const names = new Set<string>();
  const visit = (node: ValueType): void => {
    const leaf = genericTypeName(node);
    if (leaf !== undefined) {
      names.add(leaf);
      return;
    }
    const inputs = inputsInner(node) ?? restInner(node);
    if (inputs !== undefined) {
      visit(inputs);
      return;
    }
    const binding = bindingParts(node);
    if (binding) {
      visit(binding.type);
      return;
    }
    const mutable = mutableInner(node);
    if (mutable !== undefined) {
      visit(mutable);
      return;
    }
    const tuple = tupleParts(node);
    if (tuple !== undefined) {
      tuple.forEach(visit);
      return;
    }
    const optional = optionalInner(node);
    if (optional !== undefined) {
      visit(optional);
      return;
    }
    const result = resultParts(node);
    if (result) {
      visit(result.ok);
      visit(result.error);
      return;
    }
    const nominal = nominalGenericParts(node);
    if (nominal) {
      nominal.arguments.forEach(visit);
      return;
    }
    const callable = functionParts(node);
    if (callable) {
      callable.parameters.forEach(visit);
      visit(callable.result);
      callable.requirements.forEach((requirement) => {
        if (!rowParameterName(requirement)) visit(requirement);
      });
    }
  };
  visit(type);
  return names;
}

function inferRequirementTypeArguments(
  formal: readonly string[],
  actual: readonly string[],
  substitutions: Map<string, ValueType>,
  rowSubstitutions: Map<string, readonly string[]>,
): string | undefined {
  const patterns = formal.filter(
    (requirement) => !rowParameterName(requirement) && containsGenericType(requirement),
  );
  const actualKeys = actual.filter((requirement) => !rowParameterName(requirement));
  const solutions: Array<{
    readonly types: Map<string, ValueType>;
    readonly rows: Map<string, readonly string[]>;
  }> = [];

  const finish = (types: Map<string, ValueType>): void => {
    const rows = new Map(rowSubstitutions);
    const instantiated = formal.map((requirement) =>
      rowParameterName(requirement) ? requirement : substituteGenericType(requirement, types, rows),
    );
    const hasRow = instantiated.some(rowParameterName);
    if (!hasRow && !sameRequirements(instantiated, actual)) return;
    if (hasRow && inferRequirementRows(instantiated, actual, rows) !== undefined) return;
    solutions.push({ types, rows });
  };

  const visit = (index: number, types: Map<string, ValueType>): void => {
    const pattern = patterns[index];
    if (pattern === undefined) {
      finish(types);
      return;
    }
    const instantiated = substituteGenericType(pattern, types);
    if (!containsGenericType(instantiated)) {
      visit(index + 1, types);
      return;
    }
    const candidates = actualKeys.flatMap((key) => {
      const inferred = new Map(types);
      return matchGenericTypePattern(instantiated, key, inferred) ? [inferred] : [];
    });
    // In `$ R + K`, K may be absent; the least row solution then retains the
    // whole actual row in R (req.row.least.absent-key).
    if (candidates.length === 0 && formal.some(rowParameterName)) {
      visit(index + 1, types);
      return;
    }
    for (const inferred of candidates) visit(index + 1, inferred);
  };

  visit(0, new Map(substitutions));
  const unique = new Map<
    string,
    { readonly types: Map<string, ValueType>; readonly rows: Map<string, readonly string[]> }
  >();
  for (const solution of solutions) {
    const key = JSON.stringify({
      types: [...solution.types].sort(([left], [right]) => left.localeCompare(right)),
      rows: [...solution.rows]
        .map(([name, row]) => [name, normalizedRequirements(row)] as const)
        .sort(([left], [right]) => left.localeCompare(right)),
    });
    unique.set(key, solution);
  }
  if (unique.size > 1)
    markAmbiguous(
      substitutions,
      [...unique.values()].flatMap(({ types }) =>
        [...types.keys()].filter((name) => !substitutions.has(name)),
      ),
    );
  if (unique.size !== 1) return undefined;
  const solution = unique.values().next().value!;
  for (const [name, type] of solution.types) substitutions.set(name, type);
  for (const [name, row] of solution.rows) rowSubstitutions.set(name, row);
  return undefined;
}

export function inferGenericType(
  formal: ValueType,
  actual: ValueType,
  substitutions: Map<string, ValueType>,
  rowSubstitutions: Map<string, readonly string[]> = new Map(),
): string | undefined {
  const formalBinding = bindingParts(formal);
  const actualBinding = bindingParts(actual);
  if (formalBinding || actualBinding)
    return formalBinding && actualBinding && formalBinding.name === actualBinding.name
      ? inferGenericType(formalBinding.type, actualBinding.type, substitutions, rowSubstitutions)
      : undefined;
  const formalRest = restInner(formal);
  const actualRest = restInner(actual);
  if (formalRest !== undefined || actualRest !== undefined)
    return formalRest !== undefined && actualRest !== undefined
      ? inferGenericType(formalRest, actualRest, substitutions, rowSubstitutions)
      : undefined;
  const generic = genericTypeName(formal);
  if (generic) {
    const existing = substitutions.get(generic);
    // A `mut T` argument weakens to a parameter already inferred as `T`.
    if (existing && mutableInner(actual) === existing) return undefined;
    if (existing && existing !== actual)
      return `generic parameter '${generic}' was inferred as both ${displayType(existing)} and ${displayType(actual)}`;
    substitutions.set(generic, actual);
    return undefined;
  }
  // A cold call converts to the stored `mut Suspend[T]` of its result
  // (11-requirements-and-suspension.md#r-req.combinator.race-signature).
  const formalStored = storedSuspensionParts(formal);
  const actualCold = suspensionParts(actual) ?? traitSuspensionParts(actual);
  if (formalStored && actualCold)
    return inferGenericType(
      formalStored.result,
      actualCold.result,
      substitutions,
      rowSubstitutions,
    );
  const formalMutable = mutableInner(formal);
  const actualMutable = mutableInner(actual);
  if (formalMutable !== undefined && actualMutable !== undefined) {
    return inferGenericType(formalMutable, actualMutable, substitutions, rowSubstitutions);
  }
  const formalTuple = tupleParts(formal);
  const actualTuple = tupleParts(actual);
  if (
    formalTuple !== undefined &&
    actualTuple !== undefined &&
    formalTuple.length === actualTuple.length
  ) {
    for (let index = 0; index < formalTuple.length; index += 1) {
      const conflict = inferGenericType(
        formalTuple[index]!,
        actualTuple[index]!,
        substitutions,
        rowSubstitutions,
      );
      if (conflict) return conflict;
    }
    return undefined;
  }
  const formalOptional = optionalInner(formal);
  const actualOptional = optionalInner(actual);
  if (formalOptional !== undefined && actualOptional !== undefined)
    return inferGenericType(formalOptional, actualOptional, substitutions, rowSubstitutions);
  const formalResult = resultParts(formal);
  const actualResult = resultParts(actual);
  if (formalResult && actualResult) {
    return (
      inferGenericType(formalResult.ok, actualResult.ok, substitutions, rowSubstitutions) ??
      inferGenericType(formalResult.error, actualResult.error, substitutions, rowSubstitutions)
    );
  }
  const formalNominal = nominalGenericParts(formal);
  const actualNominal = nominalGenericParts(actual);
  if (
    formalNominal &&
    actualNominal &&
    formalNominal.name === actualNominal.name &&
    formalNominal.arguments.length === actualNominal.arguments.length
  ) {
    for (let index = 0; index < formalNominal.arguments.length; index += 1) {
      const conflict = inferGenericType(
        formalNominal.arguments[index]!,
        actualNominal.arguments[index]!,
        substitutions,
        rowSubstitutions,
      );
      if (conflict) return conflict;
    }
    return undefined;
  }
  const formalCallable = functionParts(formal);
  const actualCallable = functionParts(actual);
  // `Fn[Args, O, $ R]` solves `Args` as the tuple of the actual inputs
  // (07-functions.md#r-fn.type.ctor.inputs).
  const formalInputs = formalCallable && tupleInputs(formalCallable);
  if (formalCallable && actualCallable && formalInputs !== undefined) {
    if (formalCallable.suspending !== actualCallable.suspending) return undefined;
    const conflict = inferGenericType(
      formalInputs,
      functionInputsTuple(actualCallable),
      substitutions,
      rowSubstitutions,
    );
    if (conflict) return conflict;
    return (
      inferGenericType(
        formalCallable.result,
        actualCallable.result,
        substitutions,
        rowSubstitutions,
      ) ??
      inferRequirementTypeArguments(
        formalCallable.requirements,
        actualCallable.requirements,
        substitutions,
        rowSubstitutions,
      )
    );
  }
  if (
    formalCallable &&
    actualCallable &&
    formalCallable.suspending === actualCallable.suspending &&
    formalCallable.variadic === actualCallable.variadic &&
    formalCallable.parameters.length === actualCallable.parameters.length
  ) {
    for (let index = 0; index < formalCallable.parameters.length; index += 1) {
      const conflict = inferGenericType(
        formalCallable.parameters[index]!,
        actualCallable.parameters[index]!,
        substitutions,
        rowSubstitutions,
      );
      if (conflict) return conflict;
    }
    const resultConflict = inferGenericType(
      formalCallable.result,
      actualCallable.result,
      substitutions,
      rowSubstitutions,
    );
    return (
      resultConflict ??
      inferRequirementTypeArguments(
        formalCallable.requirements,
        actualCallable.requirements,
        substitutions,
        rowSubstitutions,
      )
    );
  }
  return undefined;
}

interface TraitImplementationPattern {
  readonly targetType: ValueType;
  readonly traitArguments: readonly ValueType[];
  readonly trait?: { readonly index: number };
  readonly traitIndex?: number;
  readonly family?: NumericFamily;
}

/**
 * Whether an implementation's target matches `type`, adding the parameters
 * it solves to `substitutions`. A numeric-family target matches only a type
 * its bound lists.
 */
export function matchImplementationTarget(
  implementation: { readonly targetType: ValueType; readonly family?: NumericFamily },
  type: ValueType,
  substitutions: Map<string, ValueType>,
): boolean {
  return (
    matchGenericTypePattern(implementation.targetType, type, substitutions) &&
    familyHolds(implementation.family, substitutions, false)
  );
}

export function matchTraitImplementation(
  implementation: TraitImplementationPattern,
  traitIndex: number,
  targetType: ValueType,
  traitArguments: readonly ValueType[],
): Map<string, ValueType> | undefined {
  if (
    (implementation.traitIndex ?? implementation.trait?.index) !== traitIndex ||
    implementation.traitArguments.length !== traitArguments.length
  )
    return undefined;
  const substitutions = new Map<string, ValueType>();
  if (!matchGenericTypePattern(implementation.targetType, targetType, substitutions))
    return undefined;
  return implementation.traitArguments.every((argument, index) =>
    matchGenericTypePattern(argument, traitArguments[index]!, substitutions),
  ) && familyHolds(implementation.family, substitutions, true)
    ? substitutions
    : undefined;
}

/**
 * The implementation of a trait for `type`, and the type it matched. An
 * implementation for `X` also serves `mut X` through its readonly view.
 */
export function findImpl<T extends TraitImplementationPattern>(
  implementations: readonly T[],
  traitIndex: number,
  type: ValueType,
  traitArguments: readonly ValueType[],
): { readonly impl: T; readonly type: ValueType } | undefined {
  for (const candidate of [type, readonlyType(type)]) {
    const implementation = implementations.find((item) =>
      matchTraitImplementation(item, traitIndex, candidate, traitArguments),
    );
    if (implementation) return { impl: implementation, type: candidate };
  }
  return undefined;
}

function symbolicRequirement(name: string): string {
  return `row:${name}`;
}

function inferRequirementRows(
  formal: readonly string[],
  actual: readonly string[],
  substitutions: Map<string, readonly string[]>,
): string | undefined {
  const rowNames = [
    ...new Set(formal.map(rowParameterName).filter((name): name is string => name !== undefined)),
  ];
  if (rowNames.length === 0) return undefined;
  const concrete = formal.filter((requirement) => !rowParameterName(requirement));
  // A callback whose row lacks a concrete key of the pattern still matches,
  // with the row parameter set to its own row
  // (11-requirements-and-suspension.md#r-req.row.least.absent-key).
  const actualSet = new Set(actual);
  const present = concrete.filter((requirement) => actualSet.has(requirement));
  const boundRequirements = formal.flatMap((requirement) => {
    const name = rowParameterName(requirement);
    return name ? (substitutions.get(name) ?? []) : [];
  });
  const unbound = rowNames.filter((name) => !substitutions.has(name));
  if (unbound.length > 1) return undefined;
  if (unbound.length === 1) {
    const occupied = new Set([...concrete, ...boundRequirements]);
    substitutions.set(
      unbound[0]!,
      normalizedRequirements(actual.filter((requirement) => !occupied.has(requirement))),
    );
  }
  const instantiated = normalizedRequirements([
    ...present,
    ...formal.flatMap((requirement) => {
      const name = rowParameterName(requirement);
      return name ? (substitutions.get(name) ?? []) : [];
    }),
  ]);
  if (!sameRequirements(instantiated, actual)) {
    const names = rowNames.map((name) => `'${name}'`).join(" and ");
    return `requirement-row parameter${rowNames.length === 1 ? "" : "s"} ${names} cannot match both ${writtenRow(instantiated)} and ${writtenRow(normalizedRequirements(actual))}`;
  }
  return undefined;
}

/** A row as source writes it: `$()` or `$ A + B` (02-grammar.md#types). */
function writtenRow(keys: readonly string[]): string {
  return keys.length === 0 ? "$()" : `$ ${keys.join(" + ")}`;
}

/**
 * True when `actual` is the instantiated row pattern `instantiated` of
 * `formal` except that its row lacks some concrete keys of the pattern, as a
 * callback lacking the discharged key of `$ R + K` does
 * (11-requirements-and-suspension.md#r-req.row.least.absent-key).
 */
export function lacksOnlyPatternKeys(
  formal: ValueType,
  instantiated: ValueType,
  actual: ValueType,
): boolean {
  const pattern = functionParts(formal);
  const wide = functionParts(instantiated);
  const narrow = functionParts(actual);
  if (!pattern || !wide || !narrow || !pattern.requirements.some(rowParameterName)) return false;
  if (
    wide.suspending !== narrow.suspending ||
    wide.variadic !== narrow.variadic ||
    wide.result !== narrow.result ||
    wide.parameters.length !== narrow.parameters.length ||
    !wide.parameters.every((parameter, index) => parameter === narrow.parameters[index])
  )
    return false;
  const present = new Set(narrow.requirements);
  const concrete = new Set(pattern.requirements.filter((key) => !rowParameterName(key)));
  return (
    narrow.requirements.every((key) => wide.requirements.includes(key)) &&
    wide.requirements.every((key) => present.has(key) || concrete.has(key))
  );
}

export function functionTypeMatchesRowPattern(
  formalType: ValueType,
  actualType: ValueType,
): boolean {
  const formal = functionParts(formalType);
  const actual = functionParts(actualType);
  if (
    !formal ||
    !actual ||
    formal.variadic !== actual.variadic ||
    formal.suspending !== actual.suspending ||
    formal.parameters.length !== actual.parameters.length
  )
    return false;
  if (
    !formal.parameters.every((parameter, index) => parameter === actual.parameters[index]) ||
    formal.result !== actual.result
  )
    return false;
  return inferRequirementRows(formal.requirements, actual.requirements, new Map()) === undefined;
}

export function resolveGenericType(
  type: ValueType,
  genericParameters: ReadonlySet<string>,
  rowParameters: ReadonlySet<string> = new Set(),
): ValueType {
  if (genericParameters.has(type)) return `generic:${type}`;
  const binding = bindingParts(type);
  if (binding)
    return bindingType(binding, resolveGenericType(binding.type, genericParameters, rowParameters));
  const rest = restInner(type);
  if (rest !== undefined) return `${resolveGenericType(rest, genericParameters, rowParameters)}...`;
  const inputs = inputsInner(type);
  if (inputs !== undefined)
    return `*${resolveGenericType(inputs, genericParameters, rowParameters)}`;
  const row = rowArgumentKeys(type);
  if (row)
    return rowArgumentType(
      row.flatMap((requirement) => resolveGenericRequirement(requirement, rowParameters)),
    );
  const context = contextKeys(type);
  if (context)
    return contextType(
      context.flatMap((requirement) => resolveGenericRequirement(requirement, rowParameters)),
    );
  const projection = /^([^:]+)::([A-Za-z_][A-Za-z0-9_]*)$/.exec(type);
  if (projection && genericParameters.has(projection[1]!)) return `generic:${type}`;
  const mutable = mutableInner(type);
  if (mutable !== undefined)
    return mutableType(resolveGenericType(mutable, genericParameters, rowParameters));
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tupleType(
      tuple.map((element) => resolveGenericType(element, genericParameters, rowParameters)),
    );
  const optional = optionalInner(type);
  if (optional !== undefined)
    return optionalType(resolveGenericType(optional, genericParameters, rowParameters));
  const result = resultParts(type);
  if (result)
    return `Result[${resolveGenericType(result.ok, genericParameters, rowParameters)},${resolveGenericType(result.error, genericParameters, rowParameters)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal)
    return nominalGenericType(
      nominal.name,
      nominal.arguments.map((argument) =>
        resolveGenericType(argument, genericParameters, rowParameters),
      ),
    );
  const callable = functionParts(type);
  if (callable) {
    return functionType(
      callable.parameters.map((parameter) =>
        resolveGenericType(parameter, genericParameters, rowParameters),
      ),
      resolveGenericType(callable.result, genericParameters, rowParameters),
      callable.requirements.flatMap((requirement) => {
        const resolved = resolveGenericRequirement(requirement, rowParameters);
        return resolved.map((key) =>
          rowParameterName(key) ? key : resolveGenericType(key, genericParameters, rowParameters),
        );
      }),
      callable.variadic,
      callable.suspending,
    );
  }
  return type;
}

export function collectRowParameterReferences(
  type: ValueType,
  genericParameters: ReadonlySet<string>,
  output: Set<string>,
): void {
  const mutable = mutableInner(type);
  if (mutable !== undefined) {
    collectRowParameterReferences(mutable, genericParameters, output);
    return;
  }
  const tuple = tupleParts(type);
  if (tuple !== undefined) {
    tuple.forEach((element) => collectRowParameterReferences(element, genericParameters, output));
    return;
  }
  const callable = functionParts(type);
  if (callable) {
    for (const requirement of callable.requirements) {
      if (genericParameters.has(requirement)) output.add(requirement);
    }
    callable.parameters.forEach((parameter) =>
      collectRowParameterReferences(parameter, genericParameters, output),
    );
    collectRowParameterReferences(callable.result, genericParameters, output);
    return;
  }
  const optional = optionalInner(type);
  if (optional !== undefined) collectRowParameterReferences(optional, genericParameters, output);
  const result = resultParts(type);
  if (result) {
    collectRowParameterReferences(result.ok, genericParameters, output);
    collectRowParameterReferences(result.error, genericParameters, output);
    return;
  }
  // A row used inside a type argument, as `Q` of `NumSuffix[fn(N) -> R $ Q]`.
  for (const argument of nominalGenericParts(type)?.arguments ?? [])
    collectRowParameterReferences(argument, genericParameters, output);
}

export function resolveGenericRequirement(
  requirement: string,
  rowParameters: ReadonlySet<string>,
): readonly string[] {
  return rowParameters.has(requirement) ? [symbolicRequirement(requirement)] : [requirement];
}

export function firstPrivateSignatureType(type: ValueType, program: Program): string | undefined {
  const binding = bindingParts(type);
  if (binding) return firstPrivateSignatureType(binding.type, program);
  const mutable = mutableInner(type);
  if (mutable !== undefined) return firstPrivateSignatureType(mutable, program);
  const tuple = tupleParts(type);
  if (tuple !== undefined) {
    for (const element of tuple) {
      const privateType = firstPrivateSignatureType(element, program);
      if (privateType) return privateType;
    }
    return undefined;
  }
  const optional = optionalInner(type);
  if (optional !== undefined) return firstPrivateSignatureType(optional, program);
  const result = resultParts(type);
  if (result)
    return (
      firstPrivateSignatureType(result.ok, program) ??
      firstPrivateSignatureType(result.error, program)
    );
  const callable = functionParts(type);
  if (callable) {
    for (const parameter of callable.parameters) {
      const privateType = firstPrivateSignatureType(parameter, program);
      if (privateType) return privateType;
    }
    return firstPrivateSignatureType(callable.result, program);
  }
  const nominal = nominalGenericParts(type);
  const base = nominal?.name ?? type;
  // A program declaration of a prelude name is a `prelude-name-shadow`
  // error and declares nothing (10-modules.md#r-module.prelude.no-shadow).
  const named = (candidate: { readonly name: string; readonly standard?: boolean }): boolean =>
    candidate.name === base && (candidate.standard === true || !PRELUDE_NAMES.has(base));
  const declaration =
    program.data.find(named) ?? program.enums.find(named) ?? program.traits.find(named);
  if (declaration && !declaration.public) return base;
  if (nominal) {
    for (const argument of nominal.arguments) {
      const privateType = firstPrivateSignatureType(argument, program);
      if (privateType) return privateType;
    }
  }
  return undefined;
}

/**
 * Rewrites each argument of a row-kinded data parameter to the canonical row
 * `$(A+B)`, reading a single requirement key or an enclosing row parameter as
 * a row (02-grammar.md#types). A row given for a type-kinded parameter is a
 * kind mismatch.
 */
export function normalizeRowArguments(
  type: ValueType,
  dataTypes: ReadonlyMap<string, HirData>,
  _rowParameters: ReadonlySet<string>,
): ValueType | { readonly mismatch: string } {
  let mismatch: string | undefined;
  const visit = (current: ValueType): ValueType => {
    const binding = bindingParts(current);
    if (binding) return bindingType(binding, visit(binding.type));
    const mutable = mutableInner(current);
    if (mutable !== undefined) return mutableType(visit(mutable));
    const tuple = tupleParts(current);
    if (tuple !== undefined) return tupleType(tuple.map(visit));
    const optional = optionalInner(current);
    if (optional !== undefined) return optionalType(visit(optional));
    const callable = functionParts(current);
    if (callable)
      return functionType(
        callable.parameters.map(visit),
        visit(callable.result),
        callable.requirements,
        callable.variadic,
        callable.suspending,
      );
    const nominal = nominalGenericParts(current);
    if (!nominal) return current;
    const rows = new Set(dataTypes.get(nominal.name)?.rowParameters ?? []);
    const parameters = dataTypes.get(nominal.name)?.genericParameters ?? [];
    return nominalGenericType(
      nominal.name,
      nominal.arguments.map((argument, index) => {
        const row = rowArgumentKeys(argument);
        if (!rows.has(parameters[index] ?? "")) {
          if (row)
            mismatch ??= `'${nominal.name}' takes a type, not the row '${displayType(argument)}', for '${displayType(parameters[index])}'`;
          return visit(argument);
        }
        if (row) return rowArgumentType(row);
        // A row slot writes its row after `$` (11-requirements-and-suspension.md#r-req.row.slot.bare).
        mismatch ??= `'${nominal.name}' takes a requirement row, not the type '${displayType(argument)}', for '${displayType(parameters[index])}'; write '$ ${displayType(argument)}'`;
        return argument;
      }),
    );
  };
  const normalized = visit(type);
  return mismatch ? { mismatch } : normalized;
}

export function typeName(
  type: TypeRef,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait>,
  diagnostics: Diagnostic[],
  genericParameters: ReadonlySet<string> = new Set(),
  rowParameters: ReadonlySet<string> = new Set(),
  /** Type parameters bounded by `Eq` and `Hash`, which may key a map (trait.hash.map-key). */
  hashableParameters: ReadonlySet<string> = new Set(),
  options: {
    readonly validateRequirementKeys?: boolean;
    readonly validateDynamicSafety?: boolean;
    /**
     * Data and enum fields skip written-bound validation here: their bounds
     * check in `validateDeclaredTypes`, after every supertrait edge exists.
     */
    readonly validateWrittenBounds?: boolean;
  } = {},
  /** Generic bounds as written on the enclosing declaration, for checking arguments. */
  enclosingBounds: readonly GenericBound[] = [],
): ValueType | undefined {
  const kinded = normalizeRowArguments(
    resolveTraitType(resolveGenericType(type.name, genericParameters, rowParameters), traitTypes),
    dataTypes,
    rowParameters,
  );
  if (typeof kinded !== "string") {
    diagnostics.push({ code: "generic-kind-mismatch", message: kinded.mismatch, span: type.span });
    return undefined;
  }
  const resolved = kinded;
  if (options.validateRequirementKeys !== false) {
    const requirementDiagnostics = requirementKeyDiagnosticsInType(
      resolved,
      traitTypes,
      type.span,
      (argument) =>
        isKnownType(resolveTraitType(argument, traitTypes), dataTypes, enumTypes, traitTypes),
    );
    if (requirementDiagnostics.length > 0) {
      diagnostics.push(...requirementDiagnostics);
      return undefined;
    }
  }
  const bindingProblem = writtenBindingProblem(resolved, traitTypes);
  if (bindingProblem) {
    diagnostics.push({ ...bindingProblem, span: type.span });
    return undefined;
  }
  const dynamicProblem =
    options.validateDynamicSafety === false
      ? undefined
      : dynamicTraitProblemInType(resolved, traitTypes);
  if (dynamicProblem) diagnostics.push({ ...dynamicProblem, span: type.span });
  if (!isKnownType(resolved, dataTypes, enumTypes, traitTypes)) {
    diagnostics.push({
      code: "unknown-type",
      message: `unknown or unsupported type '${displayType(type.name)}'`,
      span: type.span,
    });
    return undefined;
  }
  // Written applications meet their declarations' bounds (trait.bound.no-implied).
  // Data and enum fields check in `validateDeclaredTypes`, after every
  // supertrait edge exists (trait.bound.supertraits).
  if (
    options.validateWrittenBounds !== false &&
    !type.implementationTarget &&
    pushWrittenBoundProblem(diagnostics, type.span, resolved, {
      dataTypes,
      enumTypes,
      traitTypes,
      hashableParameters,
      parameterImplied: (parameter, traitName) =>
        enclosingBoundImplies(enclosingBounds, traitTypes, parameter, traitName),
    })
  )
    return undefined;
  const restProblem = restElementProblem(resolved);
  if (restProblem) {
    diagnostics.push({ ...restProblem, span: type.span });
    return undefined;
  }
  return resolved;
}

export function resolveTraitType(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
): ValueType {
  const binding = bindingParts(type);
  if (binding) return bindingType(binding, resolveTraitType(binding.type, traitTypes));
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(resolveTraitType(mutable, traitTypes));
  if (traitTypes.has(type)) return `trait:${type}`;
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tupleType(tuple.map((element) => resolveTraitType(element, traitTypes)));
  const optional = optionalInner(type);
  if (optional !== undefined) return optionalType(resolveTraitType(optional, traitTypes));
  const resolveKey = (key: string): string =>
    resolveRequirementKeyTypes(key, (argument) => resolveTraitType(argument, traitTypes));
  const row = rowArgumentKeys(type);
  if (row) return rowArgumentType(row.map(resolveKey));
  const context = contextKeys(type);
  if (context) return contextType(context.map(resolveKey));
  const result = resultParts(type);
  if (result)
    return `Result[${resolveTraitType(result.ok, traitTypes)},${resolveTraitType(result.error, traitTypes)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal) {
    const arguments_ = nominal.arguments.map((argument) => resolveTraitType(argument, traitTypes));
    const resolved = nominalGenericType(nominal.name, arguments_);
    return traitTypes.has(nominal.name) ? `trait:${resolved}` : resolved;
  }
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map((parameter) => resolveTraitType(parameter, traitTypes)),
      resolveTraitType(callable.result, traitTypes),
      callable.requirements.map(resolveKey),
      callable.variadic,
      callable.suspending,
    );
  return type;
}

/** Rewrites each projection fixed by an associated type binding to its bound type. */
export function normalizeBoundProjections(
  type: ValueType,
  bounds: readonly HirGenericBound[],
): ValueType {
  const substitutions = new Map<string, ValueType>();
  for (const bound of bounds)
    for (const binding of bound.associatedBindings ?? [])
      substitutions.set(`${bound.parameter}::${binding.name}`, binding.type);
  return substitutions.size === 0 ? type : substituteGenericType(type, substitutions);
}

/** The deepest bound a proof may need (09-traits.md#r-trait.bound.depth.limit). */
export const MAX_BOUND_DEPTH = 64;

/**
 * A numeric literal, alone or under unary `-` or `+`, that was typed before
 * its expected type was known takes the wider `target` type directly
 * (04-type-system.md#r-types.num.binary.literal). Any other value never
 * widens implicitly (04-type-system.md#r-types.num.no-implicit).
 */
export function numericWidening(
  value: HirExpression,
  target: ValueType,
  span: SourceSpan,
): HirExpression | undefined {
  if (!widensTo(value.type, target) || !isNumericLiteralValue(value)) return undefined;
  if (value.kind === "integer")
    return numericType(target)!.bits === 64
      ? { ...value, wide: value.wide ?? String(value.value), type: target }
      : { ...value, type: target };
  if (value.kind === "float") return { ...value, type: target };
  return { kind: "unary", operator: "widen", operand: value, type: target, span };
}

/**
 * The `defaultArguments` and `parameterTypes` fields of a call node: each
 * defaulted parameter fills from its helper with the earlier call values.
 * `receiverOffset` is 1 for a method call, whose receiver fills parameter 0,
 * and 0 for a plain function call.
 */
export function defaultCallFields(
  signatures: ReadonlyMap<string, Signature>,
  defaultFunctionNames: readonly (string | undefined)[],
  defaultParameterIndices: readonly number[] | undefined,
  parameters: readonly ValueType[],
  receiverOffset: number,
): {
  readonly defaultArguments?: readonly HirDefaultArgument[];
  readonly parameterTypes?: readonly ValueType[];
} {
  const filled = (defaultParameterIndices ?? []).map((parameterIndex) => ({
    parameterIndex: parameterIndex + receiverOffset,
    functionIndex: signatures.get(defaultFunctionNames[parameterIndex]!)!.index,
  }));
  return {
    defaultArguments: filled.length > 0 ? filled : undefined,
    parameterTypes: filled.length > 0 ? parameters : undefined,
  };
}

function isNumericLiteralValue(value: HirExpression): boolean {
  let literal = value;
  while (literal.kind === "unary" && (literal.operator === "-" || literal.operator === "+"))
    literal = literal.operand;
  return literal.kind === "integer" || literal.kind === "float";
}
