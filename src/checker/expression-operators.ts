import type { SourceSpan } from "../diagnostics.ts";
import type { Expression } from "../ast.ts";
import type { HirData, HirExpression, HirLocal, ValueType } from "../hir.ts";
import {
  functionType,
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  tupleParts,
} from "../types.ts";
import { isIntegerType, numericType, widerNumeric } from "../numeric.ts";
import { isKnownType, PRELUDE_NAMES } from "./context.ts";
import type { Signature } from "./context.ts";
import { ALL_COMBINATOR } from "./standard-traits.ts";
import { FACTS_OF_INTRINSIC } from "./function-facts.ts";
import {
  containsGenericType,
  genericTypeName,
  inferGenericType,
  substituteGenericType,
  traitTypeName,
} from "./shared.ts";

import {
  ExpressionLiteralChecker,
  floatLiteralTarget,
  integerLiteralTarget,
  integerTarget,
} from "./expression-literals.ts";
type NameExpression = Extract<Expression, { kind: "name" }>;

/** The `std.ops` trait and method of each overloadable binary operator (05-expressions.md#operator-traits). */
const BINARY_OPERATOR_TRAITS: Readonly<Record<string, readonly [string, string]>> = {
  "+": ["Add", "add"],
  "-": ["Sub", "sub"],
  "*": ["Mul", "mul"],
  "/": ["Div", "div"],
  "%": ["Rem", "rem"],
  "&": ["BitAnd", "bit_and"],
  "|": ["BitOr", "bit_or"],
  "^": ["BitXor", "bit_xor"],
  "<<": ["Shl", "shl"],
  ">>": ["Shr", "shr"],
};

/** The `std.ops` trait and method of each overloadable unary operator. */
const UNARY_OPERATOR_TRAITS: Readonly<Record<string, readonly [string, string]>> = {
  "-": ["Neg", "neg"],
  "~": ["Not", "not"],
};

/** A primitive operand type, on which an operator never searches a trait (r-expr.op.primitive.typing). */
function isPrimitiveOperand(type: ValueType): boolean {
  const readonly = readonlyType(type);
  return (
    numericType(readonly) !== undefined ||
    ["bool", "char", "string", "never", "void"].includes(readonly)
  );
}

export abstract class ExpressionOperatorChecker extends ExpressionLiteralChecker {
  /**
   * The operator-trait call `Op::[R]::m(receiver, argument)` of a non-primitive
   * operand, or `Op::m(receiver)` for a unary operator
   * (05-expressions.md#r-expr.op.desugar).
   */
  protected abstract operatorTraitCall(
    trait: readonly [string, string],
    operator: string,
    receiverSource: Expression,
    receiver: HirExpression,
    argument: Expression | undefined,
    span: SourceSpan,
    expected: ValueType | undefined,
  ): HirExpression;

  /** `left.eq(right)` through a generic `Eq` implementation, or undefined. */

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
      // Omitted trailing slots are inferred and then defaulted (types.generic.short-list).
      if (typeArguments.length > signature.genericParameters.length)
        this.fail(
          "argument-count",
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
    // A generic function value takes the defaults of what nothing solved
    // (07-functions.md#r-fn.type.generic.default).
    this.applyGenericDefaults(signature, substitutions, new Map());
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

  /**
   * A type, trait, or type parameter name where a value is required
   * (spec/lang/03-names-and-scopes.md#r-names.type-as-value).
   */
  private rejectTypeAsValue(name: string, span: SourceSpan): void {
    if (
      isKnownType(name, this.dataTypes, this.enumTypes, this.traitTypes) ||
      this.traitTypes.has(name) ||
      this.signature.genericParameters.includes(name)
    )
      this.fail("type-used-as-value", `'${name}' names a type, not a value`, span);
  }

  protected checkOperatorExpression(
    expression: Expression,
    _expected?: ValueType,
  ): HirExpression | undefined {
    switch (expression.kind) {
      case "binding-expression":
        return this.checkBindingExpression(expression);
      case "range":
        return this.checkRangeExpression(expression, _expected);
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
        // `_` names a value only in a pipe step (05-expressions.md#r-expr.pipe.placeholder-outside).
        if (expression.name === "_")
          this.fail(
            "placeholder-outside-pipe",
            "'_' has a value only in a pipe step; write a closure such as 'fn(value): f(value, a)'",
            expression.span,
          );
        if (this.globals.has(expression.name)) {
          this.fail(
            "binding-not-yet-visible",
            `module binding '${expression.name}' is not visible before its binding point`,
            expression.span,
          );
        }
        const signature = this.visibleSignature(expression.name);
        // `facts_of` is only a callee (annot.facts-of.target.error).
        if (signature?.intrinsic === FACTS_OF_INTRINSIC)
          this.fail(
            "invalid-facts-of-target",
            `'${expression.name}' is a compiler intrinsic and must be called directly`,
            expression.span,
          );
        if (signature) {
          if (signature.genericParameters.length > 0 || signature.rowParameters.length > 0) {
            const instantiated = this.instantiateFunctionValue(expression, signature, _expected);
            if (instantiated) return instantiated;
            // A known function type that no type arguments fit is a mismatch.
            if (
              _expected !== undefined &&
              functionParts(_expected) &&
              !containsGenericType(_expected)
            )
              this.fail(
                "type-mismatch",
                `generic function '${signature.name}' does not fit the expected type '${_expected}'`,
                expression.span,
              );
            this.fail(
              "cannot-infer-type",
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
        this.rejectTypeAsValue(expression.name, expression.span);
        if (this.imports.get(expression.name) === ALL_COMBINATOR)
          this.fail(
            "type-mismatch",
            "all is not a function value: it must be the callee of a direct all!(...) call",
            expression.span,
          );
        this.failUnknownName(expression.name, `unknown name '${expression.name}'`, expression.span);
      }
      case "unary":
        return this.checkUnaryExpression(expression, _expected);
      case "binary": {
        // A non-primitive left operand calls its operator trait; an untyped
        // literal on the left keeps its default type (r-expr.op.left-literal).
        const operatorTrait = BINARY_OPERATOR_TRAITS[expression.operator];
        let checkedLeft: HirExpression | undefined;
        if (
          operatorTrait &&
          !isIntegerLiteral(expression.left) &&
          !isFloatLiteral(expression.left)
        ) {
          checkedLeft = this.checkExpression(expression.left);
          if (!isPrimitiveOperand(checkedLeft.type))
            return this.operatorTraitCall(
              operatorTrait,
              expression.operator,
              expression.left,
              checkedLeft,
              expression.right,
              expression.span,
              _expected,
            );
        }
        const { left, right } = this.checkNumericOperands(expression, _expected, checkedLeft);
        if (expression.operator === "is")
          return this.checkIdentityExpression(expression, left, right);
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
        // An integer exponent must be unsigned. An exponent built from
        // unsuffixed literals is typed u32 in exponent position; the prototype
        // represents it as i32.
        const unsignedExponent =
          numericType(right.type)?.family === "unsigned" ||
          (right.type === "i32" && isLiteralExponent(expression.right));
        if (
          expression.operator === "**" &&
          isIntegerType(left.type) &&
          isIntegerType(right.type) &&
          !unsignedExponent
        )
          this.fail(
            "type-mismatch",
            `an integer exponent must have an unsigned integer type, found '${right.type}'`,
            expression.right.span,
          );
        const integerPower =
          expression.operator === "**" && isIntegerType(left.type) && unsignedExponent;
        // A shift's count may have any integer type (05-expressions.md#shifts).
        const integerShift =
          (expression.operator === "<<" || expression.operator === ">>") &&
          isIntegerType(left.type) &&
          isIntegerType(right.type);
        // 04 Binary Numeric Operators: signed and unsigned integers do not mix.
        if (
          left.type !== right.type &&
          !integerPower &&
          !integerShift &&
          isIntegerType(left.type) &&
          isIntegerType(right.type) &&
          numericType(left.type)!.family !== numericType(right.type)!.family
        )
          this.fail(
            "mixed-signedness",
            `signed and unsigned operands do not mix: ${left.type} and ${right.type}; cast one explicitly`,
            expression.span,
          );
        if (left.type !== right.type && !integerPower && !integerShift) {
          if (expression.operator === "**" && numericType(left.type) && numericType(right.type))
            this.fail(
              "mixed-numeric-types",
              "integer and floating-point power operands cannot be mixed",
              expression.span,
            );
          const trait = BINARY_OPERATOR_TRAITS[expression.operator];
          this.fail(
            "type-mismatch",
            trait && !isPrimitiveOperand(right.type)
              ? `operator '${expression.operator}' needs an implementation of std.ops.${trait[0]}[${readonlyType(right.type)}] for '${left.type}'`
              : `operator operands have types ${left.type} and ${right.type}`,
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
            this.fail("missing-eq", `type '${left.type}' does not implement Eq`, expression.span);
          }
        }
        if (comparison && !equality) {
          const strategy = this.orderingStrategy(left.type, expression.span);
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
        if (bitwise && !isIntegerType(left.type))
          this.fail(
            "type-mismatch",
            `operator '${expression.operator}' requires integer operands`,
            expression.span,
          );
        // Floating `%` truncates, as C `fmod` does (05-expressions.md#r-expr.float.remainder-truncated).
        if (remainder && !numericType(left.type))
          this.fail("type-mismatch", "operator '%' requires numeric operands", expression.span);
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
          !numericType(left.type) &&
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

  /** `left is right` (05-expressions.md#identity). */
  private checkIdentityExpression(
    expression: Extract<Expression, { kind: "binary" }>,
    left: HirExpression,
    right: HirExpression,
  ): HirExpression {
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
      (numericType(literalTarget)?.family === "signed" ||
        expression.operand.value === 2_147_483_648n)
    )
      return this.integerLiteral(-expression.operand.value, _expected, expression.span);
    const operand = this.checkExpression(
      expression.operand,
      expression.operator === "-" || expression.operator === "+" || expression.operator === "~"
        ? (literalTarget ?? floatLiteralTarget(_expected))
        : undefined,
    );
    // `-a` is `Neg::neg(a)` and `~a` is `Not::not(a)` on a
    // non-primitive operand (r-expr.op.desugar).
    const operatorTrait = UNARY_OPERATOR_TRAITS[expression.operator];
    if (operatorTrait && !isPrimitiveOperand(operand.type))
      return this.operatorTraitCall(
        operatorTrait,
        expression.operator,
        expression.operand,
        operand,
        undefined,
        expression.span,
        _expected,
      );
    let type: ValueType;
    if (expression.operator === "not") {
      this.requireType(operand.type, "bool", expression.operand.span);
      type = "bool";
    } else if (expression.operator === "~") {
      if (!isIntegerType(operand.type))
        this.requireType(operand.type, "i32", expression.operand.span);
      type = operand.type;
    } else {
      // 05 Arithmetic: unary `-` does not accept an unsigned integer.
      if (expression.operator === "-" && numericType(operand.type)?.family === "unsigned")
        this.fail(
          "unsigned-negation",
          `unary '-' does not accept the unsigned type '${operand.type}'`,
          expression.span,
        );
      if (!numericType(operand.type)) {
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
    checkedLeft?: HirExpression,
  ): { left: HirExpression; right: HirExpression } {
    const arithmetic = ["+", "-", "*", "/", "%", "&", "|", "^"].includes(expression.operator);
    const numeric =
      arithmetic || ["==", "!=", "<", "<=", ">", ">=", "**"].includes(expression.operator);
    const outer = arithmetic ? integerLiteralTarget(expected) : undefined;
    const outerFloat = arithmetic ? floatLiteralTarget(expected) : undefined;
    const leftLiteral = isIntegerLiteral(expression.left);
    const rightLiteral = isIntegerLiteral(expression.right);
    const leftFloat = isFloatLiteral(expression.left);
    const rightFloat = isFloatLiteral(expression.right);
    let left =
      checkedLeft ??
      this.checkExpression(
        expression.left,
        leftLiteral ? outer : leftFloat ? outerFloat : undefined,
      );
    const rightTarget =
      expression.operator === "**"
        ? undefined
        : rightLiteral
          ? (integerTarget(left.type) ?? outer)
          : rightFloat
            ? left.type === "f32"
              ? "f32"
              : outerFloat
            : undefined;
    let right = this.checkExpression(expression.right, rightTarget);
    if (!numeric || expression.operator === "**") return { left, right };
    if (leftLiteral && left.type === "i32" && integerTarget(right.type))
      left = this.checkExpression(expression.left, right.type);
    if (leftFloat && left.type === "f64" && right.type === "f32")
      left = this.checkExpression(expression.left, "f32");
    // Operands of one family widen to the wider type (04 Binary Numeric Operators).
    const wider = widerNumeric(left.type, right.type);
    if (wider && wider !== left.type) left = this.coerce(left, wider, left.span);
    if (wider && wider !== right.type) right = this.coerce(right, wider, right.span);
    return { left, right };
  }

  /**
   * A range expression builds the `std.ops` range type of its form, with its
   * bounds as the fields (05-expressions.md#range-expressions). The bounds of
   * `a..b` and `a..=b` are typed as a binary numeric operator's operands, and
   * an expected range type gives each bound its element type.
   */
  private checkRangeExpression(
    expression: Extract<Expression, { kind: "range" }>,
    expected: ValueType | undefined,
  ): HirExpression {
    const { start, end, span } = expression;
    const form =
      start && end
        ? expression.inclusive
          ? "RangeInclusive"
          : "Range"
        : start
          ? "RangeFrom"
          : end
            ? expression.inclusive
              ? "RangeToInclusive"
              : "RangeTo"
            : "RangeFull";
    const declaration = this.standardDataType(`std.ops.${form}`);
    if (!declaration) throw new Error(`std.ops declares ${form} for a program with a range`);
    const resultType = (readonly: ValueType): ValueType =>
      expected !== undefined && mutableInner(expected) === undefined
        ? readonly
        : mutableType(readonly);
    if (!start && !end)
      return {
        kind: "data",
        dataIndex: declaration.index,
        fields: [],
        fieldIndices: [],
        type: resultType(declaration.name),
        span,
      };
    const expectedRange = expected ? nominalGenericParts(readonlyType(expected)) : undefined;
    const element =
      expectedRange?.name === declaration.name &&
      expectedRange.arguments.length === 1 &&
      isIntegerType(expectedRange.arguments[0]!)
        ? expectedRange.arguments[0]
        : undefined;
    let bounds: HirExpression[];
    if (start && end) {
      const { left, right } = this.checkNumericOperands(
        { kind: "binary", operator: "-", left: start, right: end, span },
        element,
      );
      const leftType = readonlyType(left.type);
      const rightType = readonlyType(right.type);
      if (
        isIntegerType(leftType) &&
        isIntegerType(rightType) &&
        numericType(leftType)!.family !== numericType(rightType)!.family
      )
        this.fail(
          "mixed-signedness",
          `signed and unsigned range bounds do not mix: ${leftType} and ${rightType}; cast one explicitly`,
          span,
        );
      bounds = [left, right];
    } else bounds = [this.checkExpression((start ?? end)!, element)];
    for (const bound of bounds)
      if (!isIntegerType(readonlyType(bound.type)))
        this.fail(
          "type-mismatch",
          `a range bound must have an integer type, found '${bound.type}'`,
          bound.span,
        );
    if (bounds.length === 2 && readonlyType(bounds[0]!.type) !== readonlyType(bounds[1]!.type))
      this.fail(
        "type-mismatch",
        `range bounds have types ${bounds[0]!.type} and ${bounds[1]!.type}`,
        span,
      );
    const elementType = element ?? readonlyType(bounds[0]!.type);
    const fields = bounds.map((bound) => this.requireCoercion(bound, elementType, bound.span));
    return {
      kind: "data",
      dataIndex: declaration.index,
      fields,
      fieldIndices: fields.map((_, index) => index),
      erasedFieldTypes: declaration.fields.map((field) => field.type),
      type: resultType(nominalGenericType(declaration.name, [elementType])),
      span,
    };
  }

  /** The `std` data type of a qualified name, such as `std.ops.Range`, when the program declares it. */
  protected standardDataType(standardName: string): HirData | undefined {
    for (const declaration of this.dataTypes.values())
      if (declaration.standardName === standardName) return declaration;
    return undefined;
  }

  /** Whether `type` is one of the six `std.ops` range types (05-expressions.md#range-expressions). */
  protected isRangeType(type: ValueType): boolean {
    const readonly = readonlyType(type);
    const name = nominalGenericParts(readonly)?.name ?? readonly;
    const standardName = this.dataTypes.get(name)?.standardName;
    return standardName !== undefined && /^std\.ops\.Range[A-Za-z]*$/.test(standardName);
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
function isFloatLiteral(expression: Expression): boolean {
  if (expression.kind === "float") return true;
  return (
    expression.kind === "unary" &&
    expression.operator === "-" &&
    expression.operand.kind === "float"
  );
}

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
