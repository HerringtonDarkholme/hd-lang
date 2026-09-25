import type { HirEqualityStrategy, HirOrderingStrategy, ValueType } from "../hir.ts";
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
    if (readonly === "f64")
      return `(if (result i32) (f64.lt ${left} ${right}) (then (i32.const -1)) (else (if (result i32) (f64.gt ${left} ${right}) (then (i32.const 1)) (else (if (result i32) (f64.eq ${left} ${right}) (then (i32.const 0)) (else (i32.const 2))))))`;
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
