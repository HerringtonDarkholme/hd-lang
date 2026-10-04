---
title: Modules and pub
---

# A file shares only what it marks `pub`, so its helpers stay its own business

Each file is a module, named by its path: `src/pricing.hd` is `pricing`. A
declaration is private to its file unless it says `pub`, and another file sees
a public name only after it asks for it with `use`. So the shipping fee rule in
`fee` can change, or vanish, without breaking any other file: no other file can
call it. The `use` line at the top of a file lists everything it takes from
elsewhere, and the `pub` items of a file are its whole interface. Open the
`pricing.hd` tab to see both sides.

```hd
use pkg.pricing.{Line, total}  # ← the names this file takes from pricing.hd

pub fn main() -> void $ Console:
    cart := [Line { item: "lamp", cents: 3200 }, Line { item: "bulb", cents: 450 }]
    println("total: ${total(cart)} cents")

# `fee` in pricing.hd has no `pub`. Ask for it anyway: change the
# first line to `use pkg.pricing.{Line, total, fee}` and Run:
#     private-import: 'fee' is private to module 'pricing'; mark it 'pub'
```

```hd src/pricing.hd
# Public: the interface other files may use.
pub data Line:
    pub item: string
    pub cents: i32

pub fn total(lines: List[Line]) -> i32:
    subtotal := lines.iter().fold(0, fn(sum: i32, line: Line) -> i32: sum + line.cents)
    subtotal + fee(subtotal)

# Private: only this file can call it, so this rule can change freely.
fn fee(subtotal: i32) -> i32:
    if subtotal >= 5000: 0 else: 499
```

```edit
replace: use pkg.pricing.{Line, total}  # ← the names this file takes from pricing.hd
with: use pkg.pricing.{Line, total, fee}
error: private-import: 'fee' is private to module 'pricing'; mark it 'pub'
```

```output
total: 4149 cents
```
