import type { InitGroup, Statement } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import type { HirFunction, HirGlobal, HirStatement, HirTraitImplementation } from "../hir.ts";

interface DirectReferences {
  readonly reads: Map<number, HirGlobal>;
  readonly callees: Set<string>;
}

// Checks spec/lang/10-modules.md#module-initialization. Each function's transitive
// read set is computed once over the module call graph (Tarjan SCCs), so the
// check is linear in the graph size rather than in the number of call paths.
interface InitBoundaries {
  /** The parsed top-level statements, for aligning group boundaries. */
  readonly statements: readonly Statement[];
  /** Initialization-group boundaries over `statements`. */
  readonly groups: readonly InitGroup[];
}

export function checkModuleInitialization(
  moduleFunction: HirFunction | undefined,
  functions: readonly HirFunction[],
  closures: readonly HirFunction[],
  implementations: readonly HirTraitImplementation[] = [],
  init: InitBoundaries,
): readonly Diagnostic[] {
  if (!moduleFunction) return [];
  const context = initializationContext(functions, closures, implementations);
  const original = checkBody(moduleFunction.body, context, new Set());
  if (original.length === 0) return original;
  // The source order fails, so a larger initialization group may still run
  // in dependency order (spec/lang/10-modules.md#module.init.group.step).
  // A group of one module runs in source order instead
  // (spec/lang/10-modules.md#r-module.init.source-order-single), so only the
  // linker's marked multi-module groups may reorder. Without verified group
  // boundaries keep the source order and its diagnostics, which leaves every
  // program that passes today exactly as it was.
  const partition = partitionBody(moduleFunction.body, init);
  if (!partition) return original;
  const adopted: HirStatement[] = [];
  const running = new Set<number>();
  for (const group of partition) {
    const ordered = group.multi
      ? rescueOrder(group.statements, context, running)
      : checkBody(group.statements, context, running).length === 0
        ? [...group.statements]
        : undefined;
    if (!ordered) return original;
    adopted.push(...ordered);
    for (const defined of definedGlobals(ordered)) running.add(defined);
  }
  if (checkBody(adopted, context, new Set()).length > 0) return original;
  (moduleFunction.body as HirStatement[]).splice(0, moduleFunction.body.length, ...adopted);
  return [];
}

// Splits the initializer body into the linker's initialization groups: the
// statements from each group break stay in that group until the next break,
// and statements before the first break form a leading single-module group.
// Undefined unless the HIR body matches the parsed statements one to one
// (same length, same span starts): anything else keeps the source order.
function partitionBody(
  body: readonly HirStatement[],
  init: InitBoundaries,
): { readonly statements: readonly HirStatement[]; readonly multi: boolean }[] | undefined {
  const { statements, groups } = init;
  if (body.length !== statements.length) return undefined;
  if (
    !body.every(
      (statement, index) => statement.span.start.offset === statements[index]!.span.start.offset,
    )
  )
    return undefined;
  const partition: { statements: readonly HirStatement[]; multi: boolean }[] = [];
  let cursor = 0;
  let multi = false;
  for (const group of groups) {
    if (group.start < cursor || group.start > body.length) return undefined;
    if (group.start > cursor)
      partition.push({ statements: body.slice(cursor, group.start), multi });
    cursor = group.start;
    multi = group.multi;
  }
  partition.push({ statements: body.slice(cursor), multi });
  return partition.filter((group) => group.statements.length > 0);
}

function definedGlobals(statements: readonly HirStatement[]): number[] {
  const globals: number[] = [];
  for (const statement of statements)
    if (statement.kind === "global-binding") globals.push(statement.global.index);
  return globals;
}

interface InitializationContext {
  readonly summaries: ReadSummaries;
  readonly dispatchTargets: ReadonlyMap<string, readonly string[]>;
}

function initializationContext(
  functions: readonly HirFunction[],
  closures: readonly HirFunction[],
  implementations: readonly HirTraitImplementation[],
): InitializationContext {
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
  return { summaries: new ReadSummaries(declarations, dispatchTargets), dispatchTargets };
}

function checkBody(
  body: readonly HirStatement[],
  context: InitializationContext,
  initialized: ReadonlySet<number>,
): Diagnostic[] {
  const running = new Set(initialized);
  const diagnostics: Diagnostic[] = [];

  for (const statement of body) {
    const uninitialized = [...readsOf(statement, context).values()].filter(
      (global) => !running.has(global.index),
    );
    if (uninitialized.length > 0) {
      diagnostics.push({
        code: "top-level-read-before-initialization",
        message: `top-level statement may read module binding${uninitialized.length === 1 ? "" : "s"} ${uninitialized.map((global) => `'${global.name}'`).join(", ")} before initialization`,
        span: statement.span,
      });
    }
    if (statement.kind === "global-binding") running.add(statement.global.index);
  }
  return diagnostics;
}

function readsOf(
  statement: HirStatement,
  context: InitializationContext,
): ReadonlyMap<number, HirGlobal> {
  const direct = collectReferences(statementValue(statement), context.dispatchTargets);
  const reads = new Map(direct.reads);
  for (const callee of direct.callees) {
    for (const [index, global] of context.summaries.readsOf(callee)) reads.set(index, global);
  }
  return reads;
}

// The group's dependency order (spec/lang/10-modules.md#module.init.group.step):
// one top-level statement at a time, each step the earliest remaining
// statement whose read bindings are all initialized. Earliest means earliest
// in the joined source, which already orders modules by identity, then by
// source position (spec/lang/10-modules.md#r-module.init.group.earliest).
// Undefined when statements remain but none is ready: an initialization
// cycle (spec/lang/10-modules.md#r-module.init.group.cycle).
function rescueOrder(
  body: readonly HirStatement[],
  context: InitializationContext,
  seed: ReadonlySet<number>,
): HirStatement[] | undefined {
  const reads = body.map((statement) => readsOf(statement, context));
  const initialized = new Set(seed);
  const remaining = new Set(body.map((_, index) => index));
  const order: HirStatement[] = [];
  while (remaining.size > 0) {
    let next: number | undefined;
    for (const index of remaining) {
      if ([...reads[index]!.keys()].every((global) => initialized.has(global))) {
        next = index;
        break;
      }
    }
    if (next === undefined) return undefined;
    remaining.delete(next);
    order.push(body[next]!);
    const defined = body[next]!;
    if (defined.kind === "global-binding") initialized.add(defined.global.index);
  }
  return order;
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
