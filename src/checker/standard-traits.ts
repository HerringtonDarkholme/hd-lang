import type { DataDecl, ImplDecl, Program, TraitDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";

// Standard traits that a module imports rather than receiving from the
// prelude (spec/09-traits.md#conversion-trait and #error-trait). The prototype
// compiles one module, so an imported standard trait is declared in it under
// its local name, with every span pointing at the use declaration.
const STANDARD_TRAITS: Readonly<Record<string, (name: string) => string>> = {
  "std.convert.From": (name) => `trait ${name}[T]:\n    fn from(value: T) -> Self\n`,
  "std.error.Error": (name) => `trait ${name} < Display & ${INSPECTABLE}\n`,
};

export const STANDARD_FROM = "std.convert.From";

// `std.time`, `std.ops`, and `std.process` are hd sources in `lib/std/`,
// declared by standard-library.ts under a program's local names or hidden
// names such as these.
export const HIDDEN_DURATION = "__std_time_Duration";

/** The local name of `std.time.Duration`, or its hidden name. */
export function durationName(uses: Program["uses"]): string {
  for (const declaration of uses)
    for (const imported of declaration.names)
      if (declaration.module === "std.time" && imported.name === "Duration")
        return imported.alias ?? imported.name;
  return HIDDEN_DURATION;
}

export const HIDDEN_EXIT_CODE = "__std_process_ExitCode";
export const HIDDEN_TERMINATION = "__std_process_Termination";

// `std.format.DebugWriter` (spec/09-traits.md#debug-trait): the spec leaves
// its builder calls to the standard library, so the prototype declares it
// with no members, under its imported name or a hidden one.
export const HIDDEN_DEBUG_WRITER = "__std_format_DebugWriter";

/** The local name of `std.format.DebugWriter`, or its hidden name. */
export function debugWriterName(uses: Program["uses"]): string {
  for (const declaration of uses)
    for (const imported of declaration.names)
      if (declaration.module === "std.format" && imported.name === "DebugWriter")
        return imported.alias ?? imported.name;
  return HIDDEN_DEBUG_WRITER;
}

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
  // `DebugWriter`, which the prelude `Debug` names, is hd code in
  // `lib/std/format.hd`, joined when a program mentions `Debug`. Any other
  // program gets an empty declaration of the name, which nothing uses.
  const writer = debugWriterName(program.uses);
  if (!program.data.some((declaration) => declaration.name === writer))
    data.push(...respan(parse(`pub data ${writer}: pass\n`).program!.data, program.span));
  if (traits.length === 0 && data.length === 0) return program;
  return {
    ...program,
    traits: [...program.traits, ...traits],
    data: [...program.data, ...data],
    implementations: [...program.implementations, ...implementations],
  };
}
