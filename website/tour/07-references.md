---
title: References
---

# References

The program on the right changes stock through references instead of copies.

## Reading in place

```hd example
fn units(stock: &Stock) -> i32:
    stock.units
```

A plain parameter gets a copy. `&Stock` **reads the caller's value** without
one. Writing through it, such as `stock.units = 0`, is an error.

## Writing in place

```hd example
fn restock(stock: &mut Stock, added: i32) -> void:
    stock.units += added
```

`&mut Stock` may **change the caller's value**. The call writes `&mut` too, as
in `restock(&mut local, 3)`. These local references cannot escape the call.

## Stored references

```hd example
struct SharedStock:
    view: *Stock
    editable: *mut Stock
```

A field can hold a **stored reference**. `*Stock` reads, and `*mut Stock`
also writes. A plain `Stock` value does not fit such a field.

## Shared objects

```hd example
let shared: *mut Stock = &mut Stock { units: local.units }
```

`&mut` on a new value creates a **shared object**. It stays alive while a
stored reference points to it. Both fields of `holder` point to it, so a
write through `editable` shows through `view`.

**Run it.** It prints `local: 2` before the restock. The shared stock starts
at 5 and loses 1, so it then prints `shared: 4`.

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
