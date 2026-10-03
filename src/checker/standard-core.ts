import type { ValueType } from "../hir.ts";

// Compiler-owned std.core identity that needs resolution before ordinary type
// checking. `usize` is a transparent alias, so it must disappear before
// lowering rather than becoming another primitive ValueType.

/** Compiler-owned transparent aliases, by their spelling in std.core. */
export const STANDARD_CORE_TYPE_ALIASES: ReadonlyMap<string, ValueType> = new Map([
  ["usize", "u32"],
]);

/** Compiler-owned std.core declarations accepted by the std-use validator. */
export const STANDARD_CORE_NAMES: readonly string[] = [...STANDARD_CORE_TYPE_ALIASES.keys()];

/** The canonical type of a local or qualified compiler-owned alias spelling. */
export function standardCoreTypeAlias(name: string): ValueType | undefined {
  const prefix = "std.core.";
  return STANDARD_CORE_TYPE_ALIASES.get(name.startsWith(prefix) ? name.slice(prefix.length) : name);
}
