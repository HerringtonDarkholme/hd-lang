// test-latency: an agent edits one function, then runs one test.
//
// A 10k-line generated package gets one warm-up `hd test --filter NAME`.
// Then 5 edits each change a private function body in one module and run
// `hd test --filter NAME --format json` for one test case of that module.
// Each run must pass exactly that one test case. The value is the median.
// Target (Pillar 1): ≤ 300 ms. No n/a case.

import { editBody } from "../lib/gen.ts";
import { editFile, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { jsonLines, runHd } from "../lib/hd.ts";
import { failed, judge, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";

const NAME = "test-latency";
const EDITS = 5;
const WARM_TIMEOUT_MS = 60_000;
const TIMEOUT_MS = 10_000;
const TARGET = "≤ 300 ms";

export const testLatency: Metric = {
  name: NAME,
  pillar: 1,
  summary: "edit, then hd test --filter for one test case in a 10k-line package; median of 5",
  async run(context) {
    const { dir, pkg } = materialize("10k", NAME);
    const env = freshCache();
    const label = "median edit-then-test";
    const first = pkg.modules[0]!;
    context.log(`${NAME}: ${pkg.lines} lines; warm-up test`);
    const warm = await runHd(context.hd, ["test", "--filter", first.testName], {
      cwd: dir,
      env,
      timeoutMs: WARM_TIMEOUT_MS,
    });
    const warmProblem = runProblem(warm, WARM_TIMEOUT_MS, "the warm-up test");
    if (warmProblem) return [failed(NAME, label, TARGET, warmProblem)];
    const samples: number[] = [];
    for (let index = 0; index < EDITS; index++) {
      const module = pkg.modules[Math.floor(((index + 0.5) * pkg.modules.length) / EDITS)]!;
      editFile(dir, module.file, editBody);
      const result = await runHd(
        context.hd,
        ["test", "--filter", module.testName, "--format", "json"],
        { cwd: dir, env, timeoutMs: TIMEOUT_MS },
      );
      const problem = runProblem(result, TIMEOUT_MS, `test after edit ${index + 1}`);
      if (problem) return [failed(NAME, label, TARGET, problem)];
      const tests = jsonLines(result.stdout).filter((record) => record.kind === "test");
      if (tests.length !== 1 || tests[0]!.outcome !== "passed")
        return [
          failed(
            NAME,
            label,
            TARGET,
            `--filter ${module.testName} ran ${tests.length} test cases, not one passing`,
          ),
        ];
      samples.push(result.wallMs);
    }
    return [judge(NAME, label, p50(samples), 300, "ms")];
  },
};
