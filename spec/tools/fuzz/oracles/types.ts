import type { Action, Outcome } from "../common.ts";

/** Outcomes of one implementation on one input, keyed by command. */
export type Execution = Partial<Record<Action, Outcome>>;

export interface OracleInput {
  /** One entry per `--compiler`, in command-line order. */
  readonly executions: readonly Execution[];
  /** Codes from the spec reference parser (`parseSource`); empty means accept. */
  readonly reference: readonly string[];
}

export interface Observation {
  /** fuzzer | outcome class | first code. Stable across runs and implementations. */
  readonly signature: string;
  readonly detail: string;
}

/** Prefix used when more than one implementation is under test. */
export function who(index: number, total: number): string {
  return total > 1 ? `c${index}:` : "";
}
