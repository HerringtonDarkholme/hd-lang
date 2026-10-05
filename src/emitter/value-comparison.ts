import type {
  HirBuiltinTraitImplementation,
  HirEqualityDispatch,
  HirEqualityStrategy,
  HirExpression,
  HirOrderingStrategy,
  ValueType,
} from "../hir.ts";
import { STANDARD_INSPECTABLE } from "../checker/standard-traits.ts";
import { mutableInner, optionalInner, readonlyType } from "../types.ts";
import { EmitterContext } from "./context.ts";
import { numericType } from "../numeric.ts";
import { scalarWasm } from "./scalars.ts";
import {
  emitCast,
  emitSizedBinary,
  emitSizedUnary,
  powerFunction,
  emitWiden,
  isSizedNumeric,
  type SizedNumericContext,
} from "./sized-numeric.ts";
import {
  containsGenericValueType,
  functionName,
  localName,
  methodBoundParameters,
  stringLiteral,
  traitSuspensionName,
} from "./shared.ts";

/** The receiver of a nested `runtime_type` read that composes a type argument's key. */
const NESTED_TYPE_ID_RECEIVER = "(ref.i31 (i32.const 0))";

/**
 * The operator of each intrinsic method of an operator trait
 * (09-traits.md#intrinsic-methods), which compiles as that operator does on
 * primitive operands.
 */
const BINARY_INTRINSICS: Readonly<Record<string, string>> = {
  add: "+",
  sub: "-",
  mul: "*",
  div: "/",
  rem: "%",
  bit_and: "&",
  bit_or: "|",
  bit_xor: "^",
  shl: "<<",
  shr: ">>",
};

const UNARY_INTRINSICS: Readonly<Record<string, string>> = { neg: "-", not: "~" };

/** The wrapper of one intrinsic method for one type, behind its dictionaries. */
interface IntrinsicAdapter {
  readonly index: number;
  readonly builtin: Extract<HirBuiltinTraitImplementation, { kind: "intrinsic" }>;
  readonly methodIndex: number;
}

export abstract class ValueComparisonEmitter extends EmitterContext {
  protected abstract emitExpression(expression: HirExpression): string;

  protected allocateTemporary(type: ValueType): string {
    const index = this.temporaryTypes.length;
    this.temporaryTypes.push(type);
    return `$tmp${index}`;
  }

  private sizedNumeric(): SizedNumericContext {
    return {
      allocateTemporary: (type) => this.allocateTemporary(type),
      emitRuntimePanic: (name) => this.emitRuntimePanic(name),
      emitCheckedDivision: (width, operator, left, right) =>
        this.emitCheckedDivision(width, operator, left, right),
      release: this.release,
      useFloatPower: () => {
        this.floatPower = true;
      },
      useFloatRemainder: () => {
        this.floatRemainder = true;
      },
    };
  }

  /** Widening, casts, and `-` and `~` on the sized numeric types (emitter/sized-numeric.ts). */
  protected emitNumericUnary(
    expression: Extract<HirExpression, { kind: "unary" }>,
    operand: string,
  ): string | undefined {
    if (expression.operator === "widen")
      return emitWiden(operand, expression.operand.type, expression.type);
    if (expression.operator === "cast")
      return emitCast(operand, readonlyType(expression.operand.type), expression.type);
    return isSizedNumeric(expression.type) && expression.operator !== "+"
      ? emitSizedUnary(expression.operator, operand, expression.type, this.release)
      : undefined;
  }

  /** A binary operator on the sized numeric types, or an `i64` exponent. */
  protected emitNumericBinary(
    expression: Extract<HirExpression, { kind: "binary" }>,
    left: string,
    right: string,
  ): string | undefined {
    const operator = expression.operator;
    if (["==", "!=", "is", "and", "or"].includes(operator)) return undefined;
    if (isSizedNumeric(expression.left.type))
      return emitSizedBinary(
        operator,
        left,
        right,
        expression.left.type,
        expression.right.type,
        this.sizedNumeric(),
      );
    // Floating `%` truncates, as C `fmod` does (05-expressions.md#r-expr.float.remainder-truncated).
    if (operator === "%" && expression.type === "f64") {
      this.floatRemainder = true;
      return `(call $hd.rem_f64 ${left} ${right})`;
    }
    if (expression.left.type === "i64" && (operator === "<<" || operator === ">>"))
      return `(i64.${operator === "<<" ? "shl" : "shr_s"} ${left} ${this.release ? right : `(call $hd.check_shift_i64 ${right})`})`;
    if (operator === "**" && scalarWasm(expression.right.type) === "i64")
      return `(call ${powerFunction(expression.type as "i32" | "i64", this.release)} ${left} (i32.wrap_i64 ${right}))`;
    return undefined;
  }

  /** The dictionary a bound dispatch calls through, reached along its supertrait path. */
  private boundDispatchDictionary(
    dispatch: Extract<HirEqualityDispatch, { kind: "bound" }>,
  ): string {
    let dictionary = `(local.get $bound${dispatch.boundIndex})`;
    if (!dispatch.via) return dictionary;
    let trait = this.traitsByIndex.get(dispatch.via.traitIndex)!;
    for (const fieldIndex of dispatch.via.path) {
      dictionary = `(struct.get $trait${trait.index} $trait${trait.index}s${fieldIndex} ${dictionary})`;
      trait = this.traitsByIndex.get(trait.supertraits[fieldIndex]!.traitIndex)!;
    }
    return dictionary;
  }

  /** A call of an implementation's method, with its bounds' dictionaries after the operands. */
  private emitFunctionDispatch(
    dispatch: Extract<HirEqualityDispatch, { kind: "function" }>,
    left: string,
    right: string,
  ): string {
    const bounds = (dispatch.bounds ?? []).map((bound) => ` ${this.emitExpression(bound)}`);
    return `(call ${functionName(dispatch.functionIndex)} ${left} ${right}${bounds.join("")})`;
  }

  /** The `Eq` function of each declared map key type, by function index. */
  private readonly keyEqualityTypes = new Map<number, ValueType>();

  /** The `Eq` traits whose bound dictionaries key a map (kind 3), by trait index. */
  private readonly boundKeyTraits = new Set<number>();

  /**
   * A map's key equality and key context, its last two `$hd.map` operands:
   * null for a scalar or string key (kinds 0 and 1); a wrapper of the key
   * type's `Eq` implementation, such as std's `Eq` for `i64` (kind 2); or, for a type-parameter key (kind 3), a wrapper that calls
   * `Eq` through the bound's dictionary, which is the context.
   */
  protected keyEquality(
    keyType: ValueType,
    keyKind: number,
    dispatch?: HirEqualityDispatch,
    dictionary?: HirExpression,
  ): string {
    if (keyKind === 3 && dictionary?.kind === "trait-dictionary") {
      this.boundKeyTraits.add(dictionary.traitIndex);
      return `(ref.func $hd.keqb${dictionary.traitIndex}) ${this.emitExpression(dictionary)}`;
    }
    if (keyKind === 3 && dispatch?.kind === "bound") {
      this.boundKeyTraits.add(dispatch.traitIndex);
      return `(ref.func $hd.keqb${dispatch.traitIndex}) ${this.boundDispatchDictionary(dispatch)}`;
    }
    if (keyKind !== 2) return `(ref.null $hd.key-eq) (ref.null any)`;
    const type = readonlyType(keyType);
    const eq = this.traitsByName.get("Eq");
    const implementation = [...this.implementationsByIndex.values()].find(
      (candidate) => candidate.traitIndex === eq?.index && candidate.targetType === type,
    );
    const method = implementation?.methodFunctions.find(({ methodIndex }) => methodIndex === 0);
    if (!method) throw new Error(`map key type '${type}' has no Eq implementation`);
    this.keyEqualityTypes.set(method.functionIndex, type);
    return `(ref.func $hd.keq${method.functionIndex}) (ref.null any)`;
  }

  keyEqualityNames(): string[] {
    return [
      ...[...this.keyEqualityTypes.keys()].map((index) => `$hd.keq${index}`),
      ...[...this.boundKeyTraits].map((index) => `$hd.keqb${index}`),
    ];
  }

  /** A map's key hash, by the key type's `Hash` implementation function index. */
  private readonly keyHashTypes = new Map<
    number,
    {
      readonly keyType: ValueType;
      readonly hasherTrait: number;
      readonly hasherImpl: number;
      readonly hasherData: number;
      readonly hasherField: number;
    }
  >();

  /** Whether a map needs the constant hash: a type-parameter key (kind 3), or a key type with no concrete `Hash`. */
  private constantHashNeeded = false;

  /**
   * A map's key hash and key hash context, two more `$hd.map` operands: a wrapper of the key type's `Hash`
   * implementation over a fresh `DefaultHasher` (as `hash_of` hashes), or the constant hash when no concrete
   * `Hash` implementation can be called (a type-parameter or otherwise generic key, kind 3), which degrades
   * that map to one chain: still a correct linear scan under `Eq`.
   */
  protected keyHash(keyType: ValueType, keyKind: number): string {
    if (keyKind !== 3) {
      const type = readonlyType(keyType);
      const hash = this.traitsByName.get("Hash");
      const implementation = [...this.implementationsByIndex.values()].find(
        (candidate) => candidate.traitIndex === hash?.index && candidate.targetType === type,
      );
      const method = implementation?.methodFunctions.find(({ methodIndex }) => methodIndex === 0);
      const hasher = this.traitsByName.get("Hasher");
      const hasherData = [...this.dataByName.values()].find((data) =>
        data.name.endsWith("DefaultHasher"),
      );
      const hasherImpl =
        hasher &&
        hasherData &&
        [...this.implementationsByIndex.values()].find(
          (candidate) =>
            candidate.traitIndex === hasher.index && candidate.targetType === hasherData.name,
        );
      const hasherMethod = hasherImpl?.methodFunctions.find(({ methodIndex }) => methodIndex === 0);
      const hasherField = hasherData?.fields.find((field) => field.name === "state")?.index ?? 0;
      if (method && hasher && hasherData && hasherImpl && hasherMethod) {
        this.keyHashTypes.set(method.functionIndex, {
          keyType: type,
          hasherTrait: hasher.index,
          hasherImpl: hasherImpl.index,
          hasherData: hasherData.index,
          hasherField,
        });
        return `(ref.func $hd.kh${method.functionIndex}) (ref.null any)`;
      }
    }
    this.constantHashNeeded = true;
    return `(ref.func $hd.kh_const) (ref.null any)`;
  }

  keyHashNames(): string[] {
    return [
      ...[...this.keyHashTypes.keys()].map((index) => `$hd.kh${index}`),
      ...(this.constantHashNeeded ? ["$hd.kh_const"] : []),
    ];
  }

  emitKeyHashes(): string {
    // The FNV offset basis that `DefaultHasher::new` starts from (lib/std/hash.hd).
    const basis = "(i64.const -3750763034362895579)";
    const wrappers = [...this.keyHashTypes].map(
      ([index, setup]) =>
        `(func $hd.kh${index} (type $hd.key-hash) (param $key anyref) (param $context anyref) (result i64)\n` +
        `  (local $hasher (ref null $d${setup.hasherData}))\n` +
        `  (local.set $hasher (struct.new $d${setup.hasherData} ${basis}))\n` +
        `  (call ${functionName(index)} ${this.unboxValue("(local.get $key)", setup.keyType)} (struct.new $trait${setup.hasherTrait} (local.get $hasher) (ref.null $hd.list) (ref.func $tadapt${setup.hasherImpl}_0)))\n` +
        `  (struct.get $d${setup.hasherData} $d${setup.hasherData}f${setup.hasherField} (local.get $hasher)))`,
    );
    if (this.constantHashNeeded)
      wrappers.push(
        `(func $hd.kh_const (type $hd.key-hash) (param $key anyref) (param $context anyref) (result i64)\n  (i64.const 0))`,
      );
    return wrappers.join("\n\n");
  }

  emitKeyEqualities(): string {
    const signature =
      "(type $hd.key-eq) (param $left anyref) (param $right anyref) (param $context anyref) (result i32)";
    return [
      ...[...this.keyEqualityTypes].map(
        ([index, type]) =>
          `(func $hd.keq${index} ${signature}\n  (call ${functionName(index)} ${this.unboxValue("(local.get $left)", type)} ${this.unboxValue("(local.get $right)", type)}))`,
      ),
      ...[...this.boundKeyTraits].map((index) => {
        const dictionary = `(ref.cast (ref null $trait${index}) (local.get $context))`;
        return `(func $hd.keqb${index} ${signature}\n  (call_ref $tsig${index}_0 (local.get $left) ${dictionary} (local.get $right) (struct.get $trait${index} $trait${index}m0 ${dictionary})))`;
      }),
    ].join("\n\n");
  }

  /** Signed integer `/` or `%`, panicking on a zero divisor and on overflow. */
  protected emitCheckedDivision(
    width: "i32" | "i64",
    operator: "/" | "%",
    left: string,
    right: string,
  ): string {
    const minimum = width === "i64" ? "-9223372036854775808" : "-2147483648";
    const leftTemporary = this.allocateTemporary(width);
    const rightTemporary = this.allocateTemporary(width);
    const overflow =
      operator === "/"
        ? [
            `  (if (i32.and`,
            `    (${width}.eq (local.get ${leftTemporary}) (${width}.const ${minimum}))`,
            `    (${width}.eq (local.get ${rightTemporary}) (${width}.const -1)))`,
            `    (then ${this.emitRuntimePanic("integer-overflow")}))`,
          ]
        : [];
    return [
      `(block (result ${width})`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (if (${width}.eqz (local.get ${rightTemporary}))`,
      `    (then ${this.emitRuntimePanic("integer-division-by-zero")}))`,
      ...overflow,
      `  (${width}.${operator === "/" ? "div_s" : "rem_s"} (local.get ${leftTemporary}) (local.get ${rightTemporary}))`,
      `)`,
    ].join("\n");
  }

  protected emitBuiltinTraitDictionary(
    builtin: HirBuiltinTraitImplementation,
    boundExpressions: readonly HirExpression[],
    bounds: readonly string[],
    value: string,
    parents: readonly string[] = [],
  ): string {
    const trait = this.traitsByIndex.get(builtin.traitIndex)!;
    if (builtin.kind === "marker")
      return `(struct.new $trait${trait.index} ${value} (ref.null $hd.list))`;
    if (builtin.kind === "intrinsic") {
      const methods = builtin.methods.flatMap((_, methodIndex) =>
        this.methodIsLive(trait.index, methodIndex)
          ? [`(ref.func ${this.intrinsicAdapter(builtin, methodIndex)})`]
          : [],
      );
      return `(struct.new $trait${trait.index} ${value} (ref.null $hd.list)${[...methods, ...parents].map((part) => ` ${part}`).join("")})`;
    }
    if (builtin.kind === "forward") return this.emitForwardingDictionary(builtin, value);
    // An Inspectable key's every bound is an Inspectable dictionary, such as
    // a handle's witness (annot.handle.fact.key).
    const boundTraits = boundExpressions.map((bound) =>
      bound.kind === "trait-bound-dictionary" ? bound.traitIndex : builtin.traitIndex,
    );
    const boundPack =
      bounds.length > 0
        ? `(array.new_fixed $hd.list ${bounds.length} ${bounds.join(" ")})`
        : `(ref.null $hd.list)`;
    if (!this.methodIsLive(trait.index, 0))
      return `(struct.new $trait${trait.index} ${value} ${boundPack}${parents.map((parent) => ` ${parent}`).join("")})`;
    const key = JSON.stringify([builtin, boundTraits]);
    let adapter = this.builtinTraitAdapters.get(key);
    if (!adapter) {
      adapter = { index: this.builtinTraitAdapters.size, implementation: builtin, boundTraits };
      this.builtinTraitAdapters.set(key, adapter);
    }
    return `(struct.new $trait${trait.index} ${value} ${boundPack} (ref.func $tbuiltin${adapter.index})${parents.map((parent) => ` ${parent}`).join("")})`;
  }

  protected emitStringLiteral(text: string): string {
    return stringLiteral(text);
  }

  /** The key string of `runtime_type()` read through an Inspectable dictionary or trait value. */
  protected emitTypeIdKey(
    dictionary: string,
    traitIndex: number,
    receiver = "(ref.null any)",
  ): string {
    const typeId = this.dataByName.get("TypeId")!.index;
    const call = `(call_ref $tsig${traitIndex}_0 ${receiver} ${dictionary} (struct.get $trait${traitIndex} $trait${traitIndex}m0 ${dictionary}))`;
    return `(struct.get $d${typeId} $d${typeId}f0 (ref.as_non_null ${call}))`;
  }

  /** `TypeId::of::[T]()` and the downcasts (spec/lang/09-traits.md#recovering-a-concrete-type). */
  protected emitInspectExpression(
    expression: Extract<HirExpression, { kind: "inspect-type-id" | "inspect-downcast" }>,
    dictionaryValue: string,
    valueCode: string,
  ): string {
    const dictionary = this.allocateTemporary(expression.dictionary.type);
    const trait = expression.traitIndex;
    const result = this.watType(expression.type);
    if (expression.kind === "inspect-type-id")
      return `(block (result ${result})
  (local.set ${dictionary} ${dictionaryValue})
  (call_ref $tsig${trait}_0 (ref.null any) (local.get ${dictionary}) (struct.get $trait${trait} $trait${trait}m0 (local.get ${dictionary})))
)`;
    const value = this.allocateTemporary(expression.value.type);
    const payload = `(struct.get $trait${trait} $trait${trait}value (local.get ${value}))`;
    const recorded = this.emitTypeIdKey(`(local.get ${value})`, trait, payload);
    const target = this.emitTypeIdKey(`(local.get ${dictionary})`, trait);
    return `(block (result ${result})
  (local.set ${value} ${valueCode})
  (local.set ${dictionary} ${dictionaryValue})
  (if (result ${result}) (call ${this.stringFunction("equal")} ${recorded} ${target})
    (then (struct.new $hd.variant (i32.const 1) ${payload}))
    (else (struct.new $hd.variant (i32.const 0) (ref.null any))))
)`;
  }

  /**
   * An intrinsic method's operation on primitive operands, inline
   * (09-traits.md#r-trait.impl.intrinsic.inline): an operator trait's
   * method compiles as its operator, and `eq`, `partial_cmp`, and `cmp` as
   * the comparison. A NaN operand makes `partial_cmp` `.None`.
   */
  protected emitIntrinsicCall(
    expression: Extract<HirExpression, { kind: "intrinsic-call" }>,
  ): string {
    const { method, type, span } = expression;
    const [left, right] = expression.arguments;
    const binary = BINARY_INTRINSICS[method];
    if (binary && left && right)
      return this.emitExpression({ kind: "binary", operator: binary, left, right, type, span });
    const unary = UNARY_INTRINSICS[method];
    if (unary && left)
      return this.emitExpression({ kind: "unary", operator: unary, operand: left, type, span });
    if (method === "eq" && left && right)
      return this.emitValueEquality(
        this.emitExpression(left),
        this.emitExpression(right),
        left.type,
      );
    if ((method === "partial_cmp" || method === "cmp") && left && right) {
      const compared = this.emitValueOrdering(
        this.emitExpression(left),
        this.emitExpression(right),
        left.type,
      );
      const temporary = this.allocateTemporary("i32");
      const code = `(local.get ${temporary})`;
      const resultType = this.watType(type);
      const orderingIndex = this.enumByName.get(readonlyType(optionalInner(type) ?? type))!.index;
      const orderingType = `(ref $e${orderingIndex})`;
      const variant = (tag: number) => `(global.get $e${orderingIndex}v${tag})`;
      const ordering = `(if (result ${orderingType}) (i32.lt_s ${code} (i32.const 0)) (then ${variant(0)}) (else (if (result ${orderingType}) (i32.eqz ${code}) (then ${variant(1)}) (else ${variant(2)}))))`;
      const value =
        method === "cmp"
          ? ordering
          : `(if (result ${resultType}) (i32.eq ${code} (i32.const 2)) (then (struct.new $hd.variant (i32.const 0) (ref.null any))) (else (struct.new $hd.variant (i32.const 1) ${ordering})))`;
      return `(block (result ${resultType}) (local.set ${temporary} ${compared}) ${value})`;
    }
    throw new Error(`no intrinsic method '${method}' (spec/lang/09-traits.md#intrinsic-methods)`);
  }

  private readonly intrinsicAdapters = new Map<string, IntrinsicAdapter>();

  /**
   * The wrapper of `builtin`'s method `methodIndex`, one per type and method;
   * a trait argument, such as the count type of `Shl[C]`, is part of the method.
   */
  private intrinsicAdapter(
    builtin: Extract<HirBuiltinTraitImplementation, { kind: "intrinsic" }>,
    methodIndex: number,
  ): string {
    const method = builtin.methods[methodIndex]!;
    const key = JSON.stringify([builtin.traitIndex, builtin.targetType, methodIndex, method]);
    let adapter = this.intrinsicAdapters.get(key);
    if (!adapter) {
      adapter = { index: this.intrinsicAdapters.size, builtin, methodIndex };
      this.intrinsicAdapters.set(key, adapter);
    }
    return `$tintrinsic${adapter.index}`;
  }

  /**
   * The wrappers behind intrinsic dictionaries: each unboxes its erased
   * operands into locals and computes the operation inline.
   */
  emitIntrinsicAdapters(): string {
    const savedTemporaries = [...this.temporaryTypes];
    const adapters = [...this.intrinsicAdapters.values()].map(({ index, builtin, methodIndex }) => {
      const trait = this.traitsByIndex.get(builtin.traitIndex)!;
      const method = trait.methods[methodIndex]!;
      const concrete = builtin.methods[methodIndex]!;
      if (method.suspending || method.requirements.length > 0 || method.genericBounds?.length)
        throw new Error(`intrinsic method '${concrete.name}' takes no bounds or requirements`);
      this.temporaryTypes.length = 0;
      const span = trait.span;
      const operandTypes = [builtin.targetType, ...concrete.parameterTypes];
      const operands = operandTypes.map((type, local): HirExpression => ({
        kind: "local",
        local: {
          name: `$operand${local}`,
          type,
          index: local,
          mutable: false,
          parameter: false,
          span,
        },
        type,
        span,
      }));
      const computed = this.emitIntrinsicCall({
        kind: "intrinsic-call",
        method: concrete.name,
        arguments: operands,
        type: concrete.resultType,
        span,
      });
      const body = containsGenericValueType(method.result)
        ? this.boxWatValue(computed, concrete.resultType)
        : computed;
      const parameters = method.parameters.map(
        (parameter, parameterIndex) =>
          `(param $a${parameterIndex} ${this.parameterWatType(parameter)})`,
      );
      const result = method.result === "void" ? "" : ` (result ${this.watType(method.result)})`;
      const locals = operandTypes.map(
        (type, local) => `  (local ${localName(local)} ${this.watType(type)})`,
      );
      const temporaries = this.temporaryTypes.map(
        (type, temporary) => `  (local $tmp${temporary} ${this.watType(type)})`,
      );
      const sets = [
        `  (local.set ${localName(0)} ${this.unboxValue("(local.get $self)", builtin.targetType)})`,
        ...method.parameters.map(
          (parameter, parameterIndex) =>
            `  (local.set ${localName(parameterIndex + 1)} ${
              containsGenericValueType(parameter)
                ? this.unboxValue(
                    `(local.get $a${parameterIndex})`,
                    concrete.parameterTypes[parameterIndex]!,
                  )
                : `(local.get $a${parameterIndex})`
            })`,
        ),
      ];
      return [
        `(func $tintrinsic${index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref)${parameters.length ? " " + parameters.join(" ") : ""}${result}`,
        ...locals,
        ...temporaries,
        ...sets,
        `  ${body}`,
        `)`,
      ].join("\n");
    });
    this.temporaryTypes.length = 0;
    this.temporaryTypes.push(...savedTemporaries);
    return adapters.join("\n\n");
  }

  get builtinTraitAdapterNames(): readonly string[] {
    return [
      ...[...this.builtinTraitAdapters.values()].map((adapter) => `$tbuiltin${adapter.index}`),
      ...[...this.intrinsicAdapters.values()].map((adapter) => `$tintrinsic${adapter.index}`),
      ...[...this.forwardingAdapters.values()].flatMap((adapter) =>
        this.traitsByIndex
          .get(adapter.builtin.traitIndex)!
          .methods.filter((method) => this.methodIsLive(adapter.builtin.traitIndex, method.index))
          .map((method) => `$tforward${adapter.index}_${method.index}`),
      ),
    ];
  }

  private readonly forwardingAdapters = new Map<
    string,
    {
      readonly index: number;
      readonly builtin: Extract<HirBuiltinTraitImplementation, { kind: "forward" }>;
    }
  >();

  /** A dictionary for trait `builtin.traitIndex` whose methods forward to a dynamic value. */
  private emitForwardingDictionary(
    builtin: Extract<HirBuiltinTraitImplementation, { kind: "forward" }>,
    value: string,
  ): string {
    const key = JSON.stringify(builtin);
    let adapter = this.forwardingAdapters.get(key);
    if (!adapter) {
      adapter = { index: this.forwardingAdapters.size, builtin };
      this.forwardingAdapters.set(key, adapter);
    }
    const trait = this.traitsByIndex.get(builtin.traitIndex)!;
    const methods = this.liveTraitMethods(trait).map(
      (method) => `(ref.func $tforward${adapter.index}_${method.index})`,
    );
    const parents = trait.supertraits.map((parent, fieldIndex) =>
      this.emitForwardingDictionary(
        {
          ...builtin,
          traitIndex: parent.traitIndex,
          path: [...builtin.path, { traitIndex: trait.index, fieldIndex }],
        },
        "(ref.null any)",
      ),
    );
    return `(struct.new $trait${trait.index} ${value} (ref.null $hd.list)${[...methods, ...parents].map((part) => ` ${part}`).join("")})`;
  }

  /** Static `TypeId` reads use a sentinel instead of a dynamic trait receiver. */
  private emitForwardedRuntimeType(
    builtin: Extract<HirBuiltinTraitImplementation, { kind: "forward" }>,
    forwarded: string,
  ): string {
    const inner = mutableInner(builtin.targetType);
    const target = inner ?? builtin.targetType;
    const name = target.slice("trait:".length);
    const plain = this.emitStringLiteral(name);
    const nested = inner
      ? `(call ${this.stringFunction("concat")} ${this.emitStringLiteral("mut ")} ${plain})`
      : plain;
    const typeId = this.dataByName.get("TypeId")!.index;
    const resultType = `(ref null $d${typeId})`;
    const value = (key: string): string => `(struct.new $d${typeId} ${key})`;
    return `(if (result ${resultType}) (ref.is_null (local.get $self)) (then ${value(plain)}) (else (if (result ${resultType}) (ref.test (ref i31) (local.get $self)) (then ${value(nested)}) (else ${forwarded}))))`;
  }

  /** The functions behind forwarding dictionaries, one per trait method. */
  emitForwardingAdapters(): string {
    return [...this.forwardingAdapters.values()]
      .flatMap(({ index, builtin }) => {
        const trait = this.traitsByIndex.get(builtin.traitIndex)!;
        const source = `(ref.cast (ref $trait${builtin.sourceTraitIndex}) (local.get $self))`;
        const dictionary = builtin.path.reduce(
          (current, step) =>
            `(struct.get $trait${step.traitIndex} $trait${step.traitIndex}s${step.fieldIndex} ${current})`,
          source,
        );
        return this.liveTraitMethods(trait).map((method) => {
          const parameters = method.parameters.map(
            (parameter, parameterIndex) =>
              `(param $a${parameterIndex} ${this.parameterWatType(parameter)})`,
          );
          const bounds = methodBoundParameters(method, "b");
          const providers = method.requirements.map(
            (requirement, providerIndex) =>
              `(param $p${providerIndex} ${this.providerType(requirement)})`,
          );
          const result = method.suspending
            ? ` (result (ref null ${traitSuspensionName(trait.index, method.index)}))`
            : method.result === "void"
              ? ""
              : ` (result ${this.watType(method.result)})`;
          const forwarded = [
            ...method.parameters.map((_, parameterIndex) => `(local.get $a${parameterIndex})`),
            ...bounds.map((_, boundIndex) => `(local.get $b${boundIndex})`),
            ...method.requirements.map((_, providerIndex) => `(local.get $p${providerIndex})`),
          ];
          const call = `(call_ref $tsig${trait.index}_${method.index} (struct.get $trait${builtin.sourceTraitIndex} $trait${builtin.sourceTraitIndex}value ${source}) ${dictionary}${forwarded.map((part) => ` ${part}`).join("")} (struct.get $trait${trait.index} $trait${trait.index}m${method.index} ${dictionary}))`;
          const body =
            trait.standardName === STANDARD_INSPECTABLE && method.name === "runtime_type"
              ? this.emitForwardedRuntimeType(builtin, call)
              : call;
          return `(func $tforward${index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref) ${[...parameters, ...bounds, ...providers].join(" ")}${result}\n  ${body}\n)`;
        });
      })
      .join("\n\n");
  }

  // The `runtime_type` methods of compiler-supplied `Inspectable` dictionaries,
  // which build a `TypeId` from the key. This runs until no new adapter appears.
  emitBuiltinTraitAdapters(): string {
    const savedTemporaries = [...this.temporaryTypes];
    const adapters: string[] = [];
    for (let index = 0; index < this.builtinTraitAdapters.size; index += 1) {
      const adapter = [...this.builtinTraitAdapters.values()][index]!;
      const builtin = adapter.implementation;
      const trait = this.traitsByIndex.get(builtin.traitIndex)!;
      const method = trait.methods[0]!;
      this.temporaryTypes.length = 0;
      if (builtin.kind !== "inspectable")
        throw new Error(`a ${builtin.kind} dictionary has no builtin adapter`);
      // A nested read passes an i31 receiver, which no erased value is, so
      // an `outerMut` dictionary can tell it apart from `runtime_type`.
      const key = builtin.key
        .map((part) =>
          typeof part === "string"
            ? this.emitStringLiteral(part)
            : this.emitTypeIdKey(
                `(local.get $bound${part.bound})`,
                builtin.traitIndex,
                NESTED_TYPE_ID_RECEIVER,
              ),
        )
        .reduce((left, right) => `(call ${this.stringFunction("concat")} ${left} ${right})`);
      const typeId = this.dataByName.get("TypeId")!.index;
      const body = builtin.outerMut
        ? `(struct.new $d${typeId} (if (result (ref null $hd.string)) (ref.test (ref i31) (local.get $self)) (then (call ${this.stringFunction("concat")} ${this.emitStringLiteral("mut ")} ${key})) (else ${key})))`
        : `(struct.new $d${typeId} ${key})`;
      const parameters = method.parameters.map(
        (parameter, index) => `(param $a${index} ${this.parameterWatType(parameter)})`,
      );
      const result = method.result === "void" ? "" : ` (result ${this.watType(method.result)})`;
      const boundPack = `(ref.as_non_null (struct.get $trait${trait.index} $trait${trait.index}bounds (ref.cast (ref $trait${trait.index}) (local.get $dictionary))))`;
      const boundLocals = adapter.boundTraits.map(
        (traitIndex, index) => `  (local $bound${index} (ref null $trait${traitIndex}))`,
      );
      const boundSetup = adapter.boundTraits.map(
        (traitIndex, index) =>
          `  (local.set $bound${index} (ref.cast (ref null $trait${traitIndex}) (array.get $hd.list ${boundPack} (i32.const ${index}))))`,
      );
      const temporaries = this.temporaryTypes.map(
        (type, index) => `  (local $tmp${index} ${this.watType(type)})`,
      );
      adapters.push(
        [
          `(func $tbuiltin${adapter.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref)${parameters.length ? " " + parameters.join(" ") : ""}${result}`,
          ...boundLocals,
          ...temporaries,
          ...boundSetup,
          `  ${body}`,
          `)`,
        ].join("\n"),
      );
    }
    this.temporaryTypes.length = 0;
    this.temporaryTypes.push(...savedTemporaries);
    return adapters.join("\n\n");
  }

  protected emitValueEquality(
    left: string,
    right: string,
    type: ValueType,
    strategy?: HirEqualityStrategy,
  ): string {
    if (strategy?.kind === "dispatch") {
      const dispatch = strategy.dispatch;
      if (dispatch.kind === "function") return this.emitFunctionDispatch(dispatch, left, right);
      const dictionary = this.boundDispatchDictionary(dispatch);
      return `(call_ref $tsig${dispatch.traitIndex}_${dispatch.methodIndex} ${left} ${dictionary} ${right} (struct.get $trait${dispatch.traitIndex} $trait${dispatch.traitIndex}m${dispatch.methodIndex} ${dictionary}))`;
    }
    const readonly = readonlyType(type);
    if (readonly === "string") return `(call ${this.stringFunction("equal")} ${left} ${right})`;
    if (numericType(readonly) || readonly === "bool" || readonly === "char")
      return `(${scalarWasm(readonly)}.eq ${left} ${right})`;
    throw new Error(`cannot emit Eq for '${type}'`);
  }

  protected emitValueOrdering(
    left: string,
    right: string,
    type: ValueType,
    strategy?: HirOrderingStrategy,
  ): string {
    if (strategy?.kind === "dispatch") return this.emitDispatchedOrdering(left, right, strategy);
    const readonly = readonlyType(type);
    // `string_compare` returns -1, 0, or 1 already.
    if (readonly === "string") return `(call ${this.stringFunction("compare")} ${left} ${right})`;
    const numeric = numericType(readonly);
    if (numeric || readonly === "char") {
      const wasm = scalarWasm(readonly);
      const leftTemporary = this.allocateTemporary(readonly);
      const rightTemporary = this.allocateTemporary(readonly);
      const a = `(local.get ${leftTemporary})`;
      const b = `(local.get ${rightTemporary})`;
      const [less, greater] =
        numeric?.family === "float"
          ? ["lt", "gt"]
          : numeric?.family === "unsigned"
            ? ["lt_u", "gt_u"]
            : ["lt_s", "gt_s"];
      // 2 marks an unordered pair (a NaN operand); every relational operator is false for it.
      const equal =
        numeric?.family === "float"
          ? `(if (result i32) (${wasm}.eq ${a} ${b}) (then (i32.const 0)) (else (i32.const 2)))`
          : `(i32.const 0)`;
      return `(block (result i32) (local.set ${leftTemporary} ${left}) (local.set ${rightTemporary} ${right}) (if (result i32) (${wasm}.${less} ${a} ${b}) (then (i32.const -1)) (else (if (result i32) (${wasm}.${greater} ${a} ${b}) (then (i32.const 1)) (else ${equal})))))`;
    }
    throw new Error(`cannot emit PartialOrd for '${type}'`);
  }

  private emitDispatchedOrdering(
    left: string,
    right: string,
    strategy: Extract<HirOrderingStrategy, { kind: "dispatch" }>,
  ): string {
    const dispatch = strategy.dispatch;
    const called =
      dispatch.kind === "function"
        ? this.emitFunctionDispatch(dispatch, left, right)
        : `(call_ref $tsig${dispatch.traitIndex}_${dispatch.methodIndex} ${left} ${this.boundDispatchDictionary(dispatch)} ${right} (struct.get $trait${dispatch.traitIndex} $trait${dispatch.traitIndex}m${dispatch.methodIndex} ${this.boundDispatchDictionary(dispatch)}))`;
    const temporary = this.allocateTemporary("Ordering?");
    const value = `(local.get ${temporary})`;
    const present = `(struct.get $hd.variant $hd.variant-tag ${value})`;
    const orderingIndex = this.enumByName.get("Ordering")!.index;
    const ordering = `(ref.cast (ref $e${orderingIndex}) (struct.get $hd.variant $hd.variant-payload ${value}))`;
    const tag = `(struct.get $e${orderingIndex} $e${orderingIndex}tag ${ordering})`;
    return `(block (result i32) (local.set ${temporary} ${called}) (if (result i32) ${present} (then (i32.sub ${tag} (i32.const 1))) (else (i32.const 2))))`;
  }
}
