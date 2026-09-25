import type {
  HirExpression,
  HirDefaultArgument,
  HirFunction,
  HirLocal,
  HirMatchArm,
  HirPatternAccessStep,
  HirPatternPathStep,
  HirProviderContextEntry,
  HirStatement,
  HirTraitDictionaryPlan,
  ValueType,
} from "../hir.ts";
import {
  functionParts,
  nominalGenericParts,
  nominalGenericType,
  substituteTypeParameters,
  suspensionParts,
  traitSuspensionParts,
} from "../types.ts";
import type { CleanupFrame, EmittedArguments } from "./context.ts";
import {
  functionName,
  globalName,
  indent,
  containsGenericValueType,
  isGenericValueType,
  isRowRequirement,
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
  type LinearSuspensionSite,
} from "./shared.ts";
import { IteratorEmitter } from "./iterator.ts";

export abstract class FunctionBodyEmitter extends IteratorEmitter {
  protected abstract emitLinearContinuation(
    declaration: HirFunction,
    sites: readonly LinearSuspensionSite[],
    startIndex: number,
    cleanups: readonly (readonly HirStatement[])[],
  ): string;
  protected abstract emitSuspensionCompletion(
    declaration: HirFunction,
    value: HirExpression | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): string;
  protected abstract emitSuspensionCompletionWat(
    declaration: HirFunction,
    value: string | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): string;
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
        return `(local.set ${localName(statement.local.index)} ${this.emitExpression(statement.value)})`;
      case "assignment":
        return `(local.set ${localName(statement.local.index)} ${this.emitExpression(statement.value)})`;
      case "global-binding":
      case "global-assignment":
        return `(global.set ${globalName(statement.global.index)} ${this.emitExpression(statement.value)})`;
      case "discard":
        return `(drop ${this.emitExpression(statement.value)})`;
      case "return":
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
        if (!statement.value)
          return [...this.emitExitCleanups(true), `(br ${loop.breakLabel})`].join("\n");
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
    if (emitted !== undefined) return emitted;
    throw new Error(`unsupported expression '${expression.kind}'`);
  }

  private emitValueExpression(expression: HirExpression): string | undefined {
    switch (expression.kind) {
      case "integer":
        return `(i32.const ${expression.value})`;
      case "float":
        return `(f64.const ${expression.value})`;
      case "string":
        return expression.bytes.length === 0
          ? `(array.new_default $hd.bytes (i32.const 0))`
          : `(array.new_fixed $hd.bytes ${expression.bytes.length} ${expression.bytes.map((byte) => `(i32.const ${byte})`).join(" ")})`;
      case "string-build": {
        if (expression.segments.length === 0) return `(array.new_default $hd.bytes (i32.const 0))`;
        return expression.segments
          .slice(1)
          .reduce(
            (left, segment) => `(call $hd.string_concat ${left} ${this.emitExpression(segment)})`,
            this.emitExpression(expression.segments[0]!),
          );
      }
      case "display": {
        const operand = this.emitExpression(expression.operand);
        if (expression.operand.type === "i32") return `(call $hd.i32_to_string ${operand})`;
        if (expression.operand.type === "f64") {
          this.floatDisplay = true;
          return `(call $hd.f64_to_string ${operand})`;
        }
        if (expression.operand.type === "char") return `(call $hd.char_to_string ${operand})`;
        if (expression.operand.type === "bool") {
          return `(if (result (ref null $hd.bytes)) ${operand} (then (array.new_fixed $hd.bytes 4 (i32.const 116) (i32.const 114) (i32.const 117) (i32.const 101))) (else (array.new_fixed $hd.bytes 5 (i32.const 102) (i32.const 97) (i32.const 108) (i32.const 115) (i32.const 101))))`;
        }
        throw new Error(`unsupported Display operand ${expression.operand.type}`);
      }
      case "console-print":
        this.consoleOutput = true;
        return `(call $hd.console_print ${this.emitExpression(expression.provider)} ${this.emitExpression(expression.value)})`;
      case "value-equality":
        return this.emitValueEquality(
          this.emitExpression(expression.left),
          this.emitExpression(expression.right),
          expression.valueType,
          expression.strategy,
        );
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
          this.emitExpression(expression.left),
          this.emitExpression(expression.right),
          expression.valueType,
          expression.strategy,
        );
        return `(block (result i32) (local.set ${temporary} ${compared}) ${comparisons[expression.operator]})`;
      }
      case "assert-equal": {
        const values = Array.from({ length: 3 }, () => "");
        const setup = expression.arguments.map((argument, argumentIndex) => {
          const parameterIndex =
            expression.argumentParameterIndices?.[argumentIndex] ?? argumentIndex;
          const temporary = this.allocateTemporary(
            parameterIndex === 2 ? "string" : expression.valueType,
          );
          values[parameterIndex] = `(local.get ${temporary})`;
          return `(local.set ${temporary} ${this.emitExpression(argument)})`;
        });
        const equality = this.emitValueEquality(
          values[0]!,
          values[1]!,
          expression.valueType,
          expression.strategy,
        );
        return [
          `(block`,
          ...setup.map((line) => `  ${line}`),
          `  (if (i32.eqz ${equality})`,
          `    (then ${this.emitRuntimePanic("assertion-failed")}))`,
          `)`,
        ].join("\n");
      }
      case "assert": {
        const values = Array.from({ length: 2 }, () => "");
        const setup = expression.arguments.map((argument, argumentIndex) => {
          const parameterIndex =
            expression.argumentParameterIndices?.[argumentIndex] ?? argumentIndex;
          const temporary = this.allocateTemporary(parameterIndex === 0 ? "bool" : "string");
          values[parameterIndex] = `(local.get ${temporary})`;
          return `(local.set ${temporary} ${this.emitExpression(argument)})`;
        });
        return [
          `(block`,
          ...setup.map((line) => `  ${line}`),
          `  (if (i32.eqz ${values[0]}) (then ${this.emitRuntimePanic("assertion-failed")}))`,
          `)`,
        ].join("\n");
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
        } (i32.const 0))`;
      case "tuple":
        return expression.elements.length === 0
          ? `(array.new_default $hd.list (i32.const 0))`
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
          `      (i32.const 0)))`,
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
        return `(local.get ${localName(expression.local.index)})`;
      case "global":
        return `(global.get ${globalName(expression.global.index)})`;
      case "capture": {
        return `(struct.get $env${expression.closureIndex} $env${expression.closureIndex}f${expression.fieldIndex} (ref.cast (ref $env${expression.closureIndex}) (local.get $env)))`;
      }
      case "unary": {
        const operand = this.emitExpression(expression.operand);
        if (expression.operator === "+") return operand;
        if (expression.operator === "not") return `(i32.eqz ${operand})`;
        if (expression.operator === "~") return `(i32.xor ${operand} (i32.const -1))`;
        return expression.type === "f64" ? `(f64.neg ${operand})` : `(call $hd.neg_i32 ${operand})`;
      }
      case "binary": {
        const left = this.emitExpression(expression.left);
        const right = this.emitExpression(expression.right);
        if (expression.operator === "is") {
          if (expression.left.type.startsWith("trait:")) {
            const trait = this.traitsByName.get(traitTypeBase(expression.left.type))!;
            return `(ref.eq (ref.cast (ref null eq) (struct.get $trait${trait.index} $trait${trait.index}value ${left})) (ref.cast (ref null eq) (struct.get $trait${trait.index} $trait${trait.index}value ${right})))`;
          }
          return `(ref.eq (ref.cast (ref null eq) ${left}) (ref.cast (ref null eq) ${right}))`;
        }
        if (expression.operator === "**") {
          if (expression.type === "f64") {
            this.floatPower = true;
            return `(call $hd.pow_f64 ${left} ${right})`;
          }
          return `(call $hd.pow_i32 ${left} ${right})`;
        }
        if (expression.operator === "==" || expression.operator === "!=") {
          const equality = this.emitValueEquality(left, right, expression.left.type);
          return expression.operator === "==" ? equality : `(i32.eqz ${equality})`;
        }
        if (expression.left.type === "string") {
          if (expression.operator === "+") return `(call $hd.string_concat ${left} ${right})`;
          const comparison = `(call $hd.string_compare ${left} ${right})`;
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
        const checked: Readonly<Record<string, string>> = {
          "+": "$hd.add_i32",
          "-": "$hd.sub_i32",
          "*": "$hd.mul_i32",
          "<<": "$hd.shl_i32",
          ">>": "$hd.shr_i32",
        };
        if (expression.left.type === "i32" && checked[expression.operator]) {
          return `(call ${checked[expression.operator]} ${left} ${right})`;
        }
        if (
          expression.left.type === "i32" &&
          (expression.operator === "/" || expression.operator === "%")
        ) {
          const leftTemporary = this.allocateTemporary("i32");
          const rightTemporary = this.allocateTemporary("i32");
          const overflow =
            expression.operator === "/"
              ? [
                  `  (if (i32.and`,
                  `    (i32.eq (local.get ${leftTemporary}) (i32.const -2147483648))`,
                  `    (i32.eq (local.get ${rightTemporary}) (i32.const -1)))`,
                  `    (then ${this.emitRuntimePanic("integer-overflow")}))`,
                ]
              : [];
          return [
            `(block (result i32)`,
            `  (local.set ${leftTemporary} ${left})`,
            `  (local.set ${rightTemporary} ${right})`,
            `  (if (i32.eqz (local.get ${rightTemporary}))`,
            `    (then ${this.emitRuntimePanic("integer-division-by-zero")}))`,
            ...overflow,
            `  (i32.${expression.operator === "/" ? "div_s" : "rem_s"} (local.get ${leftTemporary}) (local.get ${rightTemporary}))`,
            `)`,
          ].join("\n");
        }
        const prefix = expression.left.type === "f64" ? "f64" : "i32";
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
        return `(${prefix}.${suffixes[expression.operator]} ${left} ${right})`;
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
        );
        const invocation = `(call ${functionName(expression.functionIndex)}${expression.arguments.length || expression.bounds?.length || expression.providers.length ? " " : ""}${[
          ...ordered.values,
          ...(expression.bounds ?? []).map((bound) => this.emitExpression(bound)),
          ...expression.providers.map((provider) => this.emitExpression(provider)),
        ].join(" ")})`;
        const rawResultType =
          expression.erasedResultType && isGenericValueType(expression.erasedResultType)
            ? expression.erasedResultType
            : expression.type;
        const call =
          ordered.setup.length === 0
            ? invocation
            : [
                `(block${rawResultType === "void" ? "" : ` (result ${this.watType(rawResultType)})`}`,
                ...ordered.setup.map((line) => `  ${line}`),
                `  ${invocation}`,
                `)`,
              ].join("\n");
        return expression.erasedResultType && isGenericValueType(expression.erasedResultType)
          ? this.unboxValue(call, expression.type)
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
        );
        const invocation = `(call ${functionName(expression.functionIndex)}${expression.arguments.length || expression.bounds?.length || expression.providers.length ? " " : ""}${[
          ...ordered.values,
          ...(expression.bounds ?? []).map((bound) => this.emitExpression(bound)),
          ...expression.providers.map((provider) => this.emitExpression(provider)),
        ].join(" ")})`;
        return ordered.setup.length === 0
          ? invocation
          : [
              `(block (result ${this.watType(expression.type)})`,
              ...ordered.setup.map((line) => `  ${line}`),
              `  ${invocation}`,
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
        const call = `(call $drive${expression.functionIndex} ${this.emitExpression(expression.suspension)})`;
        const rawType =
          expression.erasedResultType && isGenericValueType(expression.erasedResultType)
            ? expression.erasedResultType
            : expression.type;
        const guarded = expression.blockOn
          ? `(block${rawType === "void" ? "" : ` (result ${this.watType(rawType)})`} (if (global.get $hd.driver-active) (then ${this.emitRuntimePanic("suspension-nested-driver")})) ${call})`
          : call;
        return expression.erasedResultType && isGenericValueType(expression.erasedResultType)
          ? this.unboxValue(guarded, expression.type)
          : guarded;
      }
      case "suspend-cancel":
        return `(call $cancel${expression.functionIndex} ${this.emitExpression(expression.suspension)})`;
      case "trait-suspend-drive": {
        const erased = this.traitMethodErasesResult(expression.traitIndex, expression.methodIndex);
        const call = `(call ${traitSuspensionDriveName(expression.traitIndex, expression.methodIndex)} ${this.emitExpression(expression.suspension)})`;
        const guarded = expression.blockOn
          ? `(block${expression.type === "void" ? "" : ` (result ${erased ? "anyref" : this.watType(expression.type)})`} (if (global.get $hd.driver-active) (then ${this.emitRuntimePanic("suspension-nested-driver")})) ${call})`
          : call;
        return erased ? this.unboxValue(guarded, expression.type) : guarded;
      }
      case "trait-suspend-cancel":
        return `(call ${traitSuspensionCancelName(expression.traitIndex, expression.methodIndex)} ${this.emitExpression(expression.suspension)})`;
      case "suspension-drive": {
        const call = `(call $hd.suspension_drive ${this.emitExpression(expression.suspension)})`;
        const guarded = expression.blockOn
          ? `(block (result anyref) (if (global.get $hd.driver-active) (then ${this.emitRuntimePanic("suspension-nested-driver")})) ${call})`
          : call;
        return expression.type === "void"
          ? `(drop ${guarded})`
          : this.unboxValue(guarded, expression.type);
      }
      case "suspension-cancel":
        return `(call $hd.suspension_cancel ${this.emitExpression(expression.suspension)})`;
      case "function-value":
        return `(struct.new $closure${this.functionSignatures.get(expression.type)} (ref.func $fv${expression.functionIndex}) (ref.null any))`;
      case "closure-self":
        return `(struct.new $closure${this.functionSignatures.get(expression.type)} (ref.func $c${expression.closureIndex}) (local.get $env))`;
      case "closure":
        return `(struct.new $closure${this.functionSignatures.get(expression.type)} (ref.func $c${expression.closureIndex}) (struct.new $env${expression.closureIndex}${expression.captures.length ? " " : ""}${expression.captures.map((capture) => this.emitExpression(capture)).join(" ")}))`;
      case "closure-call": {
        const signature = this.functionSignatures.get(expression.callee.type);
        const temporary = this.allocateTemporary(expression.callee.type);
        return [
          `(block${expression.type === "void" ? "" : ` (result ${this.watType(expression.type)})`}`,
          `  (local.set ${temporary} ${this.emitExpression(expression.callee)})`,
          `  (call_ref $sig${signature}`,
          `    (struct.get $closure${signature} $closure${signature}env (local.get ${temporary}))`,
          ...expression.arguments.map((argument) => `    ${this.emitExpression(argument)}`),
          ...expression.providers.map((provider) => `    ${this.emitExpression(provider)}`),
          `    (struct.get $closure${signature} $closure${signature}fn (local.get ${temporary})))`,
          `)`,
        ].join("\n");
      }
      case "trait-wrap": {
        const temporary = this.allocateTemporary(expression.value.type);
        return `(block (result ${this.watType(expression.type)})
  (local.set ${temporary} ${this.emitExpression(expression.value)})
  ${this.emitTraitDictionaryPlan(expression.dictionary, this.boxWatValue(`(local.get ${temporary})`, expression.value.type))}
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
      case "trait-bound-dictionary":
        return `(local.get $bound${expression.boundIndex})`;
      case "trait-bound": {
        const trait = this.traitsByIndex.get(expression.traitIndex)!;
        const dictionary = `(local.get $bound${expression.boundIndex})`;
        const methods = trait.methods.map(
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
      case "trait-call": {
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
        );
        const call = [
          `(call_ref $tsig${trait.index}_${method.index}`,
          `  (struct.get $trait${receiverTrait.index} $trait${receiverTrait.index}value ${receiver})`,
          `  ${dispatch.dictionary}`,
          ...ordered.values.map((argument) => `  ${argument}`),
          ...expression.providers.map((provider) => `  ${this.emitExpression(provider)}`),
          `  (struct.get $trait${trait.index} $trait${trait.index}m${method.index} ${dispatch.dictionary}))`,
        ].join("\n");
        const result = expression.erasedResultType ? this.unboxValue(call, expression.type) : call;
        return [
          `(block${expression.type === "void" ? "" : ` (result ${this.watType(expression.type)})`}`,
          `  (local.set ${temporary} ${this.emitExpression(expression.receiver)})`,
          ...ordered.setup.map((line) => `  ${line}`),
          `  ${result}`,
          `)`,
        ].join("\n");
      }
      case "trait-suspend-construct": {
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
          true,
        );
        return [
          `(block (result (ref null ${traitSuspensionName(trait.index, method.index)}))`,
          `  (local.set ${temporary} ${this.emitExpression(expression.receiver)})`,
          ...ordered.setup.map((line) => `  ${line}`),
          `  (global.set $hd.host-call-site (i32.const ${expression.span.start.offset}))`,
          `  (call_ref $tsig${trait.index}_${method.index}`,
          `    (struct.get $trait${receiverTrait.index} $trait${receiverTrait.index}value ${receiver})`,
          `    ${dispatch.dictionary}`,
          ...ordered.values.map((argument) => `    ${argument}`),
          ...expression.providers.map((provider) => `    ${this.emitExpression(provider)}`),
          `    (struct.get $trait${trait.index} $trait${trait.index}m${method.index} ${dispatch.dictionary}))`,
          `)`,
        ].join("\n");
      }
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

  private emitContainerExpression(expression: HirExpression): string | undefined {
    switch (expression.kind) {
      case "data": {
        if (!expression.spread && expression.fields.length === 0)
          return `(struct.new $d${expression.dataIndex})`;
        const declaration = this.dataByIndex.get(expression.dataIndex)!;
        const spreadTemporary = expression.spread
          ? this.allocateTemporary(expression.spread.type)
          : undefined;
        const temporaries = expression.fields.map((field) => this.allocateTemporary(field.type));
        const sourceByField = new Map(
          expression.fieldIndices.map(
            (fieldIndex, sourceIndex) => [fieldIndex, sourceIndex] as const,
          ),
        );
        const storedFields = declaration.fields.map((field) => {
          const sourceIndex = sourceByField.get(field.index);
          if (sourceIndex === undefined) {
            if (!spreadTemporary)
              throw new Error(`data field '${field.name}' has no construction source`);
            return `(struct.get $d${expression.dataIndex} $d${expression.dataIndex}f${field.index} (local.get ${spreadTemporary}))`;
          }
          const value = `(local.get ${temporaries[sourceIndex]})`;
          return expression.erasedFieldTypes &&
            isGenericValueType(expression.erasedFieldTypes[field.index]!)
            ? this.boxWatValue(value, expression.fields[sourceIndex]!.type)
            : value;
        });
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...(expression.spread
            ? [`  (local.set ${spreadTemporary} ${this.emitExpression(expression.spread)})`]
            : []),
          ...expression.fields.map(
            (field, index) => `  (local.set ${temporaries[index]} ${this.emitExpression(field)})`,
          ),
          `  (struct.new $d${expression.dataIndex} ${storedFields.join(" ")})`,
          `)`,
        ].join("\n");
      }
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
          return expression.erasedFieldTypes &&
            isGenericValueType(expression.erasedFieldTypes[fieldIndex]!)
            ? this.boxWatValue(value, expression.fields[sourceIndex]!.type)
            : value;
        });
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...expression.fields.map(
            (field, index) => `  (local.set ${temporaries[index]} ${this.emitExpression(field)})`,
          ),
          `  (struct.new $e${expression.enumIndex} (i32.const ${expression.tag})${storedFields.length ? " " : ""}${storedFields.join(" ")})`,
          `)`,
        ].join("\n");
      }
      case "member": {
        const value = `(struct.get $d${expression.dataIndex} $d${expression.dataIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)})`;
        return expression.erasedFieldType && isGenericValueType(expression.erasedFieldType)
          ? this.unboxValue(value, expression.type)
          : value;
      }
      case "field-set": {
        const value = this.emitExpression(expression.value);
        const stored =
          expression.erasedFieldType && isGenericValueType(expression.erasedFieldType)
            ? this.boxWatValue(value, expression.value.type)
            : value;
        return `(struct.set $d${expression.dataIndex} $d${expression.dataIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)} ${stored})`;
      }
      case "enum-member": {
        const value = `(struct.get $e${expression.enumIndex} $e${expression.enumIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)})`;
        return expression.erasedFieldType && isGenericValueType(expression.erasedFieldType)
          ? this.unboxValue(value, expression.type)
          : value;
      }
      case "string-length":
        return `(call $hd.string_len ${this.emitExpression(expression.receiver)})`;
      case "string-transform":
        this.stringTransforms = true;
        return `(call $hd.string_${expression.operation} ${this.emitExpression(expression.receiver)})`;
      case "string-split":
        this.stringSplit = true;
        return `(call $hd.string_split ${this.emitExpression(expression.receiver)} ${this.emitExpression(expression.separator)})`;
      case "string-starts-with":
        return `(call $hd.string_starts_with ${this.emitExpression(expression.receiver)} ${this.emitExpression(expression.prefix)})`;
      case "list-length":
        return `(struct.get $hd.vector $hd.vector-size (ref.as_non_null ${this.emitExpression(expression.receiver)}))`;
      case "list-iterator":
        return this.emitListIterator(expression.receiver);
      case "iterator-next":
        return this.emitIteratorNext(expression.receiver, expression.elementType);
      case "map-iterator":
        return this.emitMapIterator(expression.receiver);
      case "list-index":
        return this.unboxValue(
          `(call $hd.vector_get (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.emitExpression(expression.index)})`,
          expression.elementType,
        );
      case "list-set":
        return `(call $hd.vector_set (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.emitExpression(expression.index)} ${this.boxValue(expression.value, expression.elementType)})`;
      case "list-append":
        return `(call $hd.vector_append (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.value, expression.elementType)})`;
      case "tuple-index":
        return this.unboxValue(
          `(array.get $hd.list (ref.as_non_null ${this.emitExpression(expression.receiver)}) (i32.const ${expression.index}))`,
          expression.elementType,
        );
      case "map-length":
        return `(struct.get $hd.map $hd.map-size (ref.as_non_null ${this.emitExpression(expression.receiver)}))`;
      case "map-index":
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
        return `(block (drop ${this.emitExpression(expression.message)}) ${this.emitRuntimePanic("explicit-panic")})`;
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
        if (expression.elseBody.length > 0) {
          const result =
            expression.type === "void" || expression.type === "never"
              ? ""
              : ` (result ${this.watType(expression.type)})`;
          const elseBody = this.emitBlock(expression.elseBody, expression.type);
          return [
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
          ].join("\n");
        }
        return [
          `(block ${labels.breakLabel}`,
          `  (loop ${labels.continueLabel}`,
          `    (br_if ${labels.breakLabel} (i32.eqz ${this.emitExpression(expression.condition)}))`,
          indent(body, 4),
          `    (br ${labels.continueLabel})`,
          `  )`,
          `)`,
        ].join("\n");
      }
      case "match": {
        const subject = this.allocateTemporary(expression.subject.type);
        const result =
          expression.type === "void" || expression.type === "never"
            ? ""
            : ` (result ${this.watType(expression.type)})`;
        return [
          `(block${result}`,
          `  (local.set ${subject} ${this.emitExpression(expression.subject)})`,
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
    stage = false,
  ): EmittedArguments {
    const emitValue = (argument: HirExpression, parameterIndex: number): string => {
      const formal = erasedParameterTypes?.[parameterIndex];
      if (
        formal &&
        functionParts(formal) &&
        formal !== argument.type &&
        functionParts(argument.type)
      ) {
        return this.emitCallableAdaptation(argument, formal, argument.type);
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
    return this.boxWatValue(this.emitExpression(expression), type);
  }

  protected emitCallableAdaptation(
    expression: HirExpression,
    formalType: ValueType,
    actualType: ValueType,
  ): string {
    const key = `${formalType}\u0000${actualType}`;
    let adapter = this.callableAdapters.get(key);
    if (!adapter) {
      adapter = { index: this.callableAdapters.size, formalType, actualType };
      this.callableAdapters.set(key, adapter);
    }
    const formalSignature = this.functionSignatures.get(formalType);
    return `(struct.new $closure${formalSignature} (ref.func $adapt${adapter.index}) ${this.emitExpression(expression)})`;
  }

  emitCallableAdapters(): string {
    return this.adapters
      .map((adapter) => {
        const formal = functionParts(adapter.formalType)!;
        const actual = functionParts(adapter.actualType)!;
        const formalSignature = this.functionSignatures.get(adapter.formalType);
        const actualSignature = this.functionSignatures.get(adapter.actualType);
        const parameters = formal.parameters.map(
          (parameter, index) => `(param $a${index} ${this.watType(parameter)})`,
        );
        const providers = formal.requirements.map(
          (requirement, index) => `(param $p${index} ${this.providerType(requirement)})`,
        );
        const result = formal.suspending
          ? ` (result (ref null $hd.suspension))`
          : formal.result === "void"
            ? ""
            : ` (result ${this.watType(formal.result)})`;
        const closure = `(ref.cast (ref $closure${actualSignature}) (local.get $env))`;
        const arguments_ = formal.parameters.map((parameter, index) => {
          const value = `(local.get $a${index})`;
          return isGenericValueType(parameter)
            ? this.unboxValue(value, actual.parameters[index]!)
            : value;
        });
        const concreteFormal = new Map(
          formal.requirements
            .map((requirement, index) => [requirement, index] as const)
            .filter(([requirement]) => !isRowRequirement(requirement)),
        );
        const formalUnion = formal.requirements.reduceRight((parent, requirement, index) => {
          if (isRowRequirement(requirement))
            return `(call $hd.provider_concat (local.get $p${index}) ${parent})`;
          const type = this.traitsByName.has(nominalGenericParts(requirement)?.name ?? requirement)
            ? `trait:${requirement}`
            : `provider:${requirement}`;
          return `(struct.new $hd.providers (i32.const ${this.providerKey(requirement)}) ${this.boxProvider(`(local.get $p${index})`, type)} ${parent})`;
        }, `(ref.null $hd.providers)`);
        const actualProviders = actual.requirements.map((requirement) => {
          if (isRowRequirement(requirement)) return formalUnion;
          const direct = concreteFormal.get(requirement);
          if (direct !== undefined) return `(local.get $p${direct})`;
          if (formal.requirements.length === 0)
            throw new Error(
              `cannot adapt requirement '${requirement}' from ${adapter.formalType} to ${adapter.actualType}`,
            );
          return this.unboxProvider(
            `(call $hd.provider_get ${formalUnion} (i32.const ${this.providerKey(requirement)}))`,
            requirement,
          );
        });
        const call = `(call_ref $sig${actualSignature} (struct.get $closure${actualSignature} $closure${actualSignature}env ${closure})${arguments_.length ? " " : ""}${arguments_.join(" ")}${actualProviders.length ? " " : ""}${actualProviders.join(" ")} (struct.get $closure${actualSignature} $closure${actualSignature}fn ${closure}))`;
        const body =
          !formal.suspending && isGenericValueType(formal.result)
            ? this.boxWatValue(call, actual.result)
            : call;
        return `(func $adapt${adapter.index} (type $sig${formalSignature}) (param $env anyref) ${[...parameters, ...providers].join(" ")}${result}\n  ${body}\n)`;
      })
      .join("\n\n");
  }

  private emitTraitDictionaryPlan(plan: HirTraitDictionaryPlan, value: string): string {
    const implementation = this.implementationsByIndex.get(plan.implementationIndex)!;
    return this.emitTraitDictionary(
      implementation,
      value,
      plan.bounds.map((bound) => this.emitExpression(bound)),
      plan.supertraits.map((parent) => this.emitTraitDictionaryPlan(parent, value)),
    );
  }

  emitTraitAdapters(): string {
    return [...this.implementationsByIndex.values()]
      .flatMap((implementation) => {
        const trait = this.traitsByIndex.get(implementation.traitIndex)!;
        const traitSubstitutions = new Map([
          ...trait.genericParameters.map(
            (parameter, index) => [parameter, implementation.traitArguments[index]!] as const,
          ),
          ...trait.associatedTypes.map(
            (associated, index) =>
              [`Self::${associated.name}`, implementation.associatedTypes[index]!] as const,
          ),
          ["Self", implementation.targetType] as const,
        ]);
        return implementation.methodFunctions.flatMap((mapping) => {
          const method = trait.methods[mapping.methodIndex]!;
          const parameters = method.parameters.map(
            (parameter, index) => `(param $a${index} ${this.watType(parameter)})`,
          );
          const providers = method.requirements.map(
            (requirement, index) => `(param $p${index} ${this.providerType(requirement)})`,
          );
          const result = method.result === "void" ? "" : ` (result ${this.watType(method.result)})`;
          const dictionary = `(ref.cast (ref $trait${trait.index}) (local.get $dictionary))`;
          const boundPack = `(ref.as_non_null (struct.get $trait${trait.index} $trait${trait.index}bounds ${dictionary}))`;
          const arguments_ = [
            ...(method.associated
              ? []
              : [this.unboxValue(`(local.get $self)`, implementation.targetType)]),
            ...method.parameters.map((parameter, index) =>
              parameter === "generic:Self"
                ? this.unboxValue(`(local.get $a${index})`, implementation.targetType)
                : containsGenericValueType(parameter)
                  ? this.unboxValue(
                      `(local.get $a${index})`,
                      substituteTypeParameters(parameter, traitSubstitutions),
                    )
                  : `(local.get $a${index})`,
            ),
            ...implementation.genericBounds.map((bound, index) =>
              this.unboxValue(
                `(array.get $hd.list ${boundPack} (i32.const ${index}))`,
                `trait:${
                  bound.traitArguments.length > 0
                    ? nominalGenericType(bound.traitName, bound.traitArguments)
                    : bound.traitName
                }`,
              ),
            ),
            ...method.requirements.map((_, index) => `(local.get $p${index})`),
          ];
          if (!method.suspending) {
            const call = `(call ${functionName(mapping.functionIndex)} ${arguments_.join(" ")})`;
            const body = containsGenericValueType(method.result)
              ? this.boxWatValue(call, substituteTypeParameters(method.result, traitSubstitutions))
              : call;
            return [
              `(func $tadapt${implementation.index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref) ${[...parameters, ...providers].join(" ")}${result}\n  ${body}\n)`,
            ];
          }
          const wrapper = traitSuspensionName(trait.index, method.index);
          const frame = `$s${mapping.functionIndex}`;
          const constructor = `(func $tadapt${implementation.index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref) ${[...parameters, ...providers].join(" ")} (result (ref null ${wrapper}))\n  (struct.new ${wrapper}\n    (call ${functionName(mapping.functionIndex)} ${arguments_.join(" ")})\n    (ref.func $tspolladapt${implementation.index}_${method.index})\n    (ref.func $tscanceladapt${implementation.index}_${method.index})\n    (ref.func $tsresultadapt${implementation.index}_${method.index}))\n)`;
          const poll = `(func $tspolladapt${implementation.index}_${method.index} (type $tspollsig${trait.index}_${method.index}) (param $inner anyref) (result i32)\n  (call $poll${mapping.functionIndex} (ref.cast (ref null ${frame}) (local.get $inner)))\n)`;
          const cancel = `(func $tscanceladapt${implementation.index}_${method.index} (type $tscancelsig${trait.index}_${method.index}) (param $inner anyref)\n  (call $cancel${mapping.functionIndex} (ref.cast (ref null ${frame}) (local.get $inner)))\n)`;
          const resultBody =
            method.result === "void"
              ? ""
              : `\n  ${
                  containsGenericValueType(method.result)
                    ? this.boxWatValue(
                        `(struct.get ${frame} ${frame}result (ref.cast (ref null ${frame}) (local.get $inner)))`,
                        substituteTypeParameters(method.result, traitSubstitutions),
                      )
                    : `(struct.get ${frame} ${frame}result (ref.cast (ref null ${frame}) (local.get $inner)))`
                }`;
          const resultAdapter = `(func $tsresultadapt${implementation.index}_${method.index} (type $tsresultsig${trait.index}_${method.index}) (param $inner anyref)${result}${resultBody}\n)`;
          return [constructor, poll, cancel, resultAdapter];
        });
      })
      .join("\n\n");
  }

  emitTraitSuspensionHelpers(): string {
    return [...this.traitsByIndex.values()]
      .flatMap((trait) =>
        trait.methods.flatMap((method) => {
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

  protected boxProvider(value: string, type: ValueType): string {
    return type.startsWith("provider:") ? `(struct.new $hd.box-extern ${value})` : value;
  }

  protected unboxProvider(value: string, requirement: string): string {
    const trait = this.traitsByName.get(requirement);
    return trait
      ? `(ref.cast (ref null $trait${trait.index}) ${value})`
      : `(struct.get $hd.box-extern $hd.box-extern-value (ref.cast (ref $hd.box-extern) ${value}))`;
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
          ? `(i32.eq (call $hd.string_compare (local.get ${subject}) ${value}) (i32.const 0))`
          : arm.literal.type === "f64"
            ? `(f64.eq (local.get ${subject}) ${value})`
            : `(i32.eq (local.get ${subject}) ${value})`;
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
            ? `(i32.eq (call $hd.string_compare ${actual} ${expected}) (i32.const 0))`
            : test.literal!.type === "f64"
              ? `(f64.eq ${actual} ${expected})`
              : `(i32.eq ${actual} ${expected})`;
      condition = condition ? `(i32.and ${condition} ${testCondition})` : testCondition;
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
      return test.tag !== undefined
        ? `(struct.get $e${test.tagEnumIndex} $e${test.tagEnumIndex}tag ${value})`
        : value;
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

  protected emitPatternAccess(subject: string, path: readonly HirPatternAccessStep[]): string {
    return path.reduce((value, step) => {
      if (step.kind === "erased-variant") {
        return this.unboxValue(
          `(struct.get $hd.variant $hd.variant-payload ${value})`,
          step.valueType,
        );
      }
      const prefix = step.kind === "data" ? `$d${step.typeIndex}` : `$e${step.typeIndex}`;
      const raw = `(struct.get ${prefix} ${prefix}f${step.fieldIndex} ${value})`;
      return step.erasedFieldType && isGenericValueType(step.erasedFieldType)
        ? this.unboxValue(raw, step.valueType)
        : raw;
    }, `(local.get ${subject})`);
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
