// Generated packages in temporary directories, and repeated timed runs.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { generatePackage, SIZES, type GeneratedPackage, type SizeName } from "./gen.ts";
import { runHd, type HdCommand, type RunOptions, type RunResult } from "./hd.ts";
import { copyTree, makeTempDir, writeTree } from "./tmp.ts";

/** Writes a generated package of a named size into a fresh temporary directory. */
export function materialize(
  size: SizeName,
  seed: string,
): { readonly dir: string; readonly pkg: GeneratedPackage } {
  const pkg = generatePackage({ seed, lines: SIZES[size] });
  const dir = makeTempDir(`pkg-${size}`);
  writeTree(dir, pkg.files);
  return { dir, pkg };
}

/**
 * Adds the executable `src/main.hd` to a generated package, so `hd build`
 * has a program to write. It prints the total of one item through the
 * package's library.
 */
export function addExecutable(dir: string): void {
  writeTree(
    dir,
    new Map([
      [
        "src/main.hd",
        [
          "use pkg.{one_total}",
          "use pkg.m000.Item0",
          "",
          "pub fn main() -> void $ Console:",
          '    println(one_total(Item0 { name: "pen", price: 2, count: 3 }))',
          "",
        ].join("\n"),
      ],
    ]),
  );
}

/** A fresh temporary copy of a package directory. */
export function copyPackage(source: string, label: string): string {
  const dir = makeTempDir(label);
  copyTree(source, dir);
  return dir;
}

/** Rewrites one file of a package directory in place. */
export function editFile(dir: string, file: string, edit: (text: string) => string): void {
  const path = join(dir, file);
  writeFileSync(path, edit(readFileSync(path, "utf8")));
}

/** A fresh, empty cache directory, passed to `hd` as HD_CACHE. */
export const freshCache = (): Record<string, string> => ({ HD_CACHE: makeTempDir("cache") });

/** The first lines of a run's output, for a failure note. */
export const firstLines = (result: RunResult, count = 2): string =>
  (result.stdout + result.stderr).trim().split("\n").slice(0, count).join(" | ").slice(0, 300);

/** Why a run that should succeed did not, or undefined when it succeeded. */
export function runProblem(result: RunResult, timeoutMs: number, what: string): string | undefined {
  if (result.timedOut) return `${what} timed out after ${timeoutMs / 1000} s`;
  if (result.status !== 0) return `${what} exited ${String(result.status)}: ${firstLines(result)}`;
  return undefined;
}

export interface Series {
  readonly samples: number[];
  /** Set when a run failed or timed out; later runs were skipped. */
  readonly problem?: string;
}

/**
 * Runs `step` up to `count` times and collects the wall time of each run.
 * `before` runs ahead of each timed run, as an edit. The series stops at
 * the first run that times out or fails.
 */
export async function timedSeries(
  hd: HdCommand,
  count: number,
  args: (index: number) => readonly string[],
  options: RunOptions & { readonly timeoutMs: number },
  what: string,
  before?: (index: number) => void,
): Promise<Series> {
  const samples: number[] = [];
  for (let index = 0; index < count; index++) {
    before?.(index);
    const result = await runHd(hd, args(index), options);
    const problem = runProblem(result, options.timeoutMs, `${what} ${index + 1}`);
    if (problem) return { samples, problem };
    samples.push(result.wallMs);
  }
  return { samples };
}

/**
 * A small generated package whose test case `t001a` fails: its expected
 * total is off by one. Returns the directory and the failing test's filter.
 */
export function failingPackage(seed: string): { readonly dir: string; readonly filter: string } {
  const { dir, pkg } = materialize("small", seed);
  const module = pkg.modules[1]!;
  editFile(dir, module.file, (text) => {
    const next = text.replace(
      /(assert_equal\(total\d+\(items\), )6,/,
      (_, head: string) => `${head}7,`,
    );
    if (next === text) throw new Error("no test expectation to break");
    return next;
  });
  return { dir, filter: module.testName };
}
