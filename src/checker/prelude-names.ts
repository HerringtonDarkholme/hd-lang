import type { UseDecl } from "../ast.ts";
import { DiagnosticError } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import { withStandardSource } from "./standard-provenance.ts";
import { standardDocument } from "./standard-sources.ts";

// The prelude is `lib/std/prelude.hd`, a module of `use` lines: every
// module has the names of its `pub use` lines, as if it began with them
// (spec/lang/10-modules.md#r-module.prelude.fixed-uses). Test code also has
// those of `lib/std/prelude/testing.hd` (r-module.prelude.test-only); they
// are prelude names everywhere, so no module binds them to another
// declaration (r-module.prelude.no-shadow).

function preludeUses(): readonly UseDecl[] {
  return (["prelude", "prelude.testing"] as const).flatMap((module) => {
    const document = standardDocument(module);
    const parsed = parse(document.text, { standardLibrary: true });
    if (!parsed.program || parsed.diagnostics.length > 0)
      throw new DiagnosticError(
        parsed.diagnostics.map((diagnostic) =>
          withStandardSource(diagnostic, document, diagnostic.span),
        ),
      );
    return parsed.program.uses;
  });
}

let origins: ReadonlyMap<string, string> | undefined;
let names: ReadonlySet<string> | undefined;

function preludeOrigins(): ReadonlyMap<string, string> {
  if (origins) return origins;
  origins = new Map(
    preludeUses()
      .filter((use) => use.public === true)
      .flatMap((use) =>
        use.names.map((imported) => [imported.alias ?? imported.name, use.module] as const),
      ),
  );
  return origins;
}

function preludeNames(): ReadonlySet<string> {
  names ??= new Set(preludeOrigins().keys());
  return names;
}

class PreludeOrigins implements ReadonlyMap<string, string> {
  get size(): number {
    return preludeOrigins().size;
  }
  get(name: string): string | undefined {
    return preludeOrigins().get(name);
  }
  has(name: string): boolean {
    return preludeOrigins().has(name);
  }
  entries(): MapIterator<[string, string]> {
    return preludeOrigins().entries();
  }
  keys(): MapIterator<string> {
    return preludeOrigins().keys();
  }
  values(): MapIterator<string> {
    return preludeOrigins().values();
  }
  forEach(
    callback: (value: string, key: string, map: ReadonlyMap<string, string>) => void,
    thisArgument?: unknown,
  ): void {
    preludeOrigins().forEach(callback, thisArgument);
  }
  [Symbol.iterator](): MapIterator<[string, string]> {
    return preludeOrigins()[Symbol.iterator]();
  }
}

class PreludeNames implements ReadonlySet<string> {
  get size(): number {
    return preludeNames().size;
  }
  has(name: string): boolean {
    return preludeNames().has(name);
  }
  entries(): SetIterator<[string, string]> {
    return preludeNames().entries();
  }
  keys(): SetIterator<string> {
    return preludeNames().keys();
  }
  values(): SetIterator<string> {
    return preludeNames().values();
  }
  forEach(
    callback: (value: string, valueAgain: string, set: ReadonlySet<string>) => void,
    thisArgument?: unknown,
  ): void {
    preludeNames().forEach(callback, thisArgument);
  }
  [Symbol.iterator](): SetIterator<string> {
    return preludeNames()[Symbol.iterator]();
  }
}

/**
 * Each prelude name, with the module that supplies it, as `std.cmp` for
 * `Eq`. A `use` of that same declaration under its own name is allowed and
 * changes nothing (spec/lang/10-modules.md#r-module.prelude.same-use).
 */
export const PRELUDE_ORIGINS: ReadonlyMap<string, string> = new PreludeOrigins();

/**
 * Every prelude name, which no declaration or binding may shadow
 * (spec/lang/10-modules.md#r-module.prelude.no-shadow).
 */
export const PRELUDE_NAMES: ReadonlySet<string> = new PreludeNames();
