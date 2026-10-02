import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";

// The shape of the `hd` command line (src/cli-args.ts): help output, the
// flags each command owns, `hd debug`, and `hd test` on a directory.

interface CommandResult {
  readonly stdout: string;
  readonly stderr: string;
  readonly code?: number;
}

const execute = promisify(execFile);
const root = resolve(import.meta.dirname, "..");
const entrypoint = resolve(root, "bin/hd.js");
const core = resolve(root, "examples/core.hd");

async function hd(args: readonly string[], cwd = root): Promise<CommandResult> {
  return execute(process.execPath, [entrypoint, ...args], { cwd, encoding: "utf8" });
}

/** Runs `hd` expecting exit status 2, and returns its output. */
async function usageError(args: readonly string[]): Promise<CommandResult> {
  let failure: CommandResult | undefined;
  await assert.rejects(hd(args), (error: CommandResult) => {
    failure = error;
    return true;
  });
  assert.equal(failure!.code, 2, `hd ${args.join(" ")} exits 2`);
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
  assert.match((await hd(["check", core, "--format", "json"])).stdout, /core\.hd: ok/);
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
