import { cellType } from "./captured-cells.ts";
import type { Expression, Statement } from "../ast.ts";
import type { HirExpression, HirGlobal, HirLocal, HirStatement, ValueType } from "../hir.ts";
import {
  functionType,
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { CheckerContext, PRELUDE_NAMES } from "./context.ts";
import { statementsReferenceName } from "./shared.ts";

export abstract class StatementChecker extends CheckerContext {
  /** `r[k] = v` on a receiver other than `List` and `Map`, through `IndexSet`. */
  protected abstract indexSetCall(
    statement: Extract<Statement, { kind: "index-assignment" }>,
    receiver: HirExpression,
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
      case "assignment": {
        if (statement.copy) this.failCopyIntoOrdinaryPlace(statement.span);
        const local = this.resolveLocal(statement.name);
        const global = local ? undefined : this.resolveGlobal(statement.name);
        if (!local && !global && this.globals.has(statement.name)) {
          this.fail(
            "binding-not-yet-visible",
            `module binding '${statement.name}' is not visible before its binding point`,
            statement.span,
          );
        }
        const captured = !local && !global ? this.availableCaptures.get(statement.name) : undefined;
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
        if (mutableReceiver === undefined) {
          this.fail(
            "readonly-root",
            `indexed assignment requires mutable access to '${receiver.type}'`,
            statement.target.receiver.span,
          );
        }
        const nominal = nominalGenericParts(mutableReceiver);
        if (nominal?.name === "List" && nominal.arguments.length === 1) {
          const index = this.requireCoercion(
            this.checkExpression(statement.target.index, "i32"),
            "i32",
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
        // Any other receiver stores through `IndexSet[K, V]::index_set`,
        // and is a place only when it implements `IndexSet`
        // (05-expressions.md#r-expr.index.trait.write, #r-expr.index.trait.place).
        return {
          kind: "expression",
          expression: this.indexSetCall(statement, receiver),
          span: statement.span,
        };
      }
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
        if (this.inferResult) this.recordInferredReturn(value?.type ?? "void", statement.span);
        else if (value) value = this.requireCoercion(value, this.signature.result, statement.span);
        else this.requireAssignable("void", this.signature.result, statement.span);
        return { kind: "return", value, span: statement.span };
      }
      case "break":
        if (this.deferDepth > 0)
          this.fail("defer-control-flow", "a defer suite cannot break", statement.span);
        if (this.loopResults.length === 0)
          this.fail("break-outside-loop", "break is only valid inside a loop", statement.span);
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
        const value = statement.value && this.checkExpression(statement.value, loopResult);
        if (value && loopResult)
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
            `a value of type '${expression.type}' must be used or explicitly discarded`,
            statement.span,
          );
        }
        return { kind: "expression", expression, span: statement.span };
      }
      case "pass":
        return { kind: "pass", span: statement.span };
      case "local-declaration":
        throw new Error("local declarations are hoisted before checking");
    }
  }

  protected checkTupleBinding(
    statement: Extract<Statement, { kind: "tuple-binding" }>,
  ): HirStatement[] {
    const annotation = statement.annotation ? this.resolveType(statement.annotation) : undefined;
    const annotatedElements = annotation ? tupleParts(annotation) : undefined;
    if (annotation && annotatedElements === undefined) {
      this.fail(
        "tuple-binding-annotation",
        `tuple binding annotation '${annotation}' is not a tuple type`,
        statement.annotation!.span,
      );
    }
    const value = this.checkExpression(statement.value, annotation);
    const elements = tupleParts(value.type);
    if (elements === undefined) {
      this.fail(
        "type-mismatch",
        `tuple binding requires a tuple value, found '${value.type}'`,
        statement.value.span,
      );
    }
    if (elements.length !== statement.bindings.length) {
      this.fail(
        "type-mismatch",
        `tuple binding has ${statement.bindings.length} names for ${elements.length} elements`,
        statement.span,
      );
    }
    const names = new Set<string>();
    for (const binding of statement.bindings) {
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
    // Each name of a multi-name binding infers its own element's access
    // (04-type-system.md#r-types.bind.let-mut-pattern).
    const bindingTypes = statement.bindings.map((binding, index) => {
      const element = elements[index]!;
      if (binding.mutableAccess) {
        const annotated = annotatedElements?.[index];
        if (annotated !== undefined) this.requireMutableAnnotation(annotated, binding);
        else this.requireMutableValue(element, binding.span);
        return element;
      }
      return annotation ? element : readonlyType(element);
    });
    for (const [index, binding] of statement.bindings.entries()) {
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
  /** `let mut` needs an annotation whose root is `mut` (04-type-system.md#r-types.bind.let-mut-annotation). */
  private requireMutableAnnotation(
    annotation: ValueType,
    site: { readonly span: SourceSpan },
  ): void {
    if (mutableInner(annotation) !== undefined) return;
    this.fail(
      "let-mut-readonly-type",
      `'let mut' asks for mutable access, but the type '${annotation}' is readonly; write 'mut ${annotation}', or drop 'mut' after 'let'`,
      site.span,
    );
  }

  /** `let mut` never upgrades a readonly value (04-type-system.md#r-types.bind.let-mut-upgrade). */
  private requireMutableValue(type: ValueType, span: SourceSpan): void {
    if (mutableInner(type) !== undefined || type === "never") return;
    this.fail(
      "mutable-upgrade",
      `'let mut' needs a value with mutable access, but '${type}' is readonly and cannot be upgraded; copy it into a fresh value instead`,
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
    if (letMut && annotation !== undefined) this.requireMutableAnnotation(annotation, statement);
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
        statement.localFunction
          ? "recursive-function-needs-result-type"
          : "recursive-closure-needs-result-type",
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
    this.pendingRecursiveClosure = recursiveLocal;
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
    }
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
    if (type === "void")
      this.fail("void-binding", "a binding cannot store a void value", statement.span);
    value = this.requireCoercion(value, type, statement.value.span);
    if (this.moduleBody) {
      const global: HirGlobal = recursiveGlobal ?? {
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
    return { kind: "binding", local, value, span: statement.span };
  }
}
