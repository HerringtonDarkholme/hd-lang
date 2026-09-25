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
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import type { SuspensionPlan } from "./suspension.ts";
import {
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
  protected loopCounter = 0;
  protected readonly loops: LoopContext[] = [];
  protected readonly cleanupFrames: CleanupFrame[] = [];
  protected readonly temporaryTypes: ValueType[] = [];
  protected floatPower = false;
  protected floatDisplay = false;
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

  protected hostSafe(type: ValueType): boolean {
    return (
      type === "i32" || type === "bool" || type === "char" || type === "f64" || type === "void"
    );
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
