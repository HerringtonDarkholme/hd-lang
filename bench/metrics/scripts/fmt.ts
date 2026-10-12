// fmt: `hd fmt` speed on a 10k-line package, and idempotence.
//
// A temporary copy of the generated package is formatted once (timed), then
// formatted again; the second run must change no file.
// Targets (Pillar 1): ≤ 200 ms; idempotent.
// n/a: both lines, when `hd help fmt` fails (no formatter).

import { materialize, runProblem } from "../lib/fixture.ts";
import { runHd, supportsCommand } from "../lib/hd.ts";
import { failed, judge, judgeBool, notApplicable, type Metric } from "../lib/metric.ts";
import { readTree } from "../lib/tmp.ts";

const NAME = "fmt";
const TIMEOUT_MS = 30_000;

export const fmt: Metric = {
  name: NAME,
  pillar: 1,
  summary: "hd fmt time on a 10k-line package, and idempotence",
  async run(context) {
    const { dir } = materialize("10k", NAME);
    if (!(await supportsCommand(context.hd, "fmt", dir)))
      return [
        notApplicable(NAME, "hd fmt, 10k lines", "≤ 200 ms", "no hd fmt"),
        notApplicable(NAME, "second hd fmt changes nothing", "idempotent", "no hd fmt"),
      ];
    const first = await runHd(context.hd, ["fmt"], { cwd: dir, timeoutMs: TIMEOUT_MS });
    const problem = runProblem(first, TIMEOUT_MS, "hd fmt");
    if (problem)
      return [
        failed(NAME, "hd fmt, 10k lines", "≤ 200 ms", problem),
        failed(NAME, "second hd fmt changes nothing", "idempotent", problem),
      ];
    const once = readTree(dir);
    const second = await runHd(context.hd, ["fmt"], { cwd: dir, timeoutMs: TIMEOUT_MS });
    const twice = readTree(dir);
    const changed = [...twice]
      .filter(([path, text]) => once.get(path) !== text)
      .map(([path]) => path);
    return [
      judge(NAME, "hd fmt, 10k lines", first.wallMs, 200, "ms"),
      judgeBool(
        NAME,
        "second hd fmt changes nothing",
        second.status === 0 && changed.length === 0 && once.size === twice.size,
        "idempotent",
        changed.length ? `${changed.length} files changed` : "no change",
        changed.length ? `changed: ${changed.slice(0, 5).join(", ")}` : undefined,
      ),
    ];
  },
};
