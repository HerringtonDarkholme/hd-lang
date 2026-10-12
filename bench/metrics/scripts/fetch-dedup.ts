// fetch-dedup: a dependency used by many worktrees is fetched once.
//
// A local remote (lib/deps.ts) serves one tagged dependency through git,
// and counts the connections made to it; there is no network. `hd add`
// writes a dependent package's hd.toml and hd.sum, using a cache of its
// own. Then 4 copies of that package, one per worktree, each run
// `hd check`, one after another, with one shared, empty HD_CACHE; a check
// fetches a version the cache lacks (cli.cache.shared).
// The value is the connections all 4 checks made, against the connections
// of the first check alone.
// Target (Pillar 2): fetched once, so the later checks connect no more.
// n/a: no git on PATH, no `hd add`, or no help text that names HD_CACHE.

import { prepareDependent } from "../lib/deps.ts";
import { copyPackage, runProblem } from "../lib/fixture.ts";
import { runHd } from "../lib/hd.ts";
import { failed, judgeBool, notApplicable, type Metric } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "fetch-dedup";
const WORKTREES = 4;
const TIMEOUT_MS = 60_000;
const LABEL = `connections to the remote, ${WORKTREES} worktrees`;
const TARGET = "= the first worktree's";

export const fetchDedup: Metric = {
  name: NAME,
  pillar: 2,
  summary: `a dependency used by ${WORKTREES} worktrees with one shared cache is fetched once`,
  async run(context) {
    const setup = await prepareDependent(context.hd);
    if ("na" in setup) return [notApplicable(NAME, LABEL, TARGET, setup.na)];
    if ("problem" in setup) return [failed(NAME, LABEL, TARGET, setup.problem)];
    const { remote } = setup;
    const env = { ...remote.env, HD_CACHE: makeTempDir("cache") };
    const start = remote.connections();
    let first = 0;
    for (let index = 0; index < WORKTREES; index++) {
      const result = await runHd(context.hd, ["check"], {
        cwd: copyPackage(setup.dir, NAME),
        env,
        timeoutMs: TIMEOUT_MS,
      });
      const problem = runProblem(result, TIMEOUT_MS, `hd check in worktree ${index + 1}`);
      if (problem) return [failed(NAME, LABEL, TARGET, problem)];
      if (index === 0) first = remote.connections() - start;
    }
    const total = remote.connections() - start;
    if (first === 0)
      return [
        failed(NAME, LABEL, TARGET, "the first check made no connection, so it fetched nothing"),
      ];
    return [
      judgeBool(
        NAME,
        LABEL,
        total === first,
        TARGET,
        String(total),
        `the first worktree's: ${first}`,
      ),
    ];
  },
};
