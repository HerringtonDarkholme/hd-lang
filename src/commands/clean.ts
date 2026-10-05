// `hd clean` (spec/cli/command-line.md#cleaning): removes the package's
// build directory, or with `--cache` the dependency cache.

import { existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { join, relative, sep } from "node:path";

import { clearCache } from "../dependencies/cache.ts";
import { Report, type OutputFormat } from "../diagnostic-report.ts";
import {
  EXIT_HD_FAILURE,
  variablesOf,
  workingDirectory,
  type CommandEnvironment,
  type CommandIo,
} from "./io.ts";
import { BUILD_DIRECTORY, MANIFEST_FILE, packageMode, workspaceMembers } from "./package-mode.ts";

export interface CleanArgs extends CommandEnvironment {
  readonly format: OutputFormat;
  /** `--cache`: remove the dependency cache instead of the build directory. */
  readonly cache?: boolean;
}

/** `hd clean [--cache]`. */
export async function cleanCommand(args: CleanArgs, io: CommandIo): Promise<number> {
  const report = new Report(args.format, io, { stream: "stdout", summary: true });
  const say = (line: string): void => {
    if (args.format === "text") io.out(line);
  };
  if (args.cache) {
    const cleared = await clearCache(variablesOf(args));
    if ("refused" in cleared) {
      report.commandError(`hd clean: ${cleared.refused}; nothing was removed`);
      return report.finish(EXIT_HD_FAILURE);
    }
    if (cleared.removed.length === 0) say(`the cache ${cleared.directory} is empty`);
    else {
      for (const version of cleared.removed) say(`removed ${version}`);
      say(
        `removed ${cleared.removed.length} ${cleared.removed.length === 1 ? "version" : "versions"} from ${cleared.directory}`,
      );
    }
    return report.finish(0);
  }
  // The package's build directory (cli.clean.build), or each member's
  // (cli.clean.build.workspace).
  const start = workingDirectory(args);
  const mode = await packageMode(start);
  if (mode.kind === "outside") {
    report.commandError(
      `hd clean: not in a package (no ${MANIFEST_FILE} in ${start} or a directory above it); create one with hd new, or remove the dependency cache with hd clean --cache`,
    );
    return report.finish(EXIT_HD_FAILURE);
  }
  let directories: string[];
  let base: string;
  if (mode.kind === "package") {
    directories = [mode.package.root];
    base = mode.package.root;
  } else {
    const members = await workspaceMembers(mode.root);
    if (typeof members === "string") {
      report.commandError(`hd clean: ${members}`);
      return report.finish(EXIT_HD_FAILURE);
    }
    directories = members.map((member) => member.root);
    base = mode.root;
  }
  let removed = 0;
  for (const directory of directories) {
    const build = join(directory, BUILD_DIRECTORY);
    if (!existsSync(build)) continue;
    await rm(build, { recursive: true, force: true });
    removed += 1;
    say(`removed ${relative(base, build).split(sep).join("/")}`);
  }
  if (removed === 0) say("nothing to clean");
  return report.finish(0);
}
