import { readFile } from "node:fs/promises";
import { availableParallelism } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { isMainThread, parentPort, Worker, workerData } from "node:worker_threads";

import { parserSelfTest, parseSource } from "./parser.ts";

interface ManifestCase {
  readonly expectation: string;
  readonly path: string;
  readonly phase: string;
}

interface WorkerTask {
  readonly cases: readonly ManifestCase[];
  readonly root: string;
}

function parseJobs(): number {
  const fallback = Math.min(8, availableParallelism());
  const jobs = Number(process.env.HD_SPEC_JOBS ?? process.env.HD_TEST_JOBS ?? fallback);
  if (!Number.isInteger(jobs) || jobs < 1)
    throw new Error("HD_SPEC_JOBS must be a positive integer");
  return jobs;
}

async function readCases(manifest: string): Promise<ManifestCase[]> {
  return (await readFile(manifest, "utf8"))
    .split(/\r?\n/)
    .slice(1)
    .filter(Boolean)
    .map((row) => {
      const [path = "", phase = "", expectation = ""] = row.split("\t");
      return { expectation, path, phase };
    });
}

async function checkCases(task: WorkerTask): Promise<string[]> {
  const failures: string[] = [];
  for (const testCase of task.cases) {
    const source = await readFile(resolve(task.root, testCase.path), "utf8");
    const diagnostics = parseSource(source);
    const codes = new Set(diagnostics.map(({ code }) => code));
    const parseReject = testCase.phase === "parse" && testCase.expectation.startsWith("reject:");
    const rendered = diagnostics.map(({ code, line }) => `${code}:${line}`).join(", ");
    if (!parseReject && diagnostics.length > 0)
      failures.push(`${testCase.path}: expected to parse, got ${rendered}`);
    else if (parseReject) {
      const expected = testCase.expectation.split(":", 2)[1]!;
      if (!codes.has(expected))
        failures.push(`${testCase.path}: expected ${expected}, got ${rendered || "accept"}`);
    }
  }
  return failures;
}

function workerCheck(task: WorkerTask): Promise<string[]> {
  return new Promise((complete, reject) => {
    const worker = new Worker(new URL(import.meta.url), { workerData: task });
    worker.once("message", (message: string[]) => complete(message));
    worker.once("error", reject);
    worker.once("exit", (code) => {
      if (code !== 0) reject(new Error(`reference parser worker exited with status ${code}`));
    });
  });
}

async function runParallel(
  cases: readonly ManifestCase[],
  root: string,
  jobs: number,
): Promise<string[]> {
  const groups = Array.from({ length: Math.min(jobs, cases.length) }, () => [] as ManifestCase[]);
  for (const [index, testCase] of cases.entries()) groups[index % groups.length]!.push(testCase);
  return (await Promise.all(groups.map((group) => workerCheck({ cases: group, root })))).flat();
}

async function main(args: readonly string[]): Promise<number> {
  if (args.length !== 2) {
    console.error("usage: reference-parser/index.ts CASES_TSV CONFORMANCE_DIR");
    return 2;
  }
  const manifest = resolve(args[0]!);
  const root = resolve(args[1]!);
  const failures = parserSelfTest();
  failures.push(...(await runParallel(await readCases(manifest), root, parseJobs())));
  if (failures.length > 0) {
    console.error(
      `reference parser failures:\n${failures.map((failure) => `- ${failure}`).join("\n")}`,
    );
    return 1;
  }
  console.log("chapter-02 Earley parser passed for every fixture");
  return 0;
}

if (!isMainThread) parentPort!.postMessage(await checkCases(workerData as WorkerTask));
else {
  const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
  if (invokedPath === fileURLToPath(import.meta.url))
    process.exitCode = await main(process.argv.slice(2));
}
