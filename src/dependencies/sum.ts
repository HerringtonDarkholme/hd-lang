// `hd.sum` (spec/cli/command-line.md#hdsum): one line per selected version,
// `HOST_PATH@VERSION h1:HASH`, with the tree hash of its files, and one
// line per version whose manifest selection reads,
// `HOST_PATH@VERSION/hd.toml h1:HASH` (cli.sum.manifest-line).

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { lstat, readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import { compareVersions, parseHostPath, parseVersion } from "./requirement.ts";

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

/** A key's host path, version, and whether it is a manifest line's. */
function splitKey(key: string): [path: string, version: string, manifest: boolean] {
  const manifest = key.endsWith(MANIFEST_SUFFIX);
  const bare = manifest ? key.slice(0, -MANIFEST_SUFFIX.length) : key;
  const at = bare.lastIndexOf("@");
  return [bare.slice(0, at), bare.slice(at + 1), manifest];
}

/**
 * The order of `hd.sum`'s keys (cli.sum.order): by host path, then by
 * version order, with a version's tree line before its manifest line.
 */
function compareKeys(left: string, right: string): number {
  const [leftPath, leftVersion, leftManifest] = splitKey(left);
  const [rightPath, rightVersion, rightManifest] = splitKey(right);
  if (leftPath !== rightPath) return leftPath < rightPath ? -1 : 1;
  if (leftVersion !== rightVersion) {
    const [a, b] = [parseVersion(leftVersion), parseVersion(rightVersion)];
    if (a && b && compareVersions(a, b) !== 0) return compareVersions(a, b);
    return leftVersion < rightVersion ? -1 : 1;
  }
  return Number(leftManifest) - Number(rightManifest);
}

/**
 * `hd.sum`'s entries, or the line and message of the first line outside
 * `cli.sum.line` and `cli.sum.manifest-line`, out of `cli.sum.order`, or a
 * repeat of an earlier key. The last line, too, ends with a newline.
 */
export function readSum(
  text: string,
): { readonly entries: SumEntries } | { readonly line: number; readonly message: string } {
  const entries = new Map<string, string>();
  const repair = "restore hd.sum from version control, or delete the line and run hd fetch";
  const lines = text.split("\n");
  // The text after the last newline: empty when every line ends with one.
  const last = lines.pop()!;
  let previous: string | undefined;
  for (const [index, line] of lines.entries()) {
    const number = index + 1;
    const match = /^(\S+)@(\S+?)(\/hd\.toml)? (h1:[A-Za-z0-9+/]{43}=)$/.exec(line);
    if (!match || typeof parseHostPath(match[1]!) === "string" || !parseVersion(match[2]!))
      return {
        line: number,
        message: `hd.sum line ${number} is not 'HOST_PATH@VERSION h1:HASH' or 'HOST_PATH@VERSION/hd.toml h1:HASH'; ${repair}`,
      };
    const key = sumKey(match[1]!, match[2]!) + (match[3] ?? "");
    if (entries.has(key))
      return { line: number, message: `hd.sum line ${number} repeats the entry ${key}; ${repair}` };
    if (previous !== undefined && compareKeys(previous, key) > 0)
      return {
        line: number,
        message: `hd.sum line ${number} is out of order: ${key} sorts before ${previous}, by host path, then version; ${repair}`,
      };
    entries.set(key, match[4]!);
    previous = key;
  }
  if (last !== "")
    return {
      line: lines.length + 1,
      message: `hd.sum line ${lines.length + 1} does not end with a newline; restore hd.sum from version control, or run hd fetch`,
    };
  return { entries };
}

/**
 * `hd.sum`'s text: sorted by host path, then by version order, with a
 * version's tree line before its manifest line (cli.sum.order).
 */
export function formatSum(entries: SumEntries): string {
  const keys = [...entries.keys()].sort(compareKeys);
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
