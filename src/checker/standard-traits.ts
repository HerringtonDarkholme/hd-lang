import type { Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import { renameStandardBindings } from "./standard-bindings.ts";
import { standardPreludeBinding } from "./standard-library.ts";
import { standardSource } from "./standard-sources.ts";

/** The conversion trait that postfix `?` calls (spec/lang/09-traits.md#conversion-trait). */
export const STANDARD_FROM = "std.convert.From";

/** The sealed marker trait that every tuple type implements (07-functions.md#r-fn.type.ctor.tuple-trait). */
export const TUPLE_TRAIT = "std.function.Tuple";

/**
 * `all!`, the one combinator with no written signature, so `lib/std` cannot
 * declare it (11-requirements-and-suspension.md#r-req.combinator.all-typing).
 */
export const ALL_COMBINATOR = "std.task.all";

/** `race!`, declared in `lib/std/task.hd`; a call needs a task argument. */
export const RACE_COMBINATOR = "std.task.race";

/**
 * The `@intrinsic` name of `all_frame` in `lib/std/task.hd`, the polling
 * frame that each `all!` call drives.
 */
export const ALL_FRAME_INTRINSIC = "task_all_frame";

// `std.ops` and `std.process` are hd sources in `lib/std/`, declared by
// standard-library.ts under a program's local names or hidden names such
// as these.
export const HIDDEN_EXIT_CODE = "__std_process_ExitCode";
export const HIDDEN_TERMINATION = "__std_process_Termination";

// Runtime type identity (spec/lang/09-traits.md#runtime-type-identity). Importing
// any `std.inspect` name, as `std.error` does, declares `lib/std/inspect.hd`,
// the sealed trait and `TypeId`, under their standard names; aliases are not
// supported. The `downcast` methods, `downcast_val`, and `TypeId::of` are
// checker intrinsics, so the trait's dictionary holds `runtime_type` alone.
export const INSPECTABLE = "Inspectable";
export const STANDARD_INSPECTABLE = "std.inspect.Inspectable";
export const TYPE_ID = "TypeId";
export const STANDARD_DOWNCAST_VAL = "std.inspect.downcast_val";
/** Members of the sealed trait that no subtrait or implementation may write. */
export const INSPECTABLE_MEMBERS: ReadonlySet<string> = new Set([
  "runtime_type",
  "downcast",
  "downcast_mut",
]);
const INSPECT_IMPORTS = new Set([
  STANDARD_INSPECTABLE,
  "std.inspect.TypeId",
  STANDARD_DOWNCAST_VAL,
]);

function respan<T>(value: T, span: SourceSpan): T {
  if (Array.isArray(value)) return value.map((item) => respan(item, span)) as T;
  if (!value || typeof value !== "object") return value;
  const result: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(value))
    result[key] = key === "span" ? span : respan(child, span);
  return result as T;
}

/**
 * Declares `std.inspect` when the program, or a std module it joins, such as
 * `std.error`, uses one of its names.
 */
export function withStandardTraits(program: Program): Program {
  if (program.traits.some((declaration) => declaration.standardName === STANDARD_INSPECTABLE))
    return program;
  const inspect = program.uses.find((declaration) =>
    declaration.names.some((imported) =>
      INSPECT_IMPORTS.has(`${declaration.module}.${imported.name}`),
    ),
  )?.span;
  const parsed = inspect && parse(standardSource("inspect")).program;
  if (!inspect || !parsed) return program;
  const declarations = [...program.data, ...program.enums, ...program.traits];
  const renames = new Map<string, string>();
  for (const use of parsed.uses) {
    for (const imported of use.names) {
      const standardName = `${use.module}.${imported.name}`;
      const declaration = declarations.find((item) => item.standardName === standardName);
      if (declaration) renames.set(imported.alias ?? imported.name, declaration.name);
    }
  }
  for (const implementation of parsed.implementations) {
    const name = implementation.traitName?.split("[")[0];
    if (!name || renames.has(name)) continue;
    const binding = standardPreludeBinding(program, name);
    if (binding) renames.set(name, binding);
  }
  const bound = renameStandardBindings(parsed, renames);
  const traits = bound.traits.map((declaration, index) => ({
    ...declaration,
    standard: true as const,
    standardName: `std.inspect.${parsed.traits[index]!.name}`,
  }));
  const data = bound.data.map((declaration, index) => ({
    ...declaration,
    standard: true as const,
    standardName: `std.inspect.${parsed.data[index]!.name}`,
  }));
  const implementations = bound.implementations.map((declaration) => ({
    ...declaration,
    standard: true as const,
  }));
  return {
    ...program,
    traits: [...program.traits, ...respan(traits, inspect)],
    data: [...program.data, ...respan(data, inspect)],
    implementations: [...program.implementations, ...respan(implementations, inspect)],
  };
}
