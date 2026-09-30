import { readdir, readFile, writeFile } from "node:fs/promises";
import { basename, extname, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import {
  analyze,
  compile,
  instantiate,
  type CompileOptions,
  type HostSuspensionCall,
  type HostSuspensionOutcome,
} from "./compiler.ts";
import { explainCommand, lookupCommand } from "./cli-queries.ts";
import { DiagnosticReporter, type OutputFormat } from "./diagnostic-report.ts";
import { DiagnosticError, type Diagnostic } from "./diagnostics.ts";
import { linkPackage, type PackageDiagnostic } from "./package.ts";
import { RuntimePanicError, UnsupportedAtRunTimeError } from "./runtime-panic.ts";
import { parse } from "./parser/index.ts";
import { runRepl } from "./repl-terminal.ts";
import { loadSpecIndex } from "./spec-index.ts";
import { regressionStore, snapshotModule, snapshotRun } from "./snapshots.ts";
import { runSelected } from "./test-runner.ts";
import { propertyRun } from "./property-tests.ts";

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

/** Every `.hd` file under a package tree, keyed by its package path. */
async function packageTreeFiles(root: string): Promise<Record<string, string>> {
  const files: Record<string, string> = {};
  for (const entry of await readdir(root, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith(".hd")) continue;
    const full = join(entry.parentPath, entry.name);
    files[relative(root, full).split(sep).join("/")] = await readFile(full, "utf8");
  }
  return files;
}

function usage(): never {
  console.error(
    [
      "usage: hd <parse|check|test|run|build|dump-hir> [--format text|json] [--wat] [--entry NAME] [--profile NAME] [--scenario NAME] [--pending-function NAME] [--tests] [--update] [--seed N] [--cases N] [--shrink N] [--test-layout test-module|integration] [--package-tree DIR --package-path PATH] FILE",
      "       hd explain [--format text|json] CODE",
      "       hd <def|doc> [--format text|json] NAME [FILE|PACKAGE-DIR]",
      "       hd repl",
    ].join("\n"),
  );
  process.exit(2);
}

function outputFormat(value: string | undefined): OutputFormat {
  if (value === "text" || value === "json") return value;
  return usage();
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
  let format: OutputFormat = "text";
  let runOptions = false;
  let checkTests = false;
  let testLayout: string | undefined;
  // A package tree (spec/conformance/README.md, Package Trees): the other
  // files of the package, and the package path FILE takes among them.
  let packageTree: string | undefined;
  let packagePath: string | undefined;
  let update = false;
  // `hd test --seed N`, `--cases N`, and `--shrink N` (Testing T36, T38, T51).
  const propertyOptions: { seed?: number; cases?: number; shrink?: number } = {};
  while (args[0]?.startsWith("--")) {
    const option = args.shift();
    if (option !== "--format") runOptions = true;
    if (option === "--format") format = outputFormat(args.shift());
    else if (option === "--wat") wat = true;
    else if (option === "--entry") {
      entryName = args.shift() ?? usage();
      explicitEntry = true;
    } else if (option === "--scenario") scenario = runtimeScenario(args.shift());
    else if (option === "--pending-function") pendingFunctionName = args.shift() ?? usage();
    else if (option === "--profile") profileName = runtimeProfile(args.shift());
    else if (option === "--tests") checkTests = true;
    else if (option === "--update") update = true;
    else if (option === "--seed" || option === "--cases" || option === "--shrink") {
      const value = Number(args.shift());
      if (!Number.isSafeInteger(value) || value < 0) usage();
      propertyOptions[option.slice(2) as "seed" | "cases" | "shrink"] = value;
    } else if (option === "--test-layout") {
      testLayout = args.shift();
      if (testLayout !== "test-module" && testLayout !== "integration") usage();
    } else if (option === "--package-tree") packageTree = args.shift() ?? usage();
    else if (option === "--package-path") packagePath = args.shift() ?? usage();
    else usage();
  }
  if (command === "explain") {
    const code = args.shift();
    if (!code || args.length > 0 || runOptions) usage();
    return explainCommand(code, format);
  }
  if (command === "def" || command === "doc") {
    const name = args.shift();
    const target = args.shift() ?? ".";
    if (!name || args.length > 0 || runOptions) usage();
    return lookupCommand(command, name, target, format);
  }
  const file = args.shift();
  if (!command || !file || args.length > 0) usage();
  if (entryName !== "main" && command !== "run") usage();
  if (checkTests && command !== "check") usage();
  if (update && command !== "test") usage();
  if (Object.keys(propertyOptions).length > 0 && command !== "test") usage();
  if (scenario && command !== "test") usage();
  if (pendingFunctionName && scenario !== "cancellation-cleanup") usage();
  if ((packageTree === undefined) !== (packagePath === undefined)) usage();
  if (packageTree !== undefined && ((command !== "check" && command !== "test") || testLayout))
    usage();
  const path = resolve(file);
  const fileSource = await readFile(path, "utf8");
  // In a package tree, FILE joins the tree's files; the linker joins the
  // modules into one program (src/package.ts).
  const treeRoot = packageTree === undefined ? undefined : resolve(packageTree);
  const treeFiles =
    treeRoot === undefined
      ? undefined
      : { ...(await packageTreeFiles(treeRoot)), [packagePath!]: fileSource };
  const linked = treeFiles
    ? linkPackage(treeFiles, packagePath!, { tests: checkTests || command === "test" })
    : undefined;
  const source = linked?.source ?? fileSource;
  const profile = profileName ? RUNTIME_PROFILES[profileName] : undefined;
  // A `*_test.hd` file is a test module (spec/10-modules.md#test-modules), as
  // is a file that `--test-layout` places as one (spec/conformance, Test
  // Layouts); the prototype has no separate integration test view.
  const parseOptions = linked
    ? { joinedModules: true }
    : path.endsWith("_test.hd") || testLayout !== undefined
      ? { testModule: true }
      : {};
  const compileOptions: CompileOptions = {
    hostCapabilities: profile?.hostCapabilities,
    parse: parseOptions,
  };
  const specIndex = format === "json" ? await loadSpecIndex() : undefined;
  const reporter = new DiagnosticReporter(format, file, fileSource, specIndex);
  // A diagnostic in a package tree names the file it points into.
  const treeReporters = new Map<string, DiagnosticReporter>();
  const report = (diagnostic: Diagnostic | PackageDiagnostic): void => {
    if (!linked || !treeFiles) return reporter.diagnostic(diagnostic);
    const located = "path" in diagnostic ? diagnostic : linked.locate(diagnostic);
    if (located.path === packagePath) return reporter.diagnostic(located);
    let treeReporter = treeReporters.get(located.path);
    if (!treeReporter) {
      treeReporter = new DiagnosticReporter(
        format,
        join(treeRoot!, located.path),
        treeFiles[located.path] ?? "",
        specIndex,
      );
      treeReporters.set(located.path, treeReporter);
    }
    treeReporter.diagnostic(located);
  };
  if (linked) {
    for (const diagnostic of linked.diagnostics) report(diagnostic);
    if (!linked.source) return 1;
  }
  try {
    if (command === "parse") {
      const result = parse(source, parseOptions);
      if (!result.program) throw new DiagnosticError(result.diagnostics);
      console.log(`${file}: ok`);
      return 0;
    }
    if (command === "check") {
      // `hd check` checks test code only with `--tests` (Testing T42).
      const result = analyze(source, { ...compileOptions, skipTestCode: !checkTests });
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const diagnostic of result.diagnostics) report(diagnostic);
      console.log(`${file}: ok`);
      return 0;
    }
    if (command === "dump-hir") {
      const result = analyze(source, compileOptions);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      console.log(JSON.stringify(result.hir, null, 2));
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
    if (command === "test" || command === "run") {
      let scenarioInstance: WebAssembly.Instance | undefined;
      let pendingFunctionIndex: number | undefined;
      // `snapshot_file` files (spec/10-modules.md#snapshots); `--update` records them.
      const snapshots = snapshotRun(path, update);
      // Property-test regression files (spec/std/testing.md#r-std-testing.prop.regression-file).
      const { root: packageRoot, module: testModule } = snapshotModule(path);
      const properties = propertyRun({
        ...propertyOptions,
        regressions: regressionStore(packageRoot, testModule),
      });
      const instantiateOptions: Parameters<typeof instantiate>[1] = {
        hostFunctions: { ...snapshots.hostFunctions, ...properties.hostFunctions },
        console: (text) => console.log(text),
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
        hostCapabilities: profile?.hostCapabilities,
        parse: parseOptions,
        hostSuspensionInvoke: profile?.invoke,
        hostSuspensionPending: profile?.pending,
      };
      const { instance, compilation } = await instantiate(source, instantiateOptions);
      scenarioInstance = instance;
      if (pendingFunctionName) {
        pendingFunctionIndex = compilation.hir.functions.find(
          ({ name, suspending }) => name === pendingFunctionName && suspending,
        )?.index;
        if (pendingFunctionIndex === undefined)
          throw new Error(`program has no suspending ${pendingFunctionName} function`);
      }
      const mainDeclaration = compilation.hir.functions.find(({ entry }) => entry);
      const scenarioProviders =
        mainDeclaration?.requirements.map((requirement) => ({ requirement })) ?? [];
      if (scenario) {
        runRuntimeScenario(scenario, instance, scenarioProviders);
        console.log(`${file}: 1 passed`);
        return 0;
      }
      // Only the entry point and test cases execute
      // (spec/conformance/README.md#runtime-execution); `--entry` names any
      // exported function for `run`.
      // A test case with the `ignore` option does not run
      // (spec/10-modules.md#r-module.testing.option.ignore).
      const selected = compilation.hir.functions.filter((declaration) => {
        if (command === "test")
          return (
            declaration.entry === true ||
            (/^\$test\.\d+$/.test(declaration.name) &&
              declaration.testOptions?.ignore === undefined)
          );
        if (entryName !== "main") return declaration.name === entryName;
        // A non-`pub` `main` is not an entry point; implementation tests may
        // still run it by naming it explicitly.
        return (
          declaration.entry === true || (explicitEntry && declaration.developmentEntry === true)
        );
      });
      if (selected.length === 0 && command === "test") {
        console.log(`${file}: 0 passed`);
        return 0;
      }
      if (selected.length === 0) throw new Error(`program has no exported ${entryName} function`);
      const outcome = await runSelected(
        selected,
        instance.exports,
        async () =>
          (await instantiate(source, { ...instantiateOptions, compilation })).instance.exports,
        snapshots.begin,
        properties,
      );
      if (outcome.kind === "exit") return outcome.code;
      if (outcome.kind === "failed") {
        reporter.entryError(outcome.subject, outcome.outcome);
        return 1;
      }
      if (command === "test") console.log(`${file}: ${outcome.count} passed`);
      else if (outcome.result !== undefined) console.log(outcome.result);
      return 0;
    }
    usage();
  } catch (error) {
    if (error instanceof DiagnosticError) {
      for (const diagnostic of error.diagnostics) report(diagnostic);
      return 1;
    }
    if (error instanceof RuntimePanicError) {
      reporter.runtimePanic(error.code);
      return 1;
    }
    if (error instanceof UnsupportedAtRunTimeError) {
      reporter.unsupported(error.code, error.message);
      return 1;
    }
    throw error;
  }
}

const invokedPath = process.argv[1] && pathToFileURL(process.argv[1]).href;
if (invokedPath === import.meta.url) process.exitCode = await main();
