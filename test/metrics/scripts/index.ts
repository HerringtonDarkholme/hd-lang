// Every metric script, in the order of the Goal Metrics tables
// (future-work/NEW_COMPILER_ARCHITECTURE.md). Pillars 2 and 3 and the
// correctness gate add their scripts here.

import type { Metric } from "../lib/metric.ts";
import { answerSize } from "./answer-size.ts";
import { coldCheck } from "./cold-check.ts";
import { determinism } from "./determinism.ts";
import { diagLocation } from "./diag-location.ts";
import { editLatency } from "./edit-latency.ts";
import { errorsPerRun } from "./errors-per-run.ts";
import { fixitSafety } from "./fixit-safety.ts";
import { fmt } from "./fmt.ts";
import { lookupLatency } from "./lookup-latency.ts";
import { mistakes } from "./mistakes.ts";
import { pathological } from "./pathological.ts";
import { recheckPrecision } from "./recheck-precision.ts";
import { releaseCheckCost } from "./release-check-cost.ts";
import { testLatency } from "./test-latency.ts";

export const METRICS: readonly Metric[] = [
  editLatency,
  coldCheck,
  testLatency,
  mistakes,
  answerSize,
  determinism,
  pathological,
  recheckPrecision,
  errorsPerRun,
  diagLocation,
  fixitSafety,
  lookupLatency,
  fmt,
  releaseCheckCost,
];
