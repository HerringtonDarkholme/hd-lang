// Persistent converter for the opt-in conformance harness. One acknowledged
// request at a time bounds raw storage to one profile, independent of case count.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { caseLineIndex } from "./case-lines.ts";

const [binary, directory] = process.argv.slice(2);
if (!binary || !directory) {
  throw new Error("pass the exact instrumented binary and output directory");
}
const root = join(import.meta.dirname, "../..");
const merged = join(directory, "current.profdata");
const records = join(directory, "cases.jsonl");
const sourceLines = join(directory, "lines.json");
let index: ReturnType<typeof caseLineIndex> | undefined;
// A new collector starts a new measurement; never append to an older run.
writeFileSync(records, "");
for await (const line of createInterface({ input: process.stdin })) {
  const [serial, elapsed, path, extra] = line.split("\t");
  if (!/^\d+$/.test(serial) || !path || extra !== undefined || !Number.isFinite(Number(elapsed))) {
    throw new Error("invalid attribution request");
  }
  const raw = join(directory, `${serial}.profraw`);
  const start = performance.now();
  const rawBytes = statSync(raw).size;
  // cost: one sparse merge and export per case, both bounded. The persistent
  // process avoids a Node startup/source-classification pass for every case.
  execFileSync(
    process.env.LLVM_PROFDATA ?? "llvm-profdata",
    ["merge", "-sparse", raw, "-o", merged],
    {
      timeout: 20_000,
      stdio: ["ignore", "ignore", "pipe"],
    },
  );
  const lcov = execFileSync(
    process.env.LLVM_COV ?? "llvm-cov",
    ["export", binary, "-format=lcov", `-instr-profile=${merged}`, "-skip-functions"],
    { encoding: "utf8", timeout: 20_000, maxBuffer: 1 << 28 },
  );
  if (!index) {
    index = caseLineIndex(lcov, (file) => readFileSync(join(root, file), "utf8"));
    writeFileSync(sourceLines, JSON.stringify(index.lines) + "\n");
    writeFileSync(
      join(directory, "measurement.json"),
      JSON.stringify({
        binary,
        commit: execFileSync("git", ["rev-parse", "HEAD"], {
          cwd: root,
          encoding: "utf8",
          timeout: 10_000,
        }).trim(),
        pipeline: process.env.HD_CONFORMANCE_PIPELINE ?? "dev",
        checked: process.env.HD_CONFORMANCE_CHECKED === "1",
        selection: process.env.HD_CONFORMANCE_ONLY ?? null,
      }) + "\n",
    );
  }
  const covered = index.covered(lcov);
  appendFileSync(
    records,
    JSON.stringify({
      id: Number(serial),
      path,
      seconds: Number(elapsed),
      rawBytes,
      conversionSeconds: (performance.now() - start) / 1000,
      lines: covered,
    }) + "\n",
  );
  rmSync(raw);
  rmSync(merged);
  process.stdout.write("ok\n");
}
