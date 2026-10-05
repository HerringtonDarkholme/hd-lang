import assert from "node:assert/strict";
import test from "node:test";

import { displayName } from "../src/checker/display-names.ts";
import { analyze } from "../src/compiler.ts";
import { linkPackage, linkedParseOptions } from "../src/package.ts";

function messages(source: string): [string, string][] {
  return analyze(source).diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.message]);
}

function packageMessages(files: Record<string, string>, entry: string): [string, string][] {
  const linked = linkPackage(files, entry);
  assert.deepEqual(linked.diagnostics, []);
  const checked = analyze(linked.source!, { parse: linkedParseOptions(linked) });
  return checked.diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.message]);
}

const HIDDEN = /__std_|__pkg_/;

test("displayName reads a hidden std rename through the imports", () => {
  const imports = new Map([["arbitrary", "std.testing.arbitrary"]]);
  assert.equal(displayName("__std_testing_arbitrary_With", imports), "arbitrary.With");
  assert.equal(displayName("__std_cmp_min", new Map()), "std.cmp.min");
  assert.equal(displayName("__std_time_parse_rfc3339", new Map()), "std.time.parse_rfc3339");
  assert.equal(displayName("total", imports), "total");
});

test("displayName reads a hidden package rename through the longest import", () => {
  const imports = new Map([
    ["shop", "pkg.shop"],
    ["cart", "pkg.shop.cart"],
  ]);
  assert.equal(displayName("__pkg_shop_cart_total", imports), "cart.total");
  assert.equal(displayName("__pkg_shop_cart_Box", new Map()), "Box");
});

test("a value use shows the name as written", () => {
  assert.deepEqual(
    messages(`use std.testing.arbitrary

pub fn main() -> void $ Console:
    x := arbitrary.With
    println("done")
`),
    [["unknown-name", "unknown name 'arbitrary.With'"]],
  );
});

test("a type use without scope shows the qualified path", () => {
  assert.deepEqual(
    messages(`use std.testing.arbitrary

fn f(x: arbitrary.With) -> usize:
    0

pub fn main() -> void:
    pass
`),
    [
      [
        "partial-generic-arguments",
        "'std.testing.arbitrary.With' needs a type argument for 'F', which has no default",
      ],
    ],
  );
});

const CART = "pub data Box[T]:\n    item: T\npub fn total() -> i32:\n    2\n";
const EXTRA =
  "pub data Box[T]:\n    other: T\npub fn total() -> i32:\n    3\npub fn helper() -> i32:\n    4\n";
const LIB = "pass\n";
const PARENT = "pass\n";

function colliding(entry: string): Record<string, string> {
  return {
    "src/lib.hd": LIB,
    "src/shop.hd": PARENT,
    "src/shop/cart.hd": CART,
    "src/extra.hd": EXTRA,
    "src/main.hd": entry,
  };
}

test("a package value never shows the hidden rename", () => {
  // Package uses are gone by checking time, so the name reads short here.
  assert.deepEqual(
    packageMessages(
      colliding(
        "use pkg.shop.cart\nuse pkg.extra.helper\n\npub fn main() -> void:\n    x := cart.Box\n",
      ),
      "src/main.hd",
    ),
    [["unknown-name", "unknown name 'Box'"]],
  );
});

test("a package type without scope shows the short name", () => {
  assert.deepEqual(
    packageMessages(
      colliding(
        "use pkg.shop.cart\nuse pkg.extra.helper\n\nfn f(x: cart.Box) -> i32:\n    0\n\npub fn main() -> void:\n    pass\n",
      ),
      "src/main.hd",
    ),
    [["partial-generic-arguments", "'Box' needs a type argument for 'T', which has no default"]],
  );
});

test("no diagnostic text shows a hidden rename", () => {
  const sources = [
    `use std.testing.arbitrary

pub fn main() -> void $ Console:
    x := arbitrary.With
    println("done")
`,
    `use std.testing.arbitrary

fn f[T < arbitrary.With](x: T) -> i32:
    0

pub fn main() -> void:
    pass
`,
    `use std.testing.arbitrary

pub fn main() -> void $ Console:
    w := arbitrary.with(1)
    println("done")
`,
  ];
  for (const source of sources) {
    for (const [code, message] of messages(source)) {
      assert.doesNotMatch(message, HIDDEN, `hidden rename in ${code}: ${message}`);
    }
  }
});
