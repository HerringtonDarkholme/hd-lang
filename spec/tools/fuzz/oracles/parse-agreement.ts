// Oracle: the reference parser and each implementation agree on accept/reject
// for `parse`, and each `parse` result obeys the command contract.
import type { Action } from "../common.ts";
import { type Observation, type OracleInput, who } from "./types.ts";

export const parseAgreementActions: readonly Action[] = ["parse"];

export function parseAgreement(input: OracleInput): Observation[] {
  const observations: Observation[] = [];
  const referenceAccepts = input.reference.length === 0;
  for (const [index, execution] of input.executions.entries()) {
    const outcome = execution.parse;
    if (!outcome) continue;
    const tag = who(index, input.executions.length);
    if (outcome.kind === "violation") {
      const code =
        outcome.code === "no-located-code" || outcome.code === "bad-exit"
          ? `${outcome.code}:${outcome.detail}`
          : outcome.code;
      observations.push({
        detail: outcome.detail,
        signature: `parse-agreement|${tag}violation:parse|${code}`,
      });
      continue;
    }
    const implementationAccepts = outcome.kind === "accept";
    if (referenceAccepts && !implementationAccepts)
      observations.push({
        detail: outcome.detail,
        signature: `parse-agreement|${tag}ref-accept/impl-reject|${outcome.code}`,
      });
    else if (!referenceAccepts && implementationAccepts)
      observations.push({
        detail: input.reference.join(","),
        signature: `parse-agreement|${tag}ref-reject/impl-accept|${input.reference[0]}`,
      });
  }
  return observations;
}
