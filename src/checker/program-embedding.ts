import type { HirData, HirDataField, ValueType } from "../hir.ts";
import { nominalGenericParts, readonlyType } from "../types.ts";
import type { ProgramCheckContext } from "./program-context.ts";

const MAX_EMBEDDING_DEPTH = 64;

/** A member of a data type's view: its name and the embedded fields reaching it. */
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
 * Spec 03 Member Resolution, "Conflicts are declaration errors": in each
 * namespace, the shallowest member with a name hides deeper ones, and two
 * members with one name at the smallest depth are `ambiguous-promoted-member`
 * at the outer data declaration, on the later of the two embedded fields that
 * reach them. A conflict reached through a single embedded field is a conflict
 * of that field's type and is reported there. The prototype compiles one
 * module, so every member is visible and only the declaring module's view is
 * checked; the view from other modules of a public type is not (EMB-S).
 */
export function checkEmbeddedMemberConflicts(context: ProgramCheckContext): void {
  for (const declaration of context.dataTypes.values()) {
    if (!declaration.fields.some((field) => field.embedded)) continue;
    // A repeated field name is already `duplicate-embedded-field` or `duplicate-data-field`.
    const names = declaration.fields.map((field) => field.name);
    if (new Set(names).size !== names.length) continue;
    const reported = new Set<HirDataField>();
    for (const namespace of ["field", "method"] as const) {
      for (const [name, members] of conflicts(declaration, namespace, context)) {
        const firstSteps = [...new Set(members.map((member) => member.path[0]!))];
        if (firstSteps.length < 2) continue;
        const later = firstSteps.reduce((left, right) => (right.index > left.index ? right : left));
        if (reported.has(later)) continue;
        reported.add(later);
        const paths = members
          .slice(0, 2)
          .map((member) =>
            [declaration.name, ...member.path.map((step) => step.name), name].join("."),
          );
        context.diagnostics.push({
          code: "ambiguous-promoted-member",
          message: `${namespace} '${name}' is promoted twice at one depth, as ${paths[0]} and as ${paths[1]}; declare '${name}' on '${declaration.name}' or embed differently`,
          span: later.span,
        });
      }
    }
  }
}

/** 08 Data Embedding: at most three embedded fields and at most three levels. */
const MAX_EMBEDDED_FIELDS = 3;
const MAX_PART_DEPTH = 3;

/**
 * 08 Data Embedding, embedding limits: a fourth embedded field is
 * `too-many-embedded-fields`, and a part at depth 4 is `embedding-too-deep`,
 * reported on the embedded field that begins the first such chain.
 */
export function checkEmbeddingLimits(context: ProgramCheckContext): void {
  for (const declaration of context.dataTypes.values()) {
    const embedded = declaration.fields.filter((field) => field.embedded);
    const extra = embedded[MAX_EMBEDDED_FIELDS];
    if (extra)
      context.diagnostics.push({
        code: "too-many-embedded-fields",
        message: `data '${declaration.name}' embeds ${embedded.length} types; at most ${MAX_EMBEDDED_FIELDS} embedded fields are allowed`,
        span: extra.span,
      });
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

/** The names of `declaration` with two or more members at their smallest depth. */
function conflicts(
  declaration: HirData,
  namespace: "field" | "method",
  context: ProgramCheckContext,
): Map<string, ReachedMember[]> {
  const decided = new Set(ownNames(declaration, declaration.name, namespace, context));
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
        for (const name of ownNames(partDeclaration, type, namespace, context)) {
          if (decided.has(name)) continue;
          const members = atDepth.get(name) ?? [];
          members.push({ name, path });
          atDepth.set(name, members);
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
      if (members.length > 1) result.set(name, members);
    }
    frontier = next;
  }
  return result;
}

function ownNames(
  declaration: HirData,
  type: ValueType,
  namespace: "field" | "method",
  context: ProgramCheckContext,
): string[] {
  if (namespace === "field") return declaration.fields.map((field) => field.name);
  return context.inherentMethods
    .filter((method) => !method.associated && method.targetType === type)
    .map((method) => method.name);
}
