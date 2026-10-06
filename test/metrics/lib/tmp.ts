// Temporary directories. Every file a metric writes lives in one of these,
// never in the repository: generated packages, edited copies, and caches.

import { cpSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";

const made = new Set<string>();
let keep = false;

/** Keep temporary directories after the run, for debugging a metric. */
export function keepTempDirs(value: boolean): void {
  keep = value;
}

/** A fresh directory under the system temporary directory. */
export function makeTempDir(label: string): string {
  const dir = mkdtempSync(join(tmpdir(), `hd-metrics-${label}-`));
  made.add(dir);
  return dir;
}

/** Removes one directory made by `makeTempDir`, unless directories are kept. */
export function removeTempDir(dir: string): void {
  made.delete(dir);
  if (!keep) rmSync(dir, { recursive: true, force: true });
}

/** Removes every directory still left, as at the runner's exit. */
export function removeAllTempDirs(): void {
  for (const dir of made) removeTempDir(dir);
}

/** Runs `body` with a fresh directory and removes the directory afterwards. */
export async function withTempDir<T>(label: string, body: (dir: string) => Promise<T>): Promise<T> {
  const dir = makeTempDir(label);
  try {
    return await body(dir);
  } finally {
    removeTempDir(dir);
  }
}

/** Writes a map of relative paths to file text under `root`. */
export function writeTree(root: string, files: ReadonlyMap<string, string>): void {
  for (const [path, text] of files) {
    const full = join(root, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }
}

/** Copies the directory `from` to `to`. */
export function copyTree(from: string, to: string): void {
  cpSync(from, to, { recursive: true });
}

/** Every file under `root`, as relative paths, sorted. */
export function listTree(root: string): string[] {
  const files: string[] = [];
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else files.push(relative(root, full));
    }
  };
  walk(root);
  return files.sort();
}

/** The text of every file under `root`, keyed by relative path. */
export function readTree(root: string): Map<string, string> {
  return new Map(listTree(root).map((path) => [path, readFileSync(join(root, path), "utf8")]));
}
