// resources: CPU time and peak RSS of check, test and build.
//
// For each generated package (small, 10k and 50k lines, each with the
// executable src/main.hd so `hd build` writes a program) and each of
// `hd check`, `hd test` and `hd build`: a cold run in a fresh copy with an
// empty HD_CACHE, then a warm run of the same command with nothing changed.
// Each run is one line: its CPU time and peak RSS, and whether it completed
// within the size's timeout. A run that times out or fails is a failing
// line with the reason.
// Skips, each a failing line with the reason: the warm run after a failed
// cold run; every command at a size where `hd check` timed out (test and
// build type-check the package too); a command at a size above one where
// that command timed out.
// Targets (Pillar 2): warm check of 10k lines ≤ 0.2 CPU-s and ≤ 50 MB.
// n/a: every line, when there is no /usr/bin/time to measure with.

import type { SizeName } from "../lib/gen.ts";
import { addExecutable, copyPackage, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { canMeasure, runHd, type RunResult } from "../lib/hd.ts";
import {
  failed,
  formatValue,
  judge,
  notApplicable,
  type Metric,
  type TargetResult,
} from "../lib/metric.ts";
import { removeTempDir } from "../lib/tmp.ts";

const NAME = "resources";
const SIZE_ORDER: readonly SizeName[] = ["small", "10k", "50k"];
const COMMANDS = ["check", "test", "build"] as const;
/** Per run; the 10k timeout is 30x the 1 s cold-check target, and 50k gets 5x that. */
const TIMEOUT_MS: Readonly<Record<SizeName, number>> = {
  small: 30_000,
  "10k": 30_000,
  "50k": 150_000,
};
const CPU_LABEL = "warm check, 10k lines: CPU time";
const RSS_LABEL = "warm check, 10k lines: peak RSS";
const CPU_LIMIT_MS = 200;
const RSS_LIMIT = 50 * 1024 * 1024;

/** A completed run's line: its CPU time and peak RSS. */
function measured(label: string, result: RunResult, timeoutMs: number): TargetResult {
  const cpu = result.cpuMs === undefined ? "?" : `${(result.cpuMs / 1000).toFixed(2)} CPU-s`;
  const rss = result.rssBytes === undefined ? "?" : formatValue(result.rssBytes, "MB");
  return {
    metric: NAME,
    name: label,
    value: `${cpu}, ${rss}`,
    target: `completes ≤ ${timeoutMs / 1000} s`,
    status: "pass",
    note: `wall ${formatValue(result.wallMs, "ms")}`,
  };
}

export const resources: Metric = {
  name: NAME,
  pillar: 2,
  summary:
    "CPU time and peak RSS of check, test and build on small, 10k and 50k lines, cold and warm",
  async run(context) {
    if (!canMeasure())
      return [
        notApplicable(NAME, CPU_LABEL, `≤ ${CPU_LIMIT_MS} ms`, "no /usr/bin/time on this host"),
        notApplicable(NAME, RSS_LABEL, "≤ 50.0 MB", "no /usr/bin/time on this host"),
      ];
    const results: TargetResult[] = [];
    let warmCheck10k: RunResult | undefined;
    let warmCheckProblem = "the 10k warm check did not run";
    const timedOutAt = new Map<string, SizeName>();
    for (const size of SIZE_ORDER) {
      const timeoutMs = TIMEOUT_MS[size];
      const { dir: source, pkg } = materialize(size, NAME);
      addExecutable(source);
      context.log(`${NAME}: ${size}, ${pkg.lines} lines`);
      for (const command of COMMANDS) {
        const coldLabel = `${command}, ${size}, cold`;
        const warmLabel = `${command}, ${size}, warm`;
        const target = `completes ≤ ${timeoutMs / 1000} s`;
        const blocker = timedOutAt.get("check") ?? timedOutAt.get(command);
        const skip =
          blocker &&
          `skipped: hd ${timedOutAt.has("check") ? "check" : command} timed out at ${blocker}`;
        if (skip) {
          results.push(
            failed(NAME, coldLabel, target, skip),
            failed(NAME, warmLabel, target, skip),
          );
          if (command === "check" && size === "10k") warmCheckProblem = skip;
          continue;
        }
        const dir = copyPackage(source, `${NAME}-${size}`);
        const options = { cwd: dir, env: freshCache(), timeoutMs, measure: true };
        let warm: RunResult | undefined;
        let warmSkip: string | undefined;
        const cold = await runHd(context.hd, [command], options);
        const coldProblem = runProblem(cold, timeoutMs, `cold hd ${command}`);
        if (coldProblem) {
          if (cold.timedOut) timedOutAt.set(command, size);
          results.push(failed(NAME, coldLabel, target, coldProblem));
          results.push(failed(NAME, warmLabel, target, "skipped: the cold run failed"));
          warmSkip = `the cold run failed: ${coldProblem}`;
        } else {
          results.push(measured(coldLabel, cold, timeoutMs));
          warm = await runHd(context.hd, [command], options);
          warmSkip = runProblem(warm, timeoutMs, `warm hd ${command}`);
          if (warmSkip && warm.timedOut) timedOutAt.set(command, size);
          results.push(
            warmSkip
              ? failed(NAME, warmLabel, target, warmSkip)
              : measured(warmLabel, warm, timeoutMs),
          );
        }
        if (command === "check" && size === "10k") {
          if (warmSkip) warmCheckProblem = warmSkip;
          else warmCheck10k = warm;
        }
        removeTempDir(dir);
      }
    }
    if (!warmCheck10k || warmCheck10k.cpuMs === undefined || warmCheck10k.rssBytes === undefined)
      return [
        failed(NAME, CPU_LABEL, `≤ ${CPU_LIMIT_MS} ms`, warmCheckProblem),
        failed(NAME, RSS_LABEL, "≤ 50.0 MB", warmCheckProblem),
        ...results,
      ];
    return [
      judge(NAME, CPU_LABEL, warmCheck10k.cpuMs, CPU_LIMIT_MS, "ms"),
      judge(NAME, RSS_LABEL, warmCheck10k.rssBytes, RSS_LIMIT, "MB"),
      ...results,
    ];
  },
};
