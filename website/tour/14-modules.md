---
title: Modules
---

# Files expose only `pub` names, and `use` makes dependencies visible

Each file is a module. A declaration is private unless it says `pub`, and
another file asks for public names with `use`. Open the other tab to see both
sides of this boundary.

```hd
use pkg.pricing.{Line, total}

cart := [Line { item: "lamp", cents: 3200 }, Line { item: "bulb", cents: 450 }]
println("total: ${total(cart)} cents")

# Import the private `fee` helper and Run:
#     private-import: 'fee' is private to module 'playground.pricing'
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
