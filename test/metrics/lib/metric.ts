// The contract between the runner and a metric script.
//
// A metric script exports one `Metric`. It measures, never writes to the
// repository, and returns one result per target in its row of the Goal
// Metrics tables (future-work/NEW_COMPILER_ARCHITECTURE.md). A target the
// `hd` under test cannot be measured on, such as `hd fmt` on an `hd` with
// no formatter, is `n/a` with a reason: it never fails the run and never
// gets a made-up value.

import type { HdCommand } from "./hd.ts";

/** The Arena pillar of a metric; `gate` is the correctness prerequisite. */
export type Pillar = 1 | 2 | 3 | "gate";

export type Status = "pass" | "fail" | "n/a";

export interface TargetResult {
  /** The metric script, as `edit-latency`. */
  readonly metric: string;
  /** What this line measures, as `p95 after an edit`. */
  readonly name: string;
  /** The measured value, formatted with its unit; `-` for n/a. */
  readonly value: string;
  /** The target, as `≤ 200 ms`. */
  readonly target: string;
  readonly status: Status;
  /** Why a target is n/a or failed, or extra detail. */
  readonly note?: string;
}

export interface MetricContext {
  readonly hd: HdCommand;
  /** The repository root, for reading corpus files. Never written. */
  readonly repoRoot: string;
  /** Prints a progress line to stderr. */
  readonly log: (line: string) => void;
}

export interface Metric {
  readonly name: string;
  readonly pillar: Pillar;
  /** One line: what the script measures. */
  readonly summary: string;
  readonly run: (context: MetricContext) => Promise<TargetResult[]>;
}

export type Unit = "ms" | "%" | "bytes" | "MB" | "tokens" | "count" | "x" | "exponent";

export function formatValue(value: number, unit: Unit): string {
  if (!Number.isFinite(value)) return String(value);
  switch (unit) {
    case "ms":
      return `${value < 10 ? value.toFixed(1) : Math.round(value)} ms`;
    case "%":
      return `${(value * 100).toFixed(1)}%`;
    case "bytes":
      return `${Math.round(value)} B`;
    case "MB":
      return `${(value / (1024 * 1024)).toFixed(1)} MB`;
    case "tokens":
      return `${Math.round(value)} tokens`;
    case "count":
      return String(Math.round(value));
    case "x":
      return `${value.toFixed(2)}x`;
    case "exponent":
      return value.toFixed(2);
  }
}

/**
 * Judges a measured value against a limit. `at-most` passes when the value
 * is at or below the limit; `at-least` when it is at or above it. For `MB`
 * and `%`, both the value and the limit use the base unit: bytes, and a
 * share in 0..1.
 */
export function judge(
  metric: string,
  name: string,
  value: number,
  limit: number,
  unit: Unit,
  direction: "at-most" | "at-least" = "at-most",
  note?: string,
): TargetResult {
  const ok = Number.isFinite(value) && (direction === "at-most" ? value <= limit : value >= limit);
  return {
    metric,
    name,
    value: formatValue(value, unit),
    target: `${direction === "at-most" ? "≤" : "≥"} ${formatValue(limit, unit)}`,
    status: ok ? "pass" : "fail",
    ...(note === undefined ? {} : { note }),
  };
}

/** A target that holds or not, with no number, as "byte-identical". */
export function judgeBool(
  metric: string,
  name: string,
  ok: boolean,
  target: string,
  value: string,
  note?: string,
): TargetResult {
  return {
    metric,
    name,
    value,
    target,
    status: ok ? "pass" : "fail",
    ...(note === undefined ? {} : { note }),
  };
}

/** A target this `hd` cannot be measured on. */
export const notApplicable = (
  metric: string,
  name: string,
  target: string,
  reason: string,
): TargetResult => ({
  metric,
  name,
  value: "-",
  target,
  status: "n/a",
  note: `not supported by this hd: ${reason}`,
});

/** A target that failed to produce a value, as when every run timed out. */
export const failed = (
  metric: string,
  name: string,
  target: string,
  reason: string,
): TargetResult => ({ metric, name, value: "-", target, status: "fail", note: reason });
