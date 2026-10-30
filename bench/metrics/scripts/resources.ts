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

import { addExecutable, copyPackage, materialize, type SizeName } from "../lib/gen.ts";
import { canMeasure, freshCache, runHd, runProblem, type RunResult } from "../lib/hd.ts";
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

interface CommandRuns {
  readonly results: TargetResult[];
  readonly warm?: RunResult;
  readonly problem?: string;
}

async function measureCommand({
  context,
  source,
  size,
  command,
  timedOutAt,
}: {
  context: Parameters<Metric["run"]>[0];
  source: string;
  size: SizeName;
  command: (typeof COMMANDS)[number];
  timedOutAt: Map<string, SizeName>;
}): Promise<CommandRuns> {
  const timeoutMs = TIMEOUT_MS[size];
  const coldLabel = `${command}, ${size}, cold`;
  const warmLabel = `${command}, ${size}, warm`;
  const target = `completes ≤ ${timeoutMs / 1000} s`;
  const blocker = timedOutAt.get("check") ?? timedOutAt.get(command);
  if (blocker) {
    const cause = timedOutAt.has("check") ? "check" : command;
    const problem = `skipped: hd ${cause} timed out at ${blocker}`;
    return {
      results: [failed(NAME, coldLabel, target, problem), failed(NAME, warmLabel, target, problem)],
      problem,
    };
  }

  const dir = copyPackage(source, `${NAME}-${size}`);
  const options = { cwd: dir, env: freshCache(), timeoutMs, measure: true };
  const cold = await runHd(context.hd, [command], options);
  const coldProblem = runProblem(cold, timeoutMs, `cold hd ${command}`);
  if (coldProblem) {
    if (cold.timedOut) {
      timedOutAt.set(command, size);
    }
    removeTempDir(dir);
    return {
      results: [
        failed(NAME, coldLabel, target, coldProblem),
        failed(NAME, warmLabel, target, "skipped: the cold run failed"),
      ],
      problem: `the cold run failed: ${coldProblem}`,
    };
  }

  const warm = await runHd(context.hd, [command], options);
  const warmProblem = runProblem(warm, timeoutMs, `warm hd ${command}`);
  if (warmProblem && warm.timedOut) {
    timedOutAt.set(command, size);
  }
  removeTempDir(dir);
  return {
    results: [
      measured(coldLabel, cold, timeoutMs),
      warmProblem
        ? failed(NAME, warmLabel, target, warmProblem)
        : measured(warmLabel, warm, timeoutMs),
    ],
    warm,
    problem: warmProblem,
  };
}

export const resources: Metric = {
  name: NAME,
  pillar: 2,
  summary:
    "CPU time and peak RSS of check, test and build on small, 10k and 50k lines, cold and warm",
  async run(context) {
    if (!canMeasure()) {
      return [
        notApplicable(NAME, CPU_LABEL, `≤ ${CPU_LIMIT_MS} ms`, "no /usr/bin/time on this host"),
        notApplicable(NAME, RSS_LABEL, "≤ 50.0 MB", "no /usr/bin/time on this host"),
      ];
    }
    const results: TargetResult[] = [];
    let warmCheck10k: RunResult | undefined;
    let warmCheckProblem = "the 10k warm check did not run";
    const timedOutAt = new Map<string, SizeName>();
    for (const size of SIZE_ORDER) {
      const { dir: source, pkg } = materialize(size, NAME);
      addExecutable(source);
      context.log(`${NAME}: ${size}, ${pkg.lines} lines`);
      for (const command of COMMANDS) {
        const runs = await measureCommand({ context, source, size, command, timedOutAt });
        results.push(...runs.results);
        if (command === "check" && size === "10k") {
          if (runs.problem) {
            warmCheckProblem = runs.problem;
          } else {
            warmCheck10k = runs.warm;
          }
        }
      }
    }
    if (!warmCheck10k || warmCheck10k.cpuMs === undefined || warmCheck10k.rssBytes === undefined) {
      return [
        failed(NAME, CPU_LABEL, `≤ ${CPU_LIMIT_MS} ms`, warmCheckProblem),
        failed(NAME, RSS_LABEL, "≤ 50.0 MB", warmCheckProblem),
        ...results,
      ];
    }
    return [
      judge(NAME, CPU_LABEL, warmCheck10k.cpuMs, CPU_LIMIT_MS, "ms"),
      judge(NAME, RSS_LABEL, warmCheck10k.rssBytes, RSS_LIMIT, "MB"),
      ...results,
    ];
  },
};
