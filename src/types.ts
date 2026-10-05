import type { ValueType } from "./hir.ts";

/**
 * The built-in list and map cursor behind `list.iter()` and `map.iter()`,
 * which checks iterator invalidation. `$` keeps its name out of source code.
 */
export const CURSOR_TYPE = "$Cursor";

export interface ResultParts {
  readonly ok: ValueType;
  readonly error: ValueType;
}

export interface FunctionParts {
  readonly parameters: readonly ValueType[];
  readonly suspending: boolean;
  readonly variadic: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
}

export interface NominalGenericParts {
  readonly name: string;
  readonly arguments: readonly ValueType[];
}

interface SuspensionParts {
  readonly functionIndex: number;
  readonly result: ValueType;
}

interface TraitSuspensionParts {
  readonly traitIndex: number;
  readonly methodIndex: number;
  readonly result: ValueType;
}

interface StoredSuspensionParts {
  readonly mutable: boolean;
  readonly result: ValueType;
}

export function mutableInner(type: ValueType): ValueType | undefined {
  return type.startsWith("mut:") ? type.slice("mut:".length) : undefined;
}

/** The primitive types (04-type-system.md#primitive-types). */
export const PRIMITIVE_TYPES: ReadonlySet<ValueType> = new Set(
  "bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char string".split(" "),
);

/**
 * A value that satisfies a `mut self` receiver: one with mutable access, or
 * a primitive, which has no `mut` form (04-type-system.md#r-types.prim.no-mut.self-call).
 */
export function mutableOrPrimitive(type: ValueType): boolean {
  return mutableInner(type) !== undefined || PRIMITIVE_TYPES.has(type);
}

export function mutableType(type: ValueType): ValueType {
  return `mut:${type}`;
}

export function readonlyType(type: ValueType): ValueType {
  return mutableInner(type) ?? type;
}

export function tupleParts(type: ValueType): readonly ValueType[] | undefined {
  if (!type.startsWith("(") || !type.endsWith(")")) return undefined;
  const contents = type.slice(1, -1);
  if (contents === "") return [];
  const values: string[] = [];
  let depth = 0;
  let start = 0;
  let sawComma = false;
  for (let index = 0; index < contents.length; index += 1) {
    const character = contents[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if (character === "," && depth === 0) {
      sawComma = true;
      const value = contents.slice(start, index);
      if (value) values.push(value);
      start = index + 1;
    }
  }
  const final = contents.slice(start);
  if (final) values.push(final);
  return sawComma ? values : undefined;
}

export function tupleType(elements: readonly ValueType[]): ValueType {
  return `(${elements.join(",")}${elements.length === 1 ? "," : ""})`;
}

/**
 * The type inside a rest element `List[T]...` of a tuple type or a function
 * type's inputs (04-type-system.md#rest-elements), or undefined for any other
 * element. A tuple's element list keeps the rest element's `...`.
 */
export function restInner(element: ValueType): ValueType | undefined {
  return element.endsWith("...") ? element.slice(0, -3) : undefined;
}

/**
 * The type parameter `Args` in the one parameter `*Args` that encodes
 * `Fn[Args, O, $ R]` with `Args < Tuple` (07-functions.md#r-fn.type.ctor.inputs).
 * Its value at run time is the inputs tuple, so it is one erased input.
 */
export function inputsInner(parameter: ValueType): ValueType | undefined {
  return parameter.startsWith("*") ? parameter.slice(1) : undefined;
}

/** A function type's inputs as the tuple type of them, keeping a rest element. */
export function functionInputsTuple(callable: FunctionParts): ValueType {
  const inputs =
    callable.parameters.length === 1 ? inputsInner(callable.parameters[0]!) : undefined;
  if (inputs !== undefined) return inputs;
  return tupleType(
    callable.parameters.map((parameter, index) =>
      callable.variadic && index === callable.parameters.length - 1 ? `${parameter}...` : parameter,
    ),
  );
}

/** A tuple type's fixed elements, and its rest element's `List[T]` if it ends in one. */
export function tupleRest(
  type: ValueType,
): { readonly fixed: readonly ValueType[]; readonly rest?: ValueType } | undefined {
  const parts = tupleParts(type);
  if (!parts) return undefined;
  const rest = parts.length > 0 ? restInner(parts.at(-1)!) : undefined;
  return rest === undefined ? { fixed: parts } : { fixed: parts.slice(0, -1), rest };
}

/** A tuple's runtime elements: its fixed elements, then its rest element's list. */
export function tupleLayout(type: ValueType): readonly ValueType[] | undefined {
  return tupleParts(type)?.map((element) => restInner(element) ?? element);
}

export function nominalGenericParts(type: ValueType): NominalGenericParts | undefined {
  if (type.startsWith("fn(") || type.startsWith("fn!(")) return undefined;
  const open = type.indexOf("[");
  if (open <= 0 || !type.endsWith("]")) return undefined;
  const name = type.slice(0, open);
  const contents = type.slice(open + 1, -1);
  const arguments_: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index <= contents.length; index += 1) {
    const character = contents[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if ((character === "," || index === contents.length) && depth === 0) {
      const argument = contents.slice(start, index);
      if (argument) arguments_.push(argument);
      start = index + 1;
    }
  }
  return { name, arguments: arguments_ };
}

export function nominalGenericType(name: string, arguments_: readonly ValueType[]): ValueType {
  return `${name}[${arguments_.join(",")}]`;
}

/** An associated type binding `Name=type` among a named type's arguments. */
export interface TypeBinding {
  readonly name: string;
  readonly type: ValueType;
}

/**
 * A named type's positional arguments and its associated type bindings,
 * which a trait value type or requirement key writes after them as
 * `Name=type` (09-traits.md#bound-associated-types).
 */
export function splitTypeBindings(arguments_: readonly ValueType[]): {
  readonly positional: readonly ValueType[];
  readonly bindings: readonly TypeBinding[];
} {
  const positional: ValueType[] = [];
  const bindings: TypeBinding[] = [];
  for (const argument of arguments_) {
    const binding = bindingParts(argument);
    if (binding) bindings.push(binding);
    else positional.push(argument);
  }
  return { positional, bindings };
}

/** The parts of one binding argument `Name=type`, or undefined for a type. */
export function bindingParts(argument: ValueType): TypeBinding | undefined {
  const match = /^([A-Za-z_][A-Za-z0-9_]*)=(.+)$/su.exec(argument);
  return match ? { name: match[1]!, type: match[2]! } : undefined;
}

/** A binding argument with its type rewritten. */
export function bindingType(binding: TypeBinding, type: ValueType): ValueType {
  return `${binding.name}=${type}`;
}

function isFunctionTypeText(type: ValueType): boolean {
  return (type.startsWith("fn(") || type.startsWith("fn!(")) && functionParts(type) !== undefined;
}

/**
 * The declared payload of an optional's readonly outer view. Outer mutable
 * permission does not change its generic argument: `mut:T?` contains `T`,
 * whereas `(mut:T)?` contains `mut:T`. Recursive type rewrites must inspect
 * mutableInner first to preserve that outer permission.
 * A function type's trailing `?` belongs to its result, so an optional
 * function type is rendered `(fn(...)->R)?`.
 */
export function optionalInner(type: ValueType): ValueType | undefined {
  type = readonlyType(type);
  if (!type.endsWith("?") || isFunctionTypeText(type)) return undefined;
  const inner = type.slice(0, -1);
  if (
    inner.startsWith("(") &&
    inner.endsWith(")") &&
    (mutableInner(inner.slice(1, -1)) !== undefined || isFunctionTypeText(inner.slice(1, -1)))
  )
    return inner.slice(1, -1);
  return inner;
}

/**
 * A function type's result as its type string writes it: a result that is
 * itself a function type is parenthesized, so that a requirement row after
 * it stays the outer function's, as in `fn(bool)->(fn()->string$Db)`.
 */
export function functionResultText(result: ValueType): ValueType {
  return isFunctionTypeText(result) ? `(${result})` : result;
}

/** `T?`, grouping prefixes whose scope would otherwise absorb the `?`. */
export function optionalType(inner: ValueType): ValueType {
  return mutableInner(inner) !== undefined || isFunctionTypeText(inner)
    ? `(${inner})?`
    : `${inner}?`;
}

/** Erase access permissions structurally, preserving constructor boundaries. */
export function eraseTypePermissions(type: ValueType): ValueType {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return eraseTypePermissions(mutable);
  const binding = bindingParts(type);
  if (binding) return bindingType(binding, eraseTypePermissions(binding.type));
  const inputs = inputsInner(type);
  if (inputs !== undefined) return `*${eraseTypePermissions(inputs)}`;
  const rest = restInner(type);
  if (rest !== undefined) return `${eraseTypePermissions(rest)}...`;
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tupleType(tuple.map(eraseTypePermissions));
  const optional = optionalInner(type);
  if (optional !== undefined) return optionalType(eraseTypePermissions(optional));
  const result = resultParts(type);
  if (result)
    return resultType(eraseTypePermissions(result.ok), eraseTypePermissions(result.error));
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map(eraseTypePermissions),
      eraseTypePermissions(callable.result),
      callable.requirements.map(eraseTypePermissions),
      callable.variadic,
      callable.suspending,
    );
  const context = contextKeys(type);
  if (context) return contextType(context.map(eraseTypePermissions));
  const row = rowArgumentKeys(type);
  if (row) return rowArgumentType(row.map(eraseTypePermissions));
  const nominal = nominalGenericParts(type);
  if (nominal) return nominalGenericType(nominal.name, nominal.arguments.map(eraseTypePermissions));
  return type;
}

/** Render canonical permission/constructor boundaries as hd type syntax. */
export function typeSourceText(type: ValueType): string {
  const mutable = mutableInner(type);
  if (mutable !== undefined) {
    const inner = typeSourceText(mutable);
    return `mut ${optionalInner(mutable) !== undefined ? `(${inner})` : inner}`;
  }
  const binding = bindingParts(type);
  if (binding) return bindingType(binding, typeSourceText(binding.type));
  const inputs = inputsInner(type);
  if (inputs !== undefined) return `*${typeSourceText(inputs)}`;
  const rest = restInner(type);
  if (rest !== undefined) return `${typeSourceText(rest)}...`;
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tupleType(tuple.map(typeSourceText));
  const optional = optionalInner(type);
  if (optional !== undefined) {
    const inner = typeSourceText(optional);
    return mutableInner(optional) !== undefined || functionParts(optional)
      ? `(${inner})?`
      : `${inner}?`;
  }
  const result = resultParts(type);
  if (result) return resultType(typeSourceText(result.ok), typeSourceText(result.error));
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map(typeSourceText),
      typeSourceText(callable.result),
      callable.requirements.map(typeSourceText),
      callable.variadic,
      callable.suspending,
    );
  const nominal = nominalGenericParts(type);
  if (nominal) return nominalGenericType(nominal.name, nominal.arguments.map(typeSourceText));
  return type;
}

/**
 * Renders a type, trait, or related compiler name the way the user wrote it,
 * for diagnostics (the 2026-10-04 error-message sweep). The canonical
 * `typeSourceText` keeps hidden and synthetic spellings (`__std_` renames,
 * `generic:`/`trait:`/`row:` markers, `$impl`/`$inherent` qualifiers,
 * `hd_E` tuple-element parameters, spacing-free arrows) because type strings
 * round-trip through the checker; this is the display layer over it.
 *
 * Each pattern below matches only spellings the user cannot write (a `:`
 * never appears in an identifier, and `$` never starts one), except two
 * documented heuristics: an `__std_<module>_<Name>` segment reads as the
 * hidden rename the standard library loader generates, and `hd_E<N>` reads
 * as tuple element N. A user identifier that apes either keeps its meaning
 * but prints in the generated form.
 */
export function displayType(type: ValueType): string {
  return typeSourceText(type)
    .replace(/\$(?:impl|inherent)\d+\./g, "")
    .replace(/(?<![A-Za-z0-9_])hd_E(\d+)(?![A-Za-z0-9_])/g, "element $1")
    .replaceAll("$row:", "$ ")
    .replace(/(?<![A-Za-z0-9_])row:/g, "$ ")
    .replace(/(?<![A-Za-z0-9_])generic:/g, "")
    .replace(/(?<![A-Za-z0-9_])trait:/g, "")
    .replace(/(?<![A-Za-z0-9_])__std_[a-z0-9_]+_([A-Z][A-Za-z0-9_]*)/g, "$1")
    .replaceAll(")->", ") -> ")
    .replace(/,(?! )/g, ", ");
}

export function resultParts(type: ValueType): ResultParts | undefined {
  if (!type.startsWith("Result[") || !type.endsWith("]")) return undefined;
  const contents = type.slice("Result[".length, -1);
  let depth = 0;
  for (let index = 0; index < contents.length; index += 1) {
    const character = contents[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if (character === "," && depth === 0) {
      return { ok: contents.slice(0, index), error: contents.slice(index + 1) };
    }
  }
  return undefined;
}

export function resultType(ok: ValueType, error: ValueType): ValueType {
  return `Result[${ok},${error}]`;
}

export function isErasedVariant(type: ValueType): boolean {
  return optionalInner(type) !== undefined || resultParts(type) !== undefined;
}

export function functionParts(type: ValueType): FunctionParts | undefined {
  const suspending = type.startsWith("fn!(");
  if ((!suspending && !type.startsWith("fn(")) || !type.includes(")->")) return undefined;
  const prefixLength = suspending ? 4 : 3;
  let depth = 0;
  let close = -1;
  for (let index = prefixLength; index < type.length; index += 1) {
    const character = type[index];
    if (character === "(" || character === "[") depth += 1;
    else if (character === "]") depth -= 1;
    else if (character === ")" && depth === 0) {
      close = index;
      break;
    } else if (character === ")") depth -= 1;
  }
  if (close < 0 || type.slice(close, close + 3) !== ")->") return undefined;
  const parameterText = type.slice(prefixLength, close);
  const renderedParameters: string[] = [];
  let start = 0;
  depth = 0;
  for (let index = 0; index <= parameterText.length; index += 1) {
    const character = parameterText[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if ((character === "," || index === parameterText.length) && depth === 0) {
      const parameter = parameterText.slice(start, index);
      if (parameter) renderedParameters.push(parameter);
      start = index + 1;
    }
  }
  const tail = type.slice(close + 3);
  let resultDepth = 0;
  let requirementStart = -1;
  for (let index = 0; index < tail.length; index += 1) {
    const character = tail[index];
    if (character === "[" || character === "(") resultDepth += 1;
    else if (character === "]" || character === ")") resultDepth -= 1;
    else if (character === "$" && resultDepth === 0) {
      requirementStart = index;
      break;
    }
  }
  const written = requirementStart < 0 ? tail : tail.slice(0, requirementStart);
  const result =
    written.startsWith("(") && written.endsWith(")") && isFunctionTypeText(written.slice(1, -1))
      ? written.slice(1, -1)
      : written;
  const requirements = requirementStart < 0 ? [] : splitRowKeys(tail.slice(requirementStart + 1));
  const variadic = renderedParameters.at(-1)?.endsWith("...") === true;
  const parameters = renderedParameters.map((parameter, index) =>
    variadic && index === renderedParameters.length - 1 ? parameter.slice(0, -3) : parameter,
  );
  return { parameters, suspending, variadic, result, requirements };
}

export function functionType(
  parameters: readonly ValueType[],
  result: ValueType,
  requirements: readonly string[] = [],
  variadic = false,
  suspending = false,
): ValueType {
  const row = [...new Set(requirements)].sort();
  // `*Args` solved as a tuple type is that tuple's elements, its rest
  // element the vararg (07-functions.md#r-fn.type.ctor.sugar).
  const inputs = parameters.length === 1 ? inputsInner(parameters[0]!) : undefined;
  const solved = inputs !== undefined ? tupleParts(inputs) : undefined;
  if (solved) {
    const rest = solved.length > 0 && restInner(solved.at(-1)!) !== undefined;
    return functionType(
      solved.map((element) => restInner(element) ?? element),
      result,
      requirements,
      rest,
      suspending,
    );
  }
  // A `List[T]` vararg is the rest element `List[T]...` of the inputs
  // (07-functions.md#r-fn.type.vararg-rest).
  const rendered = parameters.map((parameter, index) =>
    variadic && index === parameters.length - 1 ? `${parameter}...` : parameter,
  );
  return `fn${suspending ? "!" : ""}(${rendered.join(",")})->${functionResultText(result)}${row.length ? `$${row.join("+")}` : ""}`;
}

export function substituteTypeParameters(
  type: ValueType,
  substitutions: ReadonlyMap<string, ValueType>,
): ValueType {
  const binding = bindingParts(type);
  if (binding) return bindingType(binding, substituteTypeParameters(binding.type, substitutions));
  const inputs = inputsInner(type);
  if (inputs !== undefined) return `*${substituteTypeParameters(inputs, substitutions)}`;
  const rest = restInner(type);
  if (rest !== undefined) return `${substituteTypeParameters(rest, substitutions)}...`;
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(substituteTypeParameters(mutable, substitutions));
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tupleType(tuple.map((element) => substituteTypeParameters(element, substitutions)));
  const optional = optionalInner(type);
  if (optional !== undefined)
    return optionalType(substituteTypeParameters(optional, substitutions));
  const result = resultParts(type);
  if (result)
    return `Result[${substituteTypeParameters(result.ok, substitutions)},${substituteTypeParameters(result.error, substitutions)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal)
    return nominalGenericType(
      nominal.name,
      nominal.arguments.map((argument) => substituteTypeParameters(argument, substitutions)),
    );
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map((parameter) => substituteTypeParameters(parameter, substitutions)),
      substituteTypeParameters(callable.result, substitutions),
      callable.requirements.map((requirement) =>
        substituteTypeParameters(requirement, substitutions),
      ),
      callable.variadic,
      callable.suspending,
    );
  const generic = /^generic:([^?[\](),]+)$/.exec(type)?.[1];
  return generic ? (substitutions.get(generic) ?? type) : type;
}

/**
 * The keys of a rendered row `A+B`, split at top-level `+` only: a key such
 * as `WithLog[$(Clock+Db)]` keeps the row argument of a row alias whole
 * (11-requirements-and-suspension.md#row-aliases).
 */
export function splitRowKeys(text: string): readonly string[] {
  const keys: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index <= text.length; index += 1) {
    const character = text[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if ((character === "+" || index === text.length) && depth === 0) {
      const key = text.slice(start, index);
      if (key) keys.push(key);
      start = index + 1;
    }
  }
  return keys;
}

export function contextKeys(type: ValueType): readonly string[] | undefined {
  return type.startsWith("context:") ? splitRowKeys(type.slice("context:".length)) : undefined;
}

export function contextType(keys: readonly string[]): ValueType {
  return `context:${[...new Set(keys)].sort().join("+")}`;
}

export function suspensionType(functionIndex: number, result: ValueType): ValueType {
  return `suspend(${functionIndex}):${result}`;
}

export function suspensionParts(type: ValueType): SuspensionParts | undefined {
  const match = /^suspend\((\d+)\):(.*)$/s.exec(type);
  return match ? { functionIndex: Number(match[1]), result: match[2]! } : undefined;
}

export function traitSuspensionType(
  traitIndex: number,
  methodIndex: number,
  result: ValueType,
): ValueType {
  return `trait-suspend(${traitIndex},${methodIndex}):${result}`;
}

export function traitSuspensionParts(type: ValueType): TraitSuspensionParts | undefined {
  const match = /^trait-suspend\((\d+),(\d+)\):(.*)$/s.exec(type);
  return match
    ? { traitIndex: Number(match[1]), methodIndex: Number(match[2]), result: match[3]! }
    : undefined;
}

export function storedSuspensionParts(type: ValueType): StoredSuspensionParts | undefined {
  const mutable = mutableInner(type);
  const nominal = nominalGenericParts(mutable ?? type);
  return nominal?.name === "Suspend" && nominal.arguments.length === 1
    ? { mutable: mutable !== undefined, result: nominal.arguments[0]! }
    : undefined;
}

/**
 * The keys of a row type argument such as `$(Clock+Logger)` or `$()`, the
 * argument of a row-kinded generic parameter (02-grammar.md#types).
 */
export function rowArgumentKeys(type: ValueType | undefined): readonly string[] | undefined {
  if (!type?.startsWith("$(") || !type.endsWith(")")) return undefined;
  return splitRowKeys(type.slice(2, -1));
}

export function rowArgumentType(keys: readonly string[]): ValueType {
  return `$(${[...new Set(keys)].sort().join("+")})`;
}

/**
 * The first rest element in a type whose type is not `List[T]`, as `i32` in
 * `(i32, i32...)` or `fn(i32...) -> i32` (04-type-system.md#r-types.tuple.rest.list).
 */
export function nonListRestElement(type: ValueType): ValueType | undefined {
  for (let index = type.indexOf("..."); index >= 0; index = type.indexOf("...", index + 3)) {
    let depth = 0;
    let start = index;
    for (; start > 0; start -= 1) {
      const character = type[start - 1];
      if (character === "]" || character === ")") depth += 1;
      else if (character === "[" || character === "(") {
        if (depth === 0) break;
        depth -= 1;
      } else if (character === "," && depth === 0) break;
    }
    const element = type.slice(start, index);
    if (nominalGenericParts(readonlyType(element))?.name !== "List") return element;
  }
  return undefined;
}
