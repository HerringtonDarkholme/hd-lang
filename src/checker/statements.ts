import { cellType } from "./captured-cells.ts";
import {
  isDefaultedLiteral,
  literalText,
  recordDefaultedLocal,
  withDefaultedLocalHint,
  firstBareLiteral,
} from "./literal-join.ts";
import type { Expression, Statement } from "../ast.ts";
import type { HirExpression, HirGlobal, HirLocal, HirStatement, ValueType } from "../hir.ts";
import {
  functionType,
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  PRIMITIVE_TYPES,
  optionalInner,
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
  tupleLayout,
  tupleRest,
  displayType,
} from "../types.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { numericType } from "../numeric.ts";
import { CheckerContext, CheckFailure, PRELUDE_NAMES, TEST_CASE_FUNCTIONS } from "./context.ts";
import { closestNames, didYouMean } from "./name-suggestions.ts";
import { standardImportHint } from "./standard-uses.ts";
import { genericTypeName, statementsReferenceName, traitTypeName } from "./shared.ts";
import { speculationSafeArguments } from "./call-speculation.ts";

/**
 * The `if` expressions written as statements. Only there may an `if` omit
 * `else` (06-control-flow.md#r-flow.if.value.else, #r-flow.if.statement).
 */
export const STATEMENT_IFS = new WeakSet<Expression>();

/**
 * The readonly locals whose initializer had mutable access, so that a later
 * `mutable-receiver-required` can name the binding's valid fix: `"let"`
 * for an unannotated binding, `"annotated"` for one with a type.
 */
const MUTABLE_INITIALIZERS = new WeakMap<HirLocal, "let" | "annotated">();

export abstract class StatementChecker extends CheckerContext {
  protected failUnknownName(name: string, message: string, span: SourceSpan): never {
    const imported = this.imports.get(name);
    // The prelude's `it` is in scope only in test code; elsewhere it is an
    // unknown name (spec/lang/10-modules.md#r-module.prelude.test-only.outside).
    const preludeIt = name === "it" && this.declaration.testOnly === true;
    if (preludeIt || (imported !== undefined && TEST_CASE_FUNCTIONS.has(imported)))
      this.fail(
        "misplaced-test-case",
        `${name}(...) registers a test case only as a direct call at the top level of a tests block`,
        span,
      );
    if (this.declaration.defaultContext?.laterNames.includes(name))
      this.fail(
        "binding-not-yet-visible",
        `a default cannot refer to the later parameter '${name}'`,
        span,
      );
    this.fail("unknown-name", `${message}${this.unknownNameHint(name)}`, span);
  }

  /**
   * The fix for an unknown value name: the use that imports a std name, or
   * else the visible names it may misspell. A test registration function
   * outside test code gets no use hint, since importing it cannot help there.
   */
  private unknownNameHint(name: string): string {
    if (name === "loop") return "";
    const testCase = TEST_CASE_FUNCTIONS.has(`std.testing.${name}`) && !this.declaration.testOnly;
    const imported = testCase ? "" : standardImportHint(name, "value");
    if (imported) return imported;
    const visible = [
      ...this.scopes.flatMap((scope) => [...scope.keys()]),
      ...this.availableCaptures.keys(),
      ...[...this.globals.keys()].filter((global) => this.resolveGlobal(global)),
      ...[...this.signatures.keys()].filter((function_) => this.visibleSignature(function_)),
      ...this.imports.keys(),
    ].filter(
      (candidate) =>
        /^[A-Za-z][A-Za-z0-9_]*$|^_[A-Za-z0-9_]+$/.test(candidate) && !candidate.startsWith("__"),
    );
    return didYouMean(closestNames(name, visible));
  }

  /**
   * The fix for a readonly binding `source` that needs mutable access: the
   * `let` form or parameter type that gives it, when the spec allows one
   * (04-type-system.md#binding-forms, #r-types.param.mut). A generic
   * parameter, `self`, and a method's parameter get none: their fix is a
   * bound, or a signature that a trait may fix.
   */
  protected readonlyBindingHint(source: Expression): string {
    if (source.kind !== "name") return "";
    const binding = this.resolveLocal(source.name) ?? this.availableCaptures.get(source.name);
    if (!binding || mutableInner(binding.type) !== undefined) return "";
    const mutable = displayType(mutableType(binding.type));
    const form = MUTABLE_INITIALIZERS.get(binding);
    if (form === "let") return `; declare it 'let mut ${source.name} = ...'`;
    if (form === "annotated") return `; declare it 'let ${source.name}: ${mutable} = ...'`;
    const parameters = this.declaration.parameters;
    if (
      !binding.parameter ||
      this.insideClosure ||
      !this.locals.includes(binding) ||
      parameters[0]?.name === "self" ||
      genericTypeName(binding.type) !== undefined ||
      !parameters.some((parameter) => parameter.name === source.name && parameter.type)
    )
      return "";
    return `; declare the parameter '${source.name}: ${mutable}'`;
  }

  /** Whether a resolved type is one of std.ops' range types. */
  protected abstract isRangeType(type: ValueType): boolean;

  /** Rejects signed and non-integer built-in indices after contextual checking. */
  protected requireUnsignedIndex(
    index: HirExpression,
    receiver: "list" | "string",
    span: SourceSpan,
  ): HirExpression {
    if (numericType(readonlyType(index.type))?.family !== "unsigned")
      this.fail(
        "type-mismatch",
        `a ${receiver} index must have an unsigned integer type, found '${displayType(index.type)}'`,
        span,
      );
    return index;
  }

  /** `r[k] = v` on a receiver other than `List` and `Map`, through `IndexSet`. */
  protected abstract indexSetCall(
    statement: Extract<Statement, { kind: "index-assignment" }>,
    receiver: HirExpression,
  ): HirExpression;

  /** `v() = x` through `Update`; see `operator-calls.ts`. */
  protected abstract updateCall(
    statement: Extract<Statement, { kind: "call-assignment" }>,
  ): HirExpression;

  /** `place.field = value` and `place.Part ...= value`; see `expression-data.ts`. */
  protected abstract checkFieldAssignment(
    statement: Extract<Statement, { kind: "field-assignment" }>,
  ): HirStatement;

  /** `...=` on a local or an indexed place (08 Data Embedding). */
  protected failCopyIntoOrdinaryPlace(span: SourceSpan): never {
    this.fail(
      "copy-into-ordinary-field",
      "the copy assignment '...=' stores into an embedded field only; use '=' for any other place",
      span,
    );
  }

  protected checkStatement(
    statement: Statement,
    expected?: ValueType,
    valueContext = false,
  ): HirStatement {
    switch (statement.kind) {
      case "defer": {
        if (this.moduleBody) {
          this.fail(
            "defer-outside-cleanup-scope",
            "defer is only valid inside a function or closure body",
            statement.span,
          );
        }
        this.deferDepth += 1;
        let body: readonly HirStatement[];
        try {
          body = this.checkStatements(statement.body, true);
        } finally {
          this.deferDepth -= 1;
        }
        return { kind: "defer", body, span: statement.span };
      }
      case "binding":
        return this.checkBindingStatement(statement);
      case "tuple-binding":
        throw new Error("tuple bindings are expanded by checkStatements");
      case "pattern-binding":
        throw new Error("pattern bindings are expanded by checkStatements");
      case "assignment": {
        if (statement.copy) this.failCopyIntoOrdinaryPlace(statement.span);
        const local = this.resolveLocal(statement.name);
        const global = local ? undefined : this.resolveGlobal(statement.name);
        const captured = !local && !global ? this.availableCaptures.get(statement.name) : undefined;
        // A captured local shadows a later module binding of its name.
        if (!local && !global && !captured && this.globals.has(statement.name)) {
          this.fail(
            "binding-not-yet-visible",
            `module binding '${statement.name}' is not visible before its binding point`,
            statement.span,
          );
        }
        // A closure assigns captured `let` storage through its shared cell
        // (07-functions.md#r-fn.capture.mutate).
        if (captured?.mutable) {
          const value = this.requireCoercion(
            this.checkExpression(statement.value, captured.type),
            captured.type,
            statement.value.span,
          );
          const cell: HirExpression = {
            kind: "capture",
            closureIndex: this.closureIndex,
            fieldIndex: this.captureField(statement.name, captured),
            type: cellType(captured.type),
            span: statement.span,
          };
          return {
            kind: "expression",
            expression: { kind: "cell-set", cell, value, type: "void", span: statement.span },
            span: statement.span,
          };
        }
        if (captured)
          this.fail(
            "non-reassignable-binding",
            `binding '${statement.name}' is not reassignable`,
            statement.span,
          );
        if (!local && !global)
          this.fail("unknown-name", `unknown binding '${statement.name}'`, statement.span);
        if (local && !local.mutable) {
          const code = local.parameter
            ? "non-reassignable-parameter-binding"
            : "non-reassignable-binding";
          this.fail(code, `binding '${statement.name}' is not reassignable`, statement.span);
        }
        if (global && !global.mutable) {
          this.fail(
            "non-reassignable-binding",
            `binding '${statement.name}' is not reassignable`,
            statement.span,
          );
        }
        const target = local ?? global!;
        const value = this.requireCoercion(
          this.checkExpression(statement.value, target.type),
          target.type,
          statement.value.span,
        );
        return local
          ? { kind: "assignment", local, value, span: statement.span }
          : { kind: "global-assignment", global: global!, value, span: statement.span };
      }
      case "field-assignment":
        return this.checkFieldAssignment(statement);
      case "index-assignment": {
        if (statement.copy) this.failCopyIntoOrdinaryPlace(statement.span);
        const receiver = this.checkExpression(statement.target.receiver);
        const mutableReceiver = mutableInner(receiver.type);
        const builtInReceiver = nominalGenericParts(readonlyType(receiver.type))?.name;
        if (builtInReceiver !== "List" && builtInReceiver !== "Map")
          return {
            kind: "expression",
            expression: this.indexSetCall(statement, receiver),
            span: statement.span,
          };
        // No range type has an `IndexSet` implementation for a list, so a
        // slice is not a place (05-expressions.md#r-expr.index.slice.no-store).
        if (builtInReceiver === "List" && statement.target.index.kind === "range")
          this.fail(
            "invalid-assignment-target",
            "a list slice is a new list, not a place; assign each element instead",
            statement.target.span,
          );
        if (mutableReceiver === undefined) {
          this.fail(
            "readonly-root",
            `indexed assignment requires mutable access to '${displayType(receiver.type)}'`,
            statement.target.receiver.span,
          );
        }
        const nominal = nominalGenericParts(mutableReceiver);
        if (nominal?.name === "List" && nominal.arguments.length === 1) {
          const checkedIndex = this.checkExpression(statement.target.index, "usize");
          if (this.isRangeType(checkedIndex.type))
            this.fail(
              "invalid-assignment-target",
              "a list slice is a new list, not a place; assign each element instead",
              statement.target.span,
            );
          const index = this.requireUnsignedIndex(
            checkedIndex,
            "list",
            statement.target.index.span,
          );
          const value = this.requireCoercion(
            this.checkExpression(statement.value, nominal.arguments[0]),
            nominal.arguments[0]!,
            statement.value.span,
          );
          return {
            kind: "expression",
            expression: {
              kind: "list-set",
              receiver,
              index,
              value,
              elementType: nominal.arguments[0]!,
              type: "void",
              span: statement.span,
            },
            span: statement.span,
          };
        }
        if (nominal?.name === "Map" && nominal.arguments.length === 2) {
          const key = this.requireCoercion(
            this.checkExpression(statement.target.index, nominal.arguments[0]),
            nominal.arguments[0]!,
            statement.target.index.span,
          );
          const value = this.requireCoercion(
            this.checkExpression(statement.value, nominal.arguments[1]),
            nominal.arguments[1]!,
            statement.value.span,
          );
          return {
            kind: "expression",
            expression: {
              kind: "map-set",
              receiver,
              key,
              value,
              keyType: nominal.arguments[0]!,
              valueType: nominal.arguments[1]!,
              type: "void",
              span: statement.span,
            },
            span: statement.span,
          };
        }
        // Any other receiver stores through `IndexSet::[K, V]::index_set`,
        // and is a place only when it implements `IndexSet`
        // (05-expressions.md#r-expr.index.trait.write, #r-expr.index.trait.place).
        return {
          kind: "expression",
          expression: this.indexSetCall(statement, receiver),
          span: statement.span,
        };
      }
      case "call-assignment":
        return {
          kind: "expression",
          expression: this.updateCall(statement),
          span: statement.span,
        };
      case "discard":
        return {
          kind: "discard",
          value: this.checkExpression(statement.value),
          span: statement.span,
        };
      case "return": {
        if (this.moduleBody)
          this.fail(
            "return-outside-function",
            "return is not valid at module top level",
            statement.span,
          );
        if (this.deferDepth > 0)
          this.fail("defer-control-flow", "a defer suite cannot return", statement.span);
        let value =
          statement.value &&
          this.checkExpression(
            statement.value,
            this.inferResult ? undefined : this.signature.result,
          );
        if (this.inferResult)
          this.recordInferredReturn(value?.type ?? "void", statement.span, value);
        else if (value) value = this.requireCoercion(value, this.signature.result, statement.span);
        else this.requireAssignable("void", this.signature.result, statement.span);
        return { kind: "return", value, span: statement.span };
      }
      case "break":
        if (this.deferDepth > 0)
          this.fail("defer-control-flow", "a defer suite cannot break", statement.span);
        if (this.loopResults.length === 0)
          this.fail("break-outside-loop", "break is only valid inside a loop", statement.span);
        // The nearest loop can now complete normally (06-control-flow.md#r-flow.while.infinite.exit).
        this.loopBroken[this.loopBroken.length - 1] = true;
        const loopResult = this.loopResults.at(-1);
        if (loopResult === undefined && statement.value) {
          this.fail(
            "break-value-context",
            "break values require a loop with an else suite",
            statement.span,
          );
        }
        if (loopResult !== undefined && !statement.value) {
          this.fail(
            "break-value-context",
            "a value-producing loop requires 'break value'",
            statement.span,
          );
        }
        // A loop whose `else` value is a defaulted literal joins it with the
        // `break` values (the join model, literal-join.ts).
        const joining = this.loopJoins.at(-1);
        const value =
          statement.value &&
          this.checkExpression(statement.value, joining ? undefined : loopResult);
        if (value && joining) joining.push(value);
        else if (value && loopResult)
          this.requireAssignable(value.type, loopResult, statement.value!.span);
        return { kind: "break", value, span: statement.span };
      case "continue":
        if (this.deferDepth > 0)
          this.fail("defer-control-flow", "a defer suite cannot continue", statement.span);
        if (this.loopResults.length === 0)
          this.fail(
            "continue-outside-loop",
            "continue is only valid inside a loop",
            statement.span,
          );
        return { kind: "continue", span: statement.span };
      case "expression": {
        if (statement.expression.kind === "if") STATEMENT_IFS.add(statement.expression);
        const expression = this.checkExpression(statement.expression, expected);
        if (
          !valueContext &&
          (expected === undefined || expected === "void") &&
          (optionalInner(expression.type) !== undefined ||
            resultParts(expression.type) ||
            suspensionParts(expression.type) ||
            traitSuspensionParts(expression.type))
        ) {
          this.fail(
            "discarded-must-use-value",
            `a value of type '${displayType(expression.type)}' must be used or explicitly discarded`,
            statement.span,
          );
        }
        // A `debug(x)` statement drops the text it returns
        // (spec/lang/10-modules.md#r-module.dbg.debug-hint).
        if (
          !valueContext &&
          (expected === undefined || expected === "void") &&
          this.declaration.standard !== true &&
          statement.expression.kind === "call" &&
          statement.expression.callee.kind === "name" &&
          // `debug` is a prelude name, which no declaration may shadow.
          statement.expression.callee.name === "debug" &&
          readonlyType(expression.type) === "string"
        )
          this.diagnostics.push({
            code: "unused-debug-text",
            severity: "warning",
            message: "`debug` returns the text; to print it, use `dbg(x)`",
            span: statement.span,
          });
        return { kind: "expression", expression, span: statement.span };
      }
      case "pass":
        return { kind: "pass", span: statement.span };
      case "local-implementation":
        this.activateLocalImplementation(statement.implementation);
        return { kind: "pass", span: statement.span };
      case "local-declaration":
        throw new Error("local declarations are hoisted before checking");
    }
  }

  /**
   * A tuple pattern or `let` list against a tuple type: one that ends in a
   * spread pattern needs a rest tuple with as many fixed elements as the
   * names before it, and a rest tuple needs a spread pattern
   * (06-control-flow.md#spread-patterns).
   */
  protected checkSpreadArity(spreads: readonly boolean[], type: ValueType, span: SourceSpan): void {
    const tuple = tupleRest(readonlyType(type));
    if (!tuple) return;
    const spread = spreads.at(-1) === true;
    if (spread && tuple.rest === undefined)
      this.fail(
        "type-mismatch",
        `a spread pattern needs a tuple type with a rest element, found '${displayType(type)}'`,
        span,
      );
    if (!spread && tuple.rest !== undefined)
      this.fail(
        "type-mismatch",
        `a tuple pattern against '${displayType(type)}' must end in a spread pattern, as in '(..., xs...)'`,
        span,
      );
    if (spread && spreads.length - 1 !== tuple.fixed.length)
      this.fail(
        "type-mismatch",
        `'${displayType(type)}' has ${tuple.fixed.length} fixed element${tuple.fixed.length === 1 ? "" : "s"}, but the spread pattern follows ${spreads.length - 1}`,
        span,
      );
  }

  /** A `let` pattern; see `expression-comprehensions.ts`. */
  protected abstract checkPatternBinding(
    statement: Extract<Statement, { kind: "pattern-binding" }>,
  ): HirStatement[];

  protected checkDestructuring(
    statement: Extract<Statement, { kind: "tuple-binding" | "pattern-binding" }>,
  ): HirStatement[] {
    return statement.kind === "tuple-binding"
      ? this.checkTupleBinding(statement)
      : this.checkPatternBinding(statement);
  }

  protected checkTupleBinding(
    statement: Extract<Statement, { kind: "tuple-binding" }>,
  ): HirStatement[] {
    const annotation = statement.annotation ? this.resolveType(statement.annotation) : undefined;
    const annotatedElements = annotation ? tupleParts(annotation) : undefined;
    if (annotation && annotatedElements === undefined) {
      this.fail(
        "tuple-binding-annotation",
        `tuple binding annotation '${displayType(annotation)}' is not a tuple type`,
        statement.annotation!.span,
      );
    }
    const value = this.checkExpression(statement.value, annotation);
    const elements = tupleLayout(value.type);
    if (elements === undefined) {
      this.fail(
        "type-mismatch",
        `tuple binding requires a tuple value, found '${displayType(value.type)}'`,
        statement.value.span,
      );
    }
    this.checkSpreadArity(
      statement.bindings.map((binding) => binding.spread === true),
      value.type,
      statement.span,
    );
    if (elements.length !== statement.bindings.length) {
      this.fail(
        "type-mismatch",
        `tuple binding has ${statement.bindings.length} names for ${elements.length} elements`,
        statement.span,
      );
    }
    const names = new Set<string>();
    for (const binding of statement.bindings) {
      if (binding.name === "_") continue;
      if (names.has(binding.name))
        this.fail(
          "duplicate-binding",
          `binding '${binding.name}' appears more than once`,
          binding.span,
        );
      names.add(binding.name);
      if (PRELUDE_NAMES.has(binding.name))
        this.fail(
          "prelude-name-shadow",
          `local binding '${binding.name}' shadows a prelude name`,
          binding.span,
        );
      if (
        this.currentScope().has(binding.name) ||
        (this.moduleBody && this.globals.has(binding.name))
      ) {
        this.fail(
          "duplicate-binding",
          `binding '${binding.name}' already exists in this ${this.moduleBody ? "module" : "function"}`,
          binding.span,
        );
      }
    }
    const tupleLocal: HirLocal = {
      name: `$tuple-binding${this.locals.length}`,
      type: value.type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span: statement.span,
    };
    this.locals.push(tupleLocal);
    const output: HirStatement[] = [
      { kind: "binding", local: tupleLocal, value, span: statement.span },
    ];
    const sourceElements =
      statement.value.kind === "tuple" && !statement.value.spread
        ? statement.value.elements
        : undefined;
    const checkedElements = value.kind === "tuple" ? value.elements : undefined;
    // Each name of a multi-name binding infers its own element's access
    // (04-type-system.md#r-types.bind.let-pattern-mut).
    const bindingTypes = statement.bindings.map((binding, index) => {
      const element = elements[index]!;
      if (binding.mutableAccess) {
        const annotated = annotatedElements?.[index];
        if (annotated !== undefined) {
          this.requireMutableAnnotation(annotated, binding);
          // (04-type-system.md#r-types.bind.let-mut-pattern.redundant)
          this.warnRedundantLetMut(binding);
        } else this.requireMutableValue(element, binding.span);
        return element;
      }
      return annotation ? element : readonlyType(element);
    });
    for (const [index, binding] of statement.bindings.entries()) {
      if (binding.name === "_") continue;
      const sourceElement = sourceElements?.[index];
      const checkedElement = checkedElements?.[index];
      if (this.moduleBody) {
        const global: HirGlobal = {
          name: binding.name,
          type: bindingTypes[index]!,
          index: this.globals.size,
          mutable: statement.mutable,
          span: binding.span,
        };
        this.globals.set(binding.name, global);
        output.push({
          kind: "global-binding",
          global,
          value: {
            kind: "tuple-index",
            receiver: {
              kind: "local",
              local: tupleLocal,
              type: tupleLocal.type,
              span: statement.value.span,
            },
            index,
            elementType: elements[index]!,
            type: elements[index]!,
            span: binding.span,
          },
          span: binding.span,
        });
        continue;
      }
      const local: HirLocal = {
        name: binding.name,
        type: bindingTypes[index]!,
        index: this.locals.length,
        mutable: statement.mutable,
        parameter: false,
        span: binding.span,
      };
      this.locals.push(local);
      this.currentScope().set(binding.name, local);
      const defaultedLiteral =
        sourceElement?.kind === "integer"
          ? isDefaultedLiteral(checkedElement)
            ? sourceElement
            : undefined
          : sourceElement &&
              checkedElement &&
              ["list", "map", "tuple", "data", "range"].includes(sourceElement.kind) &&
              /\busize\b/.test(checkedElement.type)
            ? firstBareLiteral(sourceElement)
            : undefined;
      if (defaultedLiteral)
        recordDefaultedLocal(local, {
          name: binding.name,
          literal: literalText(defaultedLiteral),
          span: defaultedLiteral.span,
          ...(defaultedLiteral !== sourceElement ? { kind: "structure" as const } : {}),
          statement: statement.span,
          initializer: sourceElement!.span,
          letMut: binding.mutableAccess === true,
        });
      output.push({
        kind: "binding",
        local,
        value: {
          kind: "tuple-index",
          receiver: {
            kind: "local",
            local: tupleLocal,
            type: tupleLocal.type,
            span: statement.value.span,
          },
          index,
          elementType: elements[index]!,
          type: elements[index]!,
          span: binding.span,
        },
        span: binding.span,
      });
    }
    return output;
  }
  /**
   * A `mut` before a name whose annotated type is already `mut T` is
   * redundant; the fix-it deletes it and keeps the annotation
   * (04-type-system.md#r-types.bind.let-mut-annotated.fix).
   */
  private warnRedundantLetMut(site: {
    readonly span: SourceSpan;
    readonly mutSpan?: SourceSpan;
  }): void {
    this.diagnostics.push({
      code: "redundant-let-mut",
      message:
        "the annotated type already has mutable access, so 'mut' before the name is redundant",
      span: site.mutSpan ?? site.span,
      severity: "warning",
      ...(site.mutSpan
        ? {
            fix: {
              message: "remove 'mut'",
              edits: [{ span: site.mutSpan, replacement: "" }],
            },
          }
        : {}),
    });
  }

  /** `let mut` needs mutable access, including an optional's declared payload. */
  private requireMutableAnnotation(
    annotation: ValueType,
    site: { readonly span: SourceSpan },
  ): void {
    if (this.mutableBindingAccess(annotation)) return;
    this.rejectLetMutPrimitive(annotation, site.span);
    this.fail(
      "let-mut-readonly-type",
      `'let mut' asks for mutable access, but the type '${displayType(annotation)}' is readonly; write '${displayType(mutableType(annotation))}', or drop 'mut' after 'let'`,
      site.span,
    );
  }

  /** `let mut` never upgrades a readonly value (04-type-system.md#r-types.bind.let-mut-upgrade). */
  private requireMutableValue(type: ValueType, span: SourceSpan): void {
    if (this.mutableBindingAccess(type) || type === "never") return;
    this.rejectLetMutPrimitive(type, span);
    this.fail(
      "mutable-upgrade",
      `'let mut' needs a value with mutable access, but '${displayType(type)}' is readonly and cannot be upgraded; copy it into a fresh value instead`,
      span,
    );
  }

  /** Optional wrappers retain access to their declared mutable payload. */
  private mutableBindingAccess(type: ValueType): boolean {
    if (mutableInner(type) !== undefined) return true;
    const payload = optionalInner(type);
    return payload !== undefined && mutableInner(payload) !== undefined;
  }

  /**
   * `let mut` on a primitive is `mut-on-primitive`, and on a tuple `mut-on-tuple`, in place of
   * `mutable-upgrade` or `let-mut-readonly-type`
   * (04-type-system.md#r-types.bind.let-mut-primitive).
   */
  private rejectLetMutPrimitive(type: ValueType, span: SourceSpan): void {
    // A tuple has no `mut` form either (04-type-system.md#r-types.bind.let-mut-tuple).
    if (tupleParts(type) !== undefined)
      this.fail(
        "mut-on-tuple",
        `'let mut' asks for mutable access, but '${displayType(type)}' is a tuple with no 'mut' form; use plain 'let', and put 'mut' on the element type`,
        span,
      );
    if (!PRIMITIVE_TYPES.has(type)) return;
    this.fail(
      "mut-on-primitive",
      `'let mut' asks for mutable access, but '${displayType(type)}' is a primitive type with no mutable state; a plain 'let' is already reassignable`,
      span,
    );
  }

  /**
   * A non-generic data literal after `let mut` is used as `mut T`
   * (04-type-system.md#r-types.bind.let-mut-expected), which also gives its
   * fields their expected types.
   */
  private letMutLiteralType(value: Expression): ValueType | undefined {
    if (value.kind !== "data" || value.typeArguments) return undefined;
    const declaration = this.dataTypes.get(value.name);
    return declaration && declaration.genericParameters.length === 0 && !declaration.newtype
      ? mutableType(value.name)
      : undefined;
  }

  /** Per loop, the `break` values that join its defaulted `else` literal. */
  protected readonly loopJoins: (HirExpression[] | undefined)[] = [];

  /** Per loop, whether a `break` targets it, so an infinite loop can complete. */
  protected readonly loopBroken: boolean[] = [];

  /** A coercion whose failure gains the join model's literal fix hint (literal-join.ts). */
  protected override requireCoercion(
    expression: HirExpression,
    expected: ValueType,
    span: SourceSpan,
  ): HirExpression {
    return this.withLiteralHint([[expression, expected]], () =>
      super.requireCoercion(expression, expected, span),
    );
  }

  /** A parameter joins only by removing `mut`, never widening, variance, wrapping, or traits (types.generic.infer.join). */
  protected failArgumentJoin(
    name: string,
    parameter: string,
    earlier: ValueType,
    current: ValueType,
    span: SourceSpan,
  ): never {
    if (traitTypeName(earlier) !== undefined || traitTypeName(current) !== undefined)
      this.fail(
        "no-common-type",
        `arguments of types '${displayType(earlier)}' and '${displayType(current)}' both solve '${displayType(parameter)}' of '${name}', and inference never converts to a trait value; write '${name}::[${displayType(traitTypeName(earlier) ?? traitTypeName(current)!)}](...)'`,
        span,
      );
    this.fail(
      "type-mismatch",
      `arguments of types '${displayType(earlier)}' and '${displayType(current)}' both solve '${displayType(parameter)}' of '${name}', and inference converts only 'mut X' to 'X'; convert one argument to the other's type`,
      span,
    );
  }

  /**
   * Converts an argument to its solved formal, which is also its expected
   * type. An argument checked while the formal was still open had no expected
   * type, so a fresh value nested in it kept its `mut`, as the inner literal
   * of `nested(Box { item: Box { item: 2.25 } })` for `b: Box[Box[T]]` does.
   * When that argument does not convert, it is checked again against the
   * formal its own type solved, and the fresh value weakens there
   * (04-type-system.md#r-types.fresh.weaken). The first check had no lasting
   * effect, because the argument is speculation-safe. Only a fresh value's
   * own `mut` weakens: an already built `Box[mut Box[f64]]` still fails.
   */
  protected coerceArgument(
    source: Expression,
    checked: HirExpression,
    formal: ValueType,
    recheck: boolean,
  ): HirExpression {
    const reported = this.diagnostics.length;
    try {
      return this.requireCoercion(checked, formal, source.span);
    } catch (error) {
      if (!(error instanceof CheckFailure) || !recheck || !speculationSafeArguments(source))
        throw error;
      this.diagnostics.length = reported;
    }
    return this.requireCoercion(this.checkExpression(source, formal), formal, source.span);
  }

  /** `check`, whose failure gains the join model's literal fix hint (literal-join.ts). */
  protected withLiteralHint<T>(
    pairs: readonly (readonly [HirExpression, ValueType])[],
    check: () => T,
  ): T {
    const before = this.diagnostics.length;
    try {
      return check();
    } catch (error) {
      if (error instanceof CheckFailure && this.diagnostics.length === before + 1)
        this.diagnostics[before] = withDefaultedLocalHint(this.diagnostics[before]!, pairs);
      throw error;
    }
  }

  private checkBindingStatement(statement: Extract<Statement, { kind: "binding" }>): HirStatement {
    if (PRELUDE_NAMES.has(statement.name)) {
      this.fail(
        "prelude-name-shadow",
        `local binding '${statement.name}' shadows a prelude name`,
        statement.span,
      );
    }
    if (
      this.currentScope().has(statement.name) ||
      (this.moduleBody && this.globals.has(statement.name))
    ) {
      this.fail(
        "duplicate-binding",
        `binding '${statement.name}' already exists in this ${this.moduleBody ? "module" : "function"}`,
        statement.span,
      );
    }
    const annotation = statement.annotation ? this.resolveType(statement.annotation) : undefined;
    const letMut = statement.mutableAccess === true;
    if (letMut && annotation !== undefined) {
      this.requireMutableAnnotation(annotation, statement);
      // The `mut` after `let` is redundant with a `mut T` annotation
      // (04-type-system.md#r-types.bind.let-mut-annotated.warning).
      this.warnRedundantLetMut(statement);
    }
    const storedSuspension = annotation ? storedSuspensionParts(annotation) : undefined;
    let recursiveLocal: HirLocal | undefined;
    let recursiveGlobal: HirGlobal | undefined;
    const recursiveClosure =
      statement.value.kind === "closure" &&
      statementsReferenceName(statement.value.body, statement.name);
    if (
      recursiveClosure &&
      statement.value.kind === "closure" &&
      !statement.value.result &&
      !(annotation && functionParts(annotation))
    ) {
      this.fail(
        "recursive-function-needs-result-type",
        `recursive ${statement.localFunction ? "local function" : "closure"} '${statement.name}' needs an explicit result type`,
        statement.value.span,
      );
    }
    if (recursiveClosure && statement.value.kind === "closure") {
      let recursiveType = annotation && functionParts(annotation) ? annotation : undefined;
      if (
        !recursiveType &&
        statement.value.result &&
        statement.value.parameters.every((parameter) => parameter.type)
      ) {
        recursiveType = functionType(
          statement.value.parameters.map((parameter) => this.resolveType(parameter.type!)),
          this.resolveType(statement.value.result),
          statement.value.requirements ?? [],
          false,
          statement.value.suspending === true,
        );
      }
      if (recursiveType) {
        if (this.moduleBody) {
          recursiveGlobal = {
            ...(statement.standard ? { standard: true as const } : {}),
            name: statement.name,
            type: recursiveType,
            index: this.globals.size,
            mutable: statement.mutable,
            span: statement.span,
          };
          this.globals.set(statement.name, recursiveGlobal);
        } else {
          recursiveLocal = {
            name: statement.name,
            type: recursiveType,
            index: this.locals.length,
            mutable: statement.mutable,
            parameter: false,
            span: statement.span,
          };
          this.locals.push(recursiveLocal);
          this.currentScope().set(statement.name, recursiveLocal);
        }
      }
    }
    const previousRecursiveClosure = this.pendingRecursiveClosure;
    const previousInferredBinding = this.inferredBinding;
    this.pendingRecursiveClosure = recursiveLocal;
    this.inferredBinding = annotation ? undefined : statement;
    let value: HirExpression;
    try {
      value = this.checkExpression(
        statement.value,
        storedSuspension?.result ??
          annotation ??
          recursiveLocal?.type ??
          recursiveGlobal?.type ??
          (letMut ? this.letMutLiteralType(statement.value) : undefined),
      );
    } finally {
      this.pendingRecursiveClosure = previousRecursiveClosure;
      this.inferredBinding = previousInferredBinding;
    }
    // A bare literal that defaulted to `usize` is remembered for the fix
    // hint of a later conflict (the join model, literal-join.ts).
    const defaultedLiteral = annotation
      ? undefined
      : statement.value.kind === "integer"
        ? isDefaultedLiteral(value)
          ? statement.value
          : undefined
        : ["list", "map", "tuple", "data", "range"].includes(statement.value.kind) &&
            /\busize\b/.test(value.type)
          ? firstBareLiteral(statement.value)
          : undefined;
    // `let mut name = value`, or a `mut T` annotation, is valid for this
    // value (04-type-system.md#r-types.bind.let-mut-infer, #r-types.bind.let-mut).
    const mutableInitializer =
      !letMut &&
      (this.mutableBindingAccess(value.type) ||
        (annotation !== undefined && ["data", "enum", "list", "map"].includes(value.kind)));
    // `:=` and a plain `let` infer the readonly view, even of a fresh value;
    // `let mut` infers `mut T` and never upgrades a readonly value
    // (04-type-system.md#binding-forms).
    if (letMut && !annotation) this.requireMutableValue(value.type, statement.value.span);
    if (!letMut && !annotation) {
      const readonly = mutableInner(value.type);
      if (readonly !== undefined) {
        value = ["data", "enum", "list", "map", "tuple", "closure"].includes(value.kind)
          ? { ...value, type: readonly }
          : this.requireCoercion(value, readonly, statement.value.span);
      }
    }
    const type = annotation ?? value.type;
    if (type === "never")
      this.fail(
        "uninhabited-binding",
        "an inferred binding cannot have type never",
        statement.span,
      );
    value = this.requireCoercion(value, type, statement.value.span);
    if (this.moduleBody) {
      const global: HirGlobal = recursiveGlobal ?? {
        ...(statement.standard ? { standard: true as const } : {}),
        name: statement.name,
        type,
        index: this.globals.size,
        mutable: statement.mutable,
        drivable: storedSuspension?.mutable,
        span: statement.span,
      };
      if (!recursiveGlobal) this.globals.set(statement.name, global);
      return { kind: "global-binding", global, value, span: statement.span };
    }
    const local: HirLocal = recursiveLocal ?? {
      name: statement.name,
      type,
      index: this.locals.length,
      mutable: statement.mutable,
      drivable: storedSuspension?.mutable,
      parameter: false,
      span: statement.span,
    };
    if (!recursiveLocal) {
      this.locals.push(local);
      this.currentScope().set(statement.name, local);
    }
    if (mutableInitializer && mutableInner(type) === undefined)
      MUTABLE_INITIALIZERS.set(local, annotation ? "annotated" : "let");
    if (defaultedLiteral)
      recordDefaultedLocal(local, {
        name: statement.name,
        literal: literalText(defaultedLiteral),
        span: defaultedLiteral.span,
        ...(defaultedLiteral !== statement.value ? { kind: "structure" as const } : {}),
        statement: statement.span,
        initializer: statement.value.span,
        letMut: statement.mutableAccess === true,
      });
    return { kind: "binding", local, value, span: statement.span };
  }
}
