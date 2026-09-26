// 5.3: generic (erased) vs monomorphic sum and sort, timed and allocation-counted.
//   node --experimental-strip-types audit/scripts/arch/erasure-bench.ts
import { writeFileSync } from "node:fs";
import {
  callExport,
  instantiateCounted,
  instantiatePlain,
  median,
  readProbe,
  ROOT,
} from "./alloc-lib.ts";

const source = readProbe("audit/probes/arch/erasure/erasure-bench.hd");
const plain = await instantiatePlain(source);
const counted = await instantiateCounted(source);
const lines: string[] = [
  `# commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/erasure-bench.ts; date: ${new Date().toISOString()}`,
  "# CPU shared with other audit workers: pairs are interleaved, 9 rounds, medians; read ratios",
];
const log = (line: string): void => {
  lines.push(line);
  console.log(line);
};

interface Pair {
  readonly label: string;
  readonly generic: string;
  readonly mono: string;
  readonly n: number;
  readonly reps: number;
  readonly baseline?: string;
}

const pairs: Pair[] = [
  {
    label: "sum list[i32]",
    generic: "bench_sum_generic_i32",
    mono: "bench_sum_mono_i32",
    n: 1000,
    reps: 4000,
  },
  {
    label: "sum list[f64]",
    generic: "bench_sum_generic_f64",
    mono: "bench_sum_mono_f64",
    n: 1000,
    reps: 4000,
  },
  {
    label: "insertion sort list[i32]",
    generic: "bench_sort_generic_i32",
    mono: "bench_sort_mono_i32",
    n: 300,
    reps: 60,
    baseline: "bench_build_i32",
  },
];

const time = (name: string, n: number, reps: number): number => {
  const start = performance.now();
  callExport(plain, name, n, reps);
  return performance.now() - start;
};

log("\n## timing");
log(
  "case\tn\treps\tgeneric_ms\tmono_ms\tbuild_ms\tratio generic/mono (build subtracted)",
);
for (const pair of pairs) {
  const g = callExport(plain, pair.generic, pair.n, 1);
  const m = callExport(plain, pair.mono, pair.n, 1);
  if (g !== m) log(`# RESULT MISMATCH ${pair.label}: generic ${g} mono ${m}`);
  for (let warm = 0; warm < 3; warm += 1) {
    time(pair.generic, pair.n, pair.reps);
    time(pair.mono, pair.n, pair.reps);
  }
  const gs: number[] = [];
  const ms: number[] = [];
  const bs: number[] = [];
  for (let round = 0; round < 9; round += 1) {
    gs.push(time(pair.generic, pair.n, pair.reps));
    ms.push(time(pair.mono, pair.n, pair.reps));
    if (pair.baseline) bs.push(time(pair.baseline, pair.n, pair.reps));
  }
  const build = pair.baseline ? median(bs) : 0;
  const ratio = (median(gs) - build) / (median(ms) - build);
  log(
    `${pair.label}\t${pair.n}\t${pair.reps}\t${median(gs).toFixed(2)}\t${median(ms).toFixed(2)}\t${build.toFixed(2)}\t${ratio.toFixed(2)}`,
  );
}

log(
  "\n## allocations per element (instrumented build, one call with reps=1 minus reps=0)",
);
log(
  "case\tn\tgeneric allocs/elem\tmono allocs/elem\tgeneric by type\tmono by type",
);
for (const pair of pairs) {
  const perElement = (name: string): [number, string] => {
    counted.reset();
    callExport(counted.instance, name, pair.n, 0);
    const base = counted.counts();
    const baseTotal = counted.total();
    counted.reset();
    callExport(counted.instance, name, pair.n, 1);
    const total = counted.total() - baseTotal;
    const byType = Object.entries(counted.counts())
      .map(([type, count]) => [type, count - (base[type] ?? 0)] as const)
      .filter(([, count]) => count !== 0)
      .map(([type, count]) => `${type}=${count}`)
      .join(" ");
    return [total / pair.n, byType];
  };
  const [ga, gt] = perElement(pair.generic);
  const [ma, mt] = perElement(pair.mono);
  log(
    `${pair.label}\t${pair.n}\t${ga.toFixed(2)}\t${ma.toFixed(2)}\t${gt}\t${mt}`,
  );
}
writeFileSync(
  `${ROOT}audit/evidence/05-object-model/erasure-bench.txt`,
  lines.join("\n") + "\n",
);
