import type { Program, TraitDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";

// Standard traits that a module imports rather than receiving from the
// prelude (spec/09-traits.md#conversion-trait, #error-trait). The prototype
// compiles one module, so an imported standard trait is declared in it under
// its local name, with every span pointing at the use declaration.
const STANDARD_TRAITS: Readonly<Record<string, (name: string) => string>> = {
  "std.convert.From": (name) => `trait ${name}[T]:\n    fn from(value: T) -> Self\n`,
  "std.error.Error": (name) => `trait ${name} < Display\n`,
};

export const STANDARD_FROM = "std.convert.From";

function respan<T>(value: T, span: SourceSpan): T {
  if (Array.isArray(value)) return value.map((item) => respan(item, span)) as T;
  if (!value || typeof value !== "object") return value;
  const result: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(value))
    result[key] = key === "span" ? span : respan(child, span);
  return result as T;
}

/** Declares each imported standard trait in the program under its local name. */
export function withStandardTraits(program: Program): Program {
  const traits: TraitDecl[] = [];
  for (const declaration of program.uses) {
    for (const imported of declaration.names) {
      const source = STANDARD_TRAITS[`${declaration.module}.${imported.name}`];
      if (!source) continue;
      const parsed = parse(source(imported.alias ?? imported.name));
      const trait = parsed.program?.traits[0];
      if (trait) traits.push(respan(trait, declaration.span));
    }
  }
  return traits.length === 0 ? program : { ...program, traits: [...program.traits, ...traits] };
}
