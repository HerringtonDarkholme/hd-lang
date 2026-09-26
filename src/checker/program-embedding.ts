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
