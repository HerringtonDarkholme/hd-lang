// Runs the compile-and-run pipelines the worker uses, bundled for the browser
// with the same esbuild options as the playground build. Run,
// Check and Test go through the Rust compiler's wasm (rust-runner.ts), which
// the test builds first when it is missing, and so does the WAT view.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, before, test } from "node:test";
import { pathToFileURL } from "node:url";

import * as esbuild from "esbuild";

import { editedCode, loadTour, tourProject } from "../../src/tour-pages.ts";
import { buildOptions, COMPILER_WASM } from "../build.ts";
import type { Example } from "../src/examples.ts";
import { formatRunDiagnostic, type RunResult } from "../src/runner.ts";
import type * as RustRunnerModule from "../src/rust-runner.ts";
import type { RustMode } from "../src/rust-runner.ts";
import type { Project } from "../src/project.ts";

const playground = resolve(import.meta.dirname, "..");
let directory: string;
let rust: RustRunnerModule.RustRunner;

/** Bundles `entry` (under src/) as the playground build does and imports it. */
async function bundled<T>(entry: string): Promise<T> {
  const outfile = join(directory, entry.replace(/\.ts$/, ".mjs"));
  await esbuild.build(
    buildOptions({
      entryPoints: [join(playground, "src", entry)],
      outdir: undefined,
      outfile,
      minify: false,
      sourcemap: false,
      logLevel: "error",
    }),
  );
  const bundle = await readFile(outfile, "utf8");
  assert.doesNotMatch(bundle, /from\s*"node:/, `${entry}: the bundle needs no Node API`);
  return (await import(pathToFileURL(outfile).href)) as T;
}

before(async () => {
  directory = await mkdtemp(join(tmpdir(), "hd-playground-"));
  // The compiler wasm is a build output (not committed); build it when a
  // fresh checkout has none.
  if (!existsSync(COMPILER_WASM) || !existsSync(join(playground, "assets/hd_wat.wasm"))) {
    execFileSync(process.execPath, [join(playground, "../../scripts/build-web-compiler.js")], {
      stdio: "inherit",
    });
  }
  const { createRustRunner } = await bundled<typeof RustRunnerModule>("rust-runner.ts");
  rust = createRustRunner(await readFile(COMPILER_WASM), () =>
    readFile(join(playground, "assets/hd_wat.wasm")),
  );
});

/** Run, Check or Test, as the worker serves them. */
const run = (
  project: Project,
  mode: RustMode,
  onStdout?: (line: string, debug?: boolean) => void,
): Promise<RunResult> => rust.runProject(project, mode, onStdout);

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
  const result = await run(
    single('pub fn main() -> void $ Console:\n    println("hello, world")\n    println(40 + 2)\n'),
    "run",
    (line) => streamed.push(line),
  );
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["hello, world", "42"]);
  assert.deepEqual(streamed, result.stdout);
  assert.equal(result.summary, "exited normally");
});

test("dbg lines stream to the output panel marked as debug output", async () => {
  // spec/cli/command-line.md#debug-output: the playground shows dbg lines,
  // marked, beside the console's.
  const streamed: [string, boolean][] = [];
  const result = await run(
    single("pub fn main() -> void $ Console:\n    dbg(40 + 2)\n    println(42)\n"),
    "run",
    (line, debug) => streamed.push([line, debug === true]),
  );
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(streamed, [
    ["src/main.hd:2:5: 40 + 2 = 42", true],
    ["42", false],
  ]);
  // Top-level code runs as a script: its statements run, and a bare
  // expression prints nothing.
  const topLevel: [string, boolean][] = [];
  await run(single("dbg(+2)\n2 * 3\n"), "run", (line, debug) =>
    topLevel.push([line, debug === true]),
  );
  assert.deepEqual(topLevel, [["src/main.hd:1:1: +2 = 2", true]]);
});

test("a type error is reported at its line and column", async () => {
  const source = [
    "# a comment line",
    "pub fn main() -> void $ Console:",
    '    let count: i32 = "three"',
    "    println(count)",
  ].join("\n");
  for (const mode of ["run", "check"] as const) {
    const result = await run(single(source), mode);
    assert.equal(result.status, "compile-error");
    assert.deepEqual(located(result), ["src/main.hd:3:22:type-mismatch"]);
    assert.equal(
      formatRunDiagnostic(result.diagnostics[0]!),
      "src/main.hd:3:22: type-mismatch: in binding: expected i32, found string",
    );
  }
  const checked = await run(single("pub fn main() -> void: pass\n"), "check");
  assert.equal(checked.status, "ok");
  assert.equal(checked.summary, "no errors");
  assert.deepEqual(checked.stdout, []);
});

// The panic report names its category and message; its source position
// comes later (crates/hd_web/DESIGN.md §5, item 2).
test("a runtime panic is reported after the output before it", async () => {
  const result = await run(
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
  assert.equal(result.summary, "integer-division-by-zero: division by zero");
});

// spec/lang/06-control-flow.md#r-flow.panic.known-categories
test("unbounded recursion is a stack-exhausted panic", async () => {
  const source = [
    "fn deeper(depth: i32) -> i32: deeper(depth) + 1",
    "",
    "pub fn main() -> void $ Console:",
    '    println("before")',
    "    println(deeper(+0))",
  ].join("\n");
  const result = await run(single(source), "run");
  assert.equal(result.status, "panic");
  assert.deepEqual(result.stdout, ["before"]);
  assert.match(result.summary, /^stack-exhausted: the call stack ran out; /);
});

// The default profile's traits a browser can provide
// (spec/cli/command-line.md#r-cli.host.default-profile).
test("a program whose row names Clock and Random runs as with hd run", async () => {
  const source = [
    "use std.time.{Clock, ms, now, sleep}",
    "use std.random.{Random, rng}",
    "",
    "pub fn main!() -> void $ Console + Clock + Random:",
    "    println(now().unix_milliseconds() > 0)",
    "    sleep!(ms(1))",
    "    let mut draws = rng()",
    "    println(draws.int(0..10) < 10)",
  ].join("\n");
  const result = await run(single(source), "run");
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["true", "true"]);
  // Top-level code binds them too.
  const topLevel = await run(
    single("use std.time.now\n\nprintln(now().unix_milliseconds() > 0)\n"),
    "run",
  );
  assert.equal(topLevel.status, "ok", topLevel.summary);
  assert.deepEqual(topLevel.stdout, ["true"]);
});

test("Args gives the program's name and no arguments", async () => {
  const source = [
    "use std.host.{Args, args}",
    "",
    "pub fn main() -> void $ Console + Args:",
    "    println(args().len())",
    "    println($.use(Args).program())",
  ].join("\n");
  const result = await run(single(source), "run");
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["0", "main"]);
});

test("a trait the playground does not provide stops the run before it starts", async () => {
  const source = [
    "use std.fs.{FsRead, read_text}",
    "use std.path.Path",
    "",
    "pub fn main!() -> void $ Console + FsRead:",
    '    println("before")',
    '    _ := read_text!(Path("notes.txt"))',
  ].join("\n");
  const result = await run(single(source), "run");
  assert.equal(result.status, "failure");
  assert.deepEqual(result.stdout, []);
  assert.equal(
    result.summary,
    "main needs FsRead, which the playground does not provide; run it with hd run",
  );
  // Checking it is still fine.
  assert.equal((await run(single(source), "check")).status, "ok");
  // Top-level code has no `main` to name.
  const script = await run(
    single('use std.host.env\n\nprintln(env("HOME").unwrap_or("-"))\n'),
    "run",
  );
  assert.equal(
    script.summary,
    "the program needs Env, which the playground does not provide; run it with hd run",
  );
});

test("Http reaches the page's own origin through the browser, and no other", async () => {
  // A stand-in for the worker's location and synchronous XMLHttpRequest: no
  // request leaves the process.
  const sent: string[] = [];
  class FakeRequest {
    status = 0;
    responseType = "";
    timeout = 0;
    response: ArrayBuffer = new ArrayBuffer(0);
    #url = "";
    #headers: string[] = [];
    open(method: string, url: string, async: boolean): void {
      assert.equal(async, false);
      this.#url = url;
      sent.push(`${method} ${url}`);
    }
    setRequestHeader(name: string, value: string): void {
      this.#headers.push(`${name}=${value}`);
    }
    send(body: Uint8Array | null): void {
      if (this.#url.endsWith("/cors")) {
        throw Object.assign(new Error("x"), { name: "NetworkError" });
      }
      this.status = this.#url.endsWith("/missing") ? 404 : 200;
      const text = `${this.#headers.join(",")}|${body ? new TextDecoder().decode(body) : ""}`;
      this.response = new TextEncoder().encode(text).buffer as ArrayBuffer;
    }
    getAllResponseHeaders(): string {
      return "content-type: text/plain\r\nset-cookie: a\r\nset-cookie: b\r\n";
    }
  }
  const globals = globalThis as Record<string, unknown>;
  globals.location = { origin: "http://127.0.0.1:8123" };
  globals.XMLHttpRequest = FakeRequest;
  try {
    const source = [
      "use std.http.{Http, HttpError, Method, Request, Url, get}",
      "",
      "fn ping!(url: string) -> string $ Http:",
      "    match get!(url):",
      '        .Ok(response) => "status ${response.status.code()} ${response.text().expect("body")} ${response.header(.ContentType).unwrap_or("-")}"',
      '        .Err(.NotGranted(host)) => "the playground grants no ${host}"',
      '        .Err(.Connect(_)) => "the browser refused the request"',
      '        .Err(error) => "${error}"',
      "",
      "pub fn main!() -> void $ Console + Http:",
      '    println(ping!("http://127.0.0.1:8123/data"))',
      '    println(ping!("http://127.0.0.1:8123/missing"))',
      '    println(ping!("http://127.0.0.1:8123/cors"))',
      '    println(ping!("https://example.com/"))',
      '    println(ping!("http://127.0.0.1:9999/"))',
      '    match Request::post(Url::parse("http://127.0.0.1:8123/echo").expect("URL")).set_header(.Other("X-Tag"), "a").set_bytes("hi".to_utf8()).send!():',
      '        .Ok(response) => println(response.text().expect("body"))',
      '        .Err(error) => println("${error}")',
    ].join("\n");
    const result = await run(single(source), "run");
    assert.equal(result.status, "ok", result.summary);
    assert.deepEqual(result.stdout, [
      "status 200 | text/plain",
      "HTTP status 404 for http://127.0.0.1:8123/missing",
      "the browser refused the request",
      "the playground grants no example.com",
      "the playground grants no 127.0.0.1",
      "x-tag=a|hi",
    ]);
    // Another origin never reaches the browser (std-http.playground.other-origin).
    assert.deepEqual(sent, [
      "GET http://127.0.0.1:8123/data",
      "GET http://127.0.0.1:8123/missing",
      "GET http://127.0.0.1:8123/cors",
      "POST http://127.0.0.1:8123/echo",
    ]);
  } finally {
    delete globals.location;
    delete globals.XMLHttpRequest;
  }
  // Process stays unbound (std-http.playground.unbound).
  const process = await run(
    single(
      'use std.process.{Command, Process}\n\npub fn main!() -> void $ Console + Process:\n    _ := Command::new("git").run!()\n',
    ),
    "run",
  );
  assert.equal(
    process.summary,
    "main needs Process, which the playground does not provide; run it with hd run",
  );
});

test("Http rejects disabled redirects before constructing a browser request", async () => {
  let constructed = 0;
  class UnexpectedRequest {
    constructor() {
      constructed += 1;
      throw new Error("redirects(0) must not reach XMLHttpRequest");
    }
  }
  const globals = globalThis as Record<string, unknown>;
  globals.location = { origin: "http://127.0.0.1:8123" };
  globals.XMLHttpRequest = UnexpectedRequest;
  try {
    const source = [
      "use std.http.{Http, Request, Url}",
      "",
      "pub fn main!() -> void $ Console + Http:",
      '    match Request::get(Url::parse("http://127.0.0.1:8123/redirect").expect("URL")).send!(0):',
      "        .Err(.Other(message)) => println(message)",
      '        .Ok(_) => println("unexpected response")',
      '        .Err(error) => println("$error")',
    ].join("\n");
    const result = await run(single(source), "run");
    assert.equal(result.status, "ok", result.summary);
    assert.deepEqual(result.stdout, ["the browser host cannot stop redirects"]);
    assert.equal(constructed, 0, "no browser request is constructed or sent");
  } finally {
    delete globals.location;
    delete globals.XMLHttpRequest;
  }
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
        "pub struct User:",
        "    pub name: string",
        "    pub age: i32",
        "",
        "pub fn describe(user: User) -> string:",
        '    "${user.name} (${user.age})"',
      ].join("\n"),
    },
    main: "src/main.hd",
  };
  const result = await run(project, "run");
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
  const failed = await run(broken, "check");
  assert.equal(failed.status, "compile-error");
  assert.deepEqual(
    failed.diagnostics.map(({ path, line }) => `${path}:${line}`),
    ["src/models/user.hd:6"],
  );
  const missing = await run(
    { ...project, files: { "src/main.hd": project.files["src/main.hd"] } },
    "check",
  );
  assert.deepEqual(located(missing), ["src/main.hd:1:1:unknown-module"]);
});

// Without `main`, Run builds the file as a script: its top-level statements
// run as its initialization, and a bare expression prints nothing
// (crates/hd_web/DESIGN.md §5, item 1).
test("without main, Run runs the top-level statements as a script", async () => {
  const source = [
    "# no main: each top-level statement runs in order",
    "x := +21",
    "",
    "fn double(n: i32) -> i32: n * 2",
    'println("hi")',
    "println(double(x))",
    "if x > 1:",
    '    println("big")',
    "else:",
    '    println("small")',
    '[1, 2].len() == 2 && "a" != "b"',
  ].join("\n");
  const streamed: string[] = [];
  const result = await run(single(source), "run", (line) => streamed.push(line));
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["hi", "42", "big"]);
  assert.deepEqual(streamed, result.stdout);
  assert.equal(result.summary, "exited normally");

  const checked = await run(single(source), "check");
  assert.equal(checked.status, "ok", checked.summary);
  assert.deepEqual(checked.stdout, []);

  // A file of declarations and tests runs nothing; Test runs its tests.
  const declarations = await run(
    single('fn one() -> i32: 1\n\ntests:\n    it("is one"):\n        pass\n'),
    "run",
  );
  assert.deepEqual(
    [declarations.status, declarations.summary, ...declarations.stdout],
    ["ok", "exited normally"],
  );
});

test("without main, errors and panics point at the file's lines", async () => {
  // The whole file compiles before any of it runs.
  const unknown = await run(single('x := 1\nprintln("ok")\ny := missing\n'), "run");
  assert.equal(unknown.status, "compile-error");
  assert.deepEqual(unknown.stdout, []);
  assert.deepEqual(located(unknown), ["src/main.hd:3:6:unknown-name"]);
  const checked = await run(single("x := 1\ny := x + true\n"), "check");
  assert.deepEqual(located(checked), ["src/main.hd:2:10:type-mismatch"]);

  const panic = await run(single('println("before")\n\n1 / 0\n'), "run");
  assert.equal(panic.status, "panic");
  assert.deepEqual(panic.stdout, ["before"]);
  assert.equal(panic.summary, "integer-division-by-zero: division by zero");
});

test("without main, a multi-file project runs the entry module", async () => {
  const project = {
    files: {
      "src/main.hd": [
        "use pkg.models.user.{User, describe}",
        "",
        'ada := User { name: "Ada", age: 36 }',
        "println(describe(ada))",
      ].join("\n"),
      "src/models/user.hd": [
        "pub struct User:",
        "    pub name: string",
        "    pub age: i32",
        "",
        "pub fn describe(user: User) -> string:",
        '    "${user.name} (${user.age})"',
      ].join("\n"),
    },
    main: "src/main.hd",
  };
  const result = await run(project, "run");
  assert.equal(result.status, "ok", result.summary);
  assert.deepEqual(result.stdout, ["Ada (36)"]);
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
  const tested = await run(single(source), "test");
  assert.equal(tested.status, "ok", tested.summary);
  assert.equal(tested.summary, "1 test passed");
  assert.deepEqual(tested.stdout, []);
  const ran = await run(single(source), "run");
  assert.deepEqual(ran.stdout, ["main runs only for Run"]);
  const none = await run(single("pub fn main() -> void: pass\n"), "test");
  assert.equal(none.status, "failure");
  assert.equal(
    none.summary,
    "nothing to test: add a `tests:` block or a `_test.hd` module with `it(...)` test cases",
  );
});

// The summaries name no source line: a panic's position needs the source
// positions the playground does not have yet (crates/hd_web/DESIGN.md §5).
test("Test judges it_each rows, expected panics, and ignored cases as hd test does", async () => {
  const test = async (lines: readonly string[]) => {
    const result = await run(single(lines.join("\n")), "test");
    return [`${result.status}: ${result.summary}`, ...result.stdout];
  };
  const header = ["use std.testing.{assert, it_each}", "", "tests:"];
  assert.deepEqual(
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
    ["ok: 4 tests passed, 1 ignored", "IGNORED src/main.hd:9: waits (slow)"],
  );
  assert.deepEqual(
    await test([
      ...header,
      '    it_each("small", [1, 5], body=fn!(value: i32):',
      '        assert(value < 3, reason="rows are small")',
      "    )",
    ]),
    [
      'panic: assertion-failed: rows are small in test case "small[1]"',
      "PANIC src/main.hd:4: small[1]",
      "    panic: assertion-failed: rows are small",
    ],
  );
  assert.deepEqual(
    await test([
      ...header,
      '    it("panics", expect_panic="index-out-of-bounds"):',
      "        pass",
    ]),
    [
      'failure: test case "panics" failed: the case completed, but it expects the panic `index-out-of-bounds`',
      "FAIL src/main.hd:4: panics",
      "    the case completed, but it expects the panic `index-out-of-bounds`",
    ],
  );
});

/**
 * The `tests` example with cases added: a failing case, a failing
 * `it_each` row, and a passing and a failing property case. `hd test` on
 * this file passes 7 cases and fails these 3, with the same shrunk input.
 */
async function testsExample(): Promise<string> {
  const text = await readFile(join(playground, "examples/tests.hd"), "utf8");
  const added = [
    "        _ := first([])",
    "",
    '    it("joins with a dash"):',
    '        assert_equal(slug("A B"), "a_b", reason="joined")',
    "",
    '    it_each("keeps each word", ["hd", "Hd"], body=fn!(word: string):',
    '        assert_equal(slug(word), word, reason="unchanged")',
    "    )",
    "",
    '    it_prop("a slug has no spaces", prop=fn!(word: string):',
    '        assert(!slug(word).contains(" "), reason="no spaces")',
    "    )",
    "",
    '    it_prop("a number stays small", prop=fn!(n: i16):',
    '        assert(i32(n) < 100, reason="small")',
    "    )",
  ].join("\n");
  return text
    .replace(
      "use std.testing.{assert_equal, it_each, snapshot}",
      "use std.testing.{assert, assert_equal, it_each, it_prop, snapshot}",
    )
    .replace("        _ := first([])", added);
}

test("Test passes and fails the cases hd test passes and fails", async () => {
  const passing = await run(
    single(await readFile(join(playground, "examples/tests.hd"), "utf8")),
    "test",
  );
  assert.deepEqual([passing.status, passing.summary, ...passing.stdout], ["ok", "5 tests passed"]);

  const result = await run(single(await testsExample()), "test");
  assert.equal(result.status, "panic");
  assert.equal(
    result.summary,
    'assertion-failed: joined: actual "a-b", expected "a_b" in test case "joins with a dash"; 3 of 10 tests failed',
  );
  // hd test's report of the same file, without its frames and repro lines.
  assert.deepEqual(result.stdout, [
    "PANIC src/main.hd:22: joins with a dash",
    '    panic: assertion-failed: joined: actual "a-b", expected "a_b"',
    "PANIC src/main.hd:25: keeps each word[1]",
    '    panic: assertion-failed: unchanged: actual "hd", expected "Hd"',
    "PANIC src/main.hd:33: a number stays small",
    "    panic: assertion-failed: small",
    "    input: 100",
  ]);
});

// Each file directly under tests/ is its own program, as hd test builds it
// (spec/lang/10-modules.md#r-module.test.integration.program).
test("Test builds each integration test file as a program of its own", async () => {
  const files = {
    "src/lib.hd": [
      "pub fn add(a: i32, b: i32) -> i32:",
      "    a + b",
      "",
      "tests:",
      '    it("adds"):',
      "        pass",
    ].join("\n"),
    "tests/sums.hd": [
      "use pkg.{add}",
      "use std.host.Args",
      "use std.testing.assert_equal",
      "",
      'it("adds through the public API"):',
      '    assert_equal(add(1, 2), 3, reason="sum")',
      "",
      'it("is named by its file"):',
      '    assert_equal($.use(Args).program(), "tests/sums.hd", reason="its program")',
    ].join("\n"),
    "tests/more.hd": [
      "use pkg.{add}",
      "use std.testing.assert_equal",
      "",
      'it("adds again"):',
      '    assert_equal(add(2, 2), 5, reason="wrong on purpose")',
    ].join("\n"),
  };
  const result = await run({ files, main: "src/lib.hd" }, "test");
  assert.equal(
    result.summary,
    'assertion-failed: wrong on purpose: actual 4, expected 5 in test case "adds again"',
  );
  assert.deepEqual(result.stdout, [
    "PANIC tests/more.hd:4: adds again",
    "    panic: assertion-failed: wrong on purpose: actual 4, expected 5",
  ]);
});

// The playground has no disk: a snapshot file is recorded in memory for the
// run, and a case's temporary directory is a path there.
test("Test records snapshot files in memory and gives each case a directory", async () => {
  const result = await run(
    single(
      [
        "use std.testing.{assert, snapshot_file, temp_dir}",
        "",
        "tests:",
        '    it("records a snapshot"):',
        '        snapshot_file("hello")',
        "",
        '    it("has a directory"):',
        '        assert(temp_dir().to_string() != "", reason="a path")',
      ].join("\n"),
    ),
    "test",
  );
  assert.deepEqual([result.status, result.summary, ...result.stdout], ["ok", "2 tests passed"]);
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
  const tested = await run(project, "test");
  assert.equal(tested.summary, "2 tests passed");
  const ran = await run(project, "run");
  assert.deepEqual([ran.summary, ...ran.stdout], ["exited normally", "5"]);

  const failing = await run(
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
    'assertion-failed: one day late: actual 5, expected 4 in test case "charges a fee after 30 days"',
  );
  const misplaced = await run(
    {
      ...project,
      files: { ...files, "src/helpers_test.hd": 'tests:\n    it("x"):\n        pass\n' },
    },
    "test",
  );
  // The replaced helpers_test.hd no longer declares `overdue`, so the use
  // of it is reported too.
  assert.deepEqual(located(misplaced), [
    "src/billing_test.hd:2:23:unknown-import",
    "src/helpers_test.hd:1:1:misplaced-tests-block",
  ]);
  // Run leaves test modules out, so a use of one from other code names a
  // module the program does not have.
  const leaked = await run(
    {
      ...project,
      files: {
        ...files,
        "src/billing.hd": `use pkg.helpers_test.{overdue}\n${files["src/billing.hd"]}`,
      },
    },
    "run",
  );
  assert.deepEqual(located(leaked), ["src/billing.hd:1:1:unknown-module"]);
  const mistyped = await run(
    {
      ...project,
      files: { ...files, "src/helpers_test.hd": 'pub fn overdue() -> i32: "31"\n' },
    },
    "test",
  );
  assert.deepEqual(located(mistyped), ["src/helpers_test.hd:1:26:type-mismatch"]);
});

// A suspending `main!` is judged by `report()` on its result, as `main` is
// (spec/lang/10-modules.md#exit-status): an `.Err` prints its report on
// standard error and exits with code 1.
test("a suspending main! reports its Result", async () => {
  const outcome = async (body: string): Promise<string[]> => {
    const result = await run(
      single(
        `use std.error.{Error, message}\n\npub fn main!() -> Result[void, *dyn Error]:\n    ${body}\n`,
      ),
      "run",
    );
    return [result.status, result.summary, ...result.stdout];
  };
  assert.deepEqual(await outcome(".Ok(())"), ["ok", "exited normally"]);
  assert.deepEqual(await outcome('.Err(message("boom"))'), [
    "failure",
    "main exited with code 1",
    "boom",
  ]);
});

test("Test runs the test cases of a file without main", async () => {
  const result = await run(
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
    "test",
  );
  assert.equal(result.status, "panic");
  assert.equal(
    result.summary,
    'assertion-failed: wrong: actual 2, expected 3 in test case "fails"',
  );
});

/**
 * What Run gives for each bundled example: its status, its summary, then its
 * console lines. Without `main`, an example runs as a script, so
 * `top-level` prints what its statements print, and `tests` and
 * `effects-in-signature` run nothing; Test runs their test cases.
 */
const EXAMPLE_RUNS: Readonly<Record<string, readonly string[]>> = {
  "effects-in-signature": ["ok", "exited normally"],
  hello: ["ok", "exited normally", "value 42", "42", "λ"],
  "missing-provider": ["compile-error", "compilation failed"],
  "least-authority": ["ok", "exited normally", "ada@example.com|Welcome at 1970-01-01T00:00:00Z"],
  derive: ["ok", "exited normally", "0.05", "1.50", "-10.00"],
  errors: [
    "ok",
    "exited normally",
    "listening on 8080",
    "starting the server",
    "source: missing key 'port'",
    "starting the server",
    "source: port 'eighty' is not a number",
    "source: invalid digit at position 0",
  ],
  exhaustive: [
    "ok",
    "exited normally",
    "waiting for payment; can cancel: true",
    "paid 49 dollars; can cancel: true",
    "on its way with DHL; can cancel: false",
  ],
  concurrency: ["ok", "exited normally", "user-7 has 2 orders"],
  tests: ["ok", "exited normally"],
  closures: ["ok", "exited normally", "sale: 800 2000 320"],
  numbers: ["ok", "exited normally", "31536000000", "255"],
  suffixes: ["ok", "exited normally", "width 16px", "3 retry delays"],
  std: ["ok", "exited normally", "skipped 'bob x'", "ADA, CY"],
  "top-level": ["ok", "exited normally", "corner at 3, 4", "sum 7, product 12"],
  inventory: ["ok", "exited normally", "no sofa in stock"],
  "exit-code": ["failure", "main exited with code 1", "2 files changed"],
  panic: ["panic", "integer-division-by-zero: division by zero", "average of 10 over 2: 5"],
  package: ["ok", "exited normally", "hello Ada, visit 4 (regular)", "hello Grace, visit 2"],
};

/** The diagnostics Run and Check give for an example; none for the others. */
const EXAMPLE_DIAGNOSTICS: Readonly<Record<string, readonly string[]>> = {
  "missing-provider": ["src/main.hd:9:9:missing-requirement"],
};

test("the bundled examples run and check", async () => {
  const { EXAMPLES } = await bundled<{ EXAMPLES: readonly Example[] }>("examples.ts");
  assert.deepEqual(
    EXAMPLES.map(({ id }) => id).sort(),
    Object.keys(EXAMPLE_RUNS).sort(),
    "every example has an expected run",
  );
  for (const example of EXAMPLES) {
    const diagnostics = EXAMPLE_DIAGNOSTICS[example.id] ?? [];
    const ran = await run(example.project, "run");
    assert.deepEqual(
      [ran.status, ran.summary, ...ran.stdout],
      EXAMPLE_RUNS[example.id],
      example.id,
    );
    assert.deepEqual(located(ran), diagnostics, example.id);
    const checked = await run(example.project, "check");
    assert.equal(checked.status, diagnostics.length > 0 ? "compile-error" : "ok", example.id);
    assert.deepEqual(located(checked), diagnostics, example.id);
    assert.deepEqual(checked.stdout, [], example.id);
  }
  const tested = async (id: string): Promise<string> =>
    (await run(EXAMPLES.find((example) => example.id === id)!.project, "test")).summary;
  assert.equal(await tested("tests"), "5 tests passed");
  assert.equal(await tested("effects-in-signature"), "1 test passed");
  assert.equal(await tested("derive"), "1 test passed");
  assert.equal(await tested("exhaustive"), "1 test passed");
  for (const id of ["closures", "numbers", "suffixes", "std", "inventory", "concurrency"]) {
    assert.match(await tested(id), /^\d+ tests? passed$/, id);
  }
});

/**
 * Tour edits the Rust compiler does not reject yet, by page. Empty while
 * every documented edit breaks as its page says.
 */
const EDITS_ACCEPTED = new Set<string>([]);

// The tour pages name each edit's error as the Rust compiler words it
// (website/src/learn-check.ts checks the words); this test checks the code.
test("each tour snippet runs and tests as its page says; each edit breaks it as named", async () => {
  const pages = loadTour(resolve(playground, "../.."));
  assert.ok(pages.length > 0);
  for (const page of pages) {
    if (page.output) {
      const result = await run(tourProject(page), "run");
      assert.equal(result.status, "ok", `${page.source}: ${result.summary}`);
      assert.deepEqual(result.stdout, page.output, page.source);
    }
    if (page.tests) {
      const result = await run(tourProject(page), "test");
      assert.equal(result.status, "ok", `${page.source}: ${result.summary}`);
      assert.equal(result.summary, page.tests, page.source);
    }
    const { edit } = page;
    if (!edit) {
      continue;
    }
    if (edit.failure !== undefined) {
      const tested = await run(tourProject(page, editedCode(page)), "test");
      assert.ok(["failure", "panic"].includes(tested.status), `${page.source}: the edit passes`);
      assert.ok(tested.summary.includes(edit.failure), `${page.source}: ${tested.summary}`);
      continue;
    }
    const edited = await run(tourProject(page, editedCode(page)), "run");
    if (EDITS_ACCEPTED.has(page.source)) {
      assert.equal(edited.status, "ok", `${page.source}: the edit is rejected now`);
      continue;
    }
    assert.equal(edited.status, "compile-error", `${page.source}: the edit compiles`);
    const code = edit.error.slice(0, edit.error.indexOf(":"));
    const codes = edited.diagnostics.map((diagnostic) => diagnostic.code);
    assert.ok(codes.includes(code), `${page.source}: ${codes.join(", ")}`);
  }
});

test("the WAT of a program is the module the compiler printed", async () => {
  const shown = await rust.watProject(
    single('pub fn main() -> void $ Console:\n    println("hi")\n'),
  );
  assert.equal(shown.status, "ok", shown.summary);
  assert.match(shown.module!.wat, /^\(module\n/);
  // The name section gives the entry function its name.
  assert.match(shown.module!.wat, /\(func \$playground\/main\/main#0 /);
  assert.match(shown.module!.wat, /\(export "hd\.init"/);
});

test("the WAT view shows diagnostics when compilation fails", async () => {
  const shown = await rust.watProject(single('pub fn main() -> void:\n    let x: i32 = "no"\n'));
  assert.equal(shown.status, "compile-error");
  assert.equal(shown.module, undefined);
  assert.deepEqual(located(shown), ["src/main.hd:2:18:type-mismatch"]);
  const unlinked = await rust.watProject({
    files: { "src/main.hd": "use pkg.gone.{X}\n" },
    main: "src/main.hd",
  });
  assert.equal(unlinked.status, "compile-error");
});

test("the WAT of every example is a module, or the errors that stop it", async () => {
  const { EXAMPLES } = await bundled<{ EXAMPLES: readonly Example[] }>("examples.ts");
  for (const { id, project } of EXAMPLES) {
    const shown = await rust.watProject(project);
    const ran = await run(project, "run");
    if (ran.status === "compile-error") {
      assert.equal(shown.status, "compile-error", id);
      continue;
    }
    assert.equal(shown.status, "ok", `${id}: ${shown.summary}`);
    assert.match(shown.module!.wat, /^\(module/, id);
    assert.match(shown.module!.wat, /\(func \$playground\/main\//, id);
  }
});

test("without main, the WAT is the script's module", async () => {
  const shown = await rust.watProject(single('x := 20\nprintln("start")\nx + 1\nx * 2\n'));
  assert.equal(shown.status, "ok", shown.summary);
  assert.match(shown.module!.wat, /^\(module/);
});

test("ordinary compiler modes do not load WAT, including a failed WAT compile", async () => {
  const { createRustRunner } = await bundled<typeof RustRunnerModule>("rust-runner.ts");
  let loads = 0;
  const runner = createRustRunner(await readFile(COMPILER_WASM), async () => {
    loads += 1;
    throw new Error("printer offline");
  });
  const project = single("pub fn main() -> void:\n    pass\n");
  for (const mode of ["check", "run", "test"] as const) {
    await runner.compileProject(project, mode);
  }
  assert.equal(loads, 0);
  const broken = await runner.watProject(single('pub fn main() -> void:\n    let x: i32 = "no"\n'));
  assert.equal(broken.status, "compile-error");
  assert.equal(loads, 0);
  const failed = await runner.watProject(project);
  assert.equal(failed.status, "failure");
  assert.match(failed.summary, /printer offline/);
  assert.equal(loads, 1);
  await runner.compileProject(project, "check");
  assert.equal(loads, 1);
});
