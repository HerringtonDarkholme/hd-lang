import type { DataField, FunctionDecl, ImplDecl, Program, TypeDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { readonlyType } from "../types.ts";
import { Source_, ZERO_SPAN } from "./generated-source.ts";
import type { Target } from "./member-lines.ts";

// ---------------------------------------------------------------------------
// The derive checks of the comparison traits, whose implementations come from
// the std templates (spec/lang/09-traits.md#derived-implementations), and the
// intrinsic `@derive(Debug)` (#debug-trait).

/** What a derived field line compares or hashes, for its diagnostic. */
interface DerivedFieldCheck {
  readonly trait: string;
  readonly owner: string;
  readonly field: string;
  /** The field is a newtype's base type (spec/lang/09-traits.md#r-trait.derive.newtype.requires.error). */
  readonly base?: boolean;
}

/**
 * The spans of generated lines that compare or hash one field. A trait
 * error there is `derive-field-missing-trait` at the field
 * (spec/lang/09-traits.md#r-trait.derive.field-missing-trait).
 */
const DERIVED_FIELD_CHECKS = new WeakMap<SourceSpan, DerivedFieldCheck>();

/**
 * The spans of derived implementations of the comparison traits. An unmet
 * bound of one of their methods is `missing-derived-bound` at the use
 * (spec/lang/09-traits.md#r-trait.derive.bound-unmet).
 */
export const DERIVED_IMPLEMENTATION_SPANS = new WeakSet<SourceSpan>();

/** The traits whose derivation reports `derive-field-missing-trait` and `missing-derived-bound`. */
export const DERIVE_CHECKED_TRAITS: ReadonlySet<string> = new Set([
  "Eq",
  "PartialOrd",
  "Ord",
  "Hash",
]);

/** The trait errors a derived field line reports as `derive-field-missing-trait`. */
const DERIVED_FIELD_CODES: ReadonlySet<string> = new Set([
  "unsatisfied-trait-bound",
  "missing-eq",
  "missing-partial-ord",
  "unsupported-equality",
]);

/**
 * A trait error on a generated line that compares or hashes one field is
 * `derive-field-missing-trait`, naming the trait and the field
 * (spec/lang/09-traits.md#r-trait.derive.field-missing-trait); any other
 * diagnostic is unchanged.
 */
export function derivedFieldDiagnostic(
  code: string,
  message: string,
  span: SourceSpan,
): { readonly code: string; readonly message: string } {
  const field = DERIVED_FIELD_CHECKS.get(span);
  if (!field || !DERIVED_FIELD_CODES.has(code)) return { code, message };
  return {
    code: "derive-field-missing-trait",
    message: field.base
      ? `base type '${field.field}' of newtype '${field.owner}' does not implement ${field.trait}, which @derive(${field.trait}) requires`
      : `field '${field.field}' of '${field.owner}' does not implement ${field.trait}, which @derive(${field.trait}) requires`,
  };
}

/** A fresh span for the line that handles `field`, registered for its diagnostic. */
export function derivedFieldSpan(field: DataField, trait: string, owner: string): SourceSpan {
  const span = { ...field.span };
  DERIVED_FIELD_CHECKS.set(span, {
    trait,
    owner,
    field: field.positional ? `_${field.name}` : field.name,
  });
  return span;
}

/** A fresh span for the line that forwards to a newtype's base type, registered for its diagnostic. */
export function derivedBaseSpan(declaration: TypeDecl, trait: string): SourceSpan {
  const base = declaration.base!;
  const span = { ...base.span };
  DERIVED_FIELD_CHECKS.set(span, { trait, owner: declaration.name, field: base.name, base: true });
  return span;
}

/** A fresh span for a derived implementation, registered as derived. */
export function derivedImplementationSpan(span: SourceSpan): SourceSpan {
  const fresh = { ...span };
  DERIVED_IMPLEMENTATION_SPANS.add(fresh);
  return fresh;
}

/** Starts `impl[T < Trait] Trait for Target:` and returns the target's placeholder. */
function derivedImpl(target: Target, trait: string, out: Source_): string {
  const { name, genericParameters: parameters } = target.declaration;
  const T = out.type(parameters.length > 0 ? `${name}[${parameters.join(",")}]` : name);
  const bounds = parameters.map((parameter) => `${parameter} < ${trait}`).join(", ");
  out.add(`impl${parameters.length > 0 ? `[${bounds}]` : ""} ${trait} for ${T}:`);
  return T;
}

function deriveDebug(target: Target, writer: string, span: SourceSpan): ImplDecl {
  const out = new Source_();
  derivedImpl(target, "Debug", out);
  out.add(`    fn debug(self, out: mut ${writer}) -> void:`);
  // One builder per value, as Rust's derive does (std-format.debug.derive-builders.mapping):
  // `debug_struct` for a data type, even a fieldless one, and for a variant
  // with named members; `debug_tuple` for a variant with positional ones; and
  // the bare name for a variant without a payload. A variant that mixes both
  // uses `debug_struct`, naming a positional field `_0`, `_1`, and so on
  // (std-format.debug.derive-builders.mixed).
  const label = (member: DataField): string =>
    member.positional ? `_${member.name}` : member.name;
  const struct = (members: readonly DataField[], name: string, value: (index: number) => string) =>
    `out.debug_struct(${out.string(name)})${members.map((member, index) => `.field(${out.string(label(member))}, ${value(index)})`).join("")}.finish()`;
  const fields = (members: readonly DataField[], name: string, value: (index: number) => string) =>
    members.length === 0
      ? `out.write(${out.string(name)})`
      : members.every((member) => member.positional)
        ? `out.debug_tuple(${out.string(name)})${members.map((_, index) => `.field(${value(index)})`).join("")}.finish()`
        : struct(members, name, value);
  if (target.kind === "data") {
    const members = target.declaration.fields;
    out.add(
      `        ${struct(members, target.declaration.name, (index) => `self.${members[index]!.name}`)}`,
    );
  } else {
    const { name, variants } = target.declaration;
    if (variants.length === 0) out.add("        pass");
    else out.add("        match self:");
    for (const variant of variants) {
      const bound = variant.fields.map((_, index) => `hd_v${index}`);
      const pattern =
        bound.length === 0
          ? `${name}.${variant.name}`
          : `${name}.${variant.name}(${bound.join(", ")})`;
      out.add(
        `            ${pattern} => ${fields(variant.fields, variant.name, (index) => bound[index]!)}`,
      );
    }
  }
  return out.program(span).implementations[0]!;
}

interface IntrinsicDerivation {
  readonly trait: string;
  readonly target: Target;
  readonly span: SourceSpan;
}

const DEBUG = "hd__debug";

interface NewtypeDerivation {
  readonly trait: string;
  readonly declaration: TypeDecl;
  readonly span: SourceSpan;
}

/**
 * `@derive(Debug)` on a newtype applies the base type's method to the
 * unwrapped value (spec/lang/09-traits.md#derived-newtypes).
 */
function deriveNewtypeDebug(item: NewtypeDerivation, writer: string): ImplDecl {
  const { name, genericParameters: parameters, base } = item.declaration;
  const out = new Source_();
  const T = out.type(parameters.length > 0 ? `${name}[${parameters.join(",")}]` : name);
  const bounds = parameters.map((parameter) => `${parameter} < ${item.trait}`).join(", ");
  out.add(`impl${parameters.length > 0 ? `[${bounds}]` : ""} ${item.trait} for ${T}:`);
  const self = `${readonlyType(base!.name).split("[")[0]}(self)`;
  // A base type without the trait is `derive-field-missing-trait` at the
  // base type (spec/lang/09-traits.md#r-trait.derive.newtype.requires.error).
  out.add(
    `    fn debug(self, out: mut ${writer}) -> void: ${DEBUG}(${self}, out)`,
    derivedBaseSpan(item.declaration, item.trait),
  );
  return out.program(item.span).implementations[0]!;
}

/** The generic helper that a newtype's derived `Debug` calls. */
export function intrinsicHelpers(
  newtypes: readonly NewtypeDerivation[],
  writer: string,
): FunctionDecl[] {
  if (newtypes.length === 0) return [];
  const out = new Source_();
  out.add(`fn ${DEBUG}[T < Debug](value: T, out: mut ${writer}) -> void:`);
  out.add(`    value.debug(out)`);
  return [...out.program(ZERO_SPAN).functions];
}

// ---------------------------------------------------------------------------
// Law partners (spec/lang/09-traits.md#law-partners).

const LAW_PARTNERS: Readonly<Record<string, readonly string[]>> = {
  Eq: ["Hash", "PartialOrd", "Ord"],
  Hash: ["Eq"],
  PartialOrd: ["Eq", "Ord"],
  Ord: ["Eq", "PartialOrd"],
};

const REQUIRED_PARTNERS: Readonly<Record<string, readonly string[]>> = {
  Hash: ["Eq"],
  PartialOrd: ["Eq"],
  Ord: ["Eq", "PartialOrd"],
};

/**
 * Deriving `Hash`, `PartialOrd`, or `Ord` needs its law partners in the same
 * list, and a derived and a hand-written law partner never coexist; either
 * is `mixed-derived-law` on the `@derive` line.
 */
export function checkLawPartners(
  program: Program,
  error: (code: string, message: string, span: SourceSpan) => void,
): void {
  const handWritten = new Map<string, Set<string>>();
  for (const implementation of program.implementations) {
    const trait = implementation.traitName;
    if (!trait || !LAW_PARTNERS[trait] || implementation.standard) continue;
    const target = readonlyType(implementation.targetName).split("[")[0]!;
    handWritten.set(target, new Set([...(handWritten.get(target) ?? []), trait]));
  }
  const declarations = [...program.data, ...program.enums, ...(program.types ?? [])];
  for (const declaration of declarations) {
    const derives = (declaration.decorators?.derives ?? []).filter(
      (trait) => LAW_PARTNERS[trait.name],
    );
    const derived = new Set(derives.map((trait) => trait.name));
    const written = handWritten.get(declaration.name) ?? new Set<string>();
    for (const trait of derives) {
      const missing = (REQUIRED_PARTNERS[trait.name] ?? []).find(
        (partner) => !derived.has(partner) && !written.has(partner),
      );
      const mixed = LAW_PARTNERS[trait.name]!.find((partner) => written.has(partner));
      if (missing)
        error(
          "mixed-derived-law",
          `deriving ${trait.name} for '${declaration.name}' requires deriving ${missing} in the same list`,
          trait.span,
        );
      else if (mixed)
        error(
          "mixed-derived-law",
          `'${declaration.name}' derives ${trait.name} but implements its law partner ${mixed} by hand; derive both or write both`,
          trait.span,
        );
    }
  }
}

/** Every intrinsic `@derive(Debug)` implementation. */
export function deriveIntrinsics(
  intrinsic: readonly IntrinsicDerivation[],
  newtypes: readonly NewtypeDerivation[],
  writer: string,
): ImplDecl[] {
  return [
    ...intrinsic.map((item) => deriveDebug(item.target, writer, item.span)),
    ...newtypes.map((item) => deriveNewtypeDebug(item, writer)),
  ];
}
