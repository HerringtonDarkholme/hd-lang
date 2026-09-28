import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { copyFile, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
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

  const requirements = await hd(["explain-requirements", suspension]);
  assert.match(requirements.stdout, /main: \$ Console/);
  assert.match(requirements.stdout, /add_two: \$ Clock/);
  assert.match(requirements.stdout, /compute: \$\(\)/);

  const trace = await hd(["trace", suspension]);
  assert.match(trace.stdout, /construct main[\s\S]*poll main[\s\S]*42[\s\S]*ready main/);

  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const replaySource = join(directory, "suspension.hd");
    await copyFile(suspension, replaySource);

    const recorded = await hd(["record", replaySource]);
    const replayPath = `${replaySource}.replay.json`;
    assert.match(
      recorded.stdout,
      new RegExp(`42[\\s\\S]*${basename(replayPath).replaceAll(".", "\\.")}`),
    );
    const events = JSON.parse(await readFile(replayPath, "utf8")) as unknown[];
    assert.ok(events.length > 0);

    const replayed = await hd(["replay", replaySource]);
    assert.equal(replayed.stdout.trim(), "42");

    const built = await hd(["build", replaySource], directory);
    const wasmPath = built.stdout.trim();
    assert.equal(basename(wasmPath), "suspension.wasm");
    assert.ok((await stat(wasmPath)).size > 8);
  } finally {
    await rm(directory, { recursive: true, force: true });
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
      '    println("recorded ${buffer.lines.len()}")',
    ];
    await writeFile(source, [...lines, "    .Ok()", ""].join("\n"));
    const ran = await hd(["run", source]);
    assert.equal(ran.stdout, "hi\nrecorded 1\n");

    await writeFile(
      source,
      [
        ...lines,
        "    $.with(Console=buffer):",
        '        println("kept")',
        '    println("recorded ${buffer.lines.len()}")',
        "    .Ok()",
        "",
      ].join("\n"),
    );
    const printed = await hd(["run", source]);
    assert.equal(printed.stdout, "hi\nrecorded 1\nrecorded 2\n");
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
