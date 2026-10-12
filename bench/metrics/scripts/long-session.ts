// long-session: memory stays flat over a long session.
//
// With a REPL that reads standard input (hasStdinRepl): one `hd repl`
// session outside any package, fed 1,000 inputs in steps of 50. Even
// input i binds `wI := +(1000000 + i)` and odd input i shows `w(i-1) + 1`,
// so every input shows the value 1000000 + i, which marks how far the
// session got (cli.repl.value). After each step the RSS of the session's
// process group is sampled with `ps`. The growth is the largest RSS from
// input 100 on, minus the RSS at input 100.
// Without such a REPL: 50 rechecks of the small package, each after a
// private body edit, each measured with /usr/bin/time. The growth is the
// largest peak RSS from recheck 10 on, minus the peak RSS of recheck 10.
// Target (Pillar 2): flat, here a growth ≤ 10 MB (this harness's bound).
// n/a: without a REPL, when there is no /usr/bin/time.

import { editBody } from "../lib/gen.ts";
import { hasStdinRepl } from "../lib/capability.ts";
import { editFile, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { canMeasure, runHd, runSession, type SessionStep } from "../lib/hd.ts";
import type { MetricContext } from "../lib/metric.ts";
import { failed, formatValue, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "long-session";
const LIMIT = 10 * 1024 * 1024;
const TARGET = "≤ 10.0 MB";
const INPUTS = 1000;
const STEP = 50;
const BASELINE_INPUT = 100;
const MARK = 1_000_000;
const SESSION_TIMEOUT_MS = 120_000;
const RECHECKS = 50;
const RECHECK_BASELINE = 10;
const RECHECK_TIMEOUT_MS = 30_000;

/** Input i of the session; each shows the value MARK + i. */
export function replInput(index: number): string {
  return index % 2 === 0 ? `w${index} := +${MARK + index}` : `w${index - 1} + 1`;
}

/** The session's steps: 50 inputs each, done once the last input's value shows. */
export function replSteps(inputs = INPUTS, step = STEP): SessionStep[] {
  const steps: SessionStep[] = [];
  for (let start = 0; start < inputs; start += step) {
    const indexes = Array.from({ length: Math.min(step, inputs - start) }, (_, k) => start + k);
    const marker = `${MARK + indexes.at(-1)!} : `;
    steps.push({
      input: `${indexes.map(replInput).join("\n")}\n`,
      done: (stdout) => stdout.includes(marker),
    });
  }
  return steps;
}

/** The largest sample from `from` on, minus the sample at `from`; NaN when a sample is missing. */
export function growthFrom(samples: readonly (number | undefined)[], from: number): number {
  const base = samples[from];
  const later = samples.slice(from).filter((value): value is number => value !== undefined);
  if (base === undefined || later.length === 0) return Number.NaN;
  return Math.max(...later) - base;
}

async function replSession(context: MetricContext) {
  const label = `RSS growth, REPL inputs ${BASELINE_INPUT} to ${INPUTS}`;
  const result = await runSession([...context.hd.argv, "repl"], {
    cwd: makeTempDir(NAME),
    timeoutMs: SESSION_TIMEOUT_MS,
    steps: replSteps(),
  });
  const base = BASELINE_INPUT / STEP - 1;
  const reached = result.rssBytes.length * STEP;
  const detail = (): string => {
    const at = (index: number) => {
      const value = result.rssBytes[index];
      return value === undefined ? "?" : formatValue(value, "MB");
    };
    const last = result.rssBytes.length - 1;
    return last < 0
      ? "no input completed"
      : `RSS ${at(Math.min(base, last))} at input ${Math.min(base, last) * STEP + STEP}, ${at(last)} at input ${reached}`;
  };
  if (result.problem) return [failed(NAME, label, TARGET, `${result.problem}; ${detail()}`)];
  const growth = growthFrom(result.rssBytes, base);
  if (Number.isNaN(growth))
    return [failed(NAME, label, TARGET, "ps reported no RSS for the session")];
  return [judge(NAME, label, growth, LIMIT, "MB", "at-most", detail())];
}

async function rechecks(context: MetricContext) {
  const label = `peak RSS growth, rechecks ${RECHECK_BASELINE} to ${RECHECKS}`;
  if (!canMeasure()) return [notApplicable(NAME, label, TARGET, "no REPL and no /usr/bin/time")];
  const { dir, pkg } = materialize("small", NAME);
  const env = freshCache();
  const samples: number[] = [];
  for (let index = 0; index < RECHECKS; index++) {
    editFile(dir, pkg.modules[index % pkg.modules.length]!.file, editBody);
    const result = await runHd(context.hd, ["check"], {
      cwd: dir,
      env,
      timeoutMs: RECHECK_TIMEOUT_MS,
      measure: true,
    });
    const problem = runProblem(result, RECHECK_TIMEOUT_MS, `recheck ${index + 1}`);
    if (problem) return [failed(NAME, label, TARGET, problem)];
    samples.push(result.rssBytes ?? Number.NaN);
  }
  const growth = growthFrom(samples, RECHECK_BASELINE - 1);
  return [judge(NAME, label, growth, LIMIT, "MB", "at-most", "no REPL that reads standard input")];
}

export const longSession: Metric = {
  name: NAME,
  pillar: 2,
  summary: "RSS across 1,000 REPL inputs, or across 50 rechecks after edits without a REPL",
  async run(context) {
    if (await hasStdinRepl(context.hd)) {
      context.log(`${NAME}: ${INPUTS} REPL inputs in steps of ${STEP}`);
      return replSession(context);
    }
    context.log(`${NAME}: no REPL that reads standard input; ${RECHECKS} rechecks`);
    return rechecks(context);
  },
};
