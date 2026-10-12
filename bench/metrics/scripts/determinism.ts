// determinism: the same input gives byte-identical output, run after run.
//
// Inputs, each run 10 times in the same directory:
// - a small generated package with one failing test: `hd check`,
//   `hd check --format json`, `hd test`, `hd test --format json`, and
//   `hd doc m001.Item1` when this hd has `hd doc`;
// - three mistake programs: `hd check --tests --format json FILE`.
// A run's output is its exit status, stdout and stderr together.
// Target (Pillar 1): every input byte-identical across its 10 runs.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { failingPackage } from "../lib/fixture.ts";
import { runHd, supportsCommand } from "../lib/hd.ts";
import { judgeBool, type Metric } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "determinism";
const RUNS = 10;
const TIMEOUT_MS = 60_000;
const PROGRAMS = ["readonly-provider", "interpolate-optional", "tuple-short-binding"];

export const determinism: Metric = {
  name: NAME,
  pillar: 1,
  summary: "same inputs run 10 times give byte-identical output",
  async run(context) {
    const { dir } = failingPackage(NAME);
    const programs = makeTempDir("determinism");
    for (const name of PROGRAMS)
      writeFileSync(
        join(programs, `${name}.hd`),
        readFileSync(join(context.repoRoot, "test", "metrics", "mistakes", `${name}.hd`), "utf8"),
      );
    const inputs: { label: string; args: string[]; cwd: string }[] = [
      { label: "check", args: ["check"], cwd: dir },
      { label: "check json", args: ["check", "--format", "json"], cwd: dir },
      { label: "test", args: ["test"], cwd: dir },
      { label: "test json", args: ["test", "--format", "json"], cwd: dir },
      ...PROGRAMS.map((name) => ({
        label: name,
        args: ["check", "--tests", "--format", "json", `${name}.hd`],
        cwd: programs,
      })),
    ];
    if (await supportsCommand(context.hd, "doc", dir))
      inputs.push({ label: "doc", args: ["doc", "m001.Item1"], cwd: dir });
    context.log(`${NAME}: ${inputs.length} inputs x ${RUNS} runs`);
    const differing: string[] = [];
    for (const input of inputs) {
      const outputs = new Set<string>();
      for (let index = 0; index < RUNS; index++) {
        const result = await runHd(context.hd, input.args, {
          cwd: input.cwd,
          timeoutMs: TIMEOUT_MS,
        });
        outputs.add(
          result.timedOut
            ? "timeout"
            : `${String(result.status)}\n${result.stdout}\n--\n${result.stderr}`,
        );
      }
      if (outputs.size > 1) differing.push(`${input.label} (${outputs.size} variants)`);
    }
    return [
      judgeBool(
        NAME,
        "inputs with byte-identical output",
        differing.length === 0,
        "all identical",
        `${inputs.length - differing.length} of ${inputs.length}`,
        differing.length ? `differ: ${differing.join(", ")}` : undefined,
      ),
    ];
  },
};
