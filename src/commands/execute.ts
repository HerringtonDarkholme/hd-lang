// `hd FILE`, `hd run`, and `hd test`: the commands that compile a program
// and run it.

import { stat } from "node:fs/promises";
import { dirname, relative, resolve, sep } from "node:path";

import { analyze, instantiate } from "../compiler.ts";
import { Report } from "../diagnostic-report.ts";
import { DiagnosticError } from "../diagnostics.ts";
import type { HirFunction } from "../hir.ts";
import { SOURCE_ROOT, TEST_ROOT } from "../package.ts";
import { propertyRun } from "../property-tests.ts";
import { regressionStore, snapshotModule, snapshotRun } from "../snapshots.ts";
import { runSelected } from "../test-runner.ts";
import {
  EXIT_HD_FAILURE,
  workingDirectory,
  type CommandEnvironment,
  type CommandIo,
} from "./io.ts";
import { packageMode, type Executable, type LocalPackage } from "./package-mode.ts";
import { executableProcesses, isProcessCall, type ProcessProvider } from "./processes.ts";
import {
  exportedFunction,
  RUNTIME_PROFILES,
  pendingFirstPoll,
  runRuntimeScenario,
  type RuntimeScenario,
} from "./profiles.ts";
import {
  commandPackage,
  loadSource,
  placementOf,
  reportFailure,
  reportPackageProblems,
  shownPath,
  shownRoot,
  type LoadedSource,
  type PackagePlacement,
  type PackageTree,
  type RuntimeProfileName,
  type SourceArgs,
  type TestLayout,
} from "./source.ts";

async function isDirectory(path: string): Promise<boolean> {
  return (await stat(path).catch(() => undefined))?.isDirectory() ?? false;
}

/**
 * A positional word that names a directory is neither a NAME nor a FILE
 * (spec/cli/command-line.md#r-cli.command.positional). Reports the error and
 * returns true for one.
 */
async function directoryWord(
  command: string,
  word: string,
  report: Report,
  environment: CommandEnvironment,
): Promise<boolean> {
  if (!(await isDirectory(resolve(workingDirectory(environment), word)))) return false;
  report.commandError(
    `hd ${command}: '${word}' is a directory, which is neither a NAME nor a FILE; to work on a workspace member, select it with -p NAME`,
  );
  return true;
}

export interface FileArgs extends SourceArgs {
  /** `--entry NAME`: run this exported function instead, and print its result. */
  readonly entry?: string;
  readonly profile?: RuntimeProfileName;
  /** The program's arguments, after `--` (spec/cli/command-line.md#r-cli.args.pass). */
  readonly programArguments?: readonly string[];
}

/**
 * `hd FILE`: runs FILE as a single-file program, whether or not it lies in a
 * package (spec/cli/command-line.md#r-cli.file.run).
 */
export async function fileCommand(args: FileArgs, io: CommandIo): Promise<number> {
  // The program's standard output passes through, so a JSON run writes its
  // records to standard error (spec/cli/command-line.md#r-cli.json.run).
  const report = new Report(args.format, io, { stream: "stderr", summary: true });
  const loaded = await loadSource(args, {
    report,
    profile: args.profile,
    linkTests: false,
    singleFileNote: await singleFileNote(args),
  });
  if (typeof loaded === "number") return report.finish(loaded);
  return report.finish(
    await execute(loaded, io, {
      kind: "run",
      entry: args.entry,
      pendingFirstPoll: args.pendingFirstPoll,
      profile: args.profile,
    }),
  );
}

/**
 * The note on a package use in `hd FILE` when FILE lies in a package
 * (spec/cli/command-line.md#r-cli.file.in-package): it names the executable
 * whose entry module FILE is, with its `hd run` command
 * (spec/cli/command-line.md#r-cli.file.entry-hint), or else suggests a task.
 */
async function singleFileNote(args: FileArgs): Promise<string | undefined> {
  const path = resolve(workingDirectory(args), args.file);
  const mode = await packageMode(dirname(path));
  if (mode.kind !== "package") return undefined;
  const pkg = mode.package;
  const packagePath = relative(pkg.root, path).split(sep).join("/");
  const executable = pkg.executables.find((candidate) => candidate.path === packagePath);
  if (executable)
    return `${args.file} is the entry module of the executable '${executable.name}' of package '${pkg.name}'; run it with: hd run ${executable.name}`;
  const task = pkg.tasks.find((candidate) => candidate.path === packagePath);
  if (task)
    return `${args.file} is the task '${task.name}' of package '${pkg.name}'; run it with: hd run ${task.name}`;
  return `hd FILE runs ${args.file} on its own, outside package '${pkg.name}'; to use the package's modules, make it a task, tasks/NAME.hd, and run it with: hd run NAME`;
}

export interface RunArgs extends CommandEnvironment {
  /** NAME: the executable to run; absent for the package's one executable. */
  readonly name?: string;
  readonly format: SourceArgs["format"];
  /** `--release`: integer overflow wraps instead of panicking. */
  readonly release?: boolean;
  readonly profile?: RuntimeProfileName;
  /** The program's arguments, after `--` (spec/cli/command-line.md#r-cli.args.pass). */
  readonly programArguments?: readonly string[];
}

/**
 * `hd run [NAME]`: runs the package's executable named NAME, or its one
 * executable (spec/cli/command-line.md#choosing-what-runs).
 */
export async function runCommand(args: RunArgs, io: CommandIo): Promise<number> {
  // The program's standard output passes through, so a JSON run writes its
  // records to standard error (spec/cli/command-line.md#r-cli.json.run).
  const report = new Report(args.format, io, { stream: "stderr", summary: true });
  if (args.name !== undefined) {
    // `hd run FILE` is an error (spec/cli/command-line.md#r-cli.run.file).
    if (args.name.endsWith(".hd")) {
      report.commandError(
        `hd run: runs an executable of the package, not a FILE; use hd run, or hd run NAME for the executable NAME (run ${args.name} on its own with hd ${args.name})`,
      );
      return report.finish(EXIT_HD_FAILURE);
    }
    if (await directoryWord("run", args.name, report, args)) return report.finish(EXIT_HD_FAILURE);
  }
  const pkg = await commandPackage("run", report, args);
  if (typeof pkg === "number") return report.finish(pkg);
  if (await reportPackageProblems(report, pkg, pkg.problems, args))
    return report.finish(EXIT_HD_FAILURE);
  const executable = chosenExecutable(pkg, args.name, report);
  if (!executable) return report.finish(EXIT_HD_FAILURE);
  const loaded = await loadSource(
    { ...args, file: shownPath(pkg, executable.path, args) },
    { report, profile: args.profile, release: args.release, linkTests: false },
    {
      root: shownRoot(pkg, args),
      path: executable.path,
      files: pkg.files,
      programs: pkg.executables.map(({ path }) => path),
    },
  );
  if (typeof loaded === "number") return report.finish(loaded);
  return report.finish(
    await execute(loaded, io, {
      kind: "run",
      pendingFirstPoll: args.pendingFirstPoll,
      release: args.release,
      profile: args.profile,
    }),
  );
}

/**
 * The executable `hd run [NAME]` runs (spec/cli/command-line.md#r-cli.run.name,
 * spec/cli/command-line.md#r-cli.run.default.one), or undefined after
 * reporting why there is none.
 */
function chosenExecutable(
  pkg: LocalPackage,
  name: string | undefined,
  report: Report,
): Executable | undefined {
  const names = pkg.executables.map((executable) => executable.name);
  if (name !== undefined) {
    // NAME names an executable or a task (cli.run.name); no name names both
    // (cli.task.name-clash).
    const programs = [...pkg.executables, ...pkg.tasks];
    const named = programs.find((program) => program.name === name);
    const listed =
      programs.length === 0
        ? "it has none"
        : `it has ${programs.map((program) => program.name).join(", ")}`;
    if (!named)
      report.commandError(
        `hd run: package '${pkg.name}' has no executable or task named '${name}'; ${listed}`,
      );
    return named;
  }
  if (pkg.executables.length === 1) return pkg.executables[0];
  report.commandError(
    pkg.executables.length === 0
      ? `hd run: package '${pkg.name}' has no executable; add src/main.hd, or an [[executable]] table to hd.toml`
      : `hd run: package '${pkg.name}' has several executables, ${names.join(", ")}; name one, as in hd run ${names[0]}`,
  );
  return undefined;
}

export interface TestArgs extends CommandEnvironment {
  /** FILE; absent for the whole package. */
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
  /** The suspending function that stays pending, with the cancellation-cleanup scenario. */
  readonly pendingFunction?: string;
  readonly testLayout?: TestLayout;
  readonly packageTree?: PackageTree;
}

/** `hd test [FILE]`: runs the test cases of the package, or of FILE. */
export async function testCommand(args: TestArgs, io: CommandIo): Promise<number> {
  const report = new Report(args.format, io, { stream: "stdout", summary: true });
  return report.finish(await test(args, io, report));
}

async function test(args: TestArgs, io: CommandIo, report: Report): Promise<number> {
  if (args.path === undefined) {
    const pkg = await commandPackage("test", report, args);
    if (typeof pkg === "number") return pkg;
    return testPackage(pkg, args, io, report);
  }
  if (await directoryWord("test", args.path, report, args)) return EXIT_HD_FAILURE;
  const placement = await placementOf(args.path, args.packageTree, args.testLayout, args);
  return testFile(args, args.path, io, report, placement, {
    quietWhenEmpty: false,
    processes: executableProcesses(placement?.package),
  });
}

async function testFile(
  args: TestArgs,
  file: string,
  io: CommandIo,
  report: Report,
  placement: PackagePlacement | undefined,
  options: { readonly quietWhenEmpty: boolean; readonly processes?: ProcessProvider },
): Promise<number> {
  const loaded = await loadSource(
    { file, format: args.format, cwd: args.cwd, specDir: args.specDir },
    { report, profile: args.profile, testLayout: args.testLayout, linkTests: true },
    placement,
  );
  if (typeof loaded === "number") return loaded;
  return execute(loaded, io, { kind: "test", ...args, ...options });
}

/**
 * Whole-package `hd test` (spec/cli/command-line.md#r-cli.package.whole):
 * first it builds the executables
 * (spec/cli/command-line.md#r-cli.test.builds-executables), then it runs the
 * test cases of each module under the source root, with its `tests:` block
 * and the test modules linked, and of each integration test program, in
 * the order of their paths (spec/cli/command-line.md#r-cli.json.test.order).
 * A run that registers no test case passes
 * (spec/cli/command-line.md#r-cli.test.package-empty).
 */
async function testPackage(
  pkg: LocalPackage,
  args: TestArgs,
  io: CommandIo,
  report: Report,
): Promise<number> {
  if (await reportPackageProblems(report, pkg, pkg.problems, args)) return EXIT_HD_FAILURE;
  const programs = pkg.executables.map(({ path }) => path);
  const reported = new Set<string>();
  const placement = (path: string): PackagePlacement => ({
    root: shownRoot(pkg, args),
    path,
    files: pkg.files,
    reported,
    programs,
  });
  // `hd test` always builds in the test profile, a checked build
  // (spec/cli/command-line.md#r-cli.profile.test).
  for (const executable of pkg.executables) {
    const loaded = await loadSource(
      { ...args, file: shownPath(pkg, executable.path, args) },
      { report, linkTests: false },
      placement(executable.path),
    );
    if (typeof loaded === "number") return loaded;
    try {
      const result = analyze(loaded.source, { ...loaded.compileOptions, skipTestCode: true });
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
    } catch (error) {
      return reportFailure(loaded, error);
    }
  }
  const processes = executableProcesses(pkg);
  const targets = Object.keys(pkg.files)
    .filter(
      (path) =>
        path.startsWith(SOURCE_ROOT) ||
        // Shared test modules are no programs of their own
        // (spec/lang/10-modules.md#r-module.test.integration.shared).
        (path.startsWith(TEST_ROOT) && !path.slice(TEST_ROOT.length).includes("/")),
    )
    .sort();
  let status = 0;
  for (const path of targets) {
    const code = await testFile(args, shownPath(pkg, path, args), io, report, placement(path), {
      quietWhenEmpty: true,
      processes,
    });
    status = Math.max(status, code);
  }
  return status;
}

/** What `execute` runs: the entry point (`run`), or the test cases (`test`). */
type Execution =
  | {
      readonly kind: "run";
      readonly entry?: string;
      readonly pendingFirstPoll?: boolean;
      readonly release?: boolean;
      readonly profile?: RuntimeProfileName;
    }
  | ({
      readonly kind: "test";
      /** A whole-package run, where a module without test cases is no error. */
      readonly quietWhenEmpty: boolean;
      /** The `Process` of an integration test: the package's executables. */
      readonly processes?: ProcessProvider;
    } & TestArgs);

async function execute(loaded: LoadedSource, io: CommandIo, execution: Execution): Promise<number> {
  const { file, path, source, linked, placement, reporter } = loaded;
  const command = execution.kind;
  const entryName = (command === "run" ? execution.entry : undefined) ?? "main";
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
      console: (text) => io.out(text),
      consoleError: (text) => io.err(text),
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
      hostCapabilities: loaded.compileOptions.hostCapabilities,
      parse: loaded.parseOptions,
      integrationTest: loaded.compileOptions.integrationTest,
      // `hd test` always runs a checked build (spec/cli/command-line.md#r-cli.profile.test).
      release: command === "run" ? (execution.release ?? false) : false,
      // An integration test's `Process` runs the package's executables
      // (spec/cli/command-line.md#r-cli.test.process).
      hostSuspensionInvoke: (call) =>
        isProcessCall(call) && loaded.compileOptions.integrationTest
          ? (test?.processes ?? executableProcesses(undefined))(call)
          : (runtimeProfile?.invoke?.(call) ?? { pending: false }),
      hostSuspensionPending: execution.pendingFirstPoll
        ? pendingFirstPoll
        : runtimeProfile?.pending,
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
    // `hd test` runs the test cases, and never the entry point, so neither
    // its output nor its outcome counts as a test (cli.test.*); `hd run` and
    // `hd FILE` run the entry point, and `--entry` names any exported
    // function for `hd FILE`.
    // A test case with the `ignore` option is selected too: the runner
    // reports it as ignored without running it
    // (spec/lang/10-modules.md#r-module.testing.option.ignore).
    const selected = compilation.hir.functions.filter((declaration) => {
      if (command === "test")
        return /^\$test\.\d+$/.test(declaration.name) && inFileModule(declaration);
      if (explicitEntry) return declaration.name === entryName;
      // A non-`pub` `main` is not an entry point; implementation tests may
      // still run it by naming it explicitly.
      return declaration.entry === true;
    });
    // `hd test FILE` is an error when FILE registers no test case, even if it
    // has an entry point (spec/cli/command-line.md#r-cli.test.file-empty). The
    // modules of a whole-package `hd test` stay quiet: a run that registers
    // none passes (cli.test.package-empty).
    if (test && !test.quietWhenEmpty && selected.length === 0) {
      reporter.noTestCases();
      return EXIT_HD_FAILURE;
    }
    if (selected.length === 0 && test) return 0;
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
      snapshots.check,
      test && {
        record: (name, outcome, message) => loaded.output.test(name, outcome, message),
        // With `--format json` every test case reports (cli.json.test), so a
        // failure does not stop the run.
        keepGoing: test.format === "json",
      },
    );
    if (outcome.kind === "exit") return outcome.code;
    if (outcome.kind === "failed") {
      reporter.entryError(outcome.subject, outcome.outcome);
      return 1;
    }
    if (test) {
      if (test.format === "text" && (outcome.count > 0 || !test.quietWhenEmpty))
        io.out(`${file}: ${outcome.count} passed`);
      return loaded.output.counts.failed > 0 ? 1 : 0;
    }
    if (outcome.result !== undefined) io.out(outcome.result);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}
