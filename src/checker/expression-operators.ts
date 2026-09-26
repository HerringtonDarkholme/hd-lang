import type { Expression } from "../ast.ts";
import type { HirExpression, HirLocal, ValueType } from "../hir.ts";
import {
  functionType,
  functionParts,
  mutableInner,
  nominalGenericParts,
  tupleParts,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import { genericTypeName } from "./shared.ts";

import { ExpressionLiteralChecker } from "./expression-literals.ts";
export abstract class ExpressionOperatorChecker extends ExpressionLiteralChecker {
  protected checkOperatorExpression(
    expression: Expression,
    _expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "binding-expression":
        return this.checkBindingExpression(expression);
      case "name": {
        const local = this.resolveLocal(expression.name);
        if (local) {
          if (
            this.unavailableBindingLocals.has(local.index) &&
            !this.allowedConditionalBindingLocals.has(local.index)
          )
            this.fail(
              "possibly-uninitialized-binding",
              `binding '${local.name}' may not have been initialized on this path`,
              expression.span,
            );
          return { kind: "local", local, type: local.type, span: expression.span };
        }
        const source = this.availableCaptures.get(expression.name);
        if (source) {
          if (source === this.selfClosureLocal) {
            return {
              kind: "closure-self",
              closureIndex: this.closureIndex,
              type: source.type,
              span: expression.span,
            };
          }
          return this.captureReference(expression.name, source, expression.span);
        }
        const global = this.resolveGlobal(expression.name);
        if (global) return { kind: "global", global, type: global.type, span: expression.span };
        if (this.globals.has(expression.name)) {
          this.fail(
            "binding-not-yet-visible",
            `module binding '${expression.name}' is not visible before its binding point`,
            expression.span,
          );
        }
        const signature = this.signatures.get(expression.name);
        if (signature) {
          if (signature.genericParameters.length > 0 || signature.rowParameters.length > 0) {
            this.fail(
              "generic-function-value-needs-arguments",
              `generic function '${signature.name}' needs inferred or explicit type arguments before it can be used as a value`,
              expression.span,
            );
          }
          return {
            kind: "function-value",
            functionIndex: signature.index,
            functionName: signature.name,
            type: functionType(
              signature.parameters,
              signature.result,
              signature.requirements,
              signature.variadic,
              signature.suspending,
            ),
            span: expression.span,
          };
        }
        this.fail("unknown-name", `unknown name '${expression.name}'`, expression.span);
      }
      case "unary": {
        if (
          expression.operator === "-" &&
          expression.operand.kind === "integer" &&
          expression.operand.value === 2_147_483_648n
        ) {
          return { kind: "integer", value: -2_147_483_648, type: "i32", span: expression.span };
        }
        const operand = this.checkExpression(expression.operand);
        let type: ValueType;
        if (expression.operator === "not") {
          this.requireType(operand.type, "bool", expression.operand.span);
          type = "bool";
        } else if (expression.operator === "~") {
          this.requireType(operand.type, "i32", expression.operand.span);
          type = "i32";
        } else {
          if (operand.type !== "i32" && operand.type !== "f64") {
            const code =
              expression.operator === "+" ? "nonnumeric-unary-plus" : "invalid-unary-operand";
            this.fail(
              code,
              `operator '${expression.operator}' requires a numeric operand`,
              expression.span,
            );
          }
          type = operand.type;
        }
        return {
          kind: "unary",
          operator: expression.operator,
          operand,
          type,
          span: expression.span,
        };
      }
      case "binary": {
        const left = this.checkExpression(expression.left);
        const right = this.checkExpression(expression.right);
        if (expression.operator === "is") {
          if (left.type !== right.type) {
            this.fail(
              "type-mismatch",
              `identity operands have types ${left.type} and ${right.type}`,
              expression.span,
            );
          }
          const generic = genericTypeName(left.type);
          if (generic && !(this.signature.referenceParameters ?? []).includes(generic)) {
            this.fail(
              "identity-needs-reference-bound",
              `generic parameter '${generic}' requires a Reference bound for identity comparison`,
              expression.span,
            );
          }
          if (!this.isIdentityType(left.type)) {
            this.fail(
              "identity-requires-references",
              `identity comparison does not accept '${left.type}'`,
              expression.span,
            );
          }
          return {
            kind: "binary",
            operator: expression.operator,
            left,
            right,
            type: "bool",
            span: expression.span,
          };
        }
        const logical = expression.operator === "and" || expression.operator === "or";
        const comparison = ["==", "!=", "<", "<=", ">", ">="].includes(expression.operator);
        const equality = expression.operator === "==" || expression.operator === "!=";
        const bitwise = ["&", "|", "^", "<<", ">>"].includes(expression.operator);
        const remainder = expression.operator === "%";
        const stringConcatenation = expression.operator === "+" && left.type === "string";
        if (logical) {
          this.requireType(left.type, "bool", left.span);
          this.requireType(right.type, "bool", right.span);
          return {
            kind: "binary",
            operator: expression.operator,
            left,
            right,
            type: "bool",
            span: expression.span,
          };
        }
        if (left.type !== right.type) {
          if (expression.operator === "**")
            this.fail(
              "mixed-numeric-types",
              "integer and floating-point power operands cannot be mixed",
              expression.span,
            );
          this.fail(
            "type-mismatch",
            `operator operands have types ${left.type} and ${right.type}`,
            expression.span,
          );
        }
        if (comparison && functionParts(left.type)) {
          this.fail(
            "unsupported-equality",
            `function values do not support operator '${expression.operator}'`,
            expression.span,
          );
        }
        if (equality) {
          const compared = this.equalityExpression(left, right, expression.span);
          if (compared) {
            return expression.operator === "=="
              ? compared
              : {
                  kind: "unary",
                  operator: "not",
                  operand: compared,
                  type: "bool",
                  span: expression.span,
                };
          }
          if (
            genericTypeName(left.type) ||
            this.dataTypes.has(nominalGenericParts(left.type)?.name ?? left.type) ||
            this.enumTypes.has(nominalGenericParts(left.type)?.name ?? left.type)
          ) {
            this.fail(
              "missing-partial-eq",
              `type '${left.type}' does not implement PartialEq`,
              expression.span,
            );
          }
        }
        if (comparison && !equality) {
          const strategy = this.orderingStrategy(left.type);
          if (strategy)
            return {
              kind: "value-ordering",
              left,
              right,
              valueType: left.type,
              strategy,
              operator: expression.operator as "<" | "<=" | ">" | ">=",
              type: "bool",
              span: expression.span,
            };
          if (
            this.dataTypes.has(nominalGenericParts(left.type)?.name ?? left.type) ||
            this.enumTypes.has(nominalGenericParts(left.type)?.name ?? left.type) ||
            genericTypeName(left.type)
          )
            this.fail(
              "missing-partial-ord",
              `type '${left.type}' does not implement PartialOrd`,
              expression.span,
            );
        }
        if (bitwise && left.type !== "i32")
          this.fail(
            "invalid-binary-operands",
            `operator '${expression.operator}' requires i32 operands`,
            expression.span,
          );
        if (remainder && left.type !== "i32")
          this.fail(
            "invalid-binary-operands",
            "operator '%' requires integer operands",
            expression.span,
          );
        if (
          comparison &&
          left.type === "bool" &&
          expression.operator !== "==" &&
          expression.operator !== "!="
        ) {
          this.fail(
            "invalid-binary-operands",
            `operator '${expression.operator}' does not accept bool`,
            expression.span,
          );
        }
        if (
          !bitwise &&
          !remainder &&
          !stringConcatenation &&
          left.type !== "i32" &&
          left.type !== "f64" &&
          !(comparison && (left.type === "bool" || left.type === "char" || left.type === "string"))
        ) {
          this.fail(
            "invalid-binary-operands",
            `operator '${expression.operator}' does not accept ${left.type}`,
            expression.span,
          );
        }
        return {
          kind: "binary",
          operator: expression.operator,
          left,
          right,
          type: comparison ? "bool" : left.type,
          span: expression.span,
        };
      }
      default:
        return undefined;
    }
  }

  private checkBindingExpression(
    expression: Extract<Expression, { kind: "binding-expression" }>,
  ): HirExpression {
    const value = this.checkExpression(expression.value);
    if (value.type === "never")
      this.fail(
        "uninhabited-binding",
        "an inferred binding cannot have type never",
        expression.span,
      );
    if (value.type === "void")
      this.fail("void-binding", "a binding cannot store a void value", expression.span);
    const elementTypes = expression.bindings.length === 1 ? undefined : tupleParts(value.type);
    if (expression.bindings.length > 1 && elementTypes?.length !== expression.bindings.length)
      this.fail(
        "tuple-binding-arity",
        `binding has ${expression.bindings.length} names but '${value.type}' has ${elementTypes?.length ?? 1} element${elementTypes?.length === 1 ? "" : "s"}`,
        expression.span,
      );
    const seen = new Set<string>();
    const bindings = expression.bindings.map((binding, index) => {
      if (seen.has(binding.name) || this.currentScope().has(binding.name))
        this.fail(
          "duplicate-binding",
          `binding '${binding.name}' already exists in this scope`,
          binding.span,
        );
      seen.add(binding.name);
      if (PRELUDE_NAMES.has(binding.name))
        this.fail(
          "prelude-name-shadow",
          `binding expression '${binding.name}' shadows a prelude name`,
          binding.span,
        );
      const sourceType = elementTypes?.[index] ?? value.type;
      const local: HirLocal = {
        name: binding.name,
        type: mutableInner(sourceType) ?? sourceType,
        index: this.locals.length,
        mutable: false,
        parameter: false,
        span: binding.span,
      };
      this.locals.push(local);
      this.currentScope().set(binding.name, local);
      return local;
    });
    return {
      kind: "binding-expression",
      bindings,
      value,
      elementTypes,
      type: value.type,
      span: expression.span,
    };
  }
}
