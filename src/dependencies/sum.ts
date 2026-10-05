// `hd.sum` (spec/cli/command-line.md#hdsum): one line per selected version,
// `HOST_PATH@VERSION h1:HASH`, with the tree hash of its files, and one
// line per version whose manifest selection reads,
// `HOST_PATH@VERSION/hd.toml h1:HASH` (cli.sum.manifest-line).

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { lstat, readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import { compareVersions, parseVersion } from "./requirement.ts";

/** The file name of `hd.sum`. */
export const SUM_FILE = "hd.sum";

/**
 * `hd.sum`'s entries: each `HOST_PATH@VERSION` to its tree hash, and each
 * `HOST_PATH@VERSION/hd.toml` to its manifest hash.
 */
export type SumEntries = ReadonlyMap<string, string>;

/** The key of a version's entry, as `github.com/acme/json@2.1.0`. */
export function sumKey(hostPath: string, version: string): string {
  return `${hostPath}@${version}`;
}

/** The suffix of a manifest line's key (spec/cli/command-line.md#r-cli.sum.manifest-line). */
const MANIFEST_SUFFIX = "/hd.toml";

/** The key of a version's manifest line, as `github.com/acme/json@2.1.0/hd.toml`. */
export function manifestKey(hostPath: string, version: string): string {
  return `${sumKey(hostPath, version)}${MANIFEST_SUFFIX}`;
}

/** `hd.sum`'s entries, or the line and message of the first malformed line. */
export function readSum(
  text: string,
): { readonly entries: SumEntries } | { readonly line: number; readonly message: string } {
  const entries = new Map<string, string>();
  for (const [index, line] of text.split("\n").entries()) {
    if (line.trim() === "") continue;
    const match = /^(\S+@\S+) (h1:[A-Za-z0-9+/]{43}=)$/.exec(line);
    if (!match)
      return {
        line: index + 1,
        message: `hd.sum line ${index + 1} is not 'HOST_PATH@VERSION h1:HASH' or 'HOST_PATH@VERSION/hd.toml h1:HASH'; restore hd.sum from version control, or delete the line and run hd fetch`,
      };
    entries.set(match[1]!, match[2]!);
  }
  return { entries };
}

/**
 * `hd.sum`'s text: sorted by host path, then by version order, with a
 * version's tree line before its manifest line (cli.sum.order).
 */
export function formatSum(entries: SumEntries): string {
  const split = (key: string): [string, string, boolean] => {
    const manifest = key.endsWith(MANIFEST_SUFFIX);
    const bare = manifest ? key.slice(0, -MANIFEST_SUFFIX.length) : key;
    const at = bare.lastIndexOf("@");
    return [bare.slice(0, at), bare.slice(at + 1), manifest];
  };
  const keys = [...entries.keys()].sort((left, right) => {
    const [leftPath, leftVersion, leftManifest] = split(left);
    const [rightPath, rightVersion, rightManifest] = split(right);
    if (leftPath !== rightPath) return leftPath < rightPath ? -1 : 1;
    if (leftVersion !== rightVersion) {
      const [a, b] = [parseVersion(leftVersion), parseVersion(rightVersion)];
      return a && b ? compareVersions(a, b) : leftVersion < rightVersion ? -1 : 1;
    }
    return Number(leftManifest) - Number(rightManifest);
  });
  return keys.map((key) => `${key} ${entries.get(key)!}\n`).join("");
}

/**
 * The files of a version's tree (spec/cli/command-line.md#r-cli.sum.tree):
 * every regular file under `directory`, by its path with `/`, except the
 * files of a subdirectory that holds its own `hd.toml`, which is another
 * package. Symbolic links and the `.git` directory are not part of it.
 */
export async function treeFiles(directory: string): Promise<string[]> {
  const files: string[] = [];
  const walk = async (relative: string): Promise<void> => {
    const absolute = join(directory, relative);
    if (relative !== "" && existsSync(join(absolute, "hd.toml"))) return;
    for (const entry of await readdir(absolute, { withFileTypes: true })) {
      const path = relative === "" ? entry.name : `${relative}/${entry.name}`;
      if (entry.isDirectory()) {
        if (relative === "" && entry.name === ".git") continue;
        await walk(path);
      } else if (entry.isFile() && (await lstat(join(directory, path))).isFile()) files.push(path);
    }
  };
  await walk("");
  return files.sort();
}

/**
 * The `h1:` hash of a tree's files (spec/cli/command-line.md#r-cli.sum.hash):
 * the Base64 SHA-256 of the summary, one line per file with its SHA-256 in
 * hexadecimal, two spaces, and its path.
 */
function summaryHash(files: readonly (readonly [path: string, content: Buffer])[]): string {
  const summary = createHash("sha256");
  for (const [path, content] of files)
    summary.update(`${createHash("sha256").update(content).digest("hex")}  ${path}\n`);
  return `h1:${summary.digest("base64")}`;
}

/** The tree hash of `directory` (spec/cli/command-line.md#r-cli.sum.hash). */
export async function treeHash(directory: string): Promise<string> {
  const files: [string, Buffer][] = [];
  for (const path of await treeFiles(directory))
    files.push([path, await readFile(join(directory, path))]);
  return summaryHash(files);
}

/**
 * The manifest hash of the package in `directory`: the tree hash of a tree
 * that holds only its `hd.toml` (spec/cli/command-line.md#r-cli.sum.manifest-hash).
 */
export async function manifestHash(directory: string): Promise<string> {
  return summaryHash([["hd.toml", await readFile(join(directory, "hd.toml"))]]);
}
