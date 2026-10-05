// What `hd test` gives a test case besides the runner
// (spec/cli/command-line.md#test-environments): the default profile's
// providers for an integration test case, and a temporary directory per run
// of a test body.

import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { TempDirs } from "../test-runner.ts";
import type { DefaultProfileHost } from "./default-profile.ts";

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
