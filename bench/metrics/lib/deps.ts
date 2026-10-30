// A local "remote" for dependency metrics, with no network.
//
// The CLI fetches a host path's repository from `https://HOST/REPO` with the
// system's git, under git's own configuration (cli.dep.repo-url,
// cli.dep.git). So a temporary GIT_CONFIG_GLOBAL rewrites
// `https://hd-metrics.invalid/` to git's `ext::` transport, which runs a
// small script: it appends one line to a log, then serves a local repository
// with `git upload-pack`. The log counts the connections any hd makes to the
// remote, whatever git commands it runs. GIT_ALLOW_PROTOCOL=ext stops git
// from reaching any other transport, and `.invalid` never resolves.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { fixture, manifest } from "./fixtures.ts";
import { type HdCommand, runHd, runProblem, supportsCommand } from "./hd.ts";
import { listTree, makeTempDir, writeTree } from "./tmp.ts";

const HOST = "hd-metrics.invalid";

/** The tagged tree of the dependency, version 1.0.0 (fixtures/deps/dep.hd). */
export const DEPENDENCY_FILES: ReadonlyMap<string, string> = new Map([
  ["hd.toml", manifest("dep")],
  ["src/lib.hd", fixture("deps/dep.hd")],
]);

export interface LocalRemote {
  /** The host path of the dependency, as `hd add` takes it before `@`. */
  readonly hostPath: string;
  readonly version: string;
  /** Variables for every hd run that may fetch: git's configuration and a private HOME. */
  readonly env: Readonly<Record<string, string>>;
  /** Connections made to the remote so far. */
  connections(): number;
  /** Moves the repository away, so any later fetch fails. */
  disconnect(): void;
}

/** Whether git is on PATH. */
export const hasGit = (): boolean => spawnSync("git", ["--version"]).status === 0;

/** Makes the remote repository, tagged v1.0.0, in a temporary directory. Throws when git fails. */
export function makeLocalRemote(): LocalRemote {
  const root = makeTempDir("remote");
  const repo = join(root, "remotes", "acme", "dep.git");
  mkdirSync(join(root, "home"), { recursive: true });
  mkdirSync(repo, { recursive: true });
  const log = join(root, "connections.log");
  writeFileSync(
    join(root, "upload.sh"),
    `echo "$1" >> "${log}"\nexec git upload-pack "${root}/remotes/$1"\n`,
  );
  writeFileSync(
    join(root, "gitconfig"),
    [
      "[user]",
      "\tname = hd metrics",
      "\temail = metrics@example.invalid",
      "[init]",
      "\tdefaultBranch = main",
      "[commit]",
      "\tgpgSign = false",
      "[tag]",
      "\tgpgSign = false",
      '[protocol "ext"]',
      "\tallow = always",
      `[url "ext::sh ${root}/upload.sh "]`,
      `\tinsteadOf = https://${HOST}/`,
      "",
    ].join("\n"),
  );
  const env = {
    HOME: join(root, "home"),
    GIT_CONFIG_GLOBAL: join(root, "gitconfig"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_ALLOW_PROTOCOL: "ext",
    GIT_TERMINAL_PROMPT: "0",
  };
  const git = (...args: string[]): void => {
    const result = spawnSync("git", args, {
      cwd: repo,
      env: { ...process.env, ...env },
      encoding: "utf8",
    });
    if (result.status !== 0) {
      throw new Error(`git ${args[0]} failed: ${result.stderr.trim()}`);
    }
  };
  writeTree(repo, DEPENDENCY_FILES);
  git("init", "--quiet");
  git("add", ...DEPENDENCY_FILES.keys());
  git("commit", "--quiet", "-m", "v1.0.0");
  git("tag", "v1.0.0");
  return {
    hostPath: `${HOST}/acme/dep.git`,
    version: "1.0.0",
    env,
    connections: () =>
      existsSync(log) ? readFileSync(log, "utf8").split("\n").filter(Boolean).length : 0,
    disconnect: () => renameSync(join(root, "remotes"), join(root, "gone")),
  };
}

/**
 * Writes a package that uses the dependency as `dep`: its library
 * (fixtures/deps/dependent.hd) calls `twice`. Its hd.toml has no dependency
 * line; `hd add` writes it.
 */
export function writeDependent(dir: string): void {
  writeTree(
    dir,
    new Map([
      ["hd.toml", manifest("app")],
      ["src/lib.hd", fixture("deps/dependent.hd")],
    ]),
  );
}

/**
 * Why the cache entry of the dependency differs from its tagged tree, or
 * undefined when every file of the tree is there with the same bytes. The
 * entry is `pkg/HOST_PATH@VERSION` of the cache (cli.cache.entry).
 */
export function entryProblem(cache: string, remote: LocalRemote): string | undefined {
  const entry = join(cache, "pkg", `${remote.hostPath}@${remote.version}`);
  if (!existsSync(entry)) {
    return `no cache entry at pkg/${remote.hostPath}@${remote.version}`;
  }
  for (const [path, text] of DEPENDENCY_FILES) {
    const file = join(entry, path);
    if (!existsSync(file)) {
      return `the cache entry lacks ${path}`;
    }
    if (readFileSync(file, "utf8") !== text) {
      return `the cache entry's ${path} differs from the tag`;
    }
  }
  return undefined;
}

/** A package that requires the dependency, with its hd.sum, ready to copy into worktrees. */
export interface DependentSource {
  readonly remote: LocalRemote;
  /** The package directory, with hd.toml and hd.sum as `hd add` wrote them. */
  readonly dir: string;
}

const ADD_TIMEOUT_MS = 60_000;

/**
 * Sets up the remote and one dependent package with `hd add`, in a cache of
 * its own. Returns `{ na }` when this hd has no dependency cache to probe:
 * no git, no `hd add`, or an `hd add` that writes nothing under `HD_CACHE`. Returns
 * `{ problem }` when `hd add` fails.
 */
export async function prepareDependent(
  hd: HdCommand,
): Promise<DependentSource | { readonly na: string } | { readonly problem: string }> {
  if (!hasGit()) {
    return { na: "no git on PATH, so no local remote" };
  }
  if (!(await supportsCommand(hd, "add", makeTempDir("help")))) {
    return { na: "no hd add" };
  }
  const remote = makeLocalRemote();
  const dir = makeTempDir("dependent");
  writeDependent(dir);
  const cache = makeTempDir("cache");
  const result = await runHd(hd, ["add", `${remote.hostPath}@${remote.version}`], {
    cwd: dir,
    env: { ...remote.env, HD_CACHE: cache },
    timeoutMs: ADD_TIMEOUT_MS,
  });
  const problem = runProblem(result, ADD_TIMEOUT_MS, "hd add of a local dependency");
  if (problem) {
    return { problem };
  }
  // The cache is found by behaviour: a fetch writes the version under HD_CACHE.
  if (listTree(cache).length === 0) {
    return { na: "no dependency cache: hd add fetched a version and wrote nothing under HD_CACHE" };
  }
  return { remote, dir };
}
