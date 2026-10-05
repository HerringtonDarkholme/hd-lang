// What `hd test` gives a test case besides the runner
// (spec/cli/command-line.md#test-environments): the default profile's
// providers for an integration test case, and a temporary directory per run
// of a test body.

import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { HirFunction, HirTrait } from "../hir.ts";
import type { TempDirs } from "../test-runner.ts";
import { displayType } from "../types.ts";
import { DEFAULT_PROFILE_TRAITS, type DefaultProfileHost } from "./default-profile.ts";

/**
 * Each run's directory: a fresh `mkdtemp` directory under the system's
 * temporary directory, so no other run shares it, in this `hd test` or in
 * another one at the same time (spec/cli/command-line.md#r-cli.test.env.temp-dir).
 * It is removed with its contents when the run ends
 * (spec/cli/command-line.md#r-cli.test.env.temp-dir.removed).
 */
export const TEST_TEMP_DIRS: TempDirs = {
  make: () => mkdtempSync(join(realpathSync(tmpdir()), "hd-test-")),
  remove: (path) => rmSync(path, { recursive: true, force: true }),
};

/**
 * The default profile as an integration test case sees it
 * (spec/cli/command-line.md#r-cli.test.env.integration): the package
 * directory as the working directory (cli.test.env.cwd), no program
 * arguments, the test's file as `Args.program` (cli.test.env.args,
 * cli.test.env.args.program), and a closed standard input, which reads as
 * its end (cli.test.env.stdin).
 */
export function integrationTestHost(
  packageRoot: string,
  testFile: string,
  variables: Readonly<Record<string, string | undefined>>,
): DefaultProfileHost {
  return {
    program: testFile,
    arguments: [],
    variables,
    workingDirectory: packageRoot,
    readLine: () => undefined,
  };
}

/** The std traits a test run binds besides the default profile's (spec/cli/command-line.md#test-environments). */
const RUN_TRAITS = [
  "std.console.Console",
  "std.testing.TestRunner",
  "std.testing.PropertyRunner",
  "std.process.Process",
];

/**
 * Why an integration test case, or a doc test, is skipped: its row names a
 * trait that the run's profile does not bind
 * (spec/lang/10-modules.md#r-module.testing.skipped). `profile` names the
 * profile, and `bound` holds the traits a runner profile binds by name;
 * the default profile binds its std traits.
 */
export function profileSkip(
  traits: readonly HirTrait[],
  profile: string,
  bound: readonly string[],
): (declaration: HirFunction) => string | undefined {
  const standard = new Set([
    ...RUN_TRAITS,
    ...DEFAULT_PROFILE_TRAITS.map(({ module, name }) => `${module}.${name}`),
  ]);
  const byName = new Map(traits.map((trait) => [trait.name, trait]));
  return (declaration) => {
    const unbound = declaration.requirements.filter((requirement) => {
      const standardName = byName.get(requirement)?.standardName;
      return (
        !(standardName !== undefined && standard.has(standardName)) && !bound.includes(requirement)
      );
    });
    if (unbound.length === 0) return undefined;
    const names = unbound.map((requirement) =>
      displayType(byName.get(requirement)?.standardName?.split(".").at(-1) ?? requirement),
    );
    return `the ${profile} profile does not bind ${names.join(", ")}`;
  };
}
