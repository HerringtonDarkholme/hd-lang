import type {
  HirExpression,
  HirFunction,
  HirLocal,
  HirMatchArm,
  HirProviderContextEntry,
  HirStatement,
  ValueType,
} from "./hir.ts";
import { tupleParts } from "./types.ts";

export type HirSuspensionDrive = Extract<HirExpression, { kind: "suspend-drive" | "trait-suspend-drive" }>;

export type SuspensionOperation =
  | { readonly kind: "assign"; readonly local: HirLocal; readonly value: HirExpression }
  | { readonly kind: "evaluate"; readonly value: HirExpression }
  | { readonly kind: "cleanup"; readonly body: readonly HirStatement[] }
  | { readonly kind: "provider-entry"; readonly entry: HirProviderContextEntry }
  | { readonly kind: "context-create"; readonly local: HirLocal; readonly keys: readonly string[]; readonly entries: readonly HirProviderContextEntry[] }
  | {
      readonly kind: "match-bind";
      readonly subject: HirLocal;
      readonly representation: "enum" | "erased-variant" | "scalar" | "data";
      readonly enumIndex?: number;
      readonly bindings: HirMatchArm["bindings"];
    };

export type SuspensionTerminator =
  | { readonly kind: "jump"; readonly target: number }
  | { readonly kind: "branch"; readonly condition: HirExpression; readonly thenTarget: number; readonly elseTarget: number }
  | {
      readonly kind: "match-test";
      readonly subject: HirLocal;
      readonly representation: "enum" | "erased-variant" | "scalar" | "data";
      readonly enumIndex?: number;
      readonly arm: HirMatchArm;
      readonly thenTarget: number;
      readonly elseTarget: number;
    }
  | {
      readonly kind: "suspend";
      readonly siteIndex: number;
      readonly drive: HirSuspensionDrive;
      readonly resultLocal?: HirLocal;
      readonly next: number;
      readonly cleanups: readonly (readonly HirStatement[])[];
    }
  | {
      readonly kind: "propagate";
      readonly operand: HirExpression;
      readonly payloadType: ValueType;
      readonly successTag: number;
      readonly successLocal?: HirLocal;
      readonly successTarget: number;
      readonly cleanups: readonly (readonly HirStatement[])[];
    }
  | { readonly kind: "complete"; readonly value?: HirExpression }
  | { readonly kind: "unreachable" };

export interface SuspensionBlock {
  readonly id: number;
  readonly operations: readonly SuspensionOperation[];
  readonly terminator: SuspensionTerminator;
}

export interface SuspensionPlan {
  readonly entry: number;
  readonly blocks: readonly SuspensionBlock[];
  readonly temporaries: readonly HirLocal[];
  readonly sites: readonly Extract<SuspensionTerminator, { kind: "suspend" }>[];
}

interface LoopContext {
  readonly breakTarget: number;
  readonly continueTarget: number;
  readonly resultLocal?: HirLocal;
  readonly cleanupDepth: number;
}

interface LoweringContext {
  readonly cleanups: readonly (readonly HirStatement[])[];
  readonly loop?: LoopContext;
}

type ValueContinuation = (value: HirExpression | undefined) => number;

const containsDrive = (value: unknown): boolean => {
  if (Array.isArray(value)) return value.some(containsDrive);
  if (!value || typeof value !== "object") return false;
  const node = value as Record<string, unknown>;
  if (node.kind === "suspend-drive" || node.kind === "trait-suspend-drive") return true;
  return Object.entries(node).some(([key, child]) => key !== "span" && containsDrive(child));
};

const driveCount = (value: unknown): number => {
  if (Array.isArray(value)) return value.reduce((count, item) => count + driveCount(item), 0);
  if (!value || typeof value !== "object") return 0;
  const node = value as Record<string, unknown>;
  const self = node.kind === "suspend-drive" || node.kind === "trait-suspend-drive" ? 1 : 0;
  return self + Object.entries(node).reduce((count, [key, child]) => count + (key === "span" ? 0 : driveCount(child)), 0);
};

export function needsSuspensionCfg(declaration: HirFunction): boolean {
  return declaration.body.some((statement) => {
    const direct = statement.kind === "binding" || statement.kind === "assignment" || statement.kind === "discard"
      ? statement.value
      : statement.kind === "return" || statement.kind === "break"
        ? statement.value
        : statement.kind === "expression"
          ? statement.expression
          : undefined;
    if (!direct || !containsDrive(direct)) return statement.kind === "defer" && containsDrive(statement.body);
    return (direct.kind !== "suspend-drive" && direct.kind !== "trait-suspend-drive") || driveCount(direct) > 1;
  });
}

export function buildSuspensionPlan(declaration: HirFunction): SuspensionPlan {
  const builder = new SuspensionPlanBuilder(declaration);
  return builder.build();
}

class SuspensionPlanBuilder {
  private readonly declaration: HirFunction;
  private readonly blocks: Array<SuspensionBlock | undefined> = [];
  private readonly temporaries: HirLocal[] = [];
  private readonly sites: Extract<SuspensionTerminator, { kind: "suspend" }>[] = [];

  constructor(declaration: HirFunction) {
    this.declaration = declaration;
  }

  build(): SuspensionPlan {
    const context: LoweringContext = { cleanups: [] };
    const entry = this.lowerSuite(
      this.declaration.body,
      this.declaration.result,
      (value) => this.complete(value, context.cleanups),
      context,
    );
    return {
      entry,
      blocks: this.blocks.map((block, index) => block ?? { id: index, operations: [], terminator: { kind: "unreachable" } }),
      temporaries: this.temporaries,
      sites: this.sites,
    };
  }

  private reserveBlock(): number {
    const id = this.blocks.length;
    this.blocks.push(undefined);
    return id;
  }

  private setBlock(id: number, operations: readonly SuspensionOperation[], terminator: SuspensionTerminator): number {
    this.blocks[id] = { id, operations, terminator };
    return id;
  }

  private block(operations: readonly SuspensionOperation[], terminator: SuspensionTerminator): number {
    const id = this.reserveBlock();
    return this.setBlock(id, operations, terminator);
  }

  private temporary(type: ValueType, span: HirExpression["span"], hint = "cfg"): HirLocal {
    const local: HirLocal = {
      name: `$${hint}${this.temporaries.length}`,
      type,
      index: this.declaration.locals.length + this.temporaries.length,
      mutable: true,
      parameter: false,
      span,
    };
    this.temporaries.push(local);
    return local;
  }

  private local(local: HirLocal, span = local.span): HirExpression {
    return { kind: "local", local, type: local.type, span };
  }

  private lowerSuite(
    statements: readonly HirStatement[],
    resultType: ValueType,
    onValue: ValueContinuation,
    context: LoweringContext,
  ): number {
    const scopeBase = context.cleanups.length;
    const lowerAt = (index: number, cleanups: readonly (readonly HirStatement[])[]): number => {
      if (index >= statements.length) return this.exitScope(undefined, cleanups.slice(scopeBase), onValue);
      const statement = statements[index]!;
      if (statement.kind === "defer") return lowerAt(index + 1, [...cleanups, statement.body]);
      const nextContext: LoweringContext = { ...context, cleanups };
      const final = index === statements.length - 1;
      const next = () => lowerAt(index + 1, cleanups);
      switch (statement.kind) {
        case "binding":
        case "assignment":
          return this.lowerExpression(statement.value, (value) => {
            if (!value) throw new Error("a binding value cannot be void");
            return this.block([{ kind: "assign", local: statement.local, value }], { kind: "jump", target: next() });
          }, nextContext);
        case "discard":
          return this.lowerExpression(statement.value, (value) => this.block(value ? [{ kind: "evaluate", value }] : [], { kind: "jump", target: next() }), nextContext);
        case "return":
          return statement.value
            ? this.lowerExpression(statement.value, (value) => this.completeWithCleanups(value, cleanups), nextContext)
            : this.completeWithCleanups(undefined, cleanups);
        case "break": {
          if (!context.loop) throw new Error("checked break has no loop context");
          const finish = (value: HirExpression | undefined): number => {
            const operations: SuspensionOperation[] = [];
            if (context.loop!.resultLocal && value) operations.push({ kind: "assign", local: context.loop!.resultLocal, value });
            for (const cleanup of [...cleanups.slice(context.loop!.cleanupDepth)].reverse()) operations.push({ kind: "cleanup", body: cleanup });
            return this.block(operations, { kind: "jump", target: context.loop!.breakTarget });
          };
          return statement.value ? this.lowerExpression(statement.value, finish, nextContext) : finish(undefined);
        }
        case "continue": {
          if (!context.loop) throw new Error("checked continue has no loop context");
          const operations = [...cleanups.slice(context.loop.cleanupDepth)].reverse().map((body) => ({ kind: "cleanup", body }) as const);
          return this.block(operations, { kind: "jump", target: context.loop.continueTarget });
        }
        case "expression":
          if (final && resultType !== "void") {
            return this.lowerExpression(statement.expression, (value) => this.exitScope(value, cleanups.slice(scopeBase), onValue), nextContext);
          }
          return this.lowerExpression(statement.expression, (value) => this.block(value && value.type !== "void" ? [{ kind: "evaluate", value }] : [], { kind: "jump", target: next() }), nextContext);
        case "pass":
          return this.block([], { kind: "jump", target: next() });
      }
    };
    return lowerAt(0, context.cleanups);
  }

  private exitScope(
    value: HirExpression | undefined,
    localCleanups: readonly (readonly HirStatement[])[],
    continuation: ValueContinuation,
  ): number {
    if (localCleanups.length === 0) return continuation(value);
    const saved = value && value.type !== "void" ? this.temporary(value.type, value.span, "exit") : undefined;
    const next = continuation(saved ? this.local(saved, value!.span) : undefined);
    const operations: SuspensionOperation[] = [];
    if (saved && value) operations.push({ kind: "assign", local: saved, value });
    for (const cleanup of [...localCleanups].reverse()) operations.push({ kind: "cleanup", body: cleanup });
    return this.block(operations, { kind: "jump", target: next });
  }

  private completeWithCleanups(value: HirExpression | undefined, cleanups: readonly (readonly HirStatement[])[]): number {
    const saved = value && value.type !== "void" ? this.temporary(value.type, value.span, "return") : undefined;
    const operations: SuspensionOperation[] = [];
    if (saved && value) operations.push({ kind: "assign", local: saved, value });
    for (const cleanup of [...cleanups].reverse()) operations.push({ kind: "cleanup", body: cleanup });
    return this.block(operations, { kind: "complete", value: saved ? this.local(saved, value!.span) : undefined });
  }

  private complete(value: HirExpression | undefined, cleanups: readonly (readonly HirStatement[])[]): number {
    return this.completeWithCleanups(value, cleanups);
  }

  private lowerExpression(expression: HirExpression, continuation: ValueContinuation, context: LoweringContext): number {
    const lowerValues = (
      values: readonly HirExpression[],
      rebuild: (values: readonly HirExpression[]) => HirExpression,
    ): number => this.lowerValueList(values, (lowered) => continuation(rebuild(lowered)), context);

    switch (expression.kind) {
      case "integer":
      case "float":
      case "string":
      case "character":
      case "boolean":
      case "local":
      case "capture":
      case "function-value":
      case "closure-self":
      case "trait-dictionary":
      case "trait-bound-dictionary":
      case "provider-use":
        return continuation(expression);
      case "string-build":
        return lowerValues(expression.segments, (segments) => ({ ...expression, segments }));
      case "display":
        return this.lowerExpression(expression.operand, (operand) => continuation({ ...expression, operand: operand! }), context);
      case "console-print":
        return lowerValues([expression.provider, expression.value], ([provider, value]) => ({ ...expression, provider: provider!, value: value! }));
      case "assert-equal":
        return lowerValues(expression.arguments, (arguments_) => ({ ...expression, arguments: arguments_ }));
      case "permission-weaken":
        return this.lowerExpression(expression.operand, (operand) => continuation({ ...expression, operand: operand! }), context);
      case "list":
        return lowerValues(expression.elements, (elements) => ({ ...expression, elements }));
      case "tuple":
        return lowerValues(expression.elements, (elements) => ({ ...expression, elements }));
      case "map": {
        const values = expression.entries.flatMap((entry) => [entry.key, entry.value]);
        return this.lowerValueList(values, (lowered) => continuation({
          ...expression,
          entries: expression.entries.map((entry, index) => ({ ...entry, key: lowered[index * 2]!, value: lowered[index * 2 + 1]! })),
        }), context);
      }
      case "variant-wrap":
        return expression.payload
          ? this.lowerExpression(expression.payload, (payload) => continuation({ ...expression, payload }), context)
          : continuation(expression);
      case "propagate":
        return this.lowerExpression(expression.operand, (operand) => {
          if (!operand) throw new Error("propagation operand cannot be void");
          const operandLocal = this.temporary(operand.type, operand.span, "propagate");
          const successLocal = expression.payloadType === "void" ? undefined : this.temporary(expression.payloadType, expression.span, "payload");
          const successTarget = continuation(successLocal ? this.local(successLocal, expression.span) : undefined);
          return this.block(
            [{ kind: "assign", local: operandLocal, value: operand }],
            {
              kind: "propagate",
              operand: this.local(operandLocal, operand.span),
              payloadType: expression.payloadType,
              successTag: expression.successTag,
              successLocal,
              successTarget,
              cleanups: context.cleanups,
            },
          );
        }, context);
      case "unary":
        return this.lowerExpression(expression.operand, (operand) => continuation({ ...expression, operand: operand! }), context);
      case "binary":
        if (expression.operator === "and" || expression.operator === "or") return this.lowerShortCircuit(expression, continuation, context);
        return this.lowerValueList([expression.left, expression.right], ([left, right]) => continuation({ ...expression, left: left!, right: right! }), context);
      case "call":
      case "suspend-construct": {
        const bounds = expression.bounds ?? [];
        const values = [...expression.arguments, ...bounds, ...expression.providers];
        return this.lowerValueList(values, (lowered) => {
          const arguments_ = lowered.slice(0, expression.arguments.length);
          const loweredBounds = lowered.slice(expression.arguments.length, expression.arguments.length + bounds.length);
          const providers = lowered.slice(expression.arguments.length + bounds.length);
          return continuation({ ...expression, arguments: arguments_, bounds: expression.bounds ? loweredBounds : undefined, providers } as HirExpression);
        }, context);
      }
      case "suspend-drive":
      case "trait-suspend-drive":
        return this.lowerExpression(expression.suspension, (suspension) => {
          if (!suspension) throw new Error("suspension construction cannot be void");
          const drive = { ...expression, suspension } as HirSuspensionDrive;
          const resultLocal = expression.type === "void" ? undefined : this.temporary(expression.type, expression.span, "resume");
          const next = continuation(resultLocal ? this.local(resultLocal, expression.span) : undefined);
          const terminator: Extract<SuspensionTerminator, { kind: "suspend" }> = {
            kind: "suspend",
            siteIndex: this.sites.length,
            drive,
            resultLocal,
            next,
            cleanups: context.cleanups,
          };
          this.sites.push(terminator);
          return this.block([], terminator);
        }, context);
      case "suspend-cancel":
      case "trait-suspend-cancel":
        return this.lowerExpression(expression.suspension, (suspension) => continuation({ ...expression, suspension: suspension! } as HirExpression), context);
      case "closure":
        return lowerValues(expression.captures, (captures) => ({ ...expression, captures }));
      case "closure-call":
        return this.lowerValueList([expression.callee, ...expression.arguments, ...expression.providers], (values) => continuation({
          ...expression,
          callee: values[0]!,
          arguments: values.slice(1, 1 + expression.arguments.length),
          providers: values.slice(1 + expression.arguments.length),
        }), context);
      case "trait-wrap":
        return this.lowerExpression(expression.value, (value) => continuation({ ...expression, value: value! }), context);
      case "trait-bound":
        return this.lowerExpression(expression.value, (value) => continuation({ ...expression, value: value! }), context);
      case "trait-call":
      case "trait-suspend-construct":
        return this.lowerValueList([expression.receiver, ...expression.arguments, ...expression.providers], (values) => continuation({
          ...expression,
          receiver: values[0]!,
          arguments: values.slice(1, 1 + expression.arguments.length),
          providers: values.slice(1 + expression.arguments.length),
        } as HirExpression), context);
      case "provider-pack":
        return this.lowerValueList([...expression.bases, ...expression.providers], (values) => continuation({
          ...expression,
          bases: values.slice(0, expression.bases.length),
          providers: values.slice(expression.bases.length),
        }), context);
      case "provider-context": {
        const result = this.temporary(expression.type, expression.span, "context");
        const next = continuation(this.local(result, expression.span));
        return this.lowerProviderEntries(expression.entries, 0, (entries) => this.block(
          [{ kind: "context-create", local: result, keys: expression.keys, entries }],
          { kind: "jump", target: next },
        ), context);
      }
      case "provider-with":
        return this.lowerProviderEntries(
          expression.entries,
          0,
          () => this.lowerSuite(expression.body, expression.type, continuation, context),
          context,
        );
      case "data": {
        const values = [...(expression.spread ? [expression.spread] : []), ...expression.fields];
        return this.lowerValueList(values, (lowered) => continuation({
          ...expression,
          spread: expression.spread ? lowered[0] : undefined,
          fields: lowered.slice(expression.spread ? 1 : 0),
        }), context);
      }
      case "enum": {
        return lowerValues(expression.fields, (fields) => ({ ...expression, fields }));
      }
      case "member":
      case "enum-member":
      case "tuple-index":
      case "string-length":
      case "list-length":
      case "map-length":
        return this.lowerExpression(expression.receiver, (receiver) => continuation({ ...expression, receiver: receiver! } as HirExpression), context);
      case "field-set":
        return this.lowerValueList([expression.receiver, expression.value], ([receiver, value]) => continuation({ ...expression, receiver: receiver!, value: value! }), context);
      case "string-starts-with":
        return this.lowerValueList([expression.receiver, expression.prefix], ([receiver, prefix]) => continuation({ ...expression, receiver: receiver!, prefix: prefix! }), context);
      case "list-index":
        return this.lowerValueList([expression.receiver, expression.index], ([receiver, index]) => continuation({ ...expression, receiver: receiver!, index: index! }), context);
      case "list-set":
        return this.lowerValueList([expression.receiver, expression.index, expression.value], ([receiver, index, value]) => continuation({ ...expression, receiver: receiver!, index: index!, value: value! }), context);
      case "list-append":
        return this.lowerValueList([expression.receiver, expression.value], ([receiver, value]) => continuation({ ...expression, receiver: receiver!, value: value! }), context);
      case "map-index":
      case "map-remove":
        return this.lowerValueList([expression.receiver, expression.key], ([receiver, key]) => continuation({ ...expression, receiver: receiver!, key: key! }), context);
      case "map-set":
        return this.lowerValueList([expression.receiver, expression.key, expression.value], ([receiver, key, value]) => continuation({ ...expression, receiver: receiver!, key: key!, value: value! }), context);
      case "map-entry-key":
      case "map-entry-value":
        return this.lowerValueList([expression.receiver, expression.index], ([receiver, index]) => continuation({ ...expression, receiver: receiver!, index: index! } as HirExpression), context);
      case "panic":
        return this.lowerExpression(expression.message, (message) => continuation({ ...expression, message: message! }), context);
      case "if":
        return this.lowerIf(expression, continuation, context);
      case "for":
        return this.lowerFor(expression, continuation, context);
      case "while":
        return this.lowerWhile(expression, continuation, context);
      case "match":
        return this.lowerMatch(expression, continuation, context);
    }
  }

  private lowerValueList(
    values: readonly HirExpression[],
    continuation: (values: readonly HirExpression[]) => number,
    context: LoweringContext,
    index = 0,
    lowered: readonly HirExpression[] = [],
  ): number {
    if (index >= values.length) return continuation(lowered);
    return this.lowerExpression(values[index]!, (value) => {
      if (!value || value.type === "void") throw new Error("void value in expression operand list");
      const local = this.temporary(value.type, value.span, "operand");
      const next = this.lowerValueList(values, continuation, context, index + 1, [...lowered, this.local(local, value.span)]);
      return this.block([{ kind: "assign", local, value }], { kind: "jump", target: next });
    }, context);
  }

  private lowerShortCircuit(
    expression: Extract<HirExpression, { kind: "binary" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result = this.temporary("bool", expression.span, "logical");
    const after = continuation(this.local(result, expression.span));
    const constant = expression.operator === "or";
    const constantBlock = this.block(
      [{ kind: "assign", local: result, value: { kind: "boolean", value: constant, type: "bool", span: expression.span } }],
      { kind: "jump", target: after },
    );
    const rightEntry = this.lowerExpression(expression.right, (right) => this.block(
      [{ kind: "assign", local: result, value: right! }],
      { kind: "jump", target: after },
    ), context);
    return this.lowerExpression(expression.left, (left) => this.block([], {
      kind: "branch",
      condition: left!,
      thenTarget: expression.operator === "and" ? rightEntry : constantBlock,
      elseTarget: expression.operator === "and" ? constantBlock : rightEntry,
    }), context);
  }

  private lowerIf(
    expression: Extract<HirExpression, { kind: "if" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result = expression.type === "void" ? undefined : this.temporary(expression.type, expression.span, "if");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const finish = (value: HirExpression | undefined): number => this.block(
      result && value ? [{ kind: "assign", local: result, value }] : [],
      { kind: "jump", target: after },
    );
    const thenEntry = this.lowerSuite(expression.thenBody, expression.type, finish, context);
    const elseEntry = this.lowerSuite(expression.elseBody, expression.type, finish, context);
    return this.lowerExpression(expression.condition, (condition) => this.block([], {
      kind: "branch",
      condition: condition!,
      thenTarget: thenEntry,
      elseTarget: elseEntry,
    }), context);
  }

  private lowerWhile(
    expression: Extract<HirExpression, { kind: "while" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result = expression.type === "void" ? undefined : this.temporary(expression.type, expression.span, "while");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const head = this.reserveBlock();
    const finishElse = (value: HirExpression | undefined): number => this.block(
      result && value ? [{ kind: "assign", local: result, value }] : [],
      { kind: "jump", target: after },
    );
    const elseEntry = expression.elseBody.length > 0
      ? this.lowerSuite(expression.elseBody, expression.type, finishElse, context)
      : after;
    const loop: LoopContext = { breakTarget: after, continueTarget: head, resultLocal: result, cleanupDepth: context.cleanups.length };
    const bodyEntry = this.lowerSuite(
      expression.body,
      "void",
      () => this.block([], { kind: "jump", target: head }),
      { ...context, loop },
    );
    const conditionEntry = this.lowerExpression(expression.condition, (condition) => this.block([], {
      kind: "branch",
      condition: condition!,
      thenTarget: bodyEntry,
      elseTarget: elseEntry,
    }), context);
    this.setBlock(head, [], { kind: "jump", target: conditionEntry });
    return head;
  }

  private lowerFor(
    expression: Extract<HirExpression, { kind: "for" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result = expression.type === "void" ? undefined : this.temporary(expression.type, expression.span, "for");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const iterable = this.temporary(expression.iterable.type, expression.iterable.span, "iterable");
    const index = this.temporary("i32", expression.span, "index");
    const iterableValue = this.local(iterable, expression.iterable.span);
    const indexValue = this.local(index, expression.span);
    const length: HirExpression = expression.iteratorKind === "list"
      ? { kind: "list-length", receiver: iterableValue, type: "i32", span: expression.span }
      : { kind: "map-length", receiver: iterableValue, type: "i32", span: expression.span };
    const condition: HirExpression = { kind: "binary", operator: "<", left: indexValue, right: length, type: "bool", span: expression.span };
    const yielded: HirExpression = expression.iteratorKind === "list"
      ? { kind: "list-index", receiver: iterableValue, index: indexValue, elementType: expression.yieldType, type: expression.yieldType, span: expression.span }
      : (() => {
          const parts = tupleParts(expression.yieldType)!;
          const key: HirExpression = { kind: "map-entry-key", receiver: iterableValue, index: indexValue, keyType: parts[0]!, type: parts[0]!, span: expression.span };
          const value: HirExpression = { kind: "map-entry-value", receiver: iterableValue, index: indexValue, valueType: parts[1]!, type: parts[1]!, span: expression.span };
          return { kind: "tuple", elements: [key, value], elementTypes: parts, type: expression.yieldType, span: expression.span };
        })();
    const finishElse = (value: HirExpression | undefined): number => this.block(
      result && value ? [{ kind: "assign", local: result, value }] : [],
      { kind: "jump", target: after },
    );
    const elseEntry = expression.elseBody.length > 0
      ? this.lowerSuite(expression.elseBody, expression.type, finishElse, context)
      : after;
    const head = this.reserveBlock();
    const increment = this.block([{ kind: "assign", local: index, value: {
      kind: "binary",
      operator: "+",
      left: indexValue,
      right: { kind: "integer", value: 1, type: "i32", span: expression.span },
      type: "i32",
      span: expression.span,
    } }], { kind: "jump", target: head });
    const loop: LoopContext = { breakTarget: after, continueTarget: increment, resultLocal: result, cleanupDepth: context.cleanups.length };
    const bodyEntry = this.lowerSuite(expression.body, "void", () => increment, { ...context, loop });
    const bindingOperations: SuspensionOperation[] = [];
    if (expression.bindings.length === 1) {
      bindingOperations.push({ kind: "assign", local: expression.bindings[0]!, value: yielded });
    } else {
      const tuple = this.temporary(expression.yieldType, expression.span, "yield");
      bindingOperations.push({ kind: "assign", local: tuple, value: yielded });
      expression.bindings.forEach((binding, bindingIndex) => bindingOperations.push({
        kind: "assign",
        local: binding,
        value: {
          kind: "tuple-index",
          receiver: this.local(tuple, expression.span),
          index: bindingIndex,
          elementType: binding.type,
          type: binding.type,
          span: binding.span,
        },
      }));
    }
    const bindingEntry = this.block(bindingOperations, { kind: "jump", target: bodyEntry });
    const conditionEntry = this.block([], { kind: "branch", condition, thenTarget: bindingEntry, elseTarget: elseEntry });
    this.setBlock(head, [], { kind: "jump", target: conditionEntry });
    return this.lowerExpression(expression.iterable, (value) => this.block([
      { kind: "assign", local: iterable, value: value! },
      { kind: "assign", local: index, value: { kind: "integer", value: 0, type: "i32", span: expression.span } },
    ], { kind: "jump", target: head }), context);
  }

  private lowerMatch(
    expression: Extract<HirExpression, { kind: "match" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result = expression.type === "void" ? undefined : this.temporary(expression.type, expression.span, "match");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const finish = (value: HirExpression | undefined): number => this.block(
      result && value ? [{ kind: "assign", local: result, value }] : [],
      { kind: "jump", target: after },
    );
    return this.lowerExpression(expression.subject, (subjectValue) => {
      if (!subjectValue) throw new Error("match subject cannot be void");
      const subject = this.temporary(subjectValue.type, subjectValue.span, "subject");
      let nextArm = this.block([], { kind: "unreachable" });
      for (let index = expression.arms.length - 1; index >= 0; index -= 1) {
        const arm = expression.arms[index]!;
        const body = this.lowerSuite(arm.body, expression.type, finish, context);
        const boundBody = arm.guard
          ? this.lowerExpression(arm.guard, (guard) => this.block([], { kind: "branch", condition: guard!, thenTarget: body, elseTarget: nextArm }), context)
          : body;
        const bindings = this.block([
          { kind: "match-bind", subject, representation: expression.representation, enumIndex: expression.enumIndex, bindings: arm.bindings },
        ], { kind: "jump", target: boundBody });
        const hasCondition = Boolean(arm.literal || arm.tag !== undefined || (arm.tests && arm.tests.length > 0));
        nextArm = hasCondition
          ? this.block([], {
              kind: "match-test",
              subject,
              representation: expression.representation,
              enumIndex: expression.enumIndex,
              arm,
              thenTarget: bindings,
              elseTarget: nextArm,
            })
          : bindings;
      }
      return this.block([{ kind: "assign", local: subject, value: subjectValue }], { kind: "jump", target: nextArm });
    }, context);
  }

  private lowerProviderEntries(
    entries: readonly HirProviderContextEntry[],
    index: number,
    continuation: (entries: readonly HirProviderContextEntry[]) => number,
    context: LoweringContext,
    lowered: readonly HirProviderContextEntry[] = [],
  ): number {
    if (index >= entries.length) return continuation(lowered);
    const entry = entries[index]!;
    return this.lowerExpression(entry.value, (value) => {
      if (!value) throw new Error("provider entry cannot be void");
      const loweredEntry = { ...entry, value } as HirProviderContextEntry;
      const next = this.lowerProviderEntries(entries, index + 1, continuation, context, [...lowered, loweredEntry]);
      return this.block([{ kind: "provider-entry", entry: loweredEntry }], { kind: "jump", target: next });
    }, context);
  }
}
