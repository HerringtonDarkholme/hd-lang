---
title: Values and mut
---

# A struct is a value, so a function changes yours only if you hand it over with `&mut`

`:=` binds a name that cannot be reassigned, `let` declares a variable that can
be reassigned as a whole, and `let mut` also lets its fields change in place. A
struct is a value: `before := order` is a copy, and a parameter `order: Order`
is the function's own copy, so `receipt` cannot change yours however it is
written. A function that should change the caller's order says so in its
signature, `order: &mut Order`, and the caller says so again at the call,
`&mut order`, which needs a `let mut` variable. To find every place an order can
change, search for `&mut`.

```hd
fn apply_discount(order: &mut Order, percent: i32) -> void:  # ← changes the caller's order
    order.total_cents = order.total_cents * (100 - percent) / 100

fn receipt(order: Order) -> string:  # ← its own copy: nothing it does reaches the caller
    "${order.id}: ${order.total_cents} cents"

pub fn main() -> void $ Console:
    let mut order = Order { id: "A-1042", total_cents: 5000 }
    before := order  # ← a copy: later changes do not reach it
    apply_discount(&mut order, 10)
    println(receipt(before))
    println(receipt(order))

# Change `&mut Order` to `&Order` in apply_discount's parameter and Run:
#     readonly-root: this writes through a read-only reference &Order; take the reference with `&mut` to write through it

# ── plumbing ──
struct Order:
    id: string
    total_cents: i32
```

```edit
replace: fn apply_discount(order: &mut Order, percent: i32) -> void:  # ← changes the caller's order
with: fn apply_discount(order: &Order, percent: i32) -> void:
error: readonly-root: this writes through a read-only reference &Order; take the reference with `&mut` to write through it
```

```output
A-1042: 5000 cents
A-1042: 4500 cents
```
