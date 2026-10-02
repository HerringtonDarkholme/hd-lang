// Oracle: every command result obeys spec/tools/fuzz/CONTRACT.md.
import type { Action } from "../common.ts";
import { type Observation, type OracleInput, who } from "./types.ts";

export const contractActions: readonly Action[] = ["parse", "check", "run", "test"];

/** `label` names the fuzzer in each signature: `contract`, or `parse` for parse-only inputs. */
export function contract(input: OracleInput, label = "contract"): Observation[] {
  const observations: Observation[] = [];
  for (const [index, execution] of input.executions.entries()) {
    const tag = who(index, input.executions.length);
    for (const action of contractActions) {
      const outcome = execution[action];
      if (!outcome) continue;
      if (outcome.kind === "violation") {
        const code =
          outcome.code === "no-located-code" || outcome.code === "bad-exit"
            ? `${outcome.code}:${outcome.detail}`
            : outcome.code;
        observations.push({
          detail: outcome.detail,
          signature: `${label}|${tag}violation:${action}|${code}`,
        });
      } else if (outcome.unknownCode)
        // All uninventoried codes share one signature per command; the codes
        // themselves are tallied separately (see SUMMARY "uninventoried codes").
        observations.push({
          detail: outcome.unknownCode,
          signature: `${label}|${tag}uninventoried-code:${action}|*`,
        });
    }
  }
  return observations;
}
