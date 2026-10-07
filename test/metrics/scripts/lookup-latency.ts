// lookup-latency: how fast an agent gets an answer from the program database.
//
// On a 10k-line generated package, 10 timed runs each of two canned
// queries, after one untimed warm-up run of each: `hd callers
// m040.label40` (who calls a function) and `hd needs Http` (which code
// needs a capability; the package has none, which is an answer too).
// Target (Pillar 1): p95 ≤ 100 ms for each query.
// n/a: a query whose `hd help COMMAND` fails. Neither command is in the CLI
// specification yet; the names come from NEW_COMPILER_ARCHITECTURE.md.

import { materialize, runProblem } from "../lib/fixture.ts";
import { runHd, supportsCommand } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { p95 } from "../lib/stats.ts";

const NAME = "lookup-latency";
const RUNS = 10;
const TIMEOUT_MS = 10_000;
const WARM_TIMEOUT_MS = 60_000;

export const lookupLatency: Metric = {
  name: NAME,
  pillar: 1,
  summary:
    "canned program-database queries (hd callers, hd needs Http) wall time, p95 of 10 runs each",
  async run(context) {
    const { dir, pkg } = materialize("10k", NAME);
    const module = pkg.modules[Math.min(40, pkg.modules.length - 1)]!;
    const k = Number(module.name.slice(1));
    const lookups = [
      { command: "callers", args: ["callers", `${module.name}.label${k}`] },
      { command: "needs", args: ["needs", "Http"] },
    ];
    const results: TargetResult[] = [];
    for (const lookup of lookups) {
      const label = `p95 hd ${lookup.command}`;
      if (!(await supportsCommand(context.hd, lookup.command, dir))) {
        results.push(notApplicable(NAME, label, "≤ 100 ms", `no hd ${lookup.command}`));
        continue;
      }
      const warm = await runHd(context.hd, lookup.args, { cwd: dir, timeoutMs: WARM_TIMEOUT_MS });
      const warmProblem = runProblem(warm, WARM_TIMEOUT_MS, `warm-up hd ${lookup.command}`);
      if (warmProblem) {
        results.push(failed(NAME, label, "≤ 100 ms", warmProblem));
        continue;
      }
      const samples: number[] = [];
      let problem: string | undefined;
      for (let index = 0; index < RUNS && !problem; index++) {
        const result = await runHd(context.hd, lookup.args, { cwd: dir, timeoutMs: TIMEOUT_MS });
        problem = runProblem(result, TIMEOUT_MS, `hd ${lookup.command} ${index + 1}`);
        samples.push(result.wallMs);
      }
      results.push(
        problem
          ? failed(NAME, label, "≤ 100 ms", problem)
          : judge(NAME, label, p95(samples), 100, "ms"),
      );
    }
    return results;
  },
};
