import type { Diagnostic } from "../diagnostics.ts";
import type { HirFunction, HirGlobal, HirStatement } from "../hir.ts";

export function checkModuleInitialization(
  moduleFunction: HirFunction | undefined,
  functions: readonly HirFunction[],
  closures: readonly HirFunction[],
): readonly Diagnostic[] {
  if (!moduleFunction) return [];
  const functionsByIndex = new Map(
    functions.map((declaration) => [declaration.index, declaration]),
  );
  const closuresByIndex = new Map(closures.map((declaration) => [declaration.index, declaration]));
  const initialized = new Set<number>();
  const diagnostics: Diagnostic[] = [];

  for (const statement of moduleFunction.body) {
    const reads = new Map<number, HirGlobal>();
    visitValue(statementValue(statement), reads, functionsByIndex, closuresByIndex, new Set());
    const uninitialized = [...reads.values()].filter((global) => !initialized.has(global.index));
    if (uninitialized.length > 0) {
      diagnostics.push({
        code: "top-level-read-before-initialization",
        message: `top-level statement may read module binding${uninitialized.length === 1 ? "" : "s"} ${uninitialized.map((global) => `'${global.name}'`).join(", ")} before initialization`,
        span: statement.span,
      });
    }
    if (statement.kind === "global-binding") initialized.add(statement.global.index);
  }
  return diagnostics;
}

function statementValue(statement: HirStatement): unknown {
  switch (statement.kind) {
    case "global-binding":
    case "global-assignment":
    case "binding":
    case "assignment":
    case "discard":
      return statement.value;
    case "expression":
      return statement.expression;
    case "return":
    case "break":
      return statement.value;
    case "defer":
      return statement.body;
    case "continue":
    case "pass":
      return undefined;
  }
}

function visitValue(
  value: unknown,
  reads: Map<number, HirGlobal>,
  functions: ReadonlyMap<number, HirFunction>,
  closures: ReadonlyMap<number, HirFunction>,
  activeFunctions: Set<string>,
): void {
  if (Array.isArray(value)) {
    value.forEach((item) => visitValue(item, reads, functions, closures, activeFunctions));
    return;
  }
  if (!value || typeof value !== "object") return;
  const node = value as Record<string, unknown>;
  if (node.kind === "global") {
    const global = node.global as HirGlobal;
    reads.set(global.index, global);
    return;
  }
  if (node.kind === "call" || node.kind === "suspend-construct" || node.kind === "function-value") {
    visitFunction(node.functionIndex as number, false, reads, functions, closures, activeFunctions);
  } else if (node.kind === "closure") {
    visitFunction(node.closureIndex as number, true, reads, functions, closures, activeFunctions);
  }
  for (const [key, child] of Object.entries(node)) {
    if (key === "span" || key === "global" || key === "local") continue;
    visitValue(child, reads, functions, closures, activeFunctions);
  }
}

function visitFunction(
  index: number,
  closure: boolean,
  reads: Map<number, HirGlobal>,
  functions: ReadonlyMap<number, HirFunction>,
  closures: ReadonlyMap<number, HirFunction>,
  activeFunctions: Set<string>,
): void {
  const key = `${closure ? "closure" : "function"}:${index}`;
  if (activeFunctions.has(key)) return;
  const declaration = (closure ? closures : functions).get(index);
  if (!declaration) return;
  activeFunctions.add(key);
  visitValue(declaration.body, reads, functions, closures, activeFunctions);
  activeFunctions.delete(key);
}
