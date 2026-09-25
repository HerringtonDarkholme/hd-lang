import type { Expression, Program } from "../ast.ts";

interface AstRecord {
  readonly kind?: unknown;
  readonly [key: string]: unknown;
}

function astRecord(value: unknown): AstRecord | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as AstRecord)
    : undefined;
}

function calledName(value: unknown): string | undefined {
  const node = astRecord(value);
  if (node?.kind !== "call" && node?.kind !== "suspend-call") return undefined;
  const callee = astRecord(node.callee);
  return callee?.kind === "name" && typeof callee.name === "string" ? callee.name : undefined;
}

export function findDriverCall(
  value: unknown,
  driverFunctions: ReadonlySet<string>,
  imports: ReadonlyMap<string, string>,
): Expression | undefined {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findDriverCall(item, driverFunctions, imports);
      if (found) return found;
    }
    return undefined;
  }
  const node = astRecord(value);
  if (!node) return undefined;
  const callee = calledName(node);
  if (callee && (imports.get(callee) === "std.task.block_on" || driverFunctions.has(callee))) {
    return node as Expression;
  }
  for (const [key, child] of Object.entries(node)) {
    if (key === "span" || key === "doc") continue;
    const found = findDriverCall(child, driverFunctions, imports);
    if (found) return found;
  }
  return undefined;
}

export function findSuspensionCall(
  value: unknown,
): Extract<Expression, { kind: "suspend-call" }> | undefined {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findSuspensionCall(item);
      if (found) return found;
    }
    return undefined;
  }
  const node = astRecord(value);
  if (!node) return undefined;
  if (node.kind === "suspend-call") return node as Extract<Expression, { kind: "suspend-call" }>;
  for (const [key, child] of Object.entries(node)) {
    if (key === "span" || key === "doc") continue;
    const found = findSuspensionCall(child);
    if (found) return found;
  }
  return undefined;
}

export function driverStartingFunctionNames(
  program: Program,
  imports: ReadonlyMap<string, string>,
): ReadonlySet<string> {
  const names = new Set<string>();
  let changed = true;
  while (changed) {
    changed = false;
    for (const declaration of program.functions) {
      if (names.has(declaration.name)) continue;
      if (!findDriverCall(declaration.body, names, imports)) continue;
      names.add(declaration.name);
      changed = true;
    }
  }
  return names;
}

export function deferredDriverCalls(
  value: unknown,
  driverFunctions: ReadonlySet<string>,
  imports: ReadonlyMap<string, string>,
): readonly Expression[] {
  const calls: Expression[] = [];
  const visit = (current: unknown): void => {
    if (Array.isArray(current)) {
      current.forEach(visit);
      return;
    }
    const node = astRecord(current);
    if (!node) return;
    if (node.kind === "defer") {
      const call = findDriverCall(node.body, driverFunctions, imports);
      if (call) calls.push(call);
      return;
    }
    for (const [key, child] of Object.entries(node)) {
      if (key !== "span" && key !== "doc") visit(child);
    }
  };
  visit(value);
  return calls;
}
