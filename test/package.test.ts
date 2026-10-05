import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";
import { linkPackage, moduleIdentity } from "../src/package.ts";
import { parse } from "../src/parser/index.ts";

async function runPackage(files: Record<string, string>, entry = "src/main.hd"): Promise<string[]> {
  const linked = linkPackage(files, entry);
  assert.deepEqual(linked.diagnostics, []);
  const lines: string[] = [];
  const { instance, compilation } = await instantiate(linked.source!, {
    // Linked source parses as joined modules, as on the command path: group
    // starts, test blocks, and top-level `it` all take their joined meaning.
    parse: { joinedModules: true, initGroupStarts: linked.initGroups },
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
  assert.equal(moduleIdentity("src/mod.hd"), undefined);
  assert.equal(moduleIdentity("src/lib.hd"), "");
  assert.equal(moduleIdentity("tests/common.hd"), "tests.common");
  assert.equal(moduleIdentity("tests/api/mod.hd"), "tests.api");
  assert.equal(moduleIdentity("lib/main.hd"), undefined);
  assert.equal(moduleIdentity("src/my-models/user.hd"), undefined);
  assert.equal(moduleIdentity("src/fn.hd"), undefined);
});

test("tasks resolve relative uses from tasks/ and are programs of their own", () => {
  const files = {
    "src/util.hd": "pub fn one() -> i32: 1\n",
    "tasks/shared/zip.hd": "pub fn zip() -> i32: 2\n",
    "tasks/seed.hd":
      "use pkg.util.{one}\nuse self.shared.zip.{zip}\n\npub fn main() -> void: pass\n",
  };
  // A task's lookup starts at tasks/ (cli.task.root-file), and it uses the
  // package through pkg (cli.task.uses).
  assert.deepEqual(codes(files, "tasks/seed.hd"), []);
  // super in a task, and a use of a task, are unknown-module (cli.task.super,
  // cli.task.program-use); src cannot reach tasks/.
  assert.deepEqual(codes({ ...files, "tasks/up.hd": "use super.util.{one}\n" }, "tasks/up.hd"), [
    "tasks/up.hd:1:unknown-module",
  ]);
  assert.deepEqual(
    codes({ ...files, "tasks/other.hd": "use self.seed.{main}\n" }, "tasks/other.hd"),
    ["tasks/other.hd:1:unknown-module"],
  );
  // A file beside a directory of its name is no program (cli.task.beside-dir,
  // module.test.integration.beside-dir).
  for (const root of ["tasks", "tests"])
    assert.deepEqual(
      codes(
        { [`${root}/shared.hd`]: "pass\n", [`${root}/shared/zip.hd`]: "pub fn zip() -> i32: 2\n" },
        `${root}/shared/zip.hd`,
      ),
      [`${root}/shared.hd:1:invalid-module-path`],
    );
});

test("the root files: lib.hd is pkg, main.hd and other entries are programs, pkg is reserved", () => {
  const library = {
    "src/lib.hd": 'pub fn greet() -> string: "hi"\n',
    "src/user.hd": "use pkg.{greet}\n\npub fn welcome() -> string: greet()\n",
  };
  assert.deepEqual(codes(library, "src/user.hd"), []);
  // src/main.hd is its own program (module.path.main-no-use).
  assert.deepEqual(
    codes(
      {
        ...library,
        "src/main.hd": "pub fn main() -> void: pass\n",
        "src/a.hd": "use pkg.main.{main}\n",
      },
      "src/a.hd",
    ),
    ["src/a.hd:1:unknown-module"],
  );
  // So is any executable's entry module (cli.exe.entry-no-use).
  const tools = {
    "src/tools/migrate.hd": "pub fn run() -> void: pass\n",
    "src/a.hd": "use pkg.tools.migrate.{run}\n",
  };
  assert.deepEqual(codes(tools, "src/a.hd"), []);
  assert.deepEqual(
    linkPackage(tools, "src/a.hd", { programs: ["src/tools/migrate.hd"] }).diagnostics.map(
      ({ code }) => code,
    ),
    ["unknown-module"],
  );
  // `pkg` names the root module (module.path.reserved-pkg), and src/mod.hd
  // is no module (module.path.no-root-mod).
  assert.deepEqual(codes({ "src/pkg.hd": "\npub fn helper() -> i32: 1\n" }, "src/pkg.hd"), [
    "src/pkg.hd:2:reserved-module-name",
  ]);
  assert.deepEqual(codes({ "src/pkg/mod.hd": "pass\n" }, "src/pkg/mod.hd"), [
    "src/pkg/mod.hd:1:reserved-module-name",
  ]);
  const root = linkPackage({ "src/mod.hd": "pass\n", "src/a.hd": "pass\n" }, "src/a.hd");
  assert.deepEqual(
    root.diagnostics.map(({ path, code }) => `${path}:${code}`),
    ["src/mod.hd:invalid-module-path"],
  );
  assert.match(root.diagnostics[0]!.message, /rename it 'src\/lib\.hd'/);
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

test("an initialization group runs statements in dependency order", async () => {
  const lines = await runPackage({
    "src/main.hd": [
      "use pkg.shop.catalog.{featured}",
      "",
      "pub fn main() -> void $ Console:",
      "    println(featured())",
    ].join("\n"),
    "src/shop/catalog.hd": [
      "use super.prices.{price_of}",
      "",
      'let cached = price_of("tea")',
      "",
      "pub fn featured() -> i32:",
      "    cached",
      "",
      "pub fn base_price(sku: string) -> i32:",
      '    if sku == "tea": 10 else: 20',
    ].join("\n"),
    "src/shop/prices.hd": [
      "use super.catalog.{base_price}",
      "",
      "let markup = +5",
      "",
      "pub fn price_of(sku: string) -> i32:",
      "    base_price(sku) + markup",
    ].join("\n"),
  });
  assert.deepEqual(lines, ["15"]);
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

test("linked source reports each initialization group single or multi as data", () => {
  const linked = linkPackage(
    {
      "src/main.hd": "use pkg.shop.catalog.{featured}\n\npub fn main() -> void:\n    pass\n",
      "src/shop/catalog.hd":
        'use super.prices.{price_of}\n\nlet cached = price_of("tea")\n\npub fn featured() -> i32:\n    cached\n\npub fn base_price(sku: string) -> i32:\n    10\n',
      "src/shop/prices.hd":
        "use super.catalog.{base_price}\n\nlet markup = 5\n\npub fn price_of(sku: string) -> i32:\n    base_price(sku) + markup\n",
    },
    "src/main.hd",
  );
  assert.deepEqual(linked.diagnostics, []);
  assert.ok(!linked.source!.includes("hd:init-group"));
  assert.deepEqual(
    linked.initGroups.map(({ multi }) => multi),
    [true, false],
  );
  const program = parse(linked.source!, {
    joinedModules: true,
    initGroupStarts: linked.initGroups,
  }).program!;
  assert.deepEqual(program.initGroups, [
    { start: 0, multi: true },
    { start: program.statements.length, multi: false },
  ]);
});

test("a source-order violation in a single-module group is still rejected in a package", () => {
  const linked = linkPackage(
    {
      "src/main.hd": "use pkg.broken.{first}\n\npub fn main() -> void:\n    pass\n",
      "src/broken.hd": [
        "use pkg.helper.{apply}",
        "",
        "first := apply(first_name)",
        'let names: List[string] = ["Ada"]',
        "",
        "fn first_name() -> string:",
        "    names[0]",
        "",
        "pub fn first() -> string:",
        "    first",
        "",
      ].join("\n"),
      "src/helper.hd": [
        "pub fn apply(callback: fn() -> string) -> string:",
        "    callback()",
        "",
      ].join("\n"),
    },
    "src/main.hd",
  );
  assert.deepEqual(linked.diagnostics, []);
  const diagnostics = analyze(linked.source!, {
    parse: { joinedModules: true, initGroupStarts: linked.initGroups },
  }).diagnostics;
  assert.deepEqual(
    diagnostics.map((diagnostic) => diagnostic.code),
    ["top-level-read-before-initialization"],
  );
});

test("a trait implementation outside the trait's and target's modules is nonlocal-impl", () => {
  const alpha =
    "pub trait Greeter:\n    fn greet(self) -> string\npub data Person:\n    name: string\n";
  const body = "impl Greeter for Person:\n    fn greet(self) -> string:\n        self.name\n";
  const beta = `use pkg.alpha.{Greeter, Person}\n\n${body}`;
  assert.deepEqual(codes({ "src/alpha.hd": alpha, "src/beta.hd": beta }, "src/beta.hd"), [
    "src/beta.hd:3:nonlocal-impl",
  ]);
  // The trait's module and the target's module may each host the implementation.
  assert.deepEqual(codes({ "src/alpha.hd": `${alpha}\n${body}` }, "src/alpha.hd"), []);
  assert.deepEqual(
    codes(
      {
        "src/alpha.hd": "pub trait Greeter:\n    fn greet(self) -> string\n",
        "src/beta.hd":
          "pub data Person:\n    name: string\nuse pkg.alpha.{Greeter}\n\nimpl Greeter for Person:\n    fn greet(self) -> string:\n        self.name\n",
      },
      "src/beta.hd",
    ),
    [],
  );
  // The module declaring an outer constructor of a trait argument may too.
  assert.deepEqual(
    codes(
      {
        "src/alpha.hd":
          "pub trait Holds[T]:\n    fn get(self) -> T\npub data Box:\n    item: string\n",
        "src/beta.hd":
          "pub data Token: pass\nuse pkg.alpha.{Holds, Box}\n\nimpl Holds[Token] for Box:\n    fn get(self) -> Token:\n        Token\n",
      },
      "src/beta.hd",
    ),
    [],
  );
});

test("folders that depend on each other in a loop are rejected", () => {
  const files = {
    "src/lib.hd": "pub use pkg.shop.{Cart}\n",
    "src/error.hd": "pub enum Error:\n    Empty\n",
    "src/shop/mod.hd": "use pkg.error.{Error}\n\npub data Cart:\n    count: i32\n",
  };
  const linked = linkPackage(files, "src/lib.hd");
  assert.deepEqual(
    linked.diagnostics.map(({ path, code, span }) => `${path}:${span.start.line}:${code}`),
    ["src/shop/mod.hd:1:folder-cycle"],
  );
  const message = linked.diagnostics[0]!.message;
  assert.match(message, /tangle has 2 folders/);
  assert.match(message, /src\/ -> src\/shop\/: src\/lib\.hd:1: pub use pkg\.shop\.\{Cart\}/);
  assert.match(message, /src\/shop\/ -> src\/: src\/shop\/mod\.hd:1: use pkg\.error\.\{Error\}/);
  assert.match(message, /move src\/error\.hd to src\/error\/mod\.hd/);
  // The fix-it: a leaf folder keeps the module name and every use line.
  const { "src/error.hd": error, ...rest } = files;
  assert.deepEqual(codes({ ...rest, "src/error/mod.hd": error }, "src/lib.hd"), []);
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
    "src/lib.hd": 'pub fn suffix() -> string: "?"\n',
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
  const analysis = analyze(linked.source!, {
    parse: { joinedModules: true, initGroupStarts: linked.initGroups },
  });
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
  const analysis = analyze(linked.source!, {
    parse: { joinedModules: true, initGroupStarts: linked.initGroups },
  });
  assert.deepEqual(
    analysis.diagnostics.filter(({ severity }) => severity !== "warning"),
    [],
  );
});
