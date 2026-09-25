import type {
  HirExpression,
  HirFunction,
  HirGlobal,
  HirLocal,
  HirMatchArm,
  HirProviderContextEntry,
  HirStatement,
  ValueType,
} from "../hir.ts";
import { mutableType, nominalGenericType } from "../types.ts";

export type HirSuspensionDrive = Extract<
  HirExpression,
  { kind: "suspend-drive" | "trait-suspend-drive" | "suspension-drive" }
>;

export type SuspensionOperation =
  | { readonly kind: "assign"; readonly local: HirLocal; readonly value: HirExpression }
  | { readonly kind: "global-assign"; readonly global: HirGlobal; readonly value: HirExpression }
  | { readonly kind: "evaluate"; readonly value: HirExpression }
  | { readonly kind: "cleanup"; readonly body: readonly HirStatement[] }
  | { readonly kind: "provider-entry"; readonly entry: HirProviderContextEntry }
  | {
      readonly kind: "context-create";
      readonly local: HirLocal;
      readonly keys: readonly string[];
      readonly entries: readonly HirProviderContextEntry[];
    }
  | {
      readonly kind: "match-bind";
      readonly subject: HirLocal;
      readonly representation: "enum" | "erased-variant" | "scalar" | "data";
      readonly enumIndex?: number;
      readonly bindings: HirMatchArm["bindings"];
    };

export type SuspensionTerminator =
  | { readonly kind: "jump"; readonly target: number }
  | {
      readonly kind: "branch";
      readonly condition: HirExpression;
      readonly thenTarget: number;
      readonly elseTarget: number;
    }
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
type HirSuspensionOperandExpression = Extract<
  HirExpression,
  { kind: "suspend-cancel" | "trait-suspend-cancel" | "suspension-cancel" | "suspension-wrap" }
>;

const containsDrive = (value: unknown): boolean => {
  if (Array.isArray(value)) return value.some(containsDrive);
  if (!value || typeof value !== "object") return false;
  const node = value as Record<string, unknown>;
  if (
    node.kind === "suspend-drive" ||
    node.kind === "trait-suspend-drive" ||
    node.kind === "suspension-drive"
  )
    return true;
  return Object.entries(node).some(([key, child]) => key !== "span" && containsDrive(child));
};

const driveCount = (value: unknown): number => {
  if (Array.isArray(value)) return value.reduce((count, item) => count + driveCount(item), 0);
  if (!value || typeof value !== "object") return 0;
  const node = value as Record<string, unknown>;
  const self =
    node.kind === "suspend-drive" ||
    node.kind === "trait-suspend-drive" ||
    node.kind === "suspension-drive"
      ? 1
      : 0;
  return (
    self +
    Object.entries(node).reduce(
      (count, [key, child]) => count + (key === "span" ? 0 : driveCount(child)),
      0,
    )
  );
};

export function needsSuspensionCfg(declaration: HirFunction): boolean {
  return declaration.body.some((statement) => {
    const direct =
      statement.kind === "binding" ||
      statement.kind === "assignment" ||
      statement.kind === "global-binding" ||
      statement.kind === "global-assignment" ||
      statement.kind === "discard"
        ? statement.value
        : statement.kind === "return" || statement.kind === "break"
          ? statement.value
          : statement.kind === "expression"
            ? statement.expression
            : undefined;
    if (!direct || !containsDrive(direct))
      return statement.kind === "defer" && containsDrive(statement.body);
    return (
      (direct.kind !== "suspend-drive" &&
        direct.kind !== "trait-suspend-drive" &&
        direct.kind !== "suspension-drive") ||
      driveCount(direct) > 1
    );
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
      blocks: this.blocks.map(
        (block, index) =>
          block ?? { id: index, operations: [], terminator: { kind: "unreachable" } },
      ),
      temporaries: this.temporaries,
      sites: this.sites,
    };
  }

  private reserveBlock(): number {
    const id = this.blocks.length;
    this.blocks.push(undefined);
    return id;
  }

  private setBlock(
    id: number,
    operations: readonly SuspensionOperation[],
    terminator: SuspensionTerminator,
  ): number {
    this.blocks[id] = { id, operations, terminator };
    return id;
  }

  private block(
    operations: readonly SuspensionOperation[],
    terminator: SuspensionTerminator,
  ): number {
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
      if (index >= statements.length)
        return this.exitScope(undefined, cleanups.slice(scopeBase), onValue);
      const statement = statements[index]!;
      if (statement.kind === "defer") return lowerAt(index + 1, [...cleanups, statement.body]);
      const nextContext: LoweringContext = { ...context, cleanups };
      const final = index === statements.length - 1;
      const next = () => lowerAt(index + 1, cleanups);
      switch (statement.kind) {
        case "binding":
        case "assignment":
          return this.lowerExpression(
            statement.value,
            (value) => {
              if (!value) throw new Error("a binding value cannot be void");
              return this.block([{ kind: "assign", local: statement.local, value }], {
                kind: "jump",
                target: next(),
              });
            },
            nextContext,
          );
        case "global-binding":
        case "global-assignment":
          return this.lowerExpression(
            statement.value,
            (value) => {
              if (!value) throw new Error("a global binding value cannot be void");
              return this.block([{ kind: "global-assign", global: statement.global, value }], {
                kind: "jump",
                target: next(),
              });
            },
            nextContext,
          );
        case "discard":
          return this.lowerExpression(
            statement.value,
            (value) =>
              this.block(value ? [{ kind: "evaluate", value }] : [], {
                kind: "jump",
                target: next(),
              }),
            nextContext,
          );
        case "return":
          return statement.value
            ? this.lowerExpression(
                statement.value,
                (value) => this.completeWithCleanups(value, cleanups),
                nextContext,
              )
            : this.completeWithCleanups(undefined, cleanups);
        case "break": {
          if (!context.loop) throw new Error("checked break has no loop context");
          const finish = (value: HirExpression | undefined): number => {
            const operations: SuspensionOperation[] = [];
            if (context.loop!.resultLocal && value)
              operations.push({ kind: "assign", local: context.loop!.resultLocal, value });
            for (const cleanup of cleanups.slice(context.loop!.cleanupDepth).reverse())
              operations.push({ kind: "cleanup", body: cleanup });
            return this.block(operations, { kind: "jump", target: context.loop!.breakTarget });
          };
          return statement.value
            ? this.lowerExpression(statement.value, finish, nextContext)
            : finish(undefined);
        }
        case "continue": {
          if (!context.loop) throw new Error("checked continue has no loop context");
          const operations = cleanups
            .slice(context.loop.cleanupDepth)
            .reverse()
            .map((body) => ({ kind: "cleanup", body }) as const);
          return this.block(operations, { kind: "jump", target: context.loop.continueTarget });
        }
        case "expression":
          if (final && resultType !== "void") {
            return this.lowerExpression(
              statement.expression,
              (value) => this.exitScope(value, cleanups.slice(scopeBase), onValue),
              nextContext,
            );
          }
          return this.lowerExpression(
            statement.expression,
            (value) =>
              this.block(value && value.type !== "void" ? [{ kind: "evaluate", value }] : [], {
                kind: "jump",
                target: next(),
              }),
            nextContext,
          );
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
    const saved =
      value && value.type !== "void" ? this.temporary(value.type, value.span, "exit") : undefined;
    const next = continuation(saved ? this.local(saved, value!.span) : undefined);
    const operations: SuspensionOperation[] = [];
    if (saved && value) operations.push({ kind: "assign", local: saved, value });
    for (const cleanup of [...localCleanups].reverse())
      operations.push({ kind: "cleanup", body: cleanup });
    return this.block(operations, { kind: "jump", target: next });
  }

  private completeWithCleanups(
    value: HirExpression | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): number {
    const saved =
      value && value.type !== "void" ? this.temporary(value.type, value.span, "return") : undefined;
    const operations: SuspensionOperation[] = [];
    if (saved && value) operations.push({ kind: "assign", local: saved, value });
    for (const cleanup of [...cleanups].reverse())
      operations.push({ kind: "cleanup", body: cleanup });
    return this.block(operations, {
      kind: "complete",
      value: saved ? this.local(saved, value!.span) : undefined,
    });
  }

  private complete(
    value: HirExpression | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): number {
    return this.completeWithCleanups(value, cleanups);
  }

  private lowerAggregateExpression(
    expression: HirExpression,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number | undefined {
    const lowerValues = (
      values: readonly HirExpression[],
      rebuild: (values: readonly HirExpression[]) => HirExpression,
    ): number => this.lowerValueList(values, (lowered) => continuation(rebuild(lowered)), context);
    switch (expression.kind) {
      case "string-build":
        return lowerValues(expression.segments, (segments) => ({ ...expression, segments }));
      case "display":
      case "permission-weaken":
        return this.lowerExpression(
          expression.operand,
          (operand) => continuation({ ...expression, operand: operand! }),
          context,
        );
      case "console-print":
        return lowerValues([expression.provider, expression.value], ([provider, value]) => ({
          ...expression,
          provider: provider!,
          value: value!,
        }));
      case "value-equality":
      case "value-ordering":
        return lowerValues([expression.left, expression.right], ([left, right]) => ({
          ...expression,
          left: left!,
          right: right!,
        }));
      case "assert-equal":
      case "assert":
        return lowerValues(expression.arguments, (arguments_) => ({
          ...expression,
          arguments: arguments_,
        }));
      case "list":
      case "tuple":
        return lowerValues(expression.elements, (elements) => ({ ...expression, elements }));
      case "map": {
        const values = expression.entries.flatMap((entry) => [entry.key, entry.value]);
        return this.lowerValueList(
          values,
          (lowered) =>
            continuation({
              ...expression,
              entries: expression.entries.map((entry, index) => ({
                ...entry,
                key: lowered[index * 2]!,
                value: lowered[index * 2 + 1]!,
              })),
            }),
          context,
        );
      }
      case "variant-wrap":
        return expression.payload
          ? this.lowerExpression(
              expression.payload,
              (payload) => continuation({ ...expression, payload }),
              context,
            )
          : continuation(expression);
      default:
        return undefined;
    }
  }

  private lowerExpression(
    expression: HirExpression,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const aggregate = this.lowerAggregateExpression(expression, continuation, context);
    if (aggregate !== undefined) return aggregate;
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
      case "global":
      case "capture":
      case "function-value":
      case "closure-self":
      case "trait-dictionary":
      case "trait-bound-dictionary":
      case "provider-use":
        return continuation(expression);
      case "propagate":
        return this.lowerPropagation(expression, continuation, context);
      case "unary":
        return this.lowerExpression(
          expression.operand,
          (operand) => continuation({ ...expression, operand: operand! }),
          context,
        );
      case "binding-expression":
        return this.lowerBindingExpression(expression, continuation, context);
      case "binary":
        if (expression.operator === "and" || expression.operator === "or")
          return this.lowerShortCircuit(expression, continuation, context);
        return this.lowerValueList(
          [expression.left, expression.right],
          ([left, right]) => continuation({ ...expression, left: left!, right: right! }),
          context,
        );
      case "call":
      case "suspend-construct": {
        const bounds = expression.bounds ?? [];
        const values = [...expression.arguments, ...bounds, ...expression.providers];
        return this.lowerValueList(
          values,
          (lowered) => {
            const arguments_ = lowered.slice(0, expression.arguments.length);
            const loweredBounds = lowered.slice(
              expression.arguments.length,
              expression.arguments.length + bounds.length,
            );
            const providers = lowered.slice(expression.arguments.length + bounds.length);
            return continuation({
              ...expression,
              arguments: arguments_,
              bounds: expression.bounds ? loweredBounds : undefined,
              providers,
            } as HirExpression);
          },
          context,
        );
      }
      case "suspend-drive":
      case "trait-suspend-drive":
      case "suspension-drive":
        return this.lowerExpression(
          expression.suspension,
          (suspension) => {
            if (!suspension) throw new Error("suspension construction cannot be void");
            const drive = { ...expression, suspension } as HirSuspensionDrive;
            const resultLocal =
              expression.type === "void"
                ? undefined
                : this.temporary(expression.type, expression.span, "resume");
            const next = continuation(
              resultLocal ? this.local(resultLocal, expression.span) : undefined,
            );
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
          },
          context,
        );
      case "suspend-cancel":
      case "trait-suspend-cancel":
      case "suspension-cancel":
      case "suspension-wrap":
        return this.lowerSuspensionOperand(expression, continuation, context);
      case "closure":
        return lowerValues(expression.captures, (captures) => ({ ...expression, captures }));
      case "closure-call":
        return this.lowerValueList(
          [expression.callee, ...expression.arguments, ...expression.providers],
          (values) =>
            continuation({
              ...expression,
              callee: values[0]!,
              arguments: values.slice(1, 1 + expression.arguments.length),
              providers: values.slice(1 + expression.arguments.length),
            }),
          context,
        );
      case "trait-wrap":
        return this.lowerExpression(
          expression.value,
          (value) => continuation({ ...expression, value: value! }),
          context,
        );
      case "trait-bound":
        return this.lowerExpression(
          expression.value,
          (value) => continuation({ ...expression, value: value! }),
          context,
        );
      case "trait-call":
      case "trait-suspend-construct":
        return this.lowerValueList(
          [expression.receiver, ...expression.arguments, ...expression.providers],
          (values) =>
            continuation({
              ...expression,
              receiver: values[0]!,
              arguments: values.slice(1, 1 + expression.arguments.length),
              providers: values.slice(1 + expression.arguments.length),
            } as HirExpression),
          context,
        );
      case "provider-pack":
        return this.lowerValueList(
          [...expression.bases, ...expression.providers],
          (values) =>
            continuation({
              ...expression,
              bases: values.slice(0, expression.bases.length),
              providers: values.slice(expression.bases.length),
            }),
          context,
        );
      case "provider-context": {
        const result = this.temporary(expression.type, expression.span, "context");
        const next = continuation(this.local(result, expression.span));
        return this.lowerProviderEntries(
          expression.entries,
          0,
          (entries) =>
            this.block(
              [{ kind: "context-create", local: result, keys: expression.keys, entries }],
              { kind: "jump", target: next },
            ),
          context,
        );
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
        return this.lowerValueList(
          values,
          (lowered) =>
            continuation({
              ...expression,
              spread: expression.spread ? lowered[0] : undefined,
              fields: lowered.slice(expression.spread ? 1 : 0),
            }),
          context,
        );
      }
      case "enum": {
        return lowerValues(expression.fields, (fields) => ({ ...expression, fields }));
      }
      case "member":
      case "enum-member":
      case "tuple-index":
      case "variant-tag":
      case "variant-payload":
      case "string-length":
      case "string-transform":
      case "list-length":
      case "list-iterator":
      case "iterator-next":
      case "map-iterator":
      case "map-length":
        return this.lowerExpression(
          expression.receiver,
          (receiver) => continuation({ ...expression, receiver: receiver! } as HirExpression),
          context,
        );
      case "string-split":
        return this.lowerValueList(
          [expression.receiver, expression.separator],
          ([receiver, separator]) =>
            continuation({ ...expression, receiver: receiver!, separator: separator! }),
          context,
        );
      case "field-set":
        return this.lowerValueList(
          [expression.receiver, expression.value],
          ([receiver, value]) =>
            continuation({ ...expression, receiver: receiver!, value: value! }),
          context,
        );
      case "string-starts-with":
        return this.lowerValueList(
          [expression.receiver, expression.prefix],
          ([receiver, prefix]) =>
            continuation({ ...expression, receiver: receiver!, prefix: prefix! }),
          context,
        );
      case "list-index":
        return this.lowerValueList(
          [expression.receiver, expression.index],
          ([receiver, index]) =>
            continuation({ ...expression, receiver: receiver!, index: index! }),
          context,
        );
      case "list-set":
        return this.lowerValueList(
          [expression.receiver, expression.index, expression.value],
          ([receiver, index, value]) =>
            continuation({ ...expression, receiver: receiver!, index: index!, value: value! }),
          context,
        );
      case "list-append":
        return this.lowerValueList(
          [expression.receiver, expression.value],
          ([receiver, value]) =>
            continuation({ ...expression, receiver: receiver!, value: value! }),
          context,
        );
      case "map-index":
      case "map-remove":
        return this.lowerValueList(
          [expression.receiver, expression.key],
          ([receiver, key]) => continuation({ ...expression, receiver: receiver!, key: key! }),
          context,
        );
      case "map-set":
        return this.lowerValueList(
          [expression.receiver, expression.key, expression.value],
          ([receiver, key, value]) =>
            continuation({ ...expression, receiver: receiver!, key: key!, value: value! }),
          context,
        );
      case "map-entry-key":
      case "map-entry-value":
        return this.lowerValueList(
          [expression.receiver, expression.index],
          ([receiver, index]) =>
            continuation({ ...expression, receiver: receiver!, index: index! } as HirExpression),
          context,
        );
      case "panic":
        return this.lowerExpression(
          expression.message,
          (message) => continuation({ ...expression, message: message! }),
          context,
        );
      case "if":
        return this.lowerIf(expression, continuation, context);
      case "list-comprehension":
      case "map-comprehension":
        return continuation(expression);
      case "for":
        return this.lowerFor(expression, continuation, context);
      case "while":
        return this.lowerWhile(expression, continuation, context);
      case "match":
        return this.lowerMatch(expression, continuation, context);
    }
    throw new Error(`unhandled aggregate expression '${expression.kind}'`);
  }

  private lowerPropagation(
    expression: Extract<HirExpression, { kind: "propagate" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    return this.lowerExpression(
      expression.operand,
      (operand) => {
        if (!operand) throw new Error("propagation operand cannot be void");
        const operandLocal = this.temporary(operand.type, operand.span, "propagate");
        const successLocal =
          expression.payloadType === "void"
            ? undefined
            : this.temporary(expression.payloadType, expression.span, "payload");
        const successTarget = continuation(
          successLocal ? this.local(successLocal, expression.span) : undefined,
        );
        return this.block([{ kind: "assign", local: operandLocal, value: operand }], {
          kind: "propagate",
          operand: this.local(operandLocal, operand.span),
          payloadType: expression.payloadType,
          successTag: expression.successTag,
          successLocal,
          successTarget,
          cleanups: context.cleanups,
        });
      },
      context,
    );
  }

  private lowerBindingExpression(
    expression: Extract<HirExpression, { kind: "binding-expression" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    return this.lowerExpression(
      expression.value,
      (value) => {
        if (!value) throw new Error("binding initializer cannot be void");
        const result = this.temporary(value.type, value.span, "binding-value");
        const resultValue = this.local(result, expression.span);
        const operations: SuspensionOperation[] = [{ kind: "assign", local: result, value }];
        if (expression.elementTypes) {
          expression.bindings.forEach((binding, index) => {
            operations.push({
              kind: "assign",
              local: binding,
              value: {
                kind: "tuple-index",
                receiver: resultValue,
                index,
                elementType: expression.elementTypes![index]!,
                type: expression.elementTypes![index]!,
                span: binding.span,
              },
            });
          });
        } else {
          operations.push({ kind: "assign", local: expression.bindings[0]!, value: resultValue });
        }
        const next = continuation(resultValue);
        return this.block(operations, { kind: "jump", target: next });
      },
      context,
    );
  }

  private lowerSuspensionOperand(
    expression: HirSuspensionOperandExpression,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    return this.lowerExpression(
      expression.suspension,
      (suspension) => continuation({ ...expression, suspension: suspension! } as HirExpression),
      context,
    );
  }

  private lowerValueList(
    values: readonly HirExpression[],
    continuation: (values: readonly HirExpression[]) => number,
    context: LoweringContext,
    index = 0,
    lowered: readonly HirExpression[] = [],
  ): number {
    if (index >= values.length) return continuation(lowered);
    return this.lowerExpression(
      values[index]!,
      (value) => {
        if (!value || value.type === "void")
          throw new Error("void value in expression operand list");
        const local = this.temporary(value.type, value.span, "operand");
        const next = this.lowerValueList(values, continuation, context, index + 1, [
          ...lowered,
          this.local(local, value.span),
        ]);
        return this.block([{ kind: "assign", local, value }], { kind: "jump", target: next });
      },
      context,
    );
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
      [
        {
          kind: "assign",
          local: result,
          value: { kind: "boolean", value: constant, type: "bool", span: expression.span },
        },
      ],
      { kind: "jump", target: after },
    );
    const rightEntry = this.lowerExpression(
      expression.right,
      (right) =>
        this.block([{ kind: "assign", local: result, value: right! }], {
          kind: "jump",
          target: after,
        }),
      context,
    );
    return this.lowerExpression(
      expression.left,
      (left) =>
        this.block([], {
          kind: "branch",
          condition: left!,
          thenTarget: expression.operator === "and" ? rightEntry : constantBlock,
          elseTarget: expression.operator === "and" ? constantBlock : rightEntry,
        }),
      context,
    );
  }

  private lowerIf(
    expression: Extract<HirExpression, { kind: "if" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result =
      expression.type === "void"
        ? undefined
        : this.temporary(expression.type, expression.span, "if");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const finish = (value: HirExpression | undefined): number =>
      this.block(result && value ? [{ kind: "assign", local: result, value }] : [], {
        kind: "jump",
        target: after,
      });
    const thenEntry = this.lowerSuite(expression.thenBody, expression.type, finish, context);
    const elseEntry = this.lowerSuite(expression.elseBody, expression.type, finish, context);
    return this.lowerExpression(
      expression.condition,
      (condition) =>
        this.block([], {
          kind: "branch",
          condition: condition!,
          thenTarget: thenEntry,
          elseTarget: elseEntry,
        }),
      context,
    );
  }

  private lowerWhile(
    expression: Extract<HirExpression, { kind: "while" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result =
      expression.type === "void"
        ? undefined
        : this.temporary(expression.type, expression.span, "while");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const head = this.reserveBlock();
    const finishElse = (value: HirExpression | undefined): number =>
      this.block(result && value ? [{ kind: "assign", local: result, value }] : [], {
        kind: "jump",
        target: after,
      });
    const elseEntry =
      expression.elseBody.length > 0
        ? this.lowerSuite(expression.elseBody, expression.type, finishElse, context)
        : after;
    const loop: LoopContext = {
      breakTarget: after,
      continueTarget: head,
      resultLocal: result,
      cleanupDepth: context.cleanups.length,
    };
    const bodyEntry = this.lowerSuite(
      expression.body,
      "void",
      () => this.block([], { kind: "jump", target: head }),
      { ...context, loop },
    );
    const conditionEntry = this.lowerExpression(
      expression.condition,
      (condition) =>
        this.block([], {
          kind: "branch",
          condition: condition!,
          thenTarget: bodyEntry,
          elseTarget: elseEntry,
        }),
      context,
    );
    this.setBlock(head, [], { kind: "jump", target: conditionEntry });
    return head;
  }

  private lowerFor(
    expression: Extract<HirExpression, { kind: "for" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result =
      expression.type === "void"
        ? undefined
        : this.temporary(expression.type, expression.span, "for");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const iteratorType = mutableType(nominalGenericType("Iterator", [expression.yieldType]));
    const iterator = this.temporary(iteratorType, expression.iterable.span, "iterator");
    const next = this.temporary(`${expression.yieldType}?`, expression.span, "next");
    const iteratorValue = this.local(iterator, expression.iterable.span);
    const nextValue = this.local(next, expression.span);
    const condition: HirExpression = {
      kind: "binary",
      operator: "==",
      left: { kind: "variant-tag", receiver: nextValue, type: "i32", span: expression.span },
      right: { kind: "integer", value: 1, type: "i32", span: expression.span },
      type: "bool",
      span: expression.span,
    };
    const yielded: HirExpression = {
      kind: "variant-payload",
      receiver: nextValue,
      payloadType: expression.yieldType,
      type: expression.yieldType,
      span: expression.span,
    };
    const finishElse = (value: HirExpression | undefined): number =>
      this.block(result && value ? [{ kind: "assign", local: result, value }] : [], {
        kind: "jump",
        target: after,
      });
    const elseEntry =
      expression.elseBody.length > 0
        ? this.lowerSuite(expression.elseBody, expression.type, finishElse, context)
        : after;
    const head = this.reserveBlock();
    const loop: LoopContext = {
      breakTarget: after,
      continueTarget: head,
      resultLocal: result,
      cleanupDepth: context.cleanups.length,
    };
    const bodyEntry = this.lowerSuite(expression.body, "void", () => head, {
      ...context,
      loop,
    });
    const bindingOperations: SuspensionOperation[] = [];
    if (expression.bindings.length === 1) {
      bindingOperations.push({ kind: "assign", local: expression.bindings[0]!, value: yielded });
    } else {
      const tuple = this.temporary(expression.yieldType, expression.span, "yield");
      bindingOperations.push({ kind: "assign", local: tuple, value: yielded });
      expression.bindings.forEach((binding, bindingIndex) =>
        bindingOperations.push({
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
        }),
      );
    }
    const bindingEntry = this.block(bindingOperations, { kind: "jump", target: bodyEntry });
    const conditionEntry = this.block([], {
      kind: "branch",
      condition,
      thenTarget: bindingEntry,
      elseTarget: elseEntry,
    });
    const nextEntry = this.block(
      [
        {
          kind: "assign",
          local: next,
          value: {
            kind: "iterator-next",
            receiver: iteratorValue,
            elementType: expression.yieldType,
            type: `${expression.yieldType}?`,
            span: expression.span,
          },
        },
      ],
      { kind: "jump", target: conditionEntry },
    );
    this.setBlock(head, [], { kind: "jump", target: nextEntry });
    return this.lowerExpression(
      expression.iterable,
      (value) =>
        this.block(
          [
            {
              kind: "assign",
              local: iterator,
              value:
                expression.iteratorKind === "list"
                  ? {
                      kind: "list-iterator",
                      receiver: value!,
                      elementType: expression.yieldType,
                      type: iteratorType,
                      span: expression.span,
                    }
                  : expression.iteratorKind === "map"
                    ? {
                        kind: "map-iterator",
                        receiver: value!,
                        elementType: expression.yieldType,
                        type: iteratorType,
                        span: expression.span,
                      }
                    : value!,
            },
          ],
          { kind: "jump", target: head },
        ),
      context,
    );
  }

  private lowerMatch(
    expression: Extract<HirExpression, { kind: "match" }>,
    continuation: ValueContinuation,
    context: LoweringContext,
  ): number {
    const result =
      expression.type === "void"
        ? undefined
        : this.temporary(expression.type, expression.span, "match");
    const after = continuation(result ? this.local(result, expression.span) : undefined);
    const finish = (value: HirExpression | undefined): number =>
      this.block(result && value ? [{ kind: "assign", local: result, value }] : [], {
        kind: "jump",
        target: after,
      });
    return this.lowerExpression(
      expression.subject,
      (subjectValue) => {
        if (!subjectValue) throw new Error("match subject cannot be void");
        const subject = this.temporary(subjectValue.type, subjectValue.span, "subject");
        let nextArm = this.block([], { kind: "unreachable" });
        for (let index = expression.arms.length - 1; index >= 0; index -= 1) {
          const arm = expression.arms[index]!;
          const body = this.lowerSuite(arm.body, expression.type, finish, context);
          const boundBody = arm.guard
            ? this.lowerExpression(
                arm.guard,
                (guard) =>
                  this.block([], {
                    kind: "branch",
                    condition: guard!,
                    thenTarget: body,
                    elseTarget: nextArm,
                  }),
                context,
              )
            : body;
          const bindings = this.block(
            [
              {
                kind: "match-bind",
                subject,
                representation: expression.representation,
                enumIndex: expression.enumIndex,
                bindings: arm.bindings,
              },
            ],
            { kind: "jump", target: boundBody },
          );
          const hasCondition = Boolean(
            arm.literal || arm.tag !== undefined || (arm.tests && arm.tests.length > 0),
          );
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
        return this.block([{ kind: "assign", local: subject, value: subjectValue }], {
          kind: "jump",
          target: nextArm,
        });
      },
      context,
    );
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
    return this.lowerExpression(
      entry.value,
      (value) => {
        if (!value) throw new Error("provider entry cannot be void");
        const loweredEntry = { ...entry, value } as HirProviderContextEntry;
        const next = this.lowerProviderEntries(entries, index + 1, continuation, context, [
          ...lowered,
          loweredEntry,
        ]);
        return this.block([{ kind: "provider-entry", entry: loweredEntry }], {
          kind: "jump",
          target: next,
        });
      },
      context,
    );
  }
}
