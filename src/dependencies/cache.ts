// The cache directory (spec/cli/command-line.md#cache): one per user, shared
// by every package. A fetched version lives in `pkg/HOST_PATH@VERSION`,
// read-only once written, and appears only once complete. The tree hash
// `hd` computed when it wrote the entry is kept beside it, in
// `hash/HOST_PATH@VERSION`, so a command compares `hd.sum` with that record
// instead of hashing the tree again (cli.cache.hash).

import { randomBytes } from "node:crypto";
import { existsSync } from "node:fs";
import {
  chmod,
  cp,
  lstat,
  mkdir,
  readdir,
  readFile,
  realpath,
  rename,
  rm,
  stat,
  unlink,
  writeFile,
} from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, parse, resolve } from "node:path";

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
 * tree are copied (spec/cli/command-line.md#r-cli.sum.tree), and the entry
 * is made read-only, then moved into place in one rename, so a reader never
 * sees a partial entry (cli.cache.complete).
 *
 * Several `hd` processes may store one version at once (cli.cache.shared).
 * The first rename into place wins; a loser discards its own tree and uses
 * the winner's. The hash record is written only after the tree is in place,
 * and only as the hash of that tree, so a record never describes another
 * tree (cli.cache.hash). A record missing beside a placed tree, as after a
 * crash between the two steps, is written by the next store.
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
    let hash = await treeHash(tree);
    await makeContentsReadOnly(tree);
    const directory = entryPath(cache, hostPath, version);
    await mkdir(dirname(directory), { recursive: true });
    if (await placeTree(tree, directory)) await chmod(directory, 0o555);
    else {
      const recorded = await cachedEntry(cache, hostPath, version);
      if (recorded) return recorded;
      // The winner has not written its record yet, or it was lost: record
      // the hash of the tree in place, never of the staged one.
      hash = await treeHash(directory);
    }
    // A rename replaces a record atomically; every writer writes the same one.
    const record = hashPath(cache, hostPath, version);
    await mkdir(dirname(record), { recursive: true });
    await writeFile(join(staging, "hash"), `${hash}\n`);
    await rename(join(staging, "hash"), record);
    return { directory, hash };
  } finally {
    await removeTree(staging);
  }
}

/**
 * Moves the staged `tree` to `directory` in one rename, or reports false
 * when another store's tree is there already, which then stays.
 */
async function placeTree(tree: string, directory: string): Promise<boolean> {
  try {
    await rename(tree, directory);
    return true;
  } catch (error) {
    if (existsSync(directory)) return false;
    throw error;
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

/** The entries `hd` keeps in a cache directory; `hd clean --cache` removes only these. */
const CACHE_ENTRIES = ["pkg", "hash", "tmp"];

/** What `hd clean --cache` did: the versions it removed, or why it refused. */
export type CacheClearing =
  | { readonly directory: string; readonly removed: readonly string[] }
  | { readonly directory: string; readonly refused: string };

/** The `HOST_PATH@VERSION` of each entry under `pkg/`, without following a symbolic link. */
async function cachedVersions(directory: string, prefix = ""): Promise<string[]> {
  const found: string[] = [];
  if (!existsSync(directory)) return found;
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const name = prefix === "" ? entry.name : `${prefix}/${entry.name}`;
    if (entry.name.includes("@")) found.push(name);
    else found.push(...(await cachedVersions(join(directory, entry.name), name)));
  }
  return found;
}

/**
 * Removes every fetched version from the cache directory
 * (spec/cli/command-line.md#r-cli.clean.cache). It refuses the file system
 * root, the home directory, a non-directory, and a directory that holds
 * anything but `pkg`, `hash`, and `tmp` (cli.clean.cache.layout), and it
 * removes only those three entries, never following a link out of the
 * directory (cli.clean.cache.scope).
 */
export async function clearCache(variables: Variables): Promise<CacheClearing> {
  const directory = resolve(cacheDirectory(variables));
  if (!existsSync(directory)) return { directory, removed: [] };
  const real = await realpath(directory);
  const home = variables.HOME ?? homedir();
  const homes = [resolve(home), existsSync(home) ? await realpath(home) : home];
  if (real === parse(real).root || homes.includes(directory) || homes.includes(real))
    return {
      directory,
      refused: `${directory} is the file system root or a home directory, which is no hd cache`,
    };
  if (!(await stat(real)).isDirectory())
    return { directory, refused: `${directory} is not a directory` };
  const foreign = (await readdir(real)).filter((name) => !CACHE_ENTRIES.includes(name)).sort();
  if (foreign.length > 0)
    return {
      directory,
      refused: `${directory} holds ${foreign[0]!}, which is not an hd cache entry (pkg, hash, tmp), so it is no hd cache`,
    };
  const removed = (await cachedVersions(join(real, "pkg"))).sort();
  for (const name of CACHE_ENTRIES) {
    const path = join(real, name);
    const info = await lstat(path).catch(() => undefined);
    if (!info) continue;
    if (info.isSymbolicLink()) await unlink(path);
    else await removeTree(path);
  }
  return { directory, removed };
}
