import type { Pattern } from "../ast.ts";
import type { HirData, HirEnum, ValueType } from "../hir.ts";
import {
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
} from "../types.ts";
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

export interface ExhaustivenessEnvironment {
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
  const tuple = tupleParts(view);
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

function uncovered(
  rows: readonly (readonly Pattern[])[],
  types: readonly ValueType[],
  environment: ExhaustivenessEnvironment,
): boolean {
  if (types.length === 0) return rows.length === 0;
  const [type, ...rest] = types as [ValueType, ...ValueType[]];
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
