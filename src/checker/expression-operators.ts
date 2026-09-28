import type { Expression } from "../ast.ts";
import type { HirExpression, HirLocal, ValueType } from "../hir.ts";
import {
  functionType,
  functionParts,
  mutableInner,
  nominalGenericParts,
  readonlyType,
  tupleParts,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import type { Signature } from "./context.ts";
import {
  genericTypeName,
  inferGenericType,
  substituteGenericType,
  traitTypeName,
} from "./shared.ts";

import {
  ExpressionLiteralChecker,
  integerLiteralTarget,
  integerTarget,
} from "./expression-literals.ts";
type NameExpression = Extract<Expression, { kind: "name" }>;

export abstract class ExpressionOperatorChecker extends ExpressionLiteralChecker {
  /**
   * A generic function used as a value (07-functions.md#function-types-and-values):
   * every generic parameter comes from a complete explicit type-argument list,
   * where `_` asks for inference, or from the expected monomorphic function
   * type. The value is a closure over a call of the function, so its
   * dictionaries and boxing are those of an ordinary call. Returns undefined
   * when some parameter stays unknown.
   */
  private instantiateFunctionValue(
    expression: NameExpression,
    signature: Signature,
    expected: ValueType | undefined,
  ): HirExpression | undefined {
    if (signature.rowParameters.length > 0 || signature.variadic) return undefined;
    const substitutions = new Map<string, ValueType>();
    const typeArguments = expression.typeArguments;
    if (typeArguments) {
      if (typeArguments.length !== signature.genericParameters.length)
        this.fail(
          typeArguments.length < signature.genericParameters.length
            ? "partial-generic-arguments"
            : "generic-argument-count",
          `function '${signature.name}' expects ${signature.genericParameters.length} type arguments, received ${typeArguments.length}`,
          expression.span,
        );
      typeArguments.forEach((argument, index) => {
        if (argument.name !== "_")
          substitutions.set(signature.genericParameters[index]!, this.resolveType(argument));
      });
    }
    // An argument's expected type may still hold the call's unsolved
    // parameters; only its solved positions instantiate the value.
    const pending = this.takePendingCallGenerics();
    const callable = expected ? functionParts(expected) : undefined;
    if (callable && callable.parameters.length === signature.parameters.length) {
      signature.parameters.forEach((parameter, index) => {
        const actual = callable.parameters[index]!;
        if (!pending(actual)) inferGenericType(parameter, actual, substitutions);
      });
      if (!pending(callable.result))
        inferGenericType(signature.result, callable.result, substitutions);
    }
    if (!signature.genericParameters.every((parameter) => substitutions.has(parameter)))
      return undefined;
    const type = functionType(
      signature.parameters.map((parameter) => substituteGenericType(parameter, substitutions)),
      substituteGenericType(signature.result, substitutions),
      signature.requirements,
      false,
      signature.suspending,
    );
    const span = expression.span;
    const parameters = signature.parameters.map((_, index) => ({ name: `$value${index}`, span }));
    const call: Expression = {
      kind: signature.suspending ? "suspend-call" : "call",
      callee: { kind: "name", name: expression.name, typeArguments, span },
      arguments: parameters.map((parameter) => ({ kind: "name", name: parameter.name, span })),
      span,
    };
    const closure: Expression = {
      kind: "closure",
      ...(signature.suspending ? { suspending: true } : {}),
      parameters,
      body: [{ kind: "expression", expression: call, span }],
      span,
    };
    return this.checkExpression(closure, type);
  }

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
        const signature = this.visibleSignature(expression.name);
        if (signature) {
          if (signature.genericParameters.length > 0 || signature.rowParameters.length > 0) {
            const instantiated = this.instantiateFunctionValue(expression, signature, _expected);
            if (instantiated) return instantiated;
            this.fail(
              "unresolved-generic-placeholder",
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
        this.failUnknownName(expression.name, `unknown name '${expression.name}'`, expression.span);
      }
      case "unary":
        return this.checkUnaryExpression(expression, _expected);
      case "binary": {
        const { left, right } = this.checkNumericOperands(expression, _expected);
        if (expression.operator === "is") {
          // Function identity is unspecified, so a direct `is` on a function
          // value is rejected (05-expressions.md#r-expr.is.function).
          const functionOperand = [left, right].find(
            (operand) => functionParts(readonlyType(operand.type)) !== undefined,
          );
          if (functionOperand)
            this.fail(
              "unsupported-function-identity",
              `identity of function value of type '${functionOperand.type}' is unspecified`,
              expression.span,
            );
          const operands = this.identityOperands(left, right);
          if (!operands) {
            this.fail(
              this.isIdentityType(withoutPermissions(left.type)) &&
                this.isIdentityType(withoutPermissions(right.type))
                ? "incompatible-identity-operands"
                : "type-mismatch",
              `identity operands have types ${left.type} and ${right.type}`,
              expression.span,
            );
          }
          const identityType = withoutPermissions(operands[0].type);
          const generic = genericTypeName(identityType);
          if (generic && !(this.signature.referenceParameters ?? []).includes(generic)) {
            this.fail(
              "identity-needs-reference-bound",
              `generic parameter '${generic}' requires an AnyRef bound for identity comparison`,
              expression.span,
            );
          }
          if (!this.isIdentityType(identityType)) {
            this.fail(
              "identity-requires-references",
              `identity comparison does not accept '${left.type}'`,
              expression.span,
            );
          }
          return {
            kind: "binary",
            operator: expression.operator,
            left: operands[0],
            right: operands[1],
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
        // An integer exponent must be unsigned. This prototype has no unsigned
        // types, so only an exponent built from unsuffixed literals (typed u32
        // in exponent position) is accepted; it is represented as i32.
        if (
          expression.operator === "**" &&
          (left.type === "i32" || left.type === "i64") &&
          right.type === "i32" &&
          !isLiteralExponent(expression.right)
        )
          this.fail(
            "type-mismatch",
            `an integer exponent must have an unsigned integer type, found '${right.type}'`,
            expression.right.span,
          );
        const integerPower =
          expression.operator === "**" && left.type === "i64" && right.type === "i32";
        if (left.type !== right.type && !integerPower) {
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
              `type '${left.type}' does not implement Eq`,
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
        // The prototype shifts only i32 values.
        if (
          bitwise &&
          left.type !== "i32" &&
          !(integerTarget(left.type) && ["&", "|", "^"].includes(expression.operator))
        )
          this.fail(
            "type-mismatch",
            `operator '${expression.operator}' requires i32 operands`,
            expression.span,
          );
        if (remainder && left.type !== "i32" && !integerTarget(left.type))
          this.fail("type-mismatch", "operator '%' requires integer operands", expression.span);
        if (
          comparison &&
          left.type === "bool" &&
          expression.operator !== "==" &&
          expression.operator !== "!="
        ) {
          this.fail(
            "missing-partial-ord",
            `type 'bool' does not implement PartialOrd, required by operator '${expression.operator}'`,
            expression.span,
          );
        }
        if (
          !bitwise &&
          !remainder &&
          !stringConcatenation &&
          left.type !== "i32" &&
          !integerTarget(left.type) &&
          left.type !== "f64" &&
          !(comparison && (left.type === "bool" || left.type === "char" || left.type === "string"))
        ) {
          this.fail(
            "type-mismatch",
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

  /** A unary expression; `_expected` passes through `-`, `+`, and `~` to a literal. */
  private checkUnaryExpression(
    expression: Extract<Expression, { kind: "unary" }>,
    _expected: ValueType | undefined,
  ): HirExpression {
    const literalTarget = integerLiteralTarget(_expected);
    // A negated literal is range-checked as a unit (04 Negated Integer Literals).
    if (
      expression.operator === "-" &&
      expression.operand.kind === "integer" &&
      (literalTarget === "i64" || expression.operand.value === 2_147_483_648n)
    )
      return this.integerLiteral(-expression.operand.value, _expected, expression.span);
    const operand = this.checkExpression(
      expression.operand,
      expression.operator === "-" || expression.operator === "+" || expression.operator === "~"
        ? literalTarget
        : undefined,
    );
    let type: ValueType;
    if (expression.operator === "not") {
      this.requireType(operand.type, "bool", expression.operand.span);
      type = "bool";
    } else if (expression.operator === "~") {
      if (operand.type !== "i64") this.requireType(operand.type, "i32", expression.operand.span);
      type = operand.type;
    } else {
      // 05 Arithmetic: unary `-` does not accept an unsigned integer.
      if (expression.operator === "-" && operand.type === "u8")
        this.fail(
          "unsigned-negation",
          "unary '-' does not accept the unsigned type 'u8'",
          expression.span,
        );
      if (operand.type !== "i32" && !integerTarget(operand.type) && operand.type !== "f64") {
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

  /**
   * 04 Binary Numeric Operators: an untyped integer literal adopts `i64` or
   * `u8` from the other operand (or from an expected result of arithmetic), and
   * otherwise an `i32` operand widens to an `i64` one. An exponent keeps its
   * own type.
   */
  private checkNumericOperands(
    expression: Extract<Expression, { kind: "binary" }>,
    expected: ValueType | undefined,
  ): { left: HirExpression; right: HirExpression } {
    const arithmetic = ["+", "-", "*", "/", "%", "&", "|", "^"].includes(expression.operator);
    const numeric =
      arithmetic || ["==", "!=", "<", "<=", ">", ">=", "**"].includes(expression.operator);
    const outer = arithmetic ? integerLiteralTarget(expected) : undefined;
    const leftLiteral = isIntegerLiteral(expression.left);
    const rightLiteral = isIntegerLiteral(expression.right);
    let left = this.checkExpression(expression.left, leftLiteral ? outer : undefined);
    const rightTarget =
      rightLiteral && expression.operator !== "**"
        ? (integerTarget(left.type) ?? outer)
        : undefined;
    let right = this.checkExpression(expression.right, rightTarget);
    if (!numeric || expression.operator === "**") return { left, right };
    if (leftLiteral && left.type === "i32" && integerTarget(right.type))
      left = this.checkExpression(expression.left, right.type);
    const widen = (value: HirExpression): HirExpression =>
      value.type === "i32" ? this.coerce(value, "i64", value.span) : value;
    if (left.type === "i64" && right.type === "i32") right = widen(right);
    else if (left.type === "i32" && right.type === "i64") left = widen(left);
    return { left, right };
  }

  /** The operands of `is` made comparable, or undefined when they are incompatible. */
  private identityOperands(
    left: HirExpression,
    right: HirExpression,
  ): readonly [HirExpression, HirExpression] | undefined {
    // The emitter compares trait values through their readonly trait type.
    const readonlyTrait = (value: HirExpression): HirExpression =>
      traitTypeName(value.type) ? this.coerce(value, readonlyType(value.type), value.span) : value;
    if (withoutPermissions(left.type) === withoutPermissions(right.type))
      return [readonlyTrait(left), readonlyTrait(right)];
    for (const [trait, other] of [
      [left, right],
      [right, left],
    ] as const) {
      if (!traitTypeName(trait.type)) continue;
      const target = readonlyType(trait.type);
      const converted = this.coerce(other, target, other.span);
      if (converted.type !== target) continue;
      const traitValue = readonlyTrait(trait);
      return trait === left ? [traitValue, converted] : [converted, traitValue];
    }
    return undefined;
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
        "type-mismatch",
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

const LITERAL_EXPONENT_OPERATORS = new Set(["+", "*", "**"]);

/** True for an exponent whose leaves are integer literals, which take type u32. */
/** An unsuffixed integer literal, possibly negated. */
function isIntegerLiteral(expression: Expression): boolean {
  if (expression.kind === "integer") return true;
  return (
    expression.kind === "unary" &&
    expression.operator === "-" &&
    expression.operand.kind === "integer"
  );
}

function isLiteralExponent(expression: Expression): boolean {
  if (expression.kind === "integer") return true;
  return (
    expression.kind === "binary" &&
    LITERAL_EXPONENT_OPERATORS.has(expression.operator) &&
    isLiteralExponent(expression.left) &&
    isLiteralExponent(expression.right)
  );
}

/** The type with `mut` removed at every level; permissions never affect identity. */
function withoutPermissions(type: ValueType): ValueType {
  return type.replaceAll("mut:", "");
}
