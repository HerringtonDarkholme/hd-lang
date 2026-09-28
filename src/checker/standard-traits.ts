import type { DataDecl, ImplDecl, Program, TraitDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";

// Standard traits that a module imports rather than receiving from the
// prelude (spec/09-traits.md#conversion-trait and #error-trait). The prototype
// compiles one module, so an imported standard trait is declared in it under
// its local name, with every span pointing at the use declaration.
// `std.process.Termination` and `ExitCode` (spec/10-modules.md#exit-status)
// are not declared: `ExitCode` wraps a `u8`, which the prototype lacks.
const STANDARD_TRAITS: Readonly<Record<string, (name: string) => string>> = {
  "std.convert.From": (name) => `trait ${name}[T]:\n    fn from(value: T) -> Self\n`,
  "std.error.Error": (name) => `trait ${name} < Display + ${INSPECTABLE}\n`,
};

export const STANDARD_FROM = "std.convert.From";

// Runtime type identity (spec/09-traits.md#runtime-type-identity). Importing
// any `std.inspect` name, or `std.error.Error`, declares the sealed trait and
// `TypeId` under their standard names; aliases are not supported. `TypeId` is
// a data type holding the canonical printable name, which identifies the type
// because the prototype compiles one module. The `downcast` methods,
// `downcast_val`, and `TypeId::of` are checker intrinsics, so the trait's
// dictionary holds `runtime_type` alone.
export const INSPECTABLE = "Inspectable";
export const TYPE_ID = "TypeId";
export const STANDARD_DOWNCAST_VAL = "std.inspect.downcast_val";
/** Members of the sealed trait that no subtrait or implementation may write. */
export const INSPECTABLE_MEMBERS: ReadonlySet<string> = new Set([
  "runtime_type",
  "downcast",
  "downcast_mut",
]);
const INSPECT_IMPORTS = new Set([
  "std.inspect.Inspectable",
  "std.inspect.TypeId",
  STANDARD_DOWNCAST_VAL,
  "std.error.Error",
]);
const INSPECT_SOURCE = `trait ${INSPECTABLE}:
    fn runtime_type(self) -> ${TYPE_ID}

data ${TYPE_ID}:
    key: string

impl Eq for ${TYPE_ID}:
    fn eq(self, other: ${TYPE_ID}) -> bool: self.key == other.key

impl Display for ${TYPE_ID}:
    fn to_string(self) -> string: self.key
`;

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
  const data: DataDecl[] = [];
  const implementations: ImplDecl[] = [];
  let inspect: SourceSpan | undefined;
  for (const declaration of program.uses) {
    for (const imported of declaration.names) {
      const qualified = `${declaration.module}.${imported.name}`;
      if (INSPECT_IMPORTS.has(qualified)) inspect ??= declaration.span;
      const source = STANDARD_TRAITS[qualified];
      if (!source) continue;
      const parsed = parse(source(imported.alias ?? imported.name));
      const trait = parsed.program?.traits[0];
      if (trait) traits.push(respan(trait, declaration.span));
    }
  }
  if (inspect) {
    const parsed = parse(INSPECT_SOURCE).program;
    if (parsed) {
      traits.push(...respan(parsed.traits, inspect));
      data.push(...respan(parsed.data, inspect));
      implementations.push(...respan(parsed.implementations, inspect));
    }
  }
  if (traits.length === 0) return program;
  return {
    ...program,
    traits: [...program.traits, ...traits],
    data: [...program.data, ...data],
    implementations: [...program.implementations, ...implementations],
  };
}
