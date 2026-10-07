// parallel-speedup: one hd check uses the machine's cores.
//
// A cold `hd check` (fresh copy, empty HD_CACHE) of the 50k-line package
// with 1 thread, then with as many threads as cores, through the thread
// control `hd help` documents (findThreadControl). When the 1-thread
// check of 50k lines times out, the 10k package is used instead.
// The speedup is the 1-thread wall time over the all-threads wall time.
// Target (Pillar 2): ≥ 0.6 × cores, for up to 8 cores.
// n/a: when no help text documents a thread count flag or variable.

import { availableParallelism } from "node:os";

import { findThreadControl, helpText } from "../lib/capability.ts";
import type { SizeName } from "../lib/gen.ts";
import { copyPackage, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { runHd } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric } from "../lib/metric.ts";

const NAME = "parallel-speedup";
const TIMEOUT_MS = 150_000;

export const parallelSpeedup: Metric = {
  name: NAME,
  pillar: 2,
  summary: "check of the 50k-line package with 1 thread against all cores",
  async run(context) {
    const cores = availableParallelism();
    const limit = 0.6 * Math.min(cores, 8);
    const target = `≥ ${limit.toFixed(2)}x`;
    const label = (size: string) => `speedup, ${size} lines, ${cores} threads vs 1`;
    const help = await helpText(context.hd);
    const control = findThreadControl(help.check, help.all);
    if (!control)
      return [
        notApplicable(NAME, label("50k"), target, "no documented thread count flag or variable"),
      ];
    let problem = "";
    for (const size of ["50k", "10k"] as const satisfies readonly SizeName[]) {
      const { dir: source } = materialize(size, NAME);
      const check = async (threads: number) =>
        runHd(context.hd, ["check", ...control.args(threads)], {
          cwd: copyPackage(source, NAME),
          env: { ...freshCache(), ...control.env(threads) },
          timeoutMs: TIMEOUT_MS,
        });
      context.log(`${NAME}: ${size}, 1 thread, then ${cores}, by ${control.name}`);
      const one = await check(1);
      problem = runProblem(one, TIMEOUT_MS, `the 1-thread check of ${size}`) ?? "";
      if (one.timedOut && size === "50k") continue;
      if (problem) return [failed(NAME, label(size), target, problem)];
      const all = await check(cores);
      problem = runProblem(all, TIMEOUT_MS, `the ${cores}-thread check of ${size}`) ?? "";
      if (problem) return [failed(NAME, label(size), target, problem)];
      return [
        judge(
          NAME,
          label(size),
          one.wallMs / all.wallMs,
          limit,
          "x",
          "at-least",
          `by ${control.name}`,
        ),
      ];
    }
    return [failed(NAME, label("10k"), target, problem)];
  },
};
