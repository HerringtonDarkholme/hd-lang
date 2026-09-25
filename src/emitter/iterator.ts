import type {
  HirComprehensionClause,
  HirComprehensionForClause,
  HirExpression,
  HirStatement,
  ValueType,
} from "../hir.ts";
import { mutableType, nominalGenericType } from "../types.ts";
import { functionName, indent, localName } from "./shared.ts";
import { ValueComparisonEmitter } from "./value-comparison.ts";

type IteratorSourceKind = "list" | "map";

export abstract class IteratorEmitter extends ValueComparisonEmitter {
  protected abstract emitExpression(expression: HirExpression): string;
  protected abstract boxValue(expression: HirExpression, type: ValueType): string;
  protected abstract emitBlock(
    statements: readonly HirStatement[],
    result: ValueType,
    loopBoundary?: boolean,
  ): string;

  protected emitListIterator(receiver: HirExpression): string {
    const listTemporary = this.allocateTemporary(receiver.type);
    const list = `(ref.as_non_null (local.get ${listTemporary}))`;
    return [
      `(block (result (ref null $hd.iterator))`,
      `  (local.set ${listTemporary} ${this.emitExpression(receiver)})`,
      `  (struct.new $hd.iterator`,
      `    ${list}`,
      `    (ref.null $hd.map)`,
      `    (i32.const 0)`,
      `    (struct.get $hd.vector $hd.vector-version ${list}))`,
      `)`,
    ].join("\n");
  }

  protected emitMapIterator(receiver: HirExpression): string {
    const mapTemporary = this.allocateTemporary(receiver.type);
    const map = `(ref.as_non_null (local.get ${mapTemporary}))`;
    return [
      `(block (result (ref null $hd.iterator))`,
      `  (local.set ${mapTemporary} ${this.emitExpression(receiver)})`,
      `  (struct.new $hd.iterator`,
      `    (ref.null $hd.vector)`,
      `    ${map}`,
      `    (i32.const 0)`,
      `    (struct.get $hd.map $hd.map-version ${map}))`,
      `)`,
    ].join("\n");
  }

  protected emitIteratorNext(receiver: HirExpression, elementType: ValueType): string {
    return this.emitIteratorNextValue(this.emitExpression(receiver), receiver.type, elementType);
  }

  protected emitForExpression(expression: Extract<HirExpression, { kind: "for" }>): string {
    const id = this.loopCounter++;
    const labels = {
      breakLabel: `$break${id}`,
      continueLabel: `$continue${id}`,
      result: expression.elseBody.length > 0 ? expression.type : undefined,
    };
    const iteratorType =
      expression.iteratorKind === "trait"
        ? expression.iterable.type
        : mutableType(nominalGenericType("Iterator", [expression.yieldType]));
    const iterator = this.allocateTemporary(iteratorType);
    const next = this.allocateTemporary(`${expression.yieldType}?`);
    const yielded = this.allocateTemporary(expression.yieldType);
    const iteratorValue = `(local.get ${iterator})`;
    const nextValue = `(ref.as_non_null (local.get ${next}))`;
    const initialize =
      expression.iteratorKind === "list"
        ? this.emitListIterator(expression.iterable)
        : expression.iteratorKind === "map"
          ? this.emitMapIterator(expression.iterable)
          : this.emitExpression(expression.iterable);
    const nextCall =
      expression.iteratorKind === "trait"
        ? `(call ${functionName(expression.iteratorFunctionIndex!)} ${iteratorValue})`
        : this.emitIteratorNextValue(iteratorValue, iteratorType, expression.yieldType);
    const bind =
      expression.bindings.length === 1
        ? [`(local.set ${localName(expression.bindings[0]!.index)} (local.get ${yielded}))`]
        : expression.bindings.map(
            (binding, bindingIndex) =>
              `(local.set ${localName(binding.index)} ${this.unboxValue(`(array.get $hd.list (ref.as_non_null (local.get ${yielded})) (i32.const ${bindingIndex}))`, binding.type)})`,
          );
    this.loops.push(labels);
    const body = this.emitBlock(expression.body, "void", true);
    this.loops.pop();
    const exhausted =
      expression.elseBody.length > 0
        ? [
            `(if (i32.eqz (struct.get $hd.variant $hd.variant-tag ${nextValue}))`,
            `  (then`,
            indent(this.emitBlock(expression.elseBody, expression.type), 4),
            `    (br ${labels.breakLabel})`,
            `  ))`,
          ]
        : [
            `(br_if ${labels.breakLabel} (i32.eqz (struct.get $hd.variant $hd.variant-tag ${nextValue})))`,
          ];
    const iteration = [
      `(local.set ${next} ${nextCall})`,
      ...exhausted,
      `(local.set ${yielded} ${this.unboxValue(`(struct.get $hd.variant $hd.variant-payload ${nextValue})`, expression.yieldType)})`,
      ...bind,
      `(block ${labels.continueLabel}`,
      indent(body, 2),
      `)`,
      `(br $loop${id})`,
    ];
    const result =
      expression.type === "void" || expression.type === "never"
        ? ""
        : ` (result ${this.watType(expression.type)})`;
    return [
      `(block ${labels.breakLabel}${expression.elseBody.length > 0 ? result : ""}`,
      `  (local.set ${iterator} ${initialize})`,
      `  (loop $loop${id}`,
      indent(iteration.join("\n"), 4),
      `  )`,
      ...(expression.elseBody.length > 0 ? [`  unreachable`] : []),
      `)`,
    ].join("\n");
  }

  protected emitComprehensionExpression(
    expression: Extract<HirExpression, { kind: "list-comprehension" | "map-comprehension" }>,
  ): string {
    const result = this.allocateTemporary(expression.type);
    const resultValue = `(ref.as_non_null (local.get ${result}))`;
    const initialize =
      expression.kind === "list-comprehension"
        ? `(struct.new $hd.vector (i32.const 0) (array.new_default $hd.list (i32.const 0)) (i32.const 0))`
        : [
            `(struct.new $hd.map`,
            `  (i32.const ${expression.keyKind})`,
            `  (i32.const 0)`,
            `  (array.new_default $hd.list (i32.const 0))`,
            `  (array.new_default $hd.list (i32.const 0))`,
            `  (i32.const 0))`,
          ].join("\n");
    const append =
      expression.kind === "list-comprehension"
        ? `(call $hd.vector_append ${resultValue} ${this.boxValue(expression.value, expression.elementType)})`
        : [
            `(call $hd.map_insert`,
            `  ${resultValue}`,
            `  ${this.boxValue(expression.key, expression.keyType)}`,
            `  ${this.boxValue(expression.value, expression.valueType)})`,
          ].join("\n");
    const body = this.emitComprehensionClauses(expression.clauses, 0, append);
    const resultType = expression.kind === "list-comprehension" ? "$hd.vector" : "$hd.map";
    return [
      `(block (result (ref null ${resultType}))`,
      `  (local.set ${result}`,
      indent(initialize, 4),
      `  )`,
      indent(body, 2),
      `  (local.get ${result})`,
      `)`,
    ].join("\n");
  }

  private emitComprehensionClauses(
    clauses: readonly HirComprehensionClause[],
    index: number,
    append: string,
  ): string {
    const clause = clauses[index];
    if (!clause) return append;
    const rest = this.emitComprehensionClauses(clauses, index + 1, append);
    if (clause.kind === "if")
      return [
        `(if ${this.emitExpression(clause.condition)}`,
        `  (then`,
        indent(rest, 4),
        `  ))`,
      ].join("\n");
    return this.emitComprehensionForClause(clause, rest);
  }

  private emitComprehensionForClause(clause: HirComprehensionForClause, body: string): string {
    const id = this.loopCounter++;
    const iteratorType =
      clause.iteratorKind === "trait"
        ? clause.iterable.type
        : mutableType(nominalGenericType("Iterator", [clause.yieldType]));
    const iterator = this.allocateTemporary(iteratorType);
    const next = this.allocateTemporary(`${clause.yieldType}?`);
    const yielded = this.allocateTemporary(clause.yieldType);
    const iteratorValue = `(local.get ${iterator})`;
    const nextValue = `(ref.as_non_null (local.get ${next}))`;
    const initialize =
      clause.iteratorKind === "list"
        ? this.emitListIterator(clause.iterable)
        : clause.iteratorKind === "map"
          ? this.emitMapIterator(clause.iterable)
          : this.emitExpression(clause.iterable);
    const nextCall =
      clause.iteratorKind === "trait"
        ? `(call ${functionName(clause.iteratorFunctionIndex!)} ${iteratorValue})`
        : this.emitIteratorNextValue(iteratorValue, iteratorType, clause.yieldType);
    const bind =
      clause.bindings.length === 1
        ? [`(local.set ${localName(clause.bindings[0]!.index)} (local.get ${yielded}))`]
        : clause.bindings.map(
            (binding, bindingIndex) =>
              `(local.set ${localName(binding.index)} ${this.unboxValue(`(array.get $hd.list (ref.as_non_null (local.get ${yielded})) (i32.const ${bindingIndex}))`, binding.type)})`,
          );
    return [
      `(block $comprehension-exit${id}`,
      `  (local.set ${iterator} ${initialize})`,
      `  (loop $comprehension-loop${id}`,
      `    (local.set ${next} ${nextCall})`,
      `    (br_if $comprehension-exit${id} (i32.eqz (struct.get $hd.variant $hd.variant-tag ${nextValue})))`,
      `    (local.set ${yielded} ${this.unboxValue(`(struct.get $hd.variant $hd.variant-payload ${nextValue})`, clause.yieldType)})`,
      ...bind.map((line) => `    ${line}`),
      indent(body, 4),
      `    (br $comprehension-loop${id})`,
      `  )`,
      `)`,
    ].join("\n");
  }

  protected emitIteratorNextValue(
    receiverValue: string,
    receiverType: ValueType,
    elementType: ValueType,
  ): string {
    const iteratorTemporary = this.allocateTemporary(receiverType);
    const resultTemporary = this.allocateTemporary(`${elementType}?`);
    const iterator = `(ref.as_non_null (local.get ${iteratorTemporary}))`;
    const list = `(struct.get $hd.iterator $hd.iterator-list ${iterator})`;
    return [
      `(block (result (ref null $hd.variant))`,
      `  (local.set ${iteratorTemporary} ${receiverValue})`,
      `  (if (result (ref null $hd.variant))`,
      `    (ref.is_null ${list})`,
      `    (then ${this.emitIteratorSourceNext(iterator, resultTemporary, "map")})`,
      `    (else ${this.emitIteratorSourceNext(iterator, resultTemporary, "list")}))`,
      `)`,
    ].join("\n");
  }

  private emitIteratorSourceNext(
    iterator: string,
    resultTemporary: string,
    kind: IteratorSourceKind,
  ): string {
    const prefix = kind === "list" ? "$hd.vector" : "$hd.map";
    const sourceField = kind === "list" ? "$hd.iterator-list" : "$hd.iterator-map";
    const source = `(ref.as_non_null (struct.get $hd.iterator ${sourceField} ${iterator}))`;
    const version = `(struct.get ${prefix} ${prefix}-version ${source})`;
    const size = `(struct.get ${prefix} ${prefix}-size ${source})`;
    const index = `(struct.get $hd.iterator $hd.iterator-index ${iterator})`;
    const payload =
      kind === "list"
        ? `(call $hd.vector_get ${source} ${index})`
        : `(array.new_fixed $hd.list 2 (array.get $hd.list (struct.get $hd.map $hd.map-keys ${source}) ${index}) (array.get $hd.list (struct.get $hd.map $hd.map-values ${source}) ${index}))`;
    return [
      `(block (result (ref null $hd.variant))`,
      `  (if`,
      `    (i32.ne ${version} (struct.get $hd.iterator $hd.iterator-version ${iterator}))`,
      `    (then ${this.emitRuntimePanic("iterator-invalidated")}))`,
      `  (if (result (ref null $hd.variant))`,
      `    (i32.ge_u ${index} ${size})`,
      `    (then (struct.new $hd.variant (i32.const 0) (ref.null any)))`,
      `    (else`,
      `      (local.set ${resultTemporary}`,
      `        (struct.new $hd.variant (i32.const 1) ${payload}))`,
      `      (struct.set $hd.iterator $hd.iterator-index ${iterator}`,
      `        (i32.add ${index} (i32.const 1)))`,
      `      (local.get ${resultTemporary})))`,
      `)`,
    ].join("\n");
  }
}
