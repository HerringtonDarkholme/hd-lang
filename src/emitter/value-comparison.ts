import type {
  HirBuiltinTraitImplementation,
  HirEqualityDispatch,
  HirEqualityStrategy,
  HirExpression,
  HirOrderingStrategy,
  ValueType,
} from "../hir.ts";
import { readonlyType } from "../types.ts";
import { EmitterContext } from "./context.ts";
import { numericType } from "../numeric.ts";
import { scalarWasm } from "./scalars.ts";
import {
  emitCast,
  emitSizedBinary,
  emitSizedUnary,
  emitWiden,
  isSizedNumeric,
  type SizedNumericContext,
} from "./sized-numeric.ts";
import { functionName, methodBoundParameters, traitSuspensionName } from "./shared.ts";

/** The receiver of a nested `runtime_type` read that composes a type argument's key. */
const NESTED_TYPE_ID_RECEIVER = "(ref.i31 (i32.const 0))";

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
      ? emitSizedUnary(expression.operator, operand, expression.type)
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
      return `(i64.${operator === "<<" ? "shl" : "shr_s"} ${left} (call $hd.check_shift_i64 ${right}))`;
    if (operator === "**" && scalarWasm(expression.right.type) === "i64")
      return `(call $hd.pow_${expression.type} ${left} (i32.wrap_i64 ${right}))`;
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

  /** The map key types compared by the language's primitive `Eq`, such as `i64`. */
  private readonly primitiveKeyTypes: ValueType[] = [];

  /** The `Eq` traits whose bound dictionaries key a map (kind 3), by trait index. */
  private readonly boundKeyTraits = new Set<number>();

  /**
   * A map's key equality and key context, its last two `$hd.map` operands:
   * null for a scalar or string key (kinds 0 and 1); a wrapper of the key
   * type's `Eq` implementation, or of the primitive `Eq` of a wide integer
   * (kind 2); or, for a type-parameter key (kind 3), a wrapper that calls
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
    if (!method && numericType(type)) {
      if (!this.primitiveKeyTypes.includes(type)) this.primitiveKeyTypes.push(type);
      return `(ref.func $hd.keqp${this.primitiveKeyTypes.indexOf(type)}) (ref.null any)`;
    }
    if (!method) throw new Error(`map key type '${type}' has no Eq implementation`);
    this.keyEqualityTypes.set(method.functionIndex, type);
    return `(ref.func $hd.keq${method.functionIndex}) (ref.null any)`;
  }

  keyEqualityNames(): string[] {
    return [
      ...[...this.keyEqualityTypes.keys()].map((index) => `$hd.keq${index}`),
      ...[...this.boundKeyTraits].map((index) => `$hd.keqb${index}`),
      ...this.primitiveKeyTypes.map((_, index) => `$hd.keqp${index}`),
    ];
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
      ...this.primitiveKeyTypes.map(
        (type, index) =>
          `(func $hd.keqp${index} ${signature}\n  ${this.emitValueEquality(this.unboxValue("(local.get $left)", type), this.unboxValue("(local.get $right)", type), type, { kind: "builtin" })})`,
      ),
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

  /** The test runner hooks: exported globals the runner reads and sets. */
  protected emitTestRunnerExpression(expression: HirExpression): string | undefined {
    if (expression.kind === "each-row-index") return `(global.get $hd.each-index)`;
    if (expression.kind === "each-row-count")
      return `(global.set $hd.each-count ${this.emitExpression(expression.count)})`;
    if (expression.kind === "test-timeout")
      return `(global.set $hd.timeout-ms ${this.emitExpression(expression.millis)})`;
    return undefined;
  }

  protected emitPrimitiveDisplay(operand: string, type: ValueType): string {
    if (type === "string") return operand;
    if (["i8", "i16", "i32", "u8", "u16"].includes(type))
      return `(call $hd.i32_to_string ${operand})`;
    if (type === "u32") return `(call $hd.i64_to_string (i64.extend_i32_u ${operand}))`;
    if (type === "i64") return `(call $hd.i64_to_string ${operand})`;
    if (type === "u64") return `(call $hd.u64_to_string ${operand})`;
    if (type === "f64" || type === "f32") {
      this.floatDisplay = true;
      return `(call $hd.${type}_to_string ${operand})`;
    }
    if (type === "char") return `(call $hd.char_to_string ${operand})`;
    if (type === "bool") {
      return `(if (result (ref null $hd.bytes)) ${operand} (then (array.new_fixed $hd.bytes 4 (i32.const 116) (i32.const 114) (i32.const 117) (i32.const 101))) (else (array.new_fixed $hd.bytes 5 (i32.const 102) (i32.const 97) (i32.const 108) (i32.const 115) (i32.const 101))))`;
    }
    throw new Error(`unsupported Display operand ${type}`);
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
    if (builtin.kind === "forward") return this.emitForwardingDictionary(builtin, value);
    // An Inspectable key's every bound is an Inspectable dictionary, such as
    // a handle's witness (annot.handle.fact.key).
    const boundTraits = boundExpressions.map((bound) =>
      bound.kind === "trait-bound-dictionary"
        ? bound.traitIndex
        : builtin.kind === "inspectable"
          ? builtin.traitIndex
          : -1,
    );
    const key = JSON.stringify([builtin, boundTraits]);
    let adapter = this.builtinTraitAdapters.get(key);
    if (!adapter) {
      adapter = { index: this.builtinTraitAdapters.size, implementation: builtin, boundTraits };
      this.builtinTraitAdapters.set(key, adapter);
    }
    const boundPack =
      bounds.length > 0
        ? `(array.new_fixed $hd.list ${bounds.length} ${bounds.join(" ")})`
        : `(ref.null $hd.list)`;
    return `(struct.new $trait${trait.index} ${value} ${boundPack} (ref.func $tbuiltin${adapter.index})${parents.map((parent) => ` ${parent}`).join("")})`;
  }

  protected emitStringLiteral(text: string): string {
    const bytes = [...new TextEncoder().encode(text)];
    return bytes.length === 0
      ? `(array.new_default $hd.bytes (i32.const 0))`
      : `(array.new_fixed $hd.bytes ${bytes.length} ${bytes.map((byte) => `(i32.const ${byte})`).join(" ")})`;
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
  (if (result ${result}) (i32.eqz (call $hd.string_compare ${recorded} ${target}))
    (then (struct.new $hd.variant (i32.const 1) ${payload}))
    (else (struct.new $hd.variant (i32.const 0) (ref.null any))))
)`;
  }

  get builtinTraitAdapterNames(): readonly string[] {
    return [
      ...[...this.builtinTraitAdapters.values()].map((adapter) => `$tbuiltin${adapter.index}`),
      ...[...this.forwardingAdapters.values()].flatMap((adapter) =>
        this.traitsByIndex
          .get(adapter.builtin.traitIndex)!
          .methods.map((method) => `$tforward${adapter.index}_${method.index}`),
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
    const methods = trait.methods.map(
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
        return trait.methods.map((method) => {
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
          return `(func $tforward${index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref) ${[...parameters, ...bounds, ...providers].join(" ")}${result}\n  ${call}\n)`;
        });
      })
      .join("\n\n");
  }

  // Dictionary methods for standard-library implementations without a source
  // `impl`. Each unboxes its erased operands and runs the operator strategy.
  // An adapter's body may add adapters, as a tuple's equality adds its list
  // element's, so this runs until no new one appears.
  emitBuiltinTraitAdapters(): string {
    const savedTemporaries = [...this.temporaryTypes];
    const adapters: string[] = [];
    for (let index = 0; index < this.builtinTraitAdapters.size; index += 1) {
      const adapter = [...this.builtinTraitAdapters.values()][index]!;
      const builtin = adapter.implementation;
      const trait = this.traitsByIndex.get(builtin.traitIndex)!;
      const method = trait.methods[0]!;
      this.temporaryTypes.length = 0;
      const self = this.unboxValue(`(local.get $self)`, builtin.targetType);
      const other = () => this.unboxValue(`(local.get $a0)`, builtin.targetType);
      let body: string;
      if (builtin.kind === "display") {
        body = this.emitPrimitiveDisplay(self, builtin.targetType);
      } else if (builtin.kind === "inspectable") {
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
          .reduce((left, right) => `(call $hd.string_concat ${left} ${right})`);
        const typeId = this.dataByName.get("TypeId")!.index;
        body = builtin.outerMut
          ? `(struct.new $d${typeId} (if (result (ref null $hd.bytes)) (ref.test (ref i31) (local.get $self)) (then (call $hd.string_concat ${this.emitStringLiteral("mut ")} ${key})) (else ${key})))`
          : `(struct.new $d${typeId} ${key})`;
      } else if (builtin.kind === "equality") {
        body = this.emitValueEquality(self, other(), builtin.targetType, builtin.strategy);
      } else if (builtin.kind === "debug") {
        body = "(nop)";
      } else if (builtin.kind === "marker" || builtin.kind === "forward") {
        throw new Error(`a ${builtin.kind} dictionary has no builtin adapter`);
      } else {
        const compared = this.emitValueOrdering(
          self,
          other(),
          builtin.targetType,
          builtin.strategy,
        );
        const code = this.allocateTemporary("i32");
        const orderingIndex = this.enumByName.get("Ordering")!.index;
        const orderingType = `(ref $e${orderingIndex})`;
        const variant = (tag: number) => `(global.get $e${orderingIndex}v${tag})`;
        const ordering = `(if (result ${orderingType}) (i32.lt_s (local.get ${code}) (i32.const 0)) (then ${variant(0)}) (else (if (result ${orderingType}) (i32.eqz (local.get ${code})) (then ${variant(1)}) (else ${variant(2)}))))`;
        const resultType = this.watType(method.result);
        body =
          builtin.kind === "total-ordering"
            ? `(block (result ${resultType}) (local.set ${code} ${compared}) ${ordering})`
            : `(block (result ${resultType}) (local.set ${code} ${compared}) (if (result ${resultType}) (i32.eq (local.get ${code}) (i32.const 2)) (then (struct.new $hd.variant (i32.const 0) (ref.null any))) (else (struct.new $hd.variant (i32.const 1) ${ordering}))))`;
      }
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
    if (readonly === "string") return `(i32.eqz (call $hd.string_compare ${left} ${right}))`;
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
    if (readonly === "string") {
      const compared = `(call $hd.string_compare ${left} ${right})`;
      return `(if (result i32) (i32.lt_s ${compared} (i32.const 0)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s ${compared} (i32.const 0)) (then (i32.const 1)) (else (i32.const 0)))))`;
    }
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
