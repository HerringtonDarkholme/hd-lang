import type {
  HirExpression,
  HirFunction,
  HirLocal,
  HirMatchArm,
  HirProgram,
  HirStatement,
  ValueType,
} from "../hir.ts";
import {
  contextKeys,
  functionParts,
  functionType,
  nominalGenericParts,
  optionalInner,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import {
  buildSuspensionPlan,
  needsSuspensionCfg,
  type SuspensionOperation,
  type SuspensionPlan,
  type SuspensionTerminator,
} from "./suspension.ts";

import {
  exportName,
  globalName,
  indent,
  isGenericValueType,
  linearSuspensionSites,
  localName,
  functionName,
  providerWatType,
  suspensionCancel,
  suspensionFrameTypeName,
  suspensionPoll,
  suspensionWrapperCancelAdapterName,
  suspensionWrapperPollAdapterName,
  suspensionWrapperResultAdapterName,
  testExportName,
  traitSuspensionName,
  traitSuspensionResultName,
  type HirSuspendDrive,
  type LinearSuspensionSite,
} from "./shared.ts";

import { FunctionBodyEmitter } from "./function-body.ts";
import { emitHostProviders } from "./host-providers.ts";
import {
  emitStoredSuspensionAdapters,
  STORED_SUSPENSION_RUNTIME,
  STORED_SUSPENSION_TYPES,
  storedSuspensionAdapterReferences,
} from "./stored-suspension.ts";

function suspensionIndex(declaration: HirFunction): number {
  return declaration.suspensionIndex ?? declaration.index;
}

class FunctionEmitter extends FunctionBodyEmitter {
  emit(declaration: HirFunction): string {
    this.currentRequirements = declaration.requirements;
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map(
      (bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`,
    );
    const allParameters = declaration.closure
      ? [
          `(param $env anyref)`,
          parameters,
          ...declaration.requirements.map(
            (requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`,
          ),
        ]
          .filter(Boolean)
          .join(" ")
      : [
          parameters,
          ...boundParameters,
          ...declaration.requirements.map(
            (requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`,
          ),
        ]
          .filter(Boolean)
          .join(" ");
    const result =
      declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const locals = declaration.locals
      .filter((local) => !local.parameter)
      .map((local) => `  (local ${localName(local.index)} ${this.watType(local.type)})`);
    this.temporaryTypes.length = 0;
    this.cleanupFrames.length = 0;
    const body = this.emitBlock(declaration.body, declaration.result);
    const temporaries = this.temporaryTypes.map(
      (type, index) => `  (local $tmp${index} ${this.watType(type)})`,
    );
    const exported =
      !declaration.name.startsWith("$") &&
      !declaration.closure &&
      !declaration.suspending &&
      declaration.parameters.every((parameter) => this.hostSafe(parameter.type)) &&
      this.hostSafe(declaration.result);
    const exportClause = exported ? ` (export ${exportName(declaration.name)})` : "";
    const internalName = declaration.suspending
      ? `$body${suspensionIndex(declaration)}`
      : declaration.closure
        ? `$c${declaration.index}`
        : functionName(suspensionIndex(declaration));
    const signature =
      declaration.closure && !declaration.suspending
        ? ` (type $sig${this.functionSignatures.get(
            functionType(
              declaration.parameters.map((parameter) => parameter.type),
              declaration.result,
              declaration.requirements,
              declaration.variadic,
              declaration.suspending,
            ),
          )})`
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
    const plan = this.suspensionPlans.get(suspensionIndex(declaration));
    if (plan) return this.emitCfgSuspensionSupport(declaration, plan);
    const resumableSites = linearSuspensionSites(declaration);
    if (resumableSites.length > 0)
      return this.emitLinearSuspensionSupport(declaration, resumableSites);
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map(
      (bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`,
    );
    const providerParameters = declaration.requirements.map(
      (requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`,
    );
    const allParameters = [
      ...(declaration.closure ? ["(param $env anyref)"] : []),
      parameters,
      ...boundParameters,
      ...providerParameters,
    ]
      .filter(Boolean)
      .join(" ");
    const constructor = [
      `(func ${functionName(suspensionIndex(declaration))}${allParameters ? " " + allParameters : ""} (result (ref null $s${suspensionIndex(declaration)}))`,
      `  (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 0))`,
      `  (struct.new $s${suspensionIndex(declaration)} (i32.const 0) (i32.const 0)${declaration.closure ? " (local.get $env)" : ""}${declaration.parameters.map((parameter) => ` (local.get ${localName(parameter.index)})`).join("")}${declaration.genericBounds.map((_, index) => ` (local.get $bound${index})`).join("")}${declaration.requirements.map((_, index) => ` (local.get $provider${index})`).join("")}${declaration.result === "void" ? "" : ` ${this.defaultValue(declaration.result)}`})`,
      `)`,
    ].join("\n");
    const result =
      declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const bodyCall = `(call $body${suspensionIndex(declaration)}${declaration.closure || declaration.parameters.length || declaration.genericBounds.length || declaration.requirements.length ? " " : ""}${[
      ...(declaration.closure
        ? [
            `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}env (local.get $frame))`,
          ]
        : []),
      ...declaration.parameters.map(
        (_, index) =>
          `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}a${index} (local.get $frame))`,
      ),
      ...declaration.genericBounds.map(
        (_, index) =>
          `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}b${index} (local.get $frame))`,
      ),
      ...declaration.requirements.map(
        (_, index) =>
          `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}p${index} (local.get $frame))`,
      ),
    ].join(" ")})`;
    const poll = [
      `(func $poll${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)})) (result i32)`,
      `  (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 1))`,
      `  (if (i32.eq (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)) (i32.const 1))`,
      `    (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 4)) ${this.emitRuntimePanic("suspension-reentrant-poll")}))`,
      `  (if (i32.or`,
      `        (i32.eq (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)) (i32.const 2))`,
      `        (i32.eq (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)) (i32.const 3)))`,
      `    (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 5)) ${this.emitRuntimePanic("suspension-invalid-state")}))`,
      `  (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)`,
      `    (i32.add (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)) (i32.const 1)))`,
      `  (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 1))`,
      `  (if (call $hd.pending`,
      `        (i32.const ${suspensionIndex(declaration)})`,
      `        (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)))`,
      `    (then`,
      `      (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 4))`,
      `      (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 6))`,
      `      (return (i32.const 0))))`,
      `  (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 1))`,
      declaration.result === "void"
        ? `  ${bodyCall}`
        : `  (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (local.get $frame) ${bodyCall})`,
      `  (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 2))`,
      `  (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 2))`,
      `  (i32.const 1)`,
      `)`,
    ]
      .filter(Boolean)
      .join("\n");
    const drive = [
      `(func $drive${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)}))${result}`,
      `  (if (global.get $hd.driver-active) (then ${this.emitRuntimePanic("suspension-competing-driver")}))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (block $ready`,
      `    (loop $drive`,
      `      (br_if $ready (i32.eq (call $poll${suspensionIndex(declaration)} (local.get $frame)) (i32.const 1)))`,
      `      (br $drive)))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      declaration.result === "void"
        ? ""
        : `  (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (local.get $frame))`,
      `)`,
    ]
      .filter(Boolean)
      .join("\n");
    const cancel = [
      `(func $cancel${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)}))`,
      `  (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 3))`,
      `  (if (i32.eq (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)) (i32.const 1))`,
      `    (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 4)) ${this.emitRuntimePanic("suspension-reentrant-poll")}))`,
      `  (if (i32.or`,
      `        (i32.eq (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)) (i32.const 0))`,
      `        (i32.eq (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)) (i32.const 4)))`,
      `    (then (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 3))))`,
      `)`,
    ].join("\n");
    const entryExport = declaration.name === "main" ? "main" : testExportName(declaration.name);
    const entryProviderParameters = this.entryProviderParameters(declaration);
    const entryProviderArguments = this.entryProviderArguments(declaration);
    const entry = entryExport
      ? [
          `(func $entry${suspensionIndex(declaration)} (export ${JSON.stringify(entryExport)})${entryProviderParameters.length ? " " + entryProviderParameters.join(" ") : ""}${result}`,
          `  (call $drive${suspensionIndex(declaration)} (call ${functionName(suspensionIndex(declaration))}${entryProviderArguments.length ? " " : ""}${entryProviderArguments.join(" ")}))`,
          `)`,
        ].join("\n")
      : "";
    const developmentDriver =
      declaration.name === "main" ? this.emitSuspensionDevelopmentDriver(declaration) : "";
    return [constructor, poll, drive, cancel, entry, developmentDriver]
      .filter(Boolean)
      .join("\n\n");
  }

  private emitCfgSuspensionSupport(declaration: HirFunction, plan: SuspensionPlan): string {
    this.currentRequirements = declaration.requirements;
    this.temporaryTypes.length = 0;
    this.cleanupFrames.length = 0;
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map(
      (bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`,
    );
    const providerParameters = declaration.requirements.map(
      (requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`,
    );
    const allParameters = [
      ...(declaration.closure ? ["(param $env anyref)"] : []),
      parameters,
      ...boundParameters,
      ...providerParameters,
    ]
      .filter(Boolean)
      .join(" ");
    const storedLocals = [
      ...declaration.locals.filter((local) => !local.parameter),
      ...plan.temporaries,
    ];
    const constructorValues = [
      `(i32.const 0)`,
      `(i32.const 0)`,
      ...(declaration.closure ? ["(local.get $env)"] : []),
      ...declaration.parameters.map((parameter) => `(local.get ${localName(parameter.index)})`),
      ...declaration.genericBounds.map((_, index) => `(local.get $bound${index})`),
      ...declaration.requirements.map((_, index) => `(local.get $provider${index})`),
      ...storedLocals.map((local) => this.defaultValue(local.type)),
      ...plan.sites.map((site) => `(ref.null ${suspensionFrameTypeName(site.drive)})`),
      ...(declaration.result === "void" ? [] : [this.defaultValue(declaration.result)]),
    ];
    const constructor = [
      `(func ${functionName(suspensionIndex(declaration))}${allParameters ? " " + allParameters : ""} (result (ref null $s${suspensionIndex(declaration)}))`,
      `  (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 0))`,
      `  (struct.new $s${suspensionIndex(declaration)} ${constructorValues.join(" ")})`,
      `)`,
    ].join("\n");

    const loadFrame = this.emitSuspensionFrameLoads(declaration, storedLocals);
    const storeFrame = this.emitSuspensionFrameStores(declaration, storedLocals);
    const resumeDispatch = plan.sites.reduceRight((otherwise, site) => {
      const child = `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}child${site.siteIndex} (local.get $frame))`;
      const ready: string[] = [];
      if (site.resultLocal)
        ready.push(
          `(local.set ${localName(site.resultLocal.index)} ${this.emitSuspensionResult(site.drive, child)})`,
        );
      ready.push(`(local.set $pc (i32.const ${site.next}))`);
      return [
        `(if (i32.eq (local.get $resume-state) (i32.const ${5 + site.siteIndex}))`,
        `  (then`,
        `    (if (i32.eqz ${suspensionPoll(site.drive, child)})`,
        `      (then`,
        ...storeFrame.map((line) => `        ${line}`),
        `        (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (local.get $resume-state))`,
        `        (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 6))`,
        `        (return (i32.const 0))))`,
        ...ready.map((line) => `    ${line}`),
        `  )`,
        `  (else`,
        indent(otherwise, 4),
        `  ))`,
      ].join("\n");
    }, `(local.set $pc (i32.const ${plan.entry}))`);

    const blockBodies = plan.blocks.map((block) =>
      [
        `(if (i32.eq (local.get $pc) (i32.const ${block.id}))`,
        `  (then`,
        ...block.operations.map((operation) => indent(this.emitCfgOperation(operation), 4)),
        indent(this.emitCfgTerminator(declaration, block.terminator, storeFrame), 4),
        `  ))`,
      ].join("\n"),
    );
    const pollBody = [
      `(call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 1))`,
      `(local.set $resume-state (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 4)) ${this.emitRuntimePanic("suspension-reentrant-poll")}))`,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 2)) (i32.eq (local.get $resume-state) (i32.const 3)))`,
      `  (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 5)) ${this.emitRuntimePanic("suspension-invalid-state")}))`,
      `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)`,
      `  (i32.add (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)) (i32.const 1)))`,
      `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 1))`,
      `(if (call $hd.pending (i32.const ${suspensionIndex(declaration)}) (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)))`,
      `  (then`,
      `    (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)`,
      `      (if (result i32) (i32.eq (local.get $resume-state) (i32.const 0))`,
      `        (then (i32.const 4))`,
      `        (else (local.get $resume-state))))`,
      `    (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 6))`,
      `    (return (i32.const 0))))`,
      ...loadFrame,
      `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 1))`,
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

    const cancellationBranches = plan.sites.map((site) =>
      [
        `(if (i32.eq (local.get $resume-state) (i32.const ${5 + site.siteIndex}))`,
        `  (then`,
        `    ${suspensionCancel(site.drive, `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}child${site.siteIndex} (local.get $frame))`)}`,
        ...[...site.cleanups]
          .reverse()
          .flatMap((cleanup) => [
            `    (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 7))`,
            indent(this.emitBlock(cleanup, "void"), 4),
          ]),
        `    (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 3))`,
        `    (return)))`,
      ].join("\n"),
    );
    const cancelBody = [
      `(call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 3))`,
      `(local.set $resume-state (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 4)) ${this.emitRuntimePanic("suspension-reentrant-poll")}))`,
      ...loadFrame,
      ...cancellationBranches,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 0)) (i32.eq (local.get $resume-state) (i32.const 4)))`,
      `  (then (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 3))))`,
    ];

    const localDeclarations = [...declaration.locals, ...plan.temporaries].map(
      (local) => `  (local ${localName(local.index)} ${this.watType(local.type)})`,
    );
    const environmentLocals = declaration.closure ? [`  (local $env anyref)`] : [];
    const boundLocals = declaration.genericBounds.map(
      (bound, index) => `  (local $bound${index} (ref null $trait${bound.traitIndex}))`,
    );
    const providerLocals = declaration.requirements.map(
      (requirement, index) => `  (local $provider${index} ${this.providerType(requirement)})`,
    );
    const temporaries = this.temporaryTypes.map(
      (type, index) => `  (local $tmp${index} ${this.watType(type)})`,
    );
    const poll = [
      `(func $poll${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)})) (result i32)`,
      `  (local $resume-state i32)`,
      `  (local $pc i32)`,
      ...environmentLocals,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(pollBody.join("\n")),
      `)`,
    ].join("\n");
    const cancel = [
      `(func $cancel${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)}))`,
      `  (local $resume-state i32)`,
      ...environmentLocals,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(cancelBody.join("\n")),
      `)`,
    ].join("\n");
    const result =
      declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const drive = [
      `(func $drive${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)}))${result}`,
      `  (if (global.get $hd.driver-active) (then ${this.emitRuntimePanic("suspension-competing-driver")}))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (block $ready`,
      `    (loop $drive`,
      `      (br_if $ready (i32.eq (call $poll${suspensionIndex(declaration)} (local.get $frame)) (i32.const 1)))`,
      `      (br $drive)))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      declaration.result === "void"
        ? ""
        : `  (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (local.get $frame))`,
      `)`,
    ]
      .filter(Boolean)
      .join("\n");
    const entryExport = declaration.name === "main" ? "main" : testExportName(declaration.name);
    const entryProviderParameters = this.entryProviderParameters(declaration);
    const entryProviderArguments = this.entryProviderArguments(declaration);
    const entry = entryExport
      ? [
          `(func $entry${suspensionIndex(declaration)} (export ${JSON.stringify(entryExport)})${entryProviderParameters.length ? " " + entryProviderParameters.join(" ") : ""}${result}`,
          `  (call $drive${suspensionIndex(declaration)} (call ${functionName(suspensionIndex(declaration))}${entryProviderArguments.length ? " " : ""}${entryProviderArguments.join(" ")}))`,
          `)`,
        ].join("\n")
      : "";
    const developmentDriver =
      declaration.name === "main" ? this.emitSuspensionDevelopmentDriver(declaration) : "";
    return [constructor, poll, drive, cancel, entry, developmentDriver]
      .filter(Boolean)
      .join("\n\n");
  }

  private emitCfgOperation(operation: SuspensionOperation): string {
    switch (operation.kind) {
      case "assign":
        return `(local.set ${localName(operation.local.index)} ${this.emitExpression(operation.value)})`;
      case "global-assign":
        return `(global.set ${globalName(operation.global.index)} ${this.emitExpression(operation.value)})`;
      case "evaluate": {
        const value = this.emitExpression(operation.value);
        return operation.value.type === "void" || operation.value.type === "never"
          ? value
          : `(drop ${value})`;
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
          else
            entry.providers.forEach((provider) =>
              finalProviders.set(provider.key, localName(provider.local.index)),
            );
        }
        return `(local.set ${localName(operation.local.index)} (struct.new $context${contextIndex} ${operation.keys.map((key) => `(local.get ${finalProviders.get(key)})`).join(" ")}))`;
      }
      case "match-bind":
        return this.emitCfgMatchBindings(
          operation.subject,
          operation.representation,
          operation.enumIndex,
          operation.bindings,
        ).join("\n");
    }
  }

  private emitCfgTerminator(
    declaration: HirFunction,
    terminator: SuspensionTerminator,
    storeFrame: readonly string[],
  ): string {
    const jump = (target: number): string => `(local.set $pc (i32.const ${target}))\n(br $cfg)`;
    switch (terminator.kind) {
      case "jump":
        return jump(terminator.target);
      case "branch":
        return `(if ${this.emitExpression(terminator.condition)}\n  (then (local.set $pc (i32.const ${terminator.thenTarget})))\n  (else (local.set $pc (i32.const ${terminator.elseTarget}))))\n(br $cfg)`;
      case "match-test":
        return `(if ${this.emitCfgMatchCondition(terminator.subject, terminator.representation, terminator.enumIndex, terminator.arm)}\n  (then (local.set $pc (i32.const ${terminator.thenTarget})))\n  (else (local.set $pc (i32.const ${terminator.elseTarget}))))\n(br $cfg)`;
      case "suspend": {
        const child = `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}child${terminator.siteIndex} (local.get $frame))`;
        return [
          `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}child${terminator.siteIndex} (local.get $frame) ${this.emitExpression(terminator.drive.suspension)})`,
          `(if (i32.eqz ${suspensionPoll(terminator.drive, child)})`,
          `  (then`,
          ...storeFrame.map((line) => `    ${line}`),
          `    (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const ${5 + terminator.siteIndex}))`,
          `    (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 6))`,
          `    (return (i32.const 0))))`,
          terminator.resultLocal
            ? `(local.set ${localName(terminator.resultLocal.index)} ${this.emitSuspensionResult(terminator.drive, child)})`
            : "",
          jump(terminator.next),
        ]
          .filter(Boolean)
          .join("\n");
      }
      case "propagate": {
        const operand = this.emitExpression(terminator.operand);
        const success: string[] = [];
        if (terminator.successLocal) {
          success.push(
            `(local.set ${localName(terminator.successLocal.index)} ${this.unboxValue(`(struct.get $hd.variant $hd.variant-payload ${operand})`, terminator.payloadType)})`,
          );
        }
        success.push(`(local.set $pc (i32.const ${terminator.successTarget}))`, `(br $cfg)`);
        const failure = [
          ...(declaration.result === "void"
            ? []
            : [
                `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (local.get $frame) ${operand})`,
              ]),
          ...[...terminator.cleanups].reverse().map((cleanup) => this.emitBlock(cleanup, "void")),
          `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 2))`,
          `(call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 2))`,
          `(return (i32.const 1))`,
        ];
        return `(if (i32.eq (struct.get $hd.variant $hd.variant-tag ${operand}) (i32.const ${terminator.successTag}))\n  (then\n${indent(success.join("\n"), 4)}\n  )\n  (else\n${indent(failure.join("\n"), 4)}\n  ))`;
      }
      case "complete":
        return [
          declaration.result === "void"
            ? terminator.value
              ? this.emitExpression(terminator.value)
              : ""
            : `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (local.get $frame) ${this.emitExpression(terminator.value!)})`,
          `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 2))`,
          `(call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 2))`,
          `(return (i32.const 1))`,
        ]
          .filter(Boolean)
          .join("\n");
      case "unreachable":
        return `(unreachable)`;
    }
  }

  private emitSuspensionResult(drive: HirSuspendDrive, child: string): string {
    if (drive.kind === "suspension-drive")
      return this.unboxValue(`(call $hd.suspension_result ${child})`, drive.type);
    const raw =
      drive.kind === "suspend-drive"
        ? `(struct.get $s${drive.functionIndex} $s${drive.functionIndex}result ${child})`
        : `(call ${traitSuspensionResultName(drive.traitIndex, drive.methodIndex)} ${child})`;
    const erased =
      (drive.kind === "suspend-drive" &&
        drive.erasedResultType &&
        isGenericValueType(drive.erasedResultType)) ||
      (drive.kind === "trait-suspend-drive" &&
        this.traitMethodErasesResult(drive.traitIndex, drive.methodIndex));
    return erased ? this.unboxValue(raw, drive.type) : raw;
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
          ? this.emitEnumPayloadAccess(
              source,
              enumIndex,
              binding.enumFieldIndex,
              binding.enumErasedFieldType,
              binding.enumFieldType!,
              binding.path ?? [],
            )
          : binding.path
            ? this.emitDataPatternAccess(source, binding.path)
            : binding.fieldIndex === -1
              ? `(local.get ${source})`
              : representation === "enum"
                ? binding.erasedFieldType && isGenericValueType(binding.erasedFieldType)
                  ? this.unboxValue(
                      `(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${source}))`,
                      binding.type,
                    )
                  : `(struct.get $e${enumIndex} $e${enumIndex}f${binding.fieldIndex} (local.get ${source}))`
                : representation === "erased-variant"
                  ? this.unboxValue(
                      `(struct.get $hd.variant $hd.variant-payload (local.get ${source}))`,
                      binding.type,
                    )
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
      condition =
        arm.literal.type === "string"
          ? `(i32.eq (call $hd.string_compare (local.get ${source}) ${value}) (i32.const 0))`
          : arm.literal.type === "f64"
            ? `(f64.eq (local.get ${source}) ${value})`
            : `(i32.eq (local.get ${source}) ${value})`;
    } else if (arm.tag !== undefined) {
      const actual =
        representation === "enum"
          ? `(struct.get $e${enumIndex} $e${enumIndex}tag (local.get ${source}))`
          : `(struct.get $hd.variant $hd.variant-tag (local.get ${source}))`;
      condition = `(i32.eq ${actual} (i32.const ${arm.tag}))`;
    }
    for (const test of arm.tests ?? []) {
      const actual = this.emitMatchTestAccess(source, enumIndex, test);
      const expected =
        test.tag !== undefined ? `(i32.const ${test.tag})` : this.emitExpression(test.literal!);
      const next =
        test.tag !== undefined
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

  private emitLinearSuspensionSupport(
    declaration: HirFunction,
    sites: readonly LinearSuspensionSite[],
  ): string {
    this.currentRequirements = declaration.requirements;
    this.temporaryTypes.length = 0;
    this.cleanupFrames.length = 0;
    const parameters = declaration.parameters
      .map((parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`)
      .join(" ");
    const boundParameters = declaration.genericBounds.map(
      (bound, index) => `(param $bound${index} (ref null $trait${bound.traitIndex}))`,
    );
    const providerParameters = declaration.requirements.map(
      (requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`,
    );
    const allParameters = [
      ...(declaration.closure ? ["(param $env anyref)"] : []),
      parameters,
      ...boundParameters,
      ...providerParameters,
    ]
      .filter(Boolean)
      .join(" ");
    const storedLocals = declaration.locals.filter((local) => !local.parameter);
    const constructorValues = [
      `(i32.const 0)`,
      `(i32.const 0)`,
      ...(declaration.closure ? ["(local.get $env)"] : []),
      ...declaration.parameters.map((parameter) => `(local.get ${localName(parameter.index)})`),
      ...declaration.genericBounds.map((_, index) => `(local.get $bound${index})`),
      ...declaration.requirements.map((_, index) => `(local.get $provider${index})`),
      ...storedLocals.map((local) => this.defaultValue(local.type)),
      ...sites.map((site) => `(ref.null ${suspensionFrameTypeName(site.drive)})`),
      ...(declaration.result === "void" ? [] : [this.defaultValue(declaration.result)]),
    ];
    const constructor = [
      `(func ${functionName(suspensionIndex(declaration))}${allParameters ? " " + allParameters : ""} (result (ref null $s${suspensionIndex(declaration)}))`,
      `  (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 0))`,
      `  (struct.new $s${suspensionIndex(declaration)} ${constructorValues.join(" ")})`,
      `)`,
    ].join("\n");

    const cold = this.emitLinearContinuation(declaration, sites, 0, []);
    const resumed = sites.reduceRight(
      (otherwise, site) =>
        [
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
      `(call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 1))`,
      `(local.set $resume-state (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 4)) ${this.emitRuntimePanic("suspension-reentrant-poll")}))`,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 2)) (i32.eq (local.get $resume-state) (i32.const 3)))`,
      `  (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 5)) ${this.emitRuntimePanic("suspension-invalid-state")}))`,
      `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)`,
      `  (i32.add (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)) (i32.const 1)))`,
      `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 1))`,
      `(if (call $hd.pending (i32.const ${suspensionIndex(declaration)}) (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}polls (local.get $frame)))`,
      `  (then`,
      `    (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)`,
      `      (if (result i32) (i32.eq (local.get $resume-state) (i32.const 0))`,
      `        (then (i32.const 4))`,
      `        (else (local.get $resume-state))))`,
      `    (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 6))`,
      `    (return (i32.const 0))))`,
      ...loadFrame,
      `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 1))`,
      `(if (i32.le_u (local.get $resume-state) (i32.const 4))`,
      `  (then`,
      indent(cold, 4),
      `  )`,
      `  (else`,
      indent(resumed, 4),
      `  ))`,
    ];

    const cancellationBranches = sites.map((site) =>
      [
        `(if (i32.eq (local.get $resume-state) (i32.const ${5 + site.index}))`,
        `  (then`,
        `    ${suspensionCancel(site.drive, `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}child${site.index} (local.get $frame))`)}`,
        ...[...site.cleanups]
          .reverse()
          .flatMap((cleanup) => [
            `    (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 7))`,
            indent(this.emitBlock(cleanup, "void"), 4),
          ]),
        `    (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 3))`,
        `    (return)))`,
      ].join("\n"),
    );
    const cancelBody = [
      `(call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 3))`,
      `(local.set $resume-state (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame)))`,
      `(if (i32.eq (local.get $resume-state) (i32.const 1))`,
      `  (then (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 4)) ${this.emitRuntimePanic("suspension-reentrant-poll")}))`,
      ...loadFrame,
      ...cancellationBranches,
      `(if (i32.or (i32.eq (local.get $resume-state) (i32.const 0)) (i32.eq (local.get $resume-state) (i32.const 4)))`,
      `  (then (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 3))))`,
    ];

    const localDeclarations = declaration.locals.map(
      (local) => `  (local ${localName(local.index)} ${this.watType(local.type)})`,
    );
    const environmentLocals = declaration.closure ? [`  (local $env anyref)`] : [];
    const boundLocals = declaration.genericBounds.map(
      (bound, index) => `  (local $bound${index} (ref null $trait${bound.traitIndex}))`,
    );
    const providerLocals = declaration.requirements.map(
      (requirement, index) => `  (local $provider${index} ${this.providerType(requirement)})`,
    );
    const temporaries = this.temporaryTypes.map(
      (type, index) => `  (local $tmp${index} ${this.watType(type)})`,
    );
    const poll = [
      `(func $poll${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)})) (result i32)`,
      `  (local $resume-state i32)`,
      ...environmentLocals,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(pollBody.join("\n")),
      `)`,
    ].join("\n");
    const cancel = [
      `(func $cancel${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)}))`,
      `  (local $resume-state i32)`,
      ...environmentLocals,
      ...localDeclarations,
      ...boundLocals,
      ...providerLocals,
      ...temporaries,
      indent(cancelBody.join("\n")),
      `)`,
    ].join("\n");
    const result =
      declaration.result === "void" ? "" : ` (result ${this.watType(declaration.result)})`;
    const drive = [
      `(func $drive${suspensionIndex(declaration)} (param $frame (ref null $s${suspensionIndex(declaration)}))${result}`,
      `  (if (global.get $hd.driver-active) (then ${this.emitRuntimePanic("suspension-competing-driver")}))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (block $ready`,
      `    (loop $drive`,
      `      (br_if $ready (i32.eq (call $poll${suspensionIndex(declaration)} (local.get $frame)) (i32.const 1)))`,
      `      (br $drive)))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      declaration.result === "void"
        ? ""
        : `  (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (local.get $frame))`,
      `)`,
    ]
      .filter(Boolean)
      .join("\n");
    const entryExport = declaration.name === "main" ? "main" : testExportName(declaration.name);
    const entryProviderParameters = this.entryProviderParameters(declaration);
    const entryProviderArguments = this.entryProviderArguments(declaration);
    const entry = entryExport
      ? [
          `(func $entry${suspensionIndex(declaration)} (export ${JSON.stringify(entryExport)})${entryProviderParameters.length ? " " + entryProviderParameters.join(" ") : ""}${result}`,
          `  (call $drive${suspensionIndex(declaration)} (call ${functionName(suspensionIndex(declaration))}${entryProviderArguments.length ? " " : ""}${entryProviderArguments.join(" ")}))`,
          `)`,
        ].join("\n")
      : "";
    const developmentDriver =
      declaration.name === "main" ? this.emitSuspensionDevelopmentDriver(declaration) : "";
    return [constructor, poll, drive, cancel, entry, developmentDriver]
      .filter(Boolean)
      .join("\n\n");
  }

  private emitSuspensionDevelopmentDriver(declaration: HirFunction): string {
    if (declaration.parameters.length > 0) return "";
    const frame = `$hd.dev-frame${suspensionIndex(declaration)}`;
    const providerParameters = this.entryProviderParameters(declaration);
    const providerArguments = this.entryProviderArguments(declaration);
    const start = [
      `(global ${frame} (mut (ref null $s${suspensionIndex(declaration)})) (ref.null $s${suspensionIndex(declaration)}))`,
      `(func (export "__hd_start")${providerParameters.length ? " " + providerParameters.join(" ") : ""}`,
      `  (if (global.get $hd.driver-active) (then ${this.emitRuntimePanic("suspension-competing-driver")}))`,
      `  (global.set $hd.driver-active (i32.const 1))`,
      `  (global.set ${frame} (call ${functionName(suspensionIndex(declaration))}${providerArguments.length ? " " : ""}${providerArguments.join(" ")}))`,
      `)`,
    ].join("\n");
    const poll = [
      `(func (export "__hd_poll") (result i32)`,
      `  (local $ready i32)`,
      `  (local.set $ready (call $poll${suspensionIndex(declaration)} (global.get ${frame})))`,
      `  (if (local.get $ready) (then (global.set $hd.driver-active (i32.const 0))))`,
      `  (local.get $ready)`,
      `)`,
    ].join("\n");
    const cancel = [
      `(func (export "__hd_cancel")`,
      `  (call $cancel${suspensionIndex(declaration)} (global.get ${frame}))`,
      `  (global.set $hd.driver-active (i32.const 0))`,
      `)`,
    ].join("\n");
    const result =
      declaration.result === "void"
        ? ""
        : `(func (export "__hd_result") (result ${this.watType(declaration.result)}) (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (global.get ${frame})))`;
    return [start, poll, cancel, result].filter(Boolean).join("\n");
  }

  private emitSuspensionFrameLoads(
    declaration: HirFunction,
    storedLocals: readonly HirLocal[],
  ): string[] {
    return [
      ...(declaration.closure
        ? [
            `(local.set $env (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}env (local.get $frame)))`,
          ]
        : []),
      ...declaration.parameters.map(
        (parameter, index) =>
          `(local.set ${localName(parameter.index)} (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}a${index} (local.get $frame)))`,
      ),
      ...declaration.genericBounds.map(
        (_, index) =>
          `(local.set $bound${index} (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}b${index} (local.get $frame)))`,
      ),
      ...declaration.requirements.map(
        (_, index) =>
          `(local.set $provider${index} (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}p${index} (local.get $frame)))`,
      ),
      ...storedLocals.map(
        (local) =>
          `(local.set ${localName(local.index)} (struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}l${local.index} (local.get $frame)))`,
      ),
    ];
  }

  protected emitLinearContinuation(
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
    const child = `(struct.get $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}child${site.index} (local.get $frame))`;
    const lines: string[] = [];
    if (start) {
      lines.push(
        `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}child${site.index} (local.get $frame) ${this.emitExpression(site.drive.suspension)})`,
      );
    }
    lines.push(
      `(if (i32.eqz ${suspensionPoll(site.drive, child)})`,
      `  (then`,
      ...this.emitSuspensionFrameStores(declaration).map((line) => `    ${line}`),
      `    (struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const ${5 + site.index}))`,
      `    (call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 6))`,
      `    (return (i32.const 0))))`,
    );
    const value =
      site.drive.type === "void" ? undefined : this.emitSuspensionResult(site.drive, child);
    const final = site.statementIndex === declaration.body.length - 1;
    if (site.statement.kind === "binding" || site.statement.kind === "assignment") {
      lines.push(`(local.set ${localName(site.statement.local.index)} ${value})`);
    } else if (
      site.statement.kind === "global-binding" ||
      site.statement.kind === "global-assignment"
    ) {
      lines.push(`(global.set ${globalName(site.statement.global.index)} ${value})`);
    } else if (
      site.statement.kind === "discard" ||
      (site.statement.kind === "expression" && !final)
    ) {
      if (value) lines.push(`(drop ${value})`);
    } else if (
      site.statement.kind === "return" ||
      (site.statement.kind === "expression" && final)
    ) {
      lines.push(this.emitSuspensionCompletionWat(declaration, value, cleanups));
      return lines.join("\n");
    }
    lines.push(this.emitLinearContinuation(declaration, sites, site.statementIndex + 1, cleanups));
    return lines.join("\n");
  }

  protected emitSuspensionFrameStores(
    declaration: HirFunction,
    storedLocals: readonly HirLocal[] = declaration.locals.filter((local) => !local.parameter),
  ): string[] {
    return storedLocals.map(
      (local) =>
        `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}l${local.index} (local.get $frame) (local.get ${localName(local.index)}))`,
    );
  }

  protected emitSuspensionCompletion(
    declaration: HirFunction,
    value: HirExpression | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): string {
    return this.emitSuspensionCompletionWat(
      declaration,
      value ? this.emitExpression(value) : undefined,
      cleanups,
    );
  }

  protected emitSuspensionCompletionWat(
    declaration: HirFunction,
    value: string | undefined,
    cleanups: readonly (readonly HirStatement[])[],
  ): string {
    return [
      declaration.result === "void"
        ? (value ?? "")
        : `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}result (local.get $frame) ${value})`,
      ...[...cleanups].reverse().map((cleanup) => this.emitBlock(cleanup, "void")),
      `(struct.set $s${suspensionIndex(declaration)} $s${suspensionIndex(declaration)}state (local.get $frame) (i32.const 2))`,
      `(call $hd.trace (i32.const ${suspensionIndex(declaration)}) (i32.const 2))`,
      `(return (i32.const 1))`,
    ]
      .filter(Boolean)
      .join("\n");
  }

  emitFunctionValueWrapper(declaration: HirFunction): string {
    const type = functionType(
      declaration.parameters.map((parameter) => parameter.type),
      declaration.result,
      declaration.requirements,
      declaration.variadic,
      declaration.suspending,
    );
    const signature = this.functionSignatures.get(type);
    const parameters = declaration.parameters.map(
      (parameter) => `(param ${localName(parameter.index)} ${this.watType(parameter.type)})`,
    );
    const providers = declaration.requirements.map(
      (requirement, index) => `(param $provider${index} ${this.providerType(requirement)})`,
    );
    const result = declaration.suspending
      ? ` (result (ref null $hd.suspension))`
      : declaration.result === "void"
        ? ""
        : ` (result ${this.watType(declaration.result)})`;
    const arguments_ = [
      ...(declaration.closure ? ["(local.get $env)"] : []),
      ...declaration.parameters.map((parameter) => `(local.get ${localName(parameter.index)})`),
      ...declaration.requirements.map((_, index) => `(local.get $provider${index})`),
    ];
    const call = `(call ${functionName(suspensionIndex(declaration))}${arguments_.length ? " " : ""}${arguments_.join(" ")})`;
    const value = declaration.suspending
      ? `(struct.new $hd.suspension ${call} (ref.func ${suspensionWrapperPollAdapterName(suspensionIndex(declaration))}) (ref.func ${suspensionWrapperCancelAdapterName(suspensionIndex(declaration))}) (ref.func ${suspensionWrapperResultAdapterName(suspensionIndex(declaration))}))`
      : call;
    const wrapperName = declaration.closure ? `$c${declaration.index}` : `$fv${declaration.index}`;
    return [
      `(func ${wrapperName} (type $sig${signature}) (param $env anyref) ${[...parameters, ...providers].join(" ")}${result}`,
      `  ${value}`,
      `)`,
    ].join("\n");
  }
}

import {
  CONSOLE_RUNTIME_WAT,
  FLOAT_RUNTIME_WAT,
  MAP_RUNTIME_WAT,
  RUNTIME_WAT,
  STRING_SPLIT_RUNTIME_WAT,
  STRING_TRANSFORM_RUNTIME_WAT,
} from "./runtime/index.ts";

interface CollectedModuleTypes {
  readonly signatureNames: Map<ValueType, number>;
  readonly contextNames: Map<ValueType, number>;
}

function collectModuleTypes(program: HirProgram): CollectedModuleTypes {
  const signatureNames = new Map<ValueType, number>();
  const contextNames = new Map<ValueType, number>();
  const collectType = (type: ValueType): void => {
    if (type.startsWith("trait:")) return;
    const tuple = tupleParts(type);
    if (tuple !== undefined) {
      tuple.forEach(collectType);
      return;
    }
    const nominal = nominalGenericParts(type);
    if (storedSuspensionParts(type)) {
      collectType(storedSuspensionParts(type)!.result);
      return;
    }
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
    if (declaration.closure || declaration.genericParameters.length === 0) {
      collectType(
        functionType(
          declaration.parameters.map((parameter) => parameter.type),
          declaration.result,
          declaration.requirements,
          declaration.variadic,
          declaration.suspending,
        ),
      );
    }
  }
  program.globals.forEach((global) => collectType(global.type));
  program.data.forEach((declaration) =>
    declaration.fields.forEach((field) => collectType(field.type)),
  );
  program.enums.forEach((declaration) =>
    declaration.fields.forEach((field) => collectType(field.type)),
  );
  program.traits.forEach((trait) =>
    trait.methods.forEach((method) => {
      method.parameters.forEach(collectType);
      collectType(method.result);
    }),
  );
  return { signatureNames, contextNames };
}

export function emitWat(program: HirProgram): string {
  const { signatureNames, contextNames } = collectModuleTypes(program);
  const traitsByName = new Map(program.traits.map((trait) => [trait.name, trait]));
  const suspensionPlans = new Map(
    [...program.functions, ...program.closures]
      .filter((declaration) => declaration.suspending && needsSuspensionCfg(declaration))
      .map(
        (declaration) => [suspensionIndex(declaration), buildSuspensionPlan(declaration)] as const,
      ),
  );
  const emitter = new FunctionEmitter(
    program.data,
    program.enums,
    signatureNames,
    contextNames,
    program.closures,
    program.traits,
    program.implementations,
    suspensionPlans,
    program.hostCapabilities,
  );
  const hostProviders = emitHostProviders(program);
  const signatureTypes = [...signatureNames]
    .map(([type, index]) => {
      const callable = functionParts(type)!;
      const parameters = [
        `(param anyref)`,
        ...callable.parameters.map((parameter) => `(param ${emitter.watType(parameter)})`),
        ...callable.requirements.map(
          (requirement) => `(param ${providerWatType(requirement, traitsByName)})`,
        ),
      ].join(" ");
      const result = callable.suspending
        ? ` (result (ref null $hd.suspension))`
        : callable.result === "void"
          ? ""
          : ` (result ${emitter.watType(callable.result)})`;
      return `    (type $sig${index} (func${parameters ? " " + parameters : ""}${result}))`;
    })
    .join("\n");
  const traitMethodTypes = program.traits
    .flatMap((trait) =>
      trait.methods.map((method) => {
        const parameters = [
          `(param anyref)`,
          `(param anyref)`,
          ...method.parameters.map((parameter) => `(param ${emitter.watType(parameter)})`),
          ...method.requirements.map(
            (requirement) => `(param ${providerWatType(requirement, traitsByName)})`,
          ),
        ].join(" ");
        const result = method.suspending
          ? ` (result (ref null ${traitSuspensionName(trait.index, method.index)}))`
          : method.result === "void"
            ? ""
            : ` (result ${emitter.watType(method.result)})`;
        return `    (type $tsig${trait.index}_${method.index} (func ${parameters}${result}))`;
      }),
    )
    .join("\n");
  const traitSuspensionTypes = program.traits
    .flatMap((trait) =>
      trait.methods.flatMap((method) => {
        if (!method.suspending) return [];
        const result =
          method.result === "void" ? "" : ` (result ${emitter.watType(method.result)})`;
        const wrapper = traitSuspensionName(trait.index, method.index);
        return [
          `    (type $tspollsig${trait.index}_${method.index} (func (param anyref) (result i32)))`,
          `    (type $tscancelsig${trait.index}_${method.index} (func (param anyref)))`,
          `    (type $tsresultsig${trait.index}_${method.index} (func (param anyref)${result}))`,
          `    (type ${wrapper} (struct\n      (field ${wrapper}inner anyref)\n      (field ${wrapper}poll (ref $tspollsig${trait.index}_${method.index}))\n      (field ${wrapper}cancel (ref $tscancelsig${trait.index}_${method.index}))\n      (field ${wrapper}result (ref $tsresultsig${trait.index}_${method.index}))))`,
        ];
      }),
    )
    .join("\n");
  const dataTypes = `\n  (rec\n${signatureTypes ? signatureTypes + "\n" : ""}${traitMethodTypes ? traitMethodTypes + "\n" : ""}${traitSuspensionTypes ? traitSuspensionTypes + "\n" : ""}${STORED_SUSPENSION_TYPES}\n    (type $hd.bytes (array (mut i8)))
    (type $hd.list (array (mut anyref)))
    (type $hd.vector (struct
      (field $hd.vector-size (mut i32))
      (field $hd.vector-values (mut (ref $hd.list)))
      (field $hd.vector-version (mut i32))))
    (type $hd.iterator (struct
      (field $hd.iterator-list (ref null $hd.vector))
      (field $hd.iterator-map (ref null $hd.map))
      (field $hd.iterator-index (mut i32))
      (field $hd.iterator-version i32)))
    (type $hd.map (struct
      (field $hd.map-key-kind i32)
      (field $hd.map-size (mut i32))
      (field $hd.map-keys (mut (ref $hd.list)))
      (field $hd.map-values (mut (ref $hd.list)))
      (field $hd.map-version (mut i32))))
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
${hostProviders.types ? hostProviders.types + "\n" : ""}    (type $hd.variant (struct
      (field $hd.variant-tag i32)
      (field $hd.variant-payload (mut anyref))))
${[...signatureNames]
  .map(
    ([, index]) => `    (type $closure${index} (struct
      (field $closure${index}fn (ref $sig${index}))
      (field $closure${index}env anyref)))`,
  )
  .join("\n")}
${[...contextNames]
  .map(
    ([type, index]) =>
      `    (type $context${index} (struct\n${contextKeys(type)!
        .map(
          (key, fieldIndex) =>
            `      (field $context${index}f${fieldIndex} ${providerWatType(key, traitsByName)})`,
        )
        .join("\n")}))`,
  )
  .join("\n")}
${program.traits
  .map(
    (trait) => `    (type $trait${trait.index} (struct
      (field $trait${trait.index}value anyref)
      (field $trait${trait.index}bounds (ref null $hd.list))${trait.methods.map((method) => `\n      (field $trait${trait.index}m${method.index} (ref $tsig${trait.index}_${method.index}))`).join("")}${trait.supertraits.map((supertrait, index) => `\n      (field $trait${trait.index}s${index} (ref null $trait${supertrait.traitIndex}))`).join("")}))`,
  )
  .join("\n")}
${[...program.functions, ...program.closures]
  .filter((declaration) => declaration.suspending)
  .map((declaration) => {
    const plan = suspensionPlans.get(suspensionIndex(declaration));
    const sites = plan?.sites ?? linearSuspensionSites(declaration);
    const storedLocals =
      sites.length > 0
        ? [...declaration.locals.filter((local) => !local.parameter), ...(plan?.temporaries ?? [])]
        : [];
    return `    (type $s${suspensionIndex(declaration)} (struct
      (field $s${suspensionIndex(declaration)}state (mut i32))
      (field $s${suspensionIndex(declaration)}polls (mut i32))${declaration.closure ? `\n      (field $s${suspensionIndex(declaration)}env anyref)` : ""}${declaration.parameters.map((parameter, index) => `\n      (field $s${suspensionIndex(declaration)}a${index} ${emitter.watType(parameter.type)})`).join("")}${declaration.genericBounds.map((bound, index) => `\n      (field $s${suspensionIndex(declaration)}b${index} (ref null $trait${bound.traitIndex}))`).join("")}${declaration.requirements.map((requirement, index) => `\n      (field $s${suspensionIndex(declaration)}p${index} ${providerWatType(requirement, traitsByName)})`).join("")}${storedLocals.map((local) => `\n      (field $s${suspensionIndex(declaration)}l${local.index} (mut ${emitter.watType(local.type)}))`).join("")}${sites.map((site) => `\n      (field $s${suspensionIndex(declaration)}child${"siteIndex" in site ? site.siteIndex : site.index} (mut (ref null ${suspensionFrameTypeName(site.drive)})))`).join("")}${declaration.result === "void" ? "" : `\n      (field $s${suspensionIndex(declaration)}result (mut ${emitter.watType(declaration.result)}))`}))`;
  })
  .join("\n")}
${program.closures.map((closure) => `    (type $env${closure.index} (struct${closure.captures.length ? "\n" + closure.captures.map((capture) => `      (field $env${closure.index}f${capture.fieldIndex} ${emitter.watType(capture.source.type)})`).join("\n") : ""}))`).join("\n")}\n${program.data
    .map((declaration) => {
      const fields = declaration.fields
        .map(
          (field) =>
            `      (field $d${declaration.index}f${field.index} (mut ${emitter.watType(field.type)}))`,
        )
        .join("\n");
      return `    (type $d${declaration.index} (struct\n${fields}))`;
    })
    .join("\n")}\n${program.enums
    .map((declaration) => {
      const fields = declaration.fields
        .map(
          (field) =>
            `      (field $e${declaration.index}f${field.index} (mut ${emitter.watType(field.type)}))`,
        )
        .join("\n");
      return `    (type $e${declaration.index} (struct\n      (field $e${declaration.index}tag i32)${fields ? "\n" + fields : ""}))`;
    })
    .join(
      "\n",
    )}\n    (type $hd.runtime (struct\n      (field $status (mut i32))\n      (field $scratch (mut (ref null $hd.bytes)))))\n  )\n`;
  const enumSingletons = program.enums
    .flatMap((declaration) =>
      declaration.variants
        .filter((variant) => declaration.sharedFields.length === 0 && variant.fields.length === 0)
        .map(
          (variant) =>
            `  (global $e${declaration.index}v${variant.tag} (ref $e${declaration.index})\n    (struct.new $e${declaration.index} (i32.const ${variant.tag})${declaration.fields.map((field) => ` ${emitter.defaultValue(field.type)}`).join("")}))`,
        ),
    )
    .join("\n");
  const globals = program.globals
    .map(
      (global) =>
        `  (global ${globalName(global.index)} (mut ${emitter.watType(global.type)}) ${emitter.defaultValue(global.type)})`,
    )
    .join("\n");
  const functions = [...program.functions, ...program.closures]
    .map((declaration) =>
      [
        declaration.suspending &&
        (suspensionPlans.has(suspensionIndex(declaration)) ||
          linearSuspensionSites(declaration).length > 0)
          ? ""
          : indent(emitter.emit(declaration)),
        declaration.suspending ? indent(emitter.emitSuspensionSupport(declaration)) : "",
        (!declaration.closure && declaration.genericParameters.length === 0) ||
        (declaration.closure && declaration.suspending)
          ? indent(emitter.emitFunctionValueWrapper(declaration))
          : "",
      ]
        .filter(Boolean)
        .join("\n\n"),
    )
    .join("\n\n");
  const adapters = emitter.emitCallableAdapters();
  const traitAdapters = [emitter.emitTraitAdapters(), emitter.emitBuiltinTraitAdapters()]
    .filter(Boolean)
    .join("\n\n");
  const traitSuspensionHelpers = emitter.emitTraitSuspensionHelpers();
  const storedSuspensionAdapters = emitStoredSuspensionAdapters(program);
  const referenceableFunctions = [
    ...program.closures.map((closure) => `$c${closure.index}`),
    ...program.functions
      .filter((declaration) => declaration.genericParameters.length === 0)
      .map((declaration) => `$fv${suspensionIndex(declaration)}`),
    ...emitter.adapters.map((adapter) => `$adapt${adapter.index}`),
    ...emitter.builtinTraitAdapterNames,
    ...program.implementations.flatMap((implementation) =>
      implementation.methodFunctions.map(
        (method) => `$tadapt${implementation.index}_${method.methodIndex}`,
      ),
    ),
    ...program.implementations.flatMap((implementation) => {
      const trait = program.traits[implementation.traitIndex]!;
      return implementation.methodFunctions.flatMap((mapping) =>
        trait.methods[mapping.methodIndex]?.suspending
          ? [
              `$tspolladapt${implementation.index}_${mapping.methodIndex}`,
              `$tscanceladapt${implementation.index}_${mapping.methodIndex}`,
              `$tsresultadapt${implementation.index}_${mapping.methodIndex}`,
            ]
          : [],
      );
    }),
    ...hostProviders.references,
    ...storedSuspensionAdapterReferences(program),
  ];
  const declarations =
    referenceableFunctions.length > 0
      ? `\n  (elem declare func ${referenceableFunctions.join(" ")})\n`
      : "";
  const imports = [
    hostProviders.imports,
    [...program.functions, ...program.closures].some((declaration) => declaration.suspending)
      ? `  (import "hd" "trace" (func $hd.trace (param i32 i32)))`
      : "",
    [...program.functions, ...program.closures].some((declaration) => declaration.suspending)
      ? `  (import "hd" "pending" (func $hd.pending (param i32 i32) (result i32)))`
      : "",
    emitter.requiresFloatPower
      ? `  (import "hd" "pow_f64" (func $hd.pow_f64 (param f64 f64) (result f64)))`
      : "",
    emitter.requiresFloatDisplay
      ? `  (import "hd" "format_f64" (func $hd.format_f64 (param f64 i32) (result i32)))`
      : "",
    emitter.requiresStringTransforms
      ? [
          `  (import "hd" "string_transform_begin" (func $hd.string_transform_begin (param i32)))`,
          `  (import "hd" "string_transform_input" (func $hd.string_transform_input (param i32)))`,
          `  (import "hd" "string_transform_output" (func $hd.string_transform_output (param i32) (result i32)))`,
        ].join("\n")
      : "",
    `  (import "hd" "panic" (func $hd.panic (param i32)))`,
    emitter.requiresConsoleOutput
      ? `  (import "hd" "console_byte" (func $hd.console_byte (param externref i32)))`
      : "",
  ]
    .filter(Boolean)
    .join("\n");
  const start = program.initializer === undefined ? "" : `\n  (start $f${program.initializer})`;
  const optionalRuntime = [
    emitter.requiresFloatDisplay ? FLOAT_RUNTIME_WAT : "",
    emitter.requiresConsoleOutput ? CONSOLE_RUNTIME_WAT : "",
    emitter.requiresStringSplit ? STRING_SPLIT_RUNTIME_WAT : "",
    emitter.requiresStringTransforms ? STRING_TRANSFORM_RUNTIME_WAT : "",
  ]
    .filter(Boolean)
    .map((runtime) => `\n\n${runtime}`)
    .join("");
  return `(module${imports ? "\n" + imports : ""}${dataTypes}${enumSingletons ? "\n" + enumSingletons : ""}${globals ? "\n" + globals : ""}\n${RUNTIME_WAT}\n\n${STORED_SUSPENSION_RUNTIME}\n\n${MAP_RUNTIME_WAT}${optionalRuntime}${declarations}\n${functions}${traitSuspensionHelpers ? "\n\n" + indent(traitSuspensionHelpers) : ""}${storedSuspensionAdapters ? "\n\n" + indent(storedSuspensionAdapters) : ""}${adapters ? "\n\n" + indent(adapters) : ""}${traitAdapters ? "\n\n" + indent(traitAdapters) : ""}${hostProviders.functions ? "\n\n" + indent(hostProviders.functions) : ""}${start}\n)`;
}
