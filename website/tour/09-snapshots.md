---
title: Lists are values
---

# A copy of a list never changes when the original does, and copying is cheap

A `List`, a `Map` and a `string` are values, like a struct. `saved := cart` is an
undo point: pushing onto `cart` afterwards leaves `saved` alone, and a function
that is given a cart can build its own changed cart without touching the
caller's. Copies share their storage until one of them changes, and the change
copies it first, so keeping a history of a thousand carts costs one copy per
edit, not per snapshot. To change the caller's list on purpose, pass
`&mut cart.items`, as a struct field is a place like any other, and write the
`&mut` at the call.

```hd
fn with_item(cart: Cart, item: string) -> Cart:
    let mut next = cart  # ← the function's own copy
    next.items.push(item)
    next

fn add_gift(items: &mut List[string]) -> void:
    items.push("gift card")

pub fn main() -> void $ Console:
    let mut cart = Cart { items: ["lamp"] }
    saved := cart  # ← an undo point
    cart.items.push("bulb")
    println("${saved.items.len()} ${cart.items.len()}")
    updated := with_item(cart, "shade")
    println("${cart.items.len()} ${updated.items.len()}")
    add_gift(&mut cart.items)  # ← the caller's list changes, and the call says so
    println(cart.items.len())

# Delete `&mut ` from the `add_gift` call and Run. A list is a
# value, so passing one hands over a copy:
#     type-mismatch: in argument: expected &mut List[string], found List[string]

# ── plumbing ──
struct Cart:
    items: List[string]
```

```edit
replace:     add_gift(&mut cart.items)  # ← the caller's list changes, and the call says so
with:     add_gift(cart.items)
error: type-mismatch: in argument: expected &mut List[string], found List[string]
```

```output
1 2
2 3
3
```
