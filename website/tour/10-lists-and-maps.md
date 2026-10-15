---
title: Collections and iterators
---

# A list or map is typed by what it holds, and a lookup that may miss says so

`[...]` builds a `List` and `{key: value}` a `Map`, both from the prelude.
Every element has the collection's type, so a `Map[string, i32]` never hands
back a string. Indexing a map with `wanted[item] = ...` sets an entry, but
reading one is `get`, which gives an `i32?`, because the key may be absent.
You decide what a missing key means, here zero on hand. A comprehension
puts its clauses before `=>` and the value after it, and `iter()` starts a
chain of steps such as `map` and `sum`.

```hd
fn shortages(stock: Map[string, i32], orders: List[Order]) -> List[string]:
    let wanted: mut Map[string, i32] = {}
    for order in orders:
        wanted[order.item] = on_hand(wanted, order.item) + order.quantity
    [for (item, count) in wanted if count > on_hand(stock, item) => "$item: short ${count - on_hand(stock, item)}"]

fn on_hand(stock: Map[string, i32], item: string) -> i32:
    stock.get(item).unwrap_or(0)  # ← a lookup may miss: `get` gives an i32?

pub fn main() -> void $ Console:
    stock := {"lamp": +4, "bulb": +20}  # ← a Map[string, i32]
    orders := [Order { item: "lamp", quantity: 3 }, Order { item: "bulb", quantity: 6 }, Order { item: "lamp", quantity: 2 }]
    for line in shortages(stock, orders):
        println(line)
    units := orders.iter().map(fn(order): order.quantity).sum()  # ← an iterator chain
    println("$units units ordered")

# Change the body of `on_hand` to `stock.get(item)` and Run. A
# lookup may miss, so its result is not a plain i32:
#     type-mismatch: in result: expected i32, found i32?

# ── plumbing ──
data Order:
    item: string
    quantity: i32
```

```edit
replace:     stock.get(item).unwrap_or(0)  # ← a lookup may miss: `get` gives an i32?
with:     stock.get(item)
error: type-mismatch: in result: expected i32, found i32?
```

```output
lamp: short 1
11 units ordered
```
