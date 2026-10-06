import { sourceDocument, type DiagnosticFix, type SourceSpan } from "../diagnostics.ts";
import type { Expression } from "../ast.ts";
import type { HirData, HirEnum, HirExpression, HirLocal, ValueType } from "../hir.ts";
import {
  eraseTypePermissions,
  functionType,
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  displayType,
  tupleParts,
} from "../types.ts";
import {
  isIntegerType,
  numericType,
  sameWidthNumeric,
  widensTo,
  widerNumeric,
} from "../numeric.ts";
import { CheckFailure, isKnownType, PRELUDE_NAMES } from "./context.ts";
import type { Signature } from "./context.ts";
import { ALL_COMBINATOR } from "./standard-traits.ts";
import { loopNameHint } from "./cannot-infer.ts";
import { FACTS_OF_INTRINSIC } from "./function-facts.ts";
import { DBG_INTRINSIC } from "./debug-print.ts";
import {
  containsGenericType,
  genericTypeName,
  inferGenericType,
  orderedTypeSubstitutions,
  substituteGenericType,
  traitTypeName,
} from "./shared.ts";
import { displayName } from "./display-names.ts";

import {
  defaultedLocalHint,
  hasSignedLiteral,
  markDefaultedGroup,
  pureLiteralKind,
} from "./literal-join.ts";
import { defaultGroupWidth, forcedGroupWidth } from "./literal-retry.ts";
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

/** Numeric operators whose result has the left operand's type. */
const NUMERIC_RESULT_OPERATORS = new Set([
  "+",
  "-",
  "*",
  "/",
  "%",
  "&",
  "|",
  "^",
  "<<",
  ">>",
  "**",
]);

/** A primitive operand type, on which an operator never searches a trait (r-expr.op.primitive.types). */
/** The source form of a comparison operand for a diagnostic: a name or a field path. */
function operandText(expression: Expression): string {
  if (expression.kind === "name") return expression.name;
  if (expression.kind === "member") return `${operandText(expression.receiver)}.${expression.name}`;
  return "the operand";
}

function isPrimitiveOperand(type: ValueType): boolean {
  const readonly = readonlyType(type);
  return (
    numericType(readonly) !== undefined || ["bool", "char", "string", "never"].includes(readonly)
  );
}

export abstract class ExpressionOperatorChecker extends ExpressionLiteralChecker {
  /** `dbg` named as a function value (checker/debug-print-calls.ts). */
  protected abstract checkDebugFunctionValue(
    expression: Extract<Expression, { kind: "name" }>,
    expected: ValueType | undefined,
  ): HirExpression | undefined;

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
    ) {
      // A requirement key of this function names its provider through
      // `$.use` (spec/lang/11-requirements-and-suspension.md#r-req.use.context).
      const provider = this.signature.requirements.includes(name)
        ? `; to call its provider, write '$.use(${name})'`
        : "";
      this.fail("type-used-as-value", `'${name}' names a type, not a value${provider}`, span);
    }
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
          this.readLocals.add(local);
          return this.refinedRead({
            kind: "local",
            local,
            type: local.type,
            span: expression.span,
          });
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
        // `dbg` as a function value is an ordinary use of its declaration;
        // its body prints each argument without a call site.
        if (signature?.intrinsic === DBG_INTRINSIC) {
          const printing = this.checkDebugFunctionValue(expression, _expected);
          if (printing) return printing;
        }
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
        this.failUnknownName(
          expression.name,
          `unknown name '${displayName(expression.name, this.imports)}'${loopNameHint(expression.name)}`,
          expression.span,
        );
      }
      case "unary":
        return this.checkUnaryExpression(expression, _expected);
      case "binary": {
        // Left-dispatch keeps untyped literals on the numeric path.
        const dispatched = this.binaryLeftDispatch(expression, _expected);
        if (dispatched.traitCall) return dispatched.traitCall;
        let { left, right } = this.checkNumericOperands(
          expression,
          _expected,
          dispatched.checkedLeft,
          dispatched.checkedRight,
        );
        if (expression.operator === "is")
          return this.checkIdentityExpression(expression, left, right);
        const logical = expression.operator === "and" || expression.operator === "or";
        const comparison = ["==", "!=", "<", "<=", ">", ">="].includes(expression.operator);
        const equality = expression.operator === "==" || expression.operator === "!=";
        const bitwise = ["&", "|", "^", "<<", ">>"].includes(expression.operator);
        const remainder = expression.operator === "%";
        const stringConcatenation = expression.operator === "+" && left.type === "string";
        if (comparison) ({ left, right } = this.normalizeComparisonPermissions(left, right));
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
            `an integer exponent must have an unsigned integer type, found '${displayType(right.type)}'`,
            expression.right.span,
          );
        const integerPower =
          expression.operator === "**" && isIntegerType(left.type) && unsignedExponent;
        const integerShift = this.checkShiftCount(expression.operator, left, right);
        if (left.type !== right.type && !integerPower && !integerShift)
          this.withLiteralHint(
            [
              [left, right.type],
              [right, left.type],
            ],
            () => this.rejectOperandTypes(expression, left, right),
          );
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
          const enumeration = this.enumTypes.get(nominalGenericParts(left.type)?.name ?? left.type);
          // The message names the opt-in (05-expressions.md#r-expr.eq.enum-hint),
          // and an enum of the comparison's own file gets a fix-it
          // (05-expressions.md#r-expr.eq.enum-hint.fix).
          if (enumeration)
            this.fail(
              "missing-eq",
              `type '${displayType(left.type)}' does not implement Eq; add '@derive(Eq)' to '${displayType(enumeration.name)}'`,
              expression.span,
              deriveEqFix(enumeration, expression.span),
            );
          if (
            genericTypeName(left.type) ||
            this.dataTypes.has(nominalGenericParts(left.type)?.name ?? left.type)
          ) {
            this.fail(
              "missing-eq",
              `type '${displayType(left.type)}' does not implement Eq`,
              expression.span,
            );
          }
        }
        if (comparison && !equality) this.warnUnsignedZeroComparison(expression, left, right);
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
              `type '${displayType(left.type)}' does not implement PartialOrd`,
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
            `operator '${expression.operator}' does not accept ${displayType(left.type)}`,
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

  /**
   * `t >= 0`, `0 <= t`, `t < 0`, `0 > t` on an unsigned `t` is fixed by the
   * type, so it warns (05-expressions.md#r-expr.ord.unsigned-zero).
   */
  private warnUnsignedZeroComparison(
    expression: Extract<Expression, { kind: "binary" }>,
    left: HirExpression,
    right: HirExpression,
  ): void {
    const isZero = (value: Expression) => value.kind === "integer" && value.value === 0n;
    const operator = expression.operator;
    const zeroRight = isZero(expression.right) && (operator === ">=" || operator === "<");
    const zeroLeft = isZero(expression.left) && (operator === "<=" || operator === ">");
    if (zeroRight === zeroLeft) return;
    const operand = zeroRight ? expression.left : expression.right;
    const checked = zeroRight ? left : right;
    if (pureLiteralKind(operand) !== undefined) return;
    if (numericType(checked.type)?.family !== "unsigned") return;
    const text = operandText(operand);
    const always = operator === ">=" || operator === "<=" ? "true" : "false";
    const written = zeroRight ? `${text} ${operator} 0` : `0 ${operator} ${text}`;
    const hint = defaultedLocalHint(checked, "i32");
    this.diagnostics.push({
      code: "unsigned-comparison-always",
      message: `'${text}' is unsigned, so '${written}' is always ${always}`,
      span: expression.span,
      severity: "warning",
      ...(hint
        ? {
            notes: [hint.note],
            related: [{ message: hint.note, span: hint.span }],
            ...(hint.fix ? { fix: hint.fix } : {}),
          }
        : {}),
    });
  }

  /** Normalize only a comparison's outer access permission; nested permissions remain types. */
  private normalizeComparisonPermissions(
    left: HirExpression,
    right: HirExpression,
  ): { left: HirExpression; right: HirExpression } {
    const comparedType = readonlyType(left.type);
    if (left.type === right.type || comparedType !== readonlyType(right.type))
      return { left, right };
    // One implementation serves both access views (r-trait.target.both-views).
    return {
      left: this.coerce(left, comparedType, left.span),
      right: this.coerce(right, comparedType, right.span),
    };
  }

  /**
   * Whether `left op right` is an integer shift. Its count may have any
   * unsigned integer type; any other number is `type-mismatch`, with a
   * `u32(...)` fix-it (05-expressions.md#r-expr.shift.count-invalid).
   */
  private checkShiftCount(operator: string, left: HirExpression, right: HirExpression): boolean {
    if ((operator !== "<<" && operator !== ">>") || !isIntegerType(left.type)) return false;
    const count = numericType(readonlyType(right.type));
    if (count && count.family !== "unsigned")
      this.failWithConversion(
        `a shift count must have an unsigned integer type, found '${displayType(right.type)}'; write u32(...)`,
        "u32",
        right.span,
      );
    return isIntegerType(right.type);
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
        `identity of function value of type '${displayType(functionOperand.type)}' is unspecified`,
        expression.span,
      );
    const operands = this.identityOperands(left, right);
    if (!operands) {
      this.fail(
        this.isIdentityType(withoutPermissions(left.type)) &&
          this.isIdentityType(withoutPermissions(right.type))
          ? "incompatible-identity-operands"
          : "type-mismatch",
        `identity operands have types ${displayType(left.type)} and ${displayType(right.type)}`,
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
        `identity comparison does not accept '${displayType(left.type)}'`,
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
    // A signed literal, `-1` or `+5`, is one literal: range-checked as a unit
    // (04 Negated Integer Literals), and `i32` with no expected type. Under an
    // unsigned expected type, `-` stays the operator, which rejects it, and
    // `+` passes the type through.
    if (
      (expression.operator === "-" || expression.operator === "+") &&
      expression.operand.kind === "integer" &&
      (literalTarget === undefined || numericType(literalTarget)?.family === "signed")
    )
      return this.integerLiteral(
        expression.operator === "-" ? -expression.operand.value : expression.operand.value,
        _expected,
        expression.span,
        true,
      );
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
        this.withLiteralHint([[operand, "i32"]], () =>
          this.fail(
            "unsigned-negation",
            `unary '-' does not accept the unsigned type '${displayType(operand.type)}'`,
            expression.span,
          ),
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

  /** Operand types that differ: a signedness, width, or type mismatch. */
  private rejectOperandTypes(
    expression: Extract<Expression, { kind: "binary" }>,
    left: HirExpression,
    right: HirExpression,
  ): never {
    const [shownLeft, shownRight] = [displayType(left.type), displayType(right.type)];
    // 04 Binary Numeric Operators: signed and unsigned integers do not mix.
    if (
      isIntegerType(left.type) &&
      isIntegerType(right.type) &&
      numericType(left.type)!.family !== numericType(right.type)!.family
    )
      this.fail(
        "mixed-signedness",
        `signed and unsigned operands do not mix: ${shownLeft} and ${shownRight}; cast one explicitly`,
        expression.span,
      );
    if (
      expression.operator === "**" &&
      numericType(left.type) &&
      numericType(right.type) &&
      isIntegerType(left.type) !== isIntegerType(right.type)
    )
      this.fail(
        "mixed-numeric-types",
        "integer and floating-point power operands cannot be mixed",
        expression.span,
      );
    this.rejectMixedWidths(left, right, "operator operands");
    const trait = BINARY_OPERATOR_TRAITS[expression.operator];
    this.fail(
      "type-mismatch",
      trait && !isPrimitiveOperand(right.type)
        ? `operator '${expression.operator}' needs an implementation of std.ops.${trait[0]}[${displayType(readonlyType(right.type))}] for '${displayType(left.type)}'`
        : `operator operands have types ${shownLeft} and ${shownRight}`,
      expression.span,
    );
  }

  /**
   * Two widths of one numeric family never mix
   * (04-type-system.md#r-types.num.binary.same-type): `type-mismatch`, with a
   * fix-it that converts the narrower value.
   */
  private rejectMixedWidths(left: HirExpression, right: HirExpression, what: string): void {
    const narrower = widensTo(left.type, right.type)
      ? { value: left, to: right.type }
      : widensTo(right.type, left.type)
        ? { value: right, to: left.type }
        : undefined;
    if (narrower)
      this.failWithConversion(
        `${what} have types ${displayType(left.type)} and ${displayType(right.type)}, and numbers never widen implicitly; write ${narrower.to}(...)`,
        narrower.to,
        narrower.value.span,
      );
    // One width, two types, as `u32` and `usize` (04-type-system.md#r-types.num.same-width):
    // the fix-it converts the value that is not a size, else the right one.
    if (sameWidthNumeric(left.type, right.type)) {
      const converted = readonlyType(left.type) === "usize" ? right : left;
      const to = converted === left ? right.type : left.type;
      this.failWithConversion(
        `${what} have types ${displayType(left.type)} and ${displayType(right.type)}, which never convert implicitly; write ${displayType(to)}(...)`,
        readonlyType(to),
        converted.span,
      );
    }
  }

  /**
   * Left-dispatch for a binary operator (r-expr.op.left-dispatch): a
   * non-primitive left calls its trait, and so does a primitive left with
   * a non-primitive right, as in `i64(3) * price` with
   * `impl Mul[Money] for i64`. An untyped literal left keeps the numeric
   * path. Peeks at a non-literal right without keeping diagnostics so the
   * common two-primitive case adds no candidate trials; a peek needing
   * more context falls through to report once in the numeric path.
   */
  private binaryLeftDispatch(
    expression: Extract<Expression, { kind: "binary" }>,
    expected: ValueType | undefined,
  ): {
    traitCall?: HirExpression;
    checkedLeft?: HirExpression;
    checkedRight?: HirExpression;
  } {
    const operatorTrait = BINARY_OPERATOR_TRAITS[expression.operator];
    const leftSource = expression.left;
    if (!operatorTrait || pureLiteralKind(leftSource) !== undefined) return {};
    const checkedLeft = this.checkExpression(leftSource, numericLeftExpected(expression, expected));
    const traitCall = (right?: Expression): HirExpression =>
      this.operatorTraitCall(
        operatorTrait,
        expression.operator,
        leftSource,
        checkedLeft,
        right ?? expression.right,
        expression.span,
        expected,
      );
    if (!isPrimitiveOperand(checkedLeft.type)) return { traitCall: traitCall() };
    if (pureLiteralKind(expression.right) !== undefined) return { checkedLeft };
    const diagnosticCount = this.diagnostics.length;
    try {
      const checkedRight = this.checkExpression(expression.right, undefined);
      if (!isPrimitiveOperand(checkedRight.type)) return { traitCall: traitCall() };
      return { checkedLeft, checkedRight };
    } catch (error) {
      if (!(error instanceof CheckFailure)) throw error;
      this.diagnostics.length = diagnosticCount;
      return { checkedLeft };
    }
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
    checkedRight?: HirExpression,
  ): { left: HirExpression; right: HirExpression } {
    const arithmetic = ["+", "-", "*", "/", "%", "&", "|", "^"].includes(expression.operator);
    const numeric =
      arithmetic || ["==", "!=", "<", "<=", ">", ">=", "**"].includes(expression.operator);
    const resultTyped = NUMERIC_RESULT_OPERATORS.has(expression.operator);
    const outer = resultTyped ? integerLiteralTarget(expected) : undefined;
    const outerFloat = resultTyped ? floatLiteralTarget(expected) : undefined;
    const leftContext = resultTyped
      ? contextualNumericExpected(expression.left, expected)
      : undefined;
    const shift = expression.operator === "<<" || expression.operator === ">>";
    const countOrExponent = shift || expression.operator === "**";
    // The join model (literal-join.ts): two operands built only of literals
    // form one group, `i32` when any is signed and `usize` otherwise; a
    // literal operand beside a typed one takes that one's type, so the typed
    // side is checked first, in either order.
    const leftPure = checkedLeft === undefined ? pureLiteralKind(expression.left) : undefined;
    const rightPure = pureLiteralKind(expression.right);
    const fixedGroup =
      leftPure !== undefined && rightPure !== undefined
        ? ((leftPure === "integer" ? outer : outerFloat) ??
          forcedGroupWidth([expression.left, expression.right]))
        : undefined;
    const groupTarget =
      leftPure !== undefined && rightPure !== undefined
        ? (fixedGroup ??
          defaultGroupWidth(
            countOrExponent ? [expression.left] : [expression.left, expression.right],
          ))
        : undefined;
    const variantOperand =
      isContextualVariant(expression.left) || isContextualVariant(expression.right);
    if (
      numeric &&
      !countOrExponent &&
      leftPure !== undefined &&
      rightPure === undefined &&
      !variantOperand
    ) {
      const contextualRightKind = contextualNumericKind(expression.right);
      const right = this.checkExpression(
        expression.right,
        contextualRightKind === "integer"
          ? outer
          : contextualRightKind === "float"
            ? outerFloat
            : undefined,
      );
      const family = numericType(readonlyType(right.type))?.family;
      const typed =
        family !== undefined && (family === "float") === (leftPure === "float")
          ? readonlyType(right.type)
          : leftContext;
      const left = this.checkExpression(expression.left, typed);
      return { left, right };
    }
    // In `==` and `!=`, a contextual variant operand takes the other
    // operand's type as its expected type, on either side
    // (05-expressions.md#r-expr.eq.contextual-operand). Two contextual
    // operands leave both without one, so each reports its own error.
    const equalityOperator = expression.operator === "==" || expression.operator === "!=";
    const leftVariant = isContextualVariant(expression.left);
    const rightVariant = isContextualVariant(expression.right);
    let leftFirstRight: HirExpression | undefined;
    if (equalityOperator && leftVariant && !rightVariant && checkedLeft === undefined) {
      leftFirstRight = this.checkExpression(expression.right, undefined);
    }
    let left =
      checkedLeft ??
      this.checkExpression(
        expression.left,
        leftFirstRight ? readonlyType(leftFirstRight.type) : (groupTarget ?? leftContext),
      );
    // A floating-point literal exponent takes the base's type
    // (r-expr.power.float.same-type), an unsigned literal exponent of an
    // integer base is a `u32` (r-expr.power.int.literal), and so is a shift
    // count (r-expr.shift.count-literal).
    const contextualRight = contextualNumericKind(expression.right);
    const rightTarget = shift
      ? "u32"
      : expression.operator === "**"
        ? contextualRight === "float" && left.type === "f32"
          ? "f32"
          : contextualRight === "integer" &&
              isIntegerType(left.type) &&
              !hasSignedLiteral(expression.right)
            ? "u32"
            : undefined
        : groupTarget !== undefined
          ? groupTarget
          : contextualRight === "integer"
            ? (integerTarget(left.type) ?? outer)
            : contextualRight === "float"
              ? left.type === "f32"
                ? "f32"
                : outerFloat
              : // The join model: a typed left operand is the expected type
                // of any right operand, as of a generic call `identity(0)`.
                numeric && numericType(readonlyType(left.type))
                ? readonlyType(left.type)
                : undefined;
    let right =
      leftFirstRight ??
      checkedRight ??
      this.checkExpression(
        expression.right,
        equalityOperator && rightVariant && !leftVariant ? readonlyType(left.type) : rightTarget,
      );
    // A panic report says when the group's type is its default
    // (06-control-flow.md#r-flow.panic.report.fallback).
    if (groupTarget !== undefined && fixedGroup === undefined) markDefaultedGroup([left, right]);
    if (!numeric || expression.operator === "**") return { left, right };
    // A literal operand typed before the other operand takes its type
    // (04-type-system.md#r-types.num.binary.literal); no other operand widens.
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
    defaultElement?: ValueType,
  ): HirExpression {
    const { start, end, span } = expression;
    // `a..=b` is a `Range` and `..=b` a `RangeTo`, with `inclusive` true
    // (r-expr.range.inclusive-field).
    const form = start && end ? "Range" : start ? "RangeFrom" : end ? "RangeTo" : "RangeFull";
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
    const expectedElement =
      expectedRange?.name === declaration.name &&
      expectedRange.arguments.length === 1 &&
      isIntegerType(expectedRange.arguments[0]!)
        ? expectedRange.arguments[0]
        : undefined;
    const element = expectedElement ?? defaultElement;
    let bounds: HirExpression[];
    if (start && end) {
      // A negated slice literal always receives the unsigned index context,
      // even when its other bound has a written signed type.
      if (defaultElement !== undefined) {
        for (const bound of [start, end])
          if (bound.kind === "unary" && bound.operator === "-" && bound.operand.kind === "integer")
            this.checkExpression(bound, defaultElement);
      }
      // With a typed peer, the ordinary binary-literal rule wins: `0..end`
      // has end's type. Two literals have no peer type, so both use usize.
      const boundContext =
        defaultElement !== undefined &&
        !(contextualNumericKind(start) === "integer" && contextualNumericKind(end) === "integer")
          ? undefined
          : element;
      const { left, right } = this.checkNumericOperands(
        { kind: "binary", operator: "-", left: start, right: end, span },
        boundContext,
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
          `signed and unsigned range bounds do not mix: ${displayType(leftType)} and ${displayType(rightType)}; cast one explicitly`,
          span,
        );
      bounds = [left, right];
    } else bounds = [this.checkExpression((start ?? end)!, element)];
    for (const bound of bounds)
      if (!isIntegerType(readonlyType(bound.type)))
        this.fail(
          "type-mismatch",
          `a range bound must have an integer type, found '${displayType(bound.type)}'`,
          bound.span,
        );
    if (bounds.length === 2 && readonlyType(bounds[0]!.type) !== readonlyType(bounds[1]!.type)) {
      this.rejectMixedWidths(bounds[0]!, bounds[1]!, "range bounds");
      this.fail(
        "type-mismatch",
        `range bounds have types ${displayType(bounds[0]!.type)} and ${displayType(bounds[1]!.type)}`,
        span,
      );
    }
    // A built-in slice gives literal bounds a default, not a fixed range
    // type: a typed wider unsigned bound still determines the range element.
    const elementType =
      defaultElement === undefined && expectedElement !== undefined
        ? expectedElement
        : readonlyType(bounds[0]!.type);
    const fields: HirExpression[] = bounds.map((bound) =>
      this.requireCoercion(bound, elementType, bound.span),
    );
    if (end)
      fields.push({
        kind: "boolean",
        value: expression.inclusive === true,
        type: "bool",
        span,
      } as HirExpression);
    return {
      kind: "data",
      dataIndex: declaration.index,
      fields,
      fieldIndices: fields.map((_, index) => index),
      erasedFieldTypes: declaration.fields.map((field) => field.type),
      erasedTypeSubstitutions: orderedTypeSubstitutions(
        declaration.genericParameters,
        new Map([[declaration.genericParameters[0]!, elementType]]),
      ),
      type: resultType(nominalGenericType(declaration.name, [elementType])),
      span,
    };
  }

  /**
   * Checks the index of a built-in list or string. A written range gives its
   * bounds the `usize` context without fixing a typed wider unsigned bound to
   * `usize`; a range value is validated by its element type.
   */
  protected checkBuiltinIndexExpression(
    expression: Expression,
    receiver: "list" | "string",
  ): HirExpression {
    const index =
      expression.kind === "range"
        ? this.checkRangeExpression(expression, undefined, "usize")
        : this.checkExpression(expression, "usize");
    if (!this.isRangeType(index.type))
      return this.requireUnsignedIndex(index, receiver, expression.span);

    const readonly = readonlyType(index.type);
    const nominal = nominalGenericParts(readonly);
    const standardName = this.dataTypes.get(nominal?.name ?? readonly)?.standardName;
    if (standardName === "std.ops.RangeFull") return index;
    const element = nominal?.arguments[0];
    if (numericType(readonlyType(element ?? ""))?.family !== "unsigned")
      this.fail(
        "type-mismatch",
        `slice bounds must have an unsigned integer type, found '${displayType(element ?? index.type)}'`,
        expression.span,
      );
    return index;
  }

  /** The `std` data type of a qualified name, such as `std.ops.Range`, when the program declares it. */
  protected standardDataType(standardName: string): HirData | undefined {
    for (const declaration of this.dataTypes.values())
      if (declaration.standardName === standardName) return declaration;
    return undefined;
  }

  /** Whether `type` is one of the four `std.ops` range types (05-expressions.md#range-expressions). */
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
    const elementTypes = expression.bindings.length === 1 ? undefined : tupleParts(value.type);
    if (expression.bindings.length > 1 && elementTypes?.length !== expression.bindings.length)
      this.fail(
        "type-mismatch",
        `binding has ${expression.bindings.length} names but '${displayType(value.type)}' has ${elementTypes?.length ?? 1} element${elementTypes?.length === 1 ? "" : "s"}`,
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
type ContextualNumericKind = "integer" | "float";
const CONTEXTUAL_NUMERIC_KINDS = new WeakMap<Expression, ContextualNumericKind | null>();

/**
 * The unsuffixed numeric kind at the left edge of a built-in operator tree.
 *
 * A typed value on that edge already supplies the context for literals to its
 * right. Stopping there also keeps an expected result from participating in
 * trait-operator selection. Suffixed literals are calls in the AST and are
 * therefore intentionally absent.
 */
function contextualNumericKind(expression: Expression): ContextualNumericKind | undefined {
  const cached = CONTEXTUAL_NUMERIC_KINDS.get(expression);
  if (cached !== undefined) return cached ?? undefined;
  const kind =
    expression.kind === "integer"
      ? "integer"
      : expression.kind === "float"
        ? "float"
        : expression.kind === "unary" &&
            (expression.operator === "+" ||
              expression.operator === "-" ||
              expression.operator === "~")
          ? contextualNumericKind(expression.operand)
          : expression.kind === "binary" && NUMERIC_RESULT_OPERATORS.has(expression.operator)
            ? contextualNumericKind(expression.left)
            : undefined;
  CONTEXTUAL_NUMERIC_KINDS.set(expression, kind ?? null);
  return kind;
}

/** The expected type that can choose the unsuffixed literals in `expression`. */
function contextualNumericExpected(
  expression: Expression,
  expected: ValueType | undefined,
): ValueType | undefined {
  const kind = contextualNumericKind(expression);
  return kind === "integer"
    ? integerLiteralTarget(expected)
    : kind === "float"
      ? floatLiteralTarget(expected)
      : undefined;
}

/** Context passed through the eager left check used to choose primitive or trait dispatch. */
/** A `.Name` or `.Name(args)` operand whose enum type comes from context. */
function isContextualVariant(expression: Expression): boolean {
  return (
    expression.kind === "contextual-variant" ||
    (expression.kind === "call" && expression.callee.kind === "contextual-variant")
  );
}

function numericLeftExpected(
  expression: Extract<Expression, { kind: "binary" }>,
  expected: ValueType | undefined,
): ValueType | undefined {
  return NUMERIC_RESULT_OPERATORS.has(expression.operator)
    ? contextualNumericExpected(expression.left, expected)
    : undefined;
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
  return eraseTypePermissions(type);
}

/**
 * The fix-it that inserts `@derive(Eq)` on its own line above an enum's
 * declaration (05-expressions.md#r-expr.eq.enum-hint.fix), or undefined for an
 * enum of std or of another package. A package build keeps it only when the
 * enum is in the comparison's file (package.ts, `locate`).
 */
function deriveEqFix(enumeration: HirEnum, span: SourceSpan): DiagnosticFix | undefined {
  const own =
    enumeration.standardName === undefined &&
    !enumeration.name.startsWith("__pkg_") &&
    sourceDocument(enumeration.span) === sourceDocument(span);
  if (!own) return undefined;
  const { start } = enumeration.span;
  const lineStart = { ...start, column: 1, offset: start.offset - (start.column - 1) };
  return {
    message: `add '@derive(Eq)' to '${displayType(enumeration.name)}'`,
    edits: [
      {
        span: { start: lineStart, end: lineStart },
        replacement: `${" ".repeat(start.column - 1)}@derive(Eq)\n`,
      },
    ],
  };
}
