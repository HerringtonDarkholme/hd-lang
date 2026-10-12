// cache-growth: the compilation cache stays within a configured cap.
//
// A scripted day of edits on the small package: 100 cycles, each an edit
// then `hd check`; every 10th edit adds or removes the defaulted parameter
// of a public signature, the others change a private body. The documented cap variable (findCacheCap) is set to 5 MB,
// as a byte count. The value is the cache's size after the day: HD_CACHE,
// or the files the checks wrote inside the package.
// Target (Pillar 2): bounded by the configured cap.
// n/a: when `hd check` writes no compilation cache, or no help text
// documents a cap.

import { lstatSync } from "node:fs";
import { join } from "node:path";

import { findCacheCap, findCompileCache, helpText } from "../lib/capability.ts";
import { editBody, editSignature } from "../lib/gen.ts";
import { editFile, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { runHd } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { listTree, treeBytes } from "../lib/tmp.ts";

const NAME = "cache-growth";
const CYCLES = 100;
const CAP = 5 * 1024 * 1024;
const TIMEOUT_MS = 30_000;
const LABEL = `cache size after ${CYCLES} edits, cap 5 MB`;
const TARGET = "≤ 5.0 MB";
/** The parameter `editSignature` adds. */
const ADDED = ", extra: i32 = 0)";

export const cacheGrowth: Metric = {
  name: NAME,
  pillar: 2,
  summary: "compilation cache size after a scripted day of edits, against a configured cap",
  async run(context) {
    const cache = await findCompileCache(context.hd);
    if (cache === undefined)
      return [notApplicable(NAME, LABEL, TARGET, "hd check writes no compilation cache")];
    if (typeof cache === "string") return [failed(NAME, LABEL, TARGET, cache)];
    const cap = findCacheCap((await helpText(context.hd)).all);
    if (!cap) return [notApplicable(NAME, LABEL, TARGET, "no documented cache size cap")];
    const { dir, pkg } = materialize("small", NAME);
    const env = { ...freshCache(), [cap]: String(CAP) };
    context.log(`${NAME}: ${CYCLES} edits and checks, ${cap}=${CAP}`);
    for (let index = 0; index < CYCLES; index++) {
      const module = pkg.modules[index % pkg.modules.length]!;
      // A signature edit adds the defaulted parameter, or takes it away again.
      const signature = (text: string) =>
        text.includes(ADDED) ? text.replace(ADDED, ")") : editSignature(text);
      editFile(dir, module.file, index % 10 === 9 ? signature : editBody);
      const result = await runHd(context.hd, ["check"], { cwd: dir, env, timeoutMs: TIMEOUT_MS });
      const problem = runProblem(result, TIMEOUT_MS, `check ${index + 1}`);
      if (problem) return [failed(NAME, LABEL, TARGET, problem)];
    }
    const bytes =
      cache.where === "HD_CACHE"
        ? treeBytes(env.HD_CACHE!).bytes
        : listTree(dir)
            .filter((path) => !pkg.files.has(path))
            .reduce((sum, path) => sum + lstatSync(join(dir, path)).size, 0);
    return [judge(NAME, LABEL, bytes, CAP, "MB", "at-most", `cap set by ${cap}`)];
  },
};
