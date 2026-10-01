import type { DataField, FunctionDecl, ImplDecl, Program, TypeDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { readonlyType } from "../types.ts";
import { Source_, ZERO_SPAN } from "./generated-source.ts";
import type { Target } from "./member-lines.ts";

// ---------------------------------------------------------------------------
// The intrinsic `@derive(Eq)` (spec/09-traits.md#comparison-traits) and
// `@derive(Debug)` (#debug-trait).

/** What a derived field line compares or hashes, for its diagnostic. */
interface DerivedFieldCheck {
  readonly trait: string;
  readonly owner: string;
  readonly field: string;
  /** The field is a newtype's base type (spec/09-traits.md#r-trait.derive.newtype.requires.error). */
  readonly base?: boolean;
}

/**
 * The spans of generated lines that compare or hash one field. A trait
 * error there is `derive-field-missing-trait` at the field
 * (spec/09-traits.md#r-trait.derive.field-missing-trait).
 */
const DERIVED_FIELD_CHECKS = new WeakMap<SourceSpan, DerivedFieldCheck>();

/**
 * The spans of intrinsically derived implementations. An unmet bound of one
 * of their methods is `missing-derived-bound` at the use
 * (spec/09-traits.md#r-trait.derive.bound-unmet).
 */
export const DERIVED_IMPLEMENTATION_SPANS = new WeakSet<SourceSpan>();

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
 * (spec/09-traits.md#r-trait.derive.field-missing-trait); any other
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
function fieldSpan(field: DataField, trait: string, owner: string): SourceSpan {
  const span = { ...field.span };
  DERIVED_FIELD_CHECKS.set(span, {
    trait,
    owner,
    field: field.positional ? `_${field.name}` : field.name,
  });
  return span;
}

/** A fresh span for a derived implementation, registered as derived. */
function implementationSpan(span: SourceSpan): SourceSpan {
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

function deriveEq(target: Target, span: SourceSpan): ImplDecl {
  const declaration = target.declaration;
  const out = new Source_();
  const T = derivedImpl(target, "Eq", out);
  out.add(`    fn eq(self, other: ${T}) -> bool:`);
  // One line per field, whose span is the field's, so a field without `Eq`
  // is reported there (trait.derive.field-missing-trait).
  const compare = (field: DataField, left: string, right: string, indent: string): void =>
    out.add(
      `${indent}if !(${left} == ${right}): return false`,
      fieldSpan(field, "Eq", declaration.name),
    );
  if (target.kind === "data") {
    for (const field of target.declaration.fields)
      compare(field, `self.${field.name}`, `other.${field.name}`, "        ");
    out.add("        true");
  } else {
    const variants = target.declaration.variants;
    out.add(`        match (self, other):`);
    for (const variant of variants) {
      const names = (side: string): string[] =>
        variant.fields.map((_, position) => `${side}${position}`);
      const pattern = (side: string): string =>
        variant.fields.length === 0
          ? `${declaration.name}.${variant.name}`
          : `${declaration.name}.${variant.name}(${variant.fields.map((_, position) => names(side)[position]).join(", ")})`;
      if (variant.fields.length === 0) {
        out.add(`            (${pattern("l")}, ${pattern("r")}) => true`);
        continue;
      }
      out.add(`            (${pattern("l")}, ${pattern("r")}) =>`);
      variant.fields.forEach((field, position) =>
        compare(field, names("l")[position]!, names("r")[position]!, "                "),
      );
      out.add("                true");
    }
    if (variants.length > 1) out.add(`            _ => false`);
  }
  return out.program(implementationSpan(span)).implementations[0]!;
}

// ---------------------------------------------------------------------------
// `@derive(PartialOrd)`, `@derive(Ord)`, and `@derive(Hash)`
// (spec/09-traits.md#derived-ordering, #derived-hashing). A field is compared
// or hashed through the generated generic helpers below, whose bounds find a
// built-in or declared implementation. An enum compares, and hashes, its
// variant's declaration position first.

interface IntrinsicDerivation {
  readonly trait: string;
  readonly target: Target;
  readonly span: SourceSpan;
}

const PARTIAL_CMP = "hd__partial_cmp";
const CMP = "hd__cmp";
const HASH = "hd__hash";
const DEBUG = "hd__debug";

function rankName(target: Target): string {
  return `hd__rank_${target.declaration.name}`;
}

/** One ordering step: a decided comparison of `left` and `right` returns. */
function orderStep(total: boolean, left: string, right: string, indent: string): string[] {
  return [
    `${indent}match ${total ? CMP : PARTIAL_CMP}(${left}, ${right}):`,
    `${indent}    ${total ? ".Equal" : ".Some(.Equal)"} => pass`,
    `${indent}    decided => return decided`,
  ];
}

/** Adds an ordering step whose first line carries `field`'s span, when given. */
function addOrderStep(out: Source_, steps: readonly string[], span?: SourceSpan): void {
  steps.forEach((line, index) => out.add(line, index === 0 ? span : undefined));
}

function deriveOrdering(target: Target, total: boolean, span: SourceSpan): ImplDecl {
  const out = new Source_();
  const T = derivedImpl(target, total ? "Ord" : "PartialOrd", out);
  const result = total ? "Ordering" : "Ordering?";
  out.add(`    fn ${total ? "cmp" : "partial_cmp"}(self, other: ${T}) -> ${result}:`);
  const body = "        ";
  const trait = total ? "Ord" : "PartialOrd";
  const owner = target.declaration.name;
  const fields = (members: readonly DataField[]): void => {
    for (const member of members)
      addOrderStep(
        out,
        orderStep(total, `self.${member.name}`, `other.${member.name}`, body),
        fieldSpan(member, trait, owner),
      );
  };
  if (target.kind === "data") fields(target.declaration.fields);
  else {
    const { name, variants, sharedFields } = target.declaration;
    const rank = rankName(target);
    for (const line of orderStep(total, `${rank}(self)`, `${rank}(other)`, body)) out.add(line);
    fields(sharedFields);
    const payloads = variants.filter((variant) => variant.fields.length > 0);
    if (payloads.length > 0) out.add(`${body}match (self, other):`);
    for (const variant of payloads) {
      const names = (side: string): string[] => variant.fields.map((_, index) => `${side}${index}`);
      const pattern = (side: string): string =>
        `${name}.${variant.name}(${names(side).join(", ")})`;
      out.add(`${body}    (${pattern("hd_l")}, ${pattern("hd_r")}) =>`);
      names("hd_l").forEach((left, index) =>
        addOrderStep(
          out,
          orderStep(total, left, names("hd_r")[index]!, `${body}        `),
          fieldSpan(variant.fields[index]!, trait, owner),
        ),
      );
    }
    if (payloads.length > 0 && variants.length > 1) out.add(`${body}    _ => pass`);
  }
  out.add(`${body}${total ? ".Equal" : ".Some(.Equal)"}`);
  return out.program(implementationSpan(span)).implementations[0]!;
}

function deriveHash(target: Target, span: SourceSpan): ImplDecl {
  const out = new Source_();
  derivedImpl(target, "Hash", out);
  out.add(`    fn hash(self, state: mut Hasher) -> void:`);
  const body = "        ";
  const owner = target.declaration.name;
  const hash = (field: DataField, value: string, indent: string): void =>
    out.add(`${indent}${HASH}(${value}, state)`, fieldSpan(field, "Hash", owner));
  if (target.kind === "data") {
    const fields = target.declaration.fields;
    if (fields.length === 0) out.add(`${body}pass`);
    for (const field of fields) hash(field, `self.${field.name}`, body);
  } else {
    const { name, variants, sharedFields } = target.declaration;
    out.add(`${body}${HASH}(${rankName(target)}(self), state)`);
    for (const field of sharedFields) hash(field, `self.${field.name}`, body);
    const payloads = variants.filter((variant) => variant.fields.length > 0);
    if (payloads.length > 0) out.add(`${body}match self:`);
    for (const variant of payloads) {
      const names = variant.fields.map((_, index) => `hd_v${index}`);
      out.add(`${body}    ${name}.${variant.name}(${names.join(", ")}) =>`);
      names.forEach((value, index) => hash(variant.fields[index]!, value, `${body}        `));
    }
    if (payloads.length > 0 && payloads.length < variants.length) out.add(`${body}    _ => pass`);
  }
  return out.program(implementationSpan(span)).implementations[0]!;
}

/** The ordinary implementation that one intrinsic `@derive` entry generates. */
function deriveIntrinsic(item: IntrinsicDerivation, writer: string): ImplDecl {
  if (item.trait === "Debug") return deriveDebug(item.target, writer, item.span);
  if (item.trait === "PartialOrd") return deriveOrdering(item.target, false, item.span);
  if (item.trait === "Ord") return deriveOrdering(item.target, true, item.span);
  if (item.trait === "Hash") return deriveHash(item.target, item.span);
  return deriveEq(item.target, item.span);
}

interface NewtypeDerivation {
  readonly trait: string;
  readonly declaration: TypeDecl;
  readonly span: SourceSpan;
}

/**
 * An intrinsic derivation on a newtype applies the base type's method to the
 * unwrapped values (spec/09-traits.md#derived-newtypes).
 */
function deriveNewtypeIntrinsic(item: NewtypeDerivation, writer: string): ImplDecl {
  const { name, genericParameters: parameters, base } = item.declaration;
  const out = new Source_();
  const T = out.type(parameters.length > 0 ? `${name}[${parameters.join(",")}]` : name);
  const bounds = parameters.map((parameter) => `${parameter} < ${item.trait}`).join(", ");
  out.add(`impl${parameters.length > 0 ? `[${bounds}]` : ""} ${item.trait} for ${T}:`);
  const unwrap = (value: string): string => `${readonlyType(base!.name).split("[")[0]}(${value})`;
  const [self, other] = [unwrap("self"), unwrap("other")];
  // A base type without the trait is `derive-field-missing-trait` at the
  // base type (spec/09-traits.md#r-trait.derive.newtype.requires.error).
  const at = { ...base!.span };
  DERIVED_FIELD_CHECKS.set(at, { trait: item.trait, owner: name, field: base!.name, base: true });
  if (item.trait === "Eq") out.add(`    fn eq(self, other: ${T}) -> bool: ${self} == ${other}`, at);
  if (item.trait === "PartialOrd")
    out.add(
      `    fn partial_cmp(self, other: ${T}) -> Ordering?: ${PARTIAL_CMP}(${self}, ${other})`,
      at,
    );
  if (item.trait === "Ord")
    out.add(`    fn cmp(self, other: ${T}) -> Ordering: ${CMP}(${self}, ${other})`, at);
  if (item.trait === "Hash")
    out.add(`    fn hash(self, state: mut Hasher) -> void: ${HASH}(${self}, state)`, at);
  if (item.trait === "Debug")
    out.add(`    fn debug(self, out: mut ${writer}) -> void: ${DEBUG}(${self}, out)`, at);
  return out.program(item.span).implementations[0]!;
}

/** The generic helpers and enum ranks that the generated implementations call. */
export function intrinsicHelpers(
  intrinsic: readonly IntrinsicDerivation[],
  newtypes: readonly NewtypeDerivation[],
  writer: string,
): FunctionDecl[] {
  const helped = (item: { readonly trait: string }): boolean =>
    ["PartialOrd", "Ord", "Hash"].includes(item.trait);
  const used = intrinsic.filter(helped);
  const debugged = newtypes.some((item) => item.trait === "Debug");
  if (used.length === 0 && !newtypes.some(helped) && !debugged) return [];
  const out = new Source_();
  if (debugged) {
    out.add(`fn ${DEBUG}[T < Debug](value: T, out: mut ${writer}) -> void:`);
    out.add(`    value.debug(out)`);
  }
  out.add(`fn ${PARTIAL_CMP}[T < PartialOrd](left: T, right: T) -> Ordering?:`);
  out.add(`    left.partial_cmp(right)`);
  out.add(`fn ${CMP}[T < Ord](left: T, right: T) -> Ordering:`);
  out.add(`    left.cmp(right)`);
  out.add(`fn ${HASH}[T < Hash](value: T, state: mut Hasher) -> void:`);
  out.add(`    value.hash(state)`);
  const ranked = new Set<string>();
  for (const { target } of used) {
    if (target.kind !== "enum" || ranked.has(target.declaration.name)) continue;
    ranked.add(target.declaration.name);
    const { name, genericParameters, variants } = target.declaration;
    const generic = genericParameters.length > 0 ? `[${genericParameters.join(", ")}]` : "";
    out.add(`fn ${rankName(target)}${generic}(value: ${name}${generic}) -> i32:`);
    if (variants.length === 0) out.add("    0");
    else out.add("    match value:");
    variants.forEach((variant, position) => {
      const payload =
        variant.fields.length > 0 ? `(${variant.fields.map(() => "_").join(", ")})` : "";
      out.add(`        ${name}.${variant.name}${payload} => ${position}`);
    });
  }
  return [...out.program(ZERO_SPAN).functions];
}

// ---------------------------------------------------------------------------
// Law partners (spec/09-traits.md#law-partners).

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
function checkLawPartners(
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

/** Every intrinsic derivation's implementation, after the law-partner check. */
export function deriveIntrinsics(
  program: Program,
  intrinsic: readonly IntrinsicDerivation[],
  newtypes: readonly NewtypeDerivation[],
  writer: string,
  error: (code: string, message: string, span: SourceSpan) => void,
): ImplDecl[] {
  checkLawPartners(program, error);
  return [
    ...intrinsic.map((item) => deriveIntrinsic(item, writer)),
    ...newtypes.map((item) => deriveNewtypeIntrinsic(item, writer)),
  ];
}
