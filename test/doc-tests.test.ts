import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { docTests } from "../src/doc-tests.ts";
import { runHd } from "./hd-in-process.ts";

// Doc tests (spec/lang/10-modules.md#doc-tests) in `hd test` and
// `hd check --tests` (spec/cli/command-line.md#test-runs).

const LIB = [
  "## Prices in cents.",
  "##",
  "## ```hd",
  "## use pkg.{total}",
  "## use std.testing.assert_equal",
  "##",
  '## assert_equal(total([100, 250]), 350, reason="sums the prices")',
  "## ```",
  "pub fn total(prices: List[i32]) -> i32:",
  "    let sum = +0",
  "    for price in prices:",
  "        sum += price",
  "    sum",
  "",
].join("\n");

const TEXT = [
  "## Text helpers.",
  "##",
  "## ```hd",
  "## use pkg.text.{slugify}",
  "## use std.testing.snapshot",
  "##",
  '## snapshot(slugify("Ship It"))',
  "## ```",
  "",
  "## Turns a title into a slug.",
  "##",
  "## ```hd",
  "## use pkg.text.{slugify}",
  "## use std.testing.assert_equal",
  "##",
  '## assert_equal(slugify("a b"), "a_b", reason="a stale example")',
  "## ```",
  "pub fn slugify(title: string) -> string:",
  '    title.replace(" ", "-")',
  "",
  "pub data Slug:",
  "    text: string",
  "",
  "impl Slug:",
  "    ## Its text is private.",
  "    ##",
  "    ## ```hd",
  "    ## use pkg.text.{Slug}",
  "    ##",
  '    ## _ := Slug::new("a").text  # error: private-member',
  "    ## ```",
  "    pub fn new(title: string) -> Slug:",
  "        Slug { text: slugify(title) }",
  "",
].join("\n");

async function withPackage(run: (directory: string) => Promise<void>): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), "hd-doc-tests-"));
  try {
    await writeFile(join(directory, "hd.toml"), '[package]\nname = "shop"\n');
    await mkdir(join(directory, "src"));
    await writeFile(join(directory, "src/lib.hd"), LIB);
    await writeFile(join(directory, "src/text.hd"), TEXT);
    await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("doc tests are named by module, item, member, and index", () => {
  assert.deepEqual(
    docTests(TEXT, "text").map(({ name, line, errors }) => ({ name, line, errors })),
    [
      { name: "doc text[0]", line: 3, errors: [] },
      { name: "doc text.slugify[0]", line: 12, errors: [] },
      { name: "doc text.Slug.new[0]", line: 27, errors: ["private-member"] },
    ],
  );
  assert.deepEqual(
    docTests(LIB, "pkg").map(({ name }) => name),
    ["doc pkg.total[0]"],
  );
});

test("hd test runs doc tests and names a failure's ## line", async () => {
  await withPackage(async (directory) => {
    const ran = await runHd(["test", "--format", "json"], { cwd: directory });
    assert.equal(ran.status, 1, ran.stderr);
    const records = ran.stdout
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line) as { kind: string; name?: string; outcome?: string });
    assert.deepEqual(
      records.filter(({ kind }) => kind === "test").map(({ name, outcome }) => [name, outcome]),
      [
        ["doc pkg.total[0]", "passed"],
        ["doc text[0]", "failed"],
        ["doc text.slugify[0]", "failed"],
        ["doc text.Slug.new[0]", "passed"],
      ],
    );
    const text = await runHd(["test", "src/text.hd", "--filter", "slugify"], { cwd: directory });
    assert.equal(text.status, 1);
    // The panic names the `##` line of the call that failed (flow.panic.report).
    assert.match(text.stderr, /^src\/text\.hd:16:4: assertion-failed: a stale example/m);
    assert.match(text.stdout, /^src\/text\.hd: 0 passed, 1 failed$/m);
  });
});

test("--filter doc selects the doc tests, and a FILE of doc tests alone is not empty", async () => {
  await withPackage(async (directory) => {
    const lib = await runHd(["test", "src/lib.hd"], { cwd: directory });
    assert.equal(lib.status, 0, lib.stderr);
    assert.equal(lib.stdout, "src/lib.hd: 1 passed\n");
    const filtered = await runHd(["test", "--filter", "doc pkg"], { cwd: directory });
    assert.equal(filtered.status, 0, filtered.stderr);
    assert.equal(filtered.stdout, "src/lib.hd: 1 passed\n");
  });
});

test("a doc test's compile error names its ## line, in text and in JSON", async () => {
  await withPackage(async (directory) => {
    await writeFile(join(directory, "src/text.hd"), TEXT.replace('slugify("a b")', "slug(1)"));
    const ran = await runHd(["check", "--tests", "--format", "json"], { cwd: directory });
    assert.equal(ran.status, 101);
    const diagnostic = JSON.parse(ran.stdout.split("\n")[0]!) as Record<string, unknown>;
    assert.equal(diagnostic.file, "src/text.hd");
    assert.equal(diagnostic.line, 16);
    assert.equal(diagnostic.column, 17);
  });
});

test("hd test --update rewrites a failing doc test snapshot inside its ## lines", async () => {
  await withPackage(async (directory) => {
    const ran = await runHd(["test", "src/text.hd", "--update", "--filter", "doc text["], {
      cwd: directory,
    });
    assert.equal(ran.status, 0, ran.stderr);
    const text = await readFile(join(directory, "src/text.hd"), "utf8");
    assert.match(text, /^## snapshot\(slugify\("Ship It"\), expect="Ship-It"\)$/m);
    const again = await runHd(["test", "src/text.hd", "--filter", "doc text["], { cwd: directory });
    assert.equal(again.status, 0, again.stderr);
  });
});
