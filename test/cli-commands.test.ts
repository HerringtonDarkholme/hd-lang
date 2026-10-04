import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { main } from "../src/cli.ts";
import { bufferedIo } from "../src/commands/index.ts";
import { hd as hdInProcess } from "./hd-in-process.ts";

// The shape of the `hd` command line (src/cli-args.ts): help output, the
// flags each command owns, `hd debug`, and `hd test` on a directory.

interface CommandResult {
  readonly stdout: string;
  readonly stderr: string;
  readonly code?: number;
}

const root = resolve(import.meta.dirname, "..");
const core = resolve(root, "examples/core.hd");

/** Runs `hd ARGS...` in this process, as if started in `cwd`. */
function hd(args: readonly string[], cwd = root): Promise<CommandResult> {
  return hdInProcess(args, { cwd });
}

/** Runs `hd` expecting exit status 101 (a rejected command line), and returns its output. */
async function usageError(args: readonly string[]): Promise<CommandResult> {
  let failure: CommandResult | undefined;
  await assert.rejects(hd(args), (error: CommandResult) => {
    failure = error;
    return true;
  });
  assert.equal(failure!.code, 101, `hd ${args.join(" ")} exits 101`);
  assert.equal(failure!.stdout, "");
  return failure!;
}

test("hd, hd --help, and hd help print the same short command list", async () => {
  const outputs = await Promise.all([hd([]), hd(["--help"]), hd(["help"])]);
  const [overview] = outputs;
  for (const output of outputs) assert.equal(output.stdout, overview!.stdout);
  for (const command of ["build", "run", "test", "check", "explain", "doc", "def", "repl"])
    assert.match(overview!.stdout, new RegExp(`^  ${command} +\\S`, "m"));
  assert.match(overview!.stdout, /^ {2}debug +\S/m);
  assert.match(overview!.stdout, /^ {2}--format FORMAT/m);
  // The overview lists commands, not each command's flags.
  for (const flag of ["--wat", "--entry", "--seed", "--tests", "--profile"])
    assert.doesNotMatch(overview!.stdout, new RegExp(flag));
  // `parse` is the conformance runner's spelling of `hd debug parse`.
  assert.doesNotMatch(overview!.stdout, /^ {2}parse/m);
});

test("hd help COMMAND lists only that command's flags", async () => {
  const flagsOf = (text: string): string[] =>
    [...text.matchAll(/^ {2}(--[a-z-]+)/gm)].map((match) => match[1]!);
  const build = (await hd(["help", "build"])).stdout;
  assert.match(build, /^usage: hd build \[--wat\] FILE$/m);
  assert.deepEqual(flagsOf(build), ["--wat", "--format", "--profile"]);

  const run = (await hd(["help", "run"])).stdout;
  assert.match(run, /^usage: hd run \[--entry NAME\] FILE$/m);
  assert.deepEqual(flagsOf(run), ["--entry", "--format", "--profile"]);

  const tested = (await hd(["help", "test"])).stdout;
  assert.match(
    tested,
    /^usage: hd test \[--update\] \[--seed N\] \[--cases N\] \[--shrink N\] \[FILE\|DIR\]$/m,
  );
  assert.deepEqual(flagsOf(tested), [
    "--update",
    "--seed",
    "--cases",
    "--shrink",
    "--format",
    "--profile",
    "--scenario",
    "--pending-function",
    "--test-layout",
    "--package-tree",
    "--package-path",
  ]);
  assert.match(tested, /--package-path are temporary/);

  const check = (await hd(["help", "check"])).stdout;
  assert.match(check, /^usage: hd check \[--tests\] FILE$/m);
  assert.deepEqual(flagsOf(check), [
    "--tests",
    "--format",
    "--profile",
    "--test-layout",
    "--package-tree",
    "--package-path",
  ]);

  assert.deepEqual(flagsOf((await hd(["help", "explain"])).stdout), ["--format"]);
  assert.deepEqual(flagsOf((await hd(["help", "repl"])).stdout), []);
  assert.equal((await hd(["run", "--help"])).stdout, run);
  assert.match((await hd(["help", "debug"])).stdout, /^ {2}hir FILE +print FILE's checked HIR/m);
  assert.deepEqual(flagsOf((await hd(["help", "debug", "hir"])).stdout), ["--format", "--profile"]);

  const unknown = await usageError(["help", "nope"]);
  assert.match(unknown.stderr, /^hd help: no command 'nope'$/m);
});

test("a flag given to the wrong command names the commands that accept it", async () => {
  const cases: [string[], string][] = [
    [["run", "--wat", core], "hd run: --wat is not a flag of hd run; hd build accepts it"],
    [
      ["build", "--entry", "main", core],
      "hd build: --entry is not a flag of hd build; hd run accepts it",
    ],
    [
      ["check", "--seed", "3", core],
      "hd check: --seed is not a flag of hd check; hd test accepts it",
    ],
    [["test", "--tests", core], "hd test: --tests is not a flag of hd test; hd check accepts it"],
    [
      ["explain", "--profile", "ready-gate", "x"],
      "hd explain: --profile is not a flag of hd explain; hd build, hd run, hd test, hd check, and hd debug hir accept it",
    ],
    [["run", "--bogus", core], "hd run: unknown flag --bogus"],
  ];
  for (const [args, message] of cases) {
    const { stderr } = await usageError(args);
    const [first, second] = stderr.split("\n");
    assert.equal(first, message);
    assert.equal(second, `Run 'hd help ${args[0]}' for its flags.`);
  }
  assert.match(
    (await usageError(["test", "--seed", "x", core])).stderr,
    /--seed needs a non-negative integer/,
  );
  assert.match(
    (await usageError(["check", "--format", "yaml", core])).stderr,
    /--format must be one of text, json/,
  );
  assert.match((await usageError(["build"])).stderr, /^hd build: missing FILE$/m);
  assert.match((await usageError(["build", core, core])).stderr, /^hd build: unexpected argument/m);
  assert.match(
    (await usageError(["test", "--pending-function", "f", core])).stderr,
    /--pending-function needs --scenario cancellation-cleanup/,
  );
});

test("flags may follow FILE, and --format is global", async () => {
  assert.match((await hd(["build", core, "--wat"])).stdout, /^\(module/m);
  assert.equal((await hd(["--format", "json", "run", core])).stdout.trim(), "7");
  assert.match(
    (await hd(["check", core, "--format", "json"])).stdout,
    /^\{"kind":"summary",.*"status":0\}\n$/,
  );
});

test("hd debug parse and hd debug hir replace hd parse and hd dump-hir", async () => {
  assert.match((await hd(["debug", "parse", core])).stdout, /core\.hd: ok/);
  // The conformance runner's spelling stays (spec/conformance/README.md#command-contract).
  assert.match((await hd(["parse", core])).stdout, /core\.hd: ok/);
  const hir = JSON.parse((await hd(["debug", "hir", core])).stdout) as {
    functions: { name: string }[];
  };
  assert.ok(hir.functions.some(({ name }) => name === "main"));
  assert.match((await hd(["debug"])).stdout, /^usage: hd debug parse\|hir FILE$/m);
  assert.match((await usageError(["debug", "ast", core])).stderr, /no subcommand 'ast'/);
});

test("hd test on a package tests each module, and with no path the current package", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    await mkdir(join(directory, "src/shop"), { recursive: true });
    await mkdir(join(directory, "src/testkit"), { recursive: true });
    await writeFile(join(directory, "hd.toml"), "");
    await writeFile(join(directory, "src/shop/cart.hd"), "pub fn total() -> i32:\n    2\n");
    await writeFile(
      join(directory, "src/testkit/mod.hd"),
      "use pkg.shop.cart.{total}\n\npub fn make() -> i32:\n    total()\n",
    );
    await writeFile(
      join(directory, "src/shop/cart_test.hd"),
      [
        "use pkg.testkit.{make}",
        "use std.testing.assert_equal",
        "",
        'it("totals through the helper"):',
        '    assert_equal(make(), 2, reason="testkit calls total")',
        "",
        'it("adds"):',
        '    assert_equal(1 + 1, 2, reason="arithmetic")',
        "",
      ].join("\n"),
    );
    const expected = "src/shop/cart_test.hd: 2 passed\n";
    assert.equal((await hd(["test"], directory)).stdout, expected);
    assert.equal((await hd(["test"], join(directory, "src/shop"))).stdout, `../../${expected}`);
    assert.equal((await hd(["test", directory])).stdout, `${join(directory, expected)}`);

    // A directory that is not a package tests each .hd file in it.
    const loose = join(directory, "loose");
    await mkdir(loose);
    await writeFile(
      join(loose, "one.hd"),
      'tests:\n    it("one"):\n        pass\n    it("two"):\n        pass\n',
    );
    await writeFile(join(loose, "two.hd"), 'tests:\n    it("three"):\n        pass\n');
    assert.equal(
      (await hd(["test", "loose"], directory)).stdout,
      "loose/one.hd: 2 passed\nloose/two.hd: 1 passed\n",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("hd test on a package prints an error in a shared module once", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    await mkdir(join(directory, "src"), { recursive: true });
    await writeFile(join(directory, "hd.toml"), "");
    await writeFile(join(directory, "src/base.hd"), 'pub fn one() -> i32:\n    "one"\n');
    for (const name of ["left", "right"])
      await writeFile(
        join(directory, `src/${name}.hd`),
        `use pkg.base.{one}\n\npub fn ${name}() -> i32:\n    one()\n`,
      );
    let failure: CommandResult | undefined;
    await assert.rejects(hd(["test"], directory), (error: CommandResult) => {
      failure = error;
      return true;
    });
    assert.equal(failure!.code, 1);
    const located = (failure!.stdout + failure!.stderr)
      .split("\n")
      .filter((line) => /base\.hd:2:\d+: /.test(line));
    assert.equal(located.length, 1, located.join("\n"));
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

/** Writes `files` (package path to text) under `directory`. */
async function writeTree(
  directory: string,
  files: Readonly<Record<string, string>>,
): Promise<void> {
  for (const [path, text] of Object.entries(files)) {
    await mkdir(join(directory, path, ".."), { recursive: true });
    await writeFile(join(directory, path), text);
  }
}

/** Runs `hd` expecting exit status 1, and returns its stdout and stderr together. */
async function failure(args: readonly string[], cwd = root): Promise<string> {
  let result: CommandResult | undefined;
  await assert.rejects(hd(args, cwd), (error: CommandResult) => {
    result = error;
    return true;
  });
  assert.equal(result!.code, 1, `hd ${args.join(" ")} exits 1`);
  return result!.stdout + result!.stderr;
}

test("hd run, check, and build on a package file link the package", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    await writeTree(directory, {
      "hd.toml": "",
      "src/util.hd": 'pub fn greet() -> string: "hi"\n',
      "src/main.hd": "use self.util.greet\npub fn main() -> void $ Console: println(greet())\n",
    });
    const main = join(directory, "src/main.hd");
    assert.equal((await hd(["run", main])).stdout, "hi\n");
    assert.equal((await hd(["check", main])).stdout, `${main}: ok\n`);
    assert.match((await hd(["build", "--wat", main])).stdout, /^\(module/);
    assert.equal((await hd(["run", "src/main.hd"], directory)).stdout, "hi\n");
    // A package without hd.toml is found by its src/ directory.
    await rm(join(directory, "hd.toml"));
    assert.equal((await hd(["run", main])).stdout, "hi\n");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("hd run resolves super uses, and reports a package error in its own file", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    await writeTree(directory, {
      "hd.toml": "",
      "src/base/util.hd": 'pub fn greet() -> string: "hi"\n',
      "src/shop/cart.hd":
        "use super.super.base.util.{greet}\n\npub fn label() -> string:\n    greet()\n",
      "src/app.hd":
        "use pkg.shop.cart.{label}\npub fn main() -> void $ Console: println(label())\n",
    });
    assert.equal((await hd(["run", "src/app.hd"], directory)).stdout, "hi\n");
    await writeFile(join(directory, "src/base/util.hd"), "pub fn greet() -> string: 1\n");
    assert.match(
      await failure(["check", "src/app.hd"], directory),
      /^src\/base\/util\.hd:1:\d+: /m,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("hd run on a lone file outside any package compiles it on its own", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    await writeTree(directory, {
      "one.hd": 'pub fn shout() -> string: "lone"\n',
      "two.hd": "use self.one.{shout}\npub fn main() -> void $ Console: println(shout())\n",
    });
    // No package links two.hd with one.hd, so `shout` is unknown.
    assert.match(await failure(["run", "two.hd"], directory), /two\.hd:2:\d+: unknown-name/);
    await writeFile(
      join(directory, "two.hd"),
      'pub fn main() -> void $ Console: println("lone")\n',
    );
    assert.equal((await hd(["run", "two.hd"], directory)).stdout, "lone\n");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("hd test links integration test modules under tests/", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    await writeTree(directory, {
      "hd.toml": "",
      "src/util.hd": 'pub fn greet() -> string: "hi"\n',
      "tests/common.hd": 'pub fn expected() -> string: "hi"\n',
      "tests/greeting.hd": [
        "use pkg.util.{greet}",
        "use tests.common.{expected}",
        "use std.testing.assert_equal",
        "",
        'it("greets"):',
        '    assert_equal(greet(), expected(), reason="util greets")',
        "",
      ].join("\n"),
    });
    assert.equal((await hd(["test"], directory)).stdout, "tests/greeting.hd: 1 passed\n");
    assert.equal(
      (await hd(["test", "tests/greeting.hd"], directory)).stdout,
      "tests/greeting.hd: 1 passed\n",
    );
    // `self` in an integration test module names the test root.
    await writeFile(
      join(directory, "tests/again.hd"),
      'use self.common.{expected}\nuse std.testing.assert\n\nit("again"):\n    assert(expected() == "hi", reason="same root")\n',
    );
    assert.equal(
      (await hd(["test", "tests/again.hd"], directory)).stdout,
      "tests/again.hd: 1 passed\n",
    );

    // Only an integration test module may use the tests root.
    await writeFile(join(directory, "src/bad.hd"), "use tests.common.{expected}\n");
    assert.match(await failure(["check", "src/bad.hd"], directory), /test-only-use/);
    await rm(join(directory, "src/bad.hd"));
    // Relative uses stay under the test root.
    await writeFile(join(directory, "tests/up.hd"), "use super.util.{greet}\n");
    assert.match(
      await failure(["test", "tests/up.hd"], directory),
      /tests\/up\.hd:1:\d+: unknown-module: 'super' moves above the test root/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("pending-first-poll is a harness hook of the adapter, not an hd option", async () => {
  const fixture = resolve(root, "spec/conformance/runtime/valid/pending-first-poll-loops.hd");
  const plain = await hd(["test", fixture]);
  const pending = await hd(["test", "--scenario", "pending-first-poll", fixture]);
  assert.equal(pending.stdout, plain.stdout);
  assert.match(pending.stdout, /: 1 passed$/m);
  const io = bufferedIo();
  assert.equal(await main(["test", "--scenario", "pending-first-poll", fixture], io), 101);
  assert.match(io.output().stderr, /--scenario must be one of/);
});

test("hd test --format json reports every test case", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-json-"));
  try {
    await writeTree(directory, {
      "hd.toml": '[package]\nname = "shop"\n',
      "src/util.hd": "pub fn double(value: i32) -> i32:\n    value * 2\n",
      "tests/util.hd": [
        "use pkg.util.{double}",
        "use std.testing.assert_equal",
        "",
        'it("wrong"):',
        '    assert_equal(double(2), 5, reason="double doubles")',
        "",
        'it("right"):',
        '    assert_equal(double(2), 4, reason="double doubles")',
        "",
        'it("slow", ignore="slow"):',
        '    assert_equal(double(2), 4, reason="double doubles")',
        "",
      ].join("\n"),
    });
    let failed: CommandResult | undefined;
    await assert.rejects(hd(["test", "--format", "json"], directory), (error: CommandResult) => {
      failed = error;
      return true;
    });
    assert.equal(failed!.code, 1);
    const records = failed!.stdout
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line) as Record<string, unknown>);
    assert.deepEqual(
      records.map(({ kind, name, outcome }) => [kind, name, outcome]),
      [
        ["test", "wrong", "failed"],
        ["test", "right", "passed"],
        ["test", "slow", "ignored"],
        ["summary", undefined, undefined],
      ],
    );
    assert.equal(records[2]!.message, "slow");
    assert.deepEqual(records[3], {
      kind: "summary",
      errors: 0,
      warnings: 0,
      passed: 1,
      failed: 1,
      skipped: 0,
      ignored: 1,
      status: 1,
    });

    // The diagnostic's `file` is relative to the package root, from any directory.
    await writeTree(directory, { "src/bad.hd": "pub fn bad() -> i32:\n    true\n" });
    await assert.rejects(
      hd(["check", "--format", "json", "bad.hd"], join(directory, "src")),
      (error: CommandResult) => {
        const parsed = JSON.parse(error.stdout.split("\n")[0]!) as {
          file: string;
          line: number;
          column: number;
        };
        assert.equal(parsed.file, "src/bad.hd");
        assert.equal(parsed.line, 2);
        assert.equal(typeof parsed.column, "number");
        assert.equal(error.stderr, "");
        return true;
      },
    );

    // The whole-package `hd test` passes a file with no test case.
    await rm(join(directory, "tests"), { recursive: true });
    await rm(join(directory, "src/bad.hd"));
    assert.equal((await hd(["test"], directory)).stdout, "");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
