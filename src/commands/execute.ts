// `hd run` and `hd test`: the commands that compile FILE and run it.

import { readdir } from "node:fs/promises";
import { join, relative, resolve } from "node:path";

import { instantiate } from "../compiler.ts";
import type { HirFunction } from "../hir.ts";
import { propertyRun } from "../property-tests.ts";
import { regressionStore, snapshotModule, snapshotRun } from "../snapshots.ts";
import { runSelected } from "../test-runner.ts";
import type { CommandIo } from "./io.ts";
import {
  exportedFunction,
  RUNTIME_PROFILES,
  runRuntimeScenario,
  type RuntimeScenario,
} from "./profiles.ts";
import {
  enclosingPackageRoot,
  isDirectory,
  isPackageRoot,
  loadSource,
  packageFiles,
  placementOf,
  reportFailure,
  type LoadedSource,
  type PackagePlacement,
  type PackageTree,
  type RuntimeProfileName,
  type SourceArgs,
  type TestLayout,
} from "./source.ts";

export interface RunArgs extends SourceArgs {
  /** `--entry NAME`: run this exported function instead, and print its result. */
  readonly entry?: string;
  readonly profile?: RuntimeProfileName;
}

/** `hd run FILE`: runs FILE's entry point, the public `main` or `main!`. */
export async function runCommand(args: RunArgs, io: CommandIo): Promise<number> {
  const placement = await placementOf(args.file, undefined, undefined);
  const loaded = await loadSource(args, io, { profile: args.profile, linkTests: false }, placement);
  if (typeof loaded === "number") return loaded;
  return execute(loaded, io, { kind: "run", entry: args.entry, profile: args.profile });
}

export interface TestArgs {
  /** FILE or DIR; the default is the package that holds the current directory. */
  readonly path?: string;
  readonly format: SourceArgs["format"];
  /** `--update`: record snapshot files instead of failing on a difference. */
  readonly update: boolean;
  /** `--seed N`, `--cases N`, and `--shrink N` (Testing T36, T38, T51). */
  readonly seed?: number;
  readonly cases?: number;
  readonly shrink?: number;
  readonly profile?: RuntimeProfileName;
  readonly scenario?: RuntimeScenario;
  /** `--pending-function NAME`, with the cancellation-cleanup scenario. */
  readonly pendingFunction?: string;
  readonly testLayout?: TestLayout;
  readonly packageTree?: PackageTree;
}

/** `hd test [FILE|DIR]`: runs the test cases of FILE, of DIR, or of the current package. */
export async function testCommand(args: TestArgs, io: CommandIo): Promise<number> {
  if (args.path === undefined) {
    const root = enclosingPackageRoot(process.cwd()) ?? process.cwd();
    return testDirectory(args, relative(process.cwd(), root) || ".", io);
  }
  if (await isDirectory(args.path)) return testDirectory(args, args.path, io);
  const placement = await placementOf(args.path, args.packageTree, args.testLayout);
  return testFile(args, args.path, io, placement, false);
}

async function testFile(
  args: TestArgs,
  file: string,
  io: CommandIo,
  placement: PackagePlacement | undefined,
  quietWhenEmpty: boolean,
): Promise<number> {
  const loaded = await loadSource(
    { file, format: args.format },
    io,
    { profile: args.profile, testLayout: args.testLayout, linkTests: true },
    placement,
  );
  if (typeof loaded === "number") return loaded;
  return execute(loaded, io, { kind: "test", quietWhenEmpty, ...args });
}

/**
 * `hd test DIR`: a package (a directory with `hd.toml` or `src/`) tests each
 * module under `src/` and `tests/` with the other modules linked; any other
 * directory tests each `.hd` file directly in it.
 */
async function testDirectory(args: TestArgs, directory: string, io: CommandIo): Promise<number> {
  const root = resolve(directory);
  let status = 0;
  let ran = 0;
  if (isPackageRoot(root)) {
    const files = await packageFiles(root);
    const reported = new Set<string>();
    for (const path of Object.keys(files).sort()) {
      ran += 1;
      const code = await testFile(
        args,
        join(directory, path),
        io,
        { root: directory, path, files, reported },
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
      status = Math.max(status, await testFile(args, join(directory, name), io, undefined, true));
    }
  }
  if (ran === 0) io.out(`${directory}: no .hd files to test`);
  return status;
}

/** What `execute` runs: the entry point (`run`), or the test cases (`test`). */
type Execution =
  | { readonly kind: "run"; readonly entry?: string; readonly profile?: RuntimeProfileName }
  | ({ readonly kind: "test"; readonly quietWhenEmpty: boolean } & TestArgs);

async function execute(loaded: LoadedSource, io: CommandIo, execution: Execution): Promise<number> {
  const { file, path, source, linked, placement, reporter } = loaded;
  const command = execution.kind;
  const entryName = (command === "run" && execution.entry) || "main";
  const explicitEntry = command === "run" && execution.entry !== undefined;
  const test = command === "test" ? execution : undefined;
  const scenario = test?.scenario;
  const runtimeProfile = execution.profile ? RUNTIME_PROFILES[execution.profile] : undefined;
  try {
    let scenarioInstance: WebAssembly.Instance | undefined;
    let pendingFunctionIndex: number | undefined;
    // `snapshot_file` files (spec/std/testing.md#snapshot-files); `--update` records them.
    const snapshots = snapshotRun(path, test?.update ?? false);
    // Property-test regression files (spec/std/testing.md#r-std-testing.prop.regression-file).
    const { root: packageRoot, module: testModule } = snapshotModule(path);
    const propertyOptions: { seed?: number; cases?: number; shrink?: number } = {};
    for (const key of ["seed", "cases", "shrink"] as const)
      if (test?.[key] !== undefined) propertyOptions[key] = test[key];
    const properties = propertyRun({
      ...propertyOptions,
      regressions: regressionStore(packageRoot, testModule),
    });
    const instantiateOptions: Parameters<typeof instantiate>[1] = {
      hostFunctions: { ...snapshots.hostFunctions, ...properties.hostFunctions },
      console: (text) => io.out(text),
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
      hostCapabilities: runtimeProfile?.hostCapabilities,
      parse: loaded.parseOptions,
      hostSuspensionInvoke: runtimeProfile?.invoke,
      hostSuspensionPending: runtimeProfile?.pending,
    };
    const { instance, compilation } = await instantiate(source, instantiateOptions);
    scenarioInstance = instance;
    if (test?.pendingFunction) {
      pendingFunctionIndex = compilation.hir.functions.find(
        ({ name, suspending }) => name === test.pendingFunction && suspending,
      )?.index;
      if (pendingFunctionIndex === undefined)
        throw new Error(`program has no suspending ${test.pendingFunction} function`);
    }
    const mainDeclaration = compilation.hir.functions.find(({ entry }) => entry);
    const scenarioProviders =
      mainDeclaration?.requirements.map((requirement) => ({ requirement })) ?? [];
    if (scenario) {
      runRuntimeScenario(scenario, instance, scenarioProviders);
      io.out(`${file}: 1 passed`);
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
    // (spec/lang/10-modules.md#r-module.testing.option.ignore).
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
    if (selected.length === 0 && test) {
      if (!test.quietWhenEmpty) io.out(`${file}: 0 passed`);
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
    if (test) io.out(`${file}: ${outcome.count} passed`);
    else if (outcome.result !== undefined) io.out(outcome.result);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}
