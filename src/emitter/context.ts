import type {
  HirData,
  HirEnum,
  HirFunction,
  HirStatement,
  HirTrait,
  HirTraitImplementation,
  ValueType,
} from "../hir.ts";
import {
  contextKeys,
  functionParts,
  isErasedVariant,
  mutableInner,
  nominalGenericParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import type { SuspensionPlan } from "./suspension.ts";
import { runtimePanicCode, type RuntimePanicName } from "../runtime-panic.ts";
import {
  containsGenericValueType,
  isGenericValueType,
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
  protected floatDisplay = false;
  protected stringTransforms = false;
  protected stringSplit = false;
  protected consoleOutput = false;
  protected currentRequirements: readonly string[] = [];
  protected readonly callableAdapters = new Map<string, CallableAdapter>();
  protected readonly providerKeys = new Map<string, number>();

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
  ) {
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

  watType(type: ValueType): string {
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.watType(mutable);
    if (isGenericValueType(type)) return "anyref";
    if (type === "i32" || type === "bool" || type === "char") return "i32";
    if (type === "f64") return "f64";
    if (type === "string") return "(ref null $hd.bytes)";
    if (type.startsWith("provider-row:")) return "(ref null $hd.providers)";
    if (type.startsWith("provider:")) return "externref";
    if (type.startsWith("trait:"))
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
    if (nominalData?.name === "Iterator" && nominalData.arguments.length === 1)
      return `(ref null $hd.iterator)`;
    if (nominalData?.name === "list" && nominalData.arguments.length === 1)
      return `(ref null $hd.vector)`;
    if (nominalData?.name === "map" && nominalData.arguments.length === 2)
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

  get requiresFloatDisplay(): boolean {
    return this.floatDisplay;
  }

  get requiresStringTransforms(): boolean {
    return this.stringTransforms;
  }

  get requiresStringSplit(): boolean {
    return this.stringSplit;
  }

  protected emitRuntimePanic(name: RuntimePanicName): string {
    return `(call $hd.panic (i32.const ${runtimePanicCode(name)})) unreachable`;
  }

  get requiresConsoleOutput(): boolean {
    return this.consoleOutput;
  }

  get adapters(): readonly CallableAdapter[] {
    return [...this.callableAdapters.values()];
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

  protected hostSafe(type: ValueType): boolean {
    return (
      type === "i32" || type === "bool" || type === "char" || type === "f64" || type === "void"
    );
  }

  protected boxWatValue(value: string, type: ValueType): string {
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.boxWatValue(value, mutable);
    if (isGenericValueType(type)) return value;
    if (type === "i32" || type === "bool" || type === "char")
      return `(struct.new $hd.box-i32 ${value})`;
    if (type === "f64") return `(struct.new $hd.box-f64 ${value})`;
    if (type === "void") return `(ref.null any)`;
    return value;
  }

  protected unboxValue(payload: string, type: ValueType): string {
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.unboxValue(payload, mutable);
    if (isGenericValueType(type)) return payload;
    if (type === "i32" || type === "bool" || type === "char")
      return `(struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) ${payload}))`;
    if (type === "f64")
      return `(struct.get $hd.box-f64 $hd.box-f64-value (ref.cast (ref $hd.box-f64) ${payload}))`;
    if (type === "string") return `(ref.cast (ref null $hd.bytes) ${payload})`;
    if (type.startsWith("trait:"))
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
    if (nominalData?.name === "list" && nominalData.arguments.length === 1)
      return `(ref.cast (ref null $hd.vector) ${payload})`;
    if (nominalData?.name === "Iterator" && nominalData.arguments.length === 1)
      return `(ref.cast (ref null $hd.iterator) ${payload})`;
    if (nominalData?.name === "map" && nominalData.arguments.length === 2)
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
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.defaultValue(mutable);
    if (isGenericValueType(type)) return `(ref.null any)`;
    if (type === "i32" || type === "bool" || type === "char") return `(i32.const 0)`;
    if (type === "f64") return `(f64.const 0)`;
    if (type === "string") return `(ref.null $hd.bytes)`;
    if (type.startsWith("trait:"))
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
    if (nominalData?.name === "Iterator" && nominalData.arguments.length === 1)
      return `(ref.null $hd.iterator)`;
    if (nominalData?.name === "list" && nominalData.arguments.length === 1)
      return `(ref.null $hd.vector)`;
    if (nominalData?.name === "map" && nominalData.arguments.length === 2)
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
