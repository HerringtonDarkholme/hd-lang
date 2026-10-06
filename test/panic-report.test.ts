// Runtime panic reports name the operation that panicked
// (spec/lang/06-control-flow.md#r-flow.panic.report), and an
// `integer-overflow` at a type from the `usize` default says so and gives the
// fix (#r-flow.panic.report.fallback, #r-flow.panic.report.fallback.fix).

import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { analyze, compileToWat, instantiate } from "../src/compiler.ts";
import { emitWat, withoutSiteLines } from "../src/emitter/index.ts";
import { isStackExhaustion, RuntimePanicError } from "../src/runtime-panic.ts";
import { assembleWat } from "../src/wasm.ts";
import { runHd } from "./hd-in-process.ts";

async function withFile(
  source: string,
  body: (file: string) => Promise<void>,
  name = "main.hd",
): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), "hd-panic-report-"));
  try {
    const file = join(directory, name);
    await writeFile(file, source);
    await body(file);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

const KINDS = `fn pick(kind: i32) -> i32:
    let items = [+1, +2, +3]
    let m: Map[string, i32] = {"a": +1}
    let zero = +0
    let none: Option[i32] = .None
    if kind == 0:
        return items[5]
    if kind == 1:
        return m["missing"]
    if kind == 2:
        return items.iter().map(fn(n: i32) -> i32: n / zero).sum()
    if kind == 3:
        return none.expect("needed a value")
    panic("boom")

pub fn main() -> void $ Console:
    println("$\{pick(+KIND)}")
`;

test("a panic names the program operation it happened at, inside std calls too", async () => {
  const expected = [
    "7:16: index-out-of-bounds: runtime panic",
    // A panic inside lib/std names the program's call into it.
    "9:16: index-out-of-bounds: runtime panic",
    // A closure's own operation, under a std adapter.
    "11:52: integer-division-by-zero: runtime panic",
    "13:16: explicit-panic: needed a value",
    "14:5: explicit-panic: boom",
  ];
  for (const [kind, report] of expected.entries())
    await withFile(KINDS.replace("KIND", String(kind)), async (file) => {
      const result = await runHd([file]);
      assert.equal(result.status, 1, result.stderr);
      assert.equal(result.stderr.trim(), `${file}:${report}`);
      if (kind > 0) return;
      const json = await runHd([file, "--format", "json"]);
      const record = JSON.parse(json.stderr.split("\n")[0]!) as Record<string, unknown>;
      assert.deepEqual(
        [record.code, record.message, record.file, record.line, record.column],
        ["index-out-of-bounds", "runtime panic", file, 7, 16],
      );
    });
});

test("code the compiler writes for an expression names the call that ran it", async () => {
  // `values.iter()` makes a step closure at line 2; the panic is in `next()`.
  const source = [
    "fn invalidated(values: mut List[i32]) -> void:",
    "    let iterator: mut Iterator[i32] = values.iter()",
    "    values.push(4)",
    "    _ := iterator.next()",
    "",
    "pub fn main() -> void:",
    "    let values: mut List[i32] = [1, 2, 3]",
    "    invalidated(values)",
    "",
  ].join("\n");
  await withFile(source, async (file) => {
    assert.equal(
      (await runHd([file])).stderr.trim(),
      `${file}:4:10: iterator-invalidated: runtime panic`,
    );
  });
});

const FALLBACK = [
  "pub fn main() -> void $ Console:",
  "    let balance = 100",
  "    balance = balance - 150",
  '    println("$balance")',
  "",
].join("\n");

test("an overflow at a usize default type names the binding and the fix", async () => {
  const source = FALLBACK;
  await withFile(source, async (file) => {
    const result = await runHd([file]);
    assert.equal(result.status, 1);
    assert.equal(
      result.stderr.trim(),
      [
        `${file}:3:15: integer-overflow: runtime panic`,
        `  note: 'balance' fell back to usize because its literal '100' (${file}:2:19) has no sign; write '+100' or 'let balance: i32 = 100'`,
      ].join("\n"),
    );
    const json = await runHd([file, "--format", "json"]);
    const record = JSON.parse(json.stderr.split("\n")[0]!) as { notes: string[] };
    assert.match(record.notes[0]!, /^'balance' fell back to usize/);
  });
  // An i32 overflow has no such note, and a group of bare literals has one too.
  await withFile(
    'pub fn main() -> void $ Console:\n    let big: i32 = 2147483647\n    println("${big + 1}")\n',
    async (file) => {
      assert.equal(
        (await runHd([file])).stderr.trim(),
        `${file}:3:16: integer-overflow: runtime panic`,
      );
    },
  );
  await withFile(
    'pub fn main() -> void $ Console:\n    let gap = 0 - 1\n    println("$gap")\n',
    async (file) => {
      const report = (await runHd([file])).stderr.trim().split("\n");
      assert.equal(report[0], `${file}:2:15: integer-overflow: runtime panic`);
      assert.match(report[1]!, /^ {2}note: this operation's literals fell back to usize .*'\+0'/);
    },
  );
  // A literal that takes an expected `usize` did not fall back.
  await withFile(
    'fn under() -> usize:\n    0 - 1\n\npub fn main() -> void $ Console:\n    println("${under()}")\n',
    async (file) => {
      assert.equal(
        (await runHd([file])).stderr.trim(),
        `${file}:2:5: integer-overflow: runtime panic`,
      );
    },
  );
});

test("hd test names where a failing test case panicked", async () => {
  const source = [
    "use std.testing.assert_equal",
    "",
    "tests:",
    '    it("adds"):',
    '        assert_equal(1 + 1, 3, reason="sum")',
    "",
  ].join("\n");
  await withFile(
    source,
    async (file) => {
      const result = await runHd(["test", file]);
      assert.equal(result.status, 1);
      assert.match(result.stderr, /:5:9: assertion-failed: sum: actual 2, expected 3\n/);
      const json = await runHd(["test", "--format", "json", file]);
      const record = JSON.parse(json.stdout.split("\n")[0]!) as { message: string };
      assert.match(record.message, /:5:9: assertion-failed: sum/);
    },
    "adds.hd",
  );
});

test("a release build keeps panic sites, and they add no module bytes", async () => {
  const source =
    "fn at(items: List[i32], i: usize) -> i32:\n    items[i]\n\npub fn probe() -> i32:\n    at([+1], 4)\n";
  const { instance } = await instantiate(source, { release: true });
  const probe = instance.exports.probe as () => number;
  assert.throws(probe, (error: unknown) => {
    assert.ok(error instanceof RuntimePanicError);
    assert.equal(error.code, "index-out-of-bounds");
    assert.equal(error.location, "2:5");
    return true;
  });
  // The sites live in the WAT's debug-location lines and the host's side
  // table, so the Wasm module is the same byte for byte.
  const { hir } = analyze(source);
  const plain = await assembleWat(emitWat(hir!));
  const located = await assembleWat(compileToWat(source).wat);
  assert.match(compileToWat(source).wat, /;;@ s\d+:2:5\n/);
  assert.deepEqual(located.bytes, plain.bytes);
  assert.ok(located.siteMap && located.siteMap.offsets.length > 0);
});

test("without its debug-location lines, the WAT is the code emitted without sites", () => {
  for (const source of [KINDS.replace("KIND", "0"), FALLBACK]) {
    const { hir } = analyze(source);
    const annotated = compileToWat(source).wat;
    assert.match(annotated, /;;@ s\d+:/);
    assert.equal(withoutSiteLines(annotated), emitWat(hir!));
  }
});

// Running out of call stack is the `stack-exhausted` panic
// (spec/lang/06-control-flow.md#r-flow.panic.stable-categories), not the
// engine's own error. No panic site marks a call, so it names no location.
const RECURSION = `use std.testing.assert_equal

fn deeper(depth: i32) -> i32:
    deeper(depth) + 1

pub fn main() -> void $ Console:
    println("before")
    println("$\{deeper(+0)}")

tests:
    it("recurses"):
        _ := deeper(+0)
    it("runs after it"):
        assert_equal(1, 1, reason="same")
`;

test("unbounded recursion is a stack-exhausted panic, in hd FILE and hd test", async () => {
  await withFile(RECURSION, async (file) => {
    const run = await runHd([file]);
    assert.equal(run.status, 1);
    assert.equal(run.stdout, "before\n");
    assert.match(run.stderr, /^stack-exhausted: the call stack ran out/);
    assert.doesNotMatch(run.stderr, /RangeError|wasm-function/);
    // A test case fails with the category, and the next one still runs.
    const json = await runHd(["test", "--format", "json", file]);
    assert.equal(json.status, 1);
    const records = json.stdout
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line) as { name?: string; outcome?: string; message?: string });
    assert.deepEqual(
      records.filter(({ name }) => name !== undefined).map(({ name, outcome }) => [name, outcome]),
      [
        ["recurses", "failed"],
        ["runs after it", "passed"],
      ],
    );
    assert.match(records[0]!.message!, /^stack-exhausted: /);
  });
});

test("an instance's exports raise stack-exhausted, and other errors pass through", async () => {
  const { instance } = await instantiate(
    [
      "fn deeper(depth: i32) -> i32:",
      "    deeper(depth) + 1",
      "",
      "pub fn probe() -> i32:",
      "    deeper(+0)",
      "",
      "pub fn boom() -> i32:",
      '    panic("boom")',
      "",
    ].join("\n"),
  );
  assert.ok(instance instanceof WebAssembly.Instance);
  assert.throws(instance.exports.probe as () => number, (error: unknown) => {
    assert.ok(error instanceof RuntimePanicError);
    assert.equal(error.code, "stack-exhausted");
    assert.equal(error.location, undefined);
    return true;
  });
  assert.throws(instance.exports.boom as () => number, (error: unknown) => {
    assert.ok(error instanceof RuntimePanicError);
    assert.equal(error.code, "explicit-panic");
    return true;
  });
  // Only the engines' stack-overflow errors count.
  assert.ok(isStackExhaustion(new RangeError("Maximum call stack size exceeded")));
  const firefox = Object.assign(new Error("too much recursion"), { name: "InternalError" });
  assert.ok(isStackExhaustion(firefox));
  assert.ok(!isStackExhaustion(new RangeError("Invalid array length")));
  assert.ok(!isStackExhaustion(new Error("Maximum call stack size exceeded")));
});
