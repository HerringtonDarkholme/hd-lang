import type { GenericBound, Program } from "../ast.ts";
import { TUPLE_TRAIT } from "./standard-traits.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { functionResultText, rowArgumentKeys, tupleElements, displayType } from "../types.ts";

// The spelled function type constructors of `std.function`
// (07-functions.md#function-type-constructors). `Fn[(A, B), O, $ R]` is exactly
// `fn(A, B) -> O $ R`, and `SuspendFn[...]` is the `fn!` form; a rest element
// `List[T]...` ending the inputs is a vararg. Imported names are rewritten to
// the sugar before checking, so both spellings are one type.

type Constructor = "Fn" | "SuspendFn";

const CONSTRUCTORS: ReadonlyMap<string, Constructor> = new Map([
  ["std.function.Fn", "Fn"],
  ["std.function.SuspendFn", "SuspendFn"],
]);

interface Rewrite {
  readonly type: string;
  readonly error?: { readonly code: string; readonly message: string };
}

function splitArguments(contents: string): string[] {
  const values: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index <= contents.length; index += 1) {
    const character = contents[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if ((character === "," || index === contents.length) && depth === 0) {
      values.push(contents.slice(start, index));
      start = index + 1;
    }
  }
  return values;
}

function closingBracket(text: string, open: number): number {
  let depth = 0;
  for (let index = open; index < text.length; index += 1) {
    if (text[index] === "[") depth += 1;
    else if (text[index] === "]" && --depth === 0) return index;
  }
  return -1;
}

function rewriteType(
  text: string,
  names: ReadonlyMap<string, Constructor>,
  tupleBounded: ReadonlySet<string>,
): Rewrite {
  let error: Rewrite["error"];
  let result = "";
  let index = 0;
  while (index < text.length) {
    const match = /[A-Za-z_][A-Za-z0-9_]*\[/y;
    match.lastIndex = index;
    const found = (index === 0 || !/[A-Za-z0-9_.:]/.test(text[index - 1]!)) && match.exec(text);
    const name = found ? found[0].slice(0, -1) : undefined;
    const constructor = name ? names.get(name) : undefined;
    if (!found || !constructor) {
      result += text[index];
      index += 1;
      continue;
    }
    const open = index + name!.length;
    const close = closingBracket(text, open);
    if (close < 0) return { type: text };
    const inner = rewriteType(text.slice(open + 1, close), names, tupleBounded);
    error ??= inner.error;
    const arguments_ = splitArguments(inner.type);
    const lowered = lowerConstructor(constructor, arguments_, tupleBounded);
    error ??= lowered.error;
    result += lowered.type;
    index = close + 1;
  }
  return error ? { type: result, error } : { type: result };
}

function lowerConstructor(
  constructor: Constructor,
  arguments_: readonly string[],
  tupleBounded: ReadonlySet<string>,
): Rewrite {
  const spelled = `${constructor}[${arguments_.join(",")}]`;
  if (arguments_.length !== 3)
    return {
      type: spelled,
      error: {
        code: "generic-arity",
        message: `'${constructor}' takes the inputs, the output, and the requirement row`,
      },
    };
  const [inputs, output, row] = arguments_ as [string, string, string];
  // A `Tuple`-bounded type parameter is the one input `*Args`, which
  // substitution turns into the parameters of the tuple it is solved as.
  const elements = tupleElements(inputs) ?? (tupleBounded.has(inputs) ? [`*${inputs}`] : undefined);
  if (!elements)
    return {
      type: spelled,
      error: {
        code: "generic-kind-mismatch",
        message: `the inputs of '${constructor}' must be a tuple type or a type parameter bounded by Tuple, not '${displayType(inputs)}'`,
      },
    };
  const parameters = elements;
  // The row argument is a row slot, written after `$` (07-functions.md#r-fn.type.ctor.row.dollar).
  const keys = rowArgumentKeys(row);
  if (!keys)
    return {
      type: spelled,
      error: {
        code: "generic-kind-mismatch",
        message: `the row of '${constructor}' is written after '$', as in '$ ${displayType(row)}', not as the type '${displayType(row)}'`,
      },
    };
  const clause = keys.length > 0 ? `$${keys.join("+")}` : "";
  return {
    type: `fn${constructor === "SuspendFn" ? "!" : ""}(${parameters.join(",")})->${functionResultText(output)}${clause}`,
  };
}

/** Rewrites every spelled `std.function` constructor in `program` to the sugar. */
export function withFunctionTypeConstructors(program: Program): {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
} {
  const names = new Map<string, Constructor>();
  const tupleNames = new Set<string>();
  for (const declaration of program.uses)
    for (const imported of declaration.names) {
      const path = `${declaration.module}.${imported.name}`;
      const constructor = CONSTRUCTORS.get(path);
      if (constructor) names.set(imported.alias ?? imported.name, constructor);
      if (path === TUPLE_TRAIT) tupleNames.add(imported.alias ?? imported.name);
    }
  if (names.size === 0) return { program, diagnostics: [] };
  const diagnostics: Diagnostic[] = [];
  const rewriteText = (text: string, span: SourceSpan, bounded: ReadonlySet<string>): string => {
    if (!text.includes("[")) return text;
    const rewritten = rewriteType(text, names, bounded);
    if (rewritten.error) diagnostics.push({ ...rewritten.error, span });
    return rewritten.type;
  };
  // The type parameters bounded by `Tuple` in scope at a node.
  const visit = (value: unknown, outer: ReadonlySet<string> = new Set()): unknown => {
    if (Array.isArray(value)) {
      const items = value.map((item) => visit(item, outer));
      return items.every((item, index) => item === value[index]) ? value : items;
    }
    if (!value || typeof value !== "object") return value;
    const node = value as Record<string, unknown>;
    const span = node.span as SourceSpan | undefined;
    const bounds = Array.isArray(node.genericBounds) ? (node.genericBounds as GenericBound[]) : [];
    const tupleBounds = bounds.filter((bound) =>
      bound.traits.some((trait) => tupleNames.has(trait)),
    );
    const bounded =
      tupleBounds.length > 0
        ? new Set([...outer, ...tupleBounds.map((bound) => bound.parameter)])
        : outer;
    let changed = false;
    const result: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(node)) {
      let next: unknown;
      if (span && (key === "name" || key === "targetName") && typeof child === "string")
        next = rewriteText(child, span, bounded);
      else next = key === "span" ? child : visit(child, bounded);
      if (next !== child) changed = true;
      result[key] = next;
    }
    return changed ? result : value;
  };
  return { program: visit(program) as Program, diagnostics };
}
