import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { resolve } from "node:path";
import { PassThrough } from "node:stream";
import test from "node:test";

import { classify, highlight } from "../src/highlight.ts";
import {
  backspaceWidth,
  classifyInput,
  continuationIndent,
  INDENT_UNIT,
  needsMoreInput,
  parseReplMessage,
  ReplSession,
  respond,
  splitInputs,
} from "../src/repl.ts";
import { runRepl } from "../src/repl-terminal.ts";

const root = resolve(import.meta.dirname, "..");
const ESC = String.fromCharCode(27);

function stripColor(text: string): string {
  return text
    .split(ESC)
    .map((part, index) => (index === 0 ? part : part.replace(/^\[[0-9;]*m/, "")))
    .join("");
}

test("REPL inputs are classified by their leading words", () => {
  assert.equal(classifyInput("fn double(n: i32) -> i32: n * 2"), "declaration");
  assert.equal(classifyInput("data User:\n    name: string"), "declaration");
  assert.equal(classifyInput("fn(n: i32) -> i32: n"), "expression");
  assert.equal(classifyInput("x := 1"), "statement");
  assert.equal(classifyInput("let x: i32 = 1"), "statement");
  assert.equal(classifyInput("x = 2"), "statement");
  assert.equal(classifyInput("x == 2"), "expression");
  assert.equal(classifyInput('f(a=1, b="=")'), "expression");
  assert.equal(classifyInput("for n in names:\n    println(n)"), "statement");
});

test("REPL blocks continue until an empty line and brackets until closed", () => {
  assert.equal(needsMoreInput(["x := 1"]), false);
  assert.equal(needsMoreInput(["if x:"]), true);
  assert.equal(needsMoreInput(["if x:", "    pass"]), true);
  assert.equal(needsMoreInput(["if x:", "    pass", ""]), false);
  assert.equal(needsMoreInput(["[1,"]), true);
  assert.equal(needsMoreInput(["[1,", "2]"]), false);
  assert.equal(needsMoreInput(['"a:"']), false);
});

test("REPL continuation lines indent one level after a suite opener", () => {
  assert.equal(continuationIndent("if x:"), INDENT_UNIT);
  assert.equal(continuationIndent("fn f() -> i32:  # comment"), INDENT_UNIT);
  assert.equal(continuationIndent("    match x:"), "        ");
  assert.equal(continuationIndent("        Some(v) =>"), "            ");
  assert.equal(continuationIndent("xs.map(fn(x):"), INDENT_UNIT);
  assert.equal(continuationIndent("        x += 1"), "        ");
  assert.equal(continuationIndent("    [1,"), "    ");
  assert.equal(continuationIndent('    s := "a:"'), "    ");
  assert.equal(continuationIndent("fn f() -> i32: 1"), "");
  assert.equal(continuationIndent(""), "");
});

test("REPL Backspace in leading spaces removes one indentation level", () => {
  assert.equal(backspaceWidth(""), 0);
  assert.equal(backspaceWidth("        "), 4);
  assert.equal(backspaceWidth("    "), 4);
  assert.equal(backspaceWidth("      "), 2);
  assert.equal(backspaceWidth("    x"), 1);
  assert.equal(backspaceWidth("x    "), 1);
});

test("hd repl in a terminal indents continuation lines but not pasted ones", async () => {
  const input = new PassThrough();
  const output = new PassThrough();
  let text = "";
  output.on("data", (chunk: Buffer) => (text += chunk.toString()));
  const done = runRepl({ input, output, terminal: true, color: false });
  const type = async (keys: string): Promise<void> => {
    input.write(keys);
    await new Promise((resolve) => setImmediate(resolve));
  };
  // Typed: each line is indented for us; Backspace dedents to `else`.
  for (const keys of ["fn f(b: bool) -> i32:\r", "if b:\r", "1\r", "\x7f", "else:\r", "2\r", "\r"])
    await type(keys);
  const shown = async (value: string): Promise<void> => {
    const deadline = Date.now() + 10_000;
    while (!stripColor(text).includes(value)) {
      assert.ok(Date.now() < deadline, `no "${value}" in ${JSON.stringify(stripColor(text))}`);
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
  };
  await type("[f(true), f(false)]\r");
  await shown("[1, 2] : List[i32]");
  // Pasted, in chunks of several lines: the lines keep their own indentation,
  // so `else` stays in column 1. A plain paste, then a bracketed paste.
  await type("if true:\r    3\r");
  await type("else:\r    0\r\r");
  await shown("3 : i32");
  await type("\u001b[200~if false:\r    0\r");
  await type("else:\r    4\r\u001b[201~\r");
  await shown("4 : i32");
  input.end();
  await done;
});

test("files and snippets split into the inputs a REPL would read", () => {
  const source = [
    "# a comment before anything",
    "x := 1",
    "",
    "@derive(Debug)",
    "data User:",
    "    name: string",
    "",
    "    age: i32",
    "# between inputs",
    "if x > 0:",
    "    pass",
    "else:",
    "    pass",
    "items := [",
    "    1,",
    "]",
    "x + 1  # trailing comment",
    "",
  ].join("\n");
  assert.deepEqual(splitInputs(source), [
    { text: "x := 1", line: 2 },
    { text: "@derive(Debug)\ndata User:\n    name: string\n\n    age: i32", line: 4 },
    { text: "if x > 0:\n    pass\nelse:\n    pass", line: 10 },
    { text: "items := [\n    1,\n]", line: 14 },
    { text: "x + 1  # trailing comment", line: 17 },
  ]);
  assert.deepEqual(splitInputs("\n# only a comment\n"), []);
});

test("respond answers inputs and commands for every front end", async () => {
  const session = new ReplSession();
  assert.deepEqual(await respond(session, "x := 21"), { entries: [], kept: true });
  assert.deepEqual(await respond(session, "x * 2"), {
    entries: [{ kind: "value", text: "42", type: "i32" }],
    kept: true,
  });
  assert.deepEqual(await respond(session, "missing"), {
    entries: [{ kind: "error", text: "1:1: unknown-name: unknown name 'missing'" }],
    kept: false,
  });
  assert.deepEqual(await respond(session, ":type [x]"), {
    entries: [{ kind: "code", text: "List[i32]" }],
    kept: false,
  });
  assert.match((await respond(session, ":help")).entries[0]!.text, /:type EXPR/);
  assert.deepEqual(await respond(session, ":nope"), {
    entries: [{ kind: "info", text: "unknown command :nope; type :help" }],
    kept: false,
  });
  assert.deepEqual(await respond(session, ":reset"), {
    entries: [{ kind: "info", text: "session reset" }],
    kept: false,
    command: "reset",
  });
  assert.equal((await respond(session, "x")).kept, false);
  assert.equal((await respond(session, ":quit")).command, "quit");
});

test("REPL messages parse back into positions and codes", async () => {
  assert.deepEqual(parseReplMessage("1:6: unknown-name: unknown name 'missing'"), {
    line: 1,
    column: 6,
    severity: "error",
    code: "unknown-name",
    message: "unknown name 'missing'",
  });
  assert.deepEqual(parseReplMessage("session:3:1: warning: unused-import: unused"), {
    sessionLine: 3,
    severity: "warning",
    code: "unused-import",
    message: "unused",
  });
  assert.deepEqual(parseReplMessage("panic: integer-division-by-zero"), {
    severity: "error",
    code: "runtime-panic",
    message: "integer-division-by-zero",
  });
  const session = new ReplSession();
  for (const error of (await session.evaluate('fn bad() -> i32: "no"')).errors)
    assert.equal(parseReplMessage(error).line, 1);
});

test("REPL inputs can be checked without running them", async () => {
  const session = new ReplSession();
  assert.deepEqual(await session.evaluate('println("not printed")', { run: false }), {
    output: [],
    errors: [],
    warnings: [],
    accepted: true,
  });
  const checked = await session.evaluate("1 / 0", { run: false });
  assert.deepEqual([checked.accepted, checked.value, checked.type], [true, undefined, "i32"]);
  assert.equal((await session.evaluate("y := nope", { run: false })).accepted, false);
  // `declare` takes declarations whatever their first line is.
  const fresh = new ReplSession();
  assert.equal((await fresh.declare("# a leading comment\nfn one() -> i32: 1")).accepted, true);
  assert.equal((await fresh.evaluate("one()")).value, "1");
});

test("REPL sessions keep declarations and bindings across inputs", async () => {
  const session = new ReplSession();
  assert.deepEqual(await session.evaluate("1 + 2"), {
    output: [],
    value: "3",
    type: "i32",
    errors: [],
    warnings: [],
    accepted: true,
  });
  assert.equal((await session.evaluate("x := 21")).accepted, true);
  assert.equal((await session.evaluate("fn double(n: i32) -> i32: n * 2")).accepted, true);
  assert.equal((await session.evaluate("double(x)")).value, "42");
  const printed = await session.evaluate('println("hello")');
  assert.deepEqual(printed.output, ["hello"]);
  assert.equal(printed.value, undefined);
  // Earlier console output is not repeated when later inputs rerun the session.
  assert.deepEqual((await session.evaluate("x")).output, []);
});

test("REPL values render structurally with their types", async () => {
  const session = new ReplSession();
  await session.evaluate("data User:\n    name: string\n    age: i32");
  await session.evaluate("enum Shape:\n    Circle(radius: f64)\n    Dot");
  const cases: readonly (readonly [string, string, string])[] = [
    ['User { name: "Ada", age: 36 }', 'User { name: "Ada", age: 36 }', "User"],
    ["[1, 2]", "[1, 2]", "List[i32]"],
    ['(1, "two")', '(1, "two")', "(i32, string)"],
    ['{"k": 1}', '{"k": 1}', "Map[string, i32]"],
    ["Shape.Circle(radius=2.0)", "Shape.Circle(radius: 2.0)", "Shape"],
    ["Shape.Dot", "Shape.Dot", "Shape"],
    ["1.5", "1.5", "f64"],
    ["true", "true", "bool"],
  ];
  for (const [input, value, type] of cases) {
    const outcome = await session.evaluate(input);
    assert.deepEqual([outcome.value, outcome.type, outcome.errors], [value, type, []], input);
  }
  await session.evaluate("let maybe: i32? = .None");
  assert.equal((await session.evaluate("maybe")).value, ".None");
  await session.evaluate("let present: Option[i32] = .Some(3)");
  assert.equal((await session.evaluate("present")).value, ".Some(3)");
  await session.evaluate("let nested: i32?? = .Some(.None)");
  assert.equal((await session.evaluate("nested")).value, ".Some(.None)");
  await session.evaluate("let success: Result[i32, string] = .Ok(2)");
  assert.equal((await session.evaluate("success")).value, ".Ok(2)");
  await session.evaluate('let failure: Result[i32, string] = Result.Err("no")');
  assert.equal((await session.evaluate("failure")).value, '.Err("no")');
});

test("REPL rejects invalid inputs without changing the session", async () => {
  const session = new ReplSession();
  await session.evaluate("x := 1");
  const unknown = await session.evaluate("y := missing");
  assert.equal(unknown.accepted, false);
  assert.deepEqual(unknown.errors, ["1:6: unknown-name: unknown name 'missing'"]);
  const mismatch = await session.evaluate('fn bad() -> i32: "no"');
  assert.match(mismatch.errors[0]!, /^1:18: type-mismatch:/);
  const panic = await session.evaluate("1 / 0");
  assert.deepEqual(panic.errors, ["panic: integer-division-by-zero"]);
  assert.equal(session.source().includes("missing"), false);
  assert.equal(session.source().includes("1 / 0"), false);
  assert.equal((await session.evaluate("x + 1")).value, "2");
});

test("REPL value types keep mut access", async () => {
  const session = new ReplSession();
  await session.evaluate("let b: mut List[i32] = [1, 2]");
  assert.equal((await session.evaluate("b")).type, "mut List[i32]");
  assert.deepEqual(session.typeOf("b"), { type: "mut List[i32]", errors: [] });
  await session.evaluate("c := b");
  assert.equal((await session.evaluate("c")).type, "List[i32]");
  assert.equal((await session.evaluate("[1]")).type, "List[i32]");
});

test("REPL type queries do not run or keep the expression", () => {
  const session = new ReplSession();
  assert.deepEqual(session.typeOf("[1, 2]"), { type: "List[i32]", errors: [] });
  assert.equal(session.source().includes("[1, 2]"), false);
});

test("hd repl reads a session from standard input", async () => {
  const output = await new Promise<string>((resolvePromise, reject) => {
    const child = execFile(
      process.execPath,
      [resolve(root, "bin/hd.js"), "repl"],
      { cwd: root, encoding: "utf8" },
      (error, stdout) => (error ? reject(error) : resolvePromise(stdout)),
    );
    child.stdin?.end('x := 2\nif x > 1:\n    println("big")\n\nx * 10\n:type x\n:quit\n');
  });
  assert.equal(output, "big\n20 : i32\ni32\n");
});

test("syntax coloring classifies hd tokens and keeps the text", () => {
  const line = 'fn f(n: i32) -> string: "n=${n + 1} $n" # note';
  const spans = classify(line);
  assert.equal(spans.map(({ text }) => text).join(""), line);
  const kinds = new Map(spans.map(({ text, kind }) => [text.trim(), kind]));
  assert.equal(kinds.get("fn"), "keyword");
  assert.equal(kinds.get("f"), "function");
  assert.equal(kinds.get("i32"), "type");
  assert.equal(kinds.get("${"), "interpolation");
  assert.equal(kinds.get("$n"), "interpolation");
  assert.equal(kinds.get("1"), "number");
  assert.equal(kinds.get("# note"), "comment");
  assert.equal(kinds.get("nil"), undefined);
  // `nil` is no longer a literal word; absence is the variant `.None`.
  assert.notEqual(classify("x := nil").find(({ text }) => text === "nil")?.kind, "literal");
  // Partial input colors to the end of the line instead of failing.
  assert.equal(
    classify('"open ${a')
      .map(({ text }) => text)
      .join(""),
    '"open ${a',
  );
  const colored = highlight(line);
  assert.notEqual(colored, line);
  assert.equal(stripColor(colored), line);
});

test("syntax coloring treats raw identifiers and contextual words by position", () => {
  const kind = (line: string, text: string) =>
    classify(line).find((span) => span.text.includes(text))?.kind;
  assert.equal(kind("x := token.`type`", "`type`"), "plain");
  assert.equal(kind("use std.io as io", "use"), "keyword");
  assert.equal(kind("use std.io as io", "as"), "keyword");
  assert.equal(kind("use super.shared.{Email}", "super"), "keyword");
  assert.equal(kind("x := resource.use(f)", "use"), "function");
  assert.equal(kind("fn pick[reified T]() -> T: T()", "reified"), "keyword");
  assert.equal(kind("reified := 1", "reified"), "plain");
  assert.equal(classifyInput("use(1)"), "expression");
  assert.equal(classifyInput("use std.io"), "declaration");
});

test("colored REPL output highlights values and errors", async () => {
  const input = new PassThrough();
  const output = new PassThrough();
  let text = "";
  output.on("data", (chunk: Buffer) => (text += chunk.toString()));
  const done = runRepl({ input, output, terminal: false, color: true });
  input.end('["a"]\nnope\n');
  await done;
  assert.ok(text.includes(`[${ESC}[32m"a"${ESC}[0m]${ESC}[2m : List[string]${ESC}[0m`), text);
  assert.ok(text.includes(`${ESC}[31m1:1: unknown-name: unknown name 'nope'${ESC}[0m`), text);
});
