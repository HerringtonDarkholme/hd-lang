import type { DataField, Program, TypeDecl } from "../ast.ts";
import { sourceSpanKey, type SourceSpan } from "../diagnostics.ts";
import { readonlyType, displayType } from "../types.ts";

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
 * Origins keyed by span value, not span object: a pass that copies a span,
 * as generated-source patching does, still denotes the same origin (O-11).
 */
class SpanOrigins<Value> {
  private readonly entries = new Map<string, Value>();
  get(span: SourceSpan): Value | undefined {
    return this.entries.get(sourceSpanKey(span));
  }
  set(span: SourceSpan, value: Value): void {
    this.entries.set(sourceSpanKey(span), value);
  }
}

class SpanOriginSet {
  private readonly keys = new Set<string>();
  has(span: SourceSpan): boolean {
    return this.keys.has(sourceSpanKey(span));
  }
  add(span: SourceSpan): this {
    this.keys.add(sourceSpanKey(span));
    return this;
  }
}

/**
 * The derive origins of one compilation. Its typed derivation registers them,
 * and only the checker of the same program reads them: span keys carry no
 * program identity, so a module-level registry would match another program's
 * line and column (checker/program.ts).
 */
export class DerivedOrigins {
  /**
   * The origins of generated lines that compare or hash one field. A trait
   * error there is `derive-field-missing-trait` at the field
   * (spec/lang/09-traits.md#r-trait.derive.field-missing-trait).
   */
  readonly fieldChecks = new SpanOrigins<DerivedFieldCheck>();
  /**
   * The origins of derived implementations of the comparison traits. An unmet
   * bound of one of their methods is `missing-derived-bound` at the use
   * (spec/lang/09-traits.md#r-trait.derive.bound-unmet).
   */
  readonly implementations = new SpanOriginSet();
}

const registered = new WeakMap<object, DerivedOrigins>();

/**
 * Records a program's origins under `key`, an object every function check of
 * the program shares, such as its trait map. The lookups below take that key.
 */
export function registerDerivedOrigins(key: object, origins: DerivedOrigins): void {
  registered.set(key, origins);
}

/** Whether `span` is the origin of a derived implementation of a comparison trait. */
export function isDerivedImplementation(key: object, span: SourceSpan): boolean {
  return registered.get(key)?.implementations.has(span) === true;
}

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
  "type-mismatch",
]);

/**
 * A trait error on a generated line that compares or hashes one field is
 * `derive-field-missing-trait`, naming the trait and the field
 * (spec/lang/09-traits.md#r-trait.derive.field-missing-trait); any other
 * diagnostic is unchanged.
 */
export function derivedFieldDiagnostic(
  key: object,
  code: string,
  message: string,
  span: SourceSpan,
): { readonly code: string; readonly message: string } {
  const field = registered.get(key)?.fieldChecks.get(span);
  if (!field || !DERIVED_FIELD_CODES.has(code)) return { code, message };
  return {
    code: "derive-field-missing-trait",
    message: field.base
      ? `base type '${field.field}' of newtype '${field.owner}' does not implement ${field.trait}, which @derive(${field.trait}) requires`
      : `field '${field.field}' of '${field.owner}' does not implement ${field.trait}, which @derive(${field.trait}) requires`,
  };
}

/** A fresh span for the line that handles `field`, registered for its diagnostic. */
export function derivedFieldSpan(
  origins: DerivedOrigins,
  field: DataField,
  trait: string,
  owner: string,
): SourceSpan {
  const span = { ...field.span };
  origins.fieldChecks.set(span, {
    trait,
    owner,
    field: field.positional ? `_${field.name}` : field.name,
  });
  return span;
}

/** A fresh span for the line that forwards to a newtype's base type, registered for its diagnostic. */
export function derivedBaseSpan(
  origins: DerivedOrigins,
  declaration: TypeDecl,
  trait: string,
): SourceSpan {
  const base = declaration.base!;
  const span = { ...base.span };
  origins.fieldChecks.set(span, {
    trait,
    owner: declaration.name,
    field: displayType(base.name),
    base: true,
  });
  return span;
}

/** A fresh span for a derived implementation, registered as derived. */
export function derivedImplementationSpan(origins: DerivedOrigins, span: SourceSpan): SourceSpan {
  const fresh = { ...span };
  origins.implementations.add(fresh);
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
          `deriving ${displayType(trait.name)} for '${declaration.name}' requires deriving ${missing} in the same list`,
          trait.span,
        );
      else if (mixed)
        error(
          "mixed-derived-law",
          `'${declaration.name}' derives ${displayType(trait.name)} but implements its law partner ${mixed} by hand; derive both or write both`,
          trait.span,
        );
    }
  }
}
