import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirDataField, ValueType } from "../hir.ts";
import { nominalGenericParts, readonlyType } from "../types.ts";
import type { ProgramCheckContext } from "./program-context.ts";

const MAX_EMBEDDING_DEPTH = 64;

/** A promoted member of a data type: its name and the embedded fields reaching it. */
interface ReachedMember {
  readonly name: string;
  readonly path: readonly HirDataField[];
}

interface Part {
  readonly declaration: HirData;
  readonly type: ValueType;
  readonly path: readonly HirDataField[];
  readonly ancestors: readonly HirData[];
}

/**
 * Spec 03 Member Resolution, "Conflicts are declaration errors". Only `pub`
 * fields and inherent methods of parts are promoted, and every module sees the
 * same members, so one check per data type suffices. In each namespace, the
 * shallowest member with a name hides deeper ones; two promoted members with
 * one name at the smallest depth are `ambiguous-promoted-member` on the later
 * of the two embedded fields that reach them. A conflict reached through a
 * single embedded field is a conflict of that field's type and is reported
 * there. Every own member hides promoted ones, private or not
 * (03 names.hide.depth, batch 32 Q4).
 */
export function checkEmbeddedMemberConflicts(context: ProgramCheckContext): void {
  for (const declaration of context.dataTypes.values()) {
    if (!declaration.fields.some((field) => field.embedded)) continue;
    // A repeated field name is already `duplicate-embedded-field` or `duplicate-data-field`.
    const names = declaration.fields.map((field) => field.name);
    if (new Set(names).size !== names.length) continue;
    const reported = new Set<HirDataField>();
    for (const namespace of ["field", "method"] as const) {
      const own = ownMembers(declaration, declaration.name, namespace, context);
      const hiding = new Set(own.map((member) => member.name));
      const promoted = promotedMembers(declaration, hiding, namespace, context);
      for (const [name, members] of promoted) {
        if (members.length < 2) continue;
        const firstSteps = [...new Set(members.map((member) => member.path[0]!))];
        if (firstSteps.length < 2) continue;
        const later = firstSteps.reduce((left, right) => (right.index > left.index ? right : left));
        if (reported.has(later)) continue;
        reported.add(later);
        const paths = members.slice(0, 2).map((member) => memberPath(declaration, member));
        context.diagnostics.push({
          code: "ambiguous-promoted-member",
          message: `${namespace} '${name}' is promoted twice at one depth, as ${paths[0]} and as ${paths[1]}; declare a pub '${name}' on '${declaration.name}' or embed differently`,
          span: later.span,
        });
      }
    }
  }
}

function memberPath(declaration: HirData, member: ReachedMember): string {
  return [declaration.name, ...member.path.map((step) => step.name), member.name].join(".");
}

/** 08 Data Embedding: at most three embedded fields and at most three levels. */
const MAX_EMBEDDED_FIELDS = 3;
const MAX_PART_DEPTH = 3;

/**
 * 08 Data Embedding, embedding limits: a fourth embedded field is
 * `too-many-embedded-fields`, and a part at depth 4 is `embedding-too-deep`,
 * reported on the embedded field that begins the first such chain. A data type
 * that embeds itself is `embedding-cycle` instead, reported once per cycle on
 * the cycle's first declared type, with the cycle path in the message.
 */
export function checkEmbeddingLimits(context: ProgramCheckContext): void {
  const cyclic = new Set<HirData>();
  for (const declaration of context.dataTypes.values()) {
    if (cyclic.has(declaration)) continue;
    const cycle = embeddingCycle(declaration, context);
    if (!cycle) continue;
    for (const member of context.dataTypes.values())
      if (
        pathTo(declaration, member, new Set(), context) &&
        pathTo(member, declaration, new Set(), context)
      )
        cyclic.add(member);
    context.diagnostics.push({
      code: "embedding-cycle",
      message: `data '${declaration.name}' embeds itself through ${cycle.path.map((data) => data.name).join(" > ")}; an embedding cycle never ends`,
      span: cycle.field.span,
    });
  }
  for (const declaration of context.dataTypes.values()) {
    const embedded = declaration.fields.filter((field) => field.embedded);
    const extra = embedded[MAX_EMBEDDED_FIELDS];
    if (extra)
      context.diagnostics.push({
        code: "too-many-embedded-fields",
        message: `data '${declaration.name}' embeds ${embedded.length} types; at most ${MAX_EMBEDDED_FIELDS} embedded fields are allowed`,
        span: extra.span,
      });
    if (cyclic.has(declaration)) continue;
    for (const field of embedded) {
      const chain = tooDeepChain(field, [declaration.name], context);
      if (!chain) continue;
      context.diagnostics.push({
        code: "embedding-too-deep",
        message: `data '${declaration.name}' embeds ${chain.length - 1} levels deep through ${chain.join(" > ").replaceAll("generic:", "")}; at most ${MAX_PART_DEPTH} levels are allowed`,
        span: field.span,
      });
      break;
    }
  }
}

/** The data declaration that an embedded field's type names, if any. */
function embeddedDeclaration(
  field: HirDataField,
  context: ProgramCheckContext,
): HirData | undefined {
  const type = readonlyType(field.type);
  return context.dataTypes.get(nominalGenericParts(type)?.name ?? type);
}

/** A path of data types from `declaration` back to itself, and the field that begins it. */
function embeddingCycle(
  declaration: HirData,
  context: ProgramCheckContext,
): { readonly field: HirDataField; readonly path: readonly HirData[] } | undefined {
  for (const field of declaration.fields) {
    if (!field.embedded) continue;
    const target = embeddedDeclaration(field, context);
    const path = target && pathTo(target, declaration, new Set(), context);
    if (path) return { field, path: [declaration, ...path] };
  }
  return undefined;
}

/** The data types on a path from `from` to `goal` through embedded fields, both included, if any. */
function pathTo(
  from: HirData,
  goal: HirData,
  seen: Set<HirData>,
  context: ProgramCheckContext,
): readonly HirData[] | undefined {
  if (from === goal) return [goal];
  if (seen.has(from)) return undefined;
  seen.add(from);
  for (const field of from.fields) {
    if (!field.embedded) continue;
    const target = embeddedDeclaration(field, context);
    const rest = target && pathTo(target, goal, seen, context);
    if (rest) return [from, ...rest];
  }
  return undefined;
}

/** The type names of a chain from `field` that reaches depth 4, if any. */
function tooDeepChain(
  field: HirDataField,
  chain: readonly string[],
  context: ProgramCheckContext,
): readonly string[] | undefined {
  const type = readonlyType(field.type);
  const next = [...chain, type];
  if (next.length > MAX_PART_DEPTH + 1) return next;
  const declaration = context.dataTypes.get(nominalGenericParts(type)?.name ?? type);
  for (const inner of declaration?.fields ?? []) {
    if (!inner.embedded) continue;
    const found = tooDeepChain(inner, next, context);
    if (found) return found;
  }
  return undefined;
}

/**
 * The promoted members of `declaration` by name: for each name not in
 * `hiding`, the public members of parts at the smallest depth that has one.
 */
function promotedMembers(
  declaration: HirData,
  hiding: ReadonlySet<string>,
  namespace: "field" | "method",
  context: ProgramCheckContext,
): Map<string, ReachedMember[]> {
  const decided = new Set(hiding);
  const result = new Map<string, ReachedMember[]>();
  let frontier: Part[] = [
    { declaration, type: declaration.name, path: [], ancestors: [declaration] },
  ];
  for (let depth = 1; depth <= MAX_EMBEDDING_DEPTH && frontier.length > 0; depth += 1) {
    const next: Part[] = [];
    const atDepth = new Map<string, ReachedMember[]>();
    for (const node of frontier) {
      for (const field of node.declaration.fields) {
        if (!field.embedded) continue;
        const type = readonlyType(field.type);
        const partDeclaration = context.dataTypes.get(nominalGenericParts(type)?.name ?? type);
        if (!partDeclaration) continue;
        // A recursive embedding adds no new part.
        if (node.ancestors.includes(partDeclaration)) continue;
        const path = [...node.path, field];
        for (const member of ownMembers(partDeclaration, type, namespace, context)) {
          // A private member of a part is never promoted, even in its own module.
          if (!member.public || decided.has(member.name)) continue;
          const members = atDepth.get(member.name) ?? [];
          members.push({ name: member.name, path });
          atDepth.set(member.name, members);
        }
        next.push({
          declaration: partDeclaration,
          type,
          path,
          ancestors: [...node.ancestors, partDeclaration],
        });
      }
    }
    for (const [name, members] of atDepth) {
      decided.add(name);
      result.set(name, members);
    }
    frontier = next;
  }
  return result;
}

interface OwnMember {
  readonly name: string;
  readonly public: boolean;
  readonly span: SourceSpan;
}

function ownMembers(
  declaration: HirData,
  type: ValueType,
  namespace: "field" | "method",
  context: ProgramCheckContext,
): OwnMember[] {
  if (namespace === "field")
    return declaration.fields.map((field) => ({
      name: field.name,
      public: field.public === true,
      span: field.span,
    }));
  return context.inherentMethods
    .filter((method) => !method.associated && method.targetType === type)
    .map((method) => ({ name: method.name, public: method.public, span: method.span }));
}
