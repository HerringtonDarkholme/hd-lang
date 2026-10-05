import assert from "node:assert/strict";
import test from "node:test";

import { analyze, compileToWat, instantiate } from "../src/compiler.ts";
import { linkedParseOptions, linkPackage } from "../src/package.ts";

// `dbg` (spec/lang/10-modules.md#debug-printing): what a call prints, that it
// returns nothing, its limits, and its release-build error.

/** Runs `source`'s `main` and returns its `dbg` lines and console lines. */
async function run(
  source: string,
  options: Parameters<typeof instantiate>[1] = {},
): Promise<{ readonly debug: string[]; readonly console: string[] }> {
  const debug: string[] = [];
  const console: string[] = [];
  const { instance, compilation } = await instantiate(source, {
    ...options,
    console: (text) => console.push(text),
    debugOutput: (line) => debug.push(line),
  });
  const main = compilation.hir.functions.find(({ name }) => name === "main")!;
  (instance.exports.main as (...providers: unknown[]) => unknown)(
    ...main.requirements.map((requirement) => ({ requirement })),
  );
  return { debug, console };
}

const main = (body: string, declarations = ""): string =>
  `${declarations}\npub fn main() -> void $ Console:\n${body
    .split("\n")
    .map((line) => `    ${line}`)
    .join("\n")}\n`;

test("dbg prints each argument's location, source text, and value, and returns nothing", async () => {
  const { debug, console } = await run(
    "fn total(price: i32, qty: i32) -> i32:\n    dbg(price * qty)\n    price * qty + 50\n" +
      main('println(total(125, 10))\ndbg(1, "two")\ndbg((1, "two"))\ndbg()\nprintln("done")'),
    { debugLocation: (span) => `cart.hd:${span.start.line}:${span.start.column}` },
  );
  assert.deepEqual(debug, [
    "cart.hd:2:5: price * qty = 1250",
    "cart.hd:7:5: 1 = 1",
    'cart.hd:7:5: "two" = "two"',
    'cart.hd:8:5: (1, "two") = (1, "two")',
    "cart.hd:9:5",
  ]);
  assert.deepEqual(console, ["1300", "done"]);
});

test("a dbg call has the type void, so no value comes out of it", () => {
  const { diagnostics } = analyze("pub fn main() -> void:\n    let x: i32 = dbg(1)\n");
  assert.deepEqual(
    diagnostics.map(({ code }) => code),
    ["type-mismatch"],
  );
});

test("dbg of an optional or a result is a statement that discards nothing", () => {
  const { diagnostics } = analyze(
    "fn find(id: i32) -> i32?:\n    if id > 0: .Some(id) else: .None\n\nfn run() -> void:\n    dbg(find(1))\n",
  );
  assert.deepEqual(diagnostics, []);
});

test("a type without Debug prints its structure, private fields and payloads included", async () => {
  const declarations = [
    "data Account:",
    "    owner: string",
    "    balance: i32",
    "",
    "enum Shape:",
    "    Circle(radius: f64)",
    "    Pair(i32, string)",
    "    Mixed(i32, label: string)",
    "    Dot",
    "",
    "type Meters(f64)",
    "",
    "fn double(n: i32) -> i32: n * 2",
    "",
  ].join("\n");
  const { debug } = await run(
    main(
      [
        'dbg(Account { owner: "Ada", balance: 120 })',
        'dbg(Shape.Circle(radius=1.5), Shape.Pair(1, "a"), Shape.Mixed(2, label="x"), Shape.Dot)',
        "dbg(Meters(2.5))",
        "dbg(double)",
        "dbg(fn(x: i32) -> i32: x + 1)",
        'let found: Account? = .Some(Account { owner: "Bo", balance: 1 })',
        "dbg(found)",
        "let done: Result[void, Account] = .Ok(())",
        "dbg(done)",
      ].join("\n"),
      declarations,
    ),
  );
  assert.deepEqual(
    debug.map((line) => line.slice(line.indexOf(": ") + 2)),
    [
      'Account { owner: "Ada", balance: 120 } = Account {\n    owner: "Ada",\n    balance: 120,\n}',
      "Shape.Circle(radius=1.5) = Shape.Circle(radius=1.5)",
      'Shape.Pair(1, "a") = Shape.Pair(1, "a")',
      'Shape.Mixed(2, label="x") = Shape.Mixed(2, label="x")',
      "Shape.Dot = Shape.Dot",
      "Meters(2.5) = Meters(2.5)",
      "double = <fn double(i32) -> i32>",
      "fn(x: i32) -> i32: x + 1 = <fn(i32) -> i32>",
      'found = Option.Some(Account { owner: "Bo", balance: 1 })',
      "done = Result.Ok(())",
    ],
  );
});

test("a type's own Debug wins, and generic code prints through a Debug bound or as <T>", async () => {
  const declarations = [
    "use std.format.DebugWriter",
    "",
    "data Secret:",
    "    key: string",
    "",
    "impl Debug for Secret:",
    "    fn debug(self, out: mut DebugWriter) -> void:",
    '        out.write("Secret(***)")',
    "",
    "fn first[T](items: List[T]) -> void:",
    "    dbg(items[0])",
    "",
    "fn shown[T < Debug](items: List[T]) -> void:",
    "    dbg(items[0])",
    "",
  ].join("\n");
  const { debug } = await run(
    main('dbg(Secret { key: "k" })\nfirst([1])\nshown([2])', declarations),
  );
  assert.deepEqual(
    debug.map((line) => line.slice(line.indexOf(": ") + 2)),
    ['Secret { key: "k" } = Secret(***)', "items[0] = <T>", "items[0] = 2"],
  );
});

test("a cycle prints <cycle>, and a list, a depth, and a string are cut", async () => {
  const declarations = [
    "data Node:",
    "    name: string",
    "    next: mut Node?",
    "",
    "enum Tree:",
    "    Leaf",
    "    Branch(Tree)",
    "",
  ].join("\n");
  const { debug } = await run(
    main(
      [
        'let mut node = Node { name: "a", next: .None }',
        "node.next = .Some(node)",
        "dbg(node)",
        "let numbers: mut List[i32] = []",
        "for i in +0..+500:",
        "    numbers.push(i)",
        "dbg(numbers)",
        "let tree = Tree.Leaf",
        "for _i in +0..+12:",
        "    tree = Tree.Branch(tree)",
        "dbg(tree)",
        'let long = ""',
        "for _j in +0..+1200:",
        '    long = long + "é"',
        "dbg(long)",
      ].join("\n"),
      declarations,
    ),
  );
  const [cycle, list, tree, text] = debug.map((line) => line.slice(line.indexOf(" = ") + 3));
  assert.equal(cycle, 'Node { name: "a", next: Option.Some(<cycle>) }');
  assert.match(list!, /^\[\n {4}0,\n {4}1,\n/);
  assert.match(list!, /\n {4}99,\n {4}… 400 more\n\]$/);
  assert.equal(list!.split("\n").length, 103);
  // Ten parts deep, the eleventh prints `…` (module.dbg.limit.depth).
  assert.equal(tree!.match(/Branch\(/g)?.length, 10);
  assert.match(tree!, /\n {40}…,\n/);
  assert.doesNotMatch(tree!, /Leaf/);
  assert.equal(text, `"${"é".repeat(1000)}…"`);
});

test("debug applies none of dbg's limits", async () => {
  const { console } = await run(
    main(
      "let numbers: mut List[i32] = []\nfor i in +0..+150:\n    numbers.push(i)\nprintln(debug(numbers))",
    ),
  );
  assert.equal(console[0]!.split(", ").length, 150);
});

test("a release build rejects every dbg call of the user's own code, with a fix-it", () => {
  const source =
    "fn f(a: i32, b: i32) -> i32:\n    dbg()\n    dbg(a, b)\n    dbg(a + b)\n    a + b\n";
  const { diagnostics } = analyze(source, { release: true });
  const errors = diagnostics.filter(({ code }) => code === "dbg-in-release");
  assert.deepEqual(
    errors.map(({ span, fix }) => [span.start.line, fix?.edits[0]?.replacement]),
    [
      [2, ""],
      [3, ""],
      [4, ""],
    ],
  );
  // A debug build accepts the same calls without a warning (module.dbg.release.debug-build).
  assert.deepEqual(analyze(source).diagnostics, []);
});

test("a call in a fetched package prints nothing and warns once for the package", async () => {
  const files = {
    "src/main.hd": main("dbg(twice(3))\nprintln(twice(3))", "use dep.noisy.{twice}\n"),
  };
  const linked = linkPackage(files, "src/main.hd", {
    dependencies: {
      dependencies: { noisy: "/deps/noisy" },
      devDependencies: {},
      packages: {
        "/deps/noisy": {
          id: "/deps/noisy",
          shown: "dep.noisy",
          sourceRoot: "/deps/noisy",
          files: { "lib.hd": "pub fn twice(n: i32) -> i32:\n    dbg(n, 2)\n    n * 2\n" },
          dependencies: {},
          fetched: "github.com/acme/noisy@1.0.0",
        },
      },
    },
  });
  const options = { parse: linkedParseOptions(linked) };
  const warnings = analyze(linked.source!, options).diagnostics.filter(
    ({ code }) => code === "dbg-in-dependency",
  );
  assert.deepEqual(
    warnings.map(({ message, severity }) => [severity, message]),
    [["warning", "dependency github.com/acme/noisy@1.0.0 calls dbg; its calls print nothing"]],
  );
  const { debug, console } = await run(linked.source!, options);
  assert.equal(debug.length, 1);
  assert.match(debug[0]!, /: twice\(3\) = 6$/);
  assert.deepEqual(console, ["6"]);
  const release = analyze(linked.source!, { ...options, release: true }).diagnostics;
  assert.equal(release.filter(({ code }) => code === "dbg-in-release").length, 1);
});

test("a debug statement warns that it drops its text", () => {
  const { diagnostics } = analyze(
    "fn trace(xs: List[i32]) -> void:\n    debug(xs)\n    _ := debug(xs)\n",
  );
  assert.deepEqual(
    diagnostics.map(({ code, span, message }) => [code, span.start.line, message]),
    [["unused-debug-text", 2, "`debug` returns the text; to print it, use `dbg(x)`"]],
  );
});

test("a program without dbg links no printer and no debug output", () => {
  const { wat } = compileToWat(main("println(1)"));
  assert.doesNotMatch(wat, /dbg/);
  const printed = compileToWat(main("dbg(1)")).wat;
  assert.match(printed, /host:dbg_write/);
});
