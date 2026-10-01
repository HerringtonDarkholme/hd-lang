import type { DataDecl, DataField, EnumDecl, Program, TypeDecl } from "../ast.ts";
import {
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
} from "../types.ts";

// Self references (spec/14-annotations.md#self-references): whether a
// member's type refers to the enclosing data type or enum, and whether its
// simplest value needs one. The compiler computes them for the `self_ref`
// field of `std.structure`'s `Member` and `VariantInfo`
// (r-annot.structure.self-ref-field), and derived `Arbitrary` reads them.
//
// Owner decisions not yet in the spec text, which agree with it: any use of
// the enclosing declaration counts, whatever its type arguments (SR-args);
// another enum needs the enclosing type when every one of its variants does
// (SR-enum); an omitted member counts like any other (SR-omit).

export type SelfRef = "Absent" | "Optional" | "Required";

const STRENGTH: Readonly<Record<SelfRef, number>> = { Absent: 0, Optional: 1, Required: 2 };

/** The module's declarations that a member type may name. */
export interface SelfRefScope {
  readonly data: ReadonlyMap<string, DataDecl>;
  readonly enums: ReadonlyMap<string, EnumDecl>;
  readonly types: ReadonlyMap<string, TypeDecl>;
}

export function selfRefScope(program: Program): SelfRefScope {
  return {
    data: new Map(program.data.map((item) => [item.name, item] as const)),
    enums: new Map(program.enums.map((item) => [item.name, item] as const)),
    types: new Map((program.types ?? []).map((item) => [item.name, item] as const)),
  };
}

/** Substitutes a declaration's own parameters in one of its member types. */
function substitute(
  type: string,
  parameters: readonly string[],
  arguments_: readonly string[],
): string {
  return parameters.reduce(
    (text, parameter, index) =>
      text.replace(new RegExp(`\\b${parameter}\\b`, "g"), arguments_[index] ?? parameter),
    type,
  );
}

/** The member types of a module-level data type, enum, or newtype, with its arguments applied. */
function memberTypes(
  head: string,
  arguments_: readonly string[],
  scope: SelfRefScope,
): readonly (readonly string[])[] | undefined {
  const apply = (parameters: readonly string[], fields: readonly DataField[]): string[] =>
    fields.map((field) => substitute(field.type.name, parameters, arguments_));
  const data = scope.data.get(head);
  if (data) return [apply(data.genericParameters, data.fields)];
  const enumeration = scope.enums.get(head);
  if (enumeration)
    return enumeration.variants.map((variant) =>
      apply(enumeration.genericParameters, variant.fields),
    );
  const declared = scope.types.get(head);
  const base = declared?.base ?? declared?.alias;
  if (declared && base) return [[substitute(base.name, declared.genericParameters, arguments_)]];
  return undefined;
}

/** The head name and type arguments of a named type. */
function named(type: string): { readonly head: string; readonly arguments: readonly string[] } {
  const parts = nominalGenericParts(type);
  return parts ? { head: parts.name, arguments: parts.arguments } : { head: type, arguments: [] };
}

/** r-annot.self-ref.refers and .refers.members. */
function refers(type: string, name: string, scope: SelfRefScope, visiting: Set<string>): boolean {
  const inner = readonlyType(type);
  const optional = optionalInner(inner);
  if (optional !== undefined) return refers(optional, name, scope, visiting);
  const tuple = tupleParts(inner);
  if (tuple) return tuple.some((element) => refers(element, name, scope, visiting));
  if (inner.startsWith("fn(") || inner.startsWith("fn!(")) return false;
  const { head, arguments: arguments_ } = named(inner);
  if (head === name) return true;
  if (arguments_.some((argument) => refers(argument, name, scope, visiting))) return true;
  if (visiting.has(inner)) return false;
  visiting.add(inner);
  const members = memberTypes(head, arguments_, scope) ?? [];
  return members.some((types) => types.some((member) => refers(member, name, scope, visiting)));
}

/** r-annot.self-ref.needs.self through r-annot.self-ref.needs.only. */
function needs(type: string, name: string, scope: SelfRefScope, visiting: Set<string>): boolean {
  const inner = readonlyType(type);
  if (optionalInner(inner) !== undefined) return false;
  const tuple = tupleParts(inner);
  if (tuple) return tuple.some((element) => needs(element, name, scope, visiting));
  const result = resultParts(inner);
  if (result) return needs(result.ok, name, scope, visiting);
  if (inner.startsWith("fn(") || inner.startsWith("fn!(")) return false;
  const { head, arguments: arguments_ } = named(inner);
  if (head === name) return true;
  if (head === "List" || head === "Map") return false;
  if (visiting.has(inner)) return false;
  visiting.add(inner);
  const members = memberTypes(head, arguments_, scope);
  if (!members) return false;
  const variantNeeds = (types: readonly string[]): boolean =>
    types.some((member) => needs(member, name, scope, new Set(visiting)));
  // A data type or newtype has one variant; another enum needs the enclosing
  // type only when every one of its variants does (SR-enum).
  return members.length > 0 && members.every(variantNeeds);
}

/** A member's `self_ref` (r-annot.self-ref.member). */
function memberSelfRef(type: string, enclosing: string, scope: SelfRefScope): SelfRef {
  if (needs(type, enclosing, scope, new Set())) return "Required";
  if (refers(type, enclosing, scope, new Set())) return "Optional";
  return "Absent";
}

/** A variant's `self_ref`: its members' strongest (r-annot.self-ref.variant, .variant.empty). */
function variantSelfRef(members: readonly SelfRef[]): SelfRef {
  return members.reduce<SelfRef>(
    (strongest, member) => (STRENGTH[member] > STRENGTH[strongest] ? member : strongest),
    "Absent",
  );
}

/** The `self_ref` of each member of `fields`, and of their variant. */
export function fieldsSelfRef(
  fields: readonly DataField[],
  enclosing: string,
  scope: SelfRefScope,
): { readonly members: readonly SelfRef[]; readonly variant: SelfRef } {
  const members = fields.map((field) => memberSelfRef(field.type.name, enclosing, scope));
  return { members, variant: variantSelfRef(members) };
}
