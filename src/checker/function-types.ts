import type { Program } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { functionResultText, rowArgumentKeys, tupleParts } from "../types.ts";

// The spelled function type constructors of `std.function`
// (07-functions.md#function-type-constructors). `Fn[(A, B), O, R]` is exactly
// `fn(A, B) -> O $ R`, `SuspendFn[...]` is the `fn!` form, and `Rest[T]` as the
// final inputs element is a vararg. Imported names are rewritten to the sugar
// before checking, so both spellings are one type.

type Constructor = "Fn" | "SuspendFn" | "Rest";

const CONSTRUCTORS: ReadonlyMap<string, Constructor> = new Map([
  ["std.function.Fn", "Fn"],
  ["std.function.SuspendFn", "SuspendFn"],
  ["std.function.Rest", "Rest"],
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

function rewriteType(text: string, names: ReadonlyMap<string, Constructor>): Rewrite {
  let error: Rewrite["error"];
  let result = "";
  let index = 0;
  while (index < text.length) {
    const match = /[A-Za-z_][A-Za-z0-9_]*\[/y;
    match.lastIndex = index;
    const found = (index === 0 || !/[A-Za-z0-9_.:]/.test(text[index - 1]!)) && match.exec(text);
    const name = found ? found[0].slice(0, -1) : undefined;
    const constructor = name ? names.get(name) : undefined;
    if (!found || !constructor || constructor === "Rest") {
      result += text[index];
      index += 1;
      continue;
    }
    const open = index + name!.length;
    const close = closingBracket(text, open);
    if (close < 0) return { type: text };
    const inner = rewriteType(text.slice(open + 1, close), names);
    error ??= inner.error;
    const arguments_ = splitArguments(inner.type);
    const lowered = lowerConstructor(constructor, arguments_, names);
    error ??= lowered.error;
    result += lowered.type;
    index = close + 1;
  }
  return error ? { type: result, error } : { type: result };
}

function lowerConstructor(
  constructor: Constructor,
  arguments_: readonly string[],
  names: ReadonlyMap<string, Constructor>,
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
  // `(Is...)` is a tuple type too: one pack expansion needs no comma.
  const pack = /^\((.+\.\.\.)\)$/.exec(inputs)?.[1];
  const elements = tupleParts(inputs) ?? (inputs === "()" ? [] : pack ? [pack] : undefined);
  if (!elements)
    return {
      type: spelled,
      error: {
        code: "generic-kind-mismatch",
        message: `the inputs of '${constructor}' must be a tuple type, not '${inputs}'`,
      },
    };
  const restNames = new Set([...names].filter(([, kind]) => kind === "Rest").map(([name]) => name));
  const parameters: string[] = [];
  for (const [position, element] of elements.entries()) {
    const rest = [...restNames].find(
      (name) => element.startsWith(`${name}[`) && element.endsWith("]"),
    );
    if (!rest) {
      parameters.push(element);
      continue;
    }
    if (position !== elements.length - 1)
      return {
        type: spelled,
        error: {
          code: "nonfinal-vararg",
          message: "'Rest[T]' must be the final element of a function type's inputs",
        },
      };
    parameters.push(`${element.slice(rest.length + 1, -1)}...`);
  }
  const keys = rowArgumentKeys(row) ?? [row];
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
  for (const declaration of program.uses)
    for (const imported of declaration.names) {
      const constructor = CONSTRUCTORS.get(`${declaration.module}.${imported.name}`);
      if (constructor) names.set(imported.alias ?? imported.name, constructor);
    }
  if (names.size === 0) return { program, diagnostics: [] };
  const diagnostics: Diagnostic[] = [];
  const rewriteText = (text: string, span: SourceSpan): string => {
    if (!text.includes("[")) return text;
    const rewritten = rewriteType(text, names);
    if (rewritten.error) diagnostics.push({ ...rewritten.error, span });
    return rewritten.type;
  };
  const visit = (value: unknown): unknown => {
    if (Array.isArray(value)) {
      const items = value.map(visit);
      return items.every((item, index) => item === value[index]) ? value : items;
    }
    if (!value || typeof value !== "object") return value;
    const node = value as Record<string, unknown>;
    const span = node.span as SourceSpan | undefined;
    let changed = false;
    const result: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(node)) {
      let next: unknown;
      if (span && (key === "name" || key === "targetName") && typeof child === "string")
        next = rewriteText(child, span);
      else next = key === "span" ? child : visit(child);
      if (next !== child) changed = true;
      result[key] = next;
    }
    return changed ? result : value;
  };
  return { program: visit(program) as Program, diagnostics };
}
