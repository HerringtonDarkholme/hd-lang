// cold-check: `hd check` of a 10k-line package with nothing cached.
//
// Each of three runs checks a fresh copy of the generated package with a
// fresh, empty HD_CACHE, so no earlier run can help. The value is the median.
// Target (Pillar 1): ≤ 1 s. No n/a case.

import { copyTree, makeTempDir } from "../lib/tmp.ts";
import { freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { runHd } from "../lib/hd.ts";
import { failed, judge, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";

const NAME = "cold-check";
const RUNS = 3;
const TIMEOUT_MS = 30_000;

export const coldCheck: Metric = {
  name: NAME,
  pillar: 1,
  summary: "cold hd check of a 10k-line package, median of 3 fresh copies",
  async run(context) {
    const { dir: source, pkg } = materialize("10k", NAME);
    context.log(`${NAME}: ${pkg.lines} lines, ${RUNS} cold checks`);
    const samples: number[] = [];
    for (let index = 0; index < RUNS; index++) {
      const dir = makeTempDir("cold");
      copyTree(source, dir);
      const result = await runHd(context.hd, ["check"], {
        cwd: dir,
        env: freshCache(),
        timeoutMs: TIMEOUT_MS,
      });
      const problem = runProblem(result, TIMEOUT_MS, `cold check ${index + 1}`);
      if (problem) return [failed(NAME, "median cold check, 10k lines", "≤ 1000 ms", problem)];
      samples.push(result.wallMs);
    }
    return [judge(NAME, "median cold check, 10k lines", p50(samples), 1000, "ms")];
  },
};
