import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";
import { linkPackage, moduleIdentity } from "../src/package.ts";

async function runPackage(files: Record<string, string>, entry = "src/main.hd"): Promise<string[]> {
  const linked = linkPackage(files, entry);
  assert.deepEqual(linked.diagnostics, []);
  const lines: string[] = [];
  const { instance, compilation } = await instantiate(linked.source!, {
    console: (text) => lines.push(text),
  });
  const main = compilation.hir.functions.find(({ entry: isEntry }) => isEntry)!;
  const run = instance.exports[main.name] as (...providers: unknown[]) => unknown;
  run(...main.requirements.map((requirement) => ({ requirement })));
  return lines;
}

function codes(files: Record<string, string>, entry = "src/main.hd"): string[] {
  return linkPackage(files, entry).diagnostics.map(
    ({ path, code, span }) => `${path}:${span.start.line}:${code}`,
  );
}

test("package paths map to module identities", () => {
  assert.equal(moduleIdentity("src/main.hd"), "main");
  assert.equal(moduleIdentity("src/models/user.hd"), "models.user");
  assert.equal(moduleIdentity("src/models/mod.hd"), "models");
  assert.equal(moduleIdentity("src/mod.hd"), "");
  assert.equal(moduleIdentity("tests/common.hd"), "tests.common");
  assert.equal(moduleIdentity("tests/api/mod.hd"), "tests.api");
  assert.equal(moduleIdentity("lib/main.hd"), undefined);
  assert.equal(moduleIdentity("src/my-models/user.hd"), undefined);
  assert.equal(moduleIdentity("src/fn.hd"), undefined);
});

test("a use of another module's public declarations links and runs", async () => {
  const lines = await runPackage({
    "src/main.hd": [
      "use pkg.models.user.{User, greet}",
      "",
      "pub fn main() -> void $ Console:",
      '    println(greet(User { name: "Ada" }))',
    ].join("\n"),
    "src/models/user.hd": [
      "pub data User:",
      "    pub name: string",
      "",
      "pub fn greet(user: User) -> string:",
      '    "hello " + user.name',
    ].join("\n"),
  });
  assert.deepEqual(lines, ["hello Ada"]);
});

test("relative uses, re-exports, and initialization order follow the use graph", async () => {
  const lines = await runPackage({
    "src/main.hd": [
      "use pkg.shop.{total}",
      "",
      "pub fn main() -> void $ Console:",
      "    println(total())",
    ].join("\n"),
    "src/shop/mod.hd": "pub use self.cart.{total}\n",
    "src/shop/cart.hd": [
      "use super.super.pricing.{price}",
      "",
      "pub fn total() -> i32: price() * 2",
    ].join("\n"),
    // A leaf folder: `src/pricing.hd` would close the loop src -> src/shop -> src.
    "src/pricing/mod.hd": "pub fn price() -> i32: 21\n",
  });
  assert.deepEqual(lines, ["42"]);
  const linked = linkPackage(
    {
      "src/main.hd": "use pkg.b.{b}\nuse pkg.a.{a}\npub fn main() -> void: pass\n",
      "src/a.hd": "pub fn a() -> i32: 1\n",
      "src/b.hd": "use pkg.a.{a}\npub fn b() -> i32: a()\n",
    },
    "src/main.hd",
  );
  assert.deepEqual(
    linked.modules.map(({ identity }) => identity),
    ["a", "b", "main"],
  );
});

test("modules not reachable from the entry are not linked", () => {
  const linked = linkPackage(
    {
      "src/main.hd": "pub fn main() -> void: pass\n",
      "src/unused.hd": "fn main() -> i32: 1\n",
    },
    "src/main.hd",
  );
  assert.deepEqual(linked.diagnostics, []);
  assert.deepEqual(
    linked.modules.map(({ path }) => path),
    ["src/main.hd"],
  );
});

test("package use errors point at the use declaration of their file", () => {
  const main = (use: string): Record<string, string> => ({
    "src/main.hd": `# header\n${use}\npub fn main() -> void: pass\n`,
    "src/models.hd": "pub data User:\n    pub name: string\ndata Secret: pass\n",
  });
  assert.deepEqual(codes(main("use pkg.missing.{User}")), ["src/main.hd:2:unknown-module"]);
  assert.deepEqual(codes(main("use pkg.models.{Admin}")), ["src/main.hd:2:unknown-import"]);
  assert.deepEqual(codes(main("use pkg.models.{Secret}")), ["src/main.hd:2:private-import"]);
  assert.deepEqual(codes(main("use pkg.models")), ["src/main.hd:2:unsupported-package-use"]);
  assert.deepEqual(codes(main("use pkg.models.{User as Person}")), [
    "src/main.hd:2:unsupported-package-use",
  ]);
  assert.deepEqual(codes(main("use super.models.{User}")), ["src/main.hd:2:unknown-module"]);
  assert.deepEqual(codes(main("use dep.billing.{User}")), ["src/main.hd:2:unknown-module"]);
  assert.deepEqual(codes(main("use pkg.models.{}")), ["src/main.hd:2:syntax-error"]);
});

test("folders that depend on each other in a loop are rejected", () => {
  const files = {
    "src/mod.hd": "pub use pkg.shop.{Cart}\n",
    "src/error.hd": "pub enum Error:\n    Empty\n",
    "src/shop/mod.hd": "use pkg.error.{Error}\n\npub data Cart:\n    count: i32\n",
  };
  const linked = linkPackage(files, "src/mod.hd");
  assert.deepEqual(
    linked.diagnostics.map(({ path, code, span }) => `${path}:${span.start.line}:${code}`),
    ["src/shop/mod.hd:1:folder-cycle"],
  );
  const message = linked.diagnostics[0]!.message;
  assert.match(message, /tangle has 2 folders/);
  assert.match(message, /src\/ -> src\/shop\/: src\/mod\.hd:1: pub use pkg\.shop\.\{Cart\}/);
  assert.match(message, /src\/shop\/ -> src\/: src\/shop\/mod\.hd:1: use pkg\.error\.\{Error\}/);
  assert.match(message, /move src\/error\.hd to src\/error\/mod\.hd/);
  // The fix-it: a leaf folder keeps the module name and every use line.
  const { "src/error.hd": error, ...rest } = files;
  assert.deepEqual(codes({ ...rest, "src/error/mod.hd": error }, "src/mod.hd"), []);
  // A parent and child folder get no exemption; nested folders are separate.
  assert.deepEqual(
    codes(
      {
        "src/shop/mod.hd": "pub use pkg.shop.orders.{order}\npub fn money() -> i32: 1\n",
        "src/shop/orders/mod.hd": "use pkg.shop.{money}\npub fn order() -> i32: money()\n",
      },
      "src/shop/mod.hd",
    ),
    ["src/shop/mod.hd:1:folder-cycle"],
  );
});

test("shared names and bad paths are rejected", () => {
  assert.deepEqual(
    codes({
      "src/main.hd": "use pkg.a.{a}\nfn helper() -> i32: 1\npub fn main() -> void: pass\n",
      "src/a.hd": "pub fn a() -> i32: helper()\nfn helper() -> i32: 2\n",
    }),
    ["src/main.hd:2:package-name-collision"],
  );
  assert.deepEqual(
    codes({
      "src/main.hd": "use pkg.a.{User}\ndata User: pass\npub fn main() -> void: pass\n",
      "src/a.hd": "pub data User: pass\n",
    }),
    ["src/main.hd:1:duplicate-module-name", "src/main.hd:2:package-name-collision"],
  );
  assert.deepEqual(
    codes({ "src/main.hd": "pub fn main() -> void: pass\n", "src/Main.hd": "pass\n" }),
    ["src/main.hd:1:duplicate-module-path"],
  );
  assert.deepEqual(codes({ "src/main.hd": "pub fn main(:\n" }), [
    "src/main.hd:1:unclosed-delimiter",
  ]);
});

test("standard uses repeated across modules are imported once", async () => {
  const files = {
    "src/main.hd": [
      "use std.testing.assert_equal",
      "use pkg.math.{double}",
      "",
      "pub fn main() -> void $ Console:",
      '    assert_equal(double(2), 4, reason="double")',
      '    println("ok")',
    ].join("\n"),
    "src/math.hd": [
      "use std.testing.{assert, assert_equal}",
      "",
      "pub fn double(value: i32) -> i32:",
      '    assert(value > 0, reason="positive")',
      '    assert_equal(value, value, reason="same")',
      "    value * 2",
    ].join("\n"),
  };
  assert.deepEqual(await runPackage(files), ["ok"]);
});

test("checker diagnostics on the linked source map back to their file and line", () => {
  const linked = linkPackage(
    {
      "src/main.hd":
        "use pkg.models.{make}\n\npub fn main() -> void $ Console:\n    println(make())\n",
      "src/models.hd": '# models\npub fn make() -> i32:\n    let value: i32 = "text"\n    value\n',
    },
    "src/main.hd",
  );
  assert.deepEqual(linked.diagnostics, []);
  const analysis = analyze(linked.source!);
  const located = analysis.diagnostics.map(linked.locate);
  assert.deepEqual(
    located.map(
      ({ path, code, span }) => `${path}:${span.start.line}:${span.start.column}:${code}`,
    ),
    ["src/models.hd:3:22:type-mismatch"],
  );
});

// A test module joins as a `tests:` block with its top-level `pub` deleted,
// so a `pub fn` helper's header stays shallower than its body, and its
// diagnostics keep their columns.
test("pub fn helpers in a test module link, and their diagnostics keep their columns", () => {
  const files = (helper: string): Record<string, string> => ({
    "src/main.hd": "pub fn main() -> void: pass\n",
    "src/cart.hd": "pub fn total() -> i32: 2\n",
    "src/cart_test.hd": [
      "use pkg.cart.{total}",
      "use std.testing.assert_equal",
      "",
      helper,
      "    total() * 2",
      "",
      'it("totals"):',
      '    assert_equal(doubled(), 4, reason="twice the total")',
    ].join("\n"),
  });
  const located = (helper: string): string[] => {
    const linked = linkPackage(files(helper), "src/cart_test.hd", { tests: true });
    assert.deepEqual(linked.diagnostics, []);
    return analyze(linked.source!)
      .diagnostics.filter(({ severity }) => severity !== "warning")
      .map(linked.locate)
      .map(({ path, code, span }) => `${path}:${span.start.line}:${span.start.column}:${code}`);
  };
  assert.deepEqual(located("pub fn doubled() -> i32:"), []);
  assert.deepEqual(located("pub  fn doubled() -> NoSuchType:"), [
    "src/cart_test.hd:4:22:unknown-type",
  ]);
});

test("single-declaration uses and the package root module resolve", async () => {
  const lines = await runPackage({
    "src/main.hd": [
      "use pkg.names.shout",
      "use pkg.{suffix}",
      "",
      "pub fn main() -> void $ Console:",
      '    println(shout("hi") + suffix())',
    ].join("\n"),
    "src/names.hd": 'pub fn shout(text: string) -> string: text + "!"\n',
    "src/mod.hd": 'pub fn suffix() -> string: "?"\n',
  });
  assert.deepEqual(lines, ["hi!?"]);
});

test("each file directly under tests/ is its own program", () => {
  const files = {
    "src/text.hd": 'pub fn banner() -> string:\n    "hi"\n',
    "tests/checkout.hd": [
      "use pkg.text.banner",
      "use self.smoke.{smoke_name}",
      "",
      "pub fn shown() -> string:",
      "    banner() + smoke_name()",
    ].join("\n"),
    "tests/smoke.hd": 'pub fn smoke_name() -> string:\n    "smoke"\n',
    "tests/common/mod.hd": 'pub fn expected() -> string:\n    "hi"\n',
  };
  // A use of one integration test program from another module is
  // unknown-module (module.test.integration.program-use).
  assert.deepEqual(codes(files, "tests/checkout.hd"), ["tests/checkout.hd:2:unknown-module"]);
  // The programs link separately: checkout reaches the library module it
  // uses but not the other program, and smoke reaches neither checkout nor
  // the shared module it does not use.
  const checkout = linkPackage(files, "tests/checkout.hd", { tests: true });
  assert.deepEqual(checkout.modules.map(({ path }) => path).sort(), [
    "src/text.hd",
    "tests/checkout.hd",
  ]);
  const smoke = linkPackage(files, "tests/smoke.hd", { tests: true });
  assert.deepEqual(smoke.modules.map(({ path }) => path).sort(), ["tests/smoke.hd"]);
  // A program reaches the shared test module it uses through `self`.
  const user = linkPackage(
    {
      "tests/checkout.hd": [
        "use self.common.{expected}",
        "use std.testing.assert_equal",
        "",
        'it("uses the shared module"):',
        '    assert_equal(expected(), "hi", reason="shared")',
      ].join("\n"),
      "tests/common/mod.hd": 'pub fn expected() -> string:\n    "hi"\n',
      "tests/smoke.hd": 'pub fn smoke_name() -> string:\n    "smoke"\n',
    },
    "tests/checkout.hd",
    { tests: true },
  );
  assert.deepEqual(user.diagnostics, []);
  assert.deepEqual(user.modules.map(({ path }) => path).sort(), [
    "tests/checkout.hd",
    "tests/common/mod.hd",
  ]);
});

test("integration test sources join without re-indenting or stripping pub", () => {
  const files = {
    "src/text.hd": ["pub fn banner() -> string:", '    """line one', 'line two"""'].join("\n"),
    "tests/banner.hd": [
      "use std.testing.assert_equal",
      "use pkg.text.banner",
      "",
      'it("banner text"):',
      '    expected := """line one',
      'line two"""',
      '    assert_equal(banner(), expected, reason="same text")',
    ].join("\n"),
  };
  const linked = linkPackage(files, "tests/banner.hd", { tests: true });
  assert.deepEqual(linked.diagnostics, []);
  // The multiline string keeps its source text: no added indentation.
  assert.ok(linked.source!.includes('\nline two"""'));
  assert.ok(!linked.source!.includes("\n    line two"));
  // Top-level `it` in the joined source parses as a test case.
  const analysis = analyze(linked.source!, { parse: { joinedModules: true } });
  assert.deepEqual(
    analysis.diagnostics.filter(({ severity }) => severity !== "warning"),
    [],
  );
  assert.equal(analysis.hir!.functions.filter(({ name }) => name.startsWith("$test.")).length, 1);
});

test("a string line starting with pub fn keeps its pub", () => {
  const files = {
    "src/text.hd": 'pub fn banner() -> string:\n    "hi"\n',
    "tests/probe.hd": [
      "use std.testing.assert_equal",
      "use pkg.text.banner",
      "",
      'it("keeps pub in strings"):',
      '    body := """header',
      "pub fn not_a_declaration",
      'trailer"""',
      '    assert_equal(banner(), "hi", reason="library")',
      '    assert_equal(body.contains("pub fn"), true, reason="pub kept")',
    ].join("\n"),
  };
  const linked = linkPackage(files, "tests/probe.hd", { tests: true });
  assert.deepEqual(linked.diagnostics, []);
  assert.ok(linked.source!.includes("\npub fn not_a_declaration\n"));
  const analysis = analyze(linked.source!, { parse: { joinedModules: true } });
  assert.deepEqual(
    analysis.diagnostics.filter(({ severity }) => severity !== "warning"),
    [],
  );
});
