// edit-latency: an agent edits one function, then runs `hd check`.
//
// A 10k-line generated package gets one warm-up `hd check`. Then 20 edits,
// spread over the modules, each change a private function body (`let bump`
// in `tweakK`) and are each followed by a timed `hd check` of the package.
// Targets (Pillar 1): p50 ≤ 50 ms, p95 ≤ 200 ms.
// No n/a case: every hd has `hd check`.

import { editBody } from "../lib/gen.ts";
import { editFile, freshCache, materialize, runProblem, timedSeries } from "../lib/fixture.ts";
import { runHd } from "../lib/hd.ts";
import { failed, judge, type Metric } from "../lib/metric.ts";
import { p50, p95 } from "../lib/stats.ts";

const NAME = "edit-latency";
const EDITS = 20;
const WARM_TIMEOUT_MS = 60_000;
const EDIT_TIMEOUT_MS = 10_000;

export const editLatency: Metric = {
  name: NAME,
  pillar: 1,
  summary: "one-function edit in a 10k-line package, then hd check; p50/p95 over 20 edits",
  async run(context) {
    const { dir, pkg } = materialize("10k", NAME);
    const env = freshCache();
    context.log(`${NAME}: ${pkg.lines} lines, ${pkg.modules.length} modules; warm-up check`);
    const warm = await runHd(context.hd, ["check"], { cwd: dir, env, timeoutMs: WARM_TIMEOUT_MS });
    const warmProblem = runProblem(warm, WARM_TIMEOUT_MS, "the warm-up check");
    if (warmProblem)
      return [
        failed(NAME, "p50 check after an edit", "≤ 50 ms", warmProblem),
        failed(NAME, "p95 check after an edit", "≤ 200 ms", warmProblem),
      ];
    const series = await timedSeries(
      context.hd,
      EDITS,
      () => ["check"],
      { cwd: dir, env, timeoutMs: EDIT_TIMEOUT_MS },
      "check after edit",
      (index) => {
        const module = pkg.modules[Math.floor((index * pkg.modules.length) / EDITS)]!;
        editFile(dir, module.file, editBody);
      },
    );
    if (series.samples.length < EDITS)
      return [
        failed(NAME, "p50 check after an edit", "≤ 50 ms", series.problem!),
        failed(NAME, "p95 check after an edit", "≤ 200 ms", series.problem!),
      ];
    return [
      judge(NAME, "p50 check after an edit", p50(series.samples), 50, "ms"),
      judge(NAME, "p95 check after an edit", p95(series.samples), 200, "ms"),
    ];
  },
};
