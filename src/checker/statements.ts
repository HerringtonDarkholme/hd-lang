import type { Expression, Statement } from "../ast.ts";
import type { HirExpression, HirGlobal, HirLocal, HirStatement, ValueType } from "../hir.ts";
import {
  functionType,
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import { CheckerContext, PRELUDE_NAMES } from "./context.ts";
import { genericTypeName, substituteGenericType, statementsReferenceName } from "./shared.ts";

export abstract class StatementChecker extends CheckerContext {
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
        const local = this.resolveLocal(statement.name);
        const global = local ? undefined : this.resolveGlobal(statement.name);
        if (!local && !global && this.globals.has(statement.name)) {
          this.fail(
            "binding-not-yet-visible",
            `module binding '${statement.name}' is not visible before its binding point`,
            statement.span,
          );
        }
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
      case "field-assignment": {
        const receiver = this.checkExpression(statement.target.receiver);
        const mutableReceiver = mutableInner(receiver.type);
        if (mutableReceiver === undefined) {
          let root: Expression = statement.target.receiver;
          while (root.kind === "member") root = root.receiver;
          const rootBinding =
            root.kind === "name"
              ? (this.resolveLocal(root.name) ?? this.resolveGlobal(root.name))
              : undefined;
          const code =
            rootBinding && mutableInner(rootBinding.type) !== undefined
              ? "readonly-edge"
              : "readonly-root";
          this.fail(
            code,
            `field '${statement.target.name}' cannot be assigned through readonly type '${receiver.type}'`,
            statement.target.span,
          );
        }
        const nominal = nominalGenericParts(mutableReceiver);
        const declaration = this.dataTypes.get(nominal?.name ?? mutableReceiver);
        if (!declaration)
          this.fail(
            "member-on-non-data",
            `type '${mutableReceiver}' has no assignable data fields`,
            statement.target.receiver.span,
          );
        const field = declaration.fields.find(
          (candidate) => candidate.name === statement.target.name,
        );
        if (!field)
          this.fail(
            "unknown-data-field",
            `type '${declaration.name}' has no field '${statement.target.name}'`,
            statement.target.span,
          );
        const substitutions = new Map<string, ValueType>();
        if (nominal)
          declaration.genericParameters.forEach((parameter, index) =>
            substitutions.set(parameter, nominal.arguments[index]!),
          );
        const fieldType = substituteGenericType(field.type, substitutions);
        const value = this.requireCoercion(
          this.checkExpression(statement.value, fieldType),
          fieldType,
          statement.value.span,
        );
        const expression: HirExpression = {
          kind: "field-set",
          receiver,
          value,
          dataIndex: declaration.index,
          fieldIndex: field.index,
          erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
          type: "void",
          span: statement.span,
        };
        return { kind: "expression", expression, span: statement.span };
      }
      case "index-assignment": {
        const receiver = this.checkExpression(statement.target.receiver);
        const mutableReceiver = mutableInner(receiver.type);
        if (mutableReceiver === undefined) {
          this.fail(
            "readonly-root",
            `indexed assignment requires mutable access to '${receiver.type}'`,
            statement.target.receiver.span,
          );
        }
        const nominal = nominalGenericParts(mutableReceiver);
        if (nominal?.name === "list" && nominal.arguments.length === 1) {
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
        if (nominal?.name === "map" && nominal.arguments.length === 2) {
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
        this.fail(
          "not-indexable",
          `type '${receiver.type}' does not support indexed assignment`,
          statement.target.receiver.span,
        );
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
        "tuple-binding-requires-tuple",
        `tuple binding requires a tuple value, found '${value.type}'`,
        statement.value.span,
      );
    }
    if (elements.length !== statement.bindings.length) {
      this.fail(
        "tuple-binding-arity",
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
    for (const [index, binding] of statement.bindings.entries()) {
      if (this.moduleBody) {
        const global: HirGlobal = {
          name: binding.name,
          type: elements[index]!,
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
        type: elements[index]!,
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
        "recursive-closure-needs-result-type",
        `recursive closure '${statement.name}' needs an explicit result type`,
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
        storedSuspension?.result ?? annotation ?? recursiveLocal?.type ?? recursiveGlobal?.type,
      );
    } finally {
      this.pendingRecursiveClosure = previousRecursiveClosure;
    }
    if (!statement.mutable && !annotation) {
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
