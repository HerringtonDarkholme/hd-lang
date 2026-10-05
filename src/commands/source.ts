// What the commands that compile a FILE share: finding FILE's package,
// linking it, and reporting diagnostics against the file they point into.

import { readFile } from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";

import { SINGLE_FILE_USE } from "../checker/standard-uses.ts";
import type { CompileOptions } from "../compiler.ts";
import { DiagnosticReporter, type OutputFormat, type Report } from "../diagnostic-report.ts";
import { DiagnosticError, physicalSpan, sourceDocument, type Diagnostic } from "../diagnostics.ts";
import { linkPackage, SOURCE_ROOT, TASK_ROOT, TEST_ROOT, type LinkedPackage } from "../package.ts";
import type { PackageDiagnostic } from "../package.ts";
import type { ParseOptions } from "../parser/index.ts";
import { RuntimePanicError, UnsupportedAtRunTimeError } from "../runtime-panic.ts";
import { loadSpecIndex } from "../spec-index.ts";
import {
  hdFilesUnder,
  MANIFEST_FILE,
  packageMode,
  type LocalPackage,
  type PackageProblem,
} from "./package-mode.ts";
import { RUNTIME_PROFILES, type RUNTIME_PROFILE_NAMES } from "./profiles.ts";
import { EXIT_HD_FAILURE, workingDirectory, type CommandEnvironment } from "./io.ts";

export type RuntimeProfileName = (typeof RUNTIME_PROFILE_NAMES)[number];
export type TestLayout = "test-module" | "integration";

/** FILE takes the package path `path` in the package tree `tree` (conformance Package Trees). */
export interface PackageTree {
  readonly tree: string;
  readonly path: string;
}

/** The arguments every command that compiles a FILE takes. */
export interface SourceArgs extends CommandEnvironment {
  readonly file: string;
  readonly format: OutputFormat;
  /** The source text, when it comes from standard input rather than FILE. */
  readonly text?: string;
}

/**
 * FILE's place in a package (spec/conformance/README.md, Package Trees): the
 * package root, the package path FILE takes, and the package's files.
 */
export interface PackagePlacement {
  /** The package root, as diagnostics in its other files name it. */
  readonly root: string;
  readonly path: string;
  readonly files: Readonly<Record<string, string>>;
  /**
   * Diagnostics already printed, shared by the module runs of a
   * whole-package command, so that an error in a module that several others
   * link prints once.
   */
  readonly reported?: Set<string>;
  /** The executables' entry modules, each its own program (LinkOptions.programs). */
  readonly programs?: readonly string[];
  /** The package, in package mode; absent for a conformance package tree. */
  readonly package?: LocalPackage;
}

/** A FILE ready to compile: its source, after linking, and where its diagnostics go. */
export interface LoadedSource {
  readonly file: string;
  /** FILE's absolute path. */
  readonly path: string;
  readonly source: string;
  readonly parseOptions: ParseOptions;
  readonly compileOptions: CompileOptions;
  readonly linked?: LinkedPackage;
  readonly placement?: PackagePlacement;
  readonly reporter: DiagnosticReporter;
  /** The command's output: its test results and summary counts. */
  readonly output: Report;
  readonly report: (diagnostic: Diagnostic | PackageDiagnostic) => void;
}

interface LoadOptions {
  /** Where the diagnostics go, and what the command's summary counts. */
  readonly report: Report;
  readonly profile?: RuntimeProfileName;
  readonly testLayout?: TestLayout;
  /** `--release`: integer overflow wraps (spec/cli/command-line.md#r-cli.profile.release). */
  readonly release?: boolean;
  /** Link the test modules of FILE's package too. */
  readonly linkTests: boolean;
  /**
   * FILE is a library module that a whole-package command checks on its own,
   * not an entry module (spec/lang/10-modules.md#r-module.init.entry-module.selected).
   */
  readonly library?: boolean;
  /**
   * The note `hd FILE` adds to a package use's error when FILE lies in a
   * package (spec/cli/command-line.md#r-cli.file.in-package).
   */
  readonly singleFileNote?: string;
}

/**
 * Reads FILE and, in a package, links it with the package's other files.
 * Returns the exit status instead when linking fails; the link diagnostics
 * are already reported.
 */
export async function loadSource(
  args: SourceArgs,
  options: LoadOptions,
  placement?: PackagePlacement,
): Promise<LoadedSource | number> {
  const { file, format } = args;
  const path = resolve(workingDirectory(args), file);
  const fileSource = args.text ?? (await readFile(path, "utf8"));
  // `hd` rejects a package whose manifest is invalid
  // (spec/cli/command-line.md#r-cli.exit.hd-failure). A whole-package
  // command reports the manifest once itself, and leaves `package` unset.
  if (placement?.package) {
    const errors = placement.package.problems.filter(({ severity }) => severity === "error");
    if (await reportPackageProblems(options.report, placement.package, errors, args))
      return EXIT_HD_FAILURE;
  }
  // In a package, FILE joins the package's files; the linker joins the
  // modules into one program (src/package.ts).
  const treeFiles = placement && { ...placement.files, [placement.path]: fileSource };
  const linked =
    placement && treeFiles
      ? linkPackage(treeFiles, placement.path, {
          tests: options.linkTests,
          ...(placement.programs ? { programs: placement.programs } : {}),
        })
      : undefined;
  const source = linked?.source ?? fileSource;
  const profile = options.profile ? RUNTIME_PROFILES[options.profile] : undefined;
  // A `*_test.hd` file is a test module (spec/lang/10-modules.md#test-modules), as
  // is a file that the runner's test layout places as one (spec/conformance,
  // Test Layouts); the prototype has no separate integration test view.
  const parseOptions = linked
    ? { joinedModules: true as const, initGroupStarts: linked.initGroups }
    : path.endsWith("_test.hd") || options.testLayout !== undefined
      ? { testModule: true }
      : {};
  // FILE is an entry module unless it is a test module
  // (spec/lang/10-modules.md#r-module.init.entry-module.selected).
  // An integration test program, a file under the test root or one the
  // runner's integration layout places, takes `Process` in its test cases'
  // rows and may call `hd_run!` (spec/cli/command-line.md#r-cli.test.process).
  const integrationTest =
    options.testLayout === "integration" || (placement?.path.startsWith(TEST_ROOT) ?? false);
  const compileOptions: CompileOptions = {
    hostCapabilities: profile?.hostCapabilities,
    parse: parseOptions,
    entryModule: !("testModule" in parseOptions) && options.library !== true,
    release: options.release ?? false,
    ...(integrationTest ? { integrationTest } : {}),
  };
  const specIndex = format === "json" ? await loadSpecIndex(args.specDir) : undefined;
  // The JSON `file` is relative to the package root, and outside a package
  // the path as written (spec/cli/command-line.md#r-cli.json.diagnostic.file).
  const reporter = new DiagnosticReporter(
    options.report,
    file,
    fileSource,
    specIndex,
    placement?.path,
  );
  // A diagnostic in a package names the file it points into.
  const treeReporters = new Map<string, DiagnosticReporter>();
  const note = options.singleFileNote;
  const report = (diagnostic: Diagnostic | PackageDiagnostic): void => {
    if (
      note &&
      diagnostic.code === "unknown-module" &&
      diagnostic.message.includes(SINGLE_FILE_USE)
    )
      return reporter.diagnostic({ ...diagnostic, notes: [...(diagnostic.notes ?? []), note] });
    if (!linked || !placement || !treeFiles) return reporter.diagnostic(diagnostic);
    // A checker diagnostic from lib/std already owns its physical source;
    // package coordinates apply only to the joined package source.
    const document = sourceDocument(diagnostic.span);
    if (document) {
      const { line, column } = physicalSpan(diagnostic.span).start;
      const key = `${document.file}:${line}:${column}: ${diagnostic.code}: ${diagnostic.message}`;
      if (placement.reported?.has(key)) return;
      placement.reported?.add(key);
      return reporter.diagnostic(diagnostic);
    }
    const located = "path" in diagnostic ? diagnostic : linked.locate(diagnostic);
    const { line, column } = located.span.start;
    const key = `${located.path}:${line}:${column}: ${located.code}: ${located.message}`;
    if (placement.reported?.has(key)) return;
    placement.reported?.add(key);
    if (located.path === placement.path) return reporter.diagnostic(located);
    let treeReporter = treeReporters.get(located.path);
    if (!treeReporter) {
      treeReporter = new DiagnosticReporter(
        options.report,
        join(placement.root, located.path),
        treeFiles[located.path] ?? "",
        specIndex,
        located.path,
      );
      treeReporters.set(located.path, treeReporter);
    }
    treeReporter.diagnostic(located);
  };
  if (linked) {
    for (const diagnostic of linked.diagnostics) report(diagnostic);
    if (!linked.source) return EXIT_HD_FAILURE;
  }
  return {
    file,
    path,
    source,
    parseOptions,
    compileOptions,
    linked,
    placement,
    reporter,
    output: options.report,
    report,
  };
}

/**
 * Reports a compile error, a runtime panic, or an unsupported feature. A
 * compile error and an unsupported feature are `hd` failures and return exit
 * status 101 (`cli.exit.hd-failure`); a panic returns 1, as a failed test
 * case does (`cli.exit.test-failure`). Any other error is an internal error
 * and is thrown.
 */
export function reportFailure(loaded: LoadedSource, error: unknown): number {
  if (error instanceof DiagnosticError) {
    for (const diagnostic of error.diagnostics) loaded.report(diagnostic);
    return EXIT_HD_FAILURE;
  }
  if (error instanceof RuntimePanicError) {
    loaded.reporter.runtimePanic(error.code, error.detail);
    return 1;
  }
  if (error instanceof UnsupportedAtRunTimeError) {
    loaded.reporter.unsupported(error.code, error.message);
    return EXIT_HD_FAILURE;
  }
  throw error;
}

/**
 * Reports a package's problems (src/commands/package-mode.ts) and returns
 * whether any is an error. Text names each file relative to the working
 * directory; JSON names it relative to the package root
 * (spec/cli/command-line.md#r-cli.json.diagnostic.file).
 */
export async function reportPackageProblems(
  report: Report,
  pkg: LocalPackage,
  problems: readonly PackageProblem[],
  environment: CommandEnvironment,
): Promise<boolean> {
  const specIndex = report.format === "json" ? await loadSpecIndex(environment.specDir) : undefined;
  const cwd = workingDirectory(environment);
  for (const problem of problems) {
    const full = join(pkg.root, problem.path);
    const reporter = new DiagnosticReporter(
      report,
      relative(cwd, full) || full,
      pkg.files[problem.path] ?? "",
      specIndex,
      problem.path,
    );
    if (problem.code === null) reporter.uncoded(problem.message, problem.line, problem.column);
    else {
      const position = { offset: 0, line: problem.line, column: problem.column };
      reporter.diagnostic({
        code: problem.code,
        message: problem.message,
        severity: problem.severity,
        span: { start: position, end: position },
      });
    }
  }
  return problems.some(({ severity }) => severity === "error");
}

/**
 * The package a whole-package command works on: the package of the working
 * directory (spec/cli/command-line.md#r-cli.package.whole). Outside any
 * package, or in a workspace, it reports an error and returns the exit
 * status instead (spec/cli/command-line.md#r-cli.run.package-only,
 * spec/cli/command-line.md#r-cli.file.check-test.no-file).
 */
export async function commandPackage(
  command: "build" | "check" | "run" | "test",
  report: Report,
  environment: CommandEnvironment,
  file?: string,
): Promise<LocalPackage | number> {
  const cwd = workingDirectory(environment);
  // The start directory is FILE's, or else the working directory
  // (spec/cli/command-line.md#r-cli.mode.start).
  const start = file === undefined ? cwd : dirname(resolve(cwd, file));
  const mode = await packageMode(start);
  if (mode.kind === "package") return mode.package;
  if (mode.kind === "workspace") {
    report.commandError(
      `hd ${command}: ${join(mode.root, MANIFEST_FILE)} is a workspace manifest, and the prototype does not support workspaces yet; run hd ${command} inside a member's directory`,
    );
    return EXIT_HD_FAILURE;
  }
  const where = `no ${MANIFEST_FILE} in ${start} or a directory above it`;
  report.commandError(
    command === "check" || command === "test"
      ? `hd ${command}: not in a package (${where}); pass a FILE, as in hd ${command} notes.hd, or create a package with hd new`
      : `hd ${command}: not in a package (${where}); create a package with hd new, or run one file with hd FILE`,
  );
  return EXIT_HD_FAILURE;
}

/** Where a whole-package command names a package file: relative to the working directory. */
export function shownPath(
  pkg: LocalPackage,
  path: string,
  environment: CommandEnvironment,
): string {
  return relative(workingDirectory(environment), join(pkg.root, path)) || path;
}

/** The package root as a whole-package command names it, relative to the working directory. */
export function shownRoot(pkg: LocalPackage, environment: CommandEnvironment): string {
  return relative(workingDirectory(environment), pkg.root) || ".";
}

/**
 * FILE's place in its package, when FILE lies in a package and under its
 * source root or test root; undefined for a file that `hd` compiles as a
 * single-file program (spec/cli/command-line.md#r-cli.package.no-root).
 */
async function enclosingPlacement(
  file: string,
  cwd: string,
): Promise<PackagePlacement | undefined> {
  const path = resolve(cwd, file);
  // The start directory is FILE's directory (spec/cli/command-line.md#r-cli.mode.start).
  const mode = await packageMode(dirname(path));
  if (mode.kind !== "package") return undefined;
  const pkg = mode.package;
  const packagePath = relative(pkg.root, path).split(sep).join("/");
  // The roots are the source root, the test root, and `tasks` (cli.package.no-root).
  if (![SOURCE_ROOT, TEST_ROOT, TASK_ROOT].some((root) => packagePath.startsWith(root)))
    return undefined;
  // Diagnostics name the package's other files the way FILE was named.
  const shown = isAbsolute(file) ? pkg.root : relative(cwd, pkg.root) || ".";
  return {
    root: shown,
    path: packagePath,
    files: pkg.files,
    programs: pkg.executables.map((executable) => executable.path),
    package: pkg,
  };
}

/**
 * FILE's package for a command that links one: the tree the conformance
 * runner's package tree option names, else the package that holds FILE. A
 * test layout turns linking off. Relative paths resolve against
 * `environment`'s directory.
 */
export async function placementOf(
  file: string,
  packageTree: PackageTree | undefined,
  testLayout: TestLayout | undefined,
  environment: CommandEnvironment,
): Promise<PackagePlacement | undefined> {
  const cwd = workingDirectory(environment);
  if (packageTree)
    return {
      root: resolve(cwd, packageTree.tree),
      path: packageTree.path,
      files: await hdFilesUnder(resolve(cwd, packageTree.tree)),
    };
  return testLayout === undefined ? enclosingPlacement(file, cwd) : undefined;
}
