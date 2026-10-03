import type { HirFunction, HirProgram, HirTraitImplementation } from "../hir.ts";
import { readonlyType } from "../types.ts";

/**
 * Select code by resolved HIR references, after checking every declaration.
 * Program functions remain host-callable roots. Std code is reached through
 * calls, function values, closures, default arguments, or dictionary plans.
 * Keep original indices: references and replay identities are not renumbered.
 */
export function reachableProgram(program: HirProgram): HirProgram {
  const functions = new Map(program.functions.map((item) => [item.index, item]));
  const closures = new Map(program.closures.map((item) => [item.index, item]));
  const implementations = new Map(program.implementations.map((item) => [item.index, item]));
  const liveFunctions = new Set<number>();
  const liveClosures = new Set<number>();
  const liveImplementations = new Set<number>();
  const pending: Array<HirFunction | HirTraitImplementation> = [];
  const functionByIndex = (index: number): void => {
    if (liveFunctions.has(index)) return;
    const declaration = functions.get(index);
    if (!declaration) return;
    liveFunctions.add(index);
    pending.push(declaration);
  };
  const closureByIndex = (index: number): void => {
    if (liveClosures.has(index)) return;
    const declaration = closures.get(index);
    if (!declaration) return;
    liveClosures.add(index);
    pending.push(declaration);
  };
  const implementationByIndex = (index: number): void => {
    if (liveImplementations.has(index)) return;
    const declaration = implementations.get(index);
    if (!declaration) return; // Builtin dictionaries have no source impl.
    liveImplementations.add(index);
    pending.push(declaration);
    // Materializing a dictionary exposes its complete method table.
    declaration.methodFunctions.forEach((method) => functionByIndex(method.functionIndex));
    declaration.supertraitImplementations.forEach(implementationByIndex);
  };
  const kernel = (name: "concat" | "equal" | "compare"): void => {
    const declaration = program.functions.find((item) => item.name === `__std_text_string_${name}`);
    if (declaration) functionByIndex(declaration.index);
  };
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    if (!value || typeof value !== "object") return;
    const node = value as Record<string, unknown>;
    // These are declaration identities in the HIR, never source spellings.
    if (typeof node.functionIndex === "number") functionByIndex(node.functionIndex);
    if (typeof node.iteratorFunctionIndex === "number") functionByIndex(node.iteratorFunctionIndex);
    if (typeof node.closureIndex === "number") closureByIndex(node.closureIndex);
    if (typeof node.implementationIndex === "number")
      implementationByIndex(node.implementationIndex);
    // String operators and inspection keys lower to backend-generated calls.
    if (node.kind === "string-build" || node.kind === "inspectable") kernel("concat");
    if (node.kind === "inspect-downcast" || node.kind === "value-equality") kernel("equal");
    if (node.kind === "value-ordering") kernel("compare");
    if (node.kind === "binary") {
      const left = node.left as { readonly type?: string } | undefined;
      if (left?.type === "string") kernel(node.operator === "+" ? "concat" : "compare");
    }
    // Wide map keys have an implicit Eq call in the map runtime adapter.
    if (node.keyKind === 2 && typeof node.keyType === "string") {
      const eq = program.traits.find((trait) => trait.name === "Eq");
      const implementation = program.implementations.find(
        (item) =>
          item.traitIndex === eq?.index && item.targetType === readonlyType(node.keyType as string),
      );
      if (implementation) implementationByIndex(implementation.index);
    }
    for (const [key, child] of Object.entries(node)) if (key !== "span") visit(child);
  };
  for (const declaration of program.functions)
    if (!declaration.standard) functionByIndex(declaration.index);
  for (const declaration of program.implementations)
    if (!declaration.standard) implementationByIndex(declaration.index);
  // The shared map runtime always declares its string-key equality adapter.
  kernel("equal");
  if (program.initializer !== undefined) functionByIndex(program.initializer);
  for (let index = 0; index < pending.length; index++) visit(pending[index]);
  return {
    ...program,
    functions: program.functions.filter((item) => liveFunctions.has(item.index)),
    closures: program.closures.filter((item) => liveClosures.has(item.index)),
    implementations: program.implementations.filter((item) => liveImplementations.has(item.index)),
  };
}
