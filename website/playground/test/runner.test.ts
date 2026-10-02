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
import type { RunMode, RunResult } from "../src/runner.ts";

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

function located(result: Pick<RunResult, "diagnostics">): string[] {
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

test("test cases run when there is no entry point", async () => {
  const result = await runner.runProject(
    single(
      [
        "use std.testing.assert_equal",
        "",
        "tests:",
        '    it("adds"):',
        '        assert_equal(1 + 1, 2, reason="sum")',
        "",
        '    it("fails"):',
        '        assert_equal(1 + 1, 3, reason="wrong")',
      ].join("\n"),
    ),
    "run",
  );
  assert.equal(result.status, "panic");
  assert.equal(result.summary, 'assertion-failed: runtime panic in test case "fails"');
});

test("without main, Run evaluates top-level inputs with REPL semantics", async () => {
  const source = [
    "# no main: each top-level input runs in order",
    "x := 21",
    "",
    "fn double(n: i32) -> i32: n * 2",
    'println("hi")',
    "double(x)",
    "if x > 1:",
    '    println("big")',
    "else:",
    '    println("small")',
    '[1, 2].len() == 2 && "a" != "b"',
  ].join("\n");
  const streamed: string[] = [];
  const result = await runner.runProject(single(source), "run", (line) => streamed.push(line));
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["hi", "42 : i32", "big", "true : bool"]);
  assert.deepEqual(streamed, result.stdout);
  assert.equal(result.summary, "ran 6 top-level inputs");

  const checked = await runner.runProject(single(source), "check");
  assert.equal(checked.status, "ok", checked.summary);
  assert.deepEqual(checked.stdout, []);
});

test("without main, errors and panics point at the file's lines", async () => {
  const unknown = await runner.runProject(single('x := 1\nprintln("ok")\ny := missing\n'), "run");
  assert.equal(unknown.status, "compile-error");
  assert.deepEqual(unknown.stdout, ["ok"]);
  assert.deepEqual(located(unknown), ["src/main.hd:3:6:unknown-name"]);
  const checked = await runner.runProject(single("x := 1\ny := x + true\n"), "check");
  assert.deepEqual(located(checked).length, 1);
  assert.match(located(checked)[0]!, /^src\/main\.hd:2:/);

  const panic = await runner.runProject(single('println("before")\n\n1 / 0\n'), "run");
  assert.equal(panic.status, "panic");
  assert.deepEqual(panic.stdout, ["before"]);
  assert.equal(panic.summary, "integer-division-by-zero: runtime panic at src/main.hd:3");
});

test("without main, a multi-file project evaluates the entry module", async () => {
  const project = {
    files: {
      "src/main.hd": [
        "use pkg.models.user.{User, describe}",
        "",
        'ada := User { name: "Ada", age: 36 }',
        "describe(ada)",
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
  assert.deepEqual(result.stdout, ['"Ada (36)" : string']);
});

test("Test runs the test cases, with or without main", async () => {
  const source = [
    "use std.testing.assert_equal",
    "",
    "pub fn main() -> void $ Console:",
    '    println("main runs only for Run")',
    "",
    "tests:",
    '    it("adds"):',
    '        assert_equal(1 + 1, 2, reason="sum")',
  ].join("\n");
  const tested = await runner.runProject(single(source), "test");
  assert.equal(tested.status, "ok", tested.summary);
  assert.equal(tested.summary, "1 test passed");
  assert.deepEqual(tested.stdout, []);
  const ran = await runner.runProject(single(source), "run");
  assert.deepEqual(ran.stdout, ["main runs only for Run"]);
  const none = await runner.runProject(single("pub fn main() -> void: pass\n"), "test");
  assert.equal(none.status, "failure");
  assert.equal(
    none.summary,
    "nothing to test: add a `tests:` block or a `_test.hd` module with `it(...)` test cases",
  );
});

test("Test judges it_each rows, expected panics, and ignored cases as hd test does", async () => {
  const test = async (lines: readonly string[]) => {
    const result = await runner.runProject(single(lines.join("\n")), "test");
    return `${result.status}: ${result.summary}`;
  };
  const header = ["use std.testing.{assert, it_each}", "", "tests:"];
  assert.equal(
    await test([
      ...header,
      '    it_each("positive", [1, 2, 3], body=fn!(value: i32):',
      '        assert(value > 0, reason="rows are positive")',
      "    )",
      '    it("indexes past the end", expect_panic="index-out-of-bounds"):',
      "        _ := [1][1]",
      '    it("waits", ignore="slow"):',
      "        pass",
    ]),
    "ok: 4 tests passed, 1 ignored",
  );
  assert.equal(
    await test([
      ...header,
      '    it_each("small", [1, 5], body=fn!(value: i32):',
      '        assert(value < 3, reason="rows are small")',
      "    )",
    ]),
    'panic: assertion-failed: runtime panic in test case "small"',
  );
  assert.equal(
    await test([
      ...header,
      '    it("panics", expect_panic="index-out-of-bounds"):',
      "        pass",
    ]),
    'failure: test "panics" expecting panic index-out-of-bounds failed',
  );
});

// A `*_test.hd` file is a test module: Test links it and runs its top-level
// test cases; Run leaves it out (spec/lang/10-modules.md#test-modules).
test("Test runs the test cases of _test.hd modules", async () => {
  const files = {
    "src/main.hd": [
      "use pkg.billing.{late_fee}",
      "",
      "pub fn main() -> void $ Console:",
      "    println(late_fee(31))",
      "",
      "tests:",
      '    it("is a same-file test"):',
      "        pass",
    ].join("\n"),
    "src/billing.hd": ["pub fn late_fee(days: i32) -> i32:", "    if days > 30: 5 else: 0"].join(
      "\n",
    ),
    "src/billing_test.hd": [
      "use pkg.billing.{late_fee}",
      "use pkg.helpers_test.{overdue}",
      "use std.testing.assert_equal",
      "",
      'it("charges a fee after 30 days"):',
      '    assert_equal(late_fee(overdue()), 5, reason="one day late")',
    ].join("\n"),
    "src/helpers_test.hd": ["pub fn overdue() -> i32: 31"].join("\n"),
  };
  const project = { files, main: "src/main.hd" };
  const tested = await runner.runProject(project, "test");
  assert.equal(tested.summary, "2 tests passed");
  const ran = await runner.runProject(project, "run");
  assert.deepEqual([ran.summary, ...ran.stdout], ["exited normally", "5"]);

  const failing = await runner.runProject(
    {
      ...project,
      files: {
        ...files,
        "src/billing_test.hd": files["src/billing_test.hd"].replace("), 5,", "), 4,"),
      },
    },
    "test",
  );
  assert.equal(
    failing.summary,
    'assertion-failed: runtime panic in test case "charges a fee after 30 days"',
  );
  const misplaced = await runner.runProject(
    {
      ...project,
      files: { ...files, "src/helpers_test.hd": 'tests:\n    it("x"):\n        pass\n' },
    },
    "test",
  );
  assert.deepEqual(located(misplaced), ["src/helpers_test.hd:1:1:misplaced-tests-block"]);
  const leaked = await runner.runProject(
    {
      ...project,
      files: {
        ...files,
        "src/billing.hd": `use pkg.helpers_test.{overdue}\n${files["src/billing.hd"]}`,
      },
    },
    "run",
  );
  assert.deepEqual(located(leaked), ["src/billing.hd:1:1:test-only-use"]);
  const mistyped = await runner.runProject(
    {
      ...project,
      files: { ...files, "src/helpers_test.hd": 'pub fn overdue() -> i32: "31"\n' },
    },
    "test",
  );
  assert.deepEqual(located(mistyped), ["src/helpers_test.hd:1:26:type-mismatch"]);
});

// A suspending `main!` is judged by `report()` on its result, as `main` is
// (spec/lang/10-modules.md#exit-status).
test("a suspending main! reports its Result", async () => {
  const run = async (body: string): Promise<string> => {
    const result = await runner.runProject(
      single(`pub fn main!() -> Result[void, string]:\n    ${body}\n`),
      "run",
    );
    return `${result.status}: ${result.summary}`;
  };
  assert.equal(await run(".Ok(())"), "ok: exited normally");
  assert.equal(await run('.Err("boom")'), "failure: main returned Err");
});

test("the bundled examples run", async () => {
  const { EXAMPLES } = (await import(pathToFileURL(await bundleExamples()).href)) as {
    EXAMPLES: readonly Example[];
  };
  const expected: Record<string, RunResult["status"]> = { panic: "panic", "exit-code": "failure" };
  for (const example of EXAMPLES) {
    const result = await runner.runProject(example.project, "run");
    assert.equal(result.status, expected[example.id] ?? "ok", `${example.id}: ${result.summary}`);
  }
  const outcome = async (id: string, mode: RunMode) => {
    const result = await runner.runProject(
      EXAMPLES.find((example) => example.id === id)!.project,
      mode,
    );
    return [result.summary, ...result.stdout];
  };
  assert.deepEqual(await outcome("exit-code", "run"), [
    "main exited with code 1",
    "2 files changed",
  ]);
  assert.deepEqual(await outcome("tests", "test"), ["5 tests passed"]);
  assert.deepEqual(await outcome("std", "run"), [
    "exited normally",
    "skipped 'bob x'",
    "CY, ADA",
    "best: 41",
    "cutoff: 40",
  ]);
  assert.deepEqual(await outcome("derive", "run"), [
    "exited normally",
    "field x",
    "field y",
    "true",
  ]);
  const topLevel = EXAMPLES.find(({ id }) => id === "top-level")!;
  assert.deepEqual((await runner.runProject(topLevel.project, "run")).stdout, [
    "7 : i32",
    "Point { x: 3, y: 4 } : Point",
    "[3, 4] : List[i32]",
    "added 1",
    "added 2",
    "added 3",
    "60 : i32",
  ]);
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

test("the WAT of a program is the module Run instantiates", async () => {
  const project = single('pub fn main() -> void $ Console:\n    println("hi")\n');
  const shown = runner.watProject(project);
  assert.equal(shown.status, "ok", shown.summary);
  assert.equal(shown.module?.origin, "program");
  assert.match(shown.module!.wat, /^\(module\n/);
  assert.match(shown.module!.wat, /\(func \(export "main"\) \(param \$provider0 externref\)/);
  const modules: Runner.CompiledModule[] = [];
  const ran = await runner.runProject(project, "run", undefined, (module) => modules.push(module));
  assert.equal(ran.status, "ok", ran.summary);
  assert.deepEqual(modules, [shown.module]);
  assert.deepEqual(runner.watFromRun(ran, modules[0]), { ...shown, diagnostics: ran.diagnostics });
  const checked: Runner.CompiledModule[] = [];
  await runner.runProject(project, "check", undefined, (module) => checked.push(module));
  assert.deepEqual(checked, [], "Check compiles no module");
});

test("the WAT view shows diagnostics when compilation fails", async () => {
  const broken = single('pub fn main() -> void:\n    let x: i32 = "no"\n');
  const shown = runner.watProject(broken);
  assert.equal(shown.status, "compile-error");
  assert.equal(shown.module, undefined);
  assert.deepEqual(located(shown), ["src/main.hd:2:18:type-mismatch"]);
  const ran = await runner.runProject(broken, "run");
  assert.equal(runner.watFromRun(ran, undefined)?.status, "compile-error");
  const unlinked = runner.watProject({
    files: { "src/main.hd": "use pkg.gone.{X}\n" },
    main: "src/main.hd",
  });
  assert.equal(unlinked.status, "compile-error");
});

test("without main, the WAT is the last module Run compiled", async () => {
  const project = single('x := 20\nprintln("start")\nx + 1\nx * 2\n');
  assert.equal(runner.watProject(project).status, "not-run");
  const modules: Runner.CompiledModule[] = [];
  const ran = await runner.runProject(project, "run", undefined, (module) => modules.push(module));
  assert.equal(ran.status, "ok", ran.summary);
  assert.equal(modules.length, 1);
  const [module] = modules;
  assert.equal(module!.origin, "top-level");
  assert.equal(module!.count, 4, "each of the four inputs compiles a module");
  assert.match(module!.wat, /\(export "main"\)/);
  const declarationsOnly = runner.watProject(
    single('fn f() -> i32: 1\ntests:\n    it("t"):\n        pass\n'),
  );
  assert.equal(declarationsOnly.status, "ok", "declarations with tests compile as one module");
});

test("the crypto shim matches node:crypto", async () => {
  const { createHash } = await import("node:crypto");
  for (const text of ["", "abc", "fn main() -> i32: 1", "λ".repeat(100), "x".repeat(1000)])
    assert.equal(
      shimHash("sha256").update(text).digest("hex"),
      createHash("sha256").update(text).digest("hex"),
    );
});
