// `hd FILE`, `hd run`, and `hd test`: the commands that compile a program
// and run it.

import { stat, writeFile } from "node:fs/promises";
import { dirname, relative, resolve, sep } from "node:path";

import { analyze, instantiate } from "../compiler.ts";
import { Report, type TestOutcome } from "../diagnostic-report.ts";
import type { DocTest } from "../doc-tests.ts";
import { DiagnosticError } from "../diagnostics.ts";
import type { HirFunction } from "../hir.ts";
import { SOURCE_ROOT, TASK_ROOT, TEST_ROOT } from "../package.ts";
import { propertyRun } from "../property-tests.ts";
import { regressionStore, snapshotModule, snapshotRun } from "../snapshots.ts";
import { runSelected } from "../test-runner.ts";
import { defaultProfileAnswer, inputLines, type DefaultProfileHost } from "./default-profile.ts";
import {
  combinedStatus,
  judgeCompileFail,
  loadDocTest,
  moduleDocTests,
  snapshotRewrite,
  testsBlockLine,
  type DocTestModule,
  type TestTally,
} from "./doc-tests.ts";
import {
  EXIT_HD_FAILURE,
  variablesOf,
  workingDirectory,
  type CommandEnvironment,
  type CommandIo,
} from "./io.ts";
import { packageMode, type Executable, type LocalPackage } from "./package-mode.ts";
import { executableProcesses, isProcessCall, type ProcessProvider } from "./processes.ts";
import { integrationTestHost, TEST_TEMP_DIRS } from "./test-host.ts";
import {
  exportedFunction,
  RUNTIME_PROFILES,
  pendingFirstPoll,
  runRuntimeScenario,
  type RuntimeScenario,
} from "./profiles.ts";
import {
  commandPackages,
  loadSource,
  type CommandPackages,
  type MemberSelection,
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
    `hd ${command}: '${word}' is a directory, which is neither a NAME nor a FILE; to work on a workspace member, pass -p NAME, or run hd ${command} inside that directory`,
  );
  return true;
}

export interface FileArgs extends SourceArgs {
  /** The runner's `entry` option: run this exported function instead, and print its result. */
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
      host: defaultHost(args, args.file, workingDirectory(args)),
    }),
  );
}

/**
 * What the default profile reads for a run of `program` in `directory`
 * (spec/cli/command-line.md#host-capabilities).
 */
function defaultHost(
  environment: CommandEnvironment & { readonly programArguments?: readonly string[] },
  program: string,
  directory: string,
): RunHost {
  return {
    program,
    arguments: environment.programArguments ?? [],
    variables: variablesOf(environment),
    workingDirectory: directory,
    ...(environment.readInput ? { readInput: environment.readInput } : {}),
  };
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

export interface RunArgs extends CommandEnvironment, MemberSelection {
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
  const selected = await commandPackages(
    "run",
    report,
    args,
    args.members ? { members: args.members } : {},
  );
  if (typeof selected === "number") return report.finish(selected);
  const chosen = chosenProgram(selected, args.name, report);
  if (!chosen) return report.finish(EXIT_HD_FAILURE);
  const [pkg, executable] = chosen;
  if (await reportPackageProblems(report, pkg, pkg.problems, args))
    return report.finish(EXIT_HD_FAILURE);
  const loaded = await loadSource(
    { ...args, file: shownPath(pkg, executable.path, args) },
    { report, profile: args.profile, release: args.release, linkTests: false },
    {
      root: shownRoot(pkg, args),
      path: executable.path,
      files: pkg.files,
      programs: pkg.executables.map(({ path }) => path),
      ...(pkg.dependencies ? { dependencies: pkg.dependencies } : {}),
    },
  );
  if (typeof loaded === "number") return report.finish(loaded);
  // A task runs in its package directory, and an executable where `hd run`
  // ran (spec/cli/command-line.md#working-directory).
  const task = pkg.tasks.includes(executable);
  return report.finish(
    await execute(loaded, io, {
      kind: "run",
      pendingFirstPoll: args.pendingFirstPoll,
      release: args.release,
      profile: args.profile,
      host: defaultHost(args, executable.name, task ? pkg.root : workingDirectory(args)),
    }),
  );
}

/**
 * The package and the program `hd run [NAME]` runs, or undefined after
 * reporting why there is none. In workspace mode NAME names the one member
 * program of that name (spec/cli/command-line.md#r-cli.workspace.run-name).
 * With one member that `-p` selects, `hd run` chooses as in package mode,
 * as Cargo's `cargo run -p` does.
 */
function chosenProgram(
  selected: CommandPackages,
  name: string | undefined,
  report: Report,
): [LocalPackage, Executable] | undefined {
  const { packages } = selected;
  if (!selected.workspace || (packages.length === 1 && name === undefined)) {
    const pkg = packages[0]!;
    const executable = chosenExecutable(pkg, name, report);
    return executable && [pkg, executable];
  }
  const programs = (pkg: LocalPackage): Executable[] => [...pkg.executables, ...pkg.tasks];
  if (name === undefined) {
    // A bare `hd run` names no program (cli.workspace.run-bare).
    const listed = packages.map(
      (pkg) =>
        `  ${pkg.name}: ${
          programs(pkg)
            .map((program) => program.name)
            .join(", ") || "none"
        }`,
    );
    report.commandError(
      `hd run: in a workspace, name the executable or task to run, as in hd run NAME; the members have:\n${listed.join("\n")}`,
    );
    return undefined;
  }
  const owners = packages.flatMap((pkg): [LocalPackage, Executable][] => {
    const program = programs(pkg).find((candidate) => candidate.name === name);
    return program ? [[pkg, program]] : [];
  });
  if (owners.length === 1) return owners[0];
  report.commandError(
    owners.length === 0
      ? // No member has one (cli.workspace.run-missing).
        `hd run: no member of the workspace has an executable or task named '${name}'; the members are ${packages.map((pkg) => pkg.name).join(", ")}`
      : // Several members have one (cli.workspace.run-ambiguous).
        `hd run: the members ${owners.map(([pkg]) => pkg.name).join(", ")} each have an executable or task named '${name}'; pick one with -p, as in hd run -p ${owners[0]![0].name} ${name}`,
  );
  return undefined;
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

export interface TestArgs extends CommandEnvironment, MemberSelection {
  /** FILE; absent for the whole package. */
  readonly path?: string;
  readonly format: SourceArgs["format"];
  /** `--update`: record snapshot files instead of failing on a difference. */
  readonly update: boolean;
  /** `--filter PATTERN`: run only the test cases whose name contains PATTERN (cli.test.filter). */
  readonly filter?: string;
  /** `--deny-skipped`: a skipped test case is a failure (cli.test.deny-skipped). */
  readonly denySkipped?: boolean;
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
  if (args.path === undefined || (args.members?.length ?? 0) > 0) {
    const selected = await commandPackages("test", report, args, {
      ...(args.path === undefined ? {} : { file: args.path }),
      ...(args.members ? { members: args.members } : {}),
    });
    if (typeof selected === "number") return selected;
    // In workspace mode, the tests of every member, or of the members `-p`
    // selects (spec/cli/command-line.md#r-cli.workspace.members).
    let status = 0;
    for (const pkg of selected.packages)
      status = Math.max(status, await testPackage(pkg, args, io, report));
    return status;
  }
  if (await directoryWord("test", args.path, report, args)) return EXIT_HD_FAILURE;
  const placement = await placementOf(args.path, args.packageTree, args.testLayout, args);
  return testFile(args, args.path, io, report, placement, {
    quietWhenEmpty: false,
    processes: executableProcesses(placement?.package, variablesOf(args)),
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
  const docs = moduleDocTests(placement, loaded.fileSource);
  if (docs.length === 0) return execute(loaded, io, { kind: "test", ...args, ...options });
  return testWithDocTests(args, loaded, docs, io, report, placement!, options);
}

/** How many times an update run reruns one doc test after rewriting a snapshot of it. */
const SNAPSHOT_REWRITES = 20;

/**
 * Runs a module's test cases and its doc tests, which `hd test` runs with
 * them (spec/cli/command-line.md#r-cli.test.doc.default): each doc test is a
 * program of its own (spec/lang/10-modules.md#r-module.test.doc.program).
 * Doc tests above the module's `tests:` block run before its test cases,
 * the others after them (spec/cli/command-line.md#r-cli.json.test.order).
 * The module's result line counts them all.
 */
async function testWithDocTests(
  args: TestArgs,
  loaded: LoadedSource,
  docs: readonly DocTest[],
  io: CommandIo,
  report: Report,
  placement: PackagePlacement,
  options: { readonly quietWhenEmpty: boolean; readonly processes?: ProcessProvider },
): Promise<number> {
  const tally: TestTally = { passed: 0, failed: 0, selected: 0, registered: docs.length };
  const module: DocTestModule = { ...args, report, file: loaded.file, placement };
  // `--filter` matches a doc test by its name (cli.test.doc.filter).
  const filter = args.filter;
  const selected = docs.filter(({ name }) => filter === undefined || name.includes(filter));
  tally.selected += selected.length;
  let moduleSource = loaded.fileSource;
  const runDoc = async (first: DocTest): Promise<number> => {
    if (first.errors.length > 0) return judgeCompileFail(module, first, moduleSource, tally);
    let test = first;
    for (let attempt = 0; ; attempt += 1) {
      const doc = await loadDocTest(module, test, moduleSource);
      if (typeof doc === "number") return doc;
      // An update run rewrites a failing snapshot in place, then runs the
      // doc test again (spec/cli/command-line.md#r-cli.test.doc.update).
      let rewritten: string | undefined;
      const intercept = (_name: string, outcome: TestOutcome, message: string): boolean => {
        if (outcome === "failed") rewritten = snapshotRewrite(test, message, moduleSource);
        return rewritten !== undefined;
      };
      const counts: TestTally = { passed: 0, failed: 0, selected: 0, registered: 0 };
      const status = await execute(doc, io, {
        kind: "test",
        ...args,
        ...options,
        quietWhenEmpty: true,
        tally: counts,
        ...(args.update && attempt < SNAPSHOT_REWRITES ? { keepGoing: true, intercept } : {}),
      });
      if (rewritten === undefined) {
        tally.passed += counts.passed;
        tally.failed += counts.failed;
        return status;
      }
      moduleSource = rewritten;
      await writeFile(loaded.path, moduleSource);
      const next = moduleDocTests(placement, moduleSource).find(({ name }) => name === test.name);
      if (!next) return status;
      test = next;
    }
  };
  const testsLine = loaded.file.endsWith("_test.hd") ? 0 : testsBlockLine(loaded.fileSource);
  const runModule = (): Promise<number> =>
    execute(loaded, io, { kind: "test", ...args, ...options, quietWhenEmpty: true, tally });
  const steps = [
    ...selected.filter(({ line }) => line < testsLine).map((test) => () => runDoc(test)),
    runModule,
    ...selected.filter(({ line }) => line >= testsLine).map((test) => () => runDoc(test)),
  ];
  // A text run ends a module's run at its first failed test case, as it does
  // for the module's own test cases; a module that does not compile ends it
  // too. A doc test that does not compile is a program of its own.
  const stopAtFailure = args.format === "text" && !args.update;
  let status = 0;
  for (const step of steps) {
    const result = await step();
    status = combinedStatus(status, result);
    if (step === runModule && result === EXIT_HD_FAILURE) return status;
    if (result === 1 && stopAtFailure) break;
  }
  // `hd test FILE` is an error when FILE registers no test case, or none
  // that `--filter` selects (cli.test.file-empty, cli.test.filter.none).
  if (!options.quietWhenEmpty && tally.selected === 0 && status === 0) {
    loaded.reporter.noTestCases(tally.registered > 0 ? filter : undefined);
    return EXIT_HD_FAILURE;
  }
  if (args.format === "text" && tally.passed + tally.failed > 0)
    io.out(
      `${loaded.file}: ${tally.passed} passed${tally.failed > 0 ? `, ${tally.failed} failed` : ""}`,
    );
  return tally.failed > 0 ? combinedStatus(status, 1) : status;
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
    ...(pkg.dependencies ? { dependencies: pkg.dependencies } : {}),
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
  const processes = executableProcesses(pkg, variablesOf(args));
  // An entry module, an executable's or a task's, without a `tests:` block
  // gets no test build (spec/cli/command-line.md#r-cli.test.tasks.no-tests).
  const entries = new Set([...pkg.executables, ...pkg.tasks].map(({ path }) => path));
  const untested = (path: string): boolean =>
    entries.has(path) && !/^tests:/m.test(pkg.files[path] ?? "");
  const targets = Object.keys(pkg.files)
    .filter(
      (path) =>
        path.startsWith(SOURCE_ROOT) ||
        // The `tests:` blocks of tasks and shared task modules
        // (spec/cli/command-line.md#r-cli.test.tasks).
        path.startsWith(TASK_ROOT) ||
        // Shared test modules are no programs of their own
        // (spec/lang/10-modules.md#r-module.test.integration.shared).
        (path.startsWith(TEST_ROOT) && !path.slice(TEST_ROOT.length).includes("/")),
    )
    .filter((path) => !untested(path))
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

/**
 * The default profile's view of a run, apart from standard input, which a
 * caller may supply as text (CommandEnvironment.readInput).
 */
type RunHost = Omit<DefaultProfileHost, "readLine"> & {
  readonly readInput?: () => Promise<string>;
};

/** What `execute` runs: the entry point (`run`), or the test cases (`test`). */
type Execution =
  | {
      readonly kind: "run";
      readonly entry?: string;
      readonly pendingFirstPoll?: boolean;
      readonly release?: boolean;
      readonly profile?: RuntimeProfileName;
      /** The default profile's providers (cli.host.default-profile). */
      readonly host: RunHost;
    }
  | ({
      readonly kind: "test";
      /** A whole-package run, where a module without test cases is no error. */
      readonly quietWhenEmpty: boolean;
      /** The `Process` of an integration test: the package's executables. */
      readonly processes?: ProcessProvider;
      /**
       * Counts the program's test cases instead of printing its result line,
       * for a module run with its doc tests (commands/doc-tests.ts).
       */
      readonly tally?: TestTally;
      /** Runs every test case after a failure; the default is with `--format json`. */
      readonly keepGoing?: boolean;
      /**
       * Sees each test case's result first, and keeps it out of the report
       * when it returns true, as an update run that rewrites a doc test's
       * snapshot does.
       */
      readonly intercept?: (name: string, outcome: TestOutcome, message: string) => boolean;
    } & TestArgs);

export async function execute(
  loaded: LoadedSource,
  io: CommandIo,
  execution: Execution,
): Promise<number> {
  const { file, path, source, linked, placement, reporter } = loaded;
  const command = execution.kind;
  const entryName = (command === "run" ? execution.entry : undefined) ?? "main";
  const explicitEntry = command === "run" && execution.entry !== undefined;
  const test = command === "test" ? execution : undefined;
  const scenario = test?.scenario;
  const runtimeProfile = execution.profile ? RUNTIME_PROFILES[execution.profile] : undefined;
  // Standard input is read only once the program asks for a line.
  let readLine: (() => string | undefined | null) | undefined;
  // An integration test case gets the default profile too, from the
  // package directory, with no arguments and a closed standard input
  // (spec/cli/command-line.md#r-cli.test.env.integration); a unit test case
  // gets no host provider (spec/lang/10-modules.md#r-module.testing.unit-row.anywhere).
  const host: DefaultProfileHost | undefined =
    execution.kind === "run"
      ? { ...execution.host, readLine: () => (readLine ??= inputLines())() }
      : loaded.compileOptions.integrationTest
        ? integrationTestHost(
            resolve(workingDirectory(execution), placement?.root ?? "."),
            // A doc test's program is its module's file (cli.test.env.args.program).
            loaded.docTest?.modulePath ?? placement?.path ?? file,
            variablesOf(execution),
          )
        : undefined;
  // A failing file gets its result line too, as a passing one does: the
  // cases that passed before the failure ended the run, and the failure.
  let running = false;
  let passedCases = 0;
  const failedLine = (): void => {
    if (test?.tally) {
      test.tally.passed += passedCases;
      test.tally.failed += 1;
    } else if (test?.format === "text") io.out(`${file}: ${passedCases} passed, 1 failed`);
  };
  // `hd test` keeps a test case's `dbg` lines and shows them only when it
  // fails; `hd run` writes them to standard error (spec/cli/command-line.md#debug-output).
  let debugLines: string[] = [];
  const showDebugLines = (): void => {
    for (const line of debugLines) io.err(line);
    debugLines = [];
  };
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
      // `hd test --format json` writes only JSON lines to stdout
      // (spec/cli/command-line.md#r-cli.json.lines.build), so a test body's
      // console output goes to stderr there.
      console: (text) => (test?.format === "json" ? io.err(text) : io.out(text)),
      consoleError: (text) => io.err(text),
      debugOutput: test ? (line) => debugLines.push(line) : (line) => io.err(line),
      debugLocation: loaded.compileOptions.debugLocation,
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
      docTest: loaded.compileOptions.docTest,
      testBuild: loaded.compileOptions.testBuild,
      // `hd test` always runs a checked build (spec/cli/command-line.md#r-cli.profile.test).
      release: command === "run" ? (execution.release ?? false) : false,
      // An integration test's `Process` runs the package's executables and
      // tasks (spec/cli/command-line.md#r-cli.test.process,
      // spec/cli/command-line.md#r-cli.test.process.tasks).
      hostSuspensionInvoke: (call) =>
        isProcessCall(call) && loaded.compileOptions.integrationTest
          ? (test?.processes ?? executableProcesses(undefined))(call)
          : ((host && defaultProfileAnswer(call, host)) ??
            runtimeProfile?.invoke?.(call) ?? { pending: false }),
      hostSuspensionPending: execution.pendingFirstPoll
        ? pendingFirstPoll
        : runtimeProfile?.pending,
    };
    const { instance, compilation } = await instantiate(source, instantiateOptions);
    scenarioInstance = instance;
    // Supplied standard input arrives as a whole, before a program that
    // reads lines runs.
    if (
      execution.kind === "run" &&
      execution.host.readInput &&
      compilation.hir.traits.some(({ standardName }) => standardName === "std.console.ConsoleInput")
    )
      readLine = inputLines(await execution.host.readInput());
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
      if (test?.format === "text") io.out(`${file}: 1 passed`);
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
    const registered = compilation.hir.functions.filter(
      (declaration) =>
        command === "test" && /^\$test\.\d+$/.test(declaration.name) && inFileModule(declaration),
    );
    const filter = test?.filter;
    const selected = compilation.hir.functions.filter((declaration) => {
      if (command === "test")
        return (
          registered.includes(declaration) &&
          (filter === undefined ||
            (declaration.testOptions?.name ?? declaration.name).includes(filter))
        );
      if (explicitEntry) return declaration.name === entryName;
      // A non-`pub` `main` is not an entry point; implementation tests may
      // still run it by naming it explicitly.
      return declaration.entry === true;
    });
    // `hd test FILE` is an error when FILE registers no test case, even if it
    // has an entry point (spec/cli/command-line.md#r-cli.test.file-empty). The
    // modules of a whole-package `hd test` stay quiet: a run that registers
    // none passes (cli.test.package-empty).
    // A filter that matches no test case of a named FILE is an error too
    // (spec/cli/command-line.md#r-cli.test.filter.none).
    if (test?.tally) {
      test.tally.registered += registered.length;
      test.tally.selected += selected.length;
    }
    if (test && !test.quietWhenEmpty && selected.length === 0) {
      reporter.noTestCases(registered.length > 0 ? filter : undefined);
      return EXIT_HD_FAILURE;
    }
    if (selected.length === 0 && test) return 0;
    // A module without an entry point runs its initialization and exits 0
    // (owner decision, batch 42); `--entry` must name a function.
    if (selected.length === 0 && !explicitEntry) return 0;
    if (selected.length === 0) throw new Error(`program has no exported ${entryName} function`);
    running = true;
    const outcome = await runSelected(
      selected,
      instance.exports,
      async () =>
        (await instantiate(source, { ...instantiateOptions, compilation })).instance.exports,
      (name, row) => {
        debugLines = [];
        snapshots.begin(name, row);
      },
      properties,
      snapshots.check,
      test && {
        record: (name, outcome, message) => {
          if (test.intercept?.(name, outcome, message)) return;
          if (outcome === "passed") passedCases += 1;
          if (outcome === "failed") {
            showDebugLines();
            if (test.tally) test.tally.failed += 1;
            // Text output names a failure that does not end the run.
            if (test.format === "text") reporter.entryError(`test "${name}"`, message);
          }
          loaded.output.test(name, outcome, message);
        },
        // With `--format json` every test case reports (cli.json.test), so a
        // failure does not stop the run.
        keepGoing: test.keepGoing ?? test.format === "json",
      },
      test && TEST_TEMP_DIRS,
    );
    if (outcome.kind === "exit") return outcome.code;
    if (outcome.kind === "failed") {
      showDebugLines();
      reporter.entryError(outcome.subject, outcome.outcome);
      failedLine();
      return 1;
    }
    if (test) {
      if (test.tally) test.tally.passed += outcome.count;
      else if (test.format === "text" && (outcome.count > 0 || !test.quietWhenEmpty))
        io.out(`${file}: ${outcome.count} passed`);
      // With --deny-skipped, a skipped test case is a failure (cli.test.deny-skipped).
      if (test.denySkipped && loaded.output.counts.skipped > 0) return 1;
      return loaded.output.counts.failed > 0 ? 1 : 0;
    }
    if (outcome.result !== undefined) io.out(outcome.result);
    return 0;
  } catch (error) {
    showDebugLines();
    const status = reportFailure(loaded, error);
    if (running) failedLine();
    return status;
  }
}
