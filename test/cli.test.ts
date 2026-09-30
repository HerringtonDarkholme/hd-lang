import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";

interface CommandResult {
  readonly stdout: string;
  readonly stderr: string;
}

interface SerializedFunction {
  readonly name?: string;
}

interface SerializedHir {
  readonly functions?: readonly SerializedFunction[];
}

const execute = promisify(execFile);
const root = resolve(import.meta.dirname, "..");
const entrypoint = resolve(root, "bin/hd.js");

async function hd(args: readonly string[], cwd = root): Promise<CommandResult> {
  return execute(process.execPath, [entrypoint, ...args], {
    cwd,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
  });
}

test("documented CLI commands work end to end", async () => {
  const core = resolve(root, "examples/core.hd");
  const suspension = resolve(root, "examples/suspension.hd");
  const runtimeFixture = resolve(root, "spec/conformance/runtime/valid/generic-data-fields.hd");

  const parsed = await hd(["parse", core]);
  assert.match(parsed.stdout, /core\.hd: ok/);

  const checked = await hd(["check", core]);
  assert.match(checked.stdout, /core\.hd: ok/);

  const tested = await hd(["test", runtimeFixture]);
  assert.match(tested.stdout, /generic-data-fields\.hd: 1 passed/);

  const run = await hd(["run", core]);
  assert.equal(run.stdout.trim(), "7");

  const wat = await hd(["build", "--wat", core]);
  assert.match(wat.stdout, /^\(module/m);
  assert.match(wat.stdout, /\(type \$d0 \(struct/);

  const hir = await hd(["dump-hir", core]);
  const parsedHir = JSON.parse(hir.stdout) as SerializedHir;
  assert.ok(parsedHir.functions?.some((declaration) => declaration.name === "main"));

  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const buildSource = join(directory, "suspension.hd");
    await copyFile(suspension, buildSource);

    const built = await hd(["build", buildSource], directory);
    const wasmPath = built.stdout.trim();
    assert.equal(basename(wasmPath), "suspension.wasm");
    assert.ok((await stat(wasmPath)).size > 8);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// trace, record, replay, and explain-requirements were removed from the CLI;
// each is now an unknown command, which prints usage and exits 2.
test("removed CLI commands fail as unknown commands", async () => {
  const suspension = resolve(root, "examples/suspension.hd");
  for (const command of ["trace", "record", "replay", "explain-requirements"]) {
    await assert.rejects(hd([command, suspension]), (error: CommandResult & { code?: number }) => {
      assert.equal(error.code, 2);
      assert.equal(error.stdout, "");
      assert.match(error.stderr, /^usage: hd <parse\|check\|test\|run\|build\|dump-hir>/);
      return true;
    });
  }
});

// A test body's `.Err` result fails the test
// (spec/10-modules.md#r-module.testing.fail).
test("hd test fails a test whose result is .Err", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "failing.hd");
    await writeFile(
      source,
      [
        "fn digit(text: string) -> Result[i32, string]:",
        '    if text == "7": .Ok(7) else: .Err("not a digit")',
        "",
        "tests:",
        '    it("propagates an error", body=fn!() -> Result[void, string]:',
        '        value := digit("x")?',
        "        .Ok()",
        "    )",
        "",
      ].join("\n"),
    );
    await assert.rejects(hd(["test", source]), (error: CommandResult & { code?: number }) => {
      assert.equal(error.code, 1);
      assert.match(error.stdout + error.stderr, /test "propagates an error" returned Err/);
      return true;
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// A suspending `main!` exits with the code `report()` gives for its result,
// and a test case fails on any nonzero code (spec/10-modules.md#exit-status,
// #r-module.testing.fail).
test("hd run and hd test judge suspending results by Termination", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  const program = async (name: string, lines: readonly string[]): Promise<string> => {
    const path = join(directory, name);
    await writeFile(path, ["use std.process.ExitCode", "", ...lines, ""].join("\n"));
    return path;
  };
  const failure = async (args: readonly string[]): Promise<CommandResult & { code?: number }> => {
    let caught: (CommandResult & { code?: number }) | undefined;
    await assert.rejects(hd(args), (error: CommandResult & { code?: number }) => {
      caught = error;
      return true;
    });
    return caught!;
  };
  try {
    const ok = await program("ok.hd", ["pub fn main!() -> Result[void, string]:", "    .Ok()"]);
    const passed = await hd(["run", ok]);
    assert.equal(passed.stdout, "");

    const err = await program("err.hd", [
      "pub fn main!() -> Result[void, string]:",
      '    .Err("boom")',
    ]);
    const erred = await failure(["run", err]);
    assert.equal(erred.code, 1);
    assert.match(erred.stdout + erred.stderr, /main returned Err/);

    const code = await program("code.hd", [
      "pub fn main!() -> Result[ExitCode, string]:",
      "    .Ok(ExitCode(3))",
    ]);
    const exited = await failure(["run", code]);
    assert.equal(exited.code, 3);
    assert.equal(exited.stdout, "");

    const reported = await program("reported.hd", [
      "tests:",
      '    it("reports a code", body=fn!() -> Result[ExitCode, string]:',
      "        .Ok(ExitCode(2))",
      "    )",
    ]);
    const failed = await failure(["test", reported]);
    assert.equal(failed.code, 1);
    assert.match(failed.stdout + failed.stderr, /test "reports a code" reported exit code 2/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// `write_line!` runs on the host console and on a program-defined provider,
// and `println` drives the covering provider's `write_line!`
// (spec/10-modules.md#console, MHP-1).
test("hd run runs Console.write_line! on host and program providers", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "console.hd");
    const lines = [
      "data Buffer:",
      "    lines: mut List[string]",
      "",
      "impl Console for Buffer:",
      "    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]:",
      "        self.lines.append(text)",
      "        .Ok()",
      "",
      "fn greet!() -> Result[void, ConsoleError] $ Console:",
      "    let console: mut Console = $.use(Console)",
      '    console.write_line!("hi")',
      "",
      "pub fn main!() -> Result[void, ConsoleError] $ Console:",
      "    greet!()?",
      "    let buffer: mut Buffer = Buffer { lines: [] }",
      "    $.with(Console=buffer):",
      "        greet!()?",
      '    $.use(Console).write_line!("recorded ${buffer.lines.len()}")?',
    ];
    await writeFile(source, [...lines, "    .Ok()", ""].join("\n"));
    const ran = await hd(["run", source]);
    assert.equal(ran.stdout, "hi\nrecorded 1\n");

    // `println` drives with `block_on`: it runs outside a driver, and
    // panics under `main!`'s driver.
    const printing = [
      ...lines.slice(0, 7),
      "pub fn main() -> void $ Console:",
      "    let buffer: mut Buffer = Buffer { lines: [] }",
      "    $.with(Console=buffer):",
      '        println("kept")',
      '    println("recorded ${buffer.lines.len()}")',
      "",
    ];
    await writeFile(source, printing.join("\n"));
    const printed = await hd(["run", source]);
    assert.equal(printed.stdout, "recorded 1\n");

    await writeFile(
      source,
      [...lines, '    println("under a driver")', "    .Ok()", ""].join("\n"),
    );
    const nested = await hd(["run", source]).then(
      () => assert.fail("println under main! must panic"),
      (error: { stderr: string }) => error,
    );
    assert.match(nested.stderr, /suspension-nested-driver/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// `snapshot_file` keeps `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`;
// a missing file fails except under `hd test --update` (Testing T53).
test("hd test compares snapshot_file text with its snapshot file", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    await writeFile(join(directory, "hd.toml"), '[package]\nname = "snap"\n');
    await mkdir(join(directory, "src"));
    const source = join(directory, "src", "greet.hd");
    await writeFile(
      source,
      [
        "use std.testing.snapshot_file",
        "",
        "tests:",
        '    it("Greets Ada"):',
        '        snapshot_file("hello, Ada")',
        "",
      ].join("\n"),
    );
    await assert.rejects(hd(["test", source]));
    await hd(["test", "--update", source]);
    const snapshot = join(directory, "__snapshots__", "greet", "greets-ada-1.snap");
    assert.equal(await readFile(snapshot, "utf8"), "hello, Ada");
    assert.match((await hd(["test", source])).stdout, /1 passed/);
    await writeFile(snapshot, "changed");
    await assert.rejects(hd(["test", source]));
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// A `*_test.hd` file is a test module whose top level holds its test cases
// (spec/10-modules.md#test-modules).
test("hd test runs a _test.hd test module", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "fee_test.hd");
    const lines = [
      "use std.testing.assert_equal",
      "",
      "pub fn late_fee(days: i32) -> i32:",
      "    if days > 30: 5 else: 0",
      "",
      'it("charges a fee after 30 days"):',
      '    assert_equal(late_fee(31), 5, reason="one day late")',
      "",
    ];
    await writeFile(source, lines.join("\n"));
    const tested = await hd(["test", source]);
    assert.match(tested.stdout, /fee_test\.hd: 1 passed/);

    await writeFile(
      source,
      [...lines, "tests:", '    it("inner"):', "        pass", ""].join("\n"),
    );
    await assert.rejects(hd(["check", source]), (error: CommandResult) => {
      assert.match(error.stdout + error.stderr, /9:1: misplaced-tests-block/);
      return true;
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// Each it_each row is its own test case in a fresh program instance
// (spec/10-modules.md#table-tests and #r-module.testing.instance).
test("hd test runs each it_each row in a fresh instance", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "rows.hd");
    const lines = [
      "use std.testing.{assert_equal, it_each}",
      "",
      "let count: i32 = 0",
      "",
      "fn bump() -> i32:",
      "    count = count + 1",
      "    count",
      "",
      "tests:",
      '    it_each("bumps once", [1, 2, 3], body=fn!(value: i32):',
      '        assert_equal(bump(), 1, reason="each row starts fresh")',
      "    )",
      "",
      '    it_each("fails on two", [1, 2, 3], body=fn!(value: i32) -> Result[void, string]:',
      '        if value == 2: .Err("two") else: .Ok()',
      "    )",
      "",
    ];
    await writeFile(source, lines.slice(0, 13).join("\n"));
    assert.match((await hd(["test", source])).stdout, /: 4 passed/);
    await writeFile(source, lines.join("\n"));
    await assert.rejects(hd(["test", source]), (error: CommandResult & { code?: number }) => {
      assert.match(error.stdout + error.stderr, /test "fails on two\[1\]" returned Err/);
      return true;
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// A `timeout` is any Duration, evaluated when the test case runs; a body
// that runs longer fails (spec/10-modules.md#r-module.testing.option.timeout-at-run).
test("hd test fails a test case that runs longer than its timeout", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "timeouts.hd");
    const lines = [
      "use std.time.{Duration, h, ms}",
      "",
      "fn budget() -> Duration: 1h",
      "",
      "fn spin(rounds: i32) -> i32:",
      "    let total: i32 = 0",
      "    let index: i32 = 0",
      "    while index < rounds:",
      "        total = (total + index) % 7",
      "        index = index + 1",
      "    total",
      "",
      "tests:",
      '    it("finishes in time", timeout=budget()):',
      "        spin(10)",
      "",
      '    it("overruns", timeout=0ms):',
      "        spin(100_000)",
      "",
    ];
    await writeFile(source, lines.slice(0, 16).join("\n"));
    assert.match((await hd(["test", source])).stdout, /: 1 passed/);
    await writeFile(source, lines.join("\n"));
    await assert.rejects(hd(["test", source]), (error: CommandResult & { code?: number }) => {
      assert.match(error.stdout + error.stderr, /test "overruns" exceeding its 0ms timeout/);
      return true;
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// A failing property case is shrunk, and the report names the seed; the
// shrink cap stops shrinking early (Testing T36, T51).
test("hd test shrinks a failing property case", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "property.hd");
    await writeFile(
      source,
      [
        "use std.testing.{assert, Choices, it_prop_with}",
        "",
        "fn number(c: mut Choices) -> i64:",
        "    c.int(0, 1000)",
        "",
        "tests:",
        '    it_prop_with("stays small", gen=number, prop=fn!(n: i64):',
        '        assert(n < 10, reason="small")',
        "    )",
        "",
      ].join("\n"),
    );
    const output = (error: CommandResult) => error.stdout + error.stderr;
    await assert.rejects(hd(["test", "--seed", "3", source]), (error: CommandResult) => {
      assert.match(output(error), /property test "stays small" \(seed 3, case \d+\)/);
      assert.match(output(error), /shrunk input 10; /);
      assert.match(output(error), /saved to __regressions__\/property\/stays-small; /);
      assert.match(output(error), /shrunk choices \[10\] after \d+ runs$/m);
      return true;
    });
    // The shrunk stream is saved one draw per line and replayed first
    // (spec/10-modules.md#r-module.testing.prop.regression-file).
    const saved = join(directory, "__regressions__", "property", "stays-small");
    assert.equal(await readFile(saved, "utf8"), "10\n");
    await writeFile(saved, "12\n");
    await assert.rejects(
      hd(["test", "--seed", "3", "--shrink", "0", source]),
      (error: CommandResult) => {
        assert.match(output(error), /\(seed 3, the saved regression case\) shrunk input 12; /);
        return true;
      },
    );
    await rm(join(directory, "__regressions__"), { recursive: true, force: true });
    const capped = hd(["test", "--seed", "3", "--shrink", "1", source]);
    await assert.rejects(capped, (error: CommandResult) => {
      assert.match(output(error), /after 1 runs, shrinking stopped early/);
      return true;
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// A property's examples run before generated cases, and a failing example is
// reported with its input (spec/10-modules.md#r-module.testing.prop.examples).
test("hd test runs property examples first", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "examples.hd");
    await writeFile(
      source,
      [
        "use std.testing.{assert, Choices, it_prop_with}",
        "",
        "fn small(c: mut Choices) -> i32:",
        "    c.int(0, 9)",
        "",
        "tests:",
        '    it_prop_with("small values", gen=small, examples=[3, 42], prop=fn!(n: i32):',
        '        assert(n < 10, reason="small")',
        "    )",
        "",
      ].join("\n"),
    );
    await assert.rejects(hd(["test", source]), (error: CommandResult) => {
      const output = error.stdout + error.stderr;
      assert.match(output, /property test "small values" \(seed \d+, example 2\) input 42; /);
      return true;
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// Discarded cases do not count toward `cases`, and more than 10 × `cases`
// discards fail the property (spec/10-modules.md#r-module.testing.prop.discard-limit).
test("hd test fails a property that discards too many cases", async () => {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const source = join(directory, "discards.hd");
    await writeFile(
      source,
      [
        "use std.testing.{Choices, it_prop_with}",
        "",
        "fn rejected(c: mut Choices) -> i64:",
        "    c.assume(false)",
        "    0",
        "",
        "tests:",
        '    it_prop_with("discards every case", gen=rejected, cases=5, prop=fn!(n: i64): pass)',
        "",
      ].join("\n"),
    );
    await assert.rejects(hd(["test", source]), (error: CommandResult) => {
      assert.match(
        error.stdout + error.stderr,
        /discarded 51 cases, more than 10 × cases \(5\), after 0 checked cases/,
      );
      return true;
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
