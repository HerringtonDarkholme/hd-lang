import type { DataField, Program, TypeDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { readonlyType } from "../types.ts";

// ---------------------------------------------------------------------------
// The derive checks of the comparison traits, whose implementations come from
// the std templates (spec/lang/09-traits.md#derived-implementations).

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
