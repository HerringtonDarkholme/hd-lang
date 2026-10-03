import type { UseDecl } from "../ast.ts";
import { parse } from "../parser/index.ts";
import { standardSource } from "./standard-sources.ts";

// The prelude is `lib/std/prelude.hd`, a module of `use` lines: every
// module has the names of its `pub use` lines, as if it began with them
// (spec/lang/10-modules.md#r-module.prelude.fixed-uses). Test code also has
// those of `lib/std/prelude/testing.hd` (r-module.prelude.test-only); they
// are prelude names everywhere, so no module binds them to another
// declaration (r-module.prelude.no-shadow).

function preludeUses(): readonly UseDecl[] {
  return (["prelude", "prelude.testing"] as const).flatMap((module) => {
    const parsed = parse(standardSource(module), { standardLibrary: true });
    if (!parsed.program || parsed.diagnostics.length > 0)
      throw new Error(`std.${module} does not parse`);
    return parsed.program.uses;
  });
}

/** The `use` lines of `std.prelude` and `std.prelude.testing`. */
const PRELUDE_USES: readonly UseDecl[] = preludeUses();

/**
 * Each prelude name, with the module that supplies it, as `std.cmp` for
 * `Eq`. A `use` of that same declaration under its own name is allowed and
 * changes nothing (spec/lang/10-modules.md#r-module.prelude.same-use).
 */
export const PRELUDE_ORIGINS: ReadonlyMap<string, string> = new Map(
  PRELUDE_USES.filter((use) => use.public === true).flatMap((use) =>
    use.names.map((imported) => [imported.alias ?? imported.name, use.module] as const),
  ),
);

/**
 * Every prelude name, which no declaration or binding may shadow
 * (spec/lang/10-modules.md#r-module.prelude.no-shadow).
 */
export const PRELUDE_NAMES: ReadonlySet<string> = new Set(PRELUDE_ORIGINS.keys());
