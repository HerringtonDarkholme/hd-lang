---
title: Modules
---

# Modules

The program on the right totals a cart with helpers from another file.

## Files are modules

```hd example
pub fn total(lines: List[Line]) -> i32:
    subtotal := lines.iter().map(fn(line: Line) -> i32: line.cents).sum()
    subtotal + fee(subtotal)
```

Each file is a **module**. Open the other tab to see `src/pricing.hd`, where
`total` lives.

## `pub`

```hd example
pub struct Line:
    pub item: string
    pub cents: i32
```

A declaration is **private** unless it says `pub`. Fields need their own
`pub`. `fee` has none, so only `pricing.hd` can call it.

## `use`

```hd example
use pkg.pricing.{Line, total}
```

`use` names the **public items** this file needs. `pkg.pricing` is the module
in `src/pricing.hd`.

**Run it.** The cart costs 3650, under 5000, so the 499 fee applies. It
prints `total: 4149 cents`.

```hd
use pkg.pricing.{Line, total}

cart := [Line { item: "lamp", cents: 3200 }, Line { item: "bulb", cents: 450 }]
println("total: ${total(cart)} cents")
```

```hd src/pricing.hd
pub struct Line:
    pub item: string
    pub cents: i32

pub fn total(lines: List[Line]) -> i32:
    subtotal := lines.iter().map(fn(line: Line) -> i32: line.cents).sum()
    subtotal + fee(subtotal)

fn fee(subtotal: i32) -> i32:
    if subtotal >= 5000: 0 else: 499
```

```edit
replace: use pkg.pricing.{Line, total}
with: use pkg.pricing.{Line, total, fee}
error: private-import: 'fee' is private to module 'playground.pricing'
```

```output
total: 4149 cents
```
