import { traitValueBindings, writtenBindingProblem } from "./associated-bindings.ts";
import type { Expression, Program, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { isIntegerType, isNarrowInteger, numericType, widensTo } from "../numeric.ts";
import type {
  HirData,
  HirEnum,
  HirExpression,
  HirGenericBound,
  HirTrait,
  ValueType,
} from "../hir.ts";
import {
  contextKeys,
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
  nonListRestElement,
  restInner,
  inputsInner,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  functionInputsTuple,
  type FunctionParts,
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

export interface IterableInfo {
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
  "ConsoleError",
]);

/**
 * How a map stores and compares its keys at run time: 0 for an `i32`-like
 * scalar, 1 for a string, and 2 for any other key type, which the map
 * compares with its `Eq`. Which types may key a map is `mapKeyProblem`'s.
 */
export function mapKeyKind(type: ValueType): 0 | 1 | 2 {
  if (isNarrowInteger(type) || type === "bool" || type === "char") return 0;
  if (type === "string") return 1;
  return 2;
}

/**
 * Why `type` may not key a map, if it may not: a `mut` key type, or a key
 * type that fails the declared bound `Map[K < Eq & Hash, V]`
 * (spec/04-type-system.md#map-key-types). A type parameter meets it
 * through its own bounds, `hashableParameters`; any other type through a
 * non-generic `Eq` and `Hash` implementation, std's included, or the
 * primitive `Eq` of the language.
 */
export function mapKeyProblem(
  type: ValueType,
  hashableParameters: ReadonlySet<string> = new Set(),
): { readonly code: string; readonly message: string } | undefined {
  if (mutableInner(type) !== undefined)
    return {
      code: "invalid-map-key",
      message: `a map key type must not be mut, found 'mut ${readonlyType(type)}'`,
    };
  const generic = genericTypeName(type);
  const missing = generic
    ? hashableParameters.has(generic)
      ? undefined
      : "Eq and Hash"
    : ["Hash", "Eq"].find(
        (trait) =>
          !(implementedTraits.get(trait)?.has(type) ?? false) &&
          !(trait === "Eq" && (numericType(type) || ["bool", "char", "string"].includes(type))),
      );
  return missing
    ? {
        code: "unsatisfied-trait-bound",
        message: `type '${type}' does not implement ${missing}, required by the bound on 'K' of 'Map'`,
      }
    : undefined;
}

// The types with a non-generic `Eq` or `Hash` implementation, by trait, set
// for each checked program before its types resolve (checker/program.ts).
let implementedTraits: ReadonlyMap<string, ReadonlySet<string>> = new Map();

export function setHashableKeyTypes(program: Program): void {
  const implemented = (trait: string): Set<string> =>
    new Set(
      program.implementations
        .filter((item) => item.traitName === trait && item.genericParameters.length === 0)
        .map((item) => item.targetName),
    );
  implementedTraits = new Map(["Eq", "Hash"].map((trait) => [trait, implemented(trait)]));
}

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
    return tuple.every(
      (element) => element !== "void" && isKnownType(element, dataTypes, enumTypes, traitTypes),
    );
  const optional = optionalInner(type);
  if (optional !== undefined)
    return optional !== "void" && isKnownType(optional, dataTypes, enumTypes, traitTypes);
  const result = resultParts(type);
  if (result)
    return (
      isKnownType(result.ok, dataTypes, enumTypes, traitTypes) &&
      result.error !== "void" &&
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
        nominal.arguments[0] !== "void" &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    // `List[void]` is a valid type, as a mapped `void` callback's result.
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
        nominal.arguments[1] !== "void" &&
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
    callable.parameters.every(
      (parameter) =>
        parameter !== "void" && isKnownType(parameter, dataTypes, enumTypes, traitTypes),
    ) &&
    isKnownType(callable.result, dataTypes, enumTypes, traitTypes),
  );
}

export function statementsReferenceName(statements: readonly Statement[], name: string): boolean {
  return statements.some((statement) => nodeReferencesName(statement, name));
}

export function nodeReferencesName(value: unknown, name: string): boolean {
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
          : [requirement];
      }),
      callable.variadic,
      callable.suspending,
    );
  }
  const generic = genericTypeName(type);
  if (generic) return substitutions.get(generic) ?? type;
  return type;
}

/** The `Args` of a function type `Fn[Args, O, R]` whose inputs are a type parameter. */
export function tupleInputs(callable: FunctionParts): ValueType | undefined {
  return callable.parameters.length === 1 ? inputsInner(callable.parameters[0]!) : undefined;
}

export function genericTypeName(type: ValueType): string | undefined {
  const match = /^generic:([^?[\](),]+)$/.exec(type);
  return match?.[1];
}

/**
 * The type an argument has before its expected type converted it: the
 * operand of a numeric widening or of a trait-value conversion. A numeric
 * literal takes its expected type directly, so it has no type of its own
 * here (04-type-system.md#inference-from-several-arguments).
 */
export function argumentOwnType(source: Expression, checked: HirExpression): ValueType | undefined {
  let literal = source;
  while (literal.kind === "unary" && literal.operator === "-") literal = literal.operand;
  if (literal.kind === "integer" || literal.kind === "float") return undefined;
  if (checked.type === "never") return undefined;
  if (checked.kind === "unary" && checked.operator === "widen") return checked.operand.type;
  if (checked.kind === "trait-wrap" || checked.kind === "trait-bound") return checked.value.type;
  return checked.type;
}

export function traitTypeName(type: ValueType): string | undefined {
  const readonly = readonlyType(type);
  if (!readonly.startsWith("trait:") || readonly.endsWith("?")) return undefined;
  const key = readonly.slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
}

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
    const tuple = tupleParts(type);
    if (tuple !== undefined) return { head: `tuple:${tuple.length}`, values: tuple };
    const optional = optionalInner(type);
    if (optional !== undefined) return { head: "optional", values: [optional] };
    const result = resultParts(type);
    if (result) return { head: "Result", values: [result.ok, result.error] };
    const nominal = nominalGenericParts(type);
    if (nominal) return { head: `nominal:${nominal.name}`, values: nominal.arguments };
    const callable = functionParts(type);
    // The inputs take part as one tuple, so `Fn[Args, O, R]` unifies with
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
  return `expected '${expected}', but the call returns '${result.replace(/\b(generic|row):/g, "")}'`;
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
    (callable.parameters.some(containsGenericType) || containsGenericType(callable.result)),
  );
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
      return `generic parameter '${generic}' was inferred as both ${existing} and ${actual}`;
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
  // `Fn[Args, O, R]` solves `Args` as the tuple of the actual inputs
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
      inferRequirementRows(
        formalCallable.requirements,
        actualCallable.requirements,
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
      inferRequirementRows(
        formalCallable.requirements,
        actualCallable.requirements,
        rowSubstitutions,
      )
    );
  }
  return undefined;
}

export function matchGenericTypePattern(
  pattern: ValueType,
  actual: ValueType,
  substitutions: Map<string, ValueType>,
): boolean {
  const patternBinding = bindingParts(pattern);
  const actualBinding = bindingParts(actual);
  if (patternBinding || actualBinding)
    return Boolean(
      patternBinding &&
      actualBinding &&
      patternBinding.name === actualBinding.name &&
      matchGenericTypePattern(patternBinding.type, actualBinding.type, substitutions),
    );
  const patternRest = restInner(pattern);
  const actualRest = restInner(actual);
  if (patternRest !== undefined || actualRest !== undefined)
    return Boolean(
      patternRest !== undefined &&
      actualRest !== undefined &&
      matchGenericTypePattern(patternRest, actualRest, substitutions),
    );
  const generic = genericTypeName(pattern);
  if (generic) {
    const existing = substitutions.get(generic);
    if (existing) return existing === actual;
    substitutions.set(generic, actual);
    return true;
  }
  // An identical type that names a parameter, such as `Filter[T]` inside
  // `impl[T] ... for Filter[T]`, still binds that parameter to itself.
  if (pattern === actual && !pattern.includes("generic:")) return true;
  const patternMutable = mutableInner(pattern);
  const actualMutable = mutableInner(actual);
  if (patternMutable !== undefined || actualMutable !== undefined)
    return (
      patternMutable !== undefined &&
      actualMutable !== undefined &&
      matchGenericTypePattern(patternMutable, actualMutable, substitutions)
    );
  const patternOptional = optionalInner(pattern);
  const actualOptional = optionalInner(actual);
  if (patternOptional !== undefined || actualOptional !== undefined)
    return (
      patternOptional !== undefined &&
      actualOptional !== undefined &&
      matchGenericTypePattern(patternOptional, actualOptional, substitutions)
    );
  const patternTuple = tupleParts(pattern);
  const actualTuple = tupleParts(actual);
  if (patternTuple !== undefined || actualTuple !== undefined)
    return Boolean(
      patternTuple &&
      actualTuple &&
      patternTuple.length === actualTuple.length &&
      patternTuple.every((element, index) =>
        matchGenericTypePattern(element, actualTuple[index]!, substitutions),
      ),
    );
  const patternCallable = functionParts(pattern);
  const actualCallable = functionParts(actual);
  if (patternCallable || actualCallable) {
    if (
      !patternCallable ||
      !actualCallable ||
      patternCallable.suspending !== actualCallable.suspending ||
      !matchGenericTypePattern(patternCallable.result, actualCallable.result, substitutions)
    )
      return false;
    const rows = patternCallable.requirements.map(rowParameterName);
    if (rows.length === 1 && rows[0] !== undefined) {
      const row = rowArgumentType(actualCallable.requirements);
      const existing = substitutions.get(rows[0]);
      if (existing !== undefined && existing !== row) return false;
      substitutions.set(rows[0], row);
    } else if (!sameRequirements(patternCallable.requirements, actualCallable.requirements))
      return false;
    const inputs = tupleInputs(patternCallable);
    if (inputs !== undefined)
      return matchGenericTypePattern(inputs, functionInputsTuple(actualCallable), substitutions);
    return matchGenericTypePattern(
      functionInputsTuple(patternCallable),
      functionInputsTuple(actualCallable),
      substitutions,
    );
  }
  const patternNominal = nominalGenericParts(pattern);
  const actualNominal = nominalGenericParts(actual);
  return Boolean(
    patternNominal &&
    actualNominal &&
    patternNominal.name === actualNominal.name &&
    patternNominal.arguments.length === actualNominal.arguments.length &&
    patternNominal.arguments.every((argument, index) =>
      matchGenericTypePattern(argument, actualNominal.arguments[index]!, substitutions),
    ),
  );
}

interface TraitImplementationPattern {
  readonly targetType: ValueType;
  readonly traitArguments: readonly ValueType[];
  readonly trait?: { readonly index: number };
  readonly traitIndex?: number;
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
  )
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

export function rowParameterName(requirement: string): string | undefined {
  return requirement.startsWith("row:") ? requirement.slice("row:".length) : undefined;
}

export function symbolicRequirement(name: string): string {
  return `row:${name}`;
}

export function normalizedRequirements(requirements: readonly string[]): readonly string[] {
  return [...new Set(requirements)].sort();
}

export function sameRequirements(left: readonly string[], right: readonly string[]): boolean {
  const normalizedLeft = normalizedRequirements(left);
  const normalizedRight = normalizedRequirements(right);
  return (
    normalizedLeft.length === normalizedRight.length &&
    normalizedLeft.every((requirement, index) => requirement === normalizedRight[index])
  );
}

export function inferRequirementRows(
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
      callable.requirements.flatMap((requirement) =>
        resolveGenericRequirement(requirement, rowParameters),
      ),
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
  const declaration =
    program.data.find((candidate) => candidate.name === base) ??
    program.enums.find((candidate) => candidate.name === base) ??
    program.traits.find((candidate) => candidate.name === base);
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
  rowParameters: ReadonlySet<string>,
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
            mismatch ??= `'${nominal.name}' takes a type, not the row '${argument}', for '${parameters[index]}'`;
          return visit(argument);
        }
        if (row) return rowArgumentType(row);
        if (rowParameters.has(argument)) return rowArgumentType([symbolicRequirement(argument)]);
        if (argument.startsWith("trait:"))
          return rowArgumentType([argument.slice("trait:".length)]);
        mismatch ??= `'${nominal.name}' takes a requirement row, not the type '${argument}', for '${parameters[index]}'`;
        return argument;
      }),
    );
  };
  const normalized = visit(type);
  return mismatch ? { mismatch } : normalized;
}

/** A rest element that is not a `List[T]` (04-type-system.md#r-types.tuple.rest.list). */
export function restElementProblem(
  type: ValueType,
): { readonly code: string; readonly message: string } | undefined {
  const rest = nonListRestElement(type);
  return rest === undefined
    ? undefined
    : {
        code: "type-mismatch",
        message: `a rest element must be a List[T], as in 'List[${rest}]...', not '${rest}...'`,
      };
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
  const bindingProblem = writtenBindingProblem(resolved, traitTypes);
  if (bindingProblem) {
    diagnostics.push({ ...bindingProblem, span: type.span });
    return undefined;
  }
  const dynamicTraitName = traitTypeName(resolved);
  const dynamicTrait = dynamicTraitName && traitTypes.get(dynamicTraitName);
  if (
    dynamicTrait &&
    !traitIsDynamicallySafe(dynamicTrait, traitTypes, new Set(traitValueBindings(resolved).keys()))
  ) {
    diagnostics.push({
      code: "trait-not-dynamically-safe",
      message: `trait '${dynamicTrait.name}' cannot be used as a dynamic value`,
      span: type.span,
    });
  }
  const nominal = nominalGenericParts(resolved);
  const keyProblem =
    nominal?.name === "Map" &&
    nominal.arguments.length === 2 &&
    isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes) &&
    isKnownType(nominal.arguments[1]!, dataTypes, enumTypes, traitTypes)
      ? mapKeyProblem(nominal.arguments[0]!, hashableParameters)
      : undefined;
  if (keyProblem) {
    diagnostics.push({ ...keyProblem, span: type.span });
    return undefined;
  }
  if (!isKnownType(resolved, dataTypes, enumTypes, traitTypes)) {
    diagnostics.push({
      code: "unknown-type",
      message: `unknown or unsupported type '${type.name}'`,
      span: type.span,
    });
    return undefined;
  }
  const restProblem = restElementProblem(resolved);
  if (restProblem) {
    diagnostics.push({ ...restProblem, span: type.span });
    return undefined;
  }
  return resolved;
}

// A row parameter keeps one body: its providers pass as one bundle
// (09-traits.md#r-trait.dyn.safe.row-parameter).
function isMethodRowParameter(method: HirTrait["methods"][number], parameter: string): boolean {
  if (method.requirements.includes(parameter)) return true;
  if (method.requirements.includes(symbolicRequirement(parameter))) return true;
  const inRow = new RegExp(`\\$\\(?[^)]*\\b${parameter}\\b`);
  return method.parameters.some((type) => inRow.test(type)) || inRow.test(method.result);
}

/**
 * The one-copy rule (09-traits.md#dynamic-safety). An associated type is safe
 * only when the value type binds it, so `bound` holds the names the value
 * type binds (trait.dyn.binding.complete).
 */
export function traitIsDynamicallySafe(
  trait: HirTrait,
  traitTypes: ReadonlyMap<string, HirTrait>,
  bound: ReadonlySet<string> = new Set(),
  seen: ReadonlySet<number> = new Set(),
): boolean {
  if (seen.has(trait.index)) return true;
  if (
    trait.associatedTypes.some((associated) => !bound.has(associated.name)) ||
    trait.methods.some(
      (method) =>
        method.associated ||
        (method.reifiedParameters ?? []).length > 0 ||
        method.genericParameters.some(
          (parameter) =>
            !(method.referenceParameters ?? []).includes(parameter) &&
            !isMethodRowParameter(method, parameter),
        ) ||
        // A projection `Self::Item` is the bound type, not `Self`.
        method.parameters.some((parameter) => /generic:Self(?!::)/.test(parameter)) ||
        /generic:Self(?!::)/.test(method.result),
    )
  )
    return false;
  const next = new Set([...seen, trait.index]);
  return trait.supertraits.every((supertrait) => {
    const parent = [...traitTypes.values()].find(
      (candidate) => candidate.index === supertrait.traitIndex,
    );
    return !parent || traitIsDynamicallySafe(parent, traitTypes, bound, next);
  });
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
      callable.requirements,
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
 * Whether the standard library's `Ord` covers `type`: the ordered primitives
 * other than floats, and tuples, optionals, and lists of such types
 * (09-traits.md#comparison-traits).
 */
export function builtinTotallyOrdered(type: ValueType): boolean {
  const compared = readonlyType(type);
  if (isIntegerType(compared) || ["char", "string"].includes(compared)) return true;
  const tuple = tupleParts(compared);
  if (tuple !== undefined) return tuple.every(builtinTotallyOrdered);
  const optional = optionalInner(compared);
  if (optional !== undefined) return builtinTotallyOrdered(optional);
  const nominal = nominalGenericParts(compared);
  return (
    nominal?.name === "List" &&
    nominal.arguments.length === 1 &&
    builtinTotallyOrdered(nominal.arguments[0]!)
  );
}

/**
 * 04 Numeric Conversions: `value` widened within its family, or `f32` to
 * `f64`; a literal takes the wider type directly.
 */
export function numericWidening(
  value: HirExpression,
  target: ValueType,
  span: SourceSpan,
): HirExpression | undefined {
  if (!widensTo(value.type, target)) return undefined;
  if (value.kind === "integer")
    return numericType(target)!.bits === 64
      ? { ...value, wide: value.wide ?? String(value.value), type: target }
      : { ...value, type: target };
  if (value.kind === "float") return { ...value, type: target };
  return { kind: "unary", operator: "widen", operand: value, type: target, span };
}
