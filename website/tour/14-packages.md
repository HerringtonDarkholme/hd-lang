---
title: Folders and packages
---

# A folder's `mod.hd` lists what the folder offers, and another package is one more root

A folder of files becomes a module when it holds a `mod.hd`: `src/shop/` is
`shop`, and `src/shop/cart.hd` is `shop.cart`. The `mod.hd` is the folder's
index. Its `pub use` lines name what `pkg.shop` offers, so the rest of the
package imports from the folder, not from whichever file a name lives in
today. A name it does not list is not part of `pkg.shop`, even when it is
`pub` in `cart.hd`. Another package works the same way under one more
root: after `hd add ../money`, its public names are `dep.money`. Open the
`shop/mod.hd` and `shop/cart.hd` tabs to see the folder.

```hd
use pkg.shop.{Line, total}  # ← from the folder, as its mod.hd lists them

pub fn main() -> void $ Console:
    cart := [Line { item: "lamp", cents: 3200 }, Line { item: "bulb", cents: 450 }]
    println("total: ${total(cart)} cents")

# mod.hd does not list `shipping`. Ask for it anyway: change the
# first line to `use pkg.shop.{Line, total, shipping}` and Run:
#     unknown-import: module 'playground.shop' declares no 'shipping'
```

```hd src/shop/mod.hd
# The folder's index: what pkg.shop offers to the rest of the package.
pub use self.cart.{Line, total}
```

```hd src/shop/cart.hd
pub struct Line:
    pub item: string
    pub cents: i32

pub fn total(lines: List[Line]) -> i32:
    lines.iter().fold(0, fn(sum: i32, line: Line) -> i32: sum + line.cents) + shipping(lines)

# Public, but mod.hd does not list it, so pkg.shop does not offer it.
pub fn shipping(lines: List[Line]) -> i32:
    if lines.len() >= 3: 0 else: 499
```

```edit
replace: use pkg.shop.{Line, total}  # ← from the folder, as its mod.hd lists them
with: use pkg.shop.{Line, total, shipping}
error: unknown-import: module 'playground.shop' declares no 'shipping'
```

```output
total: 4149 cents
```
