// Every metric script, in the order of the Goal Metrics tables
// (future-work/NEW_COMPILER_ARCHITECTURE.md). Pillar 3 and the correctness
// gate add their scripts here.

import type { Metric } from "../lib/metric.ts";
import { answerSize } from "./answer-size.ts";
import { cacheContention } from "./cache-contention.ts";
import { cacheGrowth } from "./cache-growth.ts";
import { coldCheck } from "./cold-check.ts";
import { concurrency } from "./concurrency.ts";
import { determinism } from "./determinism.ts";
import { diagLocation } from "./diag-location.ts";
import { disk } from "./disk.ts";
import { editLatency } from "./edit-latency.ts";
import { errorsPerRun } from "./errors-per-run.ts";
import { fetchDedup } from "./fetch-dedup.ts";
import { fixitSafety } from "./fixit-safety.ts";
import { fmt } from "./fmt.ts";
import { ioPerCheck } from "./io-per-check.ts";
import { longSession } from "./long-session.ts";
import { lookupLatency } from "./lookup-latency.ts";
import { mistakes } from "./mistakes.ts";
import { parallelSpeedup } from "./parallel-speedup.ts";
import { pathological } from "./pathological.ts";
import { recheckPrecision } from "./recheck-precision.ts";
import { releaseCheckCost } from "./release-check-cost.ts";
import { resources } from "./resources.ts";
import { startup } from "./startup.ts";
import { suiteCpu } from "./suite-cpu.ts";
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
  resources,
  longSession,
  startup,
  concurrency,
  disk,
  suiteCpu,
  parallelSpeedup,
  cacheContention,
  cacheGrowth,
  ioPerCheck,
  fetchDedup,
];
