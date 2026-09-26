import type { Diagnostic } from "../diagnostics.ts";
import type { HirFunction, HirGlobal, HirStatement, HirTraitImplementation } from "../hir.ts";

interface DirectReferences {
  readonly reads: Map<number, HirGlobal>;
  readonly callees: Set<string>;
}

// Checks spec/10-modules.md#module-initialization. Each function's transitive
// read set is computed once over the module call graph (Tarjan SCCs), so the
// check is linear in the graph size rather than in the number of call paths.
export function checkModuleInitialization(
  moduleFunction: HirFunction | undefined,
  functions: readonly HirFunction[],
  closures: readonly HirFunction[],
  implementations: readonly HirTraitImplementation[] = [],
): readonly Diagnostic[] {
  if (!moduleFunction) return [];
  const declarations = new Map<string, HirFunction>();
  functions.forEach((declaration) =>
    declarations.set(`function:${declaration.index}`, declaration),
  );
  closures.forEach((declaration) => declarations.set(`closure:${declaration.index}`, declaration));
  const dispatchTargets = new Map<string, string[]>();
  for (const implementation of implementations) {
    for (const method of implementation.methodFunctions) {
      const key = `${implementation.traitIndex}:${method.methodIndex}`;
      const targets = dispatchTargets.get(key) ?? [];
      targets.push(`function:${method.functionIndex}`);
      dispatchTargets.set(key, targets);
    }
  }
  const summaries = new ReadSummaries(declarations, dispatchTargets);
  const initialized = new Set<number>();
  const diagnostics: Diagnostic[] = [];

  for (const statement of moduleFunction.body) {
    const direct = collectReferences(statementValue(statement), dispatchTargets);
    const reads = new Map(direct.reads);
    for (const callee of direct.callees) {
      for (const [index, global] of summaries.readsOf(callee)) reads.set(index, global);
    }
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

class ReadSummaries {
  private readonly direct = new Map<string, DirectReferences>();
  private readonly summaries = new Map<string, Map<number, HirGlobal>>();
  private readonly order = new Map<string, number>();
  private readonly lowLink = new Map<string, number>();
  private readonly stack: string[] = [];
  private readonly onStack = new Set<string>();

  private readonly declarations: ReadonlyMap<string, HirFunction>;
  private readonly dispatchTargets: ReadonlyMap<string, readonly string[]>;

  constructor(
    declarations: ReadonlyMap<string, HirFunction>,
    dispatchTargets: ReadonlyMap<string, readonly string[]>,
  ) {
    this.declarations = declarations;
    this.dispatchTargets = dispatchTargets;
  }

  readsOf(key: string): ReadonlyMap<number, HirGlobal> {
    if (!this.declarations.has(key)) return new Map();
    if (!this.summaries.has(key)) this.visit(key);
    return this.summaries.get(key)!;
  }

  private references(key: string): DirectReferences {
    let references = this.direct.get(key);
    if (!references) {
      references = collectReferences(this.declarations.get(key)!.body, this.dispatchTargets);
      this.direct.set(key, references);
    }
    return references;
  }

  // Iterative Tarjan: deep call chains must not overflow the JS stack.
  private visit(root: string): void {
    const frames: { key: string; callees: string[]; next: number }[] = [];
    const enter = (key: string): void => {
      this.order.set(key, this.order.size);
      this.lowLink.set(key, this.order.get(key)!);
      this.stack.push(key);
      this.onStack.add(key);
      const callees = [...this.references(key).callees].filter((callee) =>
        this.declarations.has(callee),
      );
      frames.push({ key, callees, next: 0 });
    };
    enter(root);
    while (frames.length > 0) {
      const frame = frames[frames.length - 1]!;
      if (frame.next < frame.callees.length) {
        const callee = frame.callees[frame.next]!;
        frame.next += 1;
        if (!this.order.has(callee)) enter(callee);
        else if (this.onStack.has(callee))
          this.lowLink.set(
            frame.key,
            Math.min(this.lowLink.get(frame.key)!, this.order.get(callee)!),
          );
        continue;
      }
      frames.pop();
      const parent = frames[frames.length - 1];
      if (parent)
        this.lowLink.set(
          parent.key,
          Math.min(this.lowLink.get(parent.key)!, this.lowLink.get(frame.key)!),
        );
      if (this.lowLink.get(frame.key) !== this.order.get(frame.key)) continue;
      const component: string[] = [];
      let member: string;
      do {
        member = this.stack.pop()!;
        this.onStack.delete(member);
        component.push(member);
      } while (member !== frame.key);
      const reads = new Map<number, HirGlobal>();
      const members = new Set(component);
      for (const key of component) {
        const references = this.references(key);
        for (const [index, global] of references.reads) reads.set(index, global);
        for (const callee of references.callees) {
          if (members.has(callee)) continue;
          const summary = this.summaries.get(callee);
          if (summary) for (const [index, global] of summary) reads.set(index, global);
        }
      }
      for (const key of component) this.summaries.set(key, reads);
    }
  }
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

// Collects the globals a HIR subtree reads directly and the functions it may
// call or reference. Any node naming a function index (calls, function values,
// default arguments, equality and iterator dispatch) is a callee. A trait
// method call through a generic bound or a trait value reaches every
// implementation of that method in the module.
function collectReferences(
  value: unknown,
  dispatchTargets: ReadonlyMap<string, readonly string[]>,
): DirectReferences {
  const references: DirectReferences = { reads: new Map(), callees: new Set() };
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    if (!value || typeof value !== "object") return;
    const node = value as Record<string, unknown>;
    if (node.kind === "global") {
      const global = node.global as HirGlobal;
      references.reads.set(global.index, global);
      return;
    }
    if (typeof node.functionIndex === "number")
      references.callees.add(`function:${node.functionIndex}`);
    if (typeof node.iteratorFunctionIndex === "number")
      references.callees.add(`function:${node.iteratorFunctionIndex}`);
    if (node.kind === "closure" || node.kind === "closure-self")
      references.callees.add(`closure:${node.closureIndex as number}`);
    if (typeof node.traitIndex === "number" && typeof node.methodIndex === "number") {
      for (const target of dispatchTargets.get(`${node.traitIndex}:${node.methodIndex}`) ?? [])
        references.callees.add(target);
    }
    for (const [key, child] of Object.entries(node)) {
      if (key === "span" || key === "global" || key === "local") continue;
      visit(child);
    }
  };
  visit(value);
  return references;
}
