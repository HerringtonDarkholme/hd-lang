// Oracle: later phases never accept what an earlier phase rejects.
//   parse rejects => check rejects;  check rejects => run and test reject.
import type { Action, Outcome } from "../common.ts";
import { type Observation, type OracleInput, who } from "./types.ts";

export const phaseActions: readonly Action[] = ["parse", "check", "run", "test"];

function rejects(outcome: Outcome | undefined): boolean {
  return outcome?.kind === "reject";
}

function succeeds(outcome: Outcome | undefined): boolean {
  return outcome?.kind === "accept" || outcome?.kind === "panic";
}

export function phaseConsistency(input: OracleInput): Observation[] {
  const observations: Observation[] = [];
  for (const [index, execution] of input.executions.entries()) {
    const tag = who(index, input.executions.length);
    const { parse, check, run, test } = execution;
    if (rejects(parse) && check?.kind === "accept")
      observations.push({
        detail: parse!.detail,
        signature: `phase|${tag}parse-reject/check-accept|${parse!.code}`,
      });
    for (const [name, later] of [
      ["run", run],
      ["test", test],
    ] as const) {
      if (rejects(check) && succeeds(later))
        observations.push({
          detail: `${check!.detail} -> ${later!.kind}:${later!.code}`,
          signature: `phase|${tag}check-reject/${name}-${later!.kind}|${check!.code}`,
        });
    }
  }
  return observations;
}
