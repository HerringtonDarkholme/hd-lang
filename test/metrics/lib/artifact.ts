// Building and timing user programs, for the pillar 3 metrics.
//
// A program is a package of one executable, `src/main.hd`, built with
// `hd build --release` (plain `hd build` when the hd under test has no
// `--release`). What the build wrote decides how the program runs: a
// `.wasm` file runs as `hd FILE.wasm` (cli.wasm.run), an executable file
// runs directly, and with neither the program runs as `hd run --release`.
// Its timings then include the build, which the metrics cancel by taking
// differences.
//
// The Node comparisons are self-timing JavaScript programs, as in
// test/perf/micro: each warms up in its process, times its own runs, and
// prints one JSON line.

import { accessSync, constants, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { copyPackage, firstLines, runProblem } from "./fixture.ts";
import { runHd, runProcess, supportsFlag, type HdCommand, type RunResult } from "./hd.ts";
import { p50 } from "./stats.ts";
import { listTree, makeTempDir, removeTempDir, writeTree } from "./tmp.ts";

/** The package name of every built program; the executable takes it. */
export const PACKAGE = "bench";

export interface Program {
  /** The package directory, the working directory of every run. */
  readonly dir: string;
  /** The command that runs the built program. */
  readonly argv: readonly string[];
  readonly how: "wasm" | "native" | "hd run";
  /** The file the build wrote, absolute; undefined for `hd run`. */
  readonly artifact?: string;
  /** Its size in bytes. */
  readonly bytes?: number;
  readonly release: boolean;
}

const releaseFlags = new Map<string, Promise<boolean>>();

/** Whether `hd help build` names `--release`; asked once per hd command. */
export function hasReleaseBuild(hd: HdCommand): Promise<boolean> {
  let found = releaseFlags.get(hd.display);
  if (!found) {
    found = supportsFlag(hd, "build", "--release", makeTempDir("release-probe"));
    releaseFlags.set(hd.display, found);
  }
  return found;
}

const isExecutable = (path: string): boolean => {
  try {
    accessSync(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
};

/**
 * The file a build wrote for the package's executable: a `.wasm` file
 * named after the package, else any `.wasm`, else an executable file. With
 * `release`, files under a `release` directory come first. `files` are
 * relative to the package's `build/`.
 */
export function pickArtifact(
  files: readonly string[],
  release: boolean,
  executable: (path: string) => boolean = () => false,
): { readonly path: string; readonly kind: "wasm" | "native" } | undefined {
  const ranked = [...files].sort((a, b) => {
    const score = (path: string) =>
      (release && path.split("/").includes("release") ? 0 : 2) +
      (path.split("/").at(-1) === `${PACKAGE}.wasm` ? 0 : 1);
    return score(a) - score(b) || a.localeCompare(b);
  });
  const wasm = ranked.find((path) => path.endsWith(".wasm"));
  if (wasm) return { path: wasm, kind: "wasm" };
  const native = ranked.find((path) => executable(path));
  return native ? { path: native, kind: "native" } : undefined;
}

/** `[package]` with the name every program shares. */
export const MANIFEST = `[package]\nname = "${PACKAGE}"\n`;

/**
 * Writes a package whose `src/main.hd` is `main` (or the given files, which
 * then hold `hd.toml` too) and builds it. Throws when the build fails.
 */
export async function buildProgram(
  hd: HdCommand,
  main: string | ReadonlyMap<string, string>,
  label: string,
  timeoutMs = 120_000,
): Promise<Program> {
  const dir = makeTempDir(label);
  writeTree(
    dir,
    typeof main === "string"
      ? new Map([
          ["hd.toml", MANIFEST],
          ["src/main.hd", main],
        ])
      : main,
  );
  const release = await hasReleaseBuild(hd);
  const args = release ? ["build", "--release"] : ["build"];
  const result = await runHd(hd, args, { cwd: dir, timeoutMs });
  const problem = runProblem(result, timeoutMs, `hd ${args.join(" ")} (${label})`);
  if (problem) throw new Error(problem);
  const buildDir = join(dir, "build");
  let files: string[] = [];
  try {
    files = listTree(buildDir);
  } catch {
    // No build directory: the program runs through `hd run`.
  }
  const found = pickArtifact(files, release, (path) => isExecutable(join(buildDir, path)));
  if (!found)
    return {
      dir,
      argv: [...hd.argv, "run", ...(release ? ["--release"] : [])],
      how: "hd run",
      release,
    };
  const artifact = join(buildDir, found.path);
  return {
    dir,
    argv: found.kind === "wasm" ? [...hd.argv, artifact] : [artifact],
    how: found.kind,
    artifact,
    bytes: statSync(artifact).size,
    release,
  };
}

/** A note on how a program ran, for a result line. */
export const howNote = (program: Program): string =>
  `${program.release ? "release" : "debug (no --release)"} build, run as ${
    program.how === "wasm"
      ? "hd FILE.wasm"
      : program.how === "native"
        ? "a native executable"
        : "hd run"
  }`;

export interface RunProgramOptions {
  readonly timeoutMs: number;
  readonly env?: Readonly<Record<string, string>>;
  readonly measure?: boolean;
}

/** Runs a built program once; throws when it fails or times out. */
export async function runProgram(
  program: Program,
  label: string,
  options: RunProgramOptions,
): Promise<RunResult> {
  const result = await runProcess(program.argv, { cwd: program.dir, ...options });
  const problem = runProblem(result, options.timeoutMs, label);
  if (problem) throw new Error(problem);
  return result;
}

/**
 * Times `runs` runs of each program after one untimed warm-up run of each.
 * Runs interleave, so a slow moment of the machine touches every program
 * alike. Returns the wall times, in ms, per program. Every run of a program
 * must print the same output.
 */
export async function timeInterleaved(
  programs: readonly Program[],
  runs: number,
  label: string,
  timeoutMs: number,
): Promise<number[][]> {
  return (await timeWithOutput(programs, runs, label, timeoutMs)).samples;
}

/** `timeInterleaved`, with the standard output of each program. */
export async function timeWithOutput(
  programs: readonly Program[],
  runs: number,
  label: string,
  timeoutMs: number,
): Promise<{ readonly samples: number[][]; readonly outputs: string[] }> {
  const outputs: (string | undefined)[] = programs.map(() => undefined);
  const samples: number[][] = programs.map(() => []);
  for (let round = -1; round < runs; round++) {
    for (const [index, program] of programs.entries()) {
      const result = await runProgram(program, `${label} (program ${index + 1})`, { timeoutMs });
      if (outputs[index] !== undefined && result.stdout !== outputs[index])
        throw new Error(`${label}: program ${index + 1} printed different output between runs`);
      outputs[index] = result.stdout;
      if (round >= 0) samples[index]!.push(result.wallMs);
    }
  }
  return { samples, outputs: outputs.map((output) => output ?? "") };
}

/**
 * Builds a program at two sizes, `source(small)` and `source(large)`, times
 * both (`timeWithOutput`), and returns the median ms per unit of size, with
 * the large program and both outputs.
 */
export async function scaledCost(
  hd: HdCommand,
  source: (size: number) => string,
  sizes: { readonly small: number; readonly large: number },
  label: string,
  runs: number,
  timeoutMs: number,
): Promise<{
  readonly msPerUnit: number;
  readonly program: Program;
  readonly small: string;
  readonly large: string;
}> {
  const tag = label.replaceAll(/[^a-z0-9]+/gi, "-");
  const small = await buildProgram(hd, source(sizes.small), `${tag}-small`);
  const large = await buildProgram(hd, source(sizes.large), `${tag}-large`);
  const { samples, outputs } = await timeWithOutput([large, small], runs, label, timeoutMs);
  return {
    msPerUnit: unitCost(samples[0]!, samples[1]!, sizes.large - sizes.small),
    program: large,
    small: outputs[1]!,
    large: outputs[0]!,
  };
}

export interface CommandTiming {
  /** Per package directory, the wall time of each timed run, in ms. */
  readonly samples: number[][];
  /** Per package directory, the output of its last run, stdout then stderr. */
  readonly outputs: string[];
}

/**
 * Times one `hd` command, such as `hd test`, in several package
 * directories: one untimed warm-up run in each, then `runs` rounds,
 * interleaved. A run must exit 0, unless `expectFailure`, when it must exit
 * with another status. With `fresh`, every run uses a fresh copy of its
 * package, so files a run writes, such as saved property regressions, never
 * reach the next.
 */
export async function timeCommand(
  hd: HdCommand,
  dirs: readonly string[],
  args: readonly string[],
  options: {
    readonly runs: number;
    readonly timeoutMs: number;
    readonly label: string;
    readonly expectFailure?: boolean;
    readonly fresh?: boolean;
  },
): Promise<CommandTiming> {
  const samples: number[][] = dirs.map(() => []);
  const outputs: string[] = dirs.map(() => "");
  for (let round = -1; round < options.runs; round++) {
    for (const [index, dir] of dirs.entries()) {
      const cwd = options.fresh ? copyPackage(dir, "fresh") : dir;
      const result = await runHd(hd, args, { cwd, timeoutMs: options.timeoutMs });
      const what = `${options.label} (package ${index + 1}, hd ${args.join(" ")})`;
      if (result.timedOut) throw new Error(`${what} timed out after ${options.timeoutMs / 1000} s`);
      if (
        options.expectFailure ? result.status === 0 || result.status === null : result.status !== 0
      )
        throw new Error(
          `${what} exited ${String(result.status)}${options.expectFailure ? ", not a test failure" : ""}: ${firstLines(result)}`,
        );
      outputs[index] = result.stdout + result.stderr;
      if (round >= 0) samples[index]!.push(result.wallMs);
      if (options.fresh) removeTempDir(cwd);
    }
  }
  return { samples, outputs };
}

/** The test cases a test run reports as passed: the sum of its "N passed" counts. */
export const passedCount = (output: string): number =>
  [...output.matchAll(/\b(\d+) passed\b/g)].reduce((sum, match) => sum + Number(match[1]), 0);

/** The number a program printed after `key=`, as `bytes=1200`. */
export function printed(stdout: string, key: string): number {
  const match = new RegExp(`\\b${key}=(\\d+)\\b`).exec(stdout);
  if (!match) throw new Error(`the program printed no ${key}=N: ${stdout.slice(0, 120)}`);
  return Number(match[1]);
}

/**
 * The cost of one unit of work from paired timings: run i of the large
 * program minus run i of the small one, over the units between them.
 */
export function perUnit(
  large: readonly number[],
  small: readonly number[],
  units: number,
): number[] {
  return large.map((ms, index) => (ms - small[index]!) / units);
}

/** The median of `perUnit`, guarded: a cost at or below zero means noise ate the work. */
export function unitCost(
  large: readonly number[],
  small: readonly number[],
  units: number,
): number {
  return p50(perUnit(large, small, units));
}

/**
 * The JavaScript prelude of a self-timing Node program: `measure(fn)` runs
 * `fn` WARMUP times, then RUNS timed times, and returns the times in ms.
 */
export const NODE_PRELUDE = [
  "const WARMUP = 3;",
  "const RUNS = 7;",
  "function measure(fn) {",
  "  for (let i = 0; i < WARMUP; i++) fn();",
  "  const times = [];",
  "  for (let i = 0; i < RUNS; i++) {",
  "    const start = performance.now();",
  "    fn();",
  "    times.push(performance.now() - start);",
  "  }",
  "  return times;",
  "}",
  "",
].join("\n");

/**
 * Runs a self-timing Node program, NODE_PRELUDE plus `body`, in its own
 * process, and returns the JSON object of its last output line.
 */
export async function runNodeProgram(
  body: string,
  label: string,
  timeoutMs = 120_000,
): Promise<Record<string, unknown>> {
  const dir = makeTempDir("node-bench");
  const file = join(dir, `${label}.mjs`);
  writeFileSync(file, NODE_PRELUDE + body);
  const result = await runProcess([process.execPath, file], { cwd: dir, timeoutMs });
  const problem = runProblem(result, timeoutMs, `node ${label}.mjs`);
  if (problem) throw new Error(problem);
  const last = result.stdout.trim().split("\n").at(-1) ?? "";
  try {
    return JSON.parse(last) as Record<string, unknown>;
  } catch {
    throw new Error(`node ${label}.mjs printed no JSON: ${firstLines(result)}`);
  }
}

/** The median of a list of numbers a Node program printed under `key`. */
export function nodeMedian(result: Record<string, unknown>, key: string): number {
  const value = result[key];
  if (!Array.isArray(value) || value.length === 0 || value.some((v) => typeof v !== "number"))
    throw new Error(`the Node program printed no ${key} list`);
  return p50(value as number[]);
}

/**
 * A V8 heap probe for an hd that runs programs on Node. `env` preloads a
 * module through NODE_OPTIONS that turns on `--trace-gc-nvp` and, at exit,
 * writes `v8.getHeapStatistics()` to a file per process. An hd that is not
 * a Node process ignores both, and `allocated` then returns undefined.
 */
export function heapProbe(): {
  readonly env: (label: string) => Record<string, string>;
  readonly allocated: (label: string, stdout: string) => number | undefined;
} {
  const dir = makeTempDir("heap-probe");
  const probe = join(dir, "probe.mjs");
  writeFileSync(
    probe,
    [
      'import { writeFileSync } from "node:fs";',
      'import v8 from "node:v8";',
      'v8.setFlagsFromString("--trace-gc-nvp");',
      "const out = process.env.HD_METRICS_HEAP_FILE;",
      'process.on("exit", () => {',
      "  if (out) writeFileSync(`${out}.${process.pid}`, JSON.stringify(v8.getHeapStatistics()));",
      "});",
      "",
    ].join("\n"),
  );
  const inherited = process.env.NODE_OPTIONS;
  return {
    env: (label) => ({
      NODE_OPTIONS: `${inherited ? `${inherited} ` : ""}--import=${pathToFileURL(probe).href}`,
      HD_METRICS_HEAP_FILE: join(dir, label),
    }),
    allocated: (label, stdout) => {
      const exits = listTree(dir).filter((file) => file.startsWith(`${label}.`));
      if (exits.length === 0) return undefined;
      const used = exits.reduce((sum, file) => {
        const stats = JSON.parse(readFileSync(join(dir, file), "utf8")) as {
          used_heap_size?: number;
        };
        return sum + (stats.used_heap_size ?? 0);
      }, 0);
      return used + freedBytes(stdout);
    },
  };
}

/**
 * The bytes all garbage collections in a `--trace-gc-nvp` log freed: the
 * sum of `start_object_size - end_object_size` over its lines. With the
 * heap in use at exit, it gives the bytes allocated over the whole run.
 */
export function freedBytes(log: string): number {
  let freed = 0;
  for (const match of log.matchAll(/start_object_size=(\d+)\b.*?\bend_object_size=(\d+)\b/g))
    freed += Number(match[1]) - Number(match[2]);
  return freed;
}
