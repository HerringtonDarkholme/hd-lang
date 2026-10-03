import { cellInner } from "../checker/captured-cells.ts";
import { DiagnosticError } from "../diagnostics.ts";
import type {
  HirBuiltinTraitImplementation,
  HirExpression,
  HirData,
  HirEnum,
  HirFunction,
  HirStatement,
  HirTrait,
  HirTraitImplementation,
  HirTypeSubstitution,
  ValueType,
} from "../hir.ts";
import {
  contextKeys,
  functionParts,
  isErasedVariant,
  mutableInner,
  nominalGenericParts,
  readonlyType,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
  inputsInner,
  CURSOR_TYPE,
  type FunctionParts,
} from "../types.ts";
import type { SuspensionPlan } from "./suspension.ts";

interface BuiltinTraitAdapter {
  readonly index: number;
  readonly implementation: HirBuiltinTraitImplementation;
  readonly boundTraits: readonly number[];
}
import { runtimePanicCode, type RuntimePanicName } from "../runtime-panic.ts";
import { numericType } from "../numeric.ts";
import { boxScalar, unboxScalar } from "./scalars.ts";
import {
  containsGenericValueType,
  exportName,
  functionName,
  isGenericValueType,
  localName,
  providerWatType,
  traitSuspensionName,
  traitTypeBase,
} from "./shared.ts";

interface LoopContext {
  readonly breakLabel: string;
  readonly continueLabel: string;
  readonly result?: ValueType;
}

export interface CleanupFrame {
  readonly cleanups: Array<readonly HirStatement[]>;
  readonly loopBoundary: boolean;
}

export interface EmittedArguments {
  readonly setup: readonly string[];
  readonly values: readonly string[];
}

interface TraitDictionaryPath {
  readonly dictionary: string;
  readonly trait: HirTrait;
}

interface CallableAdapter {
  readonly index: number;
  readonly formalType: ValueType;
  readonly actualType: ValueType;
  readonly typeSubstitutions: readonly HirTypeSubstitution[];
}

interface SuspensionResultAdapter {
  readonly index: number;
  readonly body: string;
}

export class EmitterContext {
  protected readonly dataByName: ReadonlyMap<string, HirData>;
  protected readonly dataByIndex: ReadonlyMap<number, HirData>;
  protected readonly enumByName: ReadonlyMap<string, HirEnum>;
  protected readonly functionSignatures: ReadonlyMap<ValueType, number>;
  protected readonly contextNames: ReadonlyMap<ValueType, number>;
  protected readonly closuresByIndex: ReadonlyMap<number, HirFunction>;
  protected readonly traitsByIndex: ReadonlyMap<number, HirTrait>;
  protected readonly traitsByName: ReadonlyMap<string, HirTrait>;
  protected readonly implementationsByIndex: ReadonlyMap<number, HirTraitImplementation>;
  protected readonly suspensionPlans: ReadonlyMap<number, SuspensionPlan>;
  protected readonly hostCapabilities: ReadonlySet<string>;
  protected loopCounter = 0;
  protected readonly loops: LoopContext[] = [];
  protected readonly cleanupFrames: CleanupFrame[] = [];
  protected readonly temporaryTypes: ValueType[] = [];
  protected floatPower = false;
  protected floatRemainder = false;
  protected currentRequirements: readonly string[] = [];
  /** The HIR function whose body is currently emitted, for host call-site identity. */
  protected currentFunctionIndex = -1;
  protected readonly callableAdapters = new Map<string, CallableAdapter>();
  protected readonly suspensionResultAdapters = new Map<string, SuspensionResultAdapter>();
  protected readonly builtinTraitAdapters = new Map<string, BuiltinTraitAdapter>();
  protected readonly providerKeys = new Map<string, number>();
  private readonly stringKernel: ReadonlyMap<string, number>;

  constructor(
    data: readonly HirData[],
    enums: readonly HirEnum[],
    functionSignatures: ReadonlyMap<ValueType, number>,
    contextNames: ReadonlyMap<ValueType, number>,
    closures: readonly HirFunction[],
    traits: readonly HirTrait[],
    implementations: readonly HirTraitImplementation[],
    suspensionPlans: ReadonlyMap<number, SuspensionPlan> = new Map(),
    hostCapabilities: readonly string[] = [],
    functions: readonly HirFunction[] = [],
  ) {
    this.stringKernel = new Map(
      functions
        .filter((declaration) => STRING_KERNEL_NAMES.has(declaration.name))
        .map((declaration) => [declaration.name, declaration.index]),
    );
    this.dataByName = new Map(data.map((declaration) => [declaration.name, declaration]));
    this.dataByIndex = new Map(data.map((declaration) => [declaration.index, declaration]));
    this.enumByName = new Map(enums.map((declaration) => [declaration.name, declaration]));
    this.functionSignatures = functionSignatures;
    this.contextNames = contextNames;
    this.closuresByIndex = new Map(closures.map((closure) => [closure.index, closure]));
    this.traitsByIndex = new Map(traits.map((trait) => [trait.index, trait]));
    this.traitsByName = new Map(traits.map((trait) => [trait.name, trait]));
    this.implementationsByIndex = new Map(
      implementations.map((implementation) => [implementation.index, implementation]),
    );
    this.suspensionPlans = suspensionPlans;
    this.hostCapabilities = new Set(hostCapabilities);
  }

  /**
   * The `std.text` function that string `+` and interpolation (`concat`),
   * `==` (`equal`), or the order (`compare`) compile to a call of; every
   * program declares them (checker/standard-library.ts).
   */
  protected stringFunction(name: StringKernelFunction): string {
    const index = this.stringKernel.get(stringKernelName(name));
    if (index === undefined) throw new Error(`std.text.string_${name} is not declared`);
    return functionName(index);
  }

  /** The map runtime's string-key equality (runtime/map.wat), std's `string_equal`. */
  emitStringKeyEqual(): string {
    return [
      `  (func $hd.string_key_equal (param $left anyref) (param $right anyref) (result i32)`,
      `    (call ${this.stringFunction("equal")}`,
      `      (ref.cast (ref null $hd.string) (local.get $left))`,
      `      (ref.cast (ref null $hd.string) (local.get $right))))`,
    ].join("\n");
  }

  protected emitTraitDictionary(
    implementation: HirTraitImplementation,
    value: string,
    bounds: readonly string[],
    parents: readonly string[],
  ): string {
    const trait = this.traitsByIndex.get(implementation.traitIndex)!;
    const adapters = trait.methods.map(
      (method) => `(ref.func $tadapt${implementation.index}_${method.index})`,
    );
    const boundPack =
      bounds.length > 0
        ? `(array.new_fixed $hd.list ${bounds.length} ${bounds.join(" ")})`
        : `(ref.null $hd.list)`;
    const fields = [...adapters, ...parents];
    return `(struct.new $trait${trait.index} ${value} ${boundPack}${fields.length > 0 ? ` ${fields.join(" ")}` : ""})`;
  }

  protected traitMethodErasesResult(traitIndex: number, methodIndex: number): boolean {
    const method = this.traitsByIndex.get(traitIndex)?.methods[methodIndex];
    return method !== undefined && containsGenericValueType(method.result);
  }

  protected traitDictionaryPath(
    receiverType: ValueType,
    path: readonly number[] | undefined,
    receiver: string,
  ): TraitDictionaryPath {
    let trait = this.traitsByName.get(traitTypeBase(receiverType))!;
    let dictionary = receiver;
    for (const fieldIndex of path ?? []) {
      const supertrait = trait.supertraits[fieldIndex]!;
      dictionary = `(struct.get $trait${trait.index} $trait${trait.index}s${fieldIndex} ${dictionary})`;
      trait = this.traitsByIndex.get(supertrait.traitIndex)!;
    }
    return { dictionary, trait };
  }

  /**
   * A closure keeps the enclosing function's bound dictionaries in its
   * environment after its captures (`closureBoundLoads`).
   */
  protected closureBoundValues(closureIndex: number): string[] {
    const bounds = this.closuresByIndex.get(closureIndex)?.genericBounds ?? [];
    return bounds.map((_, index) => `(local.get $bound${index})`);
  }

  /** A bound's dictionary, or a supertrait's dictionary reached through it. */
  protected boundDictionary(
    boundIndex: number,
    supertrait?: { readonly sourceTraitIndex: number; readonly path: readonly number[] },
  ): string {
    const dictionary = `(local.get $bound${boundIndex})`;
    if (!supertrait) return dictionary;
    const source = this.traitsByIndex.get(supertrait.sourceTraitIndex)!;
    return this.traitDictionaryPath(`trait:${source.name}`, supertrait.path, dictionary).dictionary;
  }

  protected emitTraitUpcast(
    sourceTrait: HirTrait,
    targetTrait: HirTrait,
    path: readonly number[],
    value: string,
  ): string {
    const { dictionary } = this.traitDictionaryPath(`trait:${sourceTrait.name}`, path, value);
    const methods = targetTrait.methods.map(
      (method) =>
        `(struct.get $trait${targetTrait.index} $trait${targetTrait.index}m${method.index} ${dictionary})`,
    );
    const parents = targetTrait.supertraits.map(
      (_, index) =>
        `(struct.get $trait${targetTrait.index} $trait${targetTrait.index}s${index} ${dictionary})`,
    );
    return `(struct.new $trait${targetTrait.index} (struct.get $trait${sourceTrait.index} $trait${sourceTrait.index}value ${value}) (struct.get $trait${targetTrait.index} $trait${targetTrait.index}bounds ${dictionary}) ${[...methods, ...parents].join(" ")})`;
  }

  /** A parameter's Wasm type; a void `self` (`impl ... for void`) is a null anyref. */
  parameterWatType(type: ValueType): string {
    return type === "void" ? "anyref" : this.watType(type);
  }

  watType(type: ValueType): string {
    if (cellInner(type) !== undefined) return "(ref null $hd.cell)";
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.watType(mutable);
    if (isGenericValueType(type)) return "anyref";
    const numeric = numericType(type);
    if (numeric) return numeric.wasm;
    if (type === "bool" || type === "char") return "i32";
    if (type === "string") return "(ref null $hd.string)";
    if (type.startsWith("provider-row:")) return "(ref null $hd.providers)";
    if (type.startsWith("provider:")) return "externref";
    if (type.startsWith("trait:") && !type.endsWith("?"))
      return `(ref null $trait${this.traitsByName.get(traitTypeBase(type))?.index})`;
    if (contextKeys(type)) return `(ref null $context${this.contextNames.get(type)})`;
    if (tupleParts(type) !== undefined) return `(ref null $hd.list)`;
    if (storedSuspensionParts(type)) return `(ref null $hd.suspension)`;
    const suspension = suspensionParts(type);
    if (suspension) return `(ref null $s${suspension.functionIndex})`;
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension)
      return `(ref null ${traitSuspensionName(traitSuspension.traitIndex, traitSuspension.methodIndex)})`;
    if (isErasedVariant(type)) return "(ref null $hd.variant)";
    const callable = functionParts(type);
    if (callable) return `(ref null $closure${this.functionSignatures.get(type)})`;
    if (type === "void") return "";
    const data = this.dataByName.get(type);
    const nominalData = nominalGenericParts(type);
    if (nominalData?.name === CURSOR_TYPE && nominalData.arguments.length === 1)
      return `(ref null $hd.iterator)`;
    if (nominalData?.name === "List" && nominalData.arguments.length === 1)
      return `(ref null $hd.vector)`;
    if (nominalData?.name === "Map" && nominalData.arguments.length === 2)
      return `(ref null $hd.map)`;
    if (nominalData && this.dataByName.has(nominalData.name))
      return `(ref null $d${this.dataByName.get(nominalData.name)!.index})`;
    if (nominalData && this.enumByName.has(nominalData.name))
      return `(ref null $e${this.enumByName.get(nominalData.name)!.index})`;
    if (data) return `(ref null $d${data.index})`;
    const enumType = this.enumByName.get(type);
    if (enumType) return `(ref null $e${enumType.index})`;
    throw new Error(`cannot emit unknown type '${type}'`);
  }

  get requiresFloatPower(): boolean {
    return this.floatPower;
  }

  get requiresFloatRemainder(): boolean {
    return this.floatRemainder;
  }

  protected emitRuntimePanic(name: RuntimePanicName): string {
    return `(call $hd.panic (i32.const ${runtimePanicCode(name)})) unreachable`;
  }

  get adapters(): readonly CallableAdapter[] {
    return [...this.callableAdapters.values()];
  }

  get resultAdapters(): readonly SuspensionResultAdapter[] {
    return [...this.suspensionResultAdapters.values()];
  }

  protected providerKey(key: string): number {
    let index = this.providerKeys.get(key);
    if (index === undefined) {
      index = this.providerKeys.size;
      this.providerKeys.set(key, index);
    }
    return index;
  }

  protected providerType(requirement: string): string {
    return providerWatType(requirement, this.traitsByName);
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

  protected hostTrait(requirement: string): HirTrait | undefined {
    if (!this.hostCapabilities.has(requirement)) return undefined;
    return this.traitsByName.get(nominalGenericParts(requirement)?.name ?? requirement);
  }

  protected entryProviderParameters(declaration: HirFunction): readonly string[] {
    return declaration.requirements.map((requirement, index) =>
      this.hostTrait(requirement)
        ? `(param $provider${index} externref)`
        : `(param $provider${index} ${this.providerType(requirement)})`,
    );
  }

  protected entryProviderArguments(declaration: HirFunction): readonly string[] {
    return declaration.requirements.map((requirement, index) => {
      const trait = this.hostTrait(requirement);
      return trait
        ? `(call $hd.host_trait${trait.index} (local.get $provider${index}))`
        : `(local.get $provider${index})`;
    });
  }

  protected emitHostProviderExport(declaration: HirFunction, internalName: string): string[] {
    const parameters = declaration.parameters.map(
      (parameter) =>
        `(param ${localName(parameter.index)} ${this.parameterWatType(parameter.type)})`,
    );
    const providers = this.entryProviderParameters(declaration);
    const result =
      declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const arguments_ = [
      ...declaration.parameters.map((parameter) => `(local.get ${localName(parameter.index)})`),
      ...this.entryProviderArguments(declaration),
    ];
    return [
      `(func (export ${exportName(declaration.name)})${[...parameters, ...providers].map((parameter) => ` ${parameter}`).join("")}${result}`,
      `  (call ${internalName}${arguments_.map((argument) => ` ${argument}`).join("")}))`,
    ];
  }

  // A `main` with a non-void result is exported through a wrapper that
  // returns its exit code (10-modules.md#exit-status), or -1 for an `.Err`,
  // which the host reports as an error before it exits with 1.
  protected emitResultEntryExport(declaration: HirFunction, internalName: string): string[] {
    if (
      !declaration.entry ||
      declaration.closure ||
      declaration.suspending ||
      declaration.parameters.length > 0 ||
      declaration.genericBounds.length > 0 ||
      declaration.result === "void" ||
      declaration.result === "never"
    )
      return [];
    const providers = this.entryProviderParameters(declaration);
    const call = `(call ${internalName}${this.entryProviderArguments(declaration)
      .map((argument) => ` ${argument}`)
      .join("")})`;
    const locals: string[] = [];
    const report = this.emitEntryReport(call, declaration.result, locals);
    return [
      `(func (export ${exportName("main")})${providers.length ? " " + providers.join(" ") : ""} (result i32)`,
      ...locals.map((local) => `  ${local}`),
      `  ${report})`,
    ];
  }

  protected hostSafe(type: ValueType): boolean {
    return (
      type === "i32" ||
      type === "u32" ||
      type === "u8" ||
      type === "bool" ||
      type === "char" ||
      type === "f64" ||
      type === "void"
    );
  }

  /** A panic node that stands for a call the prototype cannot emit, as `all!`. */
  protected rejectUnsupported(expression: Extract<HirExpression, { kind: "panic" }>): void {
    if (expression.unsupported)
      throw new DiagnosticError([{ ...expression.unsupported, span: expression.span }]);
  }

  /**
   * A call's erased result type when its value needs restoring: a boxed
   * generic, or a function type such as `Fn[Args, O, $ R]` that is adapted
   * back to the instantiated function type.
   */
  protected erasedResultType(
    erased: ValueType | undefined,
    type: ValueType,
  ): ValueType | undefined {
    if (erased === undefined) return undefined;
    if (isGenericValueType(erased)) return erased;
    const readonly = readonlyType(type);
    return erased !== readonly && functionParts(erased) && functionParts(readonly)
      ? erased
      : undefined;
  }

  protected restoreErasedResult(
    value: string,
    erased: ValueType,
    type: ValueType,
    typeSubstitutions: readonly HirTypeSubstitution[] = [],
  ): string {
    return isGenericValueType(erased)
      ? this.unboxValue(value, type)
      : this.adaptCallable(value, readonlyType(type), erased, typeSubstitutions);
  }

  /** Restore a callable result with the adapter captured by its suspension frame. */
  protected restoreSuspensionResult(
    value: string,
    frame: string,
    functionIndex: number,
    type: ValueType,
  ): string {
    const adapted = `(call_ref $hd.suspension-result-adapt-sig ${value} (ref.as_non_null (struct.get $s${functionIndex} $s${functionIndex}result_adapter ${frame})))`;
    return this.unboxValue(adapted, type);
  }

  /** Restore a callable result captured by a dynamically dispatched suspension. */
  protected restoreTraitSuspensionResult(
    value: string,
    frame: string,
    traitIndex: number,
    methodIndex: number,
    type: ValueType,
  ): string {
    const wrapper = traitSuspensionName(traitIndex, methodIndex);
    const adapted = `(call_ref $hd.suspension-result-adapt-sig ${value} (ref.as_non_null (struct.get ${wrapper} ${wrapper}result_adapter ${frame})))`;
    return this.unboxValue(adapted, type);
  }

  /**
   * The arguments a callable adapter passes on: boxed or unboxed per
   * parameter, and for `Fn[Args, O, $ R]`'s one input `*Args` the inputs tuple
   * unpacked into the actual parameters, or packed from the formal ones
   * (07-functions.md#r-fn.type.ctor.inputs).
   */
  protected adaptedArguments(formal: FunctionParts, actual: FunctionParts): string[] {
    const formalPacked = formal.parameters.length === 1 && inputsInner(formal.parameters[0]!);
    const actualPacked = actual.parameters.length === 1 && inputsInner(actual.parameters[0]!);
    if (formalPacked && !actualPacked)
      return actual.parameters.map((parameter, index) =>
        this.unboxValue(
          `(array.get $hd.list (ref.cast (ref $hd.list) (local.get $a0)) (i32.const ${index}))`,
          parameter,
        ),
      );
    if (actualPacked && !formalPacked) {
      const boxed = formal.parameters.map((parameter, index) =>
        this.boxWatValue(`(local.get $a${index})`, parameter),
      );
      return [
        boxed.length === 0
          ? `(array.new_default $hd.list (i32.const 0))`
          : `(array.new_fixed $hd.list ${boxed.length} ${boxed.join(" ")})`,
      ];
    }
    return formal.parameters.map((parameter, index) => {
      const value = `(local.get $a${index})`;
      const actualParameter = actual.parameters[index]!;
      if (isGenericValueType(parameter) && !isGenericValueType(actualParameter))
        return this.unboxValue(value, actualParameter);
      if (isGenericValueType(actualParameter) && !isGenericValueType(parameter))
        return this.boxWatValue(value, parameter);
      return value;
    });
  }

  /** Wraps the closure `value` of type `actualType` as a closure of `formalType`. */
  protected adaptCallable(
    value: string,
    formalType: ValueType,
    actualType: ValueType,
    typeSubstitutions: readonly HirTypeSubstitution[] = [],
  ): string {
    const substitutionKey = typeSubstitutions
      .map(({ parameter, type }) => `${parameter}=${type}`)
      .join("\u0001");
    const key = `${formalType}\u0000${actualType}\u0000${substitutionKey}`;
    let adapter = this.callableAdapters.get(key);
    if (!adapter) {
      adapter = {
        index: this.callableAdapters.size,
        formalType,
        actualType,
        typeSubstitutions,
      };
      this.callableAdapters.set(key, adapter);
    }
    const formalSignature = this.functionSignatures.get(formalType);
    return `(struct.new $closure${formalSignature} (ref.func $adapt${adapter.index}) ${value})`;
  }

  /**
   * A suspension frame carries this call-site adapter with the value.  Keeping
   * it in the frame matters when two generic instantiations have the same
   * normalized result type but different requirement-slot permutations.
   */
  protected suspensionResultAdapter(
    formalType: ValueType,
    actualType: ValueType,
    typeSubstitutions: readonly HirTypeSubstitution[],
  ): string {
    const substitutionKey = typeSubstitutions
      .map(({ parameter, type }) => `${parameter}=${type}`)
      .join("\u0001");
    const key = `${formalType}\u0000${actualType}\u0000${substitutionKey}`;
    let adapter = this.suspensionResultAdapters.get(key);
    if (!adapter) {
      const actualSignature = this.functionSignatures.get(actualType);
      adapter = {
        index: this.suspensionResultAdapters.size,
        body: this.adaptCallable(
          `(ref.cast (ref $closure${actualSignature}) (local.get $value))`,
          formalType,
          actualType,
          typeSubstitutions,
        ),
      };
      this.suspensionResultAdapters.set(key, adapter);
    }
    return `$sresultadapt${adapter.index}`;
  }

  /**
   * A value stored into a field whose declared type mentions a generic
   * parameter: a generic value is boxed, and a closure is adapted to the
   * erased function type.
   */
  protected storeErased(
    value: string,
    erased: ValueType | undefined,
    type: ValueType,
    typeSubstitutions: readonly HirTypeSubstitution[] = [],
  ): string {
    if (!erased) return value;
    if (isGenericValueType(erased)) return this.boxWatValue(value, type);
    const readonly = readonlyType(type);
    return functionParts(erased) && functionParts(readonly) && erased !== readonly
      ? this.adaptCallable(value, erased, readonly, typeSubstitutions)
      : value;
  }

  /** The inverse of `storeErased` for a field read. */
  protected loadErased(
    value: string,
    erased: ValueType | undefined,
    type: ValueType,
    typeSubstitutions: readonly HirTypeSubstitution[] = [],
  ): string {
    if (!erased) return value;
    if (isGenericValueType(erased)) return this.unboxValue(value, type);
    const readonly = readonlyType(type);
    return functionParts(erased) && functionParts(readonly) && erased !== readonly
      ? this.adaptCallable(value, readonly, erased, typeSubstitutions)
      : value;
  }

  protected boxWatValue(value: string, type: ValueType): string {
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.boxWatValue(value, mutable);
    if (isGenericValueType(type)) return value;
    return boxScalar(value, type);
  }

  // `report()` of an entry result: 0 for `void`, the `u8` of `ExitCode`, and
  // for a `Result` its `.Ok` value's code or -1 for `.Err`. The checker admits
  // no other result (termination.ts runnableEntryResult).
  protected emitEntryReport(value: string, type: ValueType, locals: string[]): string {
    const parts = resultParts(readonlyType(type));
    if (parts) {
      const local = `$report${locals.length}`;
      locals.push(`(local ${local} (ref null $hd.variant))`);
      const payload = this.unboxValue(
        `(struct.get $hd.variant $hd.variant-payload (local.get ${local}))`,
        parts.ok,
      );
      const ok =
        parts.ok === "void" ? "(i32.const 0)" : this.emitEntryReport(payload, parts.ok, locals);
      return `(block (result i32) (local.set ${local} ${value}) (if (result i32) (i32.eqz (struct.get $hd.variant $hd.variant-tag (local.get ${local}))) (then ${ok}) (else (i32.const -1))))`;
    }
    const data = this.dataByName.get(readonlyType(type));
    if (!data) return `(block (result i32) (drop ${value}) (i32.const 0))`;
    return `(struct.get $d${data.index} $d${data.index}f0 ${value})`;
  }

  protected unboxValue(payload: string, type: ValueType): string {
    if (cellInner(type) !== undefined) return `(ref.cast (ref null $hd.cell) ${payload})`;
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.unboxValue(payload, mutable);
    if (isGenericValueType(type) || type === "void") return payload;
    const scalar = unboxScalar(payload, type);
    if (scalar) return scalar;
    if (type === "string") return `(ref.cast (ref null $hd.string) ${payload})`;
    if (type.startsWith("trait:") && !type.endsWith("?"))
      return `(ref.cast (ref null $trait${this.traitsByName.get(traitTypeBase(type))?.index}) ${payload})`;
    if (type.startsWith("provider-row:")) return `(ref.cast (ref null $hd.providers) ${payload})`;
    if (contextKeys(type))
      return `(ref.cast (ref null $context${this.contextNames.get(type)}) ${payload})`;
    if (tupleParts(type) !== undefined) return `(ref.cast (ref null $hd.list) ${payload})`;
    if (storedSuspensionParts(type)) return `(ref.cast (ref null $hd.suspension) ${payload})`;
    const suspension = suspensionParts(type);
    if (suspension) return `(ref.cast (ref null $s${suspension.functionIndex}) ${payload})`;
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension)
      return `(ref.cast (ref null ${traitSuspensionName(traitSuspension.traitIndex, traitSuspension.methodIndex)}) ${payload})`;
    if (isErasedVariant(type)) return `(ref.cast (ref null $hd.variant) ${payload})`;
    const callable = functionParts(type);
    if (callable)
      return `(ref.cast (ref null $closure${this.functionSignatures.get(type)}) ${payload})`;
    const data = this.dataByName.get(type);
    const nominalData = nominalGenericParts(type);
    if (nominalData?.name === "List" && nominalData.arguments.length === 1)
      return `(ref.cast (ref null $hd.vector) ${payload})`;
    if (nominalData?.name === CURSOR_TYPE && nominalData.arguments.length === 1)
      return `(ref.cast (ref null $hd.iterator) ${payload})`;
    if (nominalData?.name === "Map" && nominalData.arguments.length === 2)
      return `(ref.cast (ref null $hd.map) ${payload})`;
    if (nominalData && this.dataByName.has(nominalData.name))
      return `(ref.cast (ref null $d${this.dataByName.get(nominalData.name)!.index}) ${payload})`;
    if (nominalData && this.enumByName.has(nominalData.name))
      return `(ref.cast (ref null $e${this.enumByName.get(nominalData.name)!.index}) ${payload})`;
    if (data) return `(ref.cast (ref null $d${data.index}) ${payload})`;
    const enumType = this.enumByName.get(type);
    if (enumType) return `(ref.cast (ref null $e${enumType.index}) ${payload})`;
    throw new Error(`cannot unbox '${type}'`);
  }

  defaultValue(type: ValueType): string {
    if (cellInner(type) !== undefined) return "(ref.null $hd.cell)";
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.defaultValue(mutable);
    if (isGenericValueType(type)) return `(ref.null any)`;
    const numeric = numericType(type);
    if (numeric) return `(${numeric.wasm}.const 0)`;
    if (type === "bool" || type === "char") return `(i32.const 0)`;
    if (type === "string") return `(ref.null $hd.string)`;
    if (type.startsWith("trait:") && !type.endsWith("?"))
      return `(ref.null $trait${this.traitsByName.get(traitTypeBase(type))?.index})`;
    if (type.startsWith("provider-row:")) return `(ref.null $hd.providers)`;
    if (type.startsWith("provider:")) return `(ref.null extern)`;
    if (contextKeys(type)) return `(ref.null $context${this.contextNames.get(type)})`;
    if (tupleParts(type) !== undefined) return `(ref.null $hd.list)`;
    if (storedSuspensionParts(type)) return `(ref.null $hd.suspension)`;
    const suspension = suspensionParts(type);
    if (suspension) return `(ref.null $s${suspension.functionIndex})`;
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension)
      return `(ref.null ${traitSuspensionName(traitSuspension.traitIndex, traitSuspension.methodIndex)})`;
    if (isErasedVariant(type)) return `(ref.null $hd.variant)`;
    const callable = functionParts(type);
    if (callable) return `(ref.null $closure${this.functionSignatures.get(type)})`;
    const data = this.dataByName.get(type);
    const nominalData = nominalGenericParts(type);
    if (nominalData?.name === CURSOR_TYPE && nominalData.arguments.length === 1)
      return `(ref.null $hd.iterator)`;
    if (nominalData?.name === "List" && nominalData.arguments.length === 1)
      return `(ref.null $hd.vector)`;
    if (nominalData?.name === "Map" && nominalData.arguments.length === 2)
      return `(ref.null $hd.map)`;
    if (nominalData && this.dataByName.has(nominalData.name))
      return `(ref.null $d${this.dataByName.get(nominalData.name)!.index})`;
    if (nominalData && this.enumByName.has(nominalData.name))
      return `(ref.null $e${this.enumByName.get(nominalData.name)!.index})`;
    if (data) return `(ref.null $d${data.index})`;
    const enumType = this.enumByName.get(type);
    if (enumType) return `(ref.null $e${enumType.index})`;
    throw new Error(`cannot produce a default for '${type}'`);
  }
}

/**
 * A closure's environment: its captures, then one dictionary for each
 * generic bound it shares with the enclosing function.
 */
type StringKernelFunction = "concat" | "equal" | "compare";

const stringKernelName = (name: StringKernelFunction): string => `__std_text_string_${name}`;

const STRING_KERNEL_NAMES: ReadonlySet<string> = new Set(
  (["concat", "equal", "compare"] as const).map(stringKernelName),
);

export function environmentType(
  closure: HirFunction,
  emitter: { watType(type: ValueType): string },
): string {
  const fields = [
    ...closure.captures.map(
      (capture) =>
        `      (field $env${closure.index}f${capture.fieldIndex} ${emitter.watType(capture.source.type)})`,
    ),
    ...closure.genericBounds.map(
      (bound, index) =>
        `      (field $env${closure.index}b${index} (ref null $trait${bound.traitIndex}))`,
    ),
  ];
  return `    (type $env${closure.index} (struct${fields.length ? "\n" + fields.join("\n") : ""}))`;
}

/**
 * A non-suspending closure loads the enclosing function's bound
 * dictionaries from its environment into its own `$bound` locals, so a
 * call through a bound in its body reads them as a function's do.
 */
export function closureBoundLoads(closure: HirFunction): {
  readonly locals: readonly string[];
  readonly loads: readonly string[];
} {
  if (!closure.closure) return { locals: [], loads: [] };
  const environment = `(ref.cast (ref $env${closure.index}) (local.get $env))`;
  return {
    locals: closure.genericBounds.map(
      (bound, index) => `  (local $bound${index} (ref null $trait${bound.traitIndex}))`,
    ),
    loads: closure.genericBounds.map(
      (_, index) =>
        `(local.set $bound${index} (struct.get $env${closure.index} $env${closure.index}b${index} ${environment}))`,
    ),
  };
}
