import type { TypeDecl } from "../ast.ts";

/** Nullary type aliases by name, for matching a written type to its expanded spelling. */
export function nullaryTypeAliases(types: readonly TypeDecl[] | undefined): Map<string, string> {
  return new Map(
    (types ?? []).flatMap((declaration) =>
      declaration.alias && declaration.genericParameters.length === 0
        ? [[declaration.name, declaration.alias.name] as const]
        : [],
    ),
  );
}

/** A type with nullary aliases expanded, stopping at applied names and cycles. */
export function expandTypeAlias(type: string, aliases: ReadonlyMap<string, string>): string {
  let current = type;
  const seen = new Set<string>();
  for (;;) {
    if (current.includes("[") || seen.has(current)) return current;
    const target = aliases.get(current);
    if (target === undefined) return current;
    seen.add(current);
    current = target;
  }
}
