import type { HirData, HirEnum, HirExpression, HirFunction, HirLocal, HirMatchArm, HirProgram, HirProviderContextEntry, HirStatement, HirTrait, HirTraitImplementation, ValueType } from "./hir.ts";
import { contextKeys, functionParts, functionType, isErasedVariant, mutableInner, nominalGenericParts, optionalInner, readonlyType, resultParts, suspensionParts, traitSuspensionParts, tupleParts } from "./types.ts";
import { buildSuspensionPlan, needsSuspensionCfg, type SuspensionOperation, type SuspensionPlan, type SuspensionTerminator } from "./suspension.ts";

const indent = (text: string, spaces = 2): string => {
  const prefix = " ".repeat(spaces);
  return text.split("\n").map((line) => prefix + line).join("\n");
};

const exportName = (name: string): string => JSON.stringify(name);
const functionName = (index: number): string => `$f${index}`;
const localName = (index: number): string => `$l${index}`;
const testExportName = (name: string): string | undefined => {
  const match = /^\$test\.(\d+)$/.exec(name);
  return match ? `__hd_test_${match[1]}` : undefined;
};
const isGenericValueType = (type: ValueType): boolean => /^generic:([^?\[\](),]+)$/.test(type);
const isRowRequirement = (requirement: string): boolean => requirement.startsWith("row:");

type HirSuspendDrive = Extract<HirExpression, { kind: "suspend-drive" | "trait-suspend-drive" }>;

interface LinearSuspensionSite {
  readonly index: number;
  readonly statementIndex: number;
  readonly statement: HirStatement;
  readonly drive: HirSuspendDrive;
  readonly cleanups: readonly (readonly HirStatement[])[];
}

function exactStatementDrive(statement: HirStatement): HirSuspendDrive | undefined {
  const expression = statement.kind === "binding" || statement.kind === "assignment" || statement.kind === "discard"
    ? statement.value
    : statement.kind === "return" || statement.kind === "break"
      ? statement.value
      : statement.kind === "expression"
        ? statement.expression
        : undefined;
  return expression?.kind === "suspend-drive" || expression?.kind === "trait-suspend-drive" ? expression : undefined;
}

const traitSuspensionName = (traitIndex: number, methodIndex: number): string => `$ts${traitIndex}_${methodIndex}`;
const traitSuspensionPollName = (traitIndex: number, methodIndex: number): string => `$tspoll${traitIndex}_${methodIndex}`;
const traitSuspensionCancelName = (traitIndex: number, methodIndex: number): string => `$tscancel${traitIndex}_${methodIndex}`;
const traitSuspensionResultName = (traitIndex: number, methodIndex: number): string => `$tsresult${traitIndex}_${methodIndex}`;
const traitSuspensionDriveName = (traitIndex: number, methodIndex: number): string => `$tsdrive${traitIndex}_${methodIndex}`;

function suspensionFrameTypeName(drive: HirSuspendDrive): string {
  return drive.kind === "suspend-drive"
    ? `$s${drive.functionIndex}`
    : traitSuspensionName(drive.traitIndex, drive.methodIndex);
}

function suspensionPoll(drive: HirSuspendDrive, frame: string): string {
  return drive.kind === "suspend-drive"
    ? `(call $poll${drive.functionIndex} ${frame})`
    : `(call ${traitSuspensionPollName(drive.traitIndex, drive.methodIndex)} ${frame})`;
}

function suspensionCancel(drive: HirSuspendDrive, frame: string): string {
  return drive.kind === "suspend-drive"
    ? `(call $cancel${drive.functionIndex} ${frame})`
    : `(call ${traitSuspensionCancelName(drive.traitIndex, drive.methodIndex)} ${frame})`;
}

function linearSuspensionSites(declaration: HirFunction): readonly LinearSuspensionSite[] {
  const sites: LinearSuspensionSite[] = [];
  const cleanups: Array<readonly HirStatement[]> = [];
  declaration.body.forEach((statement, statementIndex) => {
    if (statement.kind === "defer") {
      cleanups.push(statement.body);
      return;
    }
    const drive = exactStatementDrive(statement);
    if (drive) sites.push({ index: sites.length, statementIndex, statement, drive, cleanups: [...cleanups] });
  });
  return sites;
}
const providerWatType = (requirement: string, traits: ReadonlyMap<string, HirTrait>): string => {
  if (isRowRequirement(requirement)) return "(ref null $hd.providers)";
  const trait = traits.get(nominalGenericParts(requirement)?.name ?? requirement);
  return trait ? `(ref null $trait${trait.index})` : "externref";
};

const traitTypeBase = (type: ValueType): string => {
  const key = type.slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
};

class FunctionEmitter {
  private readonly dataByName: ReadonlyMap<string, HirData>;
  private readonly dataByIndex: ReadonlyMap<number, HirData>;
  private readonly enumByName: ReadonlyMap<string, HirEnum>;
  private readonly functionSignatures: ReadonlyMap<ValueType, number>;
  private readonly contextNames: ReadonlyMap<ValueType, number>;
  private readonly closuresByIndex: ReadonlyMap<number, HirFunction>;
  private readonly traitsByIndex: ReadonlyMap<number, HirTrait>;
  private readonly traitsByName: ReadonlyMap<string, HirTrait>;
  private readonly implementationsByIndex: ReadonlyMap<number, HirTraitImplementation>;
  private readonly suspensionPlans: ReadonlyMap<number, SuspensionPlan>;
  private loopCounter = 0;
  private readonly loops: Array<{ breakLabel: string; continueLabel: string; result?: ValueType }> = [];
  private readonly cleanupFrames: Array<{ cleanups: Array<readonly HirStatement[]>; loopBoundary: boolean }> = [];
  private readonly temporaryTypes: ValueType[] = [];
  private floatPower = false;
  private consoleOutput = false;
  private currentRequirements: readonly string[] = [];
  private readonly callableAdapters = new Map<string, { readonly index: number; readonly formalType: ValueType; readonly actualType: ValueType }>();
  private readonly providerKeys = new Map<string, number>();

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
    this.implementationsByIndex = new Map(implementations.map((implementation) => [implementation.index, implementation]));
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
    if (type.startsWith("trait:")) return `(ref null $trait${this.traitsByName.get(traitTypeBase(type))?.index})`;
    if (contextKeys(type)) return `(ref null $context${this.contextNames.get(type)})`;
    if (tupleParts(type) !== undefined) return `(ref null $hd.list)`;
    const suspension = suspensionParts(type);
    if (suspension) return `(ref null $s${suspension.functionIndex})`;
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension) return `(ref null ${traitSuspensionName(traitSuspension.traitIndex, traitSuspension.methodIndex)})`;
    if (isErasedVariant(type)) return "(ref null $hd.variant)";
    const callable = functionParts(type);
    if (callable) return `(ref null $closure${this.functionSignatures.get(type)})`;
    if (type === "void") return "";
    const data = this.dataByName.get(type);
    const nominalData = nominalGenericParts(type);
    if (nominalData?.name === "list" && nominalData.arguments.length === 1) return `(ref null $hd.vector)`;
    if (nominalData?.name === "map" && nominalData.arguments.length === 2) return `(ref null $hd.map)`;
    if (nominalData && this.dataByName.has(nominalData.name)) return `(ref null $d${this.dataByName.get(nominalData.name)!.index})`;
    if (nominalData && this.enumByName.has(nominalData.name)) return `(ref null $e${this.enumByName.get(nominalData.name)!.index})`;
    if (data) return `(ref null $d${data.index})`;
    const enumType = this.enumByName.get(type);
    if (enumType) return `(ref null $e${enumType.index})`;
    throw new Error(`cannot emit unknown type '${type}'`);
  }

  get requiresFloatPower(): boolean {
    return this.floatPower;
  }

  get requiresConsoleOutput(): boolean {
    return this.consoleOutput;
  }

  get adapters(): readonly { readonly index: number; readonly formalType: ValueType; readonly actualType: ValueType }[] {
    return [...this.callableAdapters.values()];
  }

  private providerKey(key: string): number {
    let index = this.providerKeys.get(key);
    if (index === undefined) {
      index = this.providerKeys.size;
      this.providerKeys.set(key, index);
    }
    return index;
  }

  private providerType(requirement: string): string {
    return providerWatType(requirement, this.traitsByName);
  }

  private hostSafe(type: ValueType): boolean {
    return type === "i32" || type === "bool" || type === "char" || type === "f64" || type === "void";
  }

  emit(declaration: HirFunction): string {
    this.currentRequirements = declaration.requirements;
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map((bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`);
    const allParameters = declaration.closure
      ? [`(param $env anyref)`, parameters, ...declaration.requirements.map((requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`)].filter(Boolean).join(" ")
      : [parameters, ...boundParameters, ...declaration.requirements.map((requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`)].filter(Boolean).join(" ");
    const result = declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const locals = declaration.locals
      .filter((local) => !local.parameter)
      .map((local) => `  (local ${localName(local.index)} ${this.watType(local.type)})`);
    this.temporaryTypes.length = 0;
    this.cleanupFrames.length = 0;
    const body = this.emitBlock(declaration.body, declaration.result);
    const temporaries = this.temporaryTypes.map((type, index) => `  (local $tmp${index} ${this.watType(type)})`);
    const exported = !declaration.name.startsWith("$") && !declaration.closure && !declaration.suspending && declaration.parameters.every((parameter) => this.hostSafe(parameter.type)) && this.hostSafe(declaration.result);
    const exportClause = exported ? ` (export ${exportName(declaration.name)})` : "";
    const internalName = declaration.closure ? `$c${declaration.index}` : declaration.suspending ? `$body${declaration.index}` : functionName(declaration.index);
    const signature = declaration.closure
      ? ` (type $sig${this.functionSignatures.get(functionType(declaration.parameters.map((parameter) => parameter.type), declaration.result, declaration.requirements, declaration.variadic))})`
      : "";
    return [
      `(func ${internalName}${signature}${exportClause}${allParameters ? " " + allParameters : ""}${result}`,
      ...locals,
      ...temporaries,
      indent(body),
      `)`,
    ].join("\n");
  }

  emitSuspensionSupport(declaration: HirFunction): string {
    const plan = this.suspensionPlans.get(declaration.index);
    if (plan) return this.emitCfgSuspensionSupport(declaration, plan);
    const resumableSites = linearSuspensionSites(declaration);
    if (resumableSites.length > 0) return this.emitLinearSuspensionSupport(declaration, resumableSites);
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map((bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`);
    const providerParameters = declaration.requirements.map((requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`);
    const allParameters = [parameters, ...boundParameters, ...providerParameters].filter(Boolean).join(" ");
    const constructor = [
      `(func ${functionName(declaration.index)}${allParameters ? " " + allParameters : ""} (result (ref null $s${declaration.index}))`,
      `  (call $hd.trace (i32.const ${declaration.index}) (i32.const 0))`,
      `  (struct.new $s${declaration.index} (i32.const 0) (i32.const 0)${declaration.parameters.map((parameter) => ` (local.get ${localName(parameter.index)})`).join("")}${declaration.genericBounds.map((_, index) => ` (local.get $bound${index})`).join("")}${declaration.requirements.map((_, index) => ` (local.get $provider${index})`).join("")}${declaration.result === "void" ? "" : ` ${this.defaultValue(declaration.result)}`})`,
      `)`,
    ].join("\n");
    const result = declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const bodyCall = `(call $body${declaration.index}${declaration.parameters.length || declaration.genericBounds.length || declaration.requirements.length ? " " : ""}${[
      ...declaration.parameters.map((_, index) => `(struct.get $s${declaration.index} $s${declaration.index}a${index} (local.get $frame))`),
      ...declaration.genericBounds.map((_, index) => `(struct.get $s${declaration.index} $s${declaration.index}b${index} (local.get $frame))`),
      ...declaration.requirements.map((_, index) => `(struct.get $s${declaration.index} $s${declaration.index}p${index} (local.get $frame))`),
    ].join(" ")})`;
    const poll = [
      `(func $poll${declaration.index} (param $frame (ref null $s${declaration.index})) (result i32)`,
      `  (call $hd.trace (i32.const ${declaration.index}) (i32.const 1))`,
      `  (if (i32.eq (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)) (i32.const 1))`,
      `    (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 4)) unreachable))`,
      `  (if (i32.or`,
      `        (i32.eq (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)) (i32.const 2))`,
      `        (i32.eq (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)) (i32.const 3)))`,
      `    (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 5)) unreachable))`,
      `  (struct.set $s${declaration.index} $s${declaration.index}polls (local.get $frame)`,
      `    (i32.add (struct.get $s${declaration.index} $s${declaration.index}polls (local.get $frame)) (i32.const 1)))`,
      `  (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 1))`,
      `  (if (call $hd.pending`,
      `        (i32.const ${declaration.index})`,
      `        (struct.get $s${declaration.index} $s${declaration.index}polls (local.get $frame)))`,
      `    (then`,
      `      (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 4))`,
      `      (call $hd.trace (i32.const ${declaration.index}) (i32.const 6))`,
      `      (return (i32.const 0))))`,
      `  (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 1))`,
      declaration.result === "void"
        ? `  ${bodyCall}`
        : `  (struct.set $s${declaration.index} $s${declaration.index}result (local.get $frame) ${bodyCall})`,
      `  (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 2))`,
      `  (call $hd.trace (i32.const ${declaration.index}) (i32.const 2))`,
      `  (i32.const 1)`,
      `)`,
    ].filter(Boolean).join("\n");
    const drive = [
      `(func $drive${declaration.index} (param $frame (ref null $s${declaration.index}))${result}`,
      `  (if (global.get $hd.driver-active) (then unreachable))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (block $ready`,
      `    (loop $drive`,
      `      (br_if $ready (i32.eq (call $poll${declaration.index} (local.get $frame)) (i32.const 1)))`,
      `      (br $drive)))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      declaration.result === "void"
        ? ""
        : `  (struct.get $s${declaration.index} $s${declaration.index}result (local.get $frame))`,
      `)`,
    ].filter(Boolean).join("\n");
    const cancel = [
      `(func $cancel${declaration.index} (param $frame (ref null $s${declaration.index}))`,
      `  (call $hd.trace (i32.const ${declaration.index}) (i32.const 3))`,
      `  (if (i32.eq (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)) (i32.const 1))`,
      `    (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 4)) unreachable))`,
      `  (if (i32.or`,
      `        (i32.eq (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)) (i32.const 0))`,
      `        (i32.eq (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)) (i32.const 4)))`,
      `    (then (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 3))))`,
      `)`,
    ].join("\n");
    const entryExport = declaration.name === "main" ? "main" : testExportName(declaration.name);
    const entry = entryExport
      ? [
          `(func $entry${declaration.index} (export ${JSON.stringify(entryExport)})${providerParameters.length ? " " + providerParameters.join(" ") : ""}${result}`,
          `  (call $drive${declaration.index} (call ${functionName(declaration.index)}${declaration.requirements.length ? " " : ""}${declaration.requirements.map((_, index) => `(local.get $provider${index})`).join(" ")}))`,
          `)`,
        ].join("\n")
      : "";
    const developmentDriver = declaration.name === "main" ? this.emitSuspensionDevelopmentDriver(declaration, providerParameters) : "";
    return [constructor, poll, drive, cancel, entry, developmentDriver].filter(Boolean).join("\n\n");
  }

  private emitCfgSuspensionSupport(declaration: HirFunction, plan: SuspensionPlan): string {
    this.currentRequirements = declaration.requirements;
    this.temporaryTypes.length = 0;
    this.cleanupFrames.length = 0;
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map((bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`);
    const providerParameters = declaration.requirements.map((requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`);
    const allParameters = [parameters, ...boundParameters, ...providerParameters].filter(Boolean).join(" ");
    const storedLocals = [...declaration.locals.filter((local) => !local.parameter), ...plan.temporaries];
    const constructorValues = [
      `(i32.const 0)`,
      `(i32.const 0)`,
      ...declaration.parameters.map((parameter) => `(local.get ${localName(parameter.index)})`),
      ...declaration.genericBounds.map((_, index) => `(local.get $bound${index})`),
      ...declaration.requirements.map((_, index) => `(local.get $provider${index})`),
      ...storedLocals.map((local) => this.defaultValue(local.type)),
      ...plan.sites.map((site) => `(ref.null ${suspensionFrameTypeName(site.drive)})`),
      ...(declaration.result === "void" ? [] : [this.defaultValue(declaration.result)]),
    ];
    const constructor = [
      `(func ${functionName(declaration.index)}${allParameters ? " " + allParameters : ""} (result (ref null $s${declaration.index}))`,
      `  (call $hd.trace (i32.const ${declaration.index}) (i32.const 0))`,
      `  (struct.new $s${declaration.index} ${constructorValues.join(" ")})`,
      `)`,
    ].join("\n");

    const loadFrame = this.emitSuspensionFrameLoads(declaration, storedLocals);
    const storeFrame = this.emitSuspensionFrameStores(declaration, storedLocals);
    const resumeDispatch = plan.sites.reduceRight((otherwise, site) => {
      const child = `(struct.get $s${declaration.index} $s${declaration.index}child${site.siteIndex} (local.get $frame))`;
      const ready: string[] = [];
      if (site.resultLocal) ready.push(`(local.set ${localName(site.resultLocal.index)} ${this.emitSuspensionResult(site.drive, child)})`);
      ready.push(`(local.set $pc (i32.const ${site.next}))`);
      return [
        `(if (i32.eq (local.get $resume-state) (i32.const ${5 + site.siteIndex}))`,
        `  (then`,
        `    (if (i32.eqz ${suspensionPoll(site.drive, child)})`,
        `      (then`,
        ...storeFrame.map((line) => `        ${line}`),
        `        (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (local.get $resume-state))`,
        `        (call $hd.trace (i32.const ${declaration.index}) (i32.const 6))`,
        `        (return (i32.const 0))))`,
        ...ready.map((line) => `    ${line}`),
        `  )`,
        `  (else`,
        indent(otherwise, 4),
        `  ))`,
      ].join("\n");
    }, `(local.set $pc (i32.const ${plan.entry}))`);

    const blockBodies = plan.blocks.map((block) => [
      `(if (i32.eq (local.get $pc) (i32.const ${block.id}))`,
      `  (then`,
      ...block.operations.map((operation) => indent(this.emitCfgOperation(operation), 4)),
      indent(this.emitCfgTerminator(declaration, block.terminator, storeFrame), 4),
      `  ))`,
    ].join("\n"));
    const pollBody = [
      `(call $hd.trace (i32.const ${declaration.index}) (i32.const 1))`,
      `(local.set $resume-state (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 4)) unreachable))`,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 2)) (i32.eq (local.get $resume-state) (i32.const 3)))`,
      `  (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 5)) unreachable))`,
      `(struct.set $s${declaration.index} $s${declaration.index}polls (local.get $frame)`,
      `  (i32.add (struct.get $s${declaration.index} $s${declaration.index}polls (local.get $frame)) (i32.const 1)))`,
      `(struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 1))`,
      `(if (call $hd.pending (i32.const ${declaration.index}) (struct.get $s${declaration.index} $s${declaration.index}polls (local.get $frame)))`,
      `  (then`,
      `    (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame)`,
      `      (if (result i32) (i32.eq (local.get $resume-state) (i32.const 0))`,
      `        (then (i32.const 4))`,
      `        (else (local.get $resume-state))))`,
      `    (call $hd.trace (i32.const ${declaration.index}) (i32.const 6))`,
      `    (return (i32.const 0))))`,
      ...loadFrame,
      `(struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 1))`,
      `(if (i32.le_u (local.get $resume-state) (i32.const 4))`,
      `  (then (local.set $pc (i32.const ${plan.entry})))`,
      `  (else`,
      indent(resumeDispatch, 4),
      `  ))`,
      `(loop $cfg`,
      ...blockBodies.map((body) => indent(body, 2)),
      `  unreachable`,
      `)`,
    ];

    const cancellationBranches = plan.sites.map((site) => [
      `(if (i32.eq (local.get $resume-state) (i32.const ${5 + site.siteIndex}))`,
      `  (then`,
      `    ${suspensionCancel(site.drive, `(struct.get $s${declaration.index} $s${declaration.index}child${site.siteIndex} (local.get $frame))`)}`,
      ...[...site.cleanups].reverse().flatMap((cleanup) => [
        `    (call $hd.trace (i32.const ${declaration.index}) (i32.const 7))`,
        indent(this.emitBlock(cleanup, "void"), 4),
      ]),
      `    (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 3))`,
      `    (return)))`,
    ].join("\n"));
    const cancelBody = [
      `(call $hd.trace (i32.const ${declaration.index}) (i32.const 3))`,
      `(local.set $resume-state (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 4)) unreachable))`,
      ...loadFrame,
      ...cancellationBranches,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 0)) (i32.eq (local.get $resume-state) (i32.const 4)))`,
      `  (then (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 3))))`,
    ];

    const localDeclarations = [...declaration.locals, ...plan.temporaries].map((local) => `  (local ${localName(local.index)} ${this.watType(local.type)})`);
    const boundLocals = declaration.genericBounds.map((bound, index) => `  (local $bound${index} (ref null $trait${bound.traitIndex}))`);
    const providerLocals = declaration.requirements.map((requirement, index) => `  (local $provider${index} ${this.providerType(requirement)})`);
    const temporaries = this.temporaryTypes.map((type, index) => `  (local $tmp${index} ${this.watType(type)})`);
    const poll = [
      `(func $poll${declaration.index} (param $frame (ref null $s${declaration.index})) (result i32)`,
      `  (local $resume-state i32)`,
      `  (local $pc i32)`,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(pollBody.join("\n")),
      `)`,
    ].join("\n");
    const cancel = [
      `(func $cancel${declaration.index} (param $frame (ref null $s${declaration.index}))`,
      `  (local $resume-state i32)`,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(cancelBody.join("\n")),
      `)`,
    ].join("\n");
    const result = declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const drive = [
      `(func $drive${declaration.index} (param $frame (ref null $s${declaration.index}))${result}`,
      `  (if (global.get $hd.driver-active) (then unreachable))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (block $ready`,
      `    (loop $drive`,
      `      (br_if $ready (i32.eq (call $poll${declaration.index} (local.get $frame)) (i32.const 1)))`,
      `      (br $drive)))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      declaration.result === "void" ? "" : `  (struct.get $s${declaration.index} $s${declaration.index}result (local.get $frame))`,
      `)`,
    ].filter(Boolean).join("\n");
    const entryExport = declaration.name === "main" ? "main" : testExportName(declaration.name);
    const entry = entryExport
      ? [
          `(func $entry${declaration.index} (export ${JSON.stringify(entryExport)})${providerParameters.length ? " " + providerParameters.join(" ") : ""}${result}`,
          `  (call $drive${declaration.index} (call ${functionName(declaration.index)}${declaration.requirements.length ? " " : ""}${declaration.requirements.map((_, index) => `(local.get $provider${index})`).join(" ")}))`,
          `)`,
        ].join("\n")
      : "";
    const developmentDriver = declaration.name === "main" ? this.emitSuspensionDevelopmentDriver(declaration, providerParameters) : "";
    return [constructor, poll, drive, cancel, entry, developmentDriver].filter(Boolean).join("\n\n");
  }

  private emitCfgOperation(operation: SuspensionOperation): string {
    switch (operation.kind) {
      case "assign":
        return `(local.set ${localName(operation.local.index)} ${this.emitExpression(operation.value)})`;
      case "evaluate": {
        const value = this.emitExpression(operation.value);
        return operation.value.type === "void" || operation.value.type === "never" ? value : `(drop ${value})`;
      }
      case "cleanup":
        return this.emitBlock(operation.body, "void");
      case "provider-entry":
        return this.emitProviderEntries([operation.entry]).join("\n");
      case "context-create": {
        const contextIndex = this.contextNames.get(operation.local.type);
        const finalProviders = new Map<string, string>();
        for (const entry of operation.entries) {
          if (entry.kind === "binding") finalProviders.set(entry.key, localName(entry.local.index));
          else entry.providers.forEach((provider) => finalProviders.set(provider.key, localName(provider.local.index)));
        }
        return `(local.set ${localName(operation.local.index)} (struct.new $context${contextIndex} ${operation.keys.map((key) => `(local.get ${finalProviders.get(key)})`).join(" ")}))`;
      }
      case "match-bind":
        return this.emitCfgMatchBindings(operation.subject, operation.representation, operation.enumIndex, operation.bindings).join("\n");
    }
  }

  private emitCfgTerminator(declaration: HirFunction, terminator: SuspensionTerminator, storeFrame: readonly string[]): string {
    const jump = (target: number): string => `(local.set $pc (i32.const ${target}))\n(br $cfg)`;
    switch (terminator.kind) {
      case "jump":
        return jump(terminator.target);
      case "branch":
        return `(if ${this.emitExpression(terminator.condition)}\n  (then (local.set $pc (i32.const ${terminator.thenTarget})))\n  (else (local.set $pc (i32.const ${terminator.elseTarget}))))\n(br $cfg)`;
      case "match-test":
        return `(if ${this.emitCfgMatchCondition(terminator.subject, terminator.representation, terminator.enumIndex, terminator.arm)}\n  (then (local.set $pc (i32.const ${terminator.thenTarget})))\n  (else (local.set $pc (i32.const ${terminator.elseTarget}))))\n(br $cfg)`;
      case "suspend": {
        const child = `(struct.get $s${declaration.index} $s${declaration.index}child${terminator.siteIndex} (local.get $frame))`;
        return [
          `(struct.set $s${declaration.index} $s${declaration.index}child${terminator.siteIndex} (local.get $frame) ${this.emitExpression(terminator.drive.suspension)})`,
          `(if (i32.eqz ${suspensionPoll(terminator.drive, child)})`,
          `  (then`,
          ...storeFrame.map((line) => `    ${line}`),
          `    (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const ${5 + terminator.siteIndex}))`,
          `    (call $hd.trace (i32.const ${declaration.index}) (i32.const 6))`,
          `    (return (i32.const 0))))`,
          terminator.resultLocal ? `(local.set ${localName(terminator.resultLocal.index)} ${this.emitSuspensionResult(terminator.drive, child)})` : "",
          jump(terminator.next),
        ].filter(Boolean).join("\n");
      }
      case "propagate": {
        const operand = this.emitExpression(terminator.operand);
        const success: string[] = [];
        if (terminator.successLocal) {
          success.push(`(local.set ${localName(terminator.successLocal.index)} ${this.unboxValue(`(struct.get $hd.variant $hd.variant-payload ${operand})`, terminator.payloadType)})`);
        }
        success.push(`(local.set $pc (i32.const ${terminator.successTarget}))`, `(br $cfg)`);
        const failure = [
          ...(declaration.result === "void" ? [] : [`(struct.set $s${declaration.index} $s${declaration.index}result (local.get $frame) ${operand})`]),
          ...[...terminator.cleanups].reverse().map((cleanup) => this.emitBlock(cleanup, "void")),
          `(struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 2))`,
          `(call $hd.trace (i32.const ${declaration.index}) (i32.const 2))`,
          `(return (i32.const 1))`,
        ];
        return `(if (i32.eq (struct.get $hd.variant $hd.variant-tag ${operand}) (i32.const ${terminator.successTag}))\n  (then\n${indent(success.join("\n"), 4)}\n  )\n  (else\n${indent(failure.join("\n"), 4)}\n  ))`;
      }
      case "complete":
        return [
          declaration.result === "void" ? (terminator.value ? this.emitExpression(terminator.value) : "") : `(struct.set $s${declaration.index} $s${declaration.index}result (local.get $frame) ${this.emitExpression(terminator.value!)})`,
          `(struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 2))`,
          `(call $hd.trace (i32.const ${declaration.index}) (i32.const 2))`,
          `(return (i32.const 1))`,
        ].filter(Boolean).join("\n");
      case "unreachable":
        return `(unreachable)`;
    }
  }

  private emitSuspensionResult(drive: HirSuspendDrive, child: string): string {
    const raw = drive.kind === "suspend-drive"
      ? `(struct.get $s${drive.functionIndex} $s${drive.functionIndex}result ${child})`
      : `(call ${traitSuspensionResultName(drive.traitIndex, drive.methodIndex)} ${child})`;
    return drive.kind === "suspend-drive" && drive.erasedResultType && isGenericValueType(drive.erasedResultType)
      ? this.unboxValue(raw, drive.type)
      : raw;
  }

  private emitCfgMatchBindings(
    subject: HirLocal,
    representation: "enum" | "erased-variant" | "scalar" | "data",
    enumIndex: number | undefined,
    bindings: HirMatchArm["bindings"],
  ): string[] {
    const source = localName(subject.index);
    return bindings.map((binding) => {
      const value = binding.accessPath
        ? this.emitPatternAccess(source, binding.accessPath)
        : binding.enumFieldIndex !== undefined
        ? this.emitEnumPayloadAccess(source, enumIndex, binding.enumFieldIndex, binding.enumErasedFieldType, binding.enumFieldType!, binding.path ?? [])
        : binding.path
          ? this.emitDataPatternAccess(source, binding.path)
        : binding.fieldIndex === -1
          ? `(local.get ${source})`
          : representation === "enum"
            ? binding.erasedFieldType && isGenericValueType(binding.erasedFieldType)
              ? this.unboxValue(`(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${source}))`, binding.type)
              : `(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${source}))`
            : representation === "erased-variant"
              ? this.unboxValue(`(struct.get $hd.variant $hd.variant-payload (local.get ${source}))`, binding.type)
              : `(local.get ${source})`;
      return `(local.set ${localName(binding.local.index)} ${value})`;
    });
  }

  private emitCfgMatchCondition(
    subject: HirLocal,
    representation: "enum" | "erased-variant" | "scalar" | "data",
    enumIndex: number | undefined,
    arm: Extract<HirExpression, { kind: "match" }>["arms"][number],
  ): string {
    const source = localName(subject.index);
    let condition: string | undefined;
    if (arm.literal) {
      const value = this.emitExpression(arm.literal);
      condition = arm.literal.type === "string"
        ? `(i32.eq (call $hd.string_compare (local.get ${source}) ${value}) (i32.const 0))`
        : arm.literal.type === "f64"
          ? `(f64.eq (local.get ${source}) ${value})`
          : `(i32.eq (local.get ${source}) ${value})`;
    } else if (arm.tag !== undefined) {
      const actual = representation === "enum"
        ? `(struct.get $e${enumIndex} $e${enumIndex}tag (local.get ${source}))`
        : `(struct.get $hd.variant $hd.variant-tag (local.get ${source}))`;
      condition = `(i32.eq ${actual} (i32.const ${arm.tag}))`;
    }
    for (const test of arm.tests ?? []) {
      const actual = this.emitMatchTestAccess(source, enumIndex, test);
      const expected = test.tag !== undefined ? `(i32.const ${test.tag})` : this.emitExpression(test.literal!);
      const next = test.tag !== undefined
        ? `(i32.eq ${actual} ${expected})`
        : test.literal!.type === "string"
        ? `(i32.eq (call $hd.string_compare ${actual} ${expected}) (i32.const 0))`
        : test.literal!.type === "f64"
          ? `(f64.eq ${actual} ${expected})`
          : `(i32.eq ${actual} ${expected})`;
      condition = condition ? `(i32.and ${condition} ${next})` : next;
    }
    return condition ?? `(i32.const 1)`;
  }

  private emitLinearSuspensionSupport(declaration: HirFunction, sites: readonly LinearSuspensionSite[]): string {
    this.currentRequirements = declaration.requirements;
    this.temporaryTypes.length = 0;
    this.cleanupFrames.length = 0;
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map((bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`);
    const providerParameters = declaration.requirements.map((requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`);
    const allParameters = [parameters, ...boundParameters, ...providerParameters].filter(Boolean).join(" ");
    const storedLocals = declaration.locals.filter((local) => !local.parameter);
    const constructorValues = [
      `(i32.const 0)`,
      `(i32.const 0)`,
      ...declaration.parameters.map((parameter) => `(local.get ${localName(parameter.index)})`),
      ...declaration.genericBounds.map((_, index) => `(local.get $bound${index})`),
      ...declaration.requirements.map((_, index) => `(local.get $provider${index})`),
      ...storedLocals.map((local) => this.defaultValue(local.type)),
      ...sites.map((site) => `(ref.null ${suspensionFrameTypeName(site.drive)})`),
      ...(declaration.result === "void" ? [] : [this.defaultValue(declaration.result)]),
    ];
    const constructor = [
      `(func ${functionName(declaration.index)}${allParameters ? " " + allParameters : ""} (result (ref null $s${declaration.index}))`,
      `  (call $hd.trace (i32.const ${declaration.index}) (i32.const 0))`,
      `  (struct.new $s${declaration.index} ${constructorValues.join(" ")})`,
      `)`,
    ].join("\n");

    const cold = this.emitLinearContinuation(declaration, sites, 0, []);
    const resumed = sites.reduceRight(
      (otherwise, site) => [
        `(if (i32.eq (local.get $resume-state) (i32.const ${5 + site.index}))`,
        `  (then`,
        indent(this.emitLinearSite(declaration, sites, site, false, site.cleanups), 4),
        `  )`,
        `  (else`,
        indent(otherwise, 4),
        `  ))`,
      ].join("\n"),
      `(unreachable)`,
    );
    const loadFrame = this.emitSuspensionFrameLoads(declaration, storedLocals);
    const pollBody = [
      `(call $hd.trace (i32.const ${declaration.index}) (i32.const 1))`,
      `(local.set $resume-state (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 4)) unreachable))`,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 2)) (i32.eq (local.get $resume-state) (i32.const 3)))`,
      `  (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 5)) unreachable))`,
      `(struct.set $s${declaration.index} $s${declaration.index}polls (local.get $frame)`,
      `  (i32.add (struct.get $s${declaration.index} $s${declaration.index}polls (local.get $frame)) (i32.const 1)))`,
      `(struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 1))`,
      `(if (call $hd.pending (i32.const ${declaration.index}) (struct.get $s${declaration.index} $s${declaration.index}polls (local.get $frame)))`,
      `  (then`,
      `    (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame)`,
      `      (if (result i32) (i32.eq (local.get $resume-state) (i32.const 0))`,
      `        (then (i32.const 4))`,
      `        (else (local.get $resume-state))))`,
      `    (call $hd.trace (i32.const ${declaration.index}) (i32.const 6))`,
      `    (return (i32.const 0))))`,
      ...loadFrame,
      `(struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 1))`,
      `(if (i32.le_u (local.get $resume-state) (i32.const 4))`,
      `  (then`,
      indent(cold, 4),
      `  )`,
      `  (else`,
      indent(resumed, 4),
      `  ))`,
    ];

    const cancellationBranches = sites.map((site) => [
      `(if (i32.eq (local.get $resume-state) (i32.const ${5 + site.index}))`,
      `  (then`,
      `    ${suspensionCancel(site.drive, `(struct.get $s${declaration.index} $s${declaration.index}child${site.index} (local.get $frame))`)}`,
      ...[...site.cleanups].reverse().flatMap((cleanup) => [
        `    (call $hd.trace (i32.const ${declaration.index}) (i32.const 7))`,
        indent(this.emitBlock(cleanup, "void"), 4),
      ]),
      `    (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 3))`,
      `    (return)))`,
    ].join("\n"));
    const cancelBody = [
      `(call $hd.trace (i32.const ${declaration.index}) (i32.const 3))`,
      `(local.set $resume-state (struct.get $s${declaration.index} $s${declaration.index}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${declaration.index}) (i32.const 4)) unreachable))`,
      ...loadFrame,
      ...cancellationBranches,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 0)) (i32.eq (local.get $resume-state) (i32.const 4)))`,
      `  (then (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 3))))`,
    ];

    const localDeclarations = declaration.locals.map((local) => `  (local ${localName(local.index)} ${this.watType(local.type)})`);
    const boundLocals = declaration.genericBounds.map((bound, index) => `  (local $bound${index} (ref null $trait${bound.traitIndex}))`);
    const providerLocals = declaration.requirements.map((requirement, index) => `  (local $provider${index} ${this.providerType(requirement)})`);
    const temporaries = this.temporaryTypes.map((type, index) => `  (local $tmp${index} ${this.watType(type)})`);
    const poll = [
      `(func $poll${declaration.index} (param $frame (ref null $s${declaration.index})) (result i32)`,
      `  (local $resume-state i32)`,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(pollBody.join("\n")),
      `)`,
    ].join("\n");
    const cancel = [
      `(func $cancel${declaration.index} (param $frame (ref null $s${declaration.index}))`,
      `  (local $resume-state i32)`,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(cancelBody.join("\n")),
      `)`,
    ].join("\n");
    const result = declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const drive = [
      `(func $drive${declaration.index} (param $frame (ref null $s${declaration.index}))${result}`,
      `  (if (global.get $hd.driver-active) (then unreachable))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (block $ready`,
      `    (loop $drive`,
      `      (br_if $ready (i32.eq (call $poll${declaration.index} (local.get $frame)) (i32.const 1)))`,
      `      (br $drive)))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      declaration.result === "void" ? "" : `  (struct.get $s${declaration.index} $s${declaration.index}result (local.get $frame))`,
      `)`,
    ].filter(Boolean).join("\n");
    const entryExport = declaration.name === "main" ? "main" : testExportName(declaration.name);
    const entry = entryExport
      ? [
          `(func $entry${declaration.index} (export ${JSON.stringify(entryExport)})${providerParameters.length ? " " + providerParameters.join(" ") : ""}${result}`,
          `  (call $drive${declaration.index} (call ${functionName(declaration.index)}${declaration.requirements.length ? " " : ""}${declaration.requirements.map((_, index) => `(local.get $provider${index})`).join(" ")}))`,
          `)`,
        ].join("\n")
      : "";
    const developmentDriver = declaration.name === "main" ? this.emitSuspensionDevelopmentDriver(declaration, providerParameters) : "";
    return [constructor, poll, drive, cancel, entry, developmentDriver].filter(Boolean).join("\n\n");
  }

  private emitSuspensionDevelopmentDriver(declaration: HirFunction, providerParameters: readonly string[]): string {
    if (declaration.parameters.length > 0) return "";
    const frame = `$hd.dev-frame${declaration.index}`;
    const providerArguments = declaration.requirements.map((_, index) => `(local.get $provider${index})`);
    const start = [
      `(global ${frame} (mut (ref null $s${declaration.index})) (ref.null $s${declaration.index}))`,
      `(func (export "__hd_start")${providerParameters.length ? " " + providerParameters.join(" ") : ""}`,
      `  (if (global.get $hd.driver-active) (then unreachable))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (global.set ${frame} (call ${functionName(declaration.index)}${providerArguments.length ? " " : ""}${providerArguments.join(" ")}))`,
      `)`,
    ].join("\n");
    const poll = [
      `(func (export "__hd_poll") (result i32)`,
      `  (local $ready i32)`,
      `  (local.set $ready (call $poll${declaration.index} (global.get ${frame})))`,
      `  (if (local.get $ready) (then (global.set $hd.driver-active (i32.const 0))))`,
      `  (local.get $ready)`,
      `)`,
    ].join("\n");
    const cancel = [
      `(func (export "__hd_cancel")`,
      `  (call $cancel${declaration.index} (global.get ${frame}))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      `)`,
    ].join("\n");
    const result = declaration.result === "void"
      ? ""
      : `(func (export "__hd_result") (result ${this.watType(declaration.result)}) (struct.get $s${declaration.index} $s${declaration.index}result (global.get ${frame})))`;
    return [start, poll, cancel, result].filter(Boolean).join("\n");
  }

  private emitSuspensionFrameLoads(declaration: HirFunction, storedLocals: readonly HirLocal[]): string[] {
    return [
      ...declaration.parameters.map((parameter, index) => `(local.set ${localName(parameter.index)} (struct.get $s${declaration.index} $s${declaration.index}a${index} (local.get $frame)))`),
      ...declaration.genericBounds.map((_, index) => `(local.set $bound${index} (struct.get $s${declaration.index} $s${declaration.index}b${index} (local.get $frame)))`),
      ...declaration.requirements.map((_, index) => `(local.set $provider${index} (struct.get $s${declaration.index} $s${declaration.index}p${index} (local.get $frame)))`),
      ...storedLocals.map((local) => `(local.set ${localName(local.index)} (struct.get $s${declaration.index} $s${declaration.index}l${local.index} (local.get $frame)))`),
    ];
  }

  private emitLinearContinuation(
    declaration: HirFunction,
    sites: readonly LinearSuspensionSite[],
    start: number,
    inheritedCleanups: readonly (readonly HirStatement[])[],
  ): string {
    const lines: string[] = [];
    const cleanups = [...inheritedCleanups];
    for (let index = start; index < declaration.body.length; index += 1) {
      const statement = declaration.body[index]!;
      if (statement.kind === "defer") {
        cleanups.push(statement.body);
        continue;
      }
      const site = sites.find((candidate) => candidate.statementIndex === index);
      if (site) {
        lines.push(this.emitLinearSite(declaration, sites, site, true, cleanups));
        return lines.join("\n");
      }
      if (statement.kind === "return") {
        lines.push(this.emitSuspensionCompletion(declaration, statement.value, cleanups));
        return lines.join("\n");
      }
      const final = index === declaration.body.length - 1;
      if (final && statement.kind === "expression") {
        lines.push(this.emitSuspensionCompletion(declaration, statement.expression, cleanups));
        return lines.join("\n");
      }
      lines.push(this.emitStatement(statement, "void"));
    }
    lines.push(this.emitSuspensionCompletion(declaration, undefined, cleanups));
    return lines.join("\n");
  }

  private emitLinearSite(
    declaration: HirFunction,
    sites: readonly LinearSuspensionSite[],
    site: LinearSuspensionSite,
    start: boolean,
    cleanups: readonly (readonly HirStatement[])[],
  ): string {
    const child = `(struct.get $s${declaration.index} $s${declaration.index}child${site.index} (local.get $frame))`;
    const lines: string[] = [];
    if (start) {
      lines.push(`(struct.set $s${declaration.index} $s${declaration.index}child${site.index} (local.get $frame) ${this.emitExpression(site.drive.suspension)})`);
    }
    lines.push(
      `(if (i32.eqz ${suspensionPoll(site.drive, child)})`,
      `  (then`,
      ...this.emitSuspensionFrameStores(declaration).map((line) => `    ${line}`),
      `    (struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const ${5 + site.index}))`,
      `    (call $hd.trace (i32.const ${declaration.index}) (i32.const 6))`,
      `    (return (i32.const 0))))`,
    );
    const rawValue = site.drive.type === "void"
      ? undefined
      : site.drive.kind === "suspend-drive"
        ? `(struct.get $s${site.drive.functionIndex} $s${site.drive.functionIndex}result ${child})`
        : `(call ${traitSuspensionResultName(site.drive.traitIndex, site.drive.methodIndex)} ${child})`;
    const value = rawValue && site.drive.kind === "suspend-drive" && site.drive.erasedResultType && isGenericValueType(site.drive.erasedResultType)
      ? this.unboxValue(rawValue, site.drive.type)
      : rawValue;
    const final = site.statementIndex === declaration.body.length - 1;
    if (site.statement.kind === "binding" || site.statement.kind === "assignment") {
      lines.push(`(local.set ${localName(site.statement.local.index)} ${value})`);
    } else if (site.statement.kind === "discard" || (site.statement.kind === "expression" && !final)) {
      if (value) lines.push(`(drop ${value})`);
    } else if (site.statement.kind === "return" || (site.statement.kind === "expression" && final)) {
      lines.push(this.emitSuspensionCompletionWat(declaration, value, cleanups));
      return lines.join("\n");
    }
    lines.push(this.emitLinearContinuation(declaration, sites, site.statementIndex + 1, cleanups));
    return lines.join("\n");
  }

  private emitSuspensionFrameStores(
    declaration: HirFunction,
    storedLocals: readonly HirLocal[] = declaration.locals.filter((local) => !local.parameter),
  ): string[] {
    return storedLocals
      .map((local) => `(struct.set $s${declaration.index} $s${declaration.index}l${local.index} (local.get $frame) (local.get ${localName(local.index)}))`);
  }

  private emitSuspensionCompletion(
    declaration: HirFunction,
    value: HirExpression | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): string {
    return this.emitSuspensionCompletionWat(declaration, value ? this.emitExpression(value) : undefined, cleanups);
  }

  private emitSuspensionCompletionWat(
    declaration: HirFunction,
    value: string | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): string {
    return [
      declaration.result === "void" ? value ?? "" : `(struct.set $s${declaration.index} $s${declaration.index}result (local.get $frame) ${value})`,
      ...[...cleanups].reverse().map((cleanup) => this.emitBlock(cleanup, "void")),
      `(struct.set $s${declaration.index} $s${declaration.index}state (local.get $frame) (i32.const 2))`,
      `(call $hd.trace (i32.const ${declaration.index}) (i32.const 2))`,
      `(return (i32.const 1))`,
    ].filter(Boolean).join("\n");
  }

  emitFunctionValueWrapper(declaration: HirFunction): string {
    const type = functionType(declaration.parameters.map((parameter) => parameter.type), declaration.result, declaration.requirements, declaration.variadic);
    const signature = this.functionSignatures.get(type);
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`);
    const providers = declaration.requirements.map((requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`);
    const result = declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const arguments_ = [
      ...declaration.parameters.map((parameter) => `(local.get ${localName(parameter.index)})`),
      ...declaration.requirements.map((_, index) => `(local.get $provider${index})`),
    ];
    return [
      `(func $fv${declaration.index} (type $sig${signature}) (param $env anyref) ${[...parameters, ...providers].join(" ")}${result}`,
      `  (call ${functionName(declaration.index)}${arguments_.length ? " " : ""}${arguments_.join(" ")})`,
      `)`,
    ].join("\n");
  }

  private emitBlock(statements: readonly HirStatement[], result: ValueType, loopBoundary = false): string {
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
      if (final && result !== "void" && statement.kind === "expression" && frame.cleanups.length > 0) {
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

  private emitStatement(statement: HirStatement, expected: ValueType): string {
    switch (statement.kind) {
      case "defer":
        throw new Error("defer registration must be emitted by its containing block");
      case "binding":
        return `(local.set ${localName(statement.local.index)} ${this.emitExpression(statement.value)})`;
      case "assignment":
        return `(local.set ${localName(statement.local.index)} ${this.emitExpression(statement.value)})`;
      case "discard":
        return `(drop ${this.emitExpression(statement.value)})`;
      case "return":
        if (statement.value) {
          const temporary = this.allocateTemporary(statement.value.type);
          return [`(local.set ${temporary} ${this.emitExpression(statement.value)})`, ...this.emitExitCleanups(false), `(return (local.get ${temporary}))`].join("\n");
        }
        return [...this.emitExitCleanups(false), `(return)`].join("\n");
      case "break": {
        const loop = this.loops.at(-1)!;
        if (!statement.value) return [...this.emitExitCleanups(true), `(br ${loop.breakLabel})`].join("\n");
        const temporary = this.allocateTemporary(statement.value.type);
        return [
          `(local.set ${temporary} ${this.emitExpression(statement.value)})`,
          ...this.emitExitCleanups(true),
          `(br ${loop.breakLabel} (local.get ${temporary}))`,
        ].join("\n");
      }
      case "continue":
        return [...this.emitExitCleanups(true), `(br ${this.loops.at(-1)!.continueLabel})`].join("\n");
      case "pass":
        return `(nop)`;
      case "expression": {
        const expression = this.emitExpression(statement.expression);
        if (expected !== "void") return expression;
        return statement.expression.type === "void" ? expression : `(drop ${expression})`;
      }
    }
  }

  private emitExpression(expression: HirExpression): string {
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
        return expression.segments.slice(1).reduce(
          (left, segment) => `(call $hd.string_concat ${left} ${this.emitExpression(segment)})`,
          this.emitExpression(expression.segments[0]!),
        );
      }
      case "display": {
        const operand = this.emitExpression(expression.operand);
        if (expression.operand.type === "i32") return `(call $hd.i32_to_string ${operand})`;
        if (expression.operand.type === "char") return `(call $hd.char_to_string ${operand})`;
        if (expression.operand.type === "bool") {
          return `(if (result (ref null $hd.bytes)) ${operand} (then (array.new_fixed $hd.bytes 4 (i32.const 116) (i32.const 114) (i32.const 117) (i32.const 101))) (else (array.new_fixed $hd.bytes 5 (i32.const 102) (i32.const 97) (i32.const 108) (i32.const 115) (i32.const 101))))`;
        }
        throw new Error(`unsupported Display operand ${expression.operand.type}`);
      }
      case "console-print":
        this.consoleOutput = true;
        return `(call $hd.console_print ${this.emitExpression(expression.provider)} ${this.emitExpression(expression.value)})`;
      case "assert-equal": {
        const values = new Array<string>(3);
        const setup = expression.arguments.map((argument, argumentIndex) => {
          const parameterIndex = expression.argumentParameterIndices?.[argumentIndex] ?? argumentIndex;
          const temporary = this.allocateTemporary(parameterIndex === 2 ? "string" : expression.valueType);
          values[parameterIndex] = `(local.get ${temporary})`;
          return `(local.set ${temporary} ${this.emitExpression(argument)})`;
        });
        return [
          `(block`,
          ...setup.map((line) => `  ${line}`),
          `  (if (i32.eqz ${this.emitValueEquality(values[0]!, values[1]!, expression.valueType)})`,
          `    (then unreachable))`,
          `)`,
        ].join("\n");
      }
      case "permission-weaken":
        return this.emitExpression(expression.operand);
      case "character":
        return `(i32.const ${expression.value})`;
      case "boolean":
        return `(i32.const ${expression.value ? 1 : 0})`;
      case "list":
        return `(struct.new $hd.vector (i32.const ${expression.elements.length}) ${expression.elements.length === 0
          ? `(array.new_default $hd.list (i32.const 0))`
          : `(array.new_fixed $hd.list ${expression.elements.length} ${expression.elements.map((element) => this.boxValue(element, expression.elementType)).join(" ")})`})`;
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
          `      (array.new_default $hd.list (i32.const ${capacity}))))`,
          ...expression.entries.map((entry) => [
            `  (call $hd.map_insert`,
            `    (ref.as_non_null (local.get ${temporary}))`,
            `    ${this.boxValue(entry.key, expression.keyType)}`,
            `    ${this.boxValue(entry.value, expression.valueType)})`,
          ].join("\n")),
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
        const payload = expression.payload && expression.payloadType
          ? this.boxValue(expression.payload, expression.payloadType)
          : `(ref.null any)`;
        return `(struct.new $hd.variant (i32.const ${tags[expression.variant]}) ${payload})`;
      }
      case "propagate": {
        const temporary = this.allocateTemporary(expression.operand.type);
        const result = expression.payloadType === "void" ? "" : ` (result ${this.watType(expression.payloadType)})`;
        const success = expression.payloadType === "void"
          ? `(nop)`
          : this.unboxValue(`(struct.get $hd.variant $hd.variant-payload (local.get ${temporary}))`, expression.payloadType);
        const failure = [...this.emitExitCleanups(false), `(return (local.get ${temporary}))`].join("\n");
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
      case "capture": {
        const closure = this.closuresByIndex.get(expression.closureIndex)!;
        const type = closure.captures[expression.fieldIndex]!.source.type;
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
        if (expression.left.type === "string") {
          if (expression.operator === "+") return `(call $hd.string_concat ${left} ${right})`;
          const comparison = `(call $hd.string_compare ${left} ${right})`;
          const operators: Readonly<Record<string, string>> = {
            "==": `(i32.eqz ${comparison})`,
            "!=": `(i32.ne ${comparison} (i32.const 0))`,
            "<": `(i32.lt_s ${comparison} (i32.const 0))`,
            "<=": `(i32.le_s ${comparison} (i32.const 0))`,
            ">": `(i32.gt_s ${comparison} (i32.const 0))`,
            ">=": `(i32.ge_s ${comparison} (i32.const 0))`,
          };
          return operators[expression.operator]!;
        }
        if (expression.operator === "and") return `(if (result i32) ${left} (then ${right}) (else (i32.const 0)))`;
        if (expression.operator === "or") return `(if (result i32) ${left} (then (i32.const 1)) (else ${right}))`;
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
        const prefix = expression.left.type === "f64" ? "f64" : "i32";
        const suffixes: Readonly<Record<string, string>> = {
          "+": "add", "-": "sub", "*": "mul", "/": prefix === "f64" ? "div" : "div_s",
          "%": "rem_s", "&": "and", "|": "or", "^": "xor",
          "==": "eq", "!=": "ne", "<": prefix === "f64" ? "lt" : "lt_s",
          "<=": prefix === "f64" ? "le" : "le_s", ">": prefix === "f64" ? "gt" : "gt_s",
          ">=": prefix === "f64" ? "ge" : "ge_s",
        };
        return `(${prefix}.${suffixes[expression.operator]} ${left} ${right})`;
      }
      case "call":
        {
          const ordered = this.emitOrderedArguments(expression.arguments, expression.argumentParameterIndices, expression.erasedParameterTypes, expression.defaultArguments, expression.parameterTypes, expression.bounds);
          const invocation = `(call ${functionName(expression.functionIndex)}${expression.arguments.length || expression.bounds?.length || expression.providers.length ? " " : ""}${[
            ...ordered.values,
            ...(expression.bounds ?? []).map((bound) => this.emitExpression(bound)),
            ...expression.providers.map((provider) => this.emitExpression(provider)),
          ].join(" ")})`;
          const rawResultType = expression.erasedResultType && isGenericValueType(expression.erasedResultType)
            ? expression.erasedResultType
            : expression.type;
          const call = ordered.setup.length === 0
            ? invocation
            : [`(block${rawResultType === "void" ? "" : ` (result ${this.watType(rawResultType)})`}`, ...ordered.setup.map((line) => `  ${line}`), `  ${invocation}`, `)`].join("\n");
          return expression.erasedResultType && isGenericValueType(expression.erasedResultType)
            ? this.unboxValue(call, expression.type)
            : call;
        }
      case "suspend-construct": {
        const ordered = this.emitOrderedArguments(expression.arguments, expression.argumentParameterIndices, expression.erasedParameterTypes, expression.defaultArguments, expression.parameterTypes, expression.bounds);
        const invocation = `(call ${functionName(expression.functionIndex)}${expression.arguments.length || expression.bounds?.length || expression.providers.length ? " " : ""}${[
          ...ordered.values,
          ...(expression.bounds ?? []).map((bound) => this.emitExpression(bound)),
          ...expression.providers.map((provider) => this.emitExpression(provider)),
        ].join(" ")})`;
        return ordered.setup.length === 0
          ? invocation
          : [`(block (result ${this.watType(expression.type)})`, ...ordered.setup.map((line) => `  ${line}`), `  ${invocation}`, `)`].join("\n");
      }
      case "suspend-drive": {
        const call = `(call $drive${expression.functionIndex} ${this.emitExpression(expression.suspension)})`;
        return expression.erasedResultType && isGenericValueType(expression.erasedResultType)
          ? this.unboxValue(call, expression.type)
          : call;
      }
      case "suspend-cancel":
        return `(call $cancel${expression.functionIndex} ${this.emitExpression(expression.suspension)})`;
      case "trait-suspend-drive":
        return `(call ${traitSuspensionDriveName(expression.traitIndex, expression.methodIndex)} ${this.emitExpression(expression.suspension)})`;
      case "trait-suspend-cancel":
        return `(call ${traitSuspensionCancelName(expression.traitIndex, expression.methodIndex)} ${this.emitExpression(expression.suspension)})`;
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
        const trait = this.traitsByIndex.get(expression.traitIndex)!;
        const implementation = this.implementationsByIndex.get(expression.implementationIndex)!;
        const adapters = trait.methods.map((method) => `(ref.func $tadapt${implementation.index}_${method.index})`);
        return `(struct.new $trait${trait.index} ${this.boxWatValue(this.emitExpression(expression.value), expression.value.type)}${adapters.length ? " " : ""}${adapters.join(" ")})`;
      }
      case "trait-dictionary": {
        const trait = this.traitsByIndex.get(expression.traitIndex)!;
        const implementation = this.implementationsByIndex.get(expression.implementationIndex)!;
        const adapters = trait.methods.map((method) => `(ref.func $tadapt${implementation.index}_${method.index})`);
        return `(struct.new $trait${trait.index} (ref.null any)${adapters.length ? " " : ""}${adapters.join(" ")})`;
      }
      case "trait-bound-dictionary":
        return `(local.get $bound${expression.boundIndex})`;
      case "trait-bound": {
        const trait = this.traitsByIndex.get(expression.traitIndex)!;
        const dictionary = `(local.get $bound${expression.boundIndex})`;
        const methods = trait.methods.map((method) => `(struct.get $trait${trait.index} $trait${trait.index}m${method.index} ${dictionary})`);
        return `(struct.new $trait${trait.index} ${this.boxValue(expression.value, expression.value.type)}${methods.length ? " " : ""}${methods.join(" ")})`;
      }
      case "trait-call": {
        const trait = this.traitsByIndex.get(expression.traitIndex)!;
        const method = trait.methods[expression.methodIndex]!;
        const temporary = this.allocateTemporary(expression.receiver.type);
        const ordered = this.emitOrderedArguments(expression.arguments, expression.argumentParameterIndices);
        return [
          `(block${expression.type === "void" ? "" : ` (result ${this.watType(expression.type)})`}`,
          `  (local.set ${temporary} ${this.emitExpression(expression.receiver)})`,
          ...ordered.setup.map((line) => `  ${line}`),
          `  (call_ref $tsig${trait.index}_${method.index}`,
          `    (struct.get $trait${trait.index} $trait${trait.index}value (local.get ${temporary}))`,
          ...ordered.values.map((argument) => `    ${argument}`),
          ...expression.providers.map((provider) => `    ${this.emitExpression(provider)}`),
          `    (struct.get $trait${trait.index} $trait${trait.index}m${method.index} (local.get ${temporary})))`,
          `)`,
        ].join("\n");
      }
      case "trait-suspend-construct": {
        const trait = this.traitsByIndex.get(expression.traitIndex)!;
        const method = trait.methods[expression.methodIndex]!;
        const temporary = this.allocateTemporary(expression.receiver.type);
        const ordered = this.emitOrderedArguments(expression.arguments, expression.argumentParameterIndices);
        return [
          `(block (result (ref null ${traitSuspensionName(trait.index, method.index)}))`,
          `  (local.set ${temporary} ${this.emitExpression(expression.receiver)})`,
          ...ordered.setup.map((line) => `  ${line}`),
          `  (call_ref $tsig${trait.index}_${method.index}`,
          `    (struct.get $trait${trait.index} $trait${trait.index}value (local.get ${temporary}))`,
          ...ordered.values.map((argument) => `    ${argument}`),
          ...expression.providers.map((provider) => `    ${this.emitExpression(provider)}`),
          `    (struct.get $trait${trait.index} $trait${trait.index}m${method.index} (local.get ${temporary})))`,
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
          (parent, provider, index) => `(struct.new $hd.providers (i32.const ${this.providerKey(expression.keys[index]!)}) ${this.boxProvider(this.emitExpression(provider), provider.type)} ${parent})`,
          base,
        );
      }
      case "provider-context": {
        const contextIndex = this.contextNames.get(expression.type);
        const finalProviders = new Map<string, string>();
        for (const entry of expression.entries) {
          if (entry.kind === "binding") finalProviders.set(entry.key, localName(entry.local.index));
          else entry.providers.forEach((provider) => finalProviders.set(provider.key, localName(provider.local.index)));
        }
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...this.emitProviderEntries(expression.entries).map((line) => `  ${line}`),
          `  (struct.new $context${contextIndex} ${expression.keys.map((key) => `(local.get ${finalProviders.get(key)})`).join(" ")})`,
          `)`,
        ].join("\n");
      }
      case "provider-with": {
        const result = expression.type === "void" ? "" : ` (result ${this.watType(expression.type)})`;
        return [
          `(block${result}`,
          ...this.emitProviderEntries(expression.entries).map((line) => `  ${line}`),
          indent(this.emitBlock(expression.body, expression.type), 2),
          `)`,
        ].join("\n");
      }
      case "data": {
        if (!expression.spread && expression.fields.length === 0) return `(struct.new $d${expression.dataIndex})`;
        const declaration = this.dataByIndex.get(expression.dataIndex)!;
        const spreadTemporary = expression.spread ? this.allocateTemporary(expression.spread.type) : undefined;
        const temporaries = expression.fields.map((field) => this.allocateTemporary(field.type));
        const sourceByField = new Map(expression.fieldIndices.map((fieldIndex, sourceIndex) => [fieldIndex, sourceIndex] as const));
        const storedFields = declaration.fields.map((field) => {
          const sourceIndex = sourceByField.get(field.index);
          if (sourceIndex === undefined) {
            if (!spreadTemporary) throw new Error(`data field '${field.name}' has no construction source`);
            return `(struct.get $d${expression.dataIndex} $d${expression.dataIndex}f${field.index} (local.get ${spreadTemporary}))`;
          }
          const value = `(local.get ${temporaries[sourceIndex]})`;
          return expression.erasedFieldTypes && isGenericValueType(expression.erasedFieldTypes[field.index]!)
            ? this.boxWatValue(value, expression.fields[sourceIndex]!.type)
            : value;
        });
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...(expression.spread ? [`  (local.set ${spreadTemporary} ${this.emitExpression(expression.spread)})`] : []),
          ...expression.fields.map((field, index) => `  (local.set ${temporaries[index]} ${this.emitExpression(field)})`),
          `  (struct.new $d${expression.dataIndex} ${storedFields.join(" ")})`,
          `)`,
        ].join("\n");
      }
      case "enum": {
        if (expression.fields.length === 0) {
          return `(global.get $e${expression.enumIndex}v${expression.tag})`;
        }
        const temporaries = expression.fields.map((field) => this.allocateTemporary(field.type));
        const sourceByField = new Map(expression.fieldIndices.map((fieldIndex, sourceIndex) => [fieldIndex, sourceIndex] as const));
        const storedFields = expression.fieldTypes.map((fieldType, fieldIndex) => {
          const sourceIndex = sourceByField.get(fieldIndex);
          if (sourceIndex === undefined) return this.defaultValue(fieldType);
          const value = `(local.get ${temporaries[sourceIndex]})`;
          return expression.erasedFieldTypes && isGenericValueType(expression.erasedFieldTypes[fieldIndex]!)
            ? this.boxWatValue(value, expression.fields[sourceIndex]!.type)
            : value;
        });
        return [
          `(block (result ${this.watType(expression.type)})`,
          ...expression.fields.map((field, index) => `  (local.set ${temporaries[index]} ${this.emitExpression(field)})`),
          `  (struct.new $e${expression.enumIndex} (i32.const ${expression.tag})${storedFields.length ? " " : ""}${storedFields.join(" ")})`,
          `)`,
        ].join("\n");
      }
      case "member":
        {
          const value = `(struct.get $d${expression.dataIndex} $d${expression.dataIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)})`;
          return expression.erasedFieldType && isGenericValueType(expression.erasedFieldType)
            ? this.unboxValue(value, expression.type)
            : value;
        }
      case "field-set": {
        const value = this.emitExpression(expression.value);
        const stored = expression.erasedFieldType && isGenericValueType(expression.erasedFieldType)
          ? this.boxWatValue(value, expression.value.type)
          : value;
        return `(struct.set $d${expression.dataIndex} $d${expression.dataIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)} ${stored})`;
      }
      case "enum-member":
        {
          const value = `(struct.get $e${expression.enumIndex} $e${expression.enumIndex}f${expression.fieldIndex} ${this.emitExpression(expression.receiver)})`;
          return expression.erasedFieldType && isGenericValueType(expression.erasedFieldType)
            ? this.unboxValue(value, expression.type)
            : value;
        }
      case "string-length":
        return `(call $hd.string_len ${this.emitExpression(expression.receiver)})`;
      case "string-starts-with":
        return `(call $hd.string_starts_with ${this.emitExpression(expression.receiver)} ${this.emitExpression(expression.prefix)})`;
      case "list-length":
        return `(struct.get $hd.vector $hd.vector-size (ref.as_non_null ${this.emitExpression(expression.receiver)}))`;
      case "list-index":
        return this.unboxValue(`(call $hd.vector_get (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.emitExpression(expression.index)})`, expression.elementType);
      case "list-set":
        return `(call $hd.vector_set (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.emitExpression(expression.index)} ${this.boxValue(expression.value, expression.elementType)})`;
      case "list-append":
        return `(call $hd.vector_append (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.value, expression.elementType)})`;
      case "tuple-index":
        return this.unboxValue(`(array.get $hd.list (ref.as_non_null ${this.emitExpression(expression.receiver)}) (i32.const ${expression.index}))`, expression.elementType);
      case "map-length":
        return `(struct.get $hd.map $hd.map-size (ref.as_non_null ${this.emitExpression(expression.receiver)}))`;
      case "map-index":
        return `(call $hd.map_get (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.key, expression.keyType)})`;
      case "map-remove":
        return `(call $hd.map_remove (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.key, expression.keyType)})`;
      case "map-set":
        return `(call $hd.map_insert (ref.as_non_null ${this.emitExpression(expression.receiver)}) ${this.boxValue(expression.key, expression.keyType)} ${this.boxValue(expression.value, expression.valueType)})`;
      case "map-entry-key":
        return this.unboxValue(`(array.get $hd.list (struct.get $hd.map $hd.map-keys (ref.as_non_null ${this.emitExpression(expression.receiver)})) ${this.emitExpression(expression.index)})`, expression.keyType);
      case "map-entry-value":
        return this.unboxValue(`(array.get $hd.list (struct.get $hd.map $hd.map-values (ref.as_non_null ${this.emitExpression(expression.receiver)})) ${this.emitExpression(expression.index)})`, expression.valueType);
      case "panic":
        return `(block (drop ${this.emitExpression(expression.message)}) unreachable)`;
      case "if": {
        const result = expression.type === "void" || expression.type === "never" ? "" : ` (result ${this.watType(expression.type)})`;
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
      case "for": {
        const id = this.loopCounter++;
        const labels = { breakLabel: `$break${id}`, continueLabel: `$continue${id}`, result: expression.elseBody.length > 0 ? expression.type : undefined };
        const iterable = this.allocateTemporary(expression.iterable.type);
        const index = this.allocateTemporary("i32");
        const yielded = this.allocateTemporary(expression.yieldType);
        const iterableValue = `(local.get ${iterable})`;
        const indexValue = `(local.get ${index})`;
        const length = expression.iteratorKind === "list"
          ? `(struct.get $hd.vector $hd.vector-size (ref.as_non_null ${iterableValue}))`
          : `(struct.get $hd.map $hd.map-size (ref.as_non_null ${iterableValue}))`;
        const nextValue = expression.iteratorKind === "list"
          ? this.unboxValue(`(call $hd.vector_get (ref.as_non_null ${iterableValue}) ${indexValue})`, expression.yieldType)
          : `(array.new_fixed $hd.list 2 (array.get $hd.list (struct.get $hd.map $hd.map-keys (ref.as_non_null ${iterableValue})) ${indexValue}) (array.get $hd.list (struct.get $hd.map $hd.map-values (ref.as_non_null ${iterableValue})) ${indexValue}))`;
        const bind = expression.bindings.length === 1
          ? [`(local.set ${localName(expression.bindings[0]!.index)} (local.get ${yielded}))`]
          : expression.bindings.map((binding, bindingIndex) => `(local.set ${localName(binding.index)} ${this.unboxValue(`(array.get $hd.list (ref.as_non_null (local.get ${yielded})) (i32.const ${bindingIndex}))`, binding.type)})`);
        this.loops.push(labels);
        const body = this.emitBlock(expression.body, "void", true);
        this.loops.pop();
        const iteration = [
          `(local.set ${yielded} ${nextValue})`,
          ...bind,
          `(block ${labels.continueLabel}`,
          indent(body, 2),
          `)`,
          `(local.set ${index} (i32.add ${indexValue} (i32.const 1)))`,
          `(br $loop${id})`,
        ];
        if (expression.elseBody.length > 0) {
          const result = expression.type === "void" || expression.type === "never" ? "" : ` (result ${this.watType(expression.type)})`;
          const elseBody = this.emitBlock(expression.elseBody, expression.type);
          return [
            `(block ${labels.breakLabel}${result}`,
            `  (local.set ${iterable} ${this.emitExpression(expression.iterable)})`,
            `  (local.set ${index} (i32.const 0))`,
            `  (loop $loop${id}`,
            `    (if (i32.lt_u ${indexValue} ${length})`,
            `      (then`,
            indent(iteration.join("\n"), 8),
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
          `  (local.set ${iterable} ${this.emitExpression(expression.iterable)})`,
          `  (local.set ${index} (i32.const 0))`,
          `  (loop $loop${id}`,
          `    (br_if ${labels.breakLabel} (i32.ge_u ${indexValue} ${length}))`,
          indent(iteration.join("\n"), 4),
          `  )`,
          `)`,
        ].join("\n");
      }
      case "while": {
        const id = this.loopCounter++;
        const labels = { breakLabel: `$break${id}`, continueLabel: `$loop${id}`, result: expression.elseBody.length > 0 ? expression.type : undefined };
        this.loops.push(labels);
        const body = this.emitBlock(expression.body, "void", true);
        this.loops.pop();
        if (expression.elseBody.length > 0) {
          const result = expression.type === "void" || expression.type === "never" ? "" : ` (result ${this.watType(expression.type)})`;
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
        const result = expression.type === "void" || expression.type === "never" ? "" : ` (result ${this.watType(expression.type)})`;
        return [
          `(block${result}`,
          `  (local.set ${subject} ${this.emitExpression(expression.subject)})`,
          indent(this.emitMatchArms(expression.representation, expression.enumIndex, subject, expression.arms, expression.type), 2),
          `)`,
        ].join("\n");
      }
    }
  }

  private emitProviderEntries(entries: readonly HirProviderContextEntry[]): string[] {
    const emitted: string[] = [];
    for (const entry of entries) {
      if (entry.kind === "binding") {
        emitted.push(`(local.set ${localName(entry.local.index)} ${this.emitExpression(entry.value)})`);
        continue;
      }
      const contextIndex = this.contextNames.get(entry.value.type);
      emitted.push(`(local.set ${localName(entry.contextLocal.index)} ${this.emitExpression(entry.value)})`);
      for (const provider of entry.providers) {
        emitted.push(`(local.set ${localName(provider.local.index)} (struct.get $context${contextIndex} $context${contextIndex}f${provider.fieldIndex} (local.get ${localName(entry.contextLocal.index)})))`);
      }
    }
    return emitted;
  }

  private allocateTemporary(type: ValueType): string {
    const index = this.temporaryTypes.length;
    this.temporaryTypes.push(type);
    return `$tmp${index}`;
  }

  private emitOrderedArguments(
    arguments_: readonly HirExpression[],
    parameterIndices?: readonly number[],
    erasedParameterTypes?: readonly ValueType[],
    defaultArguments?: readonly { readonly parameterIndex: number; readonly functionIndex: number }[],
    parameterTypes?: readonly ValueType[],
    defaultBounds?: readonly HirExpression[],
  ): { setup: readonly string[]; values: readonly string[] } {
    const emitValue = (argument: HirExpression, parameterIndex: number): string => {
      const formal = erasedParameterTypes?.[parameterIndex];
      if (formal && functionParts(formal) && formal !== argument.type && functionParts(argument.type)) {
        return this.emitCallableAdaptation(argument, formal, argument.type);
      }
      return formal && isGenericValueType(formal) ? this.boxValue(argument, argument.type) : this.emitExpression(argument);
    };
    if (!parameterIndices && !defaultArguments?.length) {
      return { setup: [], values: arguments_.map((argument, index) => emitValue(argument, index)) };
    }
    const setup: string[] = [];
    const values = new Array<string>(parameterTypes?.length ?? arguments_.length);
    arguments_.forEach((argument, argumentIndex) => {
      const parameterIndex = parameterIndices?.[argumentIndex] ?? argumentIndex;
      const temporary = this.allocateTemporary(erasedParameterTypes?.[parameterIndex] ?? parameterTypes?.[parameterIndex] ?? argument.type);
      setup.push(`(local.set ${temporary} ${emitValue(argument, parameterIndex)})`);
      values[parameterIndex] = `(local.get ${temporary})`;
    });
    for (const defaultArgument of defaultArguments ?? []) {
      const parameterIndex = defaultArgument.parameterIndex;
      const temporary = this.allocateTemporary(parameterTypes?.[parameterIndex] ?? erasedParameterTypes?.[parameterIndex] ?? "void");
      const inputs = [
        ...values.slice(0, parameterIndex),
        ...(defaultBounds ?? []).map((bound) => this.emitExpression(bound)),
      ];
      setup.push(`(local.set ${temporary} (call ${functionName(defaultArgument.functionIndex)}${inputs.length > 0 ? ` ${inputs.join(" ")}` : ""}))`);
      values[parameterIndex] = `(local.get ${temporary})`;
    }
    return { setup, values };
  }

  private emitValueEquality(left: string, right: string, type: ValueType): string {
    const readonly = readonlyType(type);
    if (readonly === "string") return `(i32.eqz (call $hd.string_compare ${left} ${right}))`;
    if (readonly === "f64") return `(f64.eq ${left} ${right})`;
    if (readonly === "i32" || readonly === "bool" || readonly === "char") return `(i32.eq ${left} ${right})`;
    const tuple = tupleParts(readonly);
    if (tuple !== undefined) {
      if (tuple.length === 0) return `(i32.const 1)`;
      const comparisons = tuple.map((elementType, index) => this.emitValueEquality(
        this.unboxValue(`(array.get $hd.list (ref.as_non_null ${left}) (i32.const ${index}))`, elementType),
        this.unboxValue(`(array.get $hd.list (ref.as_non_null ${right}) (i32.const ${index}))`, elementType),
        elementType,
      ));
      return comparisons.slice(1).reduce((combined, comparison) => `(i32.and ${combined} ${comparison})`, comparisons[0]!);
    }
    const nominal = nominalGenericParts(readonly);
    if (nominal?.name === "list" && nominal.arguments.length === 1) {
      const elementType = nominal.arguments[0]!;
      const leftTemporary = this.allocateTemporary(readonly);
      const rightTemporary = this.allocateTemporary(readonly);
      const indexTemporary = this.allocateTemporary("i32");
      const label = `$equality${this.loopCounter++}`;
      const leftElement = this.unboxValue(`(call $hd.vector_get (ref.as_non_null (local.get ${leftTemporary})) (local.get ${indexTemporary}))`, elementType);
      const rightElement = this.unboxValue(`(call $hd.vector_get (ref.as_non_null (local.get ${rightTemporary})) (local.get ${indexTemporary}))`, elementType);
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
        `    (if (i32.eqz ${this.emitValueEquality(leftElement, rightElement, elementType)})`,
        `      (then (br ${label} (i32.const 0))))`,
        `    (local.set ${indexTemporary} (i32.add (local.get ${indexTemporary}) (i32.const 1)))`,
        `    (br $${label.slice(1)}loop))`,
        `  (i32.const 1)`,
        `)`,
      ].join("\n");
    }
    throw new Error(`cannot emit PartialEq for '${type}'`);
  }

  defaultValue(type: ValueType): string {
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.defaultValue(mutable);
    if (isGenericValueType(type)) return `(ref.null any)`;
    if (type === "i32" || type === "bool" || type === "char") return `(i32.const 0)`;
    if (type === "f64") return `(f64.const 0)`;
    if (type === "string") return `(ref.null $hd.bytes)`;
    if (type.startsWith("trait:")) return `(ref.null $trait${this.traitsByName.get(traitTypeBase(type))?.index})`;
    if (type.startsWith("provider-row:")) return `(ref.null $hd.providers)`;
    if (type.startsWith("provider:")) return `(ref.null extern)`;
    if (contextKeys(type)) return `(ref.null $context${this.contextNames.get(type)})`;
    if (tupleParts(type) !== undefined) return `(ref.null $hd.list)`;
    const suspension = suspensionParts(type);
    if (suspension) return `(ref.null $s${suspension.functionIndex})`;
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension) return `(ref.null ${traitSuspensionName(traitSuspension.traitIndex, traitSuspension.methodIndex)})`;
    if (isErasedVariant(type)) return `(ref.null $hd.variant)`;
    const callable = functionParts(type);
    if (callable) return `(ref.null $closure${this.functionSignatures.get(type)})`;
    const data = this.dataByName.get(type);
    const nominalData = nominalGenericParts(type);
    if (nominalData?.name === "list" && nominalData.arguments.length === 1) return `(ref.null $hd.vector)`;
    if (nominalData?.name === "map" && nominalData.arguments.length === 2) return `(ref.null $hd.map)`;
    if (nominalData && this.dataByName.has(nominalData.name)) return `(ref.null $d${this.dataByName.get(nominalData.name)!.index})`;
    if (nominalData && this.enumByName.has(nominalData.name)) return `(ref.null $e${this.enumByName.get(nominalData.name)!.index})`;
    if (data) return `(ref.null $d${data.index})`;
    const enumType = this.enumByName.get(type);
    if (enumType) return `(ref.null $e${enumType.index})`;
    throw new Error(`cannot produce a default for '${type}'`);
  }

  private boxValue(expression: HirExpression, type: ValueType): string {
    return this.boxWatValue(this.emitExpression(expression), type);
  }

  private boxWatValue(value: string, type: ValueType): string {
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.boxWatValue(value, mutable);
    if (isGenericValueType(type)) return value;
    if (type === "i32" || type === "bool" || type === "char") return `(struct.new $hd.box-i32 ${value})`;
    if (type === "f64") return `(struct.new $hd.box-f64 ${value})`;
    if (type === "void") return `(ref.null any)`;
    return value;
  }

  private emitCallableAdaptation(expression: HirExpression, formalType: ValueType, actualType: ValueType): string {
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
    return this.adapters.map((adapter) => {
      const formal = functionParts(adapter.formalType)!;
      const actual = functionParts(adapter.actualType)!;
      const formalSignature = this.functionSignatures.get(adapter.formalType);
      const actualSignature = this.functionSignatures.get(adapter.actualType);
      const parameters = formal.parameters.map((parameter, index) => `(param $a${index} ${this.watType(parameter)})`);
      const providers = formal.requirements.map((requirement, index) => `(param $p${index} ${this.providerType(requirement)})`);
      const result = formal.result === "void" ? "" : ` (result ${this.watType(formal.result)})`;
      const closure = `(ref.cast (ref $closure${actualSignature}) (local.get $env))`;
      const arguments_ = formal.parameters.map((parameter, index) => {
        const value = `(local.get $a${index})`;
        return isGenericValueType(parameter) ? this.unboxValue(value, actual.parameters[index]!) : value;
      });
      const concreteFormal = new Map(formal.requirements.map((requirement, index) => [requirement, index] as const).filter(([requirement]) => !isRowRequirement(requirement)));
      const formalUnion = formal.requirements.reduceRight((parent, requirement, index) => {
        if (isRowRequirement(requirement)) return `(call $hd.provider_concat (local.get $p${index}) ${parent})`;
        const type = this.traitsByName.has(nominalGenericParts(requirement)?.name ?? requirement) ? `trait:${requirement}` : `provider:${requirement}`;
        return `(struct.new $hd.providers (i32.const ${this.providerKey(requirement)}) ${this.boxProvider(`(local.get $p${index})`, type)} ${parent})`;
      }, `(ref.null $hd.providers)`);
      const actualProviders = actual.requirements.map((requirement) => {
        if (isRowRequirement(requirement)) return formalUnion;
        const direct = concreteFormal.get(requirement);
        if (direct !== undefined) return `(local.get $p${direct})`;
        if (formal.requirements.length === 0) throw new Error(`cannot adapt requirement '${requirement}' from ${adapter.formalType} to ${adapter.actualType}`);
        return this.unboxProvider(`(call $hd.provider_get ${formalUnion} (i32.const ${this.providerKey(requirement)}))`, requirement);
      });
      const call = `(call_ref $sig${actualSignature} (struct.get $closure${actualSignature} $closure${actualSignature}env ${closure})${arguments_.length ? " " : ""}${arguments_.join(" ")}${actualProviders.length ? " " : ""}${actualProviders.join(" ")} (struct.get $closure${actualSignature} $closure${actualSignature}fn ${closure}))`;
      const body = isGenericValueType(formal.result) ? this.boxWatValue(call, actual.result) : call;
      return `(func $adapt${adapter.index} (type $sig${formalSignature}) (param $env anyref) ${[...parameters, ...providers].join(" ")}${result}\n  ${body}\n)`;
    }).join("\n\n");
  }

  emitTraitAdapters(): string {
    return [...this.implementationsByIndex.values()].flatMap((implementation) => {
      const trait = this.traitsByIndex.get(implementation.traitIndex)!;
      return implementation.methodFunctions.flatMap((mapping) => {
        const method = trait.methods[mapping.methodIndex]!;
        const parameters = method.parameters.map((parameter, index) => `(param $a${index} ${this.watType(parameter)})`);
        const providers = method.requirements.map((requirement, index) => `(param $p${index} ${this.providerType(requirement)})`);
        const result = method.result === "void" ? "" : ` (result ${this.watType(method.result)})`;
        const arguments_ = [
          this.unboxValue(`(local.get $self)`, implementation.targetType),
          ...method.parameters.map((_, index) => `(local.get $a${index})`),
          ...method.requirements.map((_, index) => `(local.get $p${index})`),
        ];
        if (!method.suspending) {
          return [`(func $tadapt${implementation.index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) ${[...parameters, ...providers].join(" ")}${result}\n  (call ${functionName(mapping.functionIndex)} ${arguments_.join(" ")})\n)`];
        }
        const wrapper = traitSuspensionName(trait.index, method.index);
        const frame = `$s${mapping.functionIndex}`;
        const constructor = `(func $tadapt${implementation.index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) ${[...parameters, ...providers].join(" ")} (result (ref null ${wrapper}))\n  (struct.new ${wrapper}\n    (call ${functionName(mapping.functionIndex)} ${arguments_.join(" ")})\n    (ref.func $tspolladapt${implementation.index}_${method.index})\n    (ref.func $tscanceladapt${implementation.index}_${method.index})\n    (ref.func $tsresultadapt${implementation.index}_${method.index}))\n)`;
        const poll = `(func $tspolladapt${implementation.index}_${method.index} (type $tspollsig${trait.index}_${method.index}) (param $inner anyref) (result i32)\n  (call $poll${mapping.functionIndex} (ref.cast (ref null ${frame}) (local.get $inner)))\n)`;
        const cancel = `(func $tscanceladapt${implementation.index}_${method.index} (type $tscancelsig${trait.index}_${method.index}) (param $inner anyref)\n  (call $cancel${mapping.functionIndex} (ref.cast (ref null ${frame}) (local.get $inner)))\n)`;
        const resultBody = method.result === "void"
          ? ""
          : `\n  (struct.get ${frame} ${frame}result (ref.cast (ref null ${frame}) (local.get $inner)))`;
        const resultAdapter = `(func $tsresultadapt${implementation.index}_${method.index} (type $tsresultsig${trait.index}_${method.index}) (param $inner anyref)${result}${resultBody}\n)`;
        return [constructor, poll, cancel, resultAdapter];
      });
    }).join("\n\n");
  }

  emitTraitSuspensionHelpers(): string {
    return [...this.traitsByIndex.values()].flatMap((trait) => trait.methods.flatMap((method) => {
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
    })).join("\n\n");
  }

  private unboxValue(payload: string, type: ValueType): string {
    const mutable = mutableInner(type);
    if (mutable !== undefined) return this.unboxValue(payload, mutable);
    if (isGenericValueType(type)) return payload;
    if (type === "i32" || type === "bool" || type === "char") {
      return `(struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) ${payload}))`;
    }
    if (type === "f64") {
      return `(struct.get $hd.box-f64 $hd.box-f64-value (ref.cast (ref $hd.box-f64) ${payload}))`;
    }
    if (type === "string") return `(ref.cast (ref null $hd.bytes) ${payload})`;
    if (type.startsWith("trait:")) return `(ref.cast (ref null $trait${this.traitsByName.get(traitTypeBase(type))?.index}) ${payload})`;
    if (type.startsWith("provider-row:")) return `(ref.cast (ref null $hd.providers) ${payload})`;
    if (contextKeys(type)) return `(ref.cast (ref null $context${this.contextNames.get(type)}) ${payload})`;
    if (tupleParts(type) !== undefined) return `(ref.cast (ref null $hd.list) ${payload})`;
    const suspension = suspensionParts(type);
    if (suspension) return `(ref.cast (ref null $s${suspension.functionIndex}) ${payload})`;
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension) return `(ref.cast (ref null ${traitSuspensionName(traitSuspension.traitIndex, traitSuspension.methodIndex)}) ${payload})`;
    if (isErasedVariant(type)) return `(ref.cast (ref null $hd.variant) ${payload})`;
    const callable = functionParts(type);
    if (callable) return `(ref.cast (ref null $closure${this.functionSignatures.get(type)}) ${payload})`;
    const data = this.dataByName.get(type);
    const nominalData = nominalGenericParts(type);
    if (nominalData?.name === "list" && nominalData.arguments.length === 1) return `(ref.cast (ref null $hd.vector) ${payload})`;
    if (nominalData?.name === "map" && nominalData.arguments.length === 2) return `(ref.cast (ref null $hd.map) ${payload})`;
    if (nominalData && this.dataByName.has(nominalData.name)) return `(ref.cast (ref null $d${this.dataByName.get(nominalData.name)!.index}) ${payload})`;
    if (nominalData && this.enumByName.has(nominalData.name)) return `(ref.cast (ref null $e${this.enumByName.get(nominalData.name)!.index}) ${payload})`;
    if (data) return `(ref.cast (ref null $d${data.index}) ${payload})`;
    const enumType = this.enumByName.get(type);
    if (enumType) return `(ref.cast (ref null $e${enumType.index}) ${payload})`;
    throw new Error(`cannot unbox '${type}'`);
  }

  private boxProvider(value: string, type: ValueType): string {
    return type.startsWith("provider:") ? `(struct.new $hd.box-extern ${value})` : value;
  }

  private unboxProvider(value: string, requirement: string): string {
    const trait = this.traitsByName.get(requirement);
    return trait
      ? `(ref.cast (ref null $trait${trait.index}) ${value})`
      : `(struct.get $hd.box-extern $hd.box-extern-value (ref.cast (ref $hd.box-extern) ${value}))`;
  }

  private emitMatchArms(
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
        ? this.emitEnumPayloadAccess(subject, enumIndex, binding.enumFieldIndex, binding.enumErasedFieldType, binding.enumFieldType!, binding.path ?? [])
        : binding.path
          ? this.emitDataPatternAccess(subject, binding.path)
        : binding.fieldIndex === -1
        ? `(local.get ${subject})`
        : representation === "enum"
          ? binding.erasedFieldType && isGenericValueType(binding.erasedFieldType)
            ? this.unboxValue(`(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${subject}))`, binding.type)
            : `(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${subject}))`
          : representation === "erased-variant"
            ? this.unboxValue(`(struct.get $hd.variant $hd.variant-payload (local.get ${subject}))`, binding.type)
            : `(local.get ${subject})`;
      return `(local.set ${localName(binding.local.index)} ${value})`;
    });
    const body = this.emitBlock(arm.body, resultType);
    const result = resultType === "void" || resultType === "never" ? "" : ` (result ${this.watType(resultType)})`;
    const guardedBody = arm.guard
      ? [
          ...bindings,
          `(if${result} ${this.emitExpression(arm.guard)}`,
          `  (then`,
          indent(body, 4),
          `  )`,
          `  (else`,
          indent(this.emitMatchArms(representation, enumIndex, subject, arms, resultType, index + 1), 4),
          `  )`,
          `)`,
        ].join("\n")
      : [...bindings, body].join("\n");
    let condition: string | undefined;
    if (arm.literal) {
      const value = this.emitExpression(arm.literal);
      condition = arm.literal.type === "string"
        ? `(i32.eq (call $hd.string_compare (local.get ${subject}) ${value}) (i32.const 0))`
        : arm.literal.type === "f64"
          ? `(f64.eq (local.get ${subject}) ${value})`
          : `(i32.eq (local.get ${subject}) ${value})`;
    } else if (arm.tag !== undefined) {
      const actual = representation === "enum"
        ? `(struct.get $e${enumIndex} $e${enumIndex}tag (local.get ${subject}))`
        : `(struct.get $hd.variant $hd.variant-tag (local.get ${subject}))`;
      condition = `(i32.eq ${actual} (i32.const ${arm.tag}))`;
    }
    for (const test of arm.tests ?? []) {
      const actual = this.emitMatchTestAccess(subject, enumIndex, test);
      const expected = test.tag !== undefined ? `(i32.const ${test.tag})` : this.emitExpression(test.literal!);
      const testCondition = test.tag !== undefined
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
      indent(this.emitMatchArms(representation, enumIndex, subject, arms, resultType, index + 1), 4),
      `  )`,
      `)`,
    ].join("\n");
  }

  private emitMatchTestAccess(
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
      return this.emitEnumPayloadAccess(subject, enumIndex, test.enumFieldIndex, test.erasedFieldType, test.valueType!, test.path ?? []);
    }
    return this.emitDataPatternAccess(subject, test.path!);
  }

  private emitPatternAccess(subject: string, path: readonly import("./hir.ts").HirPatternAccessStep[]): string {
    return path.reduce((value, step) => {
      if (step.kind === "erased-variant") {
        return this.unboxValue(`(struct.get $hd.variant $hd.variant-payload ${value})`, step.valueType);
      }
      const prefix = step.kind === "data" ? `$d${step.typeIndex}` : `$e${step.typeIndex}`;
      const raw = `(struct.get ${prefix} ${prefix}f${step.fieldIndex} ${value})`;
      return step.erasedFieldType && isGenericValueType(step.erasedFieldType)
        ? this.unboxValue(raw, step.valueType)
        : raw;
    }, `(local.get ${subject})`);
  }

  private emitEnumPayloadAccess(
    subject: string,
    enumIndex: number | undefined,
    fieldIndex: number,
    erasedFieldType: ValueType | undefined,
    valueType: ValueType,
    path: readonly { readonly dataIndex: number; readonly fieldIndex: number }[],
  ): string {
    if (enumIndex === undefined) throw new Error("enum payload access has no enum type");
    const raw = `(struct.get $e${enumIndex} $e${enumIndex}f${fieldIndex} (local.get ${subject}))`;
    const root = erasedFieldType && isGenericValueType(erasedFieldType) ? this.unboxValue(raw, valueType) : raw;
    return path.reduce(
      (value, step) => `(struct.get $d${step.dataIndex} $d${step.dataIndex}f${step.fieldIndex} ${value})`,
      root,
    );
  }

  private emitDataPatternAccess(subject: string, path: readonly { dataIndex: number; fieldIndex: number }[]): string {
    return path.reduce(
      (value, step) => `(struct.get $d${step.dataIndex} $d${step.dataIndex}f${step.fieldIndex} ${value})`,
      `(local.get ${subject})`,
    );
  }

  private emitFrameCleanups(frame: { cleanups: Array<readonly HirStatement[]> }): string[] {
    return [...frame.cleanups].reverse().map((cleanup) => this.emitBlock(cleanup, "void"));
  }

  private emitExitCleanups(stopAtLoop: boolean): string[] {
    const emitted: string[] = [];
    for (let index = this.cleanupFrames.length - 1; index >= 0; index -= 1) {
      const frame = this.cleanupFrames[index]!;
      emitted.push(...this.emitFrameCleanups(frame));
      if (stopAtLoop && frame.loopBoundary) break;
    }
    return emitted;
  }
}

const RUNTIME_WAT = `  (global $hd.runtime-root
    (mut (ref null $hd.runtime))
    (ref.null $hd.runtime))

  (global $hd.driver-active (mut i32) (i32.const 0))

  (func $hd.provider_get
    (param $providers (ref null $hd.providers))
    (param $key i32)
    (result anyref)
    (local $cursor (ref null $hd.providers))
    (local.set $cursor (local.get $providers))
    (block $missing
      (loop $search
        (br_if $missing (ref.is_null (local.get $cursor)))
        (if
          (i32.eq
            (struct.get $hd.providers $hd.provider-key (ref.as_non_null (local.get $cursor)))
            (local.get $key))
          (then
            (return
              (struct.get $hd.providers $hd.provider-value (ref.as_non_null (local.get $cursor))))))
        (local.set $cursor
          (struct.get $hd.providers $hd.provider-parent (ref.as_non_null (local.get $cursor))))
        (br $search)))
    unreachable)

  (func $hd.provider_concat
    (param $left (ref null $hd.providers))
    (param $right (ref null $hd.providers))
    (result (ref null $hd.providers))
    (if (result (ref null $hd.providers))
      (ref.is_null (local.get $left))
      (then (local.get $right))
      (else
        (struct.new $hd.providers
          (struct.get $hd.providers $hd.provider-key (ref.as_non_null (local.get $left)))
          (struct.get $hd.providers $hd.provider-value (ref.as_non_null (local.get $left)))
          (call $hd.provider_concat
            (struct.get $hd.providers $hd.provider-parent (ref.as_non_null (local.get $left)))
            (local.get $right))))))

  (func $hd.vector_get
    (param $vector (ref $hd.vector))
    (param $index i32)
    (result anyref)
    (if (i32.ge_u (local.get $index) (struct.get $hd.vector $hd.vector-size (local.get $vector)))
      (then unreachable))
    (array.get $hd.list
      (struct.get $hd.vector $hd.vector-values (local.get $vector))
      (local.get $index)))

  (func $hd.vector_set
    (param $vector (ref $hd.vector))
    (param $index i32)
    (param $value anyref)
    (if (i32.ge_u (local.get $index) (struct.get $hd.vector $hd.vector-size (local.get $vector)))
      (then unreachable))
    (array.set $hd.list
      (struct.get $hd.vector $hd.vector-values (local.get $vector))
      (local.get $index)
      (local.get $value)))

  (func $hd.vector_append
    (param $vector (ref $hd.vector))
    (param $value anyref)
    (local $size i32)
    (local $capacity i32)
    (local $next-capacity i32)
    (local $index i32)
    (local $old-values (ref $hd.list))
    (local $new-values (ref $hd.list))
    (local.set $size (struct.get $hd.vector $hd.vector-size (local.get $vector)))
    (local.set $old-values (struct.get $hd.vector $hd.vector-values (local.get $vector)))
    (local.set $capacity (array.len (local.get $old-values)))
    (if (i32.ge_u (local.get $size) (local.get $capacity))
      (then
        (local.set $next-capacity
          (if (result i32) (i32.eqz (local.get $capacity))
            (then (i32.const 4))
            (else (i32.mul (local.get $capacity) (i32.const 2)))))
        (local.set $new-values (array.new_default $hd.list (local.get $next-capacity)))
        (block $copied
          (loop $copy
            (br_if $copied (i32.ge_u (local.get $index) (local.get $size)))
            (array.set $hd.list
              (local.get $new-values)
              (local.get $index)
              (array.get $hd.list (local.get $old-values) (local.get $index)))
            (local.set $index (i32.add (local.get $index) (i32.const 1)))
            (br $copy)))
        (struct.set $hd.vector $hd.vector-values (local.get $vector) (local.get $new-values))))
    (array.set $hd.list
      (struct.get $hd.vector $hd.vector-values (local.get $vector))
      (local.get $size)
      (local.get $value))
    (struct.set $hd.vector $hd.vector-size
      (local.get $vector)
      (i32.add (local.get $size) (i32.const 1))))

  (func $hd.add_i32 (param $left i32) (param $right i32) (result i32)
    (local $wide i64)
    (local.set $wide
      (i64.add (i64.extend_i32_s (local.get $left)) (i64.extend_i32_s (local.get $right))))
    (if (i32.or
      (i64.lt_s (local.get $wide) (i64.const -2147483648))
      (i64.gt_s (local.get $wide) (i64.const 2147483647)))
      (then unreachable))
    (i32.wrap_i64 (local.get $wide)))

  (func $hd.sub_i32 (param $left i32) (param $right i32) (result i32)
    (local $wide i64)
    (local.set $wide
      (i64.sub (i64.extend_i32_s (local.get $left)) (i64.extend_i32_s (local.get $right))))
    (if (i32.or
      (i64.lt_s (local.get $wide) (i64.const -2147483648))
      (i64.gt_s (local.get $wide) (i64.const 2147483647)))
      (then unreachable))
    (i32.wrap_i64 (local.get $wide)))

  (func $hd.mul_i32 (param $left i32) (param $right i32) (result i32)
    (local $wide i64)
    (local.set $wide
      (i64.mul (i64.extend_i32_s (local.get $left)) (i64.extend_i32_s (local.get $right))))
    (if (i32.or
      (i64.lt_s (local.get $wide) (i64.const -2147483648))
      (i64.gt_s (local.get $wide) (i64.const 2147483647)))
      (then unreachable))
    (i32.wrap_i64 (local.get $wide)))

  (func $hd.pow_i32 (param $base i32) (param $exponent i32) (result i32)
    (local $result i32)
    (if (i32.lt_s (local.get $exponent) (i32.const 0)) (then unreachable))
    (local.set $result (i32.const 1))
    (block $done
      (loop $next
        (br_if $done (i32.eqz (local.get $exponent)))
        (if (i32.and (local.get $exponent) (i32.const 1))
          (then (local.set $result (call $hd.mul_i32 (local.get $result) (local.get $base)))))
        (local.set $exponent (i32.shr_u (local.get $exponent) (i32.const 1)))
        (if (local.get $exponent)
          (then (local.set $base (call $hd.mul_i32 (local.get $base) (local.get $base)))))
        (br $next)))
    (local.get $result))

  (func $hd.neg_i32 (param $value i32) (result i32)
    (if (i32.eq (local.get $value) (i32.const -2147483648)) (then unreachable))
    (i32.sub (i32.const 0) (local.get $value)))

  (func $hd.shl_i32 (param $value i32) (param $count i32) (result i32)
    (if (i32.ge_u (local.get $count) (i32.const 32)) (then unreachable))
    (i32.shl (local.get $value) (local.get $count)))

  (func $hd.shr_i32 (param $value i32) (param $count i32) (result i32)
    (if (i32.ge_u (local.get $count) (i32.const 32)) (then unreachable))
    (i32.shr_s (local.get $value) (local.get $count)))

  (func $hd.string_len (param $value (ref null $hd.bytes)) (result i32)
    (local $bytes (ref $hd.bytes))
    (local $index i32)
    (local $length i32)
    (local $scalars i32)
    (local.set $bytes (ref.as_non_null (local.get $value)))
    (local.set $length (array.len (local.get $bytes)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (local.get $length)))
        (if
          (i32.ne
            (i32.and
              (array.get_u $hd.bytes (local.get $bytes) (local.get $index))
              (i32.const 192))
            (i32.const 128))
          (then
            (local.set $scalars
              (i32.add (local.get $scalars) (i32.const 1)))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (local.get $scalars))

  (func $hd.string_concat
    (param $left-value (ref null $hd.bytes))
    (param $right-value (ref null $hd.bytes))
    (result (ref null $hd.bytes))
    (local $left (ref $hd.bytes))
    (local $right (ref $hd.bytes))
    (local $result (ref $hd.bytes))
    (local $left-length i32)
    (local $length i32)
    (local $index i32)
    (local.set $left (ref.as_non_null (local.get $left-value)))
    (local.set $right (ref.as_non_null (local.get $right-value)))
    (local.set $left-length (array.len (local.get $left)))
    (local.set $length
      (i32.add (local.get $left-length) (array.len (local.get $right))))
    (if (i32.lt_u (local.get $length) (local.get $left-length)) (then unreachable))
    (local.set $result (array.new_default $hd.bytes (local.get $length)))
    (block $left-done
      (loop $copy-left
        (br_if $left-done (i32.ge_u (local.get $index) (local.get $left-length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (array.get_u $hd.bytes (local.get $left) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $copy-left)))
    (block $right-done
      (loop $copy-right
        (br_if $right-done (i32.ge_u (local.get $index) (local.get $length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (array.get_u $hd.bytes
            (local.get $right)
            (i32.sub (local.get $index) (local.get $left-length))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $copy-right)))
    (local.get $result))

  (func $hd.i32_to_string (param $value i32) (result (ref null $hd.bytes))
    (local $magnitude i64)
    (local $remaining i64)
    (local $negative i32)
    (local $digits i32)
    (local $length i32)
    (local $index i32)
    (local $result (ref $hd.bytes))
    (local.set $magnitude (i64.extend_i32_s (local.get $value)))
    (if (i64.lt_s (local.get $magnitude) (i64.const 0))
      (then
        (local.set $negative (i32.const 1))
        (local.set $magnitude (i64.sub (i64.const 0) (local.get $magnitude)))))
    (local.set $remaining (local.get $magnitude))
    (local.set $digits (i32.const 1))
    (block $counted
      (loop $count
        (br_if $counted (i64.lt_u (local.get $remaining) (i64.const 10)))
        (local.set $remaining (i64.div_u (local.get $remaining) (i64.const 10)))
        (local.set $digits (i32.add (local.get $digits) (i32.const 1)))
        (br $count)))
    (local.set $length (i32.add (local.get $digits) (local.get $negative)))
    (local.set $index (local.get $length))
    (local.set $result (array.new_default $hd.bytes (local.get $length)))
    (local.set $remaining (local.get $magnitude))
    (block $written
      (loop $write
        (local.set $index (i32.sub (local.get $index) (i32.const 1)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (i32.add (i32.wrap_i64 (i64.rem_u (local.get $remaining) (i64.const 10))) (i32.const 48)))
        (local.set $remaining (i64.div_u (local.get $remaining) (i64.const 10)))
        (br_if $write (i32.gt_u (local.get $index) (local.get $negative)))))
    (if (local.get $negative)
      (then (array.set $hd.bytes (local.get $result) (i32.const 0) (i32.const 45))))
    (local.get $result))

  (func $hd.char_to_string (param $value i32) (result (ref null $hd.bytes))
    (local $result (ref $hd.bytes))
    (if (result (ref null $hd.bytes)) (i32.le_u (local.get $value) (i32.const 127))
      (then (array.new_fixed $hd.bytes 1 (local.get $value)))
      (else
        (if (result (ref null $hd.bytes)) (i32.le_u (local.get $value) (i32.const 2047))
          (then
            (array.new_fixed $hd.bytes 2
              (i32.or (i32.const 192) (i32.shr_u (local.get $value) (i32.const 6)))
              (i32.or (i32.const 128) (i32.and (local.get $value) (i32.const 63)))))
          (else
            (if (result (ref null $hd.bytes)) (i32.le_u (local.get $value) (i32.const 65535))
              (then
                (array.new_fixed $hd.bytes 3
                  (i32.or (i32.const 224) (i32.shr_u (local.get $value) (i32.const 12)))
                  (i32.or (i32.const 128) (i32.and (i32.shr_u (local.get $value) (i32.const 6)) (i32.const 63)))
                  (i32.or (i32.const 128) (i32.and (local.get $value) (i32.const 63)))))
              (else
                (array.new_fixed $hd.bytes 4
                  (i32.or (i32.const 240) (i32.shr_u (local.get $value) (i32.const 18)))
                  (i32.or (i32.const 128) (i32.and (i32.shr_u (local.get $value) (i32.const 12)) (i32.const 63)))
                  (i32.or (i32.const 128) (i32.and (i32.shr_u (local.get $value) (i32.const 6)) (i32.const 63)))
                  (i32.or (i32.const 128) (i32.and (local.get $value) (i32.const 63)))))))))))

  (func $hd.string_starts_with
    (param $value-source (ref null $hd.bytes))
    (param $prefix-source (ref null $hd.bytes))
    (result i32)
    (local $value (ref $hd.bytes))
    (local $prefix (ref $hd.bytes))
    (local $index i32)
    (local.set $value (ref.as_non_null (local.get $value-source)))
    (local.set $prefix (ref.as_non_null (local.get $prefix-source)))
    (if (i32.gt_u (array.len (local.get $prefix)) (array.len (local.get $value)))
      (then (return (i32.const 0))))
    (block $matched
      (loop $next
        (br_if $matched (i32.ge_u (local.get $index) (array.len (local.get $prefix))))
        (if
          (i32.ne
            (array.get_u $hd.bytes (local.get $value) (local.get $index))
            (array.get_u $hd.bytes (local.get $prefix) (local.get $index)))
          (then (return (i32.const 0))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (i32.const 1))

  (func $hd.string_compare
    (param $left-value (ref null $hd.bytes))
    (param $right-value (ref null $hd.bytes))
    (result i32)
    (local $left (ref $hd.bytes))
    (local $right (ref $hd.bytes))
    (local $index i32)
    (local $limit i32)
    (local $left-byte i32)
    (local $right-byte i32)
    (local.set $left (ref.as_non_null (local.get $left-value)))
    (local.set $right (ref.as_non_null (local.get $right-value)))
    (local.set $limit
      (if (result i32)
        (i32.lt_u (array.len (local.get $left)) (array.len (local.get $right)))
        (then (array.len (local.get $left)))
        (else (array.len (local.get $right)))))
    (block $different
      (loop $next-byte
        (br_if $different (i32.ge_u (local.get $index) (local.get $limit)))
        (local.set $left-byte (array.get_u $hd.bytes (local.get $left) (local.get $index)))
        (local.set $right-byte (array.get_u $hd.bytes (local.get $right) (local.get $index)))
        (if (i32.lt_u (local.get $left-byte) (local.get $right-byte)) (then (return (i32.const -1))))
        (if (i32.gt_u (local.get $left-byte) (local.get $right-byte)) (then (return (i32.const 1))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next-byte)))
    (if (result i32)
      (i32.lt_u (array.len (local.get $left)) (array.len (local.get $right)))
      (then (i32.const -1))
      (else
        (if (result i32)
          (i32.gt_u (array.len (local.get $left)) (array.len (local.get $right)))
          (then (i32.const 1))
          (else (i32.const 0))))))`;

const MAP_RUNTIME_WAT = `  (func $hd.map_key_equal
    (param $kind i32)
    (param $left anyref)
    (param $right anyref)
    (result i32)
    (if (result i32)
      (i32.eqz (local.get $kind))
      (then
        (i32.eq
          (struct.get $hd.box-i32 $hd.box-i32-value
            (ref.cast (ref $hd.box-i32) (local.get $left)))
          (struct.get $hd.box-i32 $hd.box-i32-value
            (ref.cast (ref $hd.box-i32) (local.get $right)))))
      (else
        (i32.eqz
          (call $hd.string_compare
            (ref.cast (ref $hd.bytes) (local.get $left))
            (ref.cast (ref $hd.bytes) (local.get $right)))))))

  (func $hd.map_insert
    (param $map (ref $hd.map))
    (param $key anyref)
    (param $value anyref)
    (local $index i32)
    (local $size i32)
    (local $capacity i32)
    (local $new-keys (ref $hd.list))
    (local $new-values (ref $hd.list))
    (local.set $size
      (struct.get $hd.map $hd.map-size (local.get $map)))
    (block $append
      (loop $scan
        (br_if $append
          (i32.ge_u (local.get $index) (local.get $size)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key))
          (then
            (array.set $hd.list
              (struct.get $hd.map $hd.map-values (local.get $map))
              (local.get $index)
              (local.get $value))
            (return)))
        (local.set $index
          (i32.add (local.get $index) (i32.const 1)))
        (br $scan)))
    (if
      (i32.ge_u
        (local.get $size)
        (array.len (struct.get $hd.map $hd.map-keys (local.get $map))))
      (then
        (local.set $capacity
          (if (result i32)
            (i32.eqz (local.get $size))
            (then (i32.const 4))
            (else (i32.mul (local.get $size) (i32.const 2)))))
        (local.set $new-keys
          (array.new_default $hd.list (local.get $capacity)))
        (local.set $new-values
          (array.new_default $hd.list (local.get $capacity)))
        (local.set $index (i32.const 0))
        (block $copied
          (loop $copy
            (br_if $copied
              (i32.ge_u (local.get $index) (local.get $size)))
            (array.set $hd.list
              (local.get $new-keys)
              (local.get $index)
              (array.get $hd.list
                (struct.get $hd.map $hd.map-keys (local.get $map))
                (local.get $index)))
            (array.set $hd.list
              (local.get $new-values)
              (local.get $index)
              (array.get $hd.list
                (struct.get $hd.map $hd.map-values (local.get $map))
                (local.get $index)))
            (local.set $index
              (i32.add (local.get $index) (i32.const 1)))
            (br $copy)))
        (struct.set $hd.map $hd.map-keys
          (local.get $map)
          (local.get $new-keys))
        (struct.set $hd.map $hd.map-values
          (local.get $map)
          (local.get $new-values))))
    (array.set $hd.list
      (struct.get $hd.map $hd.map-keys (local.get $map))
      (local.get $size)
      (local.get $key))
    (array.set $hd.list
      (struct.get $hd.map $hd.map-values (local.get $map))
      (local.get $size)
      (local.get $value))
    (struct.set $hd.map $hd.map-size
      (local.get $map)
      (i32.add (local.get $size) (i32.const 1))))

  (func $hd.map_get
    (param $map (ref $hd.map))
    (param $key anyref)
    (result (ref $hd.variant))
    (local $index i32)
    (local.set $index
      (struct.get $hd.map $hd.map-size (local.get $map)))
    (block $missing
      (loop $scan
        (br_if $missing (i32.eqz (local.get $index)))
        (local.set $index
          (i32.sub (local.get $index) (i32.const 1)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key))
          (then
            (return
              (struct.new $hd.variant
                (i32.const 1)
                (array.get $hd.list
                  (struct.get $hd.map $hd.map-values (local.get $map))
                  (local.get $index))))))
        (br $scan)))
    (struct.new $hd.variant (i32.const 0) (ref.null any)))

  (func $hd.map_remove
    (param $map (ref $hd.map))
    (param $key anyref)
    (result (ref $hd.variant))
    (local $index i32)
    (local $size i32)
    (local $removed anyref)
    (local.set $size
      (struct.get $hd.map $hd.map-size (local.get $map)))
    (block $missing
      (loop $scan
        (br_if $missing
          (i32.ge_u (local.get $index) (local.get $size)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key))
          (then
            (local.set $removed
              (array.get $hd.list
                (struct.get $hd.map $hd.map-values (local.get $map))
                (local.get $index)))
            (block $shifted
              (loop $shift
                (br_if $shifted
                  (i32.ge_u
                    (i32.add (local.get $index) (i32.const 1))
                    (local.get $size)))
                (array.set $hd.list
                  (struct.get $hd.map $hd.map-keys (local.get $map))
                  (local.get $index)
                  (array.get $hd.list
                    (struct.get $hd.map $hd.map-keys (local.get $map))
                    (i32.add (local.get $index) (i32.const 1))))
                (array.set $hd.list
                  (struct.get $hd.map $hd.map-values (local.get $map))
                  (local.get $index)
                  (array.get $hd.list
                    (struct.get $hd.map $hd.map-values (local.get $map))
                    (i32.add (local.get $index) (i32.const 1))))
                (local.set $index
                  (i32.add (local.get $index) (i32.const 1)))
                (br $shift)))
            (array.set $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (i32.sub (local.get $size) (i32.const 1))
              (ref.null any))
            (array.set $hd.list
              (struct.get $hd.map $hd.map-values (local.get $map))
              (i32.sub (local.get $size) (i32.const 1))
              (ref.null any))
            (struct.set $hd.map $hd.map-size
              (local.get $map)
              (i32.sub (local.get $size) (i32.const 1)))
            (return
              (struct.new $hd.variant
                (i32.const 1)
                (local.get $removed)))))
        (local.set $index
          (i32.add (local.get $index) (i32.const 1)))
        (br $scan)))
    (struct.new $hd.variant (i32.const 0) (ref.null any)))`;

const CONSOLE_RUNTIME_WAT = `  (func $hd.console_print
    (param $provider externref)
    (param $value-source (ref null $hd.bytes))
    (local $value (ref $hd.bytes))
    (local $index i32)
    (local.set $value (ref.as_non_null (local.get $value-source)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (array.len (local.get $value))))
        (call $hd.console_byte
          (local.get $provider)
          (array.get_u $hd.bytes (local.get $value) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (call $hd.console_byte (local.get $provider) (i32.const -1)))`;

export function emitWat(program: HirProgram): string {
  const signatureNames = new Map<ValueType, number>();
  const contextNames = new Map<ValueType, number>();
  const traitsByName = new Map(program.traits.map((trait) => [trait.name, trait]));
  const collectType = (type: ValueType): void => {
    if (type.startsWith("trait:")) return;
    const tuple = tupleParts(type);
    if (tuple !== undefined) {
      tuple.forEach(collectType);
      return;
    }
    const nominal = nominalGenericParts(type);
    if (nominal?.name === "list" && nominal.arguments.length === 1) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (nominal?.name === "map" && nominal.arguments.length === 2) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (nominal && program.data.some((declaration) => declaration.name === nominal.name)) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (nominal && program.enums.some((declaration) => declaration.name === nominal.name)) {
      nominal.arguments.forEach(collectType);
      return;
    }
    if (contextKeys(type)) {
      if (!contextNames.has(type)) contextNames.set(type, contextNames.size);
      return;
    }
    const suspension = suspensionParts(type);
    if (suspension) {
      collectType(suspension.result);
      return;
    }
    const traitSuspension = traitSuspensionParts(type);
    if (traitSuspension) {
      collectType(traitSuspension.result);
      return;
    }
    const callable = functionParts(type);
    if (callable) {
      if (!signatureNames.has(type)) signatureNames.set(type, signatureNames.size);
      callable.parameters.forEach(collectType);
      collectType(callable.result);
      return;
    }
    const optional = optionalInner(type);
    if (optional !== undefined) collectType(optional);
    const result = resultParts(type);
    if (result) {
      collectType(result.ok);
      collectType(result.error);
    }
  };
  for (const declaration of [...program.functions, ...program.closures]) {
    declaration.parameters.forEach((parameter) => collectType(parameter.type));
    declaration.locals.forEach((local) => collectType(local.type));
    collectType(declaration.result);
    if (!declaration.closure && !declaration.suspending) {
      collectType(functionType(declaration.parameters.map((parameter) => parameter.type), declaration.result, declaration.requirements, declaration.variadic));
    }
  }
  program.data.forEach((declaration) => declaration.fields.forEach((field) => collectType(field.type)));
  program.enums.forEach((declaration) => declaration.fields.forEach((field) => collectType(field.type)));
  program.traits.forEach((trait) => trait.methods.forEach((method) => {
    method.parameters.forEach(collectType);
    collectType(method.result);
  }));

  const suspensionPlans = new Map(program.functions
    .filter((declaration) => declaration.suspending && needsSuspensionCfg(declaration))
    .map((declaration) => [declaration.index, buildSuspensionPlan(declaration)] as const));
  const emitter = new FunctionEmitter(program.data, program.enums, signatureNames, contextNames, program.closures, program.traits, program.implementations, suspensionPlans);
  const signatureTypes = [...signatureNames].map(([type, index]) => {
    const callable = functionParts(type)!;
    const parameters = [
      `(param anyref)`,
      ...callable.parameters.map((parameter) => `(param ${emitter.watType(parameter)})`),
      ...callable.requirements.map((requirement) => `(param ${providerWatType(requirement, traitsByName)})`),
    ].join(" ");
    const result = callable.result === "void" ? "" : ` (result ${emitter.watType(callable.result)})`;
    return `    (type $sig${index} (func${parameters ? " " + parameters : ""}${result}))`;
  }).join("\n");
  const traitMethodTypes = program.traits.flatMap((trait) => trait.methods.map((method) => {
    const parameters = [
      `(param anyref)`,
      ...method.parameters.map((parameter) => `(param ${emitter.watType(parameter)})`),
      ...method.requirements.map((requirement) => `(param ${providerWatType(requirement, traitsByName)})`),
    ].join(" ");
    const result = method.suspending
      ? ` (result (ref null ${traitSuspensionName(trait.index, method.index)}))`
      : method.result === "void" ? "" : ` (result ${emitter.watType(method.result)})`;
    return `    (type $tsig${trait.index}_${method.index} (func ${parameters}${result}))`;
  })).join("\n");
  const traitSuspensionTypes = program.traits.flatMap((trait) => trait.methods.flatMap((method) => {
    if (!method.suspending) return [];
    const result = method.result === "void" ? "" : ` (result ${emitter.watType(method.result)})`;
    const wrapper = traitSuspensionName(trait.index, method.index);
    return [
      `    (type $tspollsig${trait.index}_${method.index} (func (param anyref) (result i32)))`,
      `    (type $tscancelsig${trait.index}_${method.index} (func (param anyref)))`,
      `    (type $tsresultsig${trait.index}_${method.index} (func (param anyref)${result}))`,
      `    (type ${wrapper} (struct\n      (field ${wrapper}inner anyref)\n      (field ${wrapper}poll (ref $tspollsig${trait.index}_${method.index}))\n      (field ${wrapper}cancel (ref $tscancelsig${trait.index}_${method.index}))\n      (field ${wrapper}result (ref $tsresultsig${trait.index}_${method.index}))))`,
    ];
  })).join("\n");
  const dataTypes = `\n  (rec\n${signatureTypes ? signatureTypes + "\n" : ""}${traitMethodTypes ? traitMethodTypes + "\n" : ""}${traitSuspensionTypes ? traitSuspensionTypes + "\n" : ""}    (type $hd.bytes (array (mut i8)))
    (type $hd.list (array (mut anyref)))
    (type $hd.vector (struct
      (field $hd.vector-size (mut i32))
      (field $hd.vector-values (mut (ref $hd.list)))))
    (type $hd.map (struct
      (field $hd.map-key-kind i32)
      (field $hd.map-size (mut i32))
      (field $hd.map-keys (mut (ref $hd.list)))
      (field $hd.map-values (mut (ref $hd.list)))))
    (type $hd.providers (struct
      (field $hd.provider-key i32)
      (field $hd.provider-value anyref)
      (field $hd.provider-parent (ref null $hd.providers))))
    (type $hd.box-i32 (struct
      (field $hd.box-i32-value i32)))
    (type $hd.box-f64 (struct
      (field $hd.box-f64-value f64)))
    (type $hd.box-extern (struct
      (field $hd.box-extern-value externref)))
    (type $hd.variant (struct
      (field $hd.variant-tag i32)
      (field $hd.variant-payload (mut anyref))))
${[...signatureNames].map(([, index]) => `    (type $closure${index} (struct
      (field $closure${index}fn (ref $sig${index}))
      (field $closure${index}env anyref)))`).join("\n")}
${[...contextNames].map(([type, index]) => `    (type $context${index} (struct\n${contextKeys(type)!.map((key, fieldIndex) => `      (field $context${index}f${fieldIndex} ${providerWatType(key, traitsByName)})`).join("\n")}))`).join("\n")}
${program.traits.map((trait) => `    (type $trait${trait.index} (struct
      (field $trait${trait.index}value anyref)${trait.methods.map((method) => `\n      (field $trait${trait.index}m${method.index} (ref $tsig${trait.index}_${method.index}))`).join("")}))`).join("\n")}
${program.functions.filter((declaration) => declaration.suspending).map((declaration) => {
    const plan = suspensionPlans.get(declaration.index);
    const sites = plan?.sites ?? linearSuspensionSites(declaration);
    const storedLocals = sites.length > 0
      ? [...declaration.locals.filter((local) => !local.parameter), ...(plan?.temporaries ?? [])]
      : [];
    return `    (type $s${declaration.index} (struct
      (field $s${declaration.index}state (mut i32))
      (field $s${declaration.index}polls (mut i32))${declaration.parameters.map((parameter, index) => `\n      (field $s${declaration.index}a${index} ${emitter.watType(parameter.type)})`).join("")}${declaration.genericBounds.map((bound, index) => `\n      (field $s${declaration.index}b${index} (ref null $trait${bound.traitIndex}))`).join("")}${declaration.requirements.map((requirement, index) => `\n      (field $s${declaration.index}p${index} ${providerWatType(requirement, traitsByName)})`).join("")}${storedLocals.map((local) => `\n      (field $s${declaration.index}l${local.index} (mut ${emitter.watType(local.type)}))`).join("")}${sites.map((site) => `\n      (field $s${declaration.index}child${"siteIndex" in site ? site.siteIndex : site.index} (mut (ref null ${suspensionFrameTypeName(site.drive)})))`).join("")}${declaration.result === "void" ? "" : `\n      (field $s${declaration.index}result (mut ${emitter.watType(declaration.result)}))`}))`;
  }).join("\n")}
${program.closures.map((closure) => `    (type $env${closure.index} (struct${closure.captures.length ? "\n" + closure.captures.map((capture) => `      (field $env${closure.index}f${capture.fieldIndex} ${emitter.watType(capture.source.type)})`).join("\n") : ""}))`).join("\n")}\n${program.data.map((declaration) => {
    const fields = declaration.fields.map((field) => `      (field $d${declaration.index}f${field.index} (mut ${emitter.watType(field.type)}))`).join("\n");
    return `    (type $d${declaration.index} (struct\n${fields}))`;
  }).join("\n")}\n${program.enums.map((declaration) => {
    const fields = declaration.fields.map((field) => `      (field $e${declaration.index}f${field.index} (mut ${emitter.watType(field.type)}))`).join("\n");
    return `    (type $e${declaration.index} (struct\n      (field $e${declaration.index}tag i32)${fields ? "\n" + fields : ""}))`;
  }).join("\n")}\n    (type $hd.runtime (struct\n      (field $status (mut i32))\n      (field $scratch (mut (ref null $hd.bytes)))))\n  )\n`;
  const enumSingletons = program.enums.flatMap((declaration) => declaration.variants
    .filter((variant) => declaration.sharedFields.length === 0 && variant.fields.length === 0)
    .map((variant) => `  (global $e${declaration.index}v${variant.tag} (ref $e${declaration.index})\n    (struct.new $e${declaration.index} (i32.const ${variant.tag})${declaration.fields.map((field) => ` ${emitter.defaultValue(field.type)}`).join("")}))`))
    .join("\n");
  const functions = [...program.functions, ...program.closures].map((declaration) => [
    declaration.suspending && (suspensionPlans.has(declaration.index) || linearSuspensionSites(declaration).length > 0) ? "" : indent(emitter.emit(declaration)),
    declaration.suspending ? indent(emitter.emitSuspensionSupport(declaration)) : "",
    !declaration.closure && !declaration.suspending && declaration.genericParameters.length === 0 ? indent(emitter.emitFunctionValueWrapper(declaration)) : "",
  ].filter(Boolean).join("\n\n")).join("\n\n");
  const adapters = emitter.emitCallableAdapters();
  const traitAdapters = emitter.emitTraitAdapters();
  const traitSuspensionHelpers = emitter.emitTraitSuspensionHelpers();
  const referenceableFunctions = [
    ...program.closures.map((closure) => `$c${closure.index}`),
    ...program.functions.filter((declaration) => !declaration.suspending && declaration.genericParameters.length === 0).map((declaration) => `$fv${declaration.index}`),
    ...emitter.adapters.map((adapter) => `$adapt${adapter.index}`),
    ...program.implementations.flatMap((implementation) => implementation.methodFunctions.map((method) => `$tadapt${implementation.index}_${method.methodIndex}`)),
    ...program.implementations.flatMap((implementation) => {
      const trait = program.traits[implementation.traitIndex]!;
      return implementation.methodFunctions.flatMap((mapping) => trait.methods[mapping.methodIndex]?.suspending
        ? [
            `$tspolladapt${implementation.index}_${mapping.methodIndex}`,
            `$tscanceladapt${implementation.index}_${mapping.methodIndex}`,
            `$tsresultadapt${implementation.index}_${mapping.methodIndex}`,
          ]
        : []);
    }),
  ];
  const declarations = referenceableFunctions.length > 0
    ? `\n  (elem declare func ${referenceableFunctions.join(" ")})\n`
    : "";
  const imports = [
    program.functions.some((declaration) => declaration.suspending)
      ? `  (import "hd" "trace" (func $hd.trace (param i32 i32)))`
      : "",
    program.functions.some((declaration) => declaration.suspending)
      ? `  (import "hd" "pending" (func $hd.pending (param i32 i32) (result i32)))`
      : "",
    emitter.requiresFloatPower
      ? `  (import "hd" "pow_f64" (func $hd.pow_f64 (param f64 f64) (result f64)))`
      : "",
    emitter.requiresConsoleOutput
      ? `  (import "hd" "console_byte" (func $hd.console_byte (param externref i32)))`
      : "",
  ].filter(Boolean).join("\n");
  return `(module${imports ? "\n" + imports : ""}${dataTypes}${enumSingletons ? "\n" + enumSingletons : ""}\n${RUNTIME_WAT}\n\n${MAP_RUNTIME_WAT}${emitter.requiresConsoleOutput ? "\n\n" + CONSOLE_RUNTIME_WAT : ""}${declarations}\n${functions}${traitSuspensionHelpers ? "\n\n" + indent(traitSuspensionHelpers) : ""}${adapters ? "\n\n" + indent(adapters) : ""}${traitAdapters ? "\n\n" + indent(traitAdapters) : ""}\n)`;
}
