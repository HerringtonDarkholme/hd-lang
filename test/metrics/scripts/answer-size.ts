// answer-size: bytes an agent must read for common answers, on fixed inputs.
//
// Inputs: a small generated package (seeded) whose test case `t001a` fails,
// and the mistake program `length-as-i32.hd`.
// - `hd doc m001.Item1`: one data type with three fields and one trait;
// - `hd test --filter t001a`: the text of one failing test;
// - the `--format json` test object of that failure;
// - the `--format json` diagnostic object of `length-as-i32.hd`.
// Targets (Pillar 1): fixed byte budgets, regression-gated. The budgets are
// this harness's proposal, set from what an agent needs to read, not from
// any implementation's output; see README.md.
// n/a: the doc line, when `hd help doc` fails.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { failingPackage, firstLines } from "../lib/fixture.ts";
import { diagnosticsOf, jsonLines, runHd, supportsCommand } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "answer-size";
const TIMEOUT_MS = 60_000;

export const BUDGETS = {
  doc: 1_000,
  failingTest: 800,
  testRecord: 400,
  diagnosticRecord: 600,
} as const;

const bytes = (text: string): number => Buffer.byteLength(text, "utf8");

export const answerSize: Metric = {
  name: NAME,
  pillar: 1,
  summary: "bytes of hd doc ITEM, one failing hd test, and one --format json record",
  async run(context) {
    const results: TargetResult[] = [];
    const { dir, filter } = failingPackage(NAME);
    const run = (args: string[], cwd = dir) =>
      runHd(context.hd, args, { cwd, timeoutMs: TIMEOUT_MS });

    if (!(await supportsCommand(context.hd, "doc", dir)))
      results.push(notApplicable(NAME, "hd doc of a data type", `≤ ${BUDGETS.doc} B`, "no hd doc"));
    else {
      const doc = await run(["doc", "m001.Item1"]);
      results.push(
        doc.status === 0 && !doc.timedOut
          ? judge(
              NAME,
              "hd doc of a data type",
              bytes(doc.stdout + doc.stderr),
              BUDGETS.doc,
              "bytes",
            )
          : failed(
              NAME,
              "hd doc of a data type",
              `≤ ${BUDGETS.doc} B`,
              `hd doc failed: ${firstLines(doc)}`,
            ),
      );
    }

    const text = await run(["test", "--filter", filter]);
    results.push(
      text.status !== 0 && !text.timedOut
        ? judge(
            NAME,
            "one failing hd test, text",
            bytes(text.stdout + text.stderr),
            BUDGETS.failingTest,
            "bytes",
          )
        : failed(
            NAME,
            "one failing hd test, text",
            `≤ ${BUDGETS.failingTest} B`,
            `expected one failure: ${firstLines(text)}`,
          ),
    );

    const json = await run(["test", "--filter", filter, "--format", "json"]);
    const record = jsonLines(json.stdout).find((r) => r.kind === "test" && r.outcome === "failed");
    results.push(
      record
        ? judge(
            NAME,
            "one failing test, JSON object",
            bytes(JSON.stringify(record)),
            BUDGETS.testRecord,
            "bytes",
          )
        : failed(
            NAME,
            "one failing test, JSON object",
            `≤ ${BUDGETS.testRecord} B`,
            "no failed test object",
          ),
    );

    const mistakeDir = makeTempDir("answer");
    const source = join(context.repoRoot, "test", "metrics", "mistakes", "length-as-i32.hd");
    writeFileSync(join(mistakeDir, "length-as-i32.hd"), readFileSync(source, "utf8"));
    const check = await run(["check", "--format", "json", "length-as-i32.hd"], mistakeDir);
    const diagnostic = diagnosticsOf(check.stdout)[0];
    results.push(
      diagnostic
        ? judge(
            NAME,
            "one diagnostic, JSON object",
            bytes(JSON.stringify(diagnostic)),
            BUDGETS.diagnosticRecord,
            "bytes",
          )
        : failed(
            NAME,
            "one diagnostic, JSON object",
            `≤ ${BUDGETS.diagnosticRecord} B`,
            "no diagnostic object",
          ),
    );
    return results;
  },
};
