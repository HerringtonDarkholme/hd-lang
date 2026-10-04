import type { HirFunction, HirProgram, HirStatement, HirTraitImplementation } from "../hir.ts";
import { readonlyType, traitSuspensionParts } from "../types.ts";

/** A stable identity for one resolved trait method in HIR. */
export function traitMethodKey(traitIndex: number, methodIndex: number): string {
  return `${traitIndex}:${methodIndex}`;
}

const TRAIT_METHOD_DISPATCH_KINDS = new Set([
  "trait-call",
  "trait-suspend-construct",
  "trait-suspend-drive",
  "trait-suspend-cancel",
  "bound",
]);

function markTraitMethods(
  node: Record<string, unknown>,
  mark: (traitIndex: number, methodIndex: number) => void,
): void {
  if (
    TRAIT_METHOD_DISPATCH_KINDS.has(String(node.kind)) &&
    typeof node.traitIndex === "number" &&
    typeof node.methodIndex === "number"
  )
    mark(node.traitIndex, node.methodIndex);
  if (
    (node.kind === "inspect-type-id" || node.kind === "inspect-downcast") &&
    typeof node.traitIndex === "number"
  )
    mark(node.traitIndex, 0);
  if (
    (node.kind === "map" || node.kind === "map-comprehension") &&
    node.keyKind === 3 &&
    node.keyDictionary &&
    typeof node.keyDictionary === "object" &&
    (node.keyDictionary as { readonly kind?: unknown }).kind === "trait-dictionary" &&
    typeof (node.keyDictionary as { readonly traitIndex?: unknown }).traitIndex === "number"
  )
    mark((node.keyDictionary as { readonly traitIndex: number }).traitIndex, 0);
  if (node.kind === "suspension-wrap") {
    const type = (node.suspension as { readonly type?: unknown } | undefined)?.type;
    const method = typeof type === "string" ? traitSuspensionParts(type) : undefined;
    if (method) mark(method.traitIndex, method.methodIndex);
  }
}

/**
 * Collect methods that executable HIR can dispatch to. Call expressions carry
 * declaration indices, so this does not depend on source spellings. Pass a
 * reachable program when dead standard code must not keep methods alive.
 */
export function calledTraitMethods(program: HirProgram): ReadonlySet<string> {
  const called = new Set<string>();
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    if (!value || typeof value !== "object") return;
    const node = value as Record<string, unknown>;
    markTraitMethods(node, (traitIndex, methodIndex) =>
      called.add(traitMethodKey(traitIndex, methodIndex)),
    );
    for (const [key, child] of Object.entries(node)) if (key !== "span") visit(child);
  };
  for (const declaration of [...program.functions, ...program.closures]) visit(declaration.body);
  return called;
}

/**
 * Select code by resolved HIR references, after checking every declaration.
 * Program functions remain host-callable roots. Std code is reached through
 * calls, function values, closures, default arguments, or dictionary plans.
 * Keep original indices: references and replay identities are not renumbered.
 */
export interface EmissionReachability {
  readonly program: HirProgram;
  readonly traitMethods: ReadonlySet<string>;
}

export function emissionReachability(program: HirProgram): EmissionReachability {
  const functions = new Map(program.functions.map((item) => [item.index, item]));
  const closures = new Map(program.closures.map((item) => [item.index, item]));
  const implementations = new Map(program.implementations.map((item) => [item.index, item]));
  const globals = new Map(program.globals.map((item) => [item.index, item]));
  const initializer =
    program.initializer === undefined ? undefined : functions.get(program.initializer);
  const standardBindings = new Map<
    number,
    Extract<HirStatement, { readonly kind: "global-binding" }>
  >(
    (initializer?.body ?? []).flatMap((statement) =>
      statement.kind === "global-binding" && statement.global.standard
        ? [[statement.global.index, statement] as const]
        : [],
    ),
  );
  const liveFunctions = new Set<number>();
  const liveClosures = new Set<number>();
  const liveImplementations = new Set<number>();
  const liveGlobals = new Set<number>();
  const liveTraitMethods = new Set<string>();
  const pending: HirFunction[] = [];
  const retainInitializer = (): void => {
    if (initializer) liveFunctions.add(initializer.index);
  };
  const functionByIndex = (index: number): void => {
    if (liveFunctions.has(index)) return;
    const declaration = functions.get(index);
    if (!declaration) return;
    liveFunctions.add(index);
    // Standard constant bindings are selected independently below. Visiting
    // the whole initializer would make every one reachable again.
    if (index === program.initializer) return;
    pending.push(declaration);
  };
  const closureByIndex = (index: number): void => {
    if (liveClosures.has(index)) return;
    const declaration = closures.get(index);
    if (!declaration) return;
    liveClosures.add(index);
    pending.push(declaration);
  };
  const methodFunction = (implementation: HirTraitImplementation, methodIndex: number): void => {
    const mapping = implementation.methodFunctions.find(
      (candidate) => candidate.methodIndex === methodIndex,
    );
    if (mapping && !mapping.strengthened) functionByIndex(mapping.functionIndex);
  };
  const traitMethod = (traitIndex: number, methodIndex: number): void => {
    const key = traitMethodKey(traitIndex, methodIndex);
    if (liveTraitMethods.has(key)) return;
    liveTraitMethods.add(key);
    for (const implementation of implementations.values())
      if (implementation.traitIndex === traitIndex && liveImplementations.has(implementation.index))
        methodFunction(implementation, methodIndex);
  };
  const implementationByIndex = (index: number): void => {
    if (liveImplementations.has(index)) return;
    const declaration = implementations.get(index);
    if (!declaration) return; // Builtin dictionaries have no source impl.
    liveImplementations.add(index);
    for (const mapping of declaration.methodFunctions)
      if (liveTraitMethods.has(traitMethodKey(declaration.traitIndex, mapping.methodIndex)))
        methodFunction(declaration, mapping.methodIndex);
    declaration.supertraitImplementations.forEach(implementationByIndex);
  };
  const kernel = (name: "concat" | "equal" | "compare"): void => {
    const declaration = program.functions.find((item) => item.name === `__std_text_string_${name}`);
    if (declaration) functionByIndex(declaration.index);
  };
  let visit: (value: unknown) => void;
  const globalByIndex = (index: number): void => {
    if (liveGlobals.has(index)) return;
    const global = globals.get(index);
    if (!global) return;
    liveGlobals.add(index);
    const binding = standardBindings.get(index);
    if (binding) {
      retainInitializer();
      visit(binding.value);
    }
  };
  visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    if (!value || typeof value !== "object") return;
    const node = value as Record<string, unknown>;
    markTraitMethods(node, traitMethod);
    // These are declaration identities in the HIR, never source spellings.
    if (typeof node.functionIndex === "number") functionByIndex(node.functionIndex);
    if (typeof node.iteratorFunctionIndex === "number") functionByIndex(node.iteratorFunctionIndex);
    if (typeof node.closureIndex === "number") closureByIndex(node.closureIndex);
    if (typeof node.implementationIndex === "number")
      implementationByIndex(node.implementationIndex);
    if (
      (node.kind === "global" ||
        node.kind === "global-binding" ||
        node.kind === "global-assignment") &&
      node.global &&
      typeof (node.global as { readonly index?: unknown }).index === "number"
    )
      globalByIndex((node.global as { readonly index: number }).index);
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
      if (implementation) {
        implementationByIndex(implementation.index);
        methodFunction(implementation, 0);
      }
    }
    for (const [key, child] of Object.entries(node)) if (key !== "span") visit(child);
  };
  for (const declaration of program.functions)
    if (
      !declaration.standard &&
      (!declaration.synthetic ||
        declaration.entry ||
        declaration.developmentEntry ||
        declaration.testOptions)
    )
      functionByIndex(declaration.index);
  for (const declaration of program.implementations)
    if (!declaration.standard) implementationByIndex(declaration.index);
  // User module initialization is always observable. Compiler-generated std
  // constants are pure and join it only when a live expression reads them.
  const eagerInitialization = (initializer?.body ?? []).filter(
    (statement) => statement.kind !== "global-binding" || !statement.global.standard,
  );
  if (eagerInitialization.length > 0) {
    retainInitializer();
    eagerInitialization.forEach(visit);
  }
  // The shared map runtime always declares its string-key equality adapter.
  kernel("equal");
  for (let index = 0; index < pending.length; index++) visit(pending[index]);
  const initializerBody = (initializer?.body ?? []).filter(
    (statement) =>
      statement.kind !== "global-binding" ||
      !statement.global.standard ||
      liveGlobals.has(statement.global.index),
  );
  const keepInitializer = initializer !== undefined && initializerBody.length > 0;
  return {
    traitMethods: liveTraitMethods,
    program: {
      ...program,
      globals: program.globals.filter((item) => !item.standard || liveGlobals.has(item.index)),
      functions: program.functions
        .filter(
          (item) => liveFunctions.has(item.index) && (item !== initializer || keepInitializer),
        )
        .map((item) => (item === initializer ? { ...item, body: initializerBody } : item)),
      closures: program.closures.filter((item) => liveClosures.has(item.index)),
      implementations: program.implementations.filter((item) =>
        liveImplementations.has(item.index),
      ),
      ...(keepInitializer ? {} : { initializer: undefined }),
    },
  };
}

export function reachableProgram(program: HirProgram): HirProgram {
  return emissionReachability(program).program;
}
