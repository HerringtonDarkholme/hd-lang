// Runs one CI check step and fails it when it takes longer than its budget
// in scripts/check-budgets.tsv (Cost Model, .agents/skills/shared-rules.md:
// a check that runs on every change exists to be fast).
//
// Each budget is about 1.5 times the step's CI time in recent green runs, so
// a step that grows with accumulated state fails here instead of slowing
// every run. A step without a row fails too. Raising a budget is a reviewed
// change to the TSV that says why.
//
// Cost per run: one node process per wrapped step, reading a ten-line file.
//
// Usage: node scripts/check-budget.ts STEP -- COMMAND [ARG]...

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const BUDGETS = "scripts/check-budgets.tsv";
const root = join(import.meta.dirname, "..");

const args = process.argv.slice(2);
const split = args.indexOf("--");
const step = args[0];
const command = args.slice(split + 1);
if (split !== 1 || step === undefined || command.length === 0) {
  process.stderr.write("usage: node scripts/check-budget.ts STEP -- COMMAND [ARG]...\n");
  process.exit(2);
}

const budgets = new Map<string, number>();
for (const line of readFileSync(join(root, BUDGETS), "utf8").split("\n").slice(1)) {
  const [name, seconds] = line.split("\t");
  if (name && seconds) {
    budgets.set(name, Number(seconds));
  }
}
const budget = budgets.get(step);
if (budget === undefined || !Number.isFinite(budget)) {
  process.stderr.write(`::error::${step} has no budget in ${BUDGETS}\n`);
  process.exit(2);
}

const started = performance.now();
const run = spawnSync(command[0]!, command.slice(1), { stdio: "inherit" });
const seconds = (performance.now() - started) / 1000;
if (run.error) {
  process.stderr.write(`${step}: ${run.error.message}\n`);
  process.exit(2);
}
if (run.status !== 0) {
  process.exit(run.status ?? 1);
}
const measured = `${step} took ${seconds.toFixed(1)} s (budget ${budget} s, ${BUDGETS})`;
if (seconds > budget) {
  process.stderr.write(
    `::error::${measured}: over budget. Find the cost that grew (Cost Model, .agents/skills/shared-rules.md).\n`,
  );
  process.exit(1);
}
process.stdout.write(`${measured}\n`);
