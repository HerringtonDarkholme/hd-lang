// cache-contention: many processes writing the same cache entries at once.
//
// Dependency cache: a local remote (lib/deps.ts) serves one tagged
// dependency and counts connections. 8 copies of a dependent package share
// one empty HD_CACHE, and 8 `hd check` processes start at once, so all of
// them need the same entry.
// - No corruption: every check succeeds; the entry pkg/HOST_PATH@VERSION
//   holds the tag's files byte for byte (cli.cache.entry); and with the
//   remote gone, a second check in each copy succeeds from the cache.
// - Fetched once: the 8 checks connect no more often than one check does
//   with a cache of its own.
// Compilation cache, when `hd check` keeps one (findCompileCache): 8 copies
// of the small package, one shared empty HD_CACHE, 8 checks at once.
// - No corruption: every check succeeds, and so does a second check in
//   each copy.
// - Written once: the shared cache then holds the same files as after a
//   single check with a cache of its own.
// Targets (Pillar 2): no corruption; each entry computed once.
// n/a: the dependency lines with no git, no `hd add`, or no help text that
// names HD_CACHE; the compilation lines when `hd check` writes no cache,
// and the written-once line when that cache lives in the package.

import { findCompileCache } from "../lib/capability.ts";
import { entryProblem, prepareDependent } from "../lib/deps.ts";
import { copyPackage, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { runHd } from "../lib/hd.ts";
import type { MetricContext } from "../lib/metric.ts";
import { failed, judgeBool, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { listTree, makeTempDir } from "../lib/tmp.ts";

const NAME = "cache-contention";
const N = 8;
const TIMEOUT_MS = 60_000;
const SAFE = "no corruption";
const ONCE = "computed once";

/** Runs `hd check` in every directory at once; the first problem, if any. */
async function checkAll(
  context: MetricContext,
  dirs: readonly string[],
  env: Record<string, string>,
  what: string,
): Promise<string | undefined> {
  const results = await Promise.all(
    dirs.map((cwd) => runHd(context.hd, ["check"], { cwd, env, timeoutMs: TIMEOUT_MS })),
  );
  return results
    .map((result, index) =>
      runProblem(result, TIMEOUT_MS, `${what} ${index + 1} of ${dirs.length}`),
    )
    .find((problem) => problem !== undefined);
}

async function dependencyLines(context: MetricContext): Promise<TargetResult[]> {
  const safeLabel = `dependency cache, ${N} concurrent fetches: ${SAFE}`;
  const onceLabel = `dependency cache, ${N} concurrent fetches: fetched once`;
  const setup = await prepareDependent(context.hd);
  if ("na" in setup)
    return [
      notApplicable(NAME, safeLabel, SAFE, setup.na),
      notApplicable(NAME, onceLabel, ONCE, setup.na),
    ];
  if ("problem" in setup)
    return [
      failed(NAME, safeLabel, SAFE, setup.problem),
      failed(NAME, onceLabel, ONCE, setup.problem),
    ];
  const { remote } = setup;
  let start = remote.connections();
  const single = await checkAll(
    context,
    [copyPackage(setup.dir, NAME)],
    { ...remote.env, HD_CACHE: makeTempDir("cache") },
    "the single check",
  );
  if (single) return [failed(NAME, safeLabel, SAFE, single), failed(NAME, onceLabel, ONCE, single)];
  const one = remote.connections() - start;
  start = remote.connections();
  const cache = makeTempDir("cache");
  const env = { ...remote.env, HD_CACHE: cache };
  const dirs = Array.from({ length: N }, () => copyPackage(setup.dir, NAME));
  context.log(`${NAME}: ${N} concurrent checks that fetch one dependency`);
  const concurrent = await checkAll(context, dirs, env, "concurrent check");
  const total = remote.connections() - start;
  remote.disconnect();
  const problem =
    concurrent ??
    entryProblem(cache, remote) ??
    (await checkAll(context, dirs, env, "the check with the remote gone, in copy"));
  return [
    judgeBool(
      NAME,
      safeLabel,
      problem === undefined,
      SAFE,
      problem ? "corrupt" : "intact",
      problem,
    ),
    judgeBool(
      NAME,
      onceLabel,
      total <= one,
      `≤ ${one}, one check's connections`,
      String(total),
      "connections to the remote",
    ),
  ];
}

async function compileLines(context: MetricContext): Promise<TargetResult[]> {
  const safeLabel = `compilation cache, ${N} concurrent checks: ${SAFE}`;
  const onceLabel = `compilation cache, ${N} concurrent checks: written once`;
  const cache = await findCompileCache(context.hd);
  if (cache === undefined)
    return [
      notApplicable(NAME, safeLabel, SAFE, "hd check writes no compilation cache"),
      notApplicable(NAME, onceLabel, ONCE, "hd check writes no compilation cache"),
    ];
  if (typeof cache === "string")
    return [failed(NAME, safeLabel, SAFE, cache), failed(NAME, onceLabel, ONCE, cache)];
  const { dir: source } = materialize("small", NAME);
  const alone = freshCache();
  const single = await checkAll(context, [copyPackage(source, NAME)], alone, "the single check");
  if (single) return [failed(NAME, safeLabel, SAFE, single), failed(NAME, onceLabel, ONCE, single)];
  const env = freshCache();
  const dirs = Array.from({ length: N }, () => copyPackage(source, NAME));
  context.log(`${NAME}: ${N} concurrent checks with one compilation cache`);
  const problem =
    (await checkAll(context, dirs, env, "concurrent check")) ??
    (await checkAll(context, dirs, env, "the second check in copy"));
  const safe = judgeBool(
    NAME,
    safeLabel,
    problem === undefined,
    SAFE,
    problem ? "corrupt" : "intact",
    problem,
  );
  if (cache.where === "package")
    return [
      safe,
      notApplicable(NAME, onceLabel, ONCE, "the cache lives in each package, so none is shared"),
    ];
  const expected = listTree(alone.HD_CACHE!);
  const found = listTree(env.HD_CACHE!);
  const same = expected.length === found.length && expected.every((path, k) => found[k] === path);
  return [
    safe,
    judgeBool(
      NAME,
      onceLabel,
      same,
      "the files of one check's cache",
      `${found.length} files`,
      `one check alone: ${expected.length} files`,
    ),
  ];
}

export const cacheContention: Metric = {
  name: NAME,
  pillar: 2,
  summary: `${N} processes filling one cache at once: no corruption, each entry computed once`,
  async run(context) {
    return [...(await dependencyLines(context)), ...(await compileLines(context))];
  },
};
