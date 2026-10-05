// What the commands that compile a FILE share: finding FILE's package,
// linking it, and reporting diagnostics against the file they point into.

import { existsSync, statSync } from "node:fs";
import { readdir, readFile, stat } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";

import type { RUNTIME_PROFILE_NAMES } from "../cli-args.ts";
import type { CompileOptions } from "../compiler.ts";
import { DiagnosticReporter, type OutputFormat, type Report } from "../diagnostic-report.ts";
import { DiagnosticError, physicalSpan, sourceDocument, type Diagnostic } from "../diagnostics.ts";
import { linkPackage, SOURCE_ROOT, TEST_ROOT, type LinkedPackage } from "../package.ts";
import type { PackageDiagnostic } from "../package.ts";
import type { ParseOptions } from "../parser/index.ts";
import { RuntimePanicError, UnsupportedAtRunTimeError } from "../runtime-panic.ts";
import { loadSpecIndex } from "../spec-index.ts";
import { RUNTIME_PROFILES } from "./profiles.ts";
import { workingDirectory, type CommandEnvironment } from "./io.ts";

export type RuntimeProfileName = (typeof RUNTIME_PROFILE_NAMES)[number];
export type TestLayout = "test-module" | "integration";

/** `--package-tree DIR --package-path PATH`: FILE takes PATH in the package tree DIR. */
export interface PackageTree {
  readonly tree: string;
  readonly path: string;
}

/** The arguments every command that compiles a FILE takes. */
export interface SourceArgs extends CommandEnvironment {
  readonly file: string;
  readonly format: OutputFormat;
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
   * Diagnostics already printed, shared by the module runs of `hd test DIR`,
   * so that an error in a module that several others link prints once.
   */
  readonly reported?: Set<string>;
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
  const fileSource = await readFile(path, "utf8");
  // In a package, FILE joins the package's files; the linker joins the
  // modules into one program (src/package.ts).
  const treeFiles = placement && { ...placement.files, [placement.path]: fileSource };
  const linked =
    placement && treeFiles
      ? linkPackage(treeFiles, placement.path, { tests: options.linkTests })
      : undefined;
  const source = linked?.source ?? fileSource;
  const profile = options.profile ? RUNTIME_PROFILES[options.profile] : undefined;
  // A `*_test.hd` file is a test module (spec/lang/10-modules.md#test-modules), as
  // is a file that `--test-layout` places as one (spec/conformance, Test
  // Layouts); the prototype has no separate integration test view.
  const parseOptions = linked
    ? { joinedModules: true as const, initGroupStarts: linked.initGroups }
    : path.endsWith("_test.hd") || options.testLayout !== undefined
      ? { testModule: true }
      : {};
  // FILE is an entry module unless it is a test module
  // (spec/lang/10-modules.md#r-module.init.entry-module.selected).
  const compileOptions: CompileOptions = {
    hostCapabilities: profile?.hostCapabilities,
    parse: parseOptions,
    entryModule: !("testModule" in parseOptions),
    release: options.release ?? false,
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
  const report = (diagnostic: Diagnostic | PackageDiagnostic): void => {
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
    if (!linked.source) return 1;
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
 * Reports a compile error, a runtime panic, or an unsupported feature and
 * returns exit status 1. Any other error is an internal error and is thrown.
 */
export function reportFailure(loaded: LoadedSource, error: unknown): number {
  if (error instanceof DiagnosticError) {
    for (const diagnostic of error.diagnostics) loaded.report(diagnostic);
    return 1;
  }
  if (error instanceof RuntimePanicError) {
    loaded.reporter.runtimePanic(error.code, error.detail);
    return 1;
  }
  if (error instanceof UnsupportedAtRunTimeError) {
    loaded.reporter.unsupported(error.code, error.message);
    return 1;
  }
  throw error;
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

export async function isDirectory(path: string): Promise<boolean> {
  return (await stat(path).catch(() => undefined))?.isDirectory() ?? false;
}

function isDirectorySync(path: string): boolean {
  return statSync(path, { throwIfNoEntry: false })?.isDirectory() ?? false;
}

/** Whether `directory` is a package root: it holds `hd.toml` or `src/`. */
export function isPackageRoot(directory: string): boolean {
  return existsSync(join(directory, "hd.toml")) || isDirectorySync(join(directory, SOURCE_ROOT));
}

/**
 * The root of the package that holds the directory `start`: the nearest
 * directory at or above it with `hd.toml`, else the parent of the nearest
 * `src/` (or of a `tests/` beside a `src/`). Undefined outside any package.
 */
export function enclosingPackageRoot(start: string): string | undefined {
  const ancestors: string[] = [];
  for (let directory = resolve(start); ; directory = dirname(directory)) {
    ancestors.push(directory);
    if (dirname(directory) === directory) break;
  }
  const manifest = ancestors.find((directory) => existsSync(join(directory, "hd.toml")));
  if (manifest) return manifest;
  const root = ancestors.find((directory) => {
    const name = basename(directory);
    return (
      name === SOURCE_ROOT.slice(0, -1) ||
      (name === TEST_ROOT.slice(0, -1) && isDirectorySync(join(dirname(directory), SOURCE_ROOT)))
    );
  });
  return root && dirname(root);
}

/** A package's `.hd` files under `src/` and `tests/`, keyed by package path. */
export async function packageFiles(root: string): Promise<Record<string, string>> {
  const files: Record<string, string> = {};
  for (const prefix of [SOURCE_ROOT, TEST_ROOT]) {
    const directory = join(root, prefix);
    if (!(await isDirectory(directory))) continue;
    for (const [path, text] of Object.entries(await packageTreeFiles(directory)))
      files[`${prefix}${path}`] = text;
  }
  return files;
}

/**
 * FILE's place in its enclosing package, when FILE is a package file under
 * `src/` or `tests/`; undefined for a lone file, which compiles on its own.
 */
async function enclosingPlacement(
  file: string,
  cwd: string,
): Promise<PackagePlacement | undefined> {
  const path = resolve(cwd, file);
  const root = enclosingPackageRoot(dirname(path));
  if (root === undefined) return undefined;
  const packagePath = relative(root, path).split(sep).join("/");
  if (!packagePath.startsWith(SOURCE_ROOT) && !packagePath.startsWith(TEST_ROOT)) return undefined;
  // Diagnostics name the package's other files the way FILE was named.
  const shown = isAbsolute(file) ? root : relative(cwd, root) || ".";
  return { root: shown, path: packagePath, files: await packageFiles(root) };
}

/**
 * FILE's package for a command that links one: the tree `--package-tree`
 * names, else the package that holds FILE. `--test-layout` turns linking off.
 * Relative paths resolve against `environment`'s directory.
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
      files: await packageTreeFiles(resolve(cwd, packageTree.tree)),
    };
  return testLayout === undefined ? enclosingPlacement(file, cwd) : undefined;
}
