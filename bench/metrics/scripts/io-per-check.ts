// io-per-check: a warm check reads what changed, not the whole package.
//
// The 10k-line package gets a warm-up `hd check`, then a private body edit
// in one module, then a traced `hd check`. The tracer (lib/trace.ts) logs
// every file call of hd and the processes it starts. The value is the
// package's source files the check opened for reading; the note gives the
// files it stat'ed or checked in other ways.
// Target (Pillar 2): proportional to what changed, here 1 file read.
// n/a: when no tracer runs without elevated privileges (always on macOS,
// where fs_usage and dtruss need root), or when `hd check` writes no
// compilation cache, so there is no warm state.

import { readFileSync, realpathSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";

import { findCompileCache } from "../lib/capability.ts";
import { editBody } from "../lib/gen.ts";
import { editFile, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { runProcess, runHd } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";
import { fileTracer, parseStrace } from "../lib/trace.ts";

const NAME = "io-per-check";
const LABEL = "source files read, warm check after one edit";
const TARGET = "≤ 1";
const TIMEOUT_MS = 60_000;

/** The paths in `paths` under one of `roots`, made absolute against `cwd`, deduplicated. */
export function under(paths: Iterable<string>, roots: readonly string[], cwd: string): string[] {
  const found = new Set<string>();
  for (const path of paths) {
    const full = isAbsolute(path) ? path : resolve(cwd, path);
    if (roots.some((root) => full.startsWith(`${root}/`))) found.add(full);
  }
  return [...found].sort();
}

export const ioPerCheck: Metric = {
  name: NAME,
  pillar: 2,
  summary: "source files a warm check reads after a one-module edit",
  async run(context) {
    const log = join(makeTempDir("trace"), "trace.log");
    const tracer = fileTracer(log);
    if (typeof tracer === "string") return [notApplicable(NAME, LABEL, TARGET, tracer)];
    const cache = await findCompileCache(context.hd);
    if (cache === undefined)
      return [notApplicable(NAME, LABEL, TARGET, "hd check writes no compilation cache")];
    if (typeof cache === "string") return [failed(NAME, LABEL, TARGET, cache)];
    const { dir, pkg } = materialize("10k", NAME);
    const env = freshCache();
    const warm = await runHd(context.hd, ["check"], { cwd: dir, env, timeoutMs: TIMEOUT_MS });
    const warmProblem = runProblem(warm, TIMEOUT_MS, "the warm-up check");
    if (warmProblem) return [failed(NAME, LABEL, TARGET, warmProblem)];
    const module = pkg.modules[Math.floor(pkg.modules.length / 2)]!;
    editFile(dir, module.file, editBody);
    const traced = await runProcess([...tracer, ...context.hd.argv, "check"], {
      cwd: dir,
      env,
      timeoutMs: TIMEOUT_MS,
    });
    const problem = runProblem(traced, TIMEOUT_MS, "the traced check");
    if (problem) return [failed(NAME, LABEL, TARGET, problem)];
    const access = parseStrace(readFileSync(log, "utf8"));
    const roots = [...new Set([join(dir, "src"), join(realpathSync(dir), "src")])];
    const reads = under(access.reads, roots, dir);
    const stats = under(access.stats, roots, dir);
    const sources = [...pkg.files.keys()].filter((path) => path.startsWith("src/")).length;
    return [
      judge(
        NAME,
        LABEL,
        reads.length,
        1,
        "count",
        "at-most",
        `${stats.length} stat'ed; ${sources} source files; edited ${module.file}`,
      ),
    ];
  },
};
