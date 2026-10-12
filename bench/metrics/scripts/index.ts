// Every metric script, in the order of the Goal Metrics tables
// (future-work/compiler/goals.md): pillars 1, 2 and 3, then the
// correctness gate.

import type { Metric } from "../lib/metric.ts";
import { allocations } from "./allocations.ts";
import { answerSize } from "./answer-size.ts";
import { cacheContention } from "./cache-contention.ts";
import { cacheGrowth } from "./cache-growth.ts";
import { checkCost } from "./check-cost.ts";
import { coldCheck } from "./cold-check.ts";
import { concurrency } from "./concurrency.ts";
import { conformance } from "./conformance.ts";
import { deadCode } from "./dead-code.ts";
import { determinism } from "./determinism.ts";
import { devSpeed } from "./dev-speed.ts";
import { diagLocation } from "./diag-location.ts";
import { disk } from "./disk.ts";
import { editLatency } from "./edit-latency.ts";
import { errorsPerRun } from "./errors-per-run.ts";
import { fetchDedup } from "./fetch-dedup.ts";
import { fixitSafety } from "./fixit-safety.ts";
import { fmt } from "./fmt.ts";
import { hostCallOverhead } from "./host-call-overhead.ts";
import { integrationTestPerf } from "./integration-test-perf.ts";
import { incrementalSoundness } from "./incremental-soundness.ts";
import { ioPerCheck } from "./io-per-check.ts";
import { longRunMemory } from "./long-run-memory.ts";
import { longSession } from "./long-session.ts";
import { lookupLatency } from "./lookup-latency.ts";
import { mistakes } from "./mistakes.ts";
import { parallelSpeedup } from "./parallel-speedup.ts";
import { pathological } from "./pathological.ts";
import { proptestPerf } from "./proptest-perf.ts";
import { recheckPrecision } from "./recheck-precision.ts";
import { resources } from "./resources.ts";
import { runtime } from "./runtime.ts";
import { serdeThroughput } from "./serde-throughput.ts";
import { sizeStartupHeap } from "./size-startup-heap.ts";
import { startup } from "./startup.ts";
import { suiteCpu } from "./suite-cpu.ts";
import { suspensionOverhead } from "./suspension-overhead.ts";
import { testLatency } from "./test-latency.ts";
import { textThroughput } from "./text-throughput.ts";
import { unitTestPerf } from "./unit-test-perf.ts";

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
  checkCost,
  devSpeed,
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
  proptestPerf,
  unitTestPerf,
  integrationTestPerf,
  runtime,
  allocations,
  sizeStartupHeap,
  hostCallOverhead,
  suspensionOverhead,
  serdeThroughput,
  textThroughput,
  deadCode,
  longRunMemory,
  conformance,
  incrementalSoundness,
];
