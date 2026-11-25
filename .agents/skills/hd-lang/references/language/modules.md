# Modules And Visibility

## Files are modules

In a package, each file under `src/` is a module named by its path:
`src/pricing.hd` is `pkg.pricing`, and `src/cart/format.hd` is
`pkg.cart.format`. A folder of modules has a `mod.hd`, the folder's own
module (`src/cart/mod.hd` is `pkg.cart`). `src/main.hd` holds `main`, and
`src/lib.hd` is a library's root.

Everything is private to its module unless marked `pub`.

<!-- file: shop/hd.toml -->
```toml
[package]
name = "shop"

[capabilities]
Console = true
```

<!-- file: shop/src/pricing/mod.hd -->
```hd
pub struct Line:
    pub item: string            # a field other modules read is `pub` too
    pub cents: i32

pub fn total(lines: List[Line]) -> i32:
    let sum = +0
    for line in lines:
        sum += line.cents
    sum + fee()

fn fee() -> i32: 99             # private: only this module sees it

tests:
    it("adds the fee once"):
        assert_equal(total([Line { item: "lamp", cents: 100 }]), 199)
```

<!-- file: shop/src/cart/mod.hd -->
```hd
pub use self.format.{receipt}   # re-export: callers write `pkg.cart.receipt`
```

<!-- file: shop/src/cart/format.hd -->
```hd
use pkg.pricing.{Line, total}

pub fn receipt(lines: List[Line]) -> string:
    "${lines.len()} items, ${total(lines)} cents"
```

<!-- file: shop/src/main.hd -->
```hd
use pkg.cart.receipt
use pkg.pricing.Line

pub fn main() -> void $ Console:
    cart := [Line { item: "lamp", cents: 3200 }, Line { item: "bulb", cents: 450 }]
    println(receipt(cart))
```

`hd run` in `shop/` prints `2 items, 3749 cents`.

Folders may not import each other in a circle (`folder-cycle`). Here
`src/` imports `cart/`, and `cart/` imports `pricing/`, so `pricing` is a
folder of its own: as `src/pricing.hd` it would sit in `src/`, and
`cart/` importing it would close the circle `src -> cart -> src`.

## `use`

`use` lines go at the top of a file, or at the top of a `tests:` block.
They are never written inside a function.

| Form | Brings in |
| --- | --- |
| `use std.cmp.max` | one name |
| `use std.fs.{FsRead, read_text}` | several names |
| `use std.encoding.json.{decode as from_json}` | a name under another name |
| `use pkg.pricing` | the module, used as `pricing.total(...)` |
| `pub use self.format.{receipt}` | re-exports names; `pub use` always takes the braced form |

A path starts at one of five roots:

| Root | Means |
| --- | --- |
| `std` | the standard library |
| `pkg` | this package's `src/` |
| `dep.NAME` | the dependency `NAME` from `hd.toml` |
| `self` | the current module |
| `super` | the parent module |

A single file outside a package may import only `std`.

## Visibility rules that bite

- `pub fn` may name only `pub` types and traits in its parameters, result
  and row. Otherwise the error is `private-type-leak`: make the type `pub`
  or the function private.
- Importing a private name is `private-import`.
- A struct's fields are private to its module unless each is `pub`. Another
  module that reads one, or builds the struct with a literal, gets
  `private-member`: mark the field `pub`, or give callers a `pub fn`
  constructor and getter.
- A top-level `NAME := value` is always private. Export a function that
  returns it.
- A name may not shadow a prelude name such as `string`, `List`, `Result`
  or `println` (`prelude-name-shadow`).

## Doc comments

`##` starts a doc comment, attached to the declaration right below it with
no blank line between. `hd doc` reads them. A fenced `hd` block inside one
is a doc test ([testing/integration.md](../testing/integration.md)).

```hd
## The total price of `lines`, in cents.
pub fn total(cents: List[i32]) -> i32:
    cents.sum()
```
