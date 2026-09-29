#!/usr/bin/env node
// Iterator performance benchmark harness - Stage 2

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { compilePhased, instantiateWithBytes, median, timeCalls, exported } from "./bench-lib.ts";

// import.meta.dirname gives audit/scripts/arch
const AUDIT_DIR = join(import.meta.dirname, "..", "..");
const BENCH_DIR = join(AUDIT_DIR, "bench", "iterator");
const RESULTS_DIR = join(AUDIT_DIR, "evidence", "iterator-perf");

mkdirSync(RESULTS_DIR, { recursive: true });

const programs = ["loop", "a-closure", "b-nested", "c-flat", "c-fused"];
const sizes = { w1: [100000], w2_4: [1000000], w3: [1000000] };

async function runBenchmark() {
  console.log("Iterator Performance Benchmark - Stage 2");
  console.log(`Date: ${new Date().toISOString()}`);
  console.log(`Node: ${process.version}`);

  const compiledPrograms: Record<string, Uint8Array> = {};
  const checksums: Record<string, Record<string, number>> = {};
  const results: Record<string, Record<string, { min: number; median: number }>> = {};

  // Compile dev versions only for speed
  for (const program of programs) {
    console.log(`Compiling ${program}.hd...`);
    const source = readFileSync(join(BENCH_DIR, `${program}.hd`), "utf-8");
    const compiled = compilePhased(source);
    compiledPrograms[program] = compiled.bytes;
  }

  // Run benchmarks
  for (const program of programs) {
    console.log(`\nBenchmarking ${program}...`);
    checksums[program] = {};
    results[program] = {};

    const source = readFileSync(join(BENCH_DIR, `${program}.hd`), "utf-8");
    const instance = await instantiateWithBytes(source, compiledPrograms[program]);

    for (const [workload, sizeList] of Object.entries(sizes)) {
      for (const size of sizeList) {
        try {
          const fn = exported(instance, workload) as (n: number) => number;
          const checksum = fn(size);
          checksums[program][`${workload}@${size}`] = checksum;

          const times = timeCalls(() => fn(size), 15, 5);
          const minTime = Math.min(...times);
          const medianTime = median(times);
          const minNs = (minTime * 1e6) / size;
          const medianNs = (medianTime * 1e6) / size;

          results[program][workload] = { min: minNs, median: medianNs };
          console.log(`${workload}@${size}: ${minNs.toFixed(2)} ns/elem`);
        } catch (e) {
          console.warn(`Error: ${e}`);
        }
      }
    }
  }

  // Verify checksums
  console.log("\nVerifying checksums...");
  for (const [workload, sizeList] of Object.entries(sizes)) {
    for (const size of sizeList) {
      const key = `${workload}@${size}`;
      const first = checksums[programs[0]][key];
      let ok = true;
      for (const prog of programs.slice(1)) {
        if (checksums[prog][key] !== first) {
          console.warn(`MISMATCH: ${workload}@${size}`);
          ok = false;
        }
      }
      if (ok) console.log(`✓ ${workload}@${size}`);
    }
  }

  // Write results
  const md = `# Iterator Performance - Stage 2 Results

Date: ${new Date().toISOString()}
Node: ${process.version}

## Results (ns per element)

| Program | w1 min | w2_4 min | w3 min |
| --- | --- | --- | --- |
${programs
  .map(
    (p) =>
      `| ${p} | ${results[p]["w1"]?.min.toFixed(2) || "N/A"} | ${results[p]["w2_4"]?.min.toFixed(2) || "N/A"} | ${results[p]["w3"]?.min.toFixed(2) || "N/A"} |`,
  )
  .join("\n")}

## Checksums

${JSON.stringify(checksums, null, 2)}
`;

  writeFileSync(join(RESULTS_DIR, "results.md"), md);
  console.log(`\nResults: ${RESULTS_DIR}/results.md`);
}

await runBenchmark().catch((e) => {
  console.error(e);
  process.exit(1);
});
