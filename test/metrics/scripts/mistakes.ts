// mistakes: how well one `hd check` answers one mistake.
//
// Targets (Pillar 1): exactly one diagnostic in ≥ 95% of the corpus; the
// fix-it resolves ≥ 80%; each diagnostic ≤ 60 tokens. The same 95% bar is
// applied to the share whose one diagnostic has the expected code.
// n/a: the fix-it line, when no diagnostic object has a `fix` or `fixes`
// field (see lib/fixit.ts).

import { judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { mistakeOutcomes } from "../lib/mistake-run.ts";
import { estimateTokens, p50, share } from "../lib/stats.ts";

const NAME = "mistakes";
const TOKEN_LIMIT = 60;

export const mistakes: Metric = {
  name: NAME,
  pillar: 1,
  summary: "single-mistake corpus: one diagnostic each, fix-it resolution, diagnostic size",
  async run(context) {
    const outcomes = await mistakeOutcomes(context);
    const results: TargetResult[] = [];
    const broken = outcomes.filter((o) => o.problem).map((o) => o.mistake.name);
    const brokenNote = broken.length ? `runs failed: ${broken.join(", ")}` : undefined;

    const single = outcomes.filter((o) => o.diagnostics.length === 1);
    results.push(
      judge(
        NAME,
        "programs with exactly one diagnostic",
        share(outcomes, (o) => o.diagnostics.length === 1),
        0.95,
        "%",
        "at-least",
        `${single.length} of ${outcomes.length}${brokenNote ? `; ${brokenNote}` : ""}`,
      ),
    );
    const coded = outcomes.filter(
      (o) => o.diagnostics.length === 1 && o.diagnostics[0]!.code === o.mistake.code,
    );
    const wrong = outcomes
      .filter((o) => !coded.includes(o))
      .map((o) => o.mistake.name)
      .slice(0, 8);
    results.push(
      judge(
        NAME,
        "one diagnostic with the expected code",
        coded.length / outcomes.length,
        0.95,
        "%",
        "at-least",
        `${coded.length} of ${outcomes.length}${wrong.length ? `; first misses: ${wrong.join(", ")}` : ""}`,
      ),
    );

    if (!outcomes.some((o) => o.fixFieldSeen))
      results.push(
        notApplicable(
          NAME,
          "fix-it makes hd check pass",
          "≥ 80.0%",
          "no fix-it field in --format json",
        ),
      );
    else {
      const resolved = outcomes.filter((o) => o.afterFix && o.afterFix.length === 0);
      const offered = outcomes.filter((o) => o.fixApplied).length;
      results.push(
        judge(
          NAME,
          "fix-it makes hd check pass",
          resolved.length / outcomes.length,
          0.8,
          "%",
          "at-least",
          `${resolved.length} of ${outcomes.length} resolved; ${offered} offered a fix-it`,
        ),
      );
    }

    const answered = outcomes.filter((o) => o.text.trim() !== "");
    const tokens = answered.map((o) => estimateTokens(o.text));
    const bytes = answered.map((o) => Buffer.byteLength(o.text, "utf8"));
    const largest = answered.reduce<{ name: string; tokens: number } | undefined>((best, o) => {
      const count = estimateTokens(o.text);
      return !best || count > best.tokens ? { name: o.mistake.name, tokens: count } : best;
    }, undefined);
    results.push(
      judge(
        NAME,
        "largest diagnostic text (bytes / 4)",
        tokens.length ? Math.max(...tokens) : Number.NaN,
        TOKEN_LIMIT,
        "tokens",
        "at-most",
        tokens.length
          ? `largest: ${largest!.name}; median ${p50(tokens)} tokens, ${p50(bytes)} B; ${
              tokens.filter((t) => t <= TOKEN_LIMIT).length
            } of ${tokens.length} within`
          : "no program printed a diagnostic",
      ),
    );
    return results;
  },
};
