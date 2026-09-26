import { readFile, writeFile } from "node:fs/promises";
import { basename, extname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  analyze,
  compile,
  instantiate,
  type CompileOptions,
  type HostSuspensionCall,
  type HostSuspensionOutcome,
  type ReplayEvent,
} from "./compiler.ts";
import { DiagnosticError, formatDiagnostic } from "./diagnostics.ts";
import { RuntimePanicError } from "./runtime-panic.ts";
import { parse } from "./parser/index.ts";
import { explainRequirements } from "./requirements.ts";
import { runRepl } from "./repl.ts";
import { resultParts } from "./types.ts";

type RuntimeScenario = "cancellation-cleanup" | "competing-drivers" | "reentrant-poll";
type RuntimeProfileName =
  | "pending-gate"
  | "ready-counter"
  | "ready-float"
  | "ready-gate"
  | "ready-text";

interface RuntimeProfile {
  readonly hostCapabilities: readonly string[];
  readonly invoke?: (call: HostSuspensionCall) => HostSuspensionOutcome;
  readonly pending?: (call: HostSuspensionCall) => boolean;
}

function pendingGate(call: HostSuspensionCall): boolean {
  return call.providerKey === "Gate" && call.methodName === "wait";
}

function invokeCounter(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "Counter" || call.methodName !== "add")
    throw new Error(`ready-counter cannot invoke ${call.providerKey}.${call.methodName}`);
  return { pending: false, value: Number(call.arguments[0]) + Number(call.arguments[1]) };
}

function invokeFloat(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "FloatCell" || call.methodName !== "sample")
    throw new Error(`ready-float cannot invoke ${call.providerKey}.${call.methodName}`);
  return { pending: false, value: call.arguments[0]! };
}

function invokeText(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "TextBridge" || call.methodName !== "join")
    throw new Error(`ready-text cannot invoke ${call.providerKey}.${call.methodName}`);
  return { pending: false, value: `${call.arguments[0]}${call.arguments[1]}` };
}

const RUNTIME_PROFILES: Readonly<Record<RuntimeProfileName, RuntimeProfile>> = {
  "pending-gate": { hostCapabilities: ["Gate"], pending: pendingGate },
  "ready-counter": { hostCapabilities: ["Counter"], invoke: invokeCounter },
  "ready-float": { hostCapabilities: ["FloatCell"], invoke: invokeFloat },
  "ready-gate": { hostCapabilities: ["Gate"] },
  "ready-text": { hostCapabilities: ["TextBridge"], invoke: invokeText },
};

function runtimeScenario(value: string | undefined): RuntimeScenario {
  if (
    value === "cancellation-cleanup" ||
    value === "competing-drivers" ||
    value === "reentrant-poll"
  )
    return value;
  return usage();
}

function runtimeProfile(value: string | undefined): RuntimeProfileName {
  if (
    value === "pending-gate" ||
    value === "ready-counter" ||
    value === "ready-float" ||
    value === "ready-gate" ||
    value === "ready-text"
  )
    return value;
  return usage();
}

function exportedFunction(instance: WebAssembly.Instance, name: string): CallableFunction {
  const value = instance.exports[name];
  if (typeof value !== "function") throw new Error(`program has no ${name} runtime export`);
  return value;
}

function runRuntimeScenario(
  scenario: RuntimeScenario,
  instance: WebAssembly.Instance,
  providers: readonly unknown[],
): void {
  const start = exportedFunction(instance, "__hd_start");
  const poll = exportedFunction(instance, "__hd_poll");
  start(...providers);
  if (scenario === "cancellation-cleanup") {
    if (poll() !== 0) throw new Error("cancellation-cleanup scenario did not suspend");
    exportedFunction(instance, "__hd_cancel")();
    if (exportedFunction(instance, "cleanup_ran")() !== 1)
      throw new Error("cancellation-cleanup scenario did not run cleanup");
    return;
  }
  if (scenario === "competing-drivers") {
    if (poll() !== 0) throw new Error("competing-drivers scenario did not suspend");
    exportedFunction(instance, "main")();
  } else {
    poll();
  }
  throw new Error(`${scenario} scenario completed without a runtime panic`);
}

function usage(): never {
  console.error(
    "usage: hd <parse|check|test|run|trace|record|replay|build|dump-hir|explain-requirements> [--wat] [--entry NAME] [--profile NAME] [--scenario NAME] [--pending-function NAME] FILE\n       hd repl",
  );
  process.exit(2);
}

export async function main(args = process.argv.slice(2)): Promise<number> {
  if (args[0] === "repl") {
    if (args.length > 1) usage();
    return runRepl();
  }
  const command = args.shift();
  let wat = false;
  let entryName = "main";
  let explicitEntry = false;
  let scenario: RuntimeScenario | undefined;
  let pendingFunctionName: string | undefined;
  let profileName: RuntimeProfileName | undefined;
  while (args[0]?.startsWith("--")) {
    const option = args.shift();
    if (option === "--wat") wat = true;
    else if (option === "--entry") {
      entryName = args.shift() ?? usage();
      explicitEntry = true;
    } else if (option === "--scenario") scenario = runtimeScenario(args.shift());
    else if (option === "--pending-function") pendingFunctionName = args.shift() ?? usage();
    else if (option === "--profile") profileName = runtimeProfile(args.shift());
    else usage();
  }
  const file = args.shift();
  if (!command || !file || args.length > 0) usage();
  if (entryName !== "main" && command !== "run") usage();
  if (scenario && command !== "test") usage();
  if (pendingFunctionName && scenario !== "cancellation-cleanup") usage();
  const path = resolve(file);
  const source = await readFile(path, "utf8");
  const profile = profileName ? RUNTIME_PROFILES[profileName] : undefined;
  const compileOptions: CompileOptions = { hostCapabilities: profile?.hostCapabilities };
  try {
    if (command === "parse") {
      const result = parse(source);
      if (!result.program) throw new DiagnosticError(result.diagnostics);
      console.log(`${file}: ok`);
      return 0;
    }
    if (command === "check") {
      const result = analyze(source, compileOptions);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const diagnostic of result.diagnostics)
        console.error(formatDiagnostic(file, diagnostic));
      console.log(`${file}: ok`);
      return 0;
    }
    if (command === "dump-hir") {
      const result = analyze(source, compileOptions);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      console.log(JSON.stringify(result.hir, null, 2));
      return 0;
    }
    if (command === "explain-requirements") {
      const result = analyze(source, compileOptions);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const explanation of explainRequirements(result.hir)) {
        console.log(
          `${explanation.functionName}: ${explanation.declared.length > 0 ? "$ " + explanation.declared.join(" + ") : "$()"}`,
        );
        for (const requirement of explanation.paths)
          console.log(`  ${requirement.key}: ${requirement.path.join(" -> ")}`);
      }
      return 0;
    }
    if (command === "build") {
      const result = compile(source, compileOptions);
      if (wat) {
        console.log(result.wat);
      } else {
        const output = resolve(`${basename(path, extname(path))}.wasm`);
        await writeFile(output, result.bytes);
        console.log(output);
      }
      return 0;
    }
    if (
      command === "test" ||
      command === "run" ||
      command === "trace" ||
      command === "record" ||
      command === "replay"
    ) {
      const functionNames = new Map<number, string>();
      const eventNames = [
        "construct",
        "poll",
        "ready",
        "cancel",
        "reentrant",
        "invalid-state",
        "pending",
        "cleanup",
      ] as const;
      const replayPath = `${path}.replay.json`;
      const recorded: ReplayEvent[] = [];
      const replayEvents =
        command === "replay"
          ? (JSON.parse(await readFile(replayPath, "utf8")) as ReplayEvent[])
          : undefined;
      let scenarioInstance: WebAssembly.Instance | undefined;
      let pendingFunctionIndex: number | undefined;
      const { instance, compilation, replay } = await instantiate(source, {
        console: (text) => console.log(text),
        trace:
          command === "trace"
            ? (functionIndex, event) =>
                console.log(
                  `${eventNames[event]} ${functionNames.get(functionIndex) ?? `function#${functionIndex}`}`,
                )
            : undefined,
        record: command === "record" ? (event) => recorded.push(event) : undefined,
        replay: replayEvents,
        pending:
          scenario === "cancellation-cleanup"
            ? (functionIndex) => functionIndex === pendingFunctionIndex
            : scenario === "competing-drivers"
              ? () => true
              : scenario === "reentrant-poll"
                ? () => {
                    if (!scenarioInstance)
                      throw new Error("runtime scenario instance is not ready");
                    exportedFunction(scenarioInstance, "__hd_poll")();
                    return false;
                  }
                : undefined,
        providerConfigurationId: "cli-default",
        hostCapabilities: profile?.hostCapabilities,
        hostSuspensionInvoke: profile?.invoke,
        hostSuspensionPending: profile?.pending,
      });
      scenarioInstance = instance;
      if (pendingFunctionName) {
        pendingFunctionIndex = compilation.hir.functions.find(
          ({ name, suspending }) => name === pendingFunctionName && suspending,
        )?.index;
        if (pendingFunctionIndex === undefined)
          throw new Error(`program has no suspending ${pendingFunctionName} function`);
      }
      compilation.hir.functions.forEach((declaration) =>
        functionNames.set(declaration.index, declaration.name),
      );
      const mainDeclaration = compilation.hir.functions.find(({ entry }) => entry);
      const scenarioProviders =
        mainDeclaration?.requirements.map((requirement) => ({ requirement })) ?? [];
      if (scenario) {
        runRuntimeScenario(scenario, instance, scenarioProviders);
        replay.assertComplete();
        console.log(`${file}: 1 passed`);
        return 0;
      }
      // Only the entry point and test blocks execute
      // (spec/conformance/README.md#runtime-execution); `--entry` names any
      // exported function for `run`.
      const selected = compilation.hir.functions.filter((declaration) => {
        if (command === "test")
          return declaration.entry === true || /^\$test\.\d+$/.test(declaration.name);
        if (entryName !== "main") return declaration.name === entryName;
        // A non-`pub` `main` is not an entry point; implementation tests may
        // still run it by naming it explicitly.
        return (
          declaration.entry === true || (explicitEntry && declaration.developmentEntry === true)
        );
      });
      if (selected.length === 0 && command === "test") {
        replay.assertComplete();
        console.log(`${file}: 0 passed`);
        return 0;
      }
      if (selected.length === 0) throw new Error(`program has no exported ${entryName} function`);
      let result: unknown;
      for (const declaration of selected) {
        if (declaration.parameters.length > 0)
          throw new Error(`${declaration.name} must not declare ordinary parameters`);
        const exportName = /^\$test\.\d+$/.test(declaration.name)
          ? `__hd_test_${declaration.name.slice(6)}`
          : declaration.name;
        const entry = instance.exports[exportName];
        if (typeof entry !== "function")
          throw new Error(`${declaration.name} has no runnable export`);
        result = entry(...declaration.requirements.map((requirement) => ({ requirement })));
        if (declaration.entry && resultParts(declaration.result)?.ok === "void") {
          if (result !== 0) {
            console.error(`${file}: main returned Err`);
            return 1;
          }
          result = undefined;
        }
      }
      replay.assertComplete();
      if (command === "record") {
        await writeFile(replayPath, JSON.stringify(recorded, null, 2) + "\n");
        console.log(replayPath);
      }
      if (command === "test") console.log(`${file}: ${selected.length} passed`);
      else if (result !== undefined) console.log(result);
      return 0;
    }
    usage();
  } catch (error) {
    if (error instanceof DiagnosticError) {
      for (const diagnostic of error.diagnostics) console.error(formatDiagnostic(file, diagnostic));
      return 1;
    }
    if (error instanceof RuntimePanicError) {
      console.error(`${error.code}: runtime panic`);
      return 1;
    }
    throw error;
  }
}

const invokedPath = process.argv[1] && pathToFileURL(process.argv[1]).href;
if (invokedPath === import.meta.url) process.exitCode = await main();
