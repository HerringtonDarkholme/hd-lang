// Package mode (spec/cli/command-line.md#package-mode): which package a
// command works on, read from the nearest `hd.toml`, with the package's
// source files and its executables
// (spec/cli/command-line.md#executables).

import { existsSync } from "node:fs";
import { readdir, readFile, stat } from "node:fs/promises";
import { basename, dirname, join, relative, resolve, sep } from "node:path";

import { readManifest } from "../manifest.ts";
import { LIB_FILE, MAIN_FILE, SOURCE_ROOT, TASK_ROOT, TEST_ROOT } from "../package.ts";
import { parse } from "../parser/index.ts";

/** A package's manifest file name (spec/lang/10-modules.md#r-module.manifest.file). */
export const MANIFEST_FILE = "hd.toml";

/**
 * The build directory, under the package directory: where `hd` writes its
 * build and cache output (spec/cli/command-line.md#r-cli.doc.dir). The
 * specification does not name it; the prototype uses `build`.
 */
export const BUILD_DIRECTORY = "build";

/** One executable of a package (spec/cli/command-line.md#r-cli.exe.table). */
export interface Executable {
  readonly name: string;
  /** The package path of its entry module, such as `src/main.hd`. */
  readonly path: string;
}

/**
 * A problem `hd` finds in a package before it compiles any module: in
 * `hd.toml`, or a source file's role. `code` is null where the
 * specification names no diagnostic code.
 */
export interface PackageProblem {
  /** The package path of the file, such as `hd.toml` or `src/helper.hd`. */
  readonly path: string;
  readonly line: number;
  readonly column: number;
  readonly code: string | null;
  readonly message: string;
  readonly severity: "error" | "warning";
}

/** A package that a command works on in package mode. */
export interface LocalPackage {
  /** The package directory, which holds `hd.toml`, as an absolute path. */
  readonly root: string;
  /** The `[package]` name, or the directory's name when the manifest has none. */
  readonly name: string;
  /** Every `.hd` file under `src/`, `tests/`, and `tasks/`, keyed by package path. */
  readonly files: Readonly<Record<string, string>>;
  /** The executables, declared or default, whose entry modules exist. */
  readonly executables: readonly Executable[];
  /** The tasks, each a file `tasks/NAME.hd` (spec/cli/command-line.md#r-cli.task.file). */
  readonly tasks: readonly Executable[];
  /** Errors in the manifest and in the executables it declares. */
  readonly problems: readonly PackageProblem[];
}

/** The mode a command works in (spec/cli/command-line.md#package-mode). */
export type PackageMode =
  | { readonly kind: "package"; readonly package: LocalPackage }
  | { readonly kind: "workspace"; readonly root: string }
  | { readonly kind: "outside" };

/** The nearest directory at or above `start` that holds `hd.toml`. */
export function nearestManifestDirectory(start: string): string | undefined {
  for (let directory = resolve(start); ; directory = dirname(directory)) {
    if (existsSync(join(directory, MANIFEST_FILE))) return directory;
    if (dirname(directory) === directory) return undefined;
  }
}

async function isDirectory(path: string): Promise<boolean> {
  return (await stat(path).catch(() => undefined))?.isDirectory() ?? false;
}

/** Every `.hd` file under `directory`, keyed by its path relative to it with `/`. */
export async function hdFilesUnder(directory: string): Promise<Record<string, string>> {
  const files: Record<string, string> = {};
  if (!(await isDirectory(directory))) return files;
  for (const entry of await readdir(directory, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith(".hd")) continue;
    const full = join(entry.parentPath, entry.name);
    files[relative(directory, full).split(sep).join("/")] = await readFile(full, "utf8");
  }
  return files;
}

/** A package's `.hd` files under `src/`, `tests/`, and `tasks/`, keyed by package path. */
export async function packageFiles(root: string): Promise<Record<string, string>> {
  const files: Record<string, string> = {};
  for (const prefix of [SOURCE_ROOT, TEST_ROOT, TASK_ROOT])
    for (const [path, text] of Object.entries(await hdFilesUnder(join(root, prefix))))
      files[`${prefix}${path}`] = text;
  return files;
}

/**
 * The mode of a command that starts in `start`
 * (spec/cli/command-line.md#r-cli.mode.package.nearest): package mode when
 * the nearest `hd.toml` declares a package, workspace mode when it is a
 * workspace manifest, and otherwise outside any package.
 */
export async function packageMode(start: string): Promise<PackageMode> {
  const root = nearestManifestDirectory(start);
  if (root === undefined) return { kind: "outside" };
  const text = await readFile(join(root, MANIFEST_FILE), "utf8");
  const read = readManifest(text);
  if ("manifest" in read && read.manifest.workspace && read.manifest.name === undefined)
    return { kind: "workspace", root };
  const files = await packageFiles(root);
  if (!("manifest" in read)) {
    const problems = read.errors.map(({ line, message }): PackageProblem => ({
      path: MANIFEST_FILE,
      line,
      column: 1,
      code: null,
      message,
      severity: "error",
    }));
    return {
      kind: "package",
      package: { root, name: basename(root), files, executables: [], tasks: [], problems },
    };
  }
  const { manifest } = read;
  const name = manifest.name ?? basename(root);
  const problems: PackageProblem[] = [];
  const executables: Executable[] = [];
  const problem = (line: number, code: string | null, message: string): void => {
    problems.push({ path: MANIFEST_FILE, line, column: 1, code, message, severity: "error" });
  };
  if (manifest.executables.length === 0) {
    // With no [[executable]] table, src/main.hd is the default executable,
    // named after the package (cli.exe.default-main, cli.exe.default-name).
    if (files[MAIN_FILE] !== undefined) executables.push({ name, path: MAIN_FILE });
  } else {
    for (const declared of manifest.executables) {
      const base = `${SOURCE_ROOT}${declared.module.split(".").join("/")}`;
      const path = [`${base}.hd`, `${base}/mod.hd`].find((candidate) => candidate in files);
      // An executable whose module names no module is an error (cli.exe.missing-module).
      if (path === undefined || path === LIB_FILE)
        problem(
          declared.line,
          "missing-entry-point",
          `executable '${declared.name}' names the module '${declared.module}', but the package has no ${base}.hd or ${base}/mod.hd`,
        );
      else executables.push({ name: declared.name, path });
    }
    // A src/main.hd that no table names is an error (cli.exe.main-unlisted).
    if (files[MAIN_FILE] !== undefined && !executables.some(({ path }) => path === MAIN_FILE))
      problem(
        manifest.packageLine,
        null,
        `src/main.hd is no executable, since hd.toml declares [[executable]] tables and none names module "main"; add one:\n  [[executable]]\n  name = "${name}"\n  module = "main"`,
      );
  }
  // Each file directly under `tasks` is a task named after it
  // (spec/cli/command-line.md#r-cli.task.file). A task and an executable
  // with one name are an error (spec/cli/command-line.md#r-cli.task.name-clash).
  const tasks: Executable[] = Object.keys(files)
    .filter((path) => path.startsWith(TASK_ROOT) && !path.slice(TASK_ROOT.length).includes("/"))
    .sort()
    .map((path) => ({ name: path.slice(TASK_ROOT.length, -".hd".length), path }));
  for (const task of tasks)
    if (executables.some((executable) => executable.name === task.name))
      problem(
        manifest.executables.find((executable) => executable.name === task.name)?.line ??
          manifest.packageLine,
        null,
        `the task ${task.path} and the executable '${task.name}' have one name; rename one, since hd run ${task.name} must name one program`,
      );
  return { kind: "package", package: { root, name, files, executables, tasks, problems } };
}

/**
 * The `unselected-main` warnings of a package: a public `main` or `main!` in
 * a module under the source root that no executable names is an ordinary
 * function (spec/cli/command-line.md#r-cli.exe.unselected-main).
 */
export function unselectedMains(pkg: LocalPackage): PackageProblem[] {
  const entries = new Set(pkg.executables.map(({ path }) => path));
  const warnings: PackageProblem[] = [];
  for (const [path, text] of Object.entries(pkg.files).sort()) {
    if (!path.startsWith(SOURCE_ROOT) || entries.has(path) || path.endsWith("_test.hd")) continue;
    // A src/main.hd that no executable names has its own error.
    if (path === MAIN_FILE) continue;
    const main = parse(text).program?.functions.find(
      (declaration) => declaration.name === "main" && declaration.public,
    );
    if (!main) continue;
    warnings.push({
      path,
      line: main.span.start.line,
      column: main.span.start.column,
      code: "unselected-main",
      message: `'main' in ${path} is an ordinary function, since no executable names this module; name it in an [[executable]] table of hd.toml to run it`,
      severity: "warning",
    });
  }
  return warnings;
}
