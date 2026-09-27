import type { HirExpression, HirFunction, HirLocal, ValueType } from "../hir.ts";

// Captured `let` storage is shared with its defining scope and with every
// closure that captures it (07-functions.md#captures). After checking, each
// `let` local some closure captures becomes a heap cell of type `cell:T`: its
// binding allocates the cell, reads and assignments go through it, and a
// closure's environment holds the cell itself.

const CELL = "cell:";

export function cellType(type: ValueType): ValueType {
  return `${CELL}${type}`;
}

export function cellInner(type: ValueType): ValueType | undefined {
  return type.startsWith(CELL) ? type.slice(CELL.length) : undefined;
}

export function shareCapturedLocals(
  functions: readonly HirFunction[],
  closures: readonly HirFunction[],
): { readonly functions: readonly HirFunction[]; readonly closures: readonly HirFunction[] } {
  const shared = new Map<HirLocal, HirLocal>();
  for (const closure of closures)
    for (const capture of closure.captures)
      if (capture.source.mutable && !capture.source.parameter && !shared.has(capture.source))
        shared.set(capture.source, { ...capture.source, type: cellType(capture.source.type) });
  if (shared.size === 0) return { functions, closures };
  const sharedField = (closureIndex: number, fieldIndex: number): HirLocal | undefined => {
    const source = closures[closureIndex]?.captures.find(
      (capture) => capture.fieldIndex === fieldIndex,
    )?.source;
    return source && shared.get(source);
  };

  const rewrite = (value: unknown, captureList = false): unknown => {
    if (Array.isArray(value)) {
      const items = value.map((item) => rewrite(item, captureList));
      return items.every((item, index) => item === value[index]) ? value : items;
    }
    if (!value || typeof value !== "object") return value;
    const replaced = shared.get(value as HirLocal);
    if (replaced) return replaced;
    const node = value as Record<string, unknown> & { readonly kind?: string };
    const span = node.span;
    if (node.kind === "binding" || node.kind === "assignment") {
      const local = shared.get(node.local as HirLocal);
      if (local) {
        const assigned = rewrite(node.value) as HirExpression;
        if (node.kind === "binding")
          return {
            ...node,
            local,
            value: { kind: "cell-new", value: assigned, type: local.type, span },
          };
        return {
          kind: "expression",
          expression: {
            kind: "cell-set",
            cell: { kind: "local", local, type: local.type, span },
            value: assigned,
            type: "void",
            span,
          },
          span,
        };
      }
    }
    if (node.kind === "local") {
      const local = shared.get(node.local as HirLocal);
      if (local) {
        const cell = { ...node, local, type: local.type };
        return captureList ? cell : { kind: "cell-get", cell, type: node.type, span };
      }
    }
    if (node.kind === "capture" && !cellInner(node.type as ValueType)) {
      const local = sharedField(node.closureIndex as number, node.fieldIndex as number);
      if (local) {
        const cell = { ...node, type: local.type };
        return captureList ? cell : { kind: "cell-get", cell, type: node.type, span };
      }
    }
    let changed = false;
    const result: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(node)) {
      const next = rewrite(child, node.kind === "closure" && key === "captures");
      if (next !== child) changed = true;
      result[key] = next;
    }
    return changed ? result : value;
  };

  return {
    functions: rewrite(functions) as readonly HirFunction[],
    closures: rewrite(closures) as readonly HirFunction[],
  };
}
