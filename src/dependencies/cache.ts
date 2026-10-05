// The cache directory (spec/cli/command-line.md#cache): one per user, shared
// by every package. A fetched version lives in `pkg/HOST_PATH@VERSION`,
// read-only once written, and appears only once complete. The tree hash
// `hd` computed when it wrote the entry is kept beside it, in
// `hash/HOST_PATH@VERSION`, so a command compares `hd.sum` with that record
// instead of hashing the tree again (cli.cache.hash).

import { randomBytes } from "node:crypto";
import { existsSync } from "node:fs";
import { chmod, cp, mkdir, readdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

import { treeFiles, treeHash } from "./sum.ts";

/** The environment variables a command reads; the process's own by default. */
export type Variables = Readonly<Record<string, string | undefined>>;

/**
 * The cache directory: `HD_CACHE`, or the platform's user cache directory
 * followed by `hd` (spec/cli/command-line.md#r-cli.cache.directory).
 */
export function cacheDirectory(variables: Variables, platform = process.platform): string {
  const override = variables.HD_CACHE;
  if (override !== undefined && override !== "") return override;
  const home = variables.HOME ?? homedir();
  if (platform === "darwin") return join(home, "Library", "Caches", "hd");
  if (platform === "win32")
    return join(variables.LOCALAPPDATA ?? join(home, "AppData", "Local"), "hd");
  const xdg = variables.XDG_CACHE_HOME;
  return join(xdg !== undefined && xdg !== "" ? xdg : join(home, ".cache"), "hd");
}

/** A version's entry: its tree's directory and the tree hash recorded with it. */
export interface CacheEntry {
  readonly directory: string;
  readonly hash: string;
}

function entryPath(cache: string, hostPath: string, version: string): string {
  return join(cache, "pkg", ...`${hostPath}@${version}`.split("/"));
}

function hashPath(cache: string, hostPath: string, version: string): string {
  return join(cache, "hash", ...`${hostPath}@${version}`.split("/"));
}

/** A version's cached entry, or undefined when the cache lacks it. */
export async function cachedEntry(
  cache: string,
  hostPath: string,
  version: string,
): Promise<CacheEntry | undefined> {
  const directory = entryPath(cache, hostPath, version);
  const record = hashPath(cache, hostPath, version);
  if (!existsSync(directory) || !existsSync(record)) return undefined;
  return { directory, hash: (await readFile(record, "utf8")).trim() };
}

/** A fresh scratch directory inside the cache, on the same file system as its entries. */
export async function scratchDirectory(cache: string): Promise<string> {
  const directory = join(cache, "tmp", randomBytes(8).toString("hex"));
  await mkdir(directory, { recursive: true });
  return directory;
}

/**
 * Makes every file and directory inside `path` read-only
 * (cli.cache.read-only). `path` itself stays writable until it is in place,
 * since some systems cannot rename a directory that is not writable.
 */
async function makeContentsReadOnly(path: string): Promise<void> {
  for (const entry of await readdir(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) {
      await makeContentsReadOnly(child);
      await chmod(child, 0o555);
    } else if (entry.isFile()) await chmod(child, 0o444);
  }
}

/**
 * Stores the tree in `source` as a version's entry. Only the files of the
 * tree are copied (spec/cli/command-line.md#r-cli.sum.tree), its hash is
 * recorded, and the entry is made read-only, then moved into place in one
 * rename, so a reader never sees a partial entry (cli.cache.complete).
 */
export async function storeEntry(
  cache: string,
  hostPath: string,
  version: string,
  source: string,
): Promise<CacheEntry> {
  const staging = await scratchDirectory(cache);
  try {
    const tree = join(staging, "tree");
    await mkdir(tree);
    for (const path of await treeFiles(source)) {
      await mkdir(dirname(join(tree, path)), { recursive: true });
      await cp(join(source, path), join(tree, path));
    }
    const hash = await treeHash(tree);
    const record = hashPath(cache, hostPath, version);
    await mkdir(dirname(record), { recursive: true });
    await writeFile(join(staging, "hash"), `${hash}\n`);
    await rename(join(staging, "hash"), record);
    await makeContentsReadOnly(tree);
    const directory = entryPath(cache, hostPath, version);
    await mkdir(dirname(directory), { recursive: true });
    // Another `hd` may have stored the same version meanwhile; its entry stays.
    if (!existsSync(directory)) {
      await rename(tree, directory);
      await chmod(directory, 0o555);
    }
    return { directory, hash: (await cachedEntry(cache, hostPath, version))?.hash ?? hash };
  } finally {
    await removeTree(staging);
  }
}

/** Removes a directory that may hold read-only files. */
export async function removeTree(path: string): Promise<void> {
  if (!existsSync(path)) return;
  const writable = async (directory: string): Promise<void> => {
    await chmod(directory, 0o755);
    for (const entry of await readdir(directory, { withFileTypes: true }))
      if (entry.isDirectory()) await writable(join(directory, entry.name));
  };
  await writable(path).catch(() => undefined);
  await rm(path, { recursive: true, force: true });
}
