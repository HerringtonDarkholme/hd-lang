// `hd parse` (`hd debug parse`), `hd check`, `hd debug hir`, and `hd build`:
// the commands that compile without running. `hd check` and `hd build` work
// on the whole package, or on one FILE (spec/cli/command-line.md#building-and-checking).

import { mkdir, writeFile } from "node:fs/promises";
import { basename, extname, join, resolve } from "node:path";

import { analyze, compileToWasm, compileToWat } from "../compiler.ts";
import { Report } from "../diagnostic-report.ts";
import { DiagnosticError } from "../diagnostics.ts";
import { LIB_FILE, SOURCE_ROOT, TEST_ROOT } from "../package.ts";
import { parse } from "../parser/index.ts";
import {
  EXIT_HD_FAILURE,
  workingDirectory,
  type CommandEnvironment,
  type CommandIo,
} from "./io.ts";
import { BUILD_DIRECTORY, unselectedMains, type LocalPackage } from "./package-mode.ts";
import {
  commandPackage,
  loadSource,
  placementOf,
  reportFailure,
  reportPackageProblems,
  shownPath,
  shownRoot,
  type PackageTree,
  type RuntimeProfileName,
  type SourceArgs,
  type TestLayout,
} from "./source.ts";

/** `hd parse FILE`: parses FILE and its `tests:` block, and prints `FILE: ok`. */
export async function parseCommand(args: SourceArgs, io: CommandIo): Promise<number> {
  const report = new Report(args.format, io);
  const loaded = await loadSource(args, { report, linkTests: false });
  if (typeof loaded === "number") return loaded;
  try {
    const result = parse(loaded.source, loaded.parseOptions);
    if (!result.program) throw new DiagnosticError(result.diagnostics);
    io.out(`${args.file}: ok`);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}

export interface CheckArgs extends CommandEnvironment {
  /** FILE; absent for the whole package. */
  readonly file?: string;
  readonly format: SourceArgs["format"];
  /** `--tests`: also check the test code (spec/cli/command-line.md#r-cli.check.tests). */
  readonly tests: boolean;
  /** `--all`: also the test code and the tasks (spec/cli/command-line.md#r-cli.check.all). */
  readonly all?: boolean;
  readonly profile?: RuntimeProfileName;
  readonly testLayout?: TestLayout;
  readonly packageTree?: PackageTree;
}

/**
 * `hd check [FILE]`: type-checks the package, or FILE, and prints its
 * warnings, then `NAME: ok`.
 */
export async function checkCommand(args: CheckArgs, io: CommandIo): Promise<number> {
  const report = new Report(args.format, io, { stream: "stdout", summary: true });
  if (args.file === undefined) {
    const pkg = await commandPackage("check", report, args);
    if (typeof pkg === "number") return report.finish(pkg);
    return report.finish(
      await compilePackage(pkg, args, io, report, {
        tests: args.tests || args.all === true,
        tasks: args.all === true,
      }),
    );
  }
  return report.finish(await check({ ...args, file: args.file }, io, report));
}

async function check(
  args: CheckArgs & { readonly file: string },
  io: CommandIo,
  report: Report,
): Promise<number> {
  const placement = await placementOf(args.file, args.packageTree, args.testLayout, args);
  const loaded = await loadSource(
    args,
    {
      report,
      profile: args.profile,
      testLayout: args.testLayout,
      linkTests: args.tests || args.all === true,
    },
    placement,
  );
  if (typeof loaded === "number") return loaded;
  try {
    // `hd check` checks test code only with `--tests` (Testing T42).
    const result = analyze(loaded.source, {
      ...loaded.compileOptions,
      skipTestCode: !(args.tests || args.all === true),
    });
    if (!result.hir) throw new DiagnosticError(result.diagnostics);
    for (const diagnostic of result.diagnostics) loaded.report(diagnostic);
    if (args.format === "text") io.out(`${args.file}: ok`);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}

interface PackageCompilation {
  /** Also the test code (spec/cli/command-line.md#r-cli.check.tests). */
  readonly tests: boolean;
  /** `hd build`: write each executable's Wasm in this profile's directory. */
  readonly build?: { readonly release: boolean };
  /** Also the tasks (spec/cli/command-line.md#r-cli.check.all). */
  readonly tasks?: boolean;
}

/**
 * Whole-package `hd check` and `hd build`: the library and the executables,
 * and with `tests` the test code (spec/cli/command-line.md#r-cli.check.default).
 * Each executable, each library module, and with `tests` each test module
 * and integration test program, compiles as the entry of its own link. A
 * module that an earlier link joined is already checked, so it gets no
 * link of its own.
 */
async function compilePackage(
  pkg: LocalPackage,
  environment: CommandEnvironment & { readonly format: SourceArgs["format"] },
  io: CommandIo,
  report: Report,
  options: PackageCompilation,
): Promise<number> {
  const problems = [...pkg.problems, ...unselectedMains(pkg)];
  if (await reportPackageProblems(report, pkg, problems, environment)) return EXIT_HD_FAILURE;
  const programs = pkg.executables.map(({ path }) => path);
  const paths = Object.keys(pkg.files).sort();
  const isTestModule = (path: string): boolean => path.endsWith("_test.hd");
  const library = paths
    .filter((path) => path.startsWith(SOURCE_ROOT) && !programs.includes(path))
    .filter((path) => !isTestModule(path))
    .sort((left, right) => Number(right === LIB_FILE) - Number(left === LIB_FILE));
  // Integration test programs are the files directly under the test root
  // (spec/lang/10-modules.md#r-module.test.integration.program).
  const testCode = options.tests
    ? paths.filter(
        (path) =>
          (path.startsWith(SOURCE_ROOT) && isTestModule(path)) ||
          (path.startsWith(TEST_ROOT) && !path.slice(TEST_ROOT.length).includes("/")),
      )
    : [];
  const linked = new Set<string>();
  const reported = new Set<string>();
  const written: string[] = [];
  let status = 0;
  const tasks = options.tasks ? pkg.tasks.map(({ path }) => path) : [];
  for (const path of [...programs, ...library, ...testCode, ...tasks]) {
    if (linked.has(path)) continue;
    const executable = pkg.executables.find((candidate) => candidate.path === path);
    const loaded = await loadSource(
      { ...environment, file: shownPath(pkg, path, environment), format: environment.format },
      {
        report,
        linkTests: options.tests,
        release: options.build?.release ?? false,
        library: library.includes(path),
      },
      { root: shownRoot(pkg, environment), path, files: pkg.files, reported, programs },
    );
    if (typeof loaded === "number") {
      status = Math.max(status, loaded);
      linked.add(path);
      continue;
    }
    for (const module of loaded.linked?.modules ?? []) linked.add(module.path);
    try {
      const compileOptions = { ...loaded.compileOptions, skipTestCode: !options.tests };
      if (options.build && executable) {
        const compilation = await compileToWasm(loaded.source, compileOptions);
        for (const diagnostic of compilation.diagnostics) loaded.report(diagnostic);
        const profile = options.build.release ? "release" : "debug";
        const directory = join(pkg.root, BUILD_DIRECTORY, profile);
        await mkdir(directory, { recursive: true });
        const output = join(directory, `${executable.name}.wasm`);
        await writeFile(output, compilation.bytes);
        written.push(
          shownPath(pkg, join(BUILD_DIRECTORY, profile, `${executable.name}.wasm`), environment),
        );
        continue;
      }
      const result = analyze(loaded.source, compileOptions);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const diagnostic of result.diagnostics) loaded.report(diagnostic);
    } catch (error) {
      status = Math.max(status, reportFailure(loaded, error));
    }
  }
  if (status === 0 && environment.format === "text") {
    for (const output of written) io.out(output);
    if (!options.build) io.out(`${pkg.name}: ok`);
  }
  return status;
}

export interface HirArgs extends SourceArgs {
  readonly profile?: RuntimeProfileName;
}

/** `hd debug hir FILE`: prints FILE's checked HIR as JSON. */
export async function hirCommand(args: HirArgs, io: CommandIo): Promise<number> {
  const report = new Report(args.format, io);
  const loaded = await loadSource(args, { report, profile: args.profile, linkTests: false });
  if (typeof loaded === "number") return loaded;
  try {
    const result = analyze(loaded.source, loaded.compileOptions);
    if (!result.hir) throw new DiagnosticError(result.diagnostics);
    io.out(JSON.stringify(result.hir, null, 2));
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}

export interface BuildArgs extends CommandEnvironment {
  /** FILE; absent for the whole package. */
  readonly file?: string;
  readonly format: SourceArgs["format"];
  /** `--wat`: print the WebAssembly text instead of writing `NAME.wasm`. */
  readonly wat: boolean;
  /** `--release`: integer overflow wraps instead of panicking. */
  readonly release?: boolean;
  readonly profile?: RuntimeProfileName;
}

/**
 * `hd build [FILE]`: writes each executable of the package to
 * `build/debug/NAME.wasm` (`build/release/` with `--release`) and prints
 * its path. With FILE in a package, it writes FILE's module, linked with the
 * package, to `NAME.wasm` in the current directory, or with `--wat` prints
 * the WAT without assembling it.
 */
export async function buildCommand(args: BuildArgs, io: CommandIo): Promise<number> {
  // `--wat` prints the module, so it has no JSON summary after it.
  const report = new Report(args.format, io, { stream: "stdout", summary: !args.wat });
  // Outside any package, `hd build` is an error, with a FILE too
  // (spec/cli/command-line.md#r-cli.run.package-only).
  const pkg = await commandPackage("build", report, args, args.file);
  if (typeof pkg === "number") return report.finish(pkg);
  if (args.file === undefined) {
    if (args.wat) {
      report.commandError("hd build: --wat needs a FILE, whose module it prints");
      return report.finish(EXIT_HD_FAILURE);
    }
    return report.finish(
      await compilePackage(pkg, args, io, report, {
        tests: false,
        build: { release: args.release ?? false },
      }),
    );
  }
  return report.finish(await build({ ...args, file: args.file }, io, report));
}

async function build(
  args: BuildArgs & { readonly file: string },
  io: CommandIo,
  report: Report,
): Promise<number> {
  const placement = await placementOf(args.file, undefined, undefined, args);
  const loaded = await loadSource(
    args,
    { report, profile: args.profile, release: args.release, linkTests: false },
    placement,
  );
  if (typeof loaded === "number") return loaded;
  try {
    if (args.wat) {
      io.out(compileToWat(loaded.source, loaded.compileOptions).wat);
      return 0;
    }
    const result = await compileToWasm(loaded.source, loaded.compileOptions);
    const name = `${basename(loaded.path, extname(loaded.path))}.wasm`;
    const output = resolve(workingDirectory(args), name);
    await writeFile(output, result.bytes);
    if (args.format === "text") io.out(output);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}
