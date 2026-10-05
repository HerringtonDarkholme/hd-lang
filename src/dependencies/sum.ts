// `hd.sum` (spec/cli/command-line.md#hdsum): one line per selected version,
// `HOST_PATH@VERSION h1:HASH`, and the tree hash of a version's files.

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { lstat, readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import { compareVersions, parseVersion } from "./requirement.ts";

/** The file name of `hd.sum`. */
export const SUM_FILE = "hd.sum";

/** `hd.sum`'s entries: each `HOST_PATH@VERSION` to its tree hash. */
export type SumEntries = ReadonlyMap<string, string>;

/** The key of a version's entry, as `github.com/acme/json@2.1.0`. */
export function sumKey(hostPath: string, version: string): string {
  return `${hostPath}@${version}`;
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
        message: `hd.sum line ${index + 1} is not 'HOST_PATH@VERSION h1:HASH'; restore hd.sum from version control, or delete the line and run hd fetch`,
      };
    entries.set(match[1]!, match[2]!);
  }
  return { entries };
}

/** `hd.sum`'s text: sorted by host path, then by version order (cli.sum.order). */
export function formatSum(entries: SumEntries): string {
  const split = (key: string): [string, string] => {
    const at = key.lastIndexOf("@");
    return [key.slice(0, at), key.slice(at + 1)];
  };
  const keys = [...entries.keys()].sort((left, right) => {
    const [leftPath, leftVersion] = split(left);
    const [rightPath, rightVersion] = split(right);
    if (leftPath !== rightPath) return leftPath < rightPath ? -1 : 1;
    const [a, b] = [parseVersion(leftVersion), parseVersion(rightVersion)];
    return a && b ? compareVersions(a, b) : leftVersion < rightVersion ? -1 : 1;
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
 * The tree hash of `directory` (spec/cli/command-line.md#r-cli.sum.hash):
 * `h1:` and the Base64 SHA-256 of the summary, one line per file with its
 * SHA-256 in hexadecimal, two spaces, and its path.
 */
export async function treeHash(directory: string): Promise<string> {
  const summary = createHash("sha256");
  for (const path of await treeFiles(directory)) {
    const digest = createHash("sha256")
      .update(await readFile(join(directory, path)))
      .digest("hex");
    summary.update(`${digest}  ${path}\n`);
  }
  return `h1:${summary.digest("base64")}`;
}
