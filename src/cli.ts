import { existsSync } from "node:fs";
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { basename, dirname, extname, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import {
  analyze,
  compile,
  instantiate,
  type CompileOptions,
  type HostSuspensionCall,
  type HostSuspensionOutcome,
} from "./compiler.ts";
import {
  commandHelp,
  overviewHelp,
  parseCommandLine,
  UsageError,
  type ParsedCommand,
  type RUNTIME_PROFILE_NAMES,
  type RUNTIME_SCENARIO_NAMES,
} from "./cli-args.ts";
import { explainCommand, lookupCommand } from "./cli-queries.ts";
import { DiagnosticReporter, type OutputFormat } from "./diagnostic-report.ts";
import { DiagnosticError, type Diagnostic } from "./diagnostics.ts";
import type { HirFunction } from "./hir.ts";
import { linkPackage, SOURCE_ROOT, type PackageDiagnostic } from "./package.ts";
import { RuntimePanicError, UnsupportedAtRunTimeError } from "./runtime-panic.ts";
import { parse } from "./parser/index.ts";
import { runRepl } from "./repl-terminal.ts";
import { loadSpecIndex } from "./spec-index.ts";
import { regressionStore, snapshotModule, snapshotRun } from "./snapshots.ts";
import { runSelected } from "./test-runner.ts";
import { propertyRun } from "./property-tests.ts";

type RuntimeScenario = (typeof RUNTIME_SCENARIO_NAMES)[number];
type RuntimeProfileName = (typeof RUNTIME_PROFILE_NAMES)[number];

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

/** The flags of a command that compiles a file, after parsing. */
interface FileOptions {
  readonly command: "build" | "check" | "hir" | "parse" | "run" | "test";
  readonly format: OutputFormat;
  readonly wat: boolean;
  /** `hd run --entry NAME`. */
  readonly entryName?: string;
  readonly profileName?: RuntimeProfileName;
  readonly scenario?: RuntimeScenario;
  readonly pendingFunctionName?: string;
  /** `hd check --tests` (Testing T42). */
  readonly checkTests: boolean;
  readonly testLayout?: string;
  /** `hd test --update` records snapshot files. */
  readonly update: boolean;
  /** `hd test --seed N`, `--cases N`, and `--shrink N` (Testing T36, T38, T51). */
  readonly propertyOptions: { seed?: number; cases?: number; shrink?: number };
}

/**
 * FILE's place in a package (spec/conformance/README.md, Package Trees): the
 * package root, the package path FILE takes, and the package's files.
 */
interface PackagePlacement {
  readonly root: string;
  readonly path: string;
  readonly files: Readonly<Record<string, string>>;
  /**
   * Diagnostics already printed, shared by the module runs of `hd test DIR`,
   * so that an error in a module that several others link prints once.
   */
  readonly reported?: Set<string>;
}

/** Compiles FILE and does what `options.command` asks with it. */
async function runFile(
  options: FileOptions,
  file: string,
  placement?: PackagePlacement,
  quietWhenEmpty = false,
): Promise<number> {
  const { command, format, profileName, scenario, pendingFunctionName } = options;
  const entryName = options.entryName ?? "main";
  const explicitEntry = options.entryName !== undefined;
  const path = resolve(file);
  const fileSource = await readFile(path, "utf8");
  // In a package, FILE joins the package's files; the linker joins the
  // modules into one program (src/package.ts).
  const treeFiles = placement && { ...placement.files, [placement.path]: fileSource };
  const linked =
    placement && treeFiles
      ? linkPackage(treeFiles, placement.path, {
          tests: options.checkTests || command === "test",
        })
      : undefined;
  const source = linked?.source ?? fileSource;
  const profile = profileName ? RUNTIME_PROFILES[profileName] : undefined;
  // A `*_test.hd` file is a test module (spec/10-modules.md#test-modules), as
  // is a file that `--test-layout` places as one (spec/conformance, Test
  // Layouts); the prototype has no separate integration test view.
  const parseOptions = linked
    ? { joinedModules: true }
    : path.endsWith("_test.hd") || options.testLayout !== undefined
      ? { testModule: true }
      : {};
  const compileOptions: CompileOptions = {
    hostCapabilities: profile?.hostCapabilities,
    parse: parseOptions,
  };
  const specIndex = format === "json" ? await loadSpecIndex() : undefined;
  const reporter = new DiagnosticReporter(format, file, fileSource, specIndex);
  // A diagnostic in a package names the file it points into.
  const treeReporters = new Map<string, DiagnosticReporter>();
  const report = (diagnostic: Diagnostic | PackageDiagnostic): void => {
    if (!linked || !placement || !treeFiles) return reporter.diagnostic(diagnostic);
    const located = "path" in diagnostic ? diagnostic : linked.locate(diagnostic);
    const { line, column } = located.span.start;
    const key = `${located.path}:${line}:${column}: ${located.code}: ${located.message}`;
    if (placement.reported?.has(key)) return;
    placement.reported?.add(key);
    if (located.path === placement.path) return reporter.diagnostic(located);
    let treeReporter = treeReporters.get(located.path);
    if (!treeReporter) {
      treeReporter = new DiagnosticReporter(
        format,
        join(placement.root, located.path),
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
      const result = analyze(source, { ...compileOptions, skipTestCode: !options.checkTests });
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const diagnostic of result.diagnostics) report(diagnostic);
      console.log(`${file}: ok`);
      return 0;
    }
    if (command === "hir") {
      const result = analyze(source, compileOptions);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      console.log(JSON.stringify(result.hir, null, 2));
      return 0;
    }
    if (command === "build") {
      const result = compile(source, compileOptions);
      if (options.wat) {
        console.log(result.wat);
      } else {
        const output = resolve(`${basename(path, extname(path))}.wasm`);
        await writeFile(output, result.bytes);
        console.log(output);
      }
      return 0;
    }
    let scenarioInstance: WebAssembly.Instance | undefined;
    let pendingFunctionIndex: number | undefined;
    // `snapshot_file` files (spec/std/testing.md#snapshot-files); `--update` records them.
    const snapshots = snapshotRun(path, options.update);
    // Property-test regression files (spec/std/testing.md#r-std-testing.prop.regression-file).
    const { root: packageRoot, module: testModule } = snapshotModule(path);
    const properties = propertyRun({
      ...options.propertyOptions,
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
                  if (!scenarioInstance) throw new Error("runtime scenario instance is not ready");
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
    // In a package, `hd test` runs the test cases of FILE's module only
    // (spec/conformance/README.md, Package Trees).
    const inFileModule = (declaration: HirFunction): boolean =>
      !linked ||
      !placement ||
      linked.locate({ code: "", message: "", span: declaration.span }).path === placement.path;
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
            declaration.testOptions?.ignore === undefined &&
            inFileModule(declaration))
        );
      if (explicitEntry) return declaration.name === entryName;
      // A non-`pub` `main` is not an entry point; implementation tests may
      // still run it by naming it explicitly.
      return declaration.entry === true;
    });
    if (selected.length === 0 && command === "test") {
      if (!quietWhenEmpty) console.log(`${file}: 0 passed`);
      return 0;
    }
    // A module without an entry point runs its initialization and exits 0
    // (owner decision, batch 42); `--entry` must name a function.
    if (selected.length === 0 && !explicitEntry) return 0;
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
  } catch (error) {
    if (error instanceof DiagnosticError) {
      for (const diagnostic of error.diagnostics) report(diagnostic);
      return 1;
    }
    if (error instanceof RuntimePanicError) {
      reporter.runtimePanic(error.code, error.detail);
      return 1;
    }
    if (error instanceof UnsupportedAtRunTimeError) {
      reporter.unsupported(error.code, error.message);
      return 1;
    }
    throw error;
  }
}

async function isDirectory(path: string): Promise<boolean> {
  return (await stat(path).catch(() => undefined))?.isDirectory() ?? false;
}

/** The nearest directory at or above `start` that holds `hd.toml`, else `start`. */
function packageRootFrom(start: string): string {
  for (let directory = start; ;) {
    if (existsSync(join(directory, "hd.toml"))) return directory;
    const parent = dirname(directory);
    if (parent === directory) return start;
    directory = parent;
  }
}

/**
 * `hd test DIR`: a package (a directory with `hd.toml` or `src/`) tests each
 * module under `src/` with the other modules linked; any other directory
 * tests each `.hd` file directly in it.
 */
async function testDirectory(options: FileOptions, directory: string): Promise<number> {
  const root = resolve(directory);
  const sourceRoot = join(root, SOURCE_ROOT);
  const hasSources = await isDirectory(sourceRoot);
  let status = 0;
  let ran = 0;
  if (hasSources || existsSync(join(root, "hd.toml"))) {
    const sources = hasSources ? await packageTreeFiles(sourceRoot) : {};
    const files = Object.fromEntries(
      Object.entries(sources).map(([path, text]) => [`${SOURCE_ROOT}${path}`, text]),
    );
    const reported = new Set<string>();
    for (const path of Object.keys(files).sort()) {
      ran += 1;
      const code = await runFile(
        options,
        join(directory, path),
        { root, path, files, reported },
        true,
      );
      status = Math.max(status, code);
    }
  } else {
    const names = (await readdir(root, { withFileTypes: true }))
      .filter((entry) => entry.isFile() && entry.name.endsWith(".hd"))
      .map((entry) => entry.name)
      .sort();
    for (const name of names) {
      ran += 1;
      status = Math.max(status, await runFile(options, join(directory, name), undefined, true));
    }
  }
  if (ran === 0) console.log(`${directory}: no .hd files to test`);
  return status;
}

type CompilingCommand = ParsedCommand & { kind: "command" };

function fileOptions(parsed: CompilingCommand): FileOptions {
  const flag = (name: string): string | undefined => {
    const value = parsed.flags.get(name);
    return typeof value === "string" ? value : undefined;
  };
  const name = parsed.command.name;
  const propertyOptions: { seed?: number; cases?: number; shrink?: number } = {};
  for (const key of ["seed", "cases", "shrink"] as const) {
    const value = flag(`--${key}`);
    if (value !== undefined) propertyOptions[key] = Number(value);
  }
  const options: FileOptions = {
    command: (name === "debug parse"
      ? "parse"
      : name === "debug hir"
        ? "hir"
        : name) as FileOptions["command"],
    format: parsed.format,
    wat: parsed.flags.has("--wat"),
    entryName: flag("--entry"),
    profileName: flag("--profile") as RuntimeProfileName | undefined,
    scenario: flag("--scenario") as RuntimeScenario | undefined,
    pendingFunctionName: flag("--pending-function"),
    checkTests: parsed.flags.has("--tests"),
    testLayout: flag("--test-layout"),
    update: parsed.flags.has("--update"),
    propertyOptions,
  };
  const where = `hd ${name}`;
  if (options.pendingFunctionName && options.scenario !== "cancellation-cleanup")
    throw new UsageError(`${where}: --pending-function needs --scenario cancellation-cleanup`);
  if (parsed.flags.has("--package-tree") !== parsed.flags.has("--package-path"))
    throw new UsageError(`${where}: --package-tree and --package-path go together`);
  if (parsed.flags.has("--package-tree") && options.testLayout)
    throw new UsageError(`${where}: --package-tree and --test-layout exclude each other`);
  if (parsed.flags.has("--package-tree") && parsed.operands.length === 0)
    throw new UsageError(`${where}: --package-tree needs a FILE`);
  return options;
}

const QUERY_COMMANDS = new Set(["explain", "doc", "def", "repl"]);

export async function main(args = process.argv.slice(2)): Promise<number> {
  let parsed: ParsedCommand;
  let options: FileOptions | undefined;
  try {
    parsed = parseCommandLine(args);
    if (parsed.kind === "command" && !QUERY_COMMANDS.has(parsed.command.name))
      options = fileOptions(parsed);
  } catch (error) {
    if (!(error instanceof UsageError)) throw error;
    console.error(error.message);
    return 2;
  }
  if (parsed.kind === "help") {
    console.log(parsed.topic ? commandHelp(parsed.topic) : overviewHelp());
    return 0;
  }
  const [first, second] = parsed.operands;
  const name = parsed.command.name;
  if (name === "repl") return runRepl();
  if (name === "explain") return explainCommand(first!, parsed.format);
  if (name === "doc" || name === "def")
    return lookupCommand(name, first!, second ?? ".", parsed.format);
  if (!options) throw new Error(`hd ${name} has no file options`);
  if (options.command === "test" && first === undefined) {
    const root = packageRootFrom(process.cwd());
    return testDirectory(options, relative(process.cwd(), root) || ".");
  }
  if (options.command === "test" && (await isDirectory(first!)))
    return testDirectory(options, first!);
  const tree = parsed.flags.get("--package-tree");
  const placement =
    typeof tree === "string"
      ? {
          root: resolve(tree),
          path: String(parsed.flags.get("--package-path")),
          files: await packageTreeFiles(resolve(tree)),
        }
      : undefined;
  return runFile(options, first!, placement);
}

const invokedPath = process.argv[1] && pathToFileURL(process.argv[1]).href;
if (invokedPath === import.meta.url) process.exitCode = await main();
