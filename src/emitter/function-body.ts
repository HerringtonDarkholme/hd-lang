import { cellInner } from "../checker/captured-cells.ts";
import { physicalPosition } from "../diagnostics.ts";
import type {
  HirExpression,
  HirDefaultArgument,
  HirFunction,
  HirLocal,
  HirMatchArm,
  HirPatternPathStep,
  HirProviderContextEntry,
  HirStatement,
  HirTraitDictionaryPlan,
  HirTypeSubstitution,
  ValueType,
} from "../hir.ts";
import {
  functionParts,
  optionalInner,
  readonlyType,
  suspensionParts,
  traitSuspensionParts,
} from "../types.ts";
import type { CleanupFrame, EmittedArguments } from "./context.ts";
import {
  functionName,
  globalName,
  indent,
  isGenericValueType,
  localName,
  suspensionWrapperCancelAdapterName,
  suspensionWrapperPollAdapterName,
  suspensionWrapperResultAdapterName,
  traitSuspensionCancelName,
  traitSuspensionDriveName,
  traitSuspensionName,
  traitSuspensionPollName,
  traitSuspensionResultName,
  traitSuspensionWrapperCancelAdapterName,
  traitSuspensionWrapperPollAdapterName,
  traitSuspensionWrapperResultAdapterName,
  traitTypeBase,
  andThen,
  matchTestTag,
  stringLiteral,
} from "./shared.ts";
import { CallableAdapterEmitter } from "./callable-adapters.ts";
import { scalarWasm } from "./scalars.ts";
import { integerConstant, powerFunction, shiftCount } from "./sized-numeric.ts";
import { numericType } from "../numeric.ts";

/** The reads of a stored value, which give a `void` slot's null (`slotWatType`). */
const VOID_SLOT_READS: ReadonlySet<HirExpression["kind"]> = new Set([
  "variant-payload",
  "list-index",
  "tuple-index",
  "map-index",
  "map-entry-key",
  "map-entry-value",
  "member",
  "enum-member",
  "cell-get",
]);

export abstract class FunctionBodyEmitter extends CallableAdapterEmitter {
  protected abstract emitSuspensionFrameStores(
    declaration: HirFunction,
    storedLocals?: readonly HirLocal[],
  ): string[];
  protected emitBlock(
    statements: readonly HirStatement[],
    result: ValueType,
    loopBoundary = false,
  ): string {
    const frame = { cleanups: [] as Array<readonly HirStatement[]>, loopBoundary };
    this.cleanupFrames.push(frame);
    const emitted: string[] = [];
    let resultTemporary: string | undefined;
    statements.forEach((statement, index) => {
      const final = index === statements.length - 1;
      if (statement.kind === "defer") {
        frame.cleanups.push(statement.body);
        emitted.push("(nop)");
        return;
      }
      if (
        final &&
        result !== "void" &&
        statement.kind === "expression" &&
        frame.cleanups.length > 0
      ) {
        resultTemporary = this.allocateTemporary(result);
        emitted.push(`(local.set ${resultTemporary} ${this.emitExpression(statement.expression)})`);
        return;
      }
      emitted.push(this.emitStatement(statement, final ? result : "void"));
    });
    emitted.push(...this.emitFrameCleanups(frame));
    if (resultTemporary) emitted.push(`(local.get ${resultTemporary})`);
    this.cleanupFrames.pop();
    return emitted.join("\n");
  }

  protected emitStatement(statement: HirStatement, expected: ValueType): string {
    switch (statement.kind) {
      case "defer":
        throw new Error("defer registration must be emitted by its containing block");
      case "binding":
      case "assignment":
        return statement.local.type === "void"
          ? this.emitExpression(statement.value)
          : `(local.set ${localName(statement.local.index)} ${this.emitExpression(statement.value)})`;
      case "global-binding":
      case "global-assignment":
        return statement.global.type === "void"
          ? this.emitExpression(statement.value)
          : `(global.set ${globalName(statement.global.index)} ${this.emitExpression(statement.value)})`;
      case "discard": {
        const value = this.emitExpression(statement.value);
        return statement.value.type === "void" || statement.value.type === "never"
          ? value
          : `(drop ${value})`;
      }
      case "return":
        // A `void` value runs before the cleanups, and the return carries nothing.
        if (statement.value?.type === "void")
          return [
            this.emitExpression(statement.value),
            ...this.emitExitCleanups(false),
            `(return)`,
          ].join("\n");
        if (statement.value) {
          const temporary = this.allocateTemporary(statement.value.type);
          return [
            `(local.set ${temporary} ${this.emitExpression(statement.value)})`,
            ...this.emitExitCleanups(false),
            `(return (local.get ${temporary}))`,
          ].join("\n");
        }
        return [...this.emitExitCleanups(false), `(return)`].join("\n");
      case "break": {
        const loop = this.loops.at(-1)!;
        if (!statement.value || statement.value.type === "void")
          return [
            ...(statement.value ? [this.emitExpression(statement.value)] : []),
            ...this.emitExitCleanups(true),
            `(br ${loop.breakLabel})`,
          ].join("\n");
        const temporary = this.allocateTemporary(statement.value.type);
        return [
          `(local.set ${temporary} ${this.emitExpression(statement.value)})`,
          ...this.emitExitCleanups(true),
          `(br ${loop.breakLabel} (local.get ${temporary}))`,
        ].join("\n");
      }
      case "continue":
        return [...this.emitExitCleanups(true), `(br ${this.loops.at(-1)!.continueLabel})`].join(
          "\n",
        );
      case "pass":
        return `(nop)`;
      case "expression": {
        const expression = this.emitExpression(statement.expression);
        if (expected !== "void") return expression;
        return statement.expression.type === "void" ? expression : `(drop ${expression})`;
      }
    }
  }

  protected emitExpression(expression: HirExpression): string {
    const emitted =
      this.emitValueExpression(expression) ??
      this.emitCallExpression(expression) ??
      this.emitContainerExpression(expression) ??
      this.emitControlExpression(expression);
    if (emitted === undefined) throw new Error(`unsupported expression '${expression.kind}'`);
    // A stored `void` reads as its slot's null; as an expression it leaves nothing.
    return expression.type === "void" && VOID_SLOT_READS.has(expression.kind)
      ? `(drop ${emitted})`
      : emitted;
  }

  /**
   * `is` on optionals (04 Optional Types): `.None` is payload-free, so every
   * `.None` has one canonical identity even though the prototype allocates
   * each one; a `.Some` has the identity of its own construction.
   */
  private emitOptionalIdentity(
    expression: Extract<HirExpression, { kind: "binary" }>,
    left: string,
    right: string,
  ): string {
    const first = this.allocateTemporary(expression.left.type);
    const second = this.allocateTemporary(expression.right.type);
    const tag = (local: string): string =>
      `(struct.get $hd.variant $hd.variant-tag (ref.as_non_null (local.get ${local})))`;
    return [
      `(block (result i32)`,
      `  (local.set ${first} ${left})`,
      `  (local.set ${second} ${right})`,
      `  (if (result i32) (ref.eq (local.get ${first}) (local.get ${second}))`,
      `    (then (i32.const 1))`,
      `    (else (i32.and (i32.eqz ${tag(first)}) (i32.eqz ${tag(second)})))))`,
    ].join("\n");
  }

  private emitValueExpression(expression: HirExpression): string | undefined {
    switch (expression.kind) {
      case "integer":
        return integerConstant(expression.type, expression.value, expression.wide);
      case "float":
        return `(${expression.type === "f32" ? "f32" : "f64"}.const ${expression.value})`;
      case "string":
        return stringLiteral(expression.bytes);
      case "string-build": {
        if (expression.segments.length === 0) return stringLiteral([]);
        const concat = this.stringFunction("concat");
        return expression.segments
          .slice(1)
          .reduce(
            (left, segment) => `(call ${concat} ${left} ${this.emitExpression(segment)})`,
            this.emitExpression(expression.segments[0]!),
          );
      }
      case "value-equality":
        return this.emitValueEquality(
          this.boxVoid(expression.left),
          this.boxVoid(expression.right),
          expression.valueType,
          expression.strategy,
        );
      case "intrinsic-call":
        return this.emitIntrinsicCall(expression);
      case "value-ordering": {
        const temporary = this.allocateTemporary("i32");
        const ordering = `(local.get ${temporary})`;
        const comparisons: Readonly<Record<typeof expression.operator, string>> = {
          "<": `(i32.eq ${ordering} (i32.const -1))`,
          "<=": `(i32.le_s ${ordering} (i32.const 0))`,
          ">": `(i32.eq ${ordering} (i32.const 1))`,
          ">=": `(i32.and (i32.ge_s ${ordering} (i32.const 0)) (i32.le_s ${ordering} (i32.const 1)))`,
        };
        const compared = this.emitValueOrdering(
          this.boxVoid(expression.left),
          this.boxVoid(expression.right),
          expression.valueType,
          expression.strategy,
        );
        return `(block (result i32) (local.set ${temporary} ${compared}) ${comparisons[expression.operator]})`;
      }
      case "permission-weaken":
        return this.emitPermissionWeakening(expression);
      case "character":
        return `(i32.const ${expression.value})`;
      case "boolean":
        return `(i32.const ${expression.value ? 1 : 0})`;
      case "binding-expression":
        return this.emitBindingExpression(expression);
      case "list":
        return `(struct.new $hd.vector (i32.const ${expression.elements.length}) ${
          expression.elements.length === 0
            ? `(array.new_default $hd.list (i32.const 0))`
            : `(array.new_fixed $hd.list ${expression.elements.length} ${expression.elements.map((element) => this.boxValue(element, expression.elementType)).join(" ")})`
        })`;
      case "tuple":
        return expression.elements.length === 0
          ? `(nop)`
          : `(array.new_fixed $hd.list ${expression.elements.length} ${expression.elements.map((element, index) => this.boxValue(element, expression.elementTypes[index]!)).join(" ")})`;
      case "map": {
        const temporary = this.allocateTemporary(expression.type);
        const capacity = expression.entries.length;
        return [
          `(block (result (ref null $hd.map))`,
          `  (local.set ${temporary}`,
          `    (struct.new $hd.map`,
          `      (i32.const ${expression.keyKind})`,
          `      (i32.const 0)`,
          `      (array.new_default $hd.list (i32.const ${capacity}))`,
          `      (array.new_default $hd.list (i32.const ${capacity}))`,
          `      ${this.keyEquality(expression.keyType, expression.keyKind, expression.keyDispatch, expression.keyDictionary)}`,
          `      (array.new_default $hd.map-index (i32.const ${capacity}))`,
          `      (array.new_default $hd.map-index (i32.const ${capacity}))`,
          `      ${this.keyHash(expression.keyType, expression.keyKind)}))`,
          ...expression.entries.map((entry) =>
            [
              `  (call $hd.map_insert`,
              `    (ref.as_non_null (local.get ${temporary}))`,
              `    ${this.boxValue(entry.key, expression.keyType)}`,
              `    ${this.boxValue(entry.value, expression.valueType)})`,
            ].join("\n"),
          ),
          `  (local.get ${temporary})`,
          `)`,
        ].join("\n");
      }
      case "variant-wrap": {
        const tags: Readonly<Record<typeof expression.variant, number>> = {
          "optional-absent": 0,
          "optional-present": 1,
          "result-ok": 0,
          "result-error": 1,
        };
        const payload =
          expression.payload && expression.payloadType
            ? this.boxValue(expression.payload, expression.payloadType)
            : `(ref.null any)`;
        return `(struct.new $hd.variant (i32.const ${tags[expression.variant]}) ${payload})`;
      }
      case "variant-tag":
        return `(struct.get $hd.variant $hd.variant-tag (ref.as_non_null ${this.emitExpression(expression.receiver)}))`;
      case "variant-payload":
        return this.unboxValue(
          `(struct.get $hd.variant $hd.variant-payload (ref.as_non_null ${this.emitExpression(expression.receiver)}))`,
          expression.payloadType,
        );
      case "propagate": {
        const temporary = this.allocateTemporary(expression.operand.type);
        const result =
          expression.payloadType === "void"
            ? ""
            : ` (result ${this.watType(expression.payloadType)})`;
        const success =
          expression.payloadType === "void"
            ? `(nop)`
            : this.unboxValue(
                `(struct.get $hd.variant $hd.variant-payload (local.get ${temporary}))`,
                expression.payloadType,
              );
        const failure = [...this.emitExitCleanups(false), `(return (local.get ${temporary}))`].join(
          "\n",
        );
        return [
          `(block${result}`,
          `  (local.set ${temporary} ${this.emitExpression(expression.operand)})`,
          `  (if${result}`,
          `    (i32.eq`,
          `      (struct.get $hd.variant $hd.variant-tag (local.get ${temporary}))`,
          `      (i32.const ${expression.successTag}))`,
          `    (then ${success})`,
          `    (else`,
          indent(failure, 6),
          `    ))`,
          `)`,
        ].join("\n");
      }
      case "local":
        return expression.local.type === "void"
          ? `(nop)`
          : `(local.get ${localName(expression.local.index)})`;
      case "global":
        return expression.global.type === "void"
          ? `(nop)`
          : `(global.get ${globalName(expression.global.index)})`;
      case "capture": {
        if (expression.type === "void") return `(nop)`;
        return `(struct.get $env${expression.closureIndex} $env${expression.closureIndex}f${expression.fieldIndex} (ref.cast (ref $env${expression.closureIndex}) (local.get $env)))`;
      }
      case "cell-new":
      case "cell-get":
      case "cell-set":
        return this.emitCellExpression(expression);
      case "unary": {
        const operand = this.emitExpression(expression.operand);
        if (expression.operator === "+") return operand;
        if (expression.operator === "not") return `(i32.eqz ${operand})`;
        const numeric = this.emitNumericUnary(expression, operand);
        if (numeric) return numeric;
        if (expression.operator === "~")
          return expression.type === "i64"
            ? `(i64.xor ${operand} (i64.const -1))`
            : `(i32.xor ${operand} (i32.const -1))`;
        if (expression.type === "f64") return `(f64.neg ${operand})`;
        const width = expression.type === "i64" ? "i64" : "i32";
        // A release build wraps: `-MIN` is `MIN`.
        if (this.release) return `(${width}.sub (${width}.const 0) ${operand})`;
        return `(call $hd.neg_${width} ${operand})`;
      }
      case "binary": {
        const left = this.emitExpression(expression.left);
        const right = shiftCount(expression, this.emitExpression(expression.right), this.release);
        if (expression.operator === "is") {
          if (expression.left.type.startsWith("trait:")) {
            const trait = this.traitsByName.get(traitTypeBase(expression.left.type))!;
            return `(ref.eq (ref.cast (ref null eq) (struct.get $trait${trait.index} $trait${trait.index}value ${left})) (ref.cast (ref null eq) (struct.get $trait${trait.index} $trait${trait.index}value ${right})))`;
          }
          if (optionalInner(expression.left.type) !== undefined)
            return this.emitOptionalIdentity(expression, left, right);
          return `(ref.eq (ref.cast (ref null eq) ${left}) (ref.cast (ref null eq) ${right}))`;
        }
        const numeric = this.emitNumericBinary(expression, left, right);
        if (numeric) return numeric;
        if (expression.operator === "**") {
          if (expression.type === "f64") {
            this.floatPower = true;
            return `(call $hd.pow_f64 ${left} ${right})`;
          }
          if (expression.type === "i64")
            return `(call ${powerFunction("i64", this.release)} ${left} ${right})`;
          return `(call ${powerFunction("i32", this.release)} ${left} ${right})`;
        }
        if (expression.operator === "==" || expression.operator === "!=") {
          const equality = this.emitValueEquality(left, right, expression.left.type);
          return expression.operator === "==" ? equality : `(i32.eqz ${equality})`;
        }
        if (expression.left.type === "string") {
          if (expression.operator === "+")
            return `(call ${this.stringFunction("concat")} ${left} ${right})`;
          const comparison = `(call ${this.stringFunction("compare")} ${left} ${right})`;
          const operators: Readonly<Record<string, string>> = {
            "<": `(i32.lt_s ${comparison} (i32.const 0))`,
            "<=": `(i32.le_s ${comparison} (i32.const 0))`,
            ">": `(i32.gt_s ${comparison} (i32.const 0))`,
            ">=": `(i32.ge_s ${comparison} (i32.const 0))`,
          };
          return operators[expression.operator]!;
        }
        if (expression.operator === "and")
          return `(if (result i32) ${left} (then ${right}) (else (i32.const 0)))`;
        if (expression.operator === "or")
          return `(if (result i32) ${left} (then (i32.const 1)) (else ${right}))`;
        const releaseOperations: Readonly<Record<string, string>> = {
          "+": "add",
          "-": "sub",
          "*": "mul",
          "<<": "shl",
          ">>": "shr_s",
        };
        const checked: Readonly<Record<string, string>> = {
          "+": "$hd.add_i32",
          "-": "$hd.sub_i32",
          "*": "$hd.mul_i32",
          "<<": "$hd.shl_i32",
          ">>": "$hd.shr_i32",
        };
        if (expression.left.type === "i32" && checked[expression.operator]) {
          // A release build wraps `+`, `-`, `*`, and Wasm masks a shift count.
          if (this.release)
            return `(i32.${releaseOperations[expression.operator]} ${left} ${right})`;
          return `(call ${checked[expression.operator]} ${left} ${right})`;
        }
        const checkedWide: Readonly<Record<string, string>> = {
          "+": "$hd.add_i64",
          "-": "$hd.sub_i64",
          "*": "$hd.mul_i64",
        };
        if (expression.left.type === "i64" && checkedWide[expression.operator]) {
          if (this.release)
            return `(i64.${releaseOperations[expression.operator]} ${left} ${right})`;
          return `(call ${checkedWide[expression.operator]} ${left} ${right})`;
        }
        const prefix =
          expression.left.type === "f64" ? "f64" : expression.left.type === "i64" ? "i64" : "i32";
        if (prefix !== "f64" && (expression.operator === "/" || expression.operator === "%"))
          return this.emitCheckedDivision(prefix, expression.operator, left, right);
        const suffixes: Readonly<Record<string, string>> = {
          "+": "add",
          "-": "sub",
          "*": "mul",
          "/": prefix === "f64" ? "div" : "div_s",
          "%": "rem_s",
          "&": "and",
          "|": "or",
          "^": "xor",
          "<": prefix === "f64" ? "lt" : "lt_s",
          "<=": prefix === "f64" ? "le" : "le_s",
          ">": prefix === "f64" ? "gt" : "gt_s",
          ">=": prefix === "f64" ? "ge" : "ge_s",
        };
        const operation = `(${prefix}.${suffixes[expression.operator]} ${left} ${right})`;
        // `u8` is an i32 at run time; `+`, `-`, and `*` check its range.
        return !this.release &&
          expression.left.type === "u8" &&
          ["+", "-", "*"].includes(expression.operator)
          ? `(call $hd.check_u8 ${operation})`
          : operation;
      }
      default:
        return undefined;
    }
  }

  private emitPermissionWeakening(
    expression: Extract<HirExpression, { kind: "permission-weaken" }>,
  ): string {
    const actual = functionParts(expression.operand.type);
    const formal = functionParts(expression.type);
    return actual && formal
      ? this.emitCallableAdaptation(expression.operand, expression.type, expression.operand.type)
      : this.emitExpression(expression.operand);
  }

  private emitBindingExpression(
    expression: Extract<HirExpression, { kind: "binding-expression" }>,
  ): string {
    const value = this.allocateTemporary(expression.value.type);
    const valueReference = `(local.get ${value})`;
    const bind = expression.elementTypes
      ? expression.bindings.map(
          (binding, index) =>
            `(local.set ${localName(binding.index)} ${this.unboxValue(`(array.get $hd.list (ref.as_non_null ${valueReference}) (i32.const ${index}))`, expression.elementTypes![index]!)})`,
        )
      : [`(local.set ${localName(expression.bindings[0]!.index)} ${valueReference})`];
    return [
      `(block (result ${this.watType(expression.type)})`,
      `  (local.set ${value} ${this.emitExpression(expression.value)})`,
      ...bind.map((line) => `  ${line}`),
      `  ${valueReference}`,
      `)`,
    ].join("\n");
  }

  /**
   * A plain method call of a host capability names its call site to the
   * host, as a bang call does (emitter/host-providers.ts); no other trait
   * call sets it.
   */
  private hostCallSite(traitName: string, offset: number): readonly string[] {
    return this.hostCapabilities.has(traitName)
      ? [
          `  (global.set $hd.host-call-function (i32.const ${this.currentFunctionIndex}))`,
          `  (global.set $hd.host-call-site (i32.const ${offset}))`,
        ]
      : [];
  }

  private emitCallExpression(expression: HirExpression): string | undefined {
    switch (expression.kind) {
      case "call": {
        const ordered = this.emitOrderedArguments(
          expression.arguments,
          expression.argumentParameterIndices,
          expression.erasedParameterTypes,
          expression.defaultArguments,
          expression.parameterTypes,
          expression.bounds,
          expression.erasedTypeSubstitutions,
        );
        const invocation = `(call ${functionName(expression.functionIndex)}${expression.arguments.length || expression.bounds?.length || expression.providers.length ? " " : ""}${[
          ...ordered.values,
          ...(expression.bounds ?? []).map((bound) => this.emitExpression(bound)),
          ...expression.providers.map((provider) => this.emitExpression(provider)),
        ].join(" ")})`;
        const erasedResult = this.erasedResultType(expression.erasedResultType, expression.type);
        const rawResultType = erasedResult ?? expression.type;
        const call =
          ordered.setup.length === 0
            ? invocation
            : [
                `(block${rawResultType === "void" ? "" : ` (result ${this.watType(rawResultType)})`}`,
                ...ordered.setup.map((line) => `  ${line}`),
                `  ${invocation}`,
                `)`,
              ].join("\n");
        return erasedResult
          ? this.restoreErasedResult(
              call,
              erasedResult,
              expression.type,
              expression.erasedTypeSubstitutions,
            )
          : call;
      }
      case "suspend-construct": {
        const ordered = this.emitOrderedArguments(
          expression.arguments,
          expression.argumentParameterIndices,
          expression.erasedParameterTypes,
          expression.defaultArguments,
          expression.parameterTypes,
          expression.bounds,
          expression.erasedTypeSubstitutions,
        );
        const invocation = `(call ${functionName(expression.functionIndex)}${expression.arguments.length || expression.bounds?.length || expression.providers.length ? " " : ""}${[
          ...ordered.values,
          ...(expression.bounds ?? []).map((bound) => this.emitExpression(bound)),
          ...expression.providers.map((provider) => this.emitExpression(provider)),
        ].join(" ")})`;
        const resultType = suspensionParts(expression.type)!.result;
        const erasedResult = this.erasedResultType(expression.erasedResultType, resultType);
        const resultAdapter =
          erasedResult && functionParts(erasedResult)
            ? this.suspensionResultAdapter(
                readonlyType(resultType),
                erasedResult,
                expression.erasedTypeSubstitutions ?? [],
              )
            : undefined;
        if (ordered.setup.length === 0 && resultAdapter === undefined) return invocation;
        const frame = resultAdapter ? this.allocateTemporary(expression.type) : undefined;
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...ordered.setup.map((line) => `  ${line}`),
          ...(frame
            ? [
                `  (local.set ${frame} ${invocation})`,
                `  (struct.set $s${expression.functionIndex} $s${expression.functionIndex}result_adapter (local.get ${frame}) (ref.func ${resultAdapter}))`,
                `  (local.get ${frame})`,
              ]
            : [`  ${invocation}`]),
          `)`,
        ].join("\n");
      }
      case "suspension-wrap": {
        const suspension = suspensionParts(expression.suspension.type);
        const traitSuspension = traitSuspensionParts(expression.suspension.type);
        if (!suspension && !traitSuspension)
          throw new Error(`cannot wrap suspension type '${expression.suspension.type}'`);
        const adapters = suspension
          ? [
              suspensionWrapperPollAdapterName(suspension.functionIndex),
              suspensionWrapperCancelAdapterName(suspension.functionIndex),
              suspensionWrapperResultAdapterName(suspension.functionIndex),
            ]
          : [
              traitSuspensionWrapperPollAdapterName(
                traitSuspension!.traitIndex,
                traitSuspension!.methodIndex,
              ),
              traitSuspensionWrapperCancelAdapterName(
                traitSuspension!.traitIndex,
                traitSuspension!.methodIndex,
              ),
              traitSuspensionWrapperResultAdapterName(
                traitSuspension!.traitIndex,
                traitSuspension!.methodIndex,
              ),
            ];
        return `(struct.new $hd.suspension ${this.emitExpression(expression.suspension)} ${adapters.map((adapter) => `(ref.func ${adapter})`).join(" ")})`;
      }
      case "suspend-drive": {
        const erasedResult = this.erasedResultType(expression.erasedResultType, expression.type);
        const frame = erasedResult ? this.allocateTemporary(expression.suspension.type) : undefined;
        const frameValue = frame
          ? `(local.get ${frame})`
          : this.emitExpression(expression.suspension);
        const call = `(call $drive${expression.functionIndex} ${frameValue})`;
        const rawType = erasedResult ?? expression.type;
        const guarded = expression.blockOn
          ? this.emitBlockOnCall(call, rawType === "void" ? undefined : this.watType(rawType))
          : call;
        const restored =
          erasedResult && functionParts(erasedResult)
            ? this.restoreSuspensionResult(
                guarded,
                frameValue,
                expression.functionIndex,
                expression.type,
              )
            : erasedResult
              ? this.restoreErasedResult(guarded, erasedResult, expression.type)
              : guarded;
        return frame
          ? `(block (result ${this.watType(expression.type)}) (local.set ${frame} ${this.emitExpression(expression.suspension)}) ${restored})`
          : restored;
      }
      case "suspend-cancel":
        return `(call $cancel${expression.functionIndex} ${this.emitExpression(expression.suspension)})`;
      case "trait-suspend-drive": {
        const method = this.traitsByIndex.get(expression.traitIndex)!.methods[
          expression.methodIndex
        ]!;
        const erasedResult = this.erasedResultType(method.result, expression.type);
        const frame =
          erasedResult && functionParts(erasedResult)
            ? this.allocateTemporary(expression.suspension.type)
            : undefined;
        const frameValue = frame
          ? `(local.get ${frame})`
          : this.emitExpression(expression.suspension);
        const call = `(call ${traitSuspensionDriveName(expression.traitIndex, expression.methodIndex)} ${frameValue})`;
        const guarded = expression.blockOn
          ? this.emitBlockOnCall(
              call,
              expression.type === "void"
                ? undefined
                : this.watType(erasedResult ?? expression.type),
            )
          : call;
        const restored =
          erasedResult && functionParts(erasedResult)
            ? this.restoreTraitSuspensionResult(
                guarded,
                frameValue,
                expression.traitIndex,
                expression.methodIndex,
                expression.type,
              )
            : erasedResult
              ? this.unboxValue(guarded, expression.type)
              : guarded;
        return frame
          ? `(block (result ${this.watType(expression.type)}) (local.set ${frame} ${this.emitExpression(expression.suspension)}) ${restored})`
          : restored;
      }
      case "trait-suspend-cancel":
        return `(call ${traitSuspensionCancelName(expression.traitIndex, expression.methodIndex)} ${this.emitExpression(expression.suspension)})`;
      case "suspension-drive": {
        const call = `(call $hd.suspension_drive ${this.emitExpression(expression.suspension)})`;
        const guarded = expression.blockOn ? this.emitBlockOnCall(call, "anyref") : call;
        return expression.type === "void"
          ? `(drop ${guarded})`
          : this.unboxValue(guarded, expression.type);
      }
      case "suspension-cancel":
        return `(call $hd.suspension_cancel ${this.emitExpression(expression.suspension)})`;
      case "function-value":
        return `(struct.new $closure${this.functionSignatures.get(expression.type)} (ref.func $fv${expression.functionIndex}) (ref.null any))`;
      case "closure-self":
        return `(struct.new $closure${this.functionSignatures.get(readonlyType(expression.type))} (ref.func $c${expression.closureIndex}) (local.get $env))`;
      case "closure":
        return `(struct.new $closure${this.functionSignatures.get(readonlyType(expression.type))} (ref.func $c${expression.closureIndex}) (struct.new $env${expression.closureIndex}${[...expression.captures.map((capture) => this.boxVoid(capture)), ...this.closureBoundValues(expression.closureIndex)].map((value) => ` ${value}`).join("")}))`;
      case "closure-call": {
        const signature = this.functionSignatures.get(readonlyType(expression.callee.type));
        const temporary = this.allocateTemporary(expression.callee.type);
        return [
          `(block${expression.type === "void" ? "" : ` (result ${this.watType(expression.type)})`}`,
          `  (local.set ${temporary} ${this.emitExpression(expression.callee)})`,
          `  (call_ref $sig${signature}`,
          `    (struct.get $closure${signature} $closure${signature}env (local.get ${temporary}))`,
          ...expression.arguments.map((argument) => `    ${this.boxVoid(argument)}`),
          ...expression.providers.map((provider) => `    ${this.emitExpression(provider)}`),
          `    (struct.get $closure${signature} $closure${signature}fn (local.get ${temporary})))`,
          `)`,
        ].join("\n");
      }
      case "trait-wrap": {
        // A `void` value has no slot: it runs, and the trait value holds null.
        if (expression.value.type === "void")
          return `(block (result ${this.watType(expression.type)}) ${this.emitExpression(expression.value)} ${this.emitTraitDictionaryPlan(expression.dictionary, "(ref.null any)")})`;
        const temporary = this.allocateTemporary(expression.value.type);
        const wrapped = this.emitTraitDictionaryPlan(
          expression.dictionary,
          this.boxWatValue(`(local.get ${temporary})`, expression.value.type),
        );
        return `(block (result ${this.watType(expression.type)})
  (local.set ${temporary} ${this.emitExpression(expression.value)})
  ${wrapped}
)`;
      }
      case "trait-upcast": {
        const sourceTrait = this.traitsByIndex.get(expression.sourceTraitIndex)!;
        const targetTrait = this.traitsByIndex.get(expression.targetTraitIndex)!;
        const temporary = this.allocateTemporary(expression.value.type);
        return `(block (result ${this.watType(expression.type)})
  (local.set ${temporary} ${this.emitExpression(expression.value)})
  ${this.emitTraitUpcast(sourceTrait, targetTrait, expression.supertraitPath, `(local.get ${temporary})`)}
)`;
      }
      case "trait-dictionary": {
        return this.emitTraitDictionaryPlan(expression.dictionary, "(ref.null any)");
      }
      case "inspect-type-id":
      case "inspect-downcast":
        return this.emitInspectExpression(
          expression,
          this.emitExpression(expression.dictionary),
          expression.kind === "inspect-downcast" ? this.emitExpression(expression.value) : "",
        );
      case "trait-bound-dictionary":
        return this.boundDictionary(expression.boundIndex, expression.supertrait);
      case "trait-bound": {
        const trait = this.traitsByIndex.get(expression.traitIndex)!;
        const dictionary = `(local.get $bound${expression.boundIndex})`;
        const methods = this.liveTraitMethods(trait).map(
          (method) =>
            `(struct.get $trait${trait.index} $trait${trait.index}m${method.index} ${dictionary})`,
        );
        const parents = trait.supertraits.map(
          (_, index) =>
            `(struct.get $trait${trait.index} $trait${trait.index}s${index} ${dictionary})`,
        );
        const fields = [...methods, ...parents];
        return `(struct.new $trait${trait.index} ${this.boxValue(expression.value, expression.value.type)} (struct.get $trait${trait.index} $trait${trait.index}bounds ${dictionary})${fields.length ? " " : ""}${fields.join(" ")})`;
      }
      case "trait-call":
      case "trait-suspend-construct":
        return this.emitTraitCallExpression(expression);
      case "provider-use":
        return `(local.get $provider${expression.providerIndex})`;
      case "provider-pack": {
        const base = expression.bases.reduceRight(
          (parent, row) => `(call $hd.provider_concat ${this.emitExpression(row)} ${parent})`,
          `(ref.null $hd.providers)`,
        );
        return expression.providers.reduceRight(
          (parent, provider, index) =>
            `(struct.new $hd.providers (i32.const ${this.providerKey(expression.keys[index]!)}) ${this.boxProvider(this.emitExpression(provider), provider.type)} ${parent})`,
          base,
        );
      }
      case "provider-context": {
        const contextIndex = this.contextNames.get(expression.type);
        const finalProviders = new Map<string, string>();
        for (const entry of expression.entries) {
          if (entry.kind === "binding") finalProviders.set(entry.key, localName(entry.local.index));
          else
            entry.providers.forEach((provider) =>
              finalProviders.set(provider.key, localName(provider.local.index)),
            );
        }
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...this.emitProviderEntries(expression.entries).map((line) => `  ${line}`),
          `  (struct.new $context${contextIndex} ${expression.keys.map((key) => `(local.get ${finalProviders.get(key)})`).join(" ")})`,
          `)`,
        ].join("\n");
      }
      case "provider-with": {
        const result =
          expression.type === "void" ? "" : ` (result ${this.watType(expression.type)})`;
        return [
          `(block${result}`,
          ...this.emitProviderEntries(expression.entries).map((line) => `  ${line}`),
          indent(this.emitBlock(expression.body, expression.type), 2),
          `)`,
        ].join("\n");
      }
      default:
        return undefined;
    }
  }

  private emitTraitCallExpression(
    expression: Extract<HirExpression, { kind: "trait-call" | "trait-suspend-construct" }>,
  ): string {
    const trait = this.traitsByIndex.get(expression.traitIndex)!;
    const method = trait.methods[expression.methodIndex]!;
    const temporary = this.allocateTemporary(expression.receiver.type);
    const receiver = `(local.get ${temporary})`;
    const dispatch = this.traitDictionaryPath(
      expression.receiver.type,
      expression.supertraitPath,
      receiver,
    );
    const receiverTrait = this.traitsByName.get(traitTypeBase(expression.receiver.type))!;
    const ordered = this.emitOrderedArguments(
      expression.arguments,
      expression.argumentParameterIndices,
      expression.erasedParameterTypes,
      undefined,
      undefined,
      undefined,
      expression.erasedTypeSubstitutions,
      expression.kind === "trait-suspend-construct",
    );
    const invocation = [
      `(call_ref $tsig${trait.index}_${method.index}`,
      `  (struct.get $trait${receiverTrait.index} $trait${receiverTrait.index}value ${receiver})`,
      `  ${dispatch.dictionary}`,
      ...ordered.values.map((argument) => `  ${argument}`),
      ...(expression.bounds ?? []).map((bound) => `  ${this.emitExpression(bound)}`),
      ...expression.providers.map((provider) => `  ${this.emitExpression(provider)}`),
      `  (struct.get $trait${trait.index} $trait${trait.index}m${method.index} ${dispatch.dictionary}))`,
    ].join("\n");
    if (expression.kind === "trait-call") {
      const erased = this.erasedResultType(expression.erasedResultType, expression.type);
      const result = erased
        ? this.restoreErasedResult(
            invocation,
            erased,
            expression.type,
            expression.erasedTypeSubstitutions,
          )
        : invocation;
      return [
        `(block${expression.type === "void" ? "" : ` (result ${this.watType(expression.type)})`}`,
        `  (local.set ${temporary} ${this.emitExpression(expression.receiver)})`,
        ...ordered.setup.map((line) => `  ${line}`),
        ...this.hostCallSite(trait.name, physicalPosition(expression.span.start).offset),
        `  ${result}`,
        `)`,
      ].join("\n");
    }
    const resultType = traitSuspensionParts(expression.type)!.result;
    const erased = this.erasedResultType(expression.erasedResultType, resultType);
    const adapter =
      erased && functionParts(erased)
        ? this.suspensionResultAdapter(
            readonlyType(resultType),
            erased,
            expression.erasedTypeSubstitutions ?? [],
          )
        : undefined;
    const frame = adapter ? this.allocateTemporary(expression.type) : undefined;
    const wrapper = traitSuspensionName(trait.index, method.index);
    return [
      `(block (result (ref null ${wrapper}))`,
      `  (local.set ${temporary} ${this.emitExpression(expression.receiver)})`,
      ...ordered.setup.map((line) => `  ${line}`),
      `  (global.set $hd.host-call-function (i32.const ${this.currentFunctionIndex}))`,
      `  (global.set $hd.host-call-site (i32.const ${physicalPosition(expression.span.start).offset}))`,
      ...(frame
        ? [
            `  (local.set ${frame} ${invocation})`,
            `  (struct.set ${wrapper} ${wrapper}result_adapter (local.get ${frame}) (ref.func ${adapter}))`,
            `  (local.get ${frame})`,
          ]
        : invocation.split("\n").map((line) => `  ${line}`)),
      `)`,
    ].join("\n");
  }

  /**
   * A `block_on` call may run while another driver is active
   * (req.drive.block-on.under-driver). It clears the active flag so its own
   * drive loop starts, drives only its argument (req.drive.block-on.inner-only),
   * and then restores the outer driver's flag. The restore leaves the call's
   * result on the stack beneath it.
   */
  private emitBlockOnCall(call: string, resultWat: string | undefined): string {
    const saved = this.allocateTemporary("bool");
    return [
      `(block${resultWat === undefined ? "" : ` (result ${resultWat})`}`,
      `(local.set ${saved} (global.get $hd.driver-active))`,
      `(global.set $hd.driver-active (i32.const 0))`,
      call,
      `(global.set $hd.driver-active (local.get ${saved})))`,
    ].join(" ");
  }

  private emitContainerExpression(expression: HirExpression): string | undefined {
    switch (expression.kind) {
      case "data":
        return this.emitDataExpression(expression);
      case "enum": {
        if (expression.fields.length === 0) {
          return `(global.get $e${expression.enumIndex}v${expression.tag})`;
        }
        const temporaries = expression.fields.map((field) => this.allocateTemporary(field.type));
        const sourceByField = new Map(
          expression.fieldIndices.map(
            (fieldIndex, sourceIndex) => [fieldIndex, sourceIndex] as const,
          ),
        );
        const storedFields = expression.fieldTypes.map((fieldType, fieldIndex) => {
          const sourceIndex = sourceByField.get(fieldIndex);
          if (sourceIndex === undefined) return this.defaultValue(fieldType);
          const value = `(local.get ${temporaries[sourceIndex]})`;
          return this.storeErased(
            value,
            expression.erasedFieldTypes?.[fieldIndex],
            expression.fields[sourceIndex]!.type,
            expression.erasedTypeSubstitutions,
          );
        });
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...expression.fields.map(
            (field, index) => `  (local.set ${temporaries[index]} ${this.boxVoid(field)})`,
          ),
          `  (struct.new $e${expression.enumIndex} (i32.const ${expression.tag})${storedFields.length ? " " : ""}${storedFields.join(" ")})`,
          `)`,
        ].join("\n");
      }
      case "member": {
        const value = `(struct.get $d${expression.dataIndex} $d${expression.dataIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)})`;
        return this.loadErased(
          value,
          expression.erasedFieldType,
          expression.type,
          expression.erasedTypeSubstitutions,
        );
      }
      case "embedded-copy":
        return this.emitEmbeddedCopy(expression);
      case "field-set": {
        const value = this.boxVoid(expression.value);
        const stored = this.storeErased(
          value,
          expression.erasedFieldType,
          expression.value.type,
          expression.erasedTypeSubstitutions,
        );
        return `(struct.set $d${expression.dataIndex} $d${expression.dataIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)} ${stored})`;
      }
      case "enum-member": {
        const value = `(struct.get $e${expression.enumIndex} $e${expression.enumIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)})`;
        return this.loadErased(
          value,
          expression.erasedFieldType,
          expression.type,
          expression.erasedTypeSubstitutions,
        );
      }
      case "list-length":
        return `(struct.get $hd.vector $hd.vector-size (ref.as_non_null ${this.emitExpression(expression.receiver)}))`;
      case "list-iterator":
        return this.emitListIterator(expression.receiver);
      case "iterator-next":
        return this.emitIteratorNext(expression.receiver, expression.elementType);
      case "map-iterator":
        return this.emitMapIterator(expression.receiver);
      case "list-index": {
        const vector = `(ref.as_non_null ${this.emitExpression(expression.receiver)})`;
        const index = this.emitExpression(expression.index);
        const get =
          numericType(readonlyType(expression.index.type))?.wasm === "i64"
            ? "$hd.vector_get_wide"
            : "$hd.vector_get";
        return this.unboxValue(`(call ${get} ${vector} ${index})`, expression.elementType);
      }
      case "string-index": {
        const text = `(ref.as_non_null ${this.emitExpression(expression.receiver)})`;
        const index = this.emitExpression(expression.index);
        return numericType(readonlyType(expression.index.type))?.wasm === "i64"
          ? `(call $hd.string_get_wide ${text} ${index})`
          : `(call $hd.string_get ${text} ${index})`;
      }
      case "list-set": {
        const vector = `(ref.as_non_null ${this.emitExpression(expression.receiver)})`;
        const index = this.emitExpression(expression.index);
        const set =
          numericType(readonlyType(expression.index.type))?.wasm === "i64"
            ? "$hd.vector_set_wide"
            : "$hd.vector_set";
        return `(call ${set} ${vector} ${index} ${this.boxValue(expression.value, expression.elementType)})`;
      }
      case "list-push":
        return `(call $hd.vector_push (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.value, expression.elementType)})`;
      case "tuple-index":
        return this.unboxValue(
          `(array.get $hd.list (ref.as_non_null ${this.emitExpression(expression.receiver)}) (i32.const ${expression.index}))`,
          expression.elementType,
        );
      case "map-length":
        return `(struct.get $hd.map $hd.map-size (ref.as_non_null ${this.emitExpression(expression.receiver)}))`;
      case "map-index":
        if (expression.required)
          return this.unboxValue(
            `(call $hd.map_get_required (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.key, expression.keyType)})`,
            expression.valueType,
          );
        return `(call $hd.map_get (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.key, expression.keyType)})`;
      case "map-remove":
        return `(call $hd.map_remove (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.key, expression.keyType)})`;
      case "map-set":
        return `(call $hd.map_insert (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.key, expression.keyType)} ${this.boxValue(expression.value, expression.valueType)})`;
      case "map-entry-key":
        return this.unboxValue(
          `(array.get $hd.list (struct.get $hd.map $hd.map-keys (ref.as_non_null ${this.emitExpression(expression.receiver)})) ${this.emitExpression(expression.index)})`,
          expression.keyType,
        );
      case "map-entry-value":
        return this.unboxValue(
          `(array.get $hd.list (struct.get $hd.map $hd.map-values (ref.as_non_null ${this.emitExpression(expression.receiver)})) ${this.emitExpression(expression.index)})`,
          expression.valueType,
        );
      case "panic":
        this.rejectUnsupported(expression);
        return this.emitRuntimePanic(
          expression.category ?? "explicit-panic",
          this.emitExpression(expression.message),
        );
      default:
        return undefined;
    }
  }

  private emitControlExpression(expression: HirExpression): string | undefined {
    switch (expression.kind) {
      case "if": {
        const result =
          expression.type === "void" || expression.type === "never"
            ? ""
            : ` (result ${this.watType(expression.type)})`;
        return [
          `(if${result} ${this.emitExpression(expression.condition)}`,
          `  (then`,
          indent(this.emitBlock(expression.thenBody, expression.type), 4),
          `  )`,
          `  (else`,
          indent(this.emitBlock(expression.elseBody, expression.type), 4),
          `  )`,
          `)`,
        ].join("\n");
      }
      case "for":
        return this.emitForExpression(expression);
      case "list-comprehension":
      case "map-comprehension":
        return this.emitComprehensionExpression(expression);
      case "while": {
        const id = this.loopCounter++;
        const labels = {
          breakLabel: `$break${id}`,
          continueLabel: `$loop${id}`,
          result: expression.elseBody.length > 0 ? expression.type : undefined,
        };
        this.loops.push(labels);
        const body = this.emitBlock(expression.body, "void", true);
        this.loops.pop();
        // An infinite loop that no `break` targets never completes, so the
        // code after it is unreachable to the validator too.
        const never = expression.type === "never" ? "\n(unreachable)" : "";
        if (expression.elseBody.length > 0) {
          const result =
            expression.type === "void" || expression.type === "never"
              ? ""
              : ` (result ${this.watType(expression.type)})`;
          // An infinite loop's `else` never runs; its value is dropped.
          const elseBody = this.emitBlock(
            expression.elseBody,
            expression.type === "never" ? "void" : expression.type,
          );
          return (
            [
              `(block ${labels.breakLabel}${result}`,
              `  (loop ${labels.continueLabel}`,
              `    (if ${this.emitExpression(expression.condition)}`,
              `      (then`,
              indent(body, 8),
              `        (br ${labels.continueLabel})`,
              `      )`,
              `      (else`,
              indent(elseBody, 8),
              `        (br ${labels.breakLabel})`,
              `      )`,
              `    )`,
              `  )`,
              `  unreachable`,
              `)`,
            ].join("\n") + never
          );
        }
        return (
          [
            `(block ${labels.breakLabel}`,
            `  (loop ${labels.continueLabel}`,
            `    (br_if ${labels.breakLabel} (i32.eqz ${this.emitExpression(expression.condition)}))`,
            indent(body, 4),
            `    (br ${labels.continueLabel})`,
            `  )`,
            `)`,
          ].join("\n") + never
        );
      }
      case "match": {
        const subject = this.allocateTemporary(expression.subject.type);
        const result =
          expression.type === "void" || expression.type === "never"
            ? ""
            : ` (result ${this.watType(expression.type)})`;
        return [
          `(block${result}`,
          `  (local.set ${subject} ${this.boxVoid(expression.subject)})`,
          indent(
            this.emitMatchArms(
              expression.representation,
              expression.enumIndex,
              subject,
              expression.arms,
              expression.type,
            ),
            2,
          ),
          `)`,
        ].join("\n");
      }
      default:
        return undefined;
    }
  }

  protected emitProviderEntries(entries: readonly HirProviderContextEntry[]): string[] {
    const emitted: string[] = [];
    for (const entry of entries) {
      if (entry.kind === "binding") {
        emitted.push(
          `(local.set ${localName(entry.local.index)} ${this.emitExpression(entry.value)})`,
        );
        continue;
      }
      const contextIndex = this.contextNames.get(entry.value.type);
      emitted.push(
        `(local.set ${localName(entry.contextLocal.index)} ${this.emitExpression(entry.value)})`,
      );
      for (const provider of entry.providers) {
        emitted.push(
          `(local.set ${localName(provider.local.index)} (struct.get $context${contextIndex} $context${contextIndex}f${provider.fieldIndex} (local.get ${localName(entry.contextLocal.index)})))`,
        );
      }
    }
    return emitted;
  }

  protected emitOrderedArguments(
    arguments_: readonly HirExpression[],
    parameterIndices?: readonly number[],
    erasedParameterTypes?: readonly ValueType[],
    defaultArguments?: readonly HirDefaultArgument[],
    parameterTypes?: readonly ValueType[],
    defaultBounds?: readonly HirExpression[],
    typeSubstitutions: readonly HirTypeSubstitution[] = [],
    stage = false,
  ): EmittedArguments {
    const emitValue = (argument: HirExpression, parameterIndex: number): string => {
      // A `void` parameter holds null (`slotWatType`).
      if (argument.type === "void") return this.boxVoid(argument);
      const formal = erasedParameterTypes?.[parameterIndex];
      if (
        formal &&
        functionParts(formal) &&
        formal !== argument.type &&
        functionParts(argument.type)
      ) {
        return this.emitCallableAdaptation(argument, formal, argument.type, typeSubstitutions);
      }
      return formal && isGenericValueType(formal)
        ? this.boxValue(argument, argument.type)
        : this.emitExpression(argument);
    };
    if (!stage && !parameterIndices && !defaultArguments?.length) {
      return { setup: [], values: arguments_.map((argument, index) => emitValue(argument, index)) };
    }
    const setup: string[] = [];
    const values = Array.from({ length: parameterTypes?.length ?? arguments_.length }, () => "");
    arguments_.forEach((argument, argumentIndex) => {
      const parameterIndex = parameterIndices?.[argumentIndex] ?? argumentIndex;
      const temporary = this.allocateTemporary(
        erasedParameterTypes?.[parameterIndex] ?? parameterTypes?.[parameterIndex] ?? argument.type,
      );
      setup.push(`(local.set ${temporary} ${emitValue(argument, parameterIndex)})`);
      values[parameterIndex] = `(local.get ${temporary})`;
    });
    for (const defaultArgument of defaultArguments ?? []) {
      const parameterIndex = defaultArgument.parameterIndex;
      const temporary = this.allocateTemporary(
        parameterTypes?.[parameterIndex] ?? erasedParameterTypes?.[parameterIndex] ?? "void",
      );
      const inputs = [
        ...values.slice(0, parameterIndex),
        ...(defaultBounds ?? []).map((bound) => this.emitExpression(bound)),
      ];
      setup.push(
        `(local.set ${temporary} (call ${functionName(defaultArgument.functionIndex)}${inputs.length > 0 ? ` ${inputs.join(" ")}` : ""}))`,
      );
      values[parameterIndex] = `(local.get ${temporary})`;
    }
    return { setup, values };
  }

  protected boxValue(expression: HirExpression, type: ValueType): string {
    if (expression.type === "void") return this.boxVoid(expression);
    return this.boxWatValue(this.emitExpression(expression), type);
  }

  protected emitCallableAdaptation(
    expression: HirExpression,
    formalType: ValueType,
    actualType: ValueType,
    typeSubstitutions: readonly HirTypeSubstitution[] = [],
  ): string {
    return this.adaptCallable(
      this.emitExpression(expression),
      formalType,
      actualType,
      typeSubstitutions,
    );
  }

  private emitTraitDictionaryPlan(plan: HirTraitDictionaryPlan, value: string): string {
    if (plan.builtin)
      return this.emitBuiltinTraitDictionary(
        plan.builtin,
        plan.bounds,
        plan.bounds.map((bound) => this.emitExpression(bound)),
        value,
        plan.supertraits.map((parent) => this.emitTraitDictionaryPlan(parent, value)),
      );
    const implementation = this.implementationsByIndex.get(plan.implementationIndex)!;
    return this.emitTraitDictionary(
      implementation,
      value,
      plan.bounds.map((bound) => this.emitExpression(bound)),
      plan.supertraits.map((parent) => this.emitTraitDictionaryPlan(parent, value)),
    );
  }

  emitTraitSuspensionHelpers(): string {
    return [...this.traitsByIndex.values()]
      .flatMap((trait) =>
        this.liveTraitMethods(trait).flatMap((method) => {
          if (!method.suspending) return [];
          const wrapper = traitSuspensionName(trait.index, method.index);
          const frame = `(local.get $frame)`;
          const inner = `(struct.get ${wrapper} ${wrapper}inner ${frame})`;
          const pollRef = `(struct.get ${wrapper} ${wrapper}poll ${frame})`;
          const cancelRef = `(struct.get ${wrapper} ${wrapper}cancel ${frame})`;
          const resultRef = `(struct.get ${wrapper} ${wrapper}result ${frame})`;
          const result = method.result === "void" ? "" : ` (result ${this.watType(method.result)})`;
          const poll = `(func ${traitSuspensionPollName(trait.index, method.index)} (param $frame (ref null ${wrapper})) (result i32)\n  (call_ref $tspollsig${trait.index}_${method.index} ${inner} ${pollRef})\n)`;
          const cancel = `(func ${traitSuspensionCancelName(trait.index, method.index)} (param $frame (ref null ${wrapper}))\n  (call_ref $tscancelsig${trait.index}_${method.index} ${inner} ${cancelRef})\n)`;
          const readResult = `(func ${traitSuspensionResultName(trait.index, method.index)} (param $frame (ref null ${wrapper}))${result}\n  (call_ref $tsresultsig${trait.index}_${method.index} ${inner} ${resultRef})\n)`;
          const drive = `(func ${traitSuspensionDriveName(trait.index, method.index)} (param $frame (ref null ${wrapper}))${result}\n  (block $ready\n    (loop $drive\n      (br_if $ready (i32.eq (call ${traitSuspensionPollName(trait.index, method.index)} (local.get $frame)) (i32.const 1)))\n      (br $drive)))\n  (call ${traitSuspensionResultName(trait.index, method.index)} (local.get $frame))\n)`;
          return [poll, cancel, readResult, drive];
        }),
      )
      .join("\n\n");
  }

  /** A captured `let` local's shared storage (07-functions.md#captures). */
  private emitCellExpression(
    expression: Extract<HirExpression, { kind: "cell-new" | "cell-get" | "cell-set" }>,
  ): string {
    if (expression.kind === "cell-new")
      return `(struct.new $hd.cell ${this.boxWatValue(this.emitExpression(expression.value), cellInner(expression.type)!)})`;
    const cell = `(ref.as_non_null ${this.emitExpression(expression.cell)})`;
    if (expression.kind === "cell-get")
      return this.unboxValue(`(struct.get $hd.cell $hd.cell-value ${cell})`, expression.type);
    return `(struct.set $hd.cell $hd.cell-value ${cell} ${this.boxWatValue(this.emitExpression(expression.value), cellInner(expression.cell.type)!)})`;
  }

  protected emitMatchArms(
    representation: "enum" | "erased-variant" | "scalar" | "data",
    enumIndex: number | undefined,
    subject: string,
    arms: Extract<HirExpression, { kind: "match" }>["arms"],
    resultType: ValueType,
    index = 0,
  ): string {
    const arm = arms[index];
    if (!arm) return `(unreachable)`;
    const bindings = arm.bindings.map((binding) => {
      const value = binding.accessPath
        ? this.emitPatternAccess(subject, binding.accessPath)
        : binding.enumFieldIndex !== undefined
          ? this.emitEnumPayloadAccess(
              subject,
              enumIndex,
              binding.enumFieldIndex,
              binding.enumErasedFieldType,
              binding.enumFieldType!,
              binding.path ?? [],
            )
          : binding.path
            ? this.emitDataPatternAccess(subject, binding.path)
            : binding.fieldIndex === -1
              ? `(local.get ${subject})`
              : representation === "enum"
                ? binding.erasedFieldType && isGenericValueType(binding.erasedFieldType)
                  ? this.unboxValue(
                      `(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${subject}))`,
                      binding.type,
                    )
                  : `(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${subject}))`
                : representation === "erased-variant"
                  ? this.unboxValue(
                      `(struct.get $hd.variant $hd.variant-payload (local.get ${subject}))`,
                      binding.type,
                    )
                  : `(local.get ${subject})`;
      return `(local.set ${localName(binding.local.index)} ${value})`;
    });
    const body = this.emitBlock(arm.body, resultType);
    const result =
      resultType === "void" || resultType === "never"
        ? ""
        : ` (result ${this.watType(resultType)})`;
    const guardedBody = arm.guard
      ? [
          ...bindings,
          `(if${result} ${this.emitExpression(arm.guard)}`,
          `  (then`,
          indent(body, 4),
          `  )`,
          `  (else`,
          indent(
            this.emitMatchArms(representation, enumIndex, subject, arms, resultType, index + 1),
            4,
          ),
          `  )`,
          `)`,
        ].join("\n")
      : [...bindings, body].join("\n");
    let condition: string | undefined;
    if (arm.literal) {
      const value = this.emitExpression(arm.literal);
      condition =
        arm.literal.type === "string"
          ? `(call ${this.stringFunction("equal")} (local.get ${subject}) ${value})`
          : `(${scalarWasm(arm.literal.type)}.eq (local.get ${subject}) ${value})`;
    } else if (arm.tag !== undefined) {
      const actual =
        representation === "enum"
          ? `(struct.get $e${enumIndex} $e${enumIndex}tag (local.get ${subject}))`
          : `(struct.get $hd.variant $hd.variant-tag (local.get ${subject}))`;
      condition = `(i32.eq ${actual} (i32.const ${arm.tag}))`;
    }
    for (const test of arm.tests ?? []) {
      const actual = this.emitMatchTestAccess(subject, enumIndex, test);
      const expected =
        test.tag !== undefined ? `(i32.const ${test.tag})` : this.emitExpression(test.literal!);
      const testCondition =
        test.tag !== undefined
          ? `(i32.eq ${actual} ${expected})`
          : test.literal!.type === "string"
            ? `(call ${this.stringFunction("equal")} ${actual} ${expected})`
            : `(${scalarWasm(test.literal!.type)}.eq ${actual} ${expected})`;
      condition = condition ? andThen(condition, testCondition) : testCondition;
    }
    if (!condition) return guardedBody;
    return [
      `(if${result}`,
      `  ${condition}`,
      `  (then`,
      indent(guardedBody, 4),
      `  )`,
      `  (else`,
      indent(
        this.emitMatchArms(representation, enumIndex, subject, arms, resultType, index + 1),
        4,
      ),
      `  )`,
      `)`,
    ].join("\n");
  }

  protected emitMatchTestAccess(
    subject: string,
    enumIndex: number | undefined,
    test: NonNullable<HirMatchArm["tests"]>[number],
  ): string {
    if (test.accessPath) {
      const value = this.emitPatternAccess(subject, test.accessPath);
      return test.tag === undefined ? value : matchTestTag(test.tagEnumIndex!, value);
    }
    if (test.enumFieldIndex !== undefined) {
      return this.emitEnumPayloadAccess(
        subject,
        enumIndex,
        test.enumFieldIndex,
        test.erasedFieldType,
        test.valueType!,
        test.path ?? [],
      );
    }
    return this.emitDataPatternAccess(subject, test.path!);
  }

  protected emitEnumPayloadAccess(
    subject: string,
    enumIndex: number | undefined,
    fieldIndex: number,
    erasedFieldType: ValueType | undefined,
    valueType: ValueType,
    path: readonly HirPatternPathStep[],
  ): string {
    if (enumIndex === undefined) throw new Error("enum payload access has no enum type");
    const raw = `(struct.get $e${enumIndex} $e${enumIndex}f${fieldIndex} (local.get ${subject}))`;
    const root =
      erasedFieldType && isGenericValueType(erasedFieldType)
        ? this.unboxValue(raw, valueType)
        : raw;
    return path.reduce(
      (value, step) =>
        `(struct.get $d${step.dataIndex} $d${step.dataIndex}f${step.fieldIndex} ${value})`,
      root,
    );
  }

  protected emitDataPatternAccess(subject: string, path: readonly HirPatternPathStep[]): string {
    return path.reduce(
      (value, step) =>
        `(struct.get $d${step.dataIndex} $d${step.dataIndex}f${step.fieldIndex} ${value})`,
      `(local.get ${subject})`,
    );
  }

  protected emitFrameCleanups(frame: CleanupFrame): string[] {
    return [...frame.cleanups].reverse().map((cleanup) => this.emitBlock(cleanup, "void"));
  }

  protected emitExitCleanups(stopAtLoop: boolean): string[] {
    const emitted: string[] = [];
    for (let index = this.cleanupFrames.length - 1; index >= 0; index -= 1) {
      const frame = this.cleanupFrames[index]!;
      emitted.push(...this.emitFrameCleanups(frame));
      if (stopAtLoop && frame.loopBoundary) break;
    }
    return emitted;
  }
}
