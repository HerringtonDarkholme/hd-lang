// Fetching with the system's `git` (spec/cli/command-line.md#r-cli.dep.git):
// git's own configuration applies, so its credential helpers, SSH keys, and
// `url.<base>.insteadOf` rewrites reach a private repository, and `hd`
// stores no credentials (spec/lang/10-modules.md#r-module.repo.credentials).
// git never asks a question (spec/cli/command-line.md#r-cli.dep.no-prompt),
// and a message never shows a credential (cli.dep.no-secret).

import { spawn } from "node:child_process";
import { join } from "node:path";

import type { Variables } from "./cache.ts";

/** What one git command did. */
interface GitResult {
  readonly status: number;
  readonly stdout: string;
  readonly stderr: string;
}

/** Why git could not do what `hd` asked, with git's last words, masked. */
export class GitError extends Error {}

/**
 * Replaces the user name and password of each URL in `text` with `***`
 * (spec/cli/command-line.md#r-cli.dep.no-secret).
 */
export function maskCredentials(text: string): string {
  return text.replace(/\b([a-z][a-z0-9+.-]*:\/\/)[^/\s@]+@/gi, "$1***@");
}

/**
 * The environment git runs in: the command's own, with prompts turned off.
 * An SSH command the user set stays; otherwise ssh runs in batch mode, so
 * a passphrase or an unknown host fails instead of asking.
 */
function gitVariables(variables: Variables): NodeJS.ProcessEnv {
  return {
    ...variables,
    GIT_TERMINAL_PROMPT: "0",
    GCM_INTERACTIVE: "never",
    GIT_SSH_COMMAND: variables.GIT_SSH_COMMAND ?? "ssh -o BatchMode=yes",
  };
}

function runGit(args: readonly string[], variables: Variables, cwd?: string): Promise<GitResult> {
  return new Promise((resolve, reject) => {
    const child = spawn("git", args, {
      cwd,
      env: gitVariables(variables),
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => (stdout += chunk));
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => (stderr += chunk));
    child.on("error", (error: NodeJS.ErrnoException) =>
      reject(
        new GitError(
          error.code === "ENOENT"
            ? "the git command is not installed or not on PATH"
            : `git did not start: ${error.message}`,
        ),
      ),
    );
    child.on("close", (status) => resolve({ status: status ?? 1, stdout, stderr }));
  });
}

/** git's last line of complaint, masked, for a message. */
function complaint(result: GitResult): string {
  const lines = result.stderr
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "" && !line.startsWith("hint:"));
  return maskCredentials(lines.at(-1) ?? `git exited with status ${result.status}`);
}

/**
 * The tags of the repository at `url`, by name, each with its commit. A
 * repository git cannot reach or read is a `GitError`.
 */
export async function listTags(url: string, variables: Variables): Promise<Map<string, string>> {
  const result = await runGit(["ls-remote", "--tags", "--refs", url], variables);
  if (result.status !== 0) throw new GitError(complaint(result));
  const tags = new Map<string, string>();
  for (const line of result.stdout.split("\n")) {
    const match = /^([0-9a-f]+)\trefs\/tags\/(.+)$/.exec(line.trim());
    if (match) tags.set(match[2]!, match[1]!);
  }
  return tags;
}

/**
 * Checks out the commit of `tag` from the repository at `url` into the new
 * directory `into`, as a shallow clone, and returns `into`. Its `.git`
 * directory stays; the cache copies only the package's tree.
 */
export async function checkOutTag(
  url: string,
  tag: string,
  into: string,
  variables: Variables,
): Promise<string> {
  const steps: (readonly string[])[] = [
    ["init", "--quiet", into],
    ["-C", into, "fetch", "--quiet", "--depth", "1", "--no-tags", url, `refs/tags/${tag}`],
    ["-C", into, "-c", "advice.detachedHead=false", "checkout", "--quiet", "FETCH_HEAD"],
  ];
  for (const args of steps) {
    const result = await runGit(args, variables);
    if (result.status !== 0) throw new GitError(complaint(result));
  }
  return into;
}

/** The directory of a package inside a checked-out repository. */
export function packageDirectory(checkout: string, subdirectory: string): string {
  return subdirectory === "" ? checkout : join(checkout, ...subdirectory.split("/"));
}
