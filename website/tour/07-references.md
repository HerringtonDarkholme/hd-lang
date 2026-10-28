---
title: References
---

# `&` reads in place, `&mut` writes in place, and `*` keeps sharing

A value passed normally is copied. `&T` reads the caller's value without a
copy, and `&mut T` may change that value. These local references cannot escape
the call. A stored `*T` or `*mut T` may live in a field. `*T` reads; `*mut T`
also writes. Writing `&mut` on a new value creates a shared object that stays
alive while a stored reference points to it.

```hd
struct Stock:
    units: i32

struct SharedStock:
    view: *Stock
    editable: *mut Stock

fn units(stock: &Stock) -> i32:
    stock.units

fn restock(stock: &mut Stock, added: i32) -> void:
    stock.units += added

let mut local = Stock { units: 2 }
println("local: ${units(&local)}")
restock(&mut local, 3)
let shared: *mut Stock = &mut Stock { units: local.units }
holder := SharedStock { view: shared, editable: shared }
holder.editable.units -= 1
println("shared: ${holder.view.units}")

# Add `stock.units = 0` inside `units` and Run:
#     not-writable: this writes through a read-only reference &Stock
# Change `restock(&mut local, 3)` to `restock(local, 3)` and Run:
#     type-mismatch: in argument: expected &mut Stock, found Stock
# Replace the stored reference with a plain Stock value and Run:
#     type-mismatch: in field: expected *Stock, found Stock
```

```edit
replace: let shared: *mut Stock = &mut Stock { units: local.units }
with: shared := Stock { units: local.units }
error: type-mismatch: in field: expected *Stock, found Stock
```

```output
local: 2
shared: 4
```
