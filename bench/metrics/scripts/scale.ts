// scale: the generated scale check (#1880). AGENTS.md "Performant":
// nothing is quadratic or exponential in program size.
//
// The shapes of lib/scale.ts are correct programs generated for this
// check alone, one construct repeated or nested k times; together they
// hold every node kind of the parser. Each runs at k = 0, n and 4n
// through `hd check`, `hd fmt --check` and, where it builds, `hd build`
// (so Collect and Link are measured too). Diagnostics are not scaled
// (owner, 2026-11-14: "don't optimize diagnostic for now, optimize
// correct code").
//
// The measure is not time: `hd` with HD_WORK=FILE writes its work
// counters there (crates/hd_base/src/work.rs: allocations and their
// bytes, table hashes, and every opt-in `work::count_*` site of the
// compiler). Runs are serial (HD_JOBS=1) from a copy of one warm std
// cache: the compiler's counters repeat exactly, and allocations vary by
// a few dozen with file-system names. Subtracting k = 0 leaves the
// program's own work; a counter whose work grows by more than 4.5x per 4x
// of the program's bytes, with at least FLOOR units at 4n, is superlinear
// and fails its line, which names the shape, the command, the counter
// and its two counts. The counters miss work that neither allocates nor
// hashes, as a linear scan per lookup.
//
// The coverage table closes the run: of the parser's closed `SyntaxKind`
// set (crates/hd_syntax/src/green.rs), as `parse_check --tree` dumps each
// shape (cargo build --release -p hd_syntax --example parse_check;
// HD_SCALE_PARSER names another), every kind but KIND_EXEMPT must appear
// in some shape, so a new kind with no shape fails; the kinds repeated
// and nested shapes hold are reported apart, with the misses listed.
//
// KNOWN lists the superlinear lines already filed: such a line passes
// with its issue in the note, and fails once it turns linear, so the row
// is removed with the fix. HD_SCALE_SHAPES=A,B measures only those shapes.
//
// Every hd run has a scratch HD_CACHE of its own (`measuredEnv`): a copy
// of one warm std cache, removed with the run's directory, never the
// default cache. Cache work still allocates and hashes, so it is in the
// counters, as the same constant at every size.
//
// Cost: 3 runs per shape and command, about 140 runs, POOL at a time,
// and one parser dump per shape.

import { availableParallelism } from "node:os";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { manifest } from "../lib/fixtures.ts";
import { firstLines, runHd, runProcess, type HdCommand } from "../lib/hd.ts";
import type { MetricContext } from "../lib/metric.ts";
import { failed, type Metric, type TargetResult } from "../lib/metric.ts";
import { SHAPES, type Form, type Shape } from "../lib/scale.ts";
import { copyTree, makeTempDir, removeTempDir, writeTree } from "../lib/tmp.ts";

const NAME = "scale";
const TIMEOUT_MS = 120_000;
/** Work at 4n over work at n, beyond which a counter is superlinear. */
export const GROWTH = 4.5;
/** Units a counter must spend at 4n, over k = 0, before its growth counts. */
export const FLOOR = 1_000;
/**
 * The floor of allocated bytes: a buffer's doubling moves a small run's
 * bytes by more than its own work, so a nest of a few dozen levels with
 * no allocation over k = 0 at n would otherwise fail on rounding alone.
 */
export const BYTES_FLOOR = 1_000_000;
const POOL = Math.max(1, Math.min(4, availableParallelism()));

export type Command = "check" | "fmt" | "build";

const ARGS: Readonly<Record<Command, readonly string[]>> = {
  check: ["check"],
  fmt: ["fmt", "--check"],
  build: ["build"],
};

/**
 * Superlinear lines already found, as `SHAPE COMMAND` to the issue. The
 * first run's findings name the issue that tracks each one.
 */
export const KNOWN: ReadonlyMap<string, string> = new Map([
  ["dyn-values build", "#1908"],
  ["row-chain build", "#1920"],
  ["closures build", "#1907"],
  ["matches build", "#1917"],
  ["enum-arms build", "#1909"],
  ["binary-chain build", "#1904"],
  ["interpolation build", "#1914"],
  ["for-nest build", "#1916"],
  ["while-nest build", "#1916"],
  ["closure-nest build", "#1906"],
  ["option-nest check", "#1918"],
  ["option-nest build", "#1918"],
  ["list-type-nest build", "#1915"],
  ["generic-nest check", "#1912"],
  ["generic-nest build", "#1912"],
  ["paren-nest build", "#1919"],
  ["bool-nest build", "#1905"],
  ["tuple-nest build", "#1924"],
  ["tuple-nest check", "#1924"],
  ["if-expr-nest build", "#1913"],
  ["fn-nest build", "#1911"],
]);

/** Each counter's total in one run. */
export type Work = Readonly<Record<string, number>>;

/**
 * The limit of each counter's growth for 4x the program's own bytes. An
 * allocation's bytes are rounded up by a growing buffer's doubling, so a
 * buffer whose length grows 4x may allocate up to 8x: allocated bytes get
 * one doubling more.
 */
const limitOf = (counter: string): number => (counter === "alloc_bytes" ? 2 * GROWTH : GROWTH);

export interface Growth {
  readonly counter: string;
  /** Work over k = 0 at n and at 4n. */
  readonly small: number;
  readonly large: number;
  readonly ratio: number;
  /** The ratio over which this counter is superlinear. */
  readonly limit: number;
}

/**
 * Each counter's growth from n to 4n over the k = 0 run, worst first, and
 * whether it is superlinear: over its limit, with at least FLOOR units at
 * 4n (BYTES_FLOOR for allocated bytes). `bytes` is the program's own
 * growth in bytes over k = 0, 4 for a repeated shape and more for a shape
 * whose indentation deepens per level; the limits scale with it, so the
 * measure is per byte of input.
 * Linear in the counters.
 */
export function growths(
  zero: Work,
  small: Work,
  large: Work,
  bytes = 4,
): (Growth & { bad: boolean })[] {
  return Object.keys(large)
    .map((counter) => {
      const base = zero[counter] ?? 0;
      const d1 = Math.max(0, (small[counter] ?? 0) - base);
      const d4 = Math.max(0, large[counter]! - base);
      let ratio = d4 / d1;
      if (d1 === 0) {
        ratio = d4 === 0 ? 0 : Infinity;
      }
      const limit = (limitOf(counter) * bytes) / 4;
      const floor = counter === "alloc_bytes" ? BYTES_FLOOR : FLOOR;
      return { counter, small: d1, large: d4, ratio, limit, bad: d4 >= floor && ratio > limit };
    })
    .sort((left, right) => right.ratio / right.limit - left.ratio / left.limit);
}

/** A shape's work at k = 0, n and 4n. */
export interface Three {
  readonly zero: Work;
  readonly small: Work;
  readonly large: Work;
}

/**
 * The line of one shape and command, from its three runs. `known` is its
 * KNOWN issue; `bytes` its program's byte growth (see `growths`).
 */
export function judgeShape(
  label: string,
  { zero, small, large }: Three,
  { known, bytes = 4 }: { readonly known?: string; readonly bytes?: number } = {},
): TargetResult {
  const all = growths(zero, small, large, bytes);
  const bad = all.filter((growth) => growth.bad);
  const counted = all.filter((growth) => growth.large >= FLOOR);
  const shown = bad[0] ?? counted[0];
  const text = (growth: Growth) =>
    `${growth.counter} ${growth.small} → ${growth.large} (${Number.isFinite(growth.ratio) ? growth.ratio.toFixed(2) : "∞"}x, limit ${growth.limit.toFixed(2)}x)`;
  const value = shown && Number.isFinite(shown.ratio) ? `${shown.ratio.toFixed(2)}x` : "-";
  let note = "no counter reached the floor";
  if (bad.length > 0) {
    note = bad.map(text).join("; ");
  } else if (shown) {
    note = text(shown);
  }
  const target = bytes === 4 ? `≤ ${GROWTH}x` : `≤ ${GROWTH}x per 4x bytes (${bytes.toFixed(1)}x)`;
  const base = { metric: NAME, name: label, value, target };
  if (known !== undefined) {
    return bad.length > 0
      ? { ...base, status: "pass", note: `known superlinear, ${known}: ${note}` }
      : { ...base, status: "fail", note: `now linear: remove the KNOWN row (${known}); ${note}` };
  }
  return { ...base, status: bad.length > 0 ? "fail" : "pass", note };
}

interface Run {
  readonly work?: Work;
  readonly problem?: string;
}

/** The package `scale` warms the std cache with, unlike every shape. */
const WARM: ReadonlyMap<string, string> = new Map([
  ["hd.toml", manifest("warm", { capabilities: ["Console"] })],
  ["src/main.hd", 'pub fn main() -> void $ Console:\n    println("warm")\n'],
]);

/**
 * The environment of one `hd` run of the check: serial, with `cache` as
 * its `HD_CACHE`, a scratch directory of its own. No run reads, walks or
 * locks the default cache (#1932), and each run's cache work is the same
 * small constant over a copy of one warm std cache, which subtracting
 * k = 0 removes. Throws before any run when `cache` is empty.
 */
export function measuredEnv(cache: string, extra: Readonly<Record<string, string>> = {}) {
  if (!cache) {
    throw new Error("scale: every hd run needs a scratch HD_CACHE of its own");
  }
  return { HD_JOBS: "1", ...extra, HD_CACHE: cache };
}

/** One run of `command` on `shape` at size k, from a copy of the warm `cache`. */
async function measure(
  { hd, cache }: { readonly hd: HdCommand; readonly cache: string },
  shape: Shape,
  command: Command,
  k: number,
): Promise<Run> {
  const dir = makeTempDir(NAME);
  try {
    writeTree(dir, shape.files(k));
    const work = join(dir, "work.json");
    const runCache = join(dir, ".cache");
    copyTree(cache, runCache);
    const result = await runHd(hd, ARGS[command], {
      cwd: dir,
      timeoutMs: TIMEOUT_MS,
      env: measuredEnv(runCache, { HD_WORK: work }),
    });
    // `fmt --check` exits 1 when a file would change; it still counted.
    const expected = result.status === 0 || (command === "fmt" && result.status === 1);
    if (result.timedOut || !expected) {
      return {
        problem: `k = ${k}: ${result.timedOut ? "timed out" : `exit ${result.status}`}: ${firstLines(result)}`,
      };
    }
    return { work: JSON.parse(readFileSync(work, "utf8")) as Work };
  } catch (error) {
    return { problem: `k = ${k}: ${error instanceof Error ? error.message : String(error)}` };
  } finally {
    removeTempDir(dir);
  }
}

/** Runs `jobs` at most `width` at a time, keeping their order. */
async function pool<Result>(
  jobs: readonly (() => Promise<Result>)[],
  width: number,
): Promise<Result[]> {
  const out: Result[] = [];
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const index = next++;
      out[index] = await jobs[index]!();
    }
  };
  await Promise.all(Array.from({ length: Math.min(width, jobs.length) }, worker));
  return out;
}

/** One shape's three runs per command. */
async function measureShape(
  context: MetricContext,
  cache: string,
  shape: Shape,
): Promise<ReadonlyMap<Command, readonly [Run, Run, Run]>> {
  const commands: Command[] = ["check", "fmt", ...(shape.builds ? ["build" as const] : [])];
  const runs = new Map<Command, [Run, Run, Run]>();
  for (const command of commands) {
    context.log(`${NAME}: ${shape.name} ${command}`);
    const three: Run[] = [];
    for (const k of [0, shape.n, 4 * shape.n]) {
      three.push(await measure({ hd: context.hd, cache }, shape, command, k));
    }
    runs.set(command, three as [Run, Run, Run]);
  }
  return runs;
}

/** The line of one shape and command. */
function lineOf(shape: Shape, command: Command, [zero, small, large]: readonly [Run, Run, Run]) {
  const label = `${shape.name} ${command}`;
  const problem = zero.problem ?? small.problem ?? large.problem;
  if (problem) {
    return failed(NAME, label, `≤ ${GROWTH}x`, problem);
  }
  const bytes = (k: number) =>
    [...shape.files(k).values()].reduce((sum, text) => sum + Buffer.byteLength(text), 0);
  const b0 = bytes(0);
  const growth = (bytes(4 * shape.n) - b0) / (bytes(shape.n) - b0);
  const work = { zero: zero.work!, small: small.work!, large: large.work! };
  const known = KNOWN.get(label);
  return judgeShape(label, work, { ...(known === undefined ? {} : { known }), bytes: growth });
}

/**
 * Node kinds no correct program can hold, each with why. Every other kind
 * of the parser's closed set must appear in some generated program.
 */
export const KIND_EXEMPT: ReadonlyMap<string, string> = new Map([
  [
    "Error",
    "ruled out: only source that does not parse holds it, and the check scales correct programs (owner, 2026-11-14)",
  ],
  ["SkippedBody", "ruled out: no parser path builds it (gap: #1927)"],
]);

/** The parser's closed set of node kinds: the variants of `SyntaxKind`. */
export function syntaxKinds(green: string): string[] {
  const body = green.slice(green.indexOf("pub enum SyntaxKind {"));
  return [...body.slice(0, body.indexOf("\n}")).matchAll(/^\s+([A-Z]\w*),/gm)].map(
    (match) => match[1]!,
  );
}

/** The node kinds of a `parse_check --tree` dump: each line's first word. */
export function kindsOfTree(dump: string): Set<string> {
  return new Set([...dump.matchAll(/^\s*([A-Z]\w*) /gm)].map((match) => match[1]!));
}

/**
 * The node kinds of each shape: one `parse_check --tree` dump per shape at
 * k = 1 (copies repeat their kinds), or why there is none.
 */
async function shapeKinds(
  context: MetricContext,
  shapes: readonly Shape[],
): Promise<Set<string>[] | string> {
  const parser =
    process.env.HD_SCALE_PARSER ?? join(context.repoRoot, "target/release/examples/parse_check");
  if (!existsSync(parser)) {
    return `no ${parser}: cargo build --release -p hd_syntax --example parse_check`;
  }
  const kinds: Set<string>[] = [];
  for (const shape of shapes) {
    const dir = makeTempDir(`${NAME}-kinds`);
    writeTree(dir, shape.files(1));
    const dump = await runProcess([parser, "--tree", dir], { cwd: dir, timeoutMs: TIMEOUT_MS });
    removeTempDir(dir);
    kinds.push(kindsOfTree(dump.stdout));
  }
  return kinds;
}

const SUFFIX = { any: "", repeat: " repeated", nest: " nested" } as const;

/**
 * The coverage table: of the parser's closed set of node kinds, less
 * KIND_EXEMPT, the kinds some shape holds (each must: a new kind with no
 * shape fails), and those a repeated and a nested shape hold (reported,
 * with the misses listed).
 */
async function kindLines(
  context: MetricContext,
  shapes: readonly Shape[],
): Promise<TargetResult[]> {
  const all = syntaxKinds(
    readFileSync(join(context.repoRoot, "crates/hd_syntax/src/green.rs"), "utf8"),
  ).filter((kind) => !KIND_EXEMPT.has(kind));
  const target = `${all.length}/${all.length}`;
  const kinds = await shapeKinds(context, shapes);
  if (typeof kinds === "string") {
    return [failed(NAME, "coverage: node kinds", target, kinds)];
  }
  const line = (form: Form | undefined): TargetResult => {
    const seen = new Set(
      shapes.flatMap((shape, index) =>
        form === undefined || shape.form === form ? [...kinds[index]!] : [],
      ),
    );
    const missing = all.filter((kind) => !seen.has(kind));
    const where = form === undefined ? "shape" : `${form} shape`;
    const misses = missing.length > 0 ? `in no ${where}: ${missing.join(", ")}; ` : "";
    return {
      metric: NAME,
      name: `coverage: node kinds${SUFFIX[form ?? "any"]}`,
      value: `${all.length - missing.length}/${all.length}`,
      target: form === undefined ? target : "reported",
      status: form === undefined && missing.length > 0 ? "fail" : "pass",
      note: `${misses}exempt: ${[...KIND_EXEMPT.keys()].join(", ")}`,
    };
  };
  return [line(undefined), line("repeat"), line("nest")];
}

/** The lines of `shapes`, measured with `hd`, and the node-kind coverage table. */
export async function scaleLines(
  context: MetricContext,
  shapes: readonly Shape[],
): Promise<TargetResult[]> {
  // The warm std cache each run copies: a scratch directory, removed
  // with the warm package once every shape is measured.
  const cache = makeTempDir(`${NAME}-cache`);
  const warm = makeTempDir(`${NAME}-warm`);
  let measured: ReadonlyMap<Command, readonly [Run, Run, Run]>[];
  try {
    writeTree(warm, WARM);
    for (const args of [["check"], ["build"]]) {
      const result = await runHd(context.hd, args, {
        cwd: warm,
        timeoutMs: TIMEOUT_MS,
        env: measuredEnv(cache),
      });
      if (result.status !== 0) {
        return [failed(NAME, "warm std cache", "builds", firstLines(result))];
      }
    }
    measured = await pool(
      shapes.map((shape) => () => measureShape(context, cache, shape)),
      POOL,
    );
  } finally {
    removeTempDir(warm);
    removeTempDir(cache);
  }
  const lines = shapes.flatMap((shape, index) =>
    [...measured[index]!].map(([command, three]) => lineOf(shape, command, three)),
  );
  return [...lines, ...(await kindLines(context, shapes))];
}

/** The shapes named in `names` (comma-separated), or every one. */
export function selected(names: string | undefined): readonly Shape[] {
  if (!names) {
    return SHAPES;
  }
  const wanted = new Set(names.split(","));
  const found = SHAPES.filter((shape) => wanted.delete(shape.name));
  if (wanted.size > 0) {
    throw new Error(`HD_SCALE_SHAPES names no shape ${[...wanted].join(", ")}`);
  }
  return found;
}

export const scale: Metric = {
  name: NAME,
  pillar: 1,
  summary: "every syntax node kind, repeated and nested, at n and 4n: no counter grows over 4.5x",
  run: (context) => scaleLines(context, selected(process.env.HD_SCALE_SHAPES)),
};
