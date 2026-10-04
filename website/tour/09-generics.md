---
title: Generics and closures
---

# One generic function serves every type its bound allows, and the bound is checked

`top_by` finds the item with the largest key: the priciest order, the latest
date, the longest name. It is written once, for any item type `T` and any key
type `K`, and the closure you pass picks the key. The bound `K < Ord` says what
`top_by` needs from a key: that two keys can be compared. The compiler checks
the bound at each call, so a key that can't be ordered is rejected where you
pass it, not deep inside the function.

```hd
fn top_by[T, K < Ord](items: List[T], key: fn(T) -> K) -> T:
    let best = items[0]
    for item in items:
        if key(item) > key(best):  # ← needs K < Ord
            best = item
    best

pub fn main() -> void $ Console:
    orders := [Order { id: "A-1", cents: 1250 }, Order { id: "A-2", cents: 4999 }]
    println(top_by(orders, fn(order): order.cents).id)  # ← K is i32
    println(top_by(["ada", "grace"], fn(name): name.len()))

# Change the first key to `fn(order): order` and Run. An
# order has no ordering, and the call is where it fails:
#     unsatisfied-trait-bound: type 'mut Order' does not implement Ord, required by the bound on 'K' of 'top_by'

# ── plumbing ──
data Order:
    id: string
    cents: i32
```

```edit
replace:     println(top_by(orders, fn(order): order.cents).id)  # ← K is i32
with:     println(top_by(orders, fn(order): order).id)
error: unsatisfied-trait-bound: type 'mut Order' does not implement Ord, required by the bound on 'K' of 'top_by'
```

```output
A-2
grace
```
