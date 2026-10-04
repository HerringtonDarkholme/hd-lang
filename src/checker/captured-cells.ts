import type { HirExpression, HirFunction, HirLocal, ValueType } from "../hir.ts";
import { mapCapturedFunction, type CaptureCellMapper } from "./captured-cells-walk.ts";

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

/**
 * Whether `value` reaches a shared local: any `HirLocal` identity the
 * `shared` map knows, wherever it appears (parameters hold bare locals, so
 * every visited object is checked, not only `local` properties). Any
 * `capture` node counts: it resolves through its closure's captures, which
 * live outside this function object. Scalar metadata (`Map`/`Set`) and spans
 * never hold locals and are skipped.
 */
function reachesShared(value: unknown, shared: ReadonlyMap<HirLocal, HirLocal>): boolean {
  if (value === null || typeof value !== "object") return false;
  if (value instanceof Map || value instanceof Set) return false;
  if (shared.has(value as HirLocal)) return true;
  if (Array.isArray(value)) return value.some((item) => reachesShared(item, shared));
  const node = value as Record<string, unknown>;
  if (node.kind === "capture") return true;
  return Object.entries(node).some(
    ([key, child]) => key !== "span" && reachesShared(child, shared),
  );
}

export function shareCapturedLocals(
  functions: readonly HirFunction[],
  closures: readonly HirFunction[],
): { readonly functions: readonly HirFunction[]; readonly closures: readonly HirFunction[] } {
  const shared = new Map<HirLocal, HirLocal>();
  for (const closure of closures)
    for (const capture of closure.captures)
      if (
        capture.source.mutable &&
        !capture.source.parameter &&
        cellInner(capture.source.type) === undefined &&
        !shared.has(capture.source)
      )
        shared.set(capture.source, { ...capture.source, type: cellType(capture.source.type) });
  if (shared.size === 0) return { functions, closures };
  const closuresByIndex = new Map(closures.map((closure) => [closure.index, closure]));
  const sharedField = (closureIndex: number, fieldIndex: number): HirLocal | undefined => {
    const source = closuresByIndex
      .get(closureIndex)
      ?.captures.find((capture) => capture.fieldIndex === fieldIndex)?.source;
    return source && shared.get(source);
  };

  // Whether conversion could change `fn`. `shared` is keyed by `HirLocal`
  // object identity and `local` maps through it (`shared.get(local) ??
  // local`), so a function none of whose locals the map knows is rebuilt
  // identically; returning it unchanged preserves every identity
  // downstream code relies on. An empty `captures` alone is not enough to
  // skip: the function defining a captured `let` keeps `captures: []` while
  // its binding still becomes a cell.
  const needsMapping = (fn: HirFunction): boolean =>
    fn.parameters.some((local) => shared.has(local)) ||
    fn.locals.some((local) => shared.has(local)) ||
    fn.captures.some((capture) => shared.has(capture.source)) ||
    reachesShared(fn.body, shared);

  const mapper: CaptureCellMapper = {
    shouldMap: needsMapping,
    local: (local) => shared.get(local) ?? local,
    expression: (mapped, original, captureOperand) => {
      let cell: HirExpression | undefined;
      if (original.kind === "local") {
        const local = shared.get(original.local);
        if (local) cell = { ...original, local, type: local.type };
      } else if (original.kind === "capture" && cellInner(original.type) === undefined) {
        const local = sharedField(original.closureIndex, original.fieldIndex);
        if (local) cell = { ...original, type: local.type };
      }
      return cell
        ? captureOperand
          ? cell
          : { kind: "cell-get", cell, type: original.type, span: original.span }
        : mapped;
    },
    statement: (mapped, original) => {
      if (original.kind !== "binding" && original.kind !== "assignment") return mapped;
      const local = shared.get(original.local);
      if (!local) return mapped;
      const span = original.span;
      if (mapped.kind === "binding")
        return {
          ...mapped,
          local,
          value: { kind: "cell-new", value: mapped.value, type: local.type, span },
        };
      if (mapped.kind === "assignment")
        return {
          kind: "expression",
          expression: {
            kind: "cell-set",
            cell: { kind: "local", local, type: local.type, span },
            value: mapped.value,
            type: "void",
            span,
          },
          span,
        };
      throw new Error("capture mapper changed a storage statement before cell conversion");
    },
  };

  return {
    functions: functions.map((fn) => mapCapturedFunction(fn, mapper)),
    closures: closures.map((fn) => mapCapturedFunction(fn, mapper)),
  };
}
