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

/** A `@derive` name with no template is unknown when no trait declares it. */
export function deriveMissing(
  traits: ReadonlyMap<string, unknown>,
  name: string,
): readonly [string, string] {
  if (!traits.has(name)) return ["unknown-trait", `unknown trait '${name}'`];
  return [
    "underivable-trait",
    name === "Error"
      ? "Error has no template and is not intrinsic; an error type uses @error"
      : `trait '${name}' has no derivation template`,
  ];
}
