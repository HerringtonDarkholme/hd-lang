// diag-location: does the diagnostic point at the mistake's line?
//
// Target (Pillar 1): in ≥ 95% of the mistake corpus, the diagnostic that
// reports the mistake (the one with the expected code, else the first) is on
// the marked line. A program with no diagnostic counts as a miss.

import { judge, type Metric } from "../lib/metric.ts";
import { mistakeOutcomes } from "../lib/mistake-run.ts";

const NAME = "diag-location";

export const diagLocation: Metric = {
  name: NAME,
  pillar: 1,
  summary: "mistake corpus: share of diagnostics on the mistake's line",
  async run(context) {
    const outcomes = await mistakeOutcomes(context);
    const hits = outcomes.filter((o) => o.reported && o.reported.line === o.mistake.line);
    const misses = outcomes
      .filter((o) => !hits.includes(o))
      .map((o) => `${o.mistake.name}@${o.reported ? String(o.reported.line) : "none"}`);
    return [
      judge(
        NAME,
        "diagnostics on the mistake's line",
        hits.length / outcomes.length,
        0.95,
        "%",
        "at-least",
        `${hits.length} of ${outcomes.length}${misses.length ? `; misses: ${misses.slice(0, 6).join(", ")}` : ""}`,
      ),
    ];
  },
};
