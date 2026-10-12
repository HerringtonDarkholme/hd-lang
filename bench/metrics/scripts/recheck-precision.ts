// recheck-precision: after an edit, recheck only what the edit can affect.
//
// On a small generated package (~1k lines), after a warm `hd check`:
// - a private function body edit in one module must recheck 1 module;
// - a public signature edit (a new defaulted parameter, so callers still
//   type-check) must recheck that module and its direct dependents only.
// The count comes from the `modules_checked` field of the `--format json`
// summary object: the modules the run type-checked rather than reused. The
// CLI specification has no such field yet; this name is the harness's
// convention (README.md).
// n/a: both lines, when the summary has no numeric `modules_checked`.

import { dependentsOf, editBody, editSignature } from "../lib/gen.ts";
import { editFile, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { runHd, summaryOf } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric } from "../lib/metric.ts";

const NAME = "recheck-precision";
const FIELD = "modules_checked";
const TIMEOUT_MS = 60_000;

export const recheckPrecision: Metric = {
  name: NAME,
  pillar: 1,
  summary: "modules rechecked after a private body edit, and after a public signature edit",
  async run(context) {
    const { dir, pkg } = materialize("small", NAME);
    const env = freshCache();
    const check = async (what: string) => {
      const result = await runHd(context.hd, ["check", "--format", "json"], {
        cwd: dir,
        env,
        timeoutMs: TIMEOUT_MS,
      });
      const problem = runProblem(result, TIMEOUT_MS, what);
      const count = summaryOf(result.stdout)?.[FIELD];
      return { problem, count: typeof count === "number" ? count : undefined };
    };
    const bodyLabel = "modules rechecked, private body edit";
    const signatureLabel = "modules rechecked, public signature edit";
    const warm = await check("the warm-up check");
    if (warm.problem)
      return [
        failed(NAME, bodyLabel, "≤ 1", warm.problem),
        failed(NAME, signatureLabel, "≤ 1 + dependents", warm.problem),
      ];
    if (warm.count === undefined)
      return [
        notApplicable(NAME, bodyLabel, "≤ 1", `no ${FIELD} in the JSON summary`),
        notApplicable(NAME, signatureLabel, "≤ 1 + dependents", `no ${FIELD} in the JSON summary`),
      ];
    const target = pkg.modules[Math.floor(pkg.modules.length / 2)]!;
    editFile(dir, target.file, editBody);
    const body = await check("the check after a body edit");
    editFile(dir, target.file, editSignature);
    const signature = await check("the check after a signature edit");
    const allowed = 1 + dependentsOf(pkg, target.name).length;
    return [
      body.count === undefined
        ? failed(NAME, bodyLabel, "≤ 1", body.problem ?? `no ${FIELD}`)
        : judge(NAME, bodyLabel, body.count, 1, "count"),
      signature.count === undefined
        ? failed(NAME, signatureLabel, `≤ ${allowed}`, signature.problem ?? `no ${FIELD}`)
        : judge(
            NAME,
            signatureLabel,
            signature.count,
            allowed,
            "count",
            "at-most",
            `${target.name} and its dependents`,
          ),
    ];
  },
};
