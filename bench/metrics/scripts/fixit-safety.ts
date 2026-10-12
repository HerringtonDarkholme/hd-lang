// fixit-safety: applying a fix-it never adds a new error.
//
// Target (Pillar 1): 100% of the fix-its offered on the mistake corpus leave
// no error code that the program did not already have. Line numbers may
// shift after an edit, so errors are compared by code, counting repeats.
// n/a: no diagnostic object has a `fix` or `fixes` field, or none of the
// corpus's diagnostics offers a fix-it to apply.

import { judge, notApplicable, type Metric } from "../lib/metric.ts";
import { errorsOf, mistakeOutcomes } from "../lib/mistake-run.ts";

const NAME = "fixit-safety";

const counts = (diagnostics: readonly Record<string, unknown>[]): Map<string, number> => {
  const result = new Map<string, number>();
  for (const d of errorsOf(diagnostics)) {
    const code = String(d.code);
    result.set(code, (result.get(code) ?? 0) + 1);
  }
  return result;
};

/** Error codes after the fix that outnumber the same codes before it. */
export function newErrors(
  before: readonly Record<string, unknown>[],
  after: readonly Record<string, unknown>[],
): string[] {
  const old = counts(before);
  return [...counts(after)].filter(([code, n]) => n > (old.get(code) ?? 0)).map(([code]) => code);
}

export const fixitSafety: Metric = {
  name: NAME,
  pillar: 1,
  summary: "mistake corpus: applied fix-its add no new error",
  async run(context) {
    const outcomes = await mistakeOutcomes(context);
    const target = "≥ 100.0%";
    if (!outcomes.some((o) => o.fixFieldSeen))
      return [
        notApplicable(
          NAME,
          "fix-its that add no error",
          target,
          "no fix-it field in --format json",
        ),
      ];
    const applied = outcomes.filter((o) => o.fixApplied && o.afterFix);
    if (applied.length === 0)
      return [
        notApplicable(
          NAME,
          "fix-its that add no error",
          target,
          "no corpus diagnostic offers a fix-it",
        ),
      ];
    const unsafe = applied
      .map((o) => ({ name: o.mistake.name, added: newErrors(o.diagnostics, o.afterFix!) }))
      .filter((entry) => entry.added.length > 0);
    return [
      judge(
        NAME,
        "fix-its that add no error",
        (applied.length - unsafe.length) / applied.length,
        1,
        "%",
        "at-least",
        `${applied.length - unsafe.length} of ${applied.length} safe${
          unsafe.length
            ? `; unsafe: ${unsafe
                .slice(0, 5)
                .map((entry) => `${entry.name} (+${entry.added.join(" +")})`)
                .join(", ")}`
            : ""
        }`,
      ),
    ];
  },
};
