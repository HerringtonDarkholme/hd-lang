import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { runHd } from "./hd-in-process.ts";

// `hd test` result lines and --filter
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

test("a workspace member without a package, and a dependency command at the root, are errors", async () => {
  await withPackage(async (directory) => {
    await writeFile(join(directory, "hd.toml"), '[workspace]\nmembers = ["a"]\n');
    // Workspace mode acts on every member (cli.workspace.members), so a
    // listed directory with no package is an error that names it.
    for (const command of ["check", "build", "test", "run"]) {
      const ran = await runHd([command], { cwd: directory });
      assert.equal(ran.status, 101);
      assert.match(ran.stderr, /the member 'a' that .*hd\.toml lists holds no package's hd\.toml/);
    }
    // hd fetch works on the whole workspace (cli.dep.workspace-fetch), so a
    // member with no package is an error; the other dependency commands
    // work in one member (cli.dep.workspace-member-only).
    const fetched = await runHd(["fetch"], { cwd: directory });
    assert.equal(fetched.status, 101);
    assert.match(fetched.stderr, /a holds no hd\.toml/);
    const added = await runHd(["remove", "x"], { cwd: directory });
    assert.equal(added.status, 101);
    assert.match(
      added.stderr,
      /is a workspace manifest, and hd remove changes one member's requirements/,
    );
    // A directory word suggests -p (cli.command.positional).
    const word = await runHd(["test", "src"], { cwd: directory });
    assert.match(word.stderr, /'src' is a directory.*pass -p NAME/);
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

// An integration test case must explicitly bind a trait the profile does not
// bind (spec/lang/10-modules.md#r-module.testing.integration-row.unbound).
const NEEDS_REPO = [
  "trait Repo:",
  "    fn count(self) -> i32",
  "",
  "fn total() -> i32 $ Repo:",
  "    $.use(Repo).count()",
  "",
  'it("needs a repo"):',
  "    _ := total()",
  "",
].join("\n");

test("an integration test must bind a trait missing from its profile", async () => {
  await withPackage(async (directory) => {
    await writeFile(join(directory, "tests/repo.hd"), NEEDS_REPO);
    const ran = await runHd(["test", "tests/repo.hd"], { cwd: directory });
    assert.equal(ran.status, 101);
    assert.match(ran.stderr, /missing-requirement: call to 'total' requires Repo/);
    assert.match(ran.stderr, /bind it with `\$\.with\(Repo=\.\.\.\)`/);
    const json = await runHd(["test", "tests/repo.hd", "--format", "json"], { cwd: directory });
    assert.equal(json.status, 101, json.stderr);
    assert.match(json.stdout, /"code":"missing-requirement"/);
    assert.doesNotMatch(json.stdout, /"kind":"test"/);
    assert.doesNotMatch(json.stdout, /"skipped"/);
  });
});

test("a doc test must bind a trait missing from its profile", async () => {
  await withPackage(async (directory) => {
    await writeFile(
      join(directory, "src/lib.hd"),
      [
        "pub trait Repo:",
        "    fn count(self) -> i32",
        "",
        "## Counts repository entries.",
        "##",
        "## ```hd",
        "## use pkg.{total}",
        "##",
        "## _ := total()",
        "## ```",
        "pub fn total() -> i32 $ Repo:",
        "    $.use(Repo).count()",
        "",
      ].join("\n"),
    );
    const ran = await runHd(["test", "src/lib.hd"], { cwd: directory });
    assert.equal(ran.status, 101);
    assert.match(ran.stderr, /missing-requirement: call to 'total' requires Repo/);
    assert.match(ran.stderr, /bind it with `\$\.with\(Repo=\.\.\.\)`/);
  });
});

// Each run of a test body gets its own temporary directory, which the
// runner removes once the run ends, pass or fail
// (spec/cli/command-line.md#r-cli.test.env.temp-dir,
// spec/cli/command-line.md#r-cli.test.env.temp-dir.removed).
const TEMP_DIRS = [
  "use std.fs.write_text",
  "use std.path.Path",
  "use std.testing.{assert, temp_dir}",
  "",
  'it("writes a file"):',
  "    dir := temp_dir()",
  '    assert(temp_dir() == dir, reason="one directory per run")',
  '    write_text!(Path("${dir}/out.txt"), "x").expect("the file")',
  '    println("dir ${dir}")',
  "",
  'it("writes another file"):',
  '    println("dir ${temp_dir()}")',
  "",
  'it("fails after writing"):',
  "    dir := temp_dir()",
  '    write_text!(Path("${dir}/out.txt"), "x").expect("the file")',
  '    println("dir ${dir}")',
  '    assert(false, reason="a failure")',
  "",
].join("\n");

test("each run gets a fresh temporary directory, removed when it ends", async () => {
  await withPackage(async (directory) => {
    await writeFile(join(directory, "tests/dirs.hd"), TEMP_DIRS);
    const ran = await runHd(["test", "--format", "json", "tests/dirs.hd"], { cwd: directory });
    assert.equal(ran.status, 1, ran.stderr);
    // With `--format json`, stdout holds only JSON lines
    // (spec/cli/command-line.md#r-cli.json.lines.build), so the test bodies'
    // console output goes to stderr.
    const dirs = [...ran.stderr.matchAll(/^dir (.+)$/gm)].map((match) => match[1]!);
    assert.equal(dirs.length, 3, ran.stderr);
    for (const line of ran.stdout.trim().split("\n")) JSON.parse(line);
    assert.equal(new Set(dirs).size, 3, "no two runs share a directory");
    for (const dir of dirs) assert.equal(existsSync(dir), false, `${dir} is removed`);
  });
});

// A unit test case's missing host trait names its std fake and the test
// root (spec/std/testing.md#r-std-testing.unit.hint); a tested script's top
// level is requirement-free (spec/lang/10-modules.md#r-module.init.tests.requirement-free).
test("a unit test's missing host trait names its fake and tests/", async () => {
  await withPackage(async (directory) => {
    await writeFile(
      join(directory, "src/late.hd"),
      [
        "use std.time.{Clock, now}",
        "",
        "fn stamp() -> i64 $ Clock:",
        "    now().unix_milliseconds()",
        "",
        "tests:",
        '    it("stamps"):',
        "        _ := stamp()",
        "",
      ].join("\n"),
    );
    const ran = await runHd(["test", "src/late.hd"], { cwd: directory });
    assert.equal(ran.status, 101);
    assert.match(ran.stderr + ran.stdout, /missing-requirement: call to 'stamp' requires Clock/);
    assert.match(
      ran.stderr + ran.stdout,
      /\$\.with\(Clock=ManualClock::new\(start\)\) from std\.time/,
    );
    assert.match(ran.stderr + ran.stdout, /move the test case to tests\//);
  });
});

test("a tested script's top level must be requirement-free, with a hint to use main", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-test-command-"));
  try {
    const file = join(directory, "seed.hd");
    await writeFile(file, 'println("seeded")\n\ntests:\n    it("runs"):\n        pass\n');
    const ran = await runHd(["test", file], { cwd: directory });
    assert.equal(ran.status, 101);
    assert.match(
      ran.stderr + ran.stdout,
      /missing-requirement: call to 'println' requires Console/,
    );
    assert.match(ran.stderr + ran.stdout, /move the script's work into `main`/);
    const run = await runHd([file], { cwd: directory });
    assert.equal(run.status, 0, run.stderr);
    assert.equal(run.stdout, "seeded\n");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
