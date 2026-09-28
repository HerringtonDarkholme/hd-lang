import type { DataDecl, ImplDecl, Program, TraitDecl, TypeDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";

// Standard traits that a module imports rather than receiving from the
// prelude (spec/09-traits.md#conversion-trait and #error-trait). The prototype
// compiles one module, so an imported standard trait is declared in it under
// its local name, with every span pointing at the use declaration.
const STANDARD_TRAITS: Readonly<Record<string, (name: string) => string>> = {
  "std.convert.From": (name) => `trait ${name}[T]:\n    fn from(value: T) -> Self\n`,
  "std.error.Error": (name) => `trait ${name} < Display + ${INSPECTABLE}\n`,
  // 09-traits.md#literal-suffix-trait
  "std.ops.LiteralSuffix": (name) => `trait ${name}[In, Out]:\n    fn from_literal(n: In) -> Out\n`,
};

export const STANDARD_FROM = "std.convert.From";

// `std.time` (spec/10-modules.md#r-module.prelude.time and
// 05-expressions.md#literal-suffixes): `Duration` and the suffix newtypes,
// each implementing `LiteralSuffix[i64, Duration]`. The spec names no
// `Duration` member, so the prototype's `nanos` field is its own. A name
// the module does not import is declared under a hidden name, and each
// suffix scales its literal by checked `i64` multiplication.
const TIME_UNITS: Readonly<Record<string, string>> = {
  ns: "1",
  us: "1000",
  ms: "1000000",
  s: "1000000000",
  min: "60000000000",
  h: "3600000000000",
};

function timeSource(
  imported: ReadonlyMap<string, string>,
  suffixTrait: string,
  declareTrait: boolean,
): string {
  const duration = imported.get("Duration") ?? "__std_time_Duration";
  const lines = declareTrait ? [STANDARD_TRAITS["std.ops.LiteralSuffix"]!(suffixTrait)] : [];
  lines.push(`data ${duration}:\n    nanos: i64\n`);
  for (const [unit, scale] of Object.entries(TIME_UNITS)) {
    const local = imported.get(unit);
    if (!local) continue;
    const nanos = scale === "1" ? "n" : `n * ${scale}`;
    lines.push(
      `type ${local}(i64)\n`,
      `impl ${suffixTrait}[i64, ${duration}] for ${local}:\n    fn from_literal(n: i64) -> ${duration}: ${duration} { nanos: ${nanos} }\n`,
    );
  }
  return lines.join("\n");
}

// `std.process` (spec/10-modules.md#exit-status): importing `ExitCode` or
// `Termination` declares both, with the implementations for `ExitCode`,
// `void`, and `Result[T, E]`; a name not imported gets a hidden name.
export const HIDDEN_EXIT_CODE = "__std_process_ExitCode";
export const HIDDEN_TERMINATION = "__std_process_Termination";

function processSource(exitCode: string, termination: string): string {
  return `pub type ${exitCode}(u8)

pub trait ${termination}:
    fn report(self) -> ${exitCode}

impl ${termination} for ${exitCode}:
    fn report(self) -> ${exitCode}: self

impl ${termination} for void:
    fn report(self) -> ${exitCode}: ${exitCode}(0)

impl[T < ${termination}, E < Display] ${termination} for Result[T, E]:
    fn report(self) -> ${exitCode}:
        match self:
            .Ok(value) => value.report()
            .Err(_) => ${exitCode}(1)
`;
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
  const types: TypeDecl[] = [];
  let inspect: SourceSpan | undefined;
  let time: SourceSpan | undefined;
  const timeNames = new Map<string, string>();
  let suffixTrait: string | undefined;
  let process: SourceSpan | undefined;
  const processNames = new Map<string, string>();
  for (const declaration of program.uses) {
    for (const imported of declaration.names) {
      const qualified = `${declaration.module}.${imported.name}`;
      if (qualified === "std.process.ExitCode" || qualified === "std.process.Termination") {
        process ??= declaration.span;
        processNames.set(imported.name, imported.alias ?? imported.name);
      }
      if (
        declaration.module === "std.time" &&
        (imported.name === "Duration" || imported.name in TIME_UNITS)
      ) {
        time ??= declaration.span;
        timeNames.set(imported.name, imported.alias ?? imported.name);
      }
      if (qualified === "std.ops.LiteralSuffix") suffixTrait = imported.alias ?? imported.name;
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
  if (time) {
    const parsed = parse(
      timeSource(timeNames, suffixTrait ?? "__std_ops_LiteralSuffix", suffixTrait === undefined),
    ).program;
    if (parsed) {
      traits.push(...respan(parsed.traits, time));
      data.push(...respan(parsed.data, time));
      types.push(...respan(parsed.types ?? [], time));
      implementations.push(...respan(parsed.implementations, time));
    }
  }
  if (process) {
    const parsed = parse(
      processSource(
        processNames.get("ExitCode") ?? HIDDEN_EXIT_CODE,
        processNames.get("Termination") ?? HIDDEN_TERMINATION,
      ),
    ).program;
    if (parsed) {
      traits.push(...respan(parsed.traits, process));
      types.push(...respan(parsed.types ?? [], process));
      implementations.push(...respan(parsed.implementations, process));
    }
  }
  if (traits.length === 0 && data.length === 0) return program;
  return {
    ...program,
    types: [...(program.types ?? []), ...types],
    traits: [...program.traits, ...traits],
    data: [...program.data, ...data],
    implementations: [...program.implementations, ...implementations],
  };
}
