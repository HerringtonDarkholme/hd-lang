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
    const broken = outcomes.filter((item) => item.problem).map((item) => item.mistake.name);
    const brokenNote = broken.length ? `runs failed: ${broken.join(", ")}` : undefined;

    const single = outcomes.filter((item) => item.diagnostics.length === 1);
    results.push(
      judge(
        NAME,
        "programs with exactly one diagnostic",
        share(outcomes, (options) => options.diagnostics.length === 1),
        0.95,
        "%",
        "at-least",
        `${single.length} of ${outcomes.length}${brokenNote ? `; ${brokenNote}` : ""}`,
      ),
    );
    const coded = outcomes.filter(
      (item) => item.diagnostics.length === 1 && item.diagnostics[0]!.code === item.mistake.code,
    );
    const wrong = outcomes
      .filter((item) => !coded.includes(item))
      .map((item) => item.mistake.name)
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

    if (!outcomes.some((item) => item.fixFieldSeen)) {
      results.push(
        notApplicable(
          NAME,
          "fix-it makes hd check pass",
          "≥ 80.0%",
          "no fix-it field in --format json",
        ),
      );
    } else {
      const resolved = outcomes.filter((item) => item.afterFix && item.afterFix.length === 0);
      const offered = outcomes.filter((item) => item.fixApplied).length;
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

    const answered = outcomes.filter((item) => item.text.trim() !== "");
    const tokens = answered.map((item) => estimateTokens(item.text));
    const bytes = answered.map((item) => Buffer.byteLength(item.text, "utf8"));
    const largest = answered.reduce<{ name: string; tokens: number } | undefined>((best, value) => {
      const count = estimateTokens(value.text);
      return !best || count > best.tokens ? { name: value.mistake.name, tokens: count } : best;
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
              tokens.filter((item) => item <= TOKEN_LIMIT).length
            } of ${tokens.length} within`
          : "no program printed a diagnostic",
      ),
    );
    return results;
  },
};
