// `hd new`: creates a package (spec/cli/command-line.md#creating-a-package).

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve, sep } from "node:path";

import { addWorkspaceMember } from "../dependencies/manifest-edit.ts";
import { KEYWORDS } from "../lexer.ts";
import { readManifest } from "../manifest.ts";
import { MAIN_FILE, LIB_FILE, TEST_ROOT } from "../package.ts";
import {
  EXIT_HD_FAILURE,
  terminalOf,
  workingDirectory,
  type CommandEnvironment,
  type CommandIo,
} from "./io.ts";
import { BUILD_DIRECTORY, MANIFEST_FILE } from "./package-mode.ts";

export type PackageKind = "app" | "lib";

export interface NewArgs extends CommandEnvironment {
  /** `--app` or `--lib`; absent when neither flag is given. */
  readonly kind?: PackageKind;
  /** `--vcs none`: run no `git init` and write no `.gitignore` (cli.new.vcs-none). */
  readonly vcs: boolean;
  /** PATH; absent, or `.`, for the working directory (cli.new.here.dir). */
  readonly path?: string;
}

const IDENTIFIER = /^[\p{ID_Start}_][\p{ID_Continue}_]*$/u;

/** The files `hd new` writes, keyed by their path in the package directory. */
function packageFiles(
  kind: PackageKind,
  name: string,
  options: { readonly vcs: boolean },
): Record<string, string> {
  // The integration test is named after the package, with each `-` a `_`
  // (spec/cli/command-line.md#r-cli.new.test-name).
  const test = `${TEST_ROOT}${name.replaceAll("-", "_")}.hd`;
  // No [[executable]] table: src/main.hd is the default executable
  // (spec/cli/command-line.md#r-cli.new.no-executable-table).
  const files: Record<string, string> = { [MANIFEST_FILE]: `[package]\nname = "${name}"\n` };
  if (kind === "app") {
    files[MAIN_FILE] = 'pub fn main() -> void $ Console:\n    println("hello, world")\n';
    // The test runs the executable, which is named after the package
    // (spec/cli/command-line.md#r-cli.new.app.test).
    files[test] = [
      "use std.testing.{assert_equal, hd_run}",
      "",
      'it("prints a greeting"):',
      `    let out = hd_run!("${name}")`,
      '    assert_equal(out.stdout, "hello, world\\n", reason="the greeting")',
      '    assert_equal(out.status, 0, reason="a clean exit")',
      "",
    ].join("\n");
  } else {
    files[LIB_FILE] = [
      "## Greets `name`.",
      "pub fn greet(name: string) -> string:",
      '    "hello, ${name}"',
      "",
    ].join("\n");
    // The test reaches the library through its public interface
    // (spec/cli/command-line.md#r-cli.new.lib.test).
    files[test] = [
      "use pkg.{greet}",
      "use std.testing.assert_equal",
      "",
      'it("greets by name"):',
      '    assert_equal(greet("world"), "hello, world", reason="the greeting")',
      "",
    ].join("\n");
  }
  // The .gitignore lists only the build directory, so hd.sum is committed
  // (spec/cli/command-line.md#r-cli.new.vcs.ignore).
  if (options.vcs) files[".gitignore"] = `/${BUILD_DIRECTORY}/\n`;
  return files;
}

/** The nearest directory at or above `path` that exists. */
function existingAncestor(path: string): string {
  let directory = path;
  while (!existsSync(directory) && dirname(directory) !== directory) directory = dirname(directory);
  return directory;
}

/** Whether `directory` lies inside a git repository, or undefined when git does not run. */
function insideGitRepository(directory: string): boolean | undefined {
  const result = spawnSync("git", ["rev-parse", "--is-inside-work-tree"], {
    cwd: directory,
    encoding: "utf8",
  });
  if (result.error) return undefined;
  return result.status === 0 && result.stdout.trim() === "true";
}

/** Asks which kind of package to create (spec/cli/command-line.md#r-cli.new.kind.ask). */
async function askKind(
  ask: (question: string) => Promise<string>,
): Promise<PackageKind | undefined> {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const answer = (await ask("Create an application or a library? [app/lib] "))
      .trim()
      .toLowerCase();
    const kind = ["app", "application", "a"].includes(answer)
      ? "app"
      : ["lib", "library", "l"].includes(answer)
        ? "lib"
        : undefined;
    if (kind) return kind;
  }
  return undefined;
}

/**
 * The workspace manifest above `directory` whose `members` gains it, with
 * its new text (spec/cli/command-line.md#r-cli.new.workspace-member); or
 * undefined when no workspace manifest is above it, or that manifest lists
 * the directory already in `members` or `exclude`.
 */
async function workspaceEdit(
  directory: string,
): Promise<{ readonly path: string; readonly text: string; readonly member: string } | undefined> {
  for (let above = dirname(directory); ; above = dirname(above)) {
    const path = join(above, MANIFEST_FILE);
    if (existsSync(path)) {
      const text = await readFile(path, "utf8");
      const read = readManifest(text);
      if ("manifest" in read && read.manifest.workspace) {
        const member = relative(above, directory).split(sep).join("/");
        const same = (listed: string): boolean => resolve(above, listed) === resolve(above, member);
        if ([...read.manifest.members, ...read.manifest.exclude].some(same)) return undefined;
        const edited = addWorkspaceMember(text, member);
        return edited === undefined ? undefined : { path, text: edited, member };
      }
    }
    if (dirname(above) === above) return undefined;
  }
}

/** `hd new [--app|--lib] [--vcs none] [PATH]`: creates a package. */
export async function newCommand(args: NewArgs, io: CommandIo): Promise<number> {
  const fail = (message: string): number => {
    io.err(`hd new: ${message}`);
    return EXIT_HD_FAILURE;
  };
  const cwd = workingDirectory(args);
  const directory = resolve(cwd, args.path ?? ".");
  // The package is named after its directory (spec/cli/command-line.md#r-cli.name.hyphen).
  const name = basename(directory);
  if (!IDENTIFIER.test(name.replaceAll("-", "_")) || KEYWORDS.has(name.replaceAll("-", "_")))
    return fail(
      `'${name}' cannot name a package: with each - as _, a package name must be an identifier, such as my-app`,
    );
  let { kind } = args;
  if (!kind) {
    const terminal = terminalOf(args);
    // Without a terminal, hd new never picks a kind itself
    // (spec/cli/command-line.md#r-cli.new.kind.no-terminal).
    if (!terminal) return fail("pass --app to create an application, or --lib to create a library");
    kind = await askKind(terminal.ask);
    if (!kind) return fail("no kind chosen; pass --app or --lib");
  }
  // Unless the directory is already in a git repository, hd new runs git
  // init (spec/cli/command-line.md#r-cli.new.vcs).
  let gitInit = false;
  if (args.vcs) {
    const inside = insideGitRepository(existingAncestor(directory));
    if (inside === undefined)
      return fail("git does not run, so hd new cannot run git init; pass --vcs none to skip it");
    gitInit = !inside;
  }
  const files = packageFiles(kind, name, { vcs: gitInit });
  // hd new writes nothing when a file it would write exists
  // (spec/cli/command-line.md#r-cli.new.existing).
  const existing = Object.keys(files).filter((path) => existsSync(join(directory, path)));
  if (existing.length > 0)
    return fail(
      `${existing.map((path) => relative(cwd, join(directory, path)) || path).join(", ")} already exist${existing.length === 1 ? "s" : ""}; hd new writes no file over another`,
    );
  const workspace = await workspaceEdit(directory);
  for (const [path, text] of Object.entries(files)) {
    await mkdir(dirname(join(directory, path)), { recursive: true });
    await writeFile(join(directory, path), text);
  }
  if (workspace) await writeFile(workspace.path, workspace.text);
  if (gitInit) {
    const result = spawnSync("git", ["init", "--quiet"], { cwd: directory, encoding: "utf8" });
    if (result.status !== 0) return fail(`git init failed: ${result.stderr.trim()}`);
  }
  const shown = relative(cwd, directory) || ".";
  io.out(`Created ${kind === "app" ? "application" : "library"} package '${name}' in ${shown}`);
  if (workspace)
    io.out(
      `Added "${workspace.member}" to the members of ${relative(cwd, workspace.path) || workspace.path}`,
    );
  return 0;
}
