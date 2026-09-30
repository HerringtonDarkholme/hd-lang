import type { DataField, Expression, MemberLine } from "../ast.ts";
import { declarationFacts, lineFacts, type Target } from "./member-lines.ts";
import { fieldsSelfRef, type SelfRef, type SelfRefScope } from "./self-ref.ts";

// The members and variants one typed derivation sees
// (spec/14-annotations.md#typed-derivation), after its member lines.

export interface MemberModel {
  readonly name: string;
  /** The field or payload name the generated code reads and constructs. */
  readonly access: string;
  readonly declared: string;
  readonly position: number;
  readonly facts: readonly Expression[];
  readonly doc?: string;
  readonly embedded: boolean;
  readonly positional: boolean;
  readonly default?: Expression;
  readonly omitted: boolean;
  readonly selfRef: SelfRef;
}

export interface VariantModel {
  readonly name: string;
  readonly index: number;
  readonly facts: readonly Expression[];
  readonly doc?: string;
  readonly ofData: boolean;
  readonly members: readonly MemberModel[];
  readonly selfRef: SelfRef;
}

// ---------------------------------------------------------------------------
// Member lines (annot.line.*).

/** The facts one derivation sees for a member, variant, or `Self`. */
export function effectiveFacts(
  target: Target,
  lines: readonly MemberLine[],
  name: string,
): { facts: readonly Expression[]; omitted: boolean } {
  let facts = [...declarationFacts(target, name)];
  let omitted = false;
  for (const line of lines) {
    if (line.name !== name) continue;
    if (line.pass) omitted = true;
    else if (line.value)
      facts =
        line.operator === "+=" ? [...facts, ...lineFacts(line.value)] : [...lineFacts(line.value)];
  }
  return { facts, omitted };
}

export function variantModels(
  target: Target,
  lines: readonly MemberLine[],
  scope: SelfRefScope,
): VariantModel[] {
  const enclosing = target.declaration.name;
  const member = (
    field: DataField,
    position: number,
    facts: readonly Expression[],
    omitted: boolean,
    selfRef: SelfRef,
  ): MemberModel => ({
    name: field.positional ? `_${field.name}` : field.name,
    access: field.name,
    declared: field.type.name,
    position,
    facts,
    ...(field.doc ? { doc: field.doc } : {}),
    embedded: field.embedded === true,
    positional: field.positional === true,
    ...(field.default ? { default: field.default } : {}),
    omitted,
    selfRef,
  });
  if (target.kind === "data") {
    const declaration = target.declaration;
    const selfRefs = fieldsSelfRef(declaration.fields, enclosing, scope);
    return [
      {
        name: declaration.name,
        index: 0,
        facts: [],
        ...(declaration.doc ? { doc: declaration.doc } : {}),
        ofData: true,
        members: declaration.fields.map((field, position) => {
          const { facts, omitted } = effectiveFacts(target, lines, field.name);
          return member(field, position, facts, omitted, selfRefs.members[position]!);
        }),
        selfRef: selfRefs.variant,
      },
    ];
  }
  return target.declaration.variants.map((variant, index) => {
    const selfRefs = fieldsSelfRef(variant.fields, enclosing, scope);
    return {
      name: variant.name,
      index,
      facts: effectiveFacts(target, lines, variant.name).facts,
      ...(variant.doc ? { doc: variant.doc } : {}),
      ofData: false,
      members: variant.fields.map((field, position) =>
        member(field, position, field.metadata ?? [], false, selfRefs.members[position]!),
      ),
      selfRef: selfRefs.variant,
    };
  });
}
