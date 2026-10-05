// `hd new`: creates a package (spec/cli/command-line.md#creating-a-package).

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";

import { KEYWORDS } from "../lexer.ts";
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
  /** `--pages`: also write the GitHub Pages workflow (cli.new.pages). */
  readonly pages: boolean;
  /** `--vcs none`: run no `git init` and write no `.gitignore` (cli.new.vcs-none). */
  readonly vcs: boolean;
  /** PATH; absent, or `.`, for the working directory (cli.new.here.dir). */
  readonly path?: string;
}

/** The version of this `hd`, which the Pages workflow pins (spec/cli/command-line.md#r-cli.new.pages.workflow). */
function hdVersion(): string {
  const manifest = join(import.meta.dirname, "..", "..", "package.json");
  return (JSON.parse(readFileSync(manifest, "utf8")) as { version: string }).version;
}

const IDENTIFIER = /^[\p{ID_Start}_][\p{ID_Continue}_]*$/u;

/** The files `hd new` writes, keyed by their path in the package directory. */
function packageFiles(
  kind: PackageKind,
  name: string,
  options: { readonly pages: boolean; readonly vcs: boolean },
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
  if (options.pages) files[".github/workflows/docs.yml"] = pagesWorkflow(hdVersion());
  return files;
}

/** The workflow that runs `hd doc` and deploys it to GitHub Pages (spec/cli/command-line.md#r-cli.new.pages.workflow). */
function pagesWorkflow(version: string): string {
  return [
    `# Written by \`hd new --pages\` (hd ${version}).`,
    "name: Docs",
    "on:",
    "  push:",
    "    branches: [main]",
    "  workflow_dispatch:",
    "permissions:",
    "  contents: read",
    "  pages: write",
    "  id-token: write",
    "concurrency:",
    "  group: pages",
    "  cancel-in-progress: false",
    "jobs:",
    "  docs:",
    "    runs-on: ubuntu-latest",
    "    environment:",
    "      name: github-pages",
    "      url: ${{ steps.deploy.outputs.page_url }}",
    "    steps:",
    "      - uses: actions/checkout@v5",
    `      - uses: hd-lang/setup-hd@${version}`,
    "      - run: hd doc --out _site",
    "      - uses: actions/upload-pages-artifact@v4",
    "        with:",
    "          path: _site",
    "      - id: deploy",
    "        uses: actions/deploy-pages@v4",
    "",
  ].join("\n");
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

/**
 * Asks which kind of package to create, and whether to publish its
 * documentation to GitHub Pages (spec/cli/command-line.md#r-cli.new.kind.ask,
 * spec/cli/command-line.md#r-cli.new.pages.ask).
 */
async function askKind(
  ask: (question: string) => Promise<string>,
): Promise<{ readonly kind: PackageKind; readonly pages: boolean } | undefined> {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const answer = (await ask("Create an application or a library? [app/lib] "))
      .trim()
      .toLowerCase();
    const kind = ["app", "application", "a"].includes(answer)
      ? "app"
      : ["lib", "library", "l"].includes(answer)
        ? "lib"
        : undefined;
    if (!kind) continue;
    const pages = (await ask("Publish the documentation to GitHub Pages? [y/N] "))
      .trim()
      .toLowerCase();
    return { kind, pages: pages === "y" || pages === "yes" };
  }
  return undefined;
}

/** `hd new [--app|--lib] [--pages] [--vcs none] [PATH]`: creates a package. */
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
  let { kind, pages } = args;
  if (!kind) {
    const terminal = terminalOf(args);
    // Without a terminal, hd new never picks a kind itself
    // (spec/cli/command-line.md#r-cli.new.kind.no-terminal).
    if (!terminal) return fail("pass --app to create an application, or --lib to create a library");
    const asked = await askKind(terminal.ask);
    if (!asked) return fail("no kind chosen; pass --app or --lib");
    kind = asked.kind;
    pages = pages || asked.pages;
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
  const files = packageFiles(kind, name, { pages, vcs: gitInit });
  // hd new writes nothing when a file it would write exists
  // (spec/cli/command-line.md#r-cli.new.existing).
  const existing = Object.keys(files).filter((path) => existsSync(join(directory, path)));
  if (existing.length > 0)
    return fail(
      `${existing.map((path) => relative(cwd, join(directory, path)) || path).join(", ")} already exist${existing.length === 1 ? "s" : ""}; hd new writes no file over another`,
    );
  for (const [path, text] of Object.entries(files)) {
    await mkdir(dirname(join(directory, path)), { recursive: true });
    await writeFile(join(directory, path), text);
  }
  if (gitInit) {
    const result = spawnSync("git", ["init", "--quiet"], { cwd: directory, encoding: "utf8" });
    if (result.status !== 0) return fail(`git init failed: ${result.stderr.trim()}`);
  }
  const shown = relative(cwd, directory) || ".";
  io.out(`Created ${kind === "app" ? "application" : "library"} package '${name}' in ${shown}`);
  return 0;
}
