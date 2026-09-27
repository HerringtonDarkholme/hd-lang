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
      "use super.pricing.{price}",
      "",
      "pub fn total() -> i32: price() * 2",
    ].join("\n"),
    "src/pricing.hd": "pub fn price() -> i32: 21\n",
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
});

test("use cycles, shared names, and bad paths are rejected", () => {
  assert.deepEqual(
    codes({
      "src/main.hd": "use pkg.a.{a}\npub fn main() -> void: pass\n",
      "src/a.hd": "use pkg.b.{b}\npub fn a() -> i32: 1\n",
      "src/b.hd": "use pkg.a.{a}\npub fn b() -> i32: 2\n",
    }),
    ["src/b.hd:1:use-cycle"],
  );
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
