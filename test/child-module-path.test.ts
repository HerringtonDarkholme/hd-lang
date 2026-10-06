import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { linkPackage, linkedParseOptions } from "../src/package.ts";

const STD_MESSAGE =
  "'arbitrary' is a child module of 'std.testing', which a path can't reach " +
  "through its parent; import it with `use std.testing.arbitrary`";
const PKG_MESSAGE =
  "'cart' is a child module of 'pkg.shop', which a path can't reach " +
  "through its parent; import it with `use pkg.shop.cart`, or have the " +
  "parent re-export it with `pub use`";

function messages(source: string): [string, string][] {
  return analyze(source).diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.message]);
}

function packageMessages(files: Record<string, string>, entry: string): [string, string][] {
  const linked = linkPackage(files, entry);
  assert.deepEqual(linked.diagnostics, []);
  const checked = analyze(linked.source!, { parse: linkedParseOptions(linked) });
  return checked.diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.message]);
}

const SHOP = "pub data Cart:\n    pub count: i32\npub fn total() -> i32:\n    2\n";
const LIB = "pass\n";
const PARENT = "pass\n";

test("a value path through a std parent names the child module", () => {
  assert.deepEqual(
    messages(`use std.testing

pub fn main() -> void $ Console:
    x := testing.arbitrary.With
    println("done")
`),
    [["unknown-name", STD_MESSAGE]],
  );
});

test("a type path through a std parent names the child module", () => {
  assert.deepEqual(
    messages(`use std.testing

fn make(x: testing.arbitrary.With) -> i32:
    0

pub fn main() -> void:
    pass
`),
    [["unknown-type", STD_MESSAGE]],
  );
});

test("a value path through a package parent names the child module", () => {
  assert.deepEqual(
    packageMessages(
      {
        "src/lib.hd": LIB,
        "src/shop.hd": PARENT,
        "src/shop/cart.hd": SHOP,
        "src/main.hd": "use pkg.shop\n\npub fn main() -> void:\n    x := shop.cart.total\n",
      },
      "src/main.hd",
    ),
    [["unknown-import", PKG_MESSAGE]],
  );
});

test("a type path through a package parent names the child module", () => {
  assert.deepEqual(
    packageMessages(
      {
        "src/lib.hd": LIB,
        "src/shop.hd": PARENT,
        "src/shop/cart.hd": SHOP,
        "src/main.hd":
          "use pkg.shop\n\nfn wrap(cart: shop.cart.Cart) -> i32:\n    cart.count\n\npub fn main() -> void:\n    pass\n",
      },
      "src/main.hd",
    ),
    [["unknown-import", PKG_MESSAGE]],
  );
});

test("importing the std child directly resolves both positions", () => {
  assert.deepEqual(
    messages(`use std.testing.arbitrary

fn make(x: arbitrary.With[i32]) -> i32:
    0

pub fn main() -> void:
    pass
`),
    [],
  );
});

test("importing the package child directly resolves both positions", () => {
  assert.deepEqual(
    packageMessages(
      {
        "src/lib.hd": LIB,
        "src/shop.hd": PARENT,
        "src/shop/cart.hd": SHOP,
        "src/main.hd":
          "use pkg.shop.cart.{Cart, total}\n\nfn wrap(cart: Cart) -> i32:\n    cart.count + total()\n\npub fn main() -> void:\n    pass\n",
      },
      "src/main.hd",
    ),
    [],
  );
});
