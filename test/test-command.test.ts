import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { runHd } from "./hd-in-process.ts";

// `hd test` result lines, --filter, and --deny-skipped
// (spec/cli/command-line.md#test-runs).

const PASSING =
  'use std.testing.assert_equal\n\nit("fine"):\n    assert_equal(+1, +1, reason="ok")\n';
const FAILING = [
  "use std.testing.assert_equal",
  "",
  'it("sums prices"):',
  '    assert_equal(+1, +1, reason="ok")',
  "",
  'it("boom"):',
  '    assert_equal(+1, +2, reason="boom")',
  "",
  'it("never runs"):',
  '    assert_equal(+1, +1, reason="ok")',
  "",
].join("\n");

async function withPackage(run: (directory: string) => Promise<void>): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), "hd-test-command-"));
  try {
    await writeFile(join(directory, "hd.toml"), '[package]\nname = "pkg"\n');
    await mkdir(join(directory, "src"));
    await writeFile(join(directory, "src/lib.hd"), "pub fn one() -> i32:\n    +1\n");
    await mkdir(join(directory, "tests"));
    await writeFile(join(directory, "tests/good.hd"), PASSING);
    await writeFile(join(directory, "tests/bad.hd"), FAILING);
    await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("a failing test file prints a result line with its counts", async () => {
  await withPackage(async (directory) => {
    const ran = await runHd(["test"], { cwd: directory });
    assert.equal(ran.status, 1);
    assert.match(ran.stderr + ran.stdout, /assertion-failed: boom/);
    assert.match(ran.stdout, /^tests\/bad\.hd: 1 passed, 1 failed$/m);
    assert.match(ran.stdout, /^tests\/good\.hd: 1 passed$/m);
  });
});

test("--filter runs only the test cases whose name contains the pattern", async () => {
  await withPackage(async (directory) => {
    const ran = await runHd(["test", "--filter", "sums"], { cwd: directory });
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "tests/bad.hd: 1 passed\n");
    const file = await runHd(["test", "tests/good.hd", "--filter", "fi"], { cwd: directory });
    assert.equal(file.status, 0, file.stderr);
    assert.equal(file.stdout, "tests/good.hd: 1 passed\n");
  });
});

test("--filter matching nothing in a named FILE is an error, but not in a package", async () => {
  await withPackage(async (directory) => {
    const file = await runHd(["test", "tests/good.hd", "--filter", "zzz"], { cwd: directory });
    assert.equal(file.status, 101);
    assert.match(file.stderr, /tests\/good\.hd: no test case has a name containing 'zzz'/);
    const whole = await runHd(["test", "--filter", "zzz"], { cwd: directory });
    assert.equal(whole.status, 0, whole.stderr);
    assert.equal(whole.stdout, "");
  });
});

test("a workspace manifest says workspaces are not supported yet", async () => {
  await withPackage(async (directory) => {
    await writeFile(join(directory, "hd.toml"), '[workspace]\nmembers = ["a"]\n');
    for (const command of ["check", "build", "test", "run", "fetch"]) {
      const ran = await runHd([command], { cwd: directory });
      assert.equal(ran.status, 101);
      assert.match(ran.stderr, /does not support workspaces yet/);
    }
    // A directory word no longer points at -p.
    const word = await runHd(["check", "src"], { cwd: directory });
    assert.doesNotMatch(word.stderr, /-p NAME|workspace member/);
  });
});

test("a FILE that does not exist is an error, not a crash", async () => {
  await withPackage(async (directory) => {
    for (const command of ["check", "test", "build"]) {
      const ran = await runHd([command, "nosuch.hd"], { cwd: directory });
      assert.equal(ran.status, 101);
      assert.match(ran.stderr, /^hd: cannot read nosuch\.hd: no such file$/m);
    }
  });
});

test("--deny-skipped passes when no test case is skipped", async () => {
  await withPackage(async (directory) => {
    const ran = await runHd(["test", "tests/good.hd", "--deny-skipped"], { cwd: directory });
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "tests/good.hd: 1 passed\n");
  });
});
