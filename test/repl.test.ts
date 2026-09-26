import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { resolve } from "node:path";
import { PassThrough } from "node:stream";
import test from "node:test";

import { classify, highlight } from "../src/highlight.ts";
import { classifyInput, needsMoreInput, ReplSession, runRepl } from "../src/repl.ts";

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
    ["[1, 2]", "[1, 2]", "list[i32]"],
    ['(1, "two")', '(1, "two")', "(i32, string)"],
    ['{"k": 1}', '{"k": 1}', "map[string, i32]"],
    ["Shape.Circle(radius=2.0)", "Shape.Circle(radius: 2.0)", "Shape"],
    ["Shape.Dot", "Shape.Dot", "Shape"],
    ["1.5", "1.5", "f64"],
    ["true", "true", "bool"],
  ];
  for (const [input, value, type] of cases) {
    const outcome = await session.evaluate(input);
    assert.deepEqual([outcome.value, outcome.type, outcome.errors], [value, type, []], input);
  }
  await session.evaluate("let maybe: i32? = nil");
  assert.equal((await session.evaluate("maybe")).value, "nil");
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

test("REPL type queries do not run or keep the expression", () => {
  const session = new ReplSession();
  assert.deepEqual(session.typeOf("[1, 2]"), { type: "list[i32]", errors: [] });
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
  assert.equal(classify("x := nil").find(({ text }) => text === "nil")?.kind, "literal");
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

test("colored REPL output highlights values and errors", async () => {
  const input = new PassThrough();
  const output = new PassThrough();
  let text = "";
  output.on("data", (chunk: Buffer) => (text += chunk.toString()));
  const done = runRepl({ input, output, terminal: false, color: true });
  input.end('["a"]\nnope\n');
  await done;
  assert.ok(text.includes(`[${ESC}[32m"a"${ESC}[0m]${ESC}[2m : list[string]${ESC}[0m`), text);
  assert.ok(text.includes(`${ESC}[31m1:1: unknown-name: unknown name 'nope'${ESC}[0m`), text);
});
