// Oracle: two or more implementations agree on the outcome class of every
// command: accept, reject by first code, panic by code, or violation kind.
import { type Action, outcomeLabel } from "../common.ts";
import type { Observation, OracleInput } from "./types.ts";

export const crossActions: readonly Action[] = ["parse", "check", "run", "test"];

export function crossImpl(input: OracleInput): Observation[] {
  const observations: Observation[] = [];
  const [first, ...others] = input.executions;
  if (!first) return observations;
  for (const action of crossActions) {
    const baseline = first[action];
    if (!baseline) continue;
    for (const [offset, execution] of others.entries()) {
      const other = execution[action];
      if (!other) continue;
      const left = outcomeLabel(baseline);
      const right = outcomeLabel(other);
      if (left !== right)
        observations.push({
          detail: `${baseline.detail} | ${other.detail}`,
          signature: `cross-impl|${action}:c0/c${offset + 1}|${left} vs ${right}`,
        });
    }
  }
  return observations;
}
