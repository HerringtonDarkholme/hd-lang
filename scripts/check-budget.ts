// Runs one CI check step and fails it on a blow-up: three times its budget in
// scripts/check-budgets.tsv (Cost Model, .agents/skills/shared-rules.md: a
// check that runs on every change exists to be fast).
//
// Each budget is a generous time for the step, a few times its CI time in
// recent green runs. Over the budget only warns, so runner noise never fails a
// run; a step that grows with accumulated state fails at three times it. A
// step without a row fails too.
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
const blowUp = seconds > 3 * budget;
if (seconds > budget) {
  process.stderr.write(
    `::${blowUp ? "error" : "warning"}::${measured}: ${blowUp ? "over three times" : "over"} its budget. Find the cost that grew (Cost Model, .agents/skills/shared-rules.md).\n`,
  );
}
process.stdout.write(`${measured}\n`);
process.exit(blowUp ? 1 : 0);
