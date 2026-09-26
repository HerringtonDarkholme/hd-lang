import type {
  HirBuiltinTraitImplementation,
  HirEqualityStrategy,
  HirExpression,
  HirOrderingStrategy,
  ValueType,
} from "../hir.ts";
import {
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
} from "../types.ts";
import { EmitterContext } from "./context.ts";
import { functionName } from "./shared.ts";

export abstract class ValueComparisonEmitter extends EmitterContext {
  protected allocateTemporary(type: ValueType): string {
    const index = this.temporaryTypes.length;
    this.temporaryTypes.push(type);
    return `$tmp${index}`;
  }

  protected emitPrimitiveDisplay(operand: string, type: ValueType): string {
    if (type === "string") return operand;
    if (type === "i32") return `(call $hd.i32_to_string ${operand})`;
    if (type === "f64") {
      this.floatDisplay = true;
      return `(call $hd.f64_to_string ${operand})`;
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
  ): string {
    const trait = this.traitsByIndex.get(builtin.traitIndex)!;
    const boundTraits = boundExpressions.map((bound) =>
      bound.kind === "trait-bound-dictionary" ? bound.traitIndex : -1,
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
    return `(struct.new $trait${trait.index} ${value} ${boundPack} (ref.func $tbuiltin${adapter.index}))`;
  }

  get builtinTraitAdapterNames(): readonly string[] {
    return [...this.builtinTraitAdapters.values()].map((adapter) => `$tbuiltin${adapter.index}`);
  }

  // Dictionary methods for standard-library implementations without a source
  // `impl`. Each unboxes its erased operands and runs the operator strategy.
  emitBuiltinTraitAdapters(): string {
    const savedTemporaries = [...this.temporaryTypes];
    const adapters = [...this.builtinTraitAdapters.values()].map((adapter) => {
      const builtin = adapter.implementation;
      const trait = this.traitsByIndex.get(builtin.traitIndex)!;
      const method = trait.methods[0]!;
      this.temporaryTypes.length = 0;
      const self = this.unboxValue(`(local.get $self)`, builtin.targetType);
      const other = () => this.unboxValue(`(local.get $a0)`, builtin.targetType);
      let body: string;
      if (builtin.kind === "display") {
        body = this.emitPrimitiveDisplay(self, builtin.targetType);
      } else if (builtin.kind === "equality") {
        body = this.emitValueEquality(self, other(), builtin.targetType, builtin.strategy);
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
        body = `(block (result ${resultType}) (local.set ${code} ${compared}) (if (result ${resultType}) (i32.eq (local.get ${code}) (i32.const 2)) (then (struct.new $hd.variant (i32.const 0) (ref.null any))) (else (struct.new $hd.variant (i32.const 1) ${ordering}))))`;
      }
      const parameters = method.parameters.map(
        (parameter, index) => `(param $a${index} ${this.watType(parameter)})`,
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
      return [
        `(func $tbuiltin${adapter.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref)${parameters.length ? " " + parameters.join(" ") : ""}${result}`,
        ...boundLocals,
        ...temporaries,
        ...boundSetup,
        `  ${body}`,
        `)`,
      ].join("\n");
    });
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
      if (dispatch.kind === "function")
        return `(call ${functionName(dispatch.functionIndex)} ${left} ${right})`;
      return `(call_ref $tsig${dispatch.traitIndex}_${dispatch.methodIndex} ${left} (local.get $bound${dispatch.boundIndex}) ${right} (struct.get $trait${dispatch.traitIndex} $trait${dispatch.traitIndex}m${dispatch.methodIndex} (local.get $bound${dispatch.boundIndex})))`;
    }
    const readonly = readonlyType(type);
    if (readonly === "string") return `(i32.eqz (call $hd.string_compare ${left} ${right}))`;
    if (readonly === "f64") return `(f64.eq ${left} ${right})`;
    if (readonly === "i32" || readonly === "bool" || readonly === "char")
      return `(i32.eq ${left} ${right})`;
    const tuple = tupleParts(readonly);
    if (tuple !== undefined)
      return this.emitTupleEquality(
        left,
        right,
        readonly,
        tuple,
        strategy?.kind === "tuple" ? strategy.elements : undefined,
      );
    const optional = optionalInner(readonly);
    if (optional !== undefined)
      return this.emitOptionalEquality(
        left,
        right,
        readonly,
        optional,
        strategy?.kind === "optional" ? strategy.value : undefined,
      );
    const result = resultParts(readonly);
    if (result)
      return this.emitVariantEquality(
        left,
        right,
        readonly,
        result.ok,
        result.error,
        strategy?.kind === "result" ? strategy.ok : undefined,
        strategy?.kind === "result" ? strategy.error : undefined,
      );
    const nominal = nominalGenericParts(readonly);
    if (nominal?.name === "list" && nominal.arguments.length === 1)
      return this.emitListEquality(
        left,
        right,
        readonly,
        nominal.arguments[0]!,
        strategy?.kind === "list" ? strategy.element : undefined,
      );
    if (nominal?.name === "map" && nominal.arguments.length === 2)
      return this.emitMapEquality(
        left,
        right,
        readonly,
        nominal.arguments[1]!,
        strategy?.kind === "map" ? strategy.value : undefined,
      );
    throw new Error(`cannot emit PartialEq for '${type}'`);
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
    if (readonly === "i32" || readonly === "char")
      return `(if (result i32) (i32.lt_s ${left} ${right}) (then (i32.const -1)) (else (if (result i32) (i32.gt_s ${left} ${right}) (then (i32.const 1)) (else (i32.const 0)))))`;
    if (readonly === "f64") {
      // 2 marks an unordered pair (a NaN operand); every relational operator is false for it.
      const leftTemporary = this.allocateTemporary("f64");
      const rightTemporary = this.allocateTemporary("f64");
      const a = `(local.get ${leftTemporary})`;
      const b = `(local.get ${rightTemporary})`;
      return `(block (result i32) (local.set ${leftTemporary} ${left}) (local.set ${rightTemporary} ${right}) (if (result i32) (f64.lt ${a} ${b}) (then (i32.const -1)) (else (if (result i32) (f64.gt ${a} ${b}) (then (i32.const 1)) (else (if (result i32) (f64.eq ${a} ${b}) (then (i32.const 0)) (else (i32.const 2))))))))`;
    }
    const tuple = tupleParts(readonly);
    if (tuple !== undefined)
      return this.emitTupleOrdering(
        left,
        right,
        readonly,
        tuple,
        strategy?.kind === "tuple" ? strategy.elements : undefined,
      );
    const optional = optionalInner(readonly);
    if (optional !== undefined)
      return this.emitOptionalOrdering(
        left,
        right,
        readonly,
        optional,
        strategy?.kind === "optional" ? strategy.value : undefined,
      );
    const nominal = nominalGenericParts(readonly);
    if (nominal?.name === "list" && nominal.arguments.length === 1)
      return this.emitListOrdering(
        left,
        right,
        readonly,
        nominal.arguments[0]!,
        strategy?.kind === "list" ? strategy.element : undefined,
      );
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
        ? `(call ${functionName(dispatch.functionIndex)} ${left} ${right})`
        : `(call_ref $tsig${dispatch.traitIndex}_${dispatch.methodIndex} ${left} (local.get $bound${dispatch.boundIndex}) ${right} (struct.get $trait${dispatch.traitIndex} $trait${dispatch.traitIndex}m${dispatch.methodIndex} (local.get $bound${dispatch.boundIndex})))`;
    const temporary = this.allocateTemporary("Ordering?");
    const value = `(local.get ${temporary})`;
    const present = `(struct.get $hd.variant $hd.variant-tag ${value})`;
    const orderingIndex = this.enumByName.get("Ordering")!.index;
    const ordering = `(ref.cast (ref $e${orderingIndex}) (struct.get $hd.variant $hd.variant-payload ${value}))`;
    const tag = `(struct.get $e${orderingIndex} $e${orderingIndex}tag ${ordering})`;
    return `(block (result i32) (local.set ${temporary} ${called}) (if (result i32) ${present} (then (i32.sub ${tag} (i32.const 1))) (else (i32.const 2))))`;
  }

  private emitTupleEquality(
    left: string,
    right: string,
    type: ValueType,
    elements: readonly ValueType[],
    strategies?: readonly HirEqualityStrategy[],
  ): string {
    if (elements.length === 0) return `(i32.const 1)`;
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const comparisons = elements.map((elementType, index) =>
      this.emitValueEquality(
        this.unboxValue(
          `(array.get $hd.list (ref.as_non_null (local.get ${leftTemporary})) (i32.const ${index}))`,
          elementType,
        ),
        this.unboxValue(
          `(array.get $hd.list (ref.as_non_null (local.get ${rightTemporary})) (i32.const ${index}))`,
          elementType,
        ),
        elementType,
        strategies?.[index],
      ),
    );
    const comparison = comparisons
      .slice(1)
      .reduce((combined, next) => `(i32.and ${combined} ${next})`, comparisons[0]!);
    return `(block (result i32) (local.set ${leftTemporary} ${left}) (local.set ${rightTemporary} ${right}) ${comparison})`;
  }

  private emitTupleOrdering(
    left: string,
    right: string,
    type: ValueType,
    elements: readonly ValueType[],
    strategies?: readonly HirOrderingStrategy[],
  ): string {
    if (elements.length === 0) return `(i32.const 0)`;
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const comparisonTemporary = this.allocateTemporary("i32");
    const label = `$ordering${this.loopCounter++}`;
    const comparisons = elements.flatMap((elementType, index) => {
      const leftElement = this.unboxValue(
        `(array.get $hd.list (ref.as_non_null (local.get ${leftTemporary})) (i32.const ${index}))`,
        elementType,
      );
      const rightElement = this.unboxValue(
        `(array.get $hd.list (ref.as_non_null (local.get ${rightTemporary})) (i32.const ${index}))`,
        elementType,
      );
      return [
        `  (local.set ${comparisonTemporary} ${this.emitValueOrdering(leftElement, rightElement, elementType, strategies?.[index])})`,
        `  (if (i32.ne (local.get ${comparisonTemporary}) (i32.const 0))`,
        `    (then (br ${label} (local.get ${comparisonTemporary}))))`,
      ];
    });
    return [
      `(block ${label} (result i32)`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      ...comparisons,
      `  (i32.const 0)`,
      `)`,
    ].join("\n");
  }

  private emitVariantEquality(
    left: string,
    right: string,
    type: ValueType,
    zeroType: ValueType,
    oneType: ValueType,
    zeroStrategy?: HirEqualityStrategy,
    oneStrategy?: HirEqualityStrategy,
  ): string {
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const leftValue = `(local.get ${leftTemporary})`;
    const rightValue = `(local.get ${rightTemporary})`;
    const leftTag = `(struct.get $hd.variant $hd.variant-tag ${leftValue})`;
    const rightTag = `(struct.get $hd.variant $hd.variant-tag ${rightValue})`;
    const payload = (value: string, payloadType: ValueType): string =>
      this.unboxValue(`(struct.get $hd.variant $hd.variant-payload ${value})`, payloadType);
    const zeroEquality = this.emitValueEquality(
      payload(leftValue, zeroType),
      payload(rightValue, zeroType),
      zeroType,
      zeroStrategy,
    );
    const oneEquality = this.emitValueEquality(
      payload(leftValue, oneType),
      payload(rightValue, oneType),
      oneType,
      oneStrategy,
    );
    return [
      `(block (result i32)`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (if (result i32) (i32.eq ${leftTag} ${rightTag})`,
      `    (then (if (result i32) (i32.eqz ${leftTag}) (then ${zeroEquality}) (else ${oneEquality})))`,
      `    (else (i32.const 0))))`,
    ].join("\n");
  }

  private emitOptionalEquality(
    left: string,
    right: string,
    type: ValueType,
    payloadType: ValueType,
    strategy?: HirEqualityStrategy,
  ): string {
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const leftValue = `(local.get ${leftTemporary})`;
    const rightValue = `(local.get ${rightTemporary})`;
    const leftTag = `(struct.get $hd.variant $hd.variant-tag ${leftValue})`;
    const rightTag = `(struct.get $hd.variant $hd.variant-tag ${rightValue})`;
    const leftPayload = this.unboxValue(
      `(struct.get $hd.variant $hd.variant-payload ${leftValue})`,
      payloadType,
    );
    const rightPayload = this.unboxValue(
      `(struct.get $hd.variant $hd.variant-payload ${rightValue})`,
      payloadType,
    );
    const payloadEquality = this.emitValueEquality(
      leftPayload,
      rightPayload,
      payloadType,
      strategy,
    );
    return [
      `(block (result i32)`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (if (result i32) (i32.eq ${leftTag} ${rightTag})`,
      `    (then (if (result i32) (i32.eqz ${leftTag}) (then (i32.const 1)) (else ${payloadEquality})))`,
      `    (else (i32.const 0))))`,
    ].join("\n");
  }

  private emitListEquality(
    left: string,
    right: string,
    type: ValueType,
    elementType: ValueType,
    strategy?: HirEqualityStrategy,
  ): string {
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const indexTemporary = this.allocateTemporary("i32");
    const label = `$equality${this.loopCounter++}`;
    const leftElement = this.unboxValue(
      `(call $hd.vector_get (ref.as_non_null (local.get ${leftTemporary})) (local.get ${indexTemporary}))`,
      elementType,
    );
    const rightElement = this.unboxValue(
      `(call $hd.vector_get (ref.as_non_null (local.get ${rightTemporary})) (local.get ${indexTemporary}))`,
      elementType,
    );
    return [
      `(block ${label} (result i32)`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (if (i32.ne`,
      `      (struct.get $hd.vector $hd.vector-size (ref.as_non_null (local.get ${leftTemporary})))`,
      `      (struct.get $hd.vector $hd.vector-size (ref.as_non_null (local.get ${rightTemporary}))))`,
      `    (then (br ${label} (i32.const 0))))`,
      `  (loop $${label.slice(1)}loop`,
      `    (br_if ${label} (i32.const 1)`,
      `      (i32.ge_u (local.get ${indexTemporary}) (struct.get $hd.vector $hd.vector-size (ref.as_non_null (local.get ${leftTemporary})))))`,
      `    (if (i32.eqz ${this.emitValueEquality(leftElement, rightElement, elementType, strategy)})`,
      `      (then (br ${label} (i32.const 0))))`,
      `    (local.set ${indexTemporary} (i32.add (local.get ${indexTemporary}) (i32.const 1)))`,
      `    (br $${label.slice(1)}loop))`,
      `  (i32.const 1)`,
      `)`,
    ].join("\n");
  }

  private emitOptionalOrdering(
    left: string,
    right: string,
    type: ValueType,
    payloadType: ValueType,
    strategy?: HirOrderingStrategy,
  ): string {
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const leftValue = `(local.get ${leftTemporary})`;
    const rightValue = `(local.get ${rightTemporary})`;
    const leftTag = `(struct.get $hd.variant $hd.variant-tag ${leftValue})`;
    const rightTag = `(struct.get $hd.variant $hd.variant-tag ${rightValue})`;
    const leftPayload = this.unboxValue(
      `(struct.get $hd.variant $hd.variant-payload ${leftValue})`,
      payloadType,
    );
    const rightPayload = this.unboxValue(
      `(struct.get $hd.variant $hd.variant-payload ${rightValue})`,
      payloadType,
    );
    return [
      `(block (result i32)`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (if (result i32) (i32.ne ${leftTag} ${rightTag})`,
      `    (then (if (result i32) (i32.lt_u ${leftTag} ${rightTag}) (then (i32.const -1)) (else (i32.const 1))))`,
      `    (else (if (result i32) (i32.eqz ${leftTag}) (then (i32.const 0))`,
      `      (else ${this.emitValueOrdering(leftPayload, rightPayload, payloadType, strategy)}))))`,
      `)`,
    ].join("\n");
  }

  private emitListOrdering(
    left: string,
    right: string,
    type: ValueType,
    elementType: ValueType,
    strategy?: HirOrderingStrategy,
  ): string {
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const indexTemporary = this.allocateTemporary("i32");
    const comparisonTemporary = this.allocateTemporary("i32");
    const label = `$ordering${this.loopCounter++}`;
    const loop = `$${label.slice(1)}loop`;
    const leftList = `(ref.as_non_null (local.get ${leftTemporary}))`;
    const rightList = `(ref.as_non_null (local.get ${rightTemporary}))`;
    const leftSize = `(struct.get $hd.vector $hd.vector-size ${leftList})`;
    const rightSize = `(struct.get $hd.vector $hd.vector-size ${rightList})`;
    const leftElement = this.unboxValue(
      `(call $hd.vector_get ${leftList} (local.get ${indexTemporary}))`,
      elementType,
    );
    const rightElement = this.unboxValue(
      `(call $hd.vector_get ${rightList} (local.get ${indexTemporary}))`,
      elementType,
    );
    const sizeOrdering = `(if (result i32) (i32.lt_u ${leftSize} ${rightSize}) (then (i32.const -1)) (else (if (result i32) (i32.gt_u ${leftSize} ${rightSize}) (then (i32.const 1)) (else (i32.const 0)))))`;
    return [
      `(block ${label} (result i32)`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (loop ${loop}`,
      `    (if (i32.or (i32.ge_u (local.get ${indexTemporary}) ${leftSize}) (i32.ge_u (local.get ${indexTemporary}) ${rightSize}))`,
      `      (then (br ${label} ${sizeOrdering})))`,
      `    (local.set ${comparisonTemporary} ${this.emitValueOrdering(leftElement, rightElement, elementType, strategy)})`,
      `    (if (i32.ne (local.get ${comparisonTemporary}) (i32.const 0))`,
      `      (then (br ${label} (local.get ${comparisonTemporary}))))`,
      `    (local.set ${indexTemporary} (i32.add (local.get ${indexTemporary}) (i32.const 1)))`,
      `    (br ${loop}))`,
      `  (i32.const 0)`,
      `)`,
    ].join("\n");
  }

  private emitMapEquality(
    left: string,
    right: string,
    type: ValueType,
    valueType: ValueType,
    strategy?: HirEqualityStrategy,
  ): string {
    const leftTemporary = this.allocateTemporary(type);
    const rightTemporary = this.allocateTemporary(type);
    const indexTemporary = this.allocateTemporary("i32");
    const foundTemporary = this.allocateTemporary(`${valueType}?`);
    const label = `$equality${this.loopCounter++}`;
    const leftMap = `(ref.as_non_null (local.get ${leftTemporary}))`;
    const rightMap = `(ref.as_non_null (local.get ${rightTemporary}))`;
    const key = `(array.get $hd.list (struct.get $hd.map $hd.map-keys ${leftMap}) (local.get ${indexTemporary}))`;
    const leftValue = this.unboxValue(
      `(array.get $hd.list (struct.get $hd.map $hd.map-values ${leftMap}) (local.get ${indexTemporary}))`,
      valueType,
    );
    const rightValue = this.unboxValue(
      `(struct.get $hd.variant $hd.variant-payload (local.get ${foundTemporary}))`,
      valueType,
    );
    return [
      `(block ${label} (result i32)`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (if (i32.ne (struct.get $hd.map $hd.map-size ${leftMap}) (struct.get $hd.map $hd.map-size ${rightMap}))`,
      `    (then (br ${label} (i32.const 0))))`,
      `  (loop $${label.slice(1)}loop`,
      `    (br_if ${label} (i32.const 1)`,
      `      (i32.ge_u (local.get ${indexTemporary}) (struct.get $hd.map $hd.map-size ${leftMap})))`,
      `    (local.set ${foundTemporary} (call $hd.map_get ${rightMap} ${key}))`,
      `    (if (i32.eqz (struct.get $hd.variant $hd.variant-tag (local.get ${foundTemporary})))`,
      `      (then (br ${label} (i32.const 0))))`,
      `    (if (i32.eqz ${this.emitValueEquality(leftValue, rightValue, valueType, strategy)})`,
      `      (then (br ${label} (i32.const 0))))`,
      `    (local.set ${indexTemporary} (i32.add (local.get ${indexTemporary}) (i32.const 1)))`,
      `    (br $${label.slice(1)}loop))`,
      `  (i32.const 1)`,
      `)`,
    ].join("\n");
  }
}
