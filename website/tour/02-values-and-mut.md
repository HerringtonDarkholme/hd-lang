---
title: Values and mut
---

# Only a `mut` value can change, so every change is easy to find

`:=` binds a name that cannot be reassigned, and `let` declares a variable that
can be. For records and lists, whether a value may be changed is part of its
type: an `Order` is read-only, a `mut Order` can be changed, and `let mut` asks
for the changeable one. So `receipt(order: Order)` promises in its signature
that it only reads the order, and the compiler holds it to that promise. To find
every place an order can change, search for `mut Order`.

```hd
fn apply_discount(order: mut Order, percent: i32) -> void:  # ← changes the order
    order.total_cents = order.total_cents * (100 - percent) / 100

fn receipt(order: Order) -> string:  # ← no mut: can only read it
    "${order.id}: ${order.total_cents} cents"

pub fn main() -> void $ Console:
    let mut order = Order { id: "A-1042", total_cents: 5000 }
    apply_discount(order, 10)
    println(receipt(order))

# Delete `mut` from apply_discount's parameter and Run:
#     readonly-root: field 'total_cents' cannot be assigned through readonly type 'Order'

# ── plumbing ──
data Order:
    id: string
    total_cents: i32
```

```edit
replace: fn apply_discount(order: mut Order, percent: i32) -> void:  # ← changes the order
with: fn apply_discount(order: Order, percent: i32) -> void:
error: readonly-root: field 'total_cents' cannot be assigned through readonly type 'Order'
```

```output
A-1042: 4500 cents
```
