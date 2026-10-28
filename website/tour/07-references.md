---
title: References
---

# `&` reads in place, `&mut` writes in place, and `*` keeps sharing

A value passed normally is copied. `&T` lends one place to a call for reading,
and `&mut T` lends a writable place. These local references cannot escape the
call. A stored `*T` or `*mut T` is counted and may live in a field. Here the
viewer reads shared stock while the lane may update the same object.
`for item in &mut items` and `&mut items[index]` borrow element places; while
such a reference lives, the collection stays pinned and cannot grow or shrink.

```hd
struct Stock:
    units: i32

struct Viewer:
    stock: *Stock

struct Lane:
    stock: *mut Stock

fn units(stock: &Stock) -> i32:
    stock.units

fn restock(stock: &mut Stock, added: i32) -> void:
    stock.units += added

let mut local = Stock { units: 2 }
println("local: ${units(&local)}")
restock(&mut local, 3)
let shared: *mut Stock = &mut Stock { units: local.units }
viewer := Viewer { stock: shared }
lane := Lane { stock: shared }
lane.stock.units -= 1
println("shared: ${viewer.stock.units}")

# Replace the counted reference with a plain Stock value and Run:
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
