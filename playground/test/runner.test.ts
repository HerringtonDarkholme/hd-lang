// Runs the compile-and-run pipeline the worker uses, bundled for the browser
// with the same esbuild options and Node shims as the playground build.

import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, before, test } from "node:test";
import { pathToFileURL } from "node:url";

import * as esbuild from "esbuild";

import { buildOptions } from "../build.ts";
import { createHash as shimHash } from "../src/shims/crypto.ts";
import type { Example } from "../src/examples.ts";
import type * as Runner from "../src/runner.ts";
import type { RunResult } from "../src/runner.ts";

const playground = resolve(import.meta.dirname, "..");
let directory: string;
let runner: typeof Runner;

before(async () => {
  directory = await mkdtemp(join(tmpdir(), "hd-playground-"));
  const outfile = join(directory, "runner.mjs");
  await esbuild.build(
    buildOptions({
      entryPoints: [join(playground, "src", "runner.ts")],
      outdir: undefined,
      outfile,
      minify: false,
      sourcemap: false,
      logLevel: "error",
    }),
  );
  const bundle = await readFile(outfile, "utf8");
  assert.doesNotMatch(bundle, /from\s*"node:(?:fs|crypto)"/, "Node APIs are shimmed");
  runner = (await import(pathToFileURL(outfile).href)) as typeof Runner;
});

after(async () => {
  await rm(directory, { recursive: true, force: true });
});

const single = (source: string) => ({ files: { "src/main.hd": source }, main: "src/main.hd" });

function located(result: RunResult): string[] {
  return result.diagnostics.map(
    ({ path, line, column, code }) => `${path}:${line}:${column}:${code}`,
  );
}

test("hello world compiles and prints", async () => {
  const streamed: string[] = [];
  const result = await runner.runProject(
    single('pub fn main() -> void $ Console:\n    println("hello, world")\n    println(40 + 2)\n'),
    "run",
    (line) => streamed.push(line),
  );
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["hello, world", "42"]);
  assert.deepEqual(streamed, result.stdout);
  assert.equal(result.summary, "exited normally");
});

test("a type error is reported at its line and column", async () => {
  const source = [
    "# a comment line",
    "pub fn main() -> void $ Console:",
    '    let count: i32 = "three"',
    "    println(count)",
  ].join("\n");
  for (const mode of ["run", "check"] as const) {
    const result = await runner.runProject(single(source), mode);
    assert.equal(result.status, "compile-error");
    assert.deepEqual(located(result), ["src/main.hd:3:22:type-mismatch"]);
    assert.match(runner.formatRunDiagnostic(result.diagnostics[0]!), /^src\/main\.hd:3:22: /);
  }
  const checked = await runner.runProject(single("pub fn main() -> void: pass\n"), "check");
  assert.equal(checked.status, "ok");
  assert.deepEqual(checked.stdout, []);
});

test("a runtime panic is reported after the output before it", async () => {
  const result = await runner.runProject(
    single(
      [
        "fn divide(a: i32, b: i32) -> i32: a / b",
        "",
        "pub fn main() -> void $ Console:",
        '    println("before")',
        "    println(divide(1, 0))",
      ].join("\n"),
    ),
    "run",
  );
  assert.equal(result.status, "panic");
  assert.deepEqual(result.stdout, ["before"]);
  assert.match(result.summary, /^integer-division-by-zero: runtime panic in main$/);
});

test("a two-file project with a package use compiles and runs", async () => {
  const project = {
    files: {
      "src/main.hd": [
        "use pkg.models.user.{User, describe}",
        "",
        "pub fn main() -> void $ Console:",
        '    println(describe(User { name: "Ada", age: 36 }))',
      ].join("\n"),
      "src/models/user.hd": [
        "pub data User:",
        "    pub name: string",
        "    pub age: i32",
        "",
        "pub fn describe(user: User) -> string:",
        '    "${user.name} (${user.age})"',
      ].join("\n"),
    },
    main: "src/main.hd",
  };
  const result = await runner.runProject(project, "run");
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["Ada (36)"]);

  const broken = {
    ...project,
    files: {
      ...project.files,
      "src/models/user.hd": project.files["src/models/user.hd"].replace(
        '"${user.name} (${user.age})"',
        "user.age",
      ),
    },
  };
  const failed = await runner.runProject(broken, "check");
  assert.equal(failed.status, "compile-error");
  assert.deepEqual(
    failed.diagnostics.map(({ path, line }) => `${path}:${line}`),
    ["src/models/user.hd:6"],
  );
  const missing = await runner.runProject(
    { ...project, files: { "src/main.hd": project.files["src/main.hd"] } },
    "check",
  );
  assert.deepEqual(located(missing), ["src/main.hd:1:1:unknown-module"]);
});

test("test blocks run when there is no entry point", async () => {
  const result = await runner.runProject(
    single(
      [
        "use std.testing.assert_equal",
        "",
        'test "adds":',
        '    assert_equal(1 + 1, 2, reason="sum")',
        "",
        'test "fails":',
        '    assert_equal(1 + 1, 3, reason="wrong")',
      ].join("\n"),
    ),
    "run",
  );
  assert.equal(result.status, "panic");
  assert.equal(result.summary, 'assertion-failed: runtime panic in test "fails"');
});

test("the bundled examples run", async () => {
  const { EXAMPLES } = (await import(pathToFileURL(await bundleExamples()).href)) as {
    EXAMPLES: readonly Example[];
  };
  const expected: Record<string, RunResult["status"]> = { panic: "panic" };
  for (const example of EXAMPLES) {
    const result = await runner.runProject(example.project, "run");
    assert.equal(result.status, expected[example.id] ?? "ok", `${example.id}: ${result.summary}`);
  }
});

async function bundleExamples(): Promise<string> {
  const outfile = join(directory, "examples.mjs");
  await esbuild.build(
    buildOptions({
      entryPoints: [join(playground, "src", "examples.ts")],
      outdir: undefined,
      outfile,
      minify: false,
      sourcemap: false,
      logLevel: "error",
    }),
  );
  return outfile;
}

test("the crypto shim matches node:crypto", async () => {
  const { createHash } = await import("node:crypto");
  for (const text of ["", "abc", "fn main() -> i32: 1", "λ".repeat(100), "x".repeat(1000)])
    assert.equal(
      shimHash("sha256").update(text).digest("hex"),
      createHash("sha256").update(text).digest("hex"),
    );
});
