// Integration tests of the metric harness: they start real processes, so
// they stay out of `pnpm test`, whose glob takes only test/metrics/*.test.ts.
// Run them with:
//   node --test --experimental-strip-types 'test/metrics/integration/*.test.ts'

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { runSession } from "../lib/hd.ts";

describe("paced sessions", () => {
  it("samples a paced session after each step", async () => {
    // A stand-in REPL: echoes each line it reads, upper-cased.
    const echo = [
      process.execPath,
      "-e",
      'require("readline").createInterface({ input: process.stdin }).on("line", (l) => console.log(l.toUpperCase()))',
    ];
    const steps = ["a", "b"].map((word) => ({
      input: `${word}\n`,
      done: (stdout: string) => stdout.includes(word.toUpperCase()),
    }));
    const result = await runSession(echo, { cwd: process.cwd(), timeoutMs: 20_000, steps });
    assert.equal(result.problem, undefined);
    assert.equal(result.rssBytes.length, 2);
    assert.ok(result.rssBytes.every((value) => value !== undefined && value > 0));
    const stuck = await runSession(echo, {
      cwd: process.cwd(),
      timeoutMs: 1_000,
      steps: [{ input: "a\n", done: () => false }],
    });
    assert.match(stuck.problem ?? "", /timed out .* step 1 of 1/);
  });
});
