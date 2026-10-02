import type { Pattern } from "../ast.ts";
import type { HirData, HirEnum, ValueType } from "../hir.ts";
import {
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  tupleLayout,
} from "../types.ts";
import { isIntegerType, numericType } from "../numeric.ts";
import { substituteGenericType } from "./shared.ts";

// Match exhaustiveness by pattern-matrix usefulness: the unguarded arms cover
// the subject type when no value escapes every row
// (06-control-flow.md#match-expressions). Types with finitely many
// constructors (bool, optionals, Result, enums, tuples, data) are split by
// constructor; any other type is covered only by a wildcard or binding.

interface Constructor {
  readonly name: string;
  readonly arguments: readonly ValueType[];
}

interface ExhaustivenessEnvironment {
  readonly enums: ReadonlyMap<string, HirEnum>;
  readonly data: ReadonlyMap<string, HirData>;
}

const WILDCARD: Pattern = {
  kind: "wildcard",
  span: {
    start: { offset: 0, line: 0, column: 0 },
    end: { offset: 0, line: 0, column: 0 },
  },
};

function substitutions(
  parameters: readonly string[],
  type: ValueType,
): ReadonlyMap<string, ValueType> {
  const nominal = nominalGenericParts(type);
  return new Map(
    parameters.map((parameter, index) => [parameter, nominal?.arguments[index] ?? parameter]),
  );
}

function constructorsOf(
  type: ValueType,
  environment: ExhaustivenessEnvironment,
): readonly Constructor[] | undefined {
  const view = readonlyType(type);
  if (view === "bool")
    return [
      { name: "true", arguments: [] },
      { name: "false", arguments: [] },
    ];
  const optional = optionalInner(view);
  if (optional !== undefined)
    return [
      { name: "Some", arguments: [optional] },
      { name: "None", arguments: [] },
    ];
  const result = resultParts(view);
  if (result)
    return [
      { name: "Ok", arguments: result.ok === "void" ? [] : [result.ok] },
      { name: "Err", arguments: [result.error] },
    ];
  const tuple = tupleLayout(view);
  if (tuple) return [{ name: "()", arguments: tuple }];
  const name = nominalGenericParts(view)?.name ?? view;
  const declaration = environment.enums.get(name);
  if (declaration) {
    const substitution = substitutions(declaration.genericParameters, view);
    return declaration.variants.map((variant) => ({
      name: variant.name,
      arguments: variant.fields.map((field) => substituteGenericType(field.type, substitution)),
    }));
  }
  const data = environment.data.get(name);
  if (data) {
    const substitution = substitutions(data.genericParameters, view);
    return [
      {
        name: "{}",
        arguments: data.fields.map((field) => substituteGenericType(field.type, substitution)),
      },
    ];
  }
  return undefined;
}

/** The constructor a pattern tests and its argument patterns, or undefined for a wildcard. */
function head(
  pattern: Pattern,
  type: ValueType,
  environment: ExhaustivenessEnvironment,
): { readonly name: string; readonly arguments: readonly Pattern[] } | undefined {
  switch (pattern.kind) {
    case "wildcard":
    case "binding":
      return undefined;
    case "boolean":
      return { name: String(pattern.value), arguments: [] };
    case "integer":
    case "float":
    case "string":
    case "character":
      return { name: `literal:${String(pattern.value)}`, arguments: [] };
    case "range":
      return {
        name: `range:${String(pattern.start)}:${String(pattern.end)}:${pattern.inclusive}`,
        arguments: [],
      };
    case "tuple":
      return { name: "()", arguments: pattern.elements };
    case "data": {
      const data = environment.data.get(
        nominalGenericParts(readonlyType(type))?.name ?? readonlyType(type),
      );
      const fields = data?.fields ?? [];
      return {
        name: "{}",
        arguments: fields.map(
          (field) => pattern.fields.find((entry) => entry.name === field.name)?.pattern ?? WILDCARD,
        ),
      };
    }
    case "result-variant":
    case "variant": {
      const payload =
        pattern.payloadPatterns ??
        pattern.bindings.map((name): Pattern =>
          name ? { kind: "binding", name, span: pattern.span } : WILDCARD,
        );
      const names = (pattern.kind === "variant" ? pattern.bindingNames : undefined) ?? [];
      const view = readonlyType(type);
      if (pattern.variantName === "Ok" && resultParts(view)?.ok === "void")
        return { name: "Ok", arguments: [] };
      const declaration = environment.enums.get(nominalGenericParts(view)?.name ?? view);
      const variant = declaration?.variants.find(
        (candidate) => candidate.name === pattern.variantName,
      );
      if (!variant || names.every((name) => name === undefined))
        return { name: pattern.variantName, arguments: payload };
      const arguments_: Pattern[] = variant.fields.map(() => WILDCARD);
      let positional = 0;
      payload.forEach((argument, index) => {
        const name = names[index];
        const field =
          name === undefined
            ? positional++
            : variant.fields.findIndex((candidate) => candidate.name === name);
        if (field >= 0) arguments_[field] = argument;
      });
      return { name: pattern.variantName, arguments: arguments_ };
    }
  }
}

/** An inclusive interval of integers; empty when `low > high`. */
export interface IntegerInterval {
  readonly low: bigint;
  readonly high: bigint;
}

/**
 * The values of an integer type that an integer literal or range pattern
 * matches (06-control-flow.md#range-patterns), or undefined for any other
 * pattern. A bound outside the type is clamped to it.
 */
export function integerPatternInterval(
  pattern: Pattern,
  type: ValueType,
): IntegerInterval | undefined {
  const numeric = numericType(readonlyType(type));
  if (!isIntegerType(readonlyType(type)) || !numeric) return undefined;
  const minimum = numeric.minimum!;
  const maximum = numeric.maximum!;
  if (pattern.kind === "integer") return { low: pattern.value, high: pattern.value };
  if (pattern.kind !== "range") return undefined;
  const low = pattern.start ?? minimum;
  const high =
    pattern.end === undefined ? maximum : pattern.inclusive ? pattern.end : pattern.end - 1n;
  return {
    low: low < minimum ? minimum : low,
    high: high > maximum ? maximum : high,
  };
}

/**
 * Integer coverage (06-control-flow.md#r-flow.match.cover.integer): the
 * type's values split at every literal and range bound into segments, each
 * of which a head interval holds whole or not at all. The rows are covered
 * when every segment is, specializing each segment as a constructor.
 */
function uncoveredIntegers(
  rows: readonly (readonly Pattern[])[],
  type: ValueType,
  rest: readonly ValueType[],
  environment: ExhaustivenessEnvironment,
): boolean | undefined {
  const intervals = rows.map((row) => integerPatternInterval(row[0]!, type));
  const numeric = numericType(readonlyType(type));
  if (!numeric || intervals.every((interval) => interval === undefined)) return undefined;
  const cuts = new Set<bigint>([numeric.minimum!, numeric.maximum! + 1n]);
  for (const interval of intervals)
    if (interval && interval.low <= interval.high) {
      cuts.add(interval.low);
      cuts.add(interval.high + 1n);
    }
  const sorted = [...cuts].sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
  const segments = sorted
    .slice(0, -1)
    .map((low, index): IntegerInterval => ({ low, high: sorted[index + 1]! - 1n }));
  const holds = (interval: IntegerInterval | undefined, segment: IntegerInterval): boolean =>
    interval === undefined || (interval.low <= segment.low && segment.high <= interval.high);
  const specialized = (segment: IntegerInterval): (readonly Pattern[])[] =>
    rows.flatMap((row, index) => (holds(intervals[index], segment) ? [row.slice(1)] : []));
  return segments.some((segment) => uncovered(specialized(segment), rest, environment));
}

function uncovered(
  rows: readonly (readonly Pattern[])[],
  types: readonly ValueType[],
  environment: ExhaustivenessEnvironment,
): boolean {
  if (types.length === 0) return rows.length === 0;
  const [type, ...rest] = types as [ValueType, ...ValueType[]];
  const integers = uncoveredIntegers(rows, type, rest, environment);
  if (integers !== undefined) return integers;
  const heads = rows.map((row) => head(row[0]!, type, environment));
  const constructors = constructorsOf(type, environment);
  const named = new Set(heads.flatMap((entry) => (entry ? [entry.name] : [])));
  if (constructors && constructors.every((constructor) => named.has(constructor.name))) {
    return constructors.some((constructor) => {
      const specialized = rows.flatMap((row, index) => {
        const entry = heads[index];
        if (!entry) return [[...constructor.arguments.map(() => WILDCARD), ...row.slice(1)]];
        if (entry.name !== constructor.name) return [];
        const arguments_ = constructor.arguments.map(
          (_, argument) => entry.arguments[argument] ?? WILDCARD,
        );
        return [[...arguments_, ...row.slice(1)]];
      });
      return uncovered(specialized, [...constructor.arguments, ...rest], environment);
    });
  }
  const defaults = rows.flatMap((row, index) => (heads[index] ? [] : [row.slice(1)]));
  return uncovered(defaults, rest, environment);
}

/** Whether the unguarded arm patterns cover every value of `type`. */
export function patternsExhaustive(
  patterns: readonly Pattern[],
  type: ValueType,
  environment: ExhaustivenessEnvironment,
): boolean {
  return !uncovered(
    patterns.map((pattern) => [pattern]),
    [type],
    environment,
  );
}
