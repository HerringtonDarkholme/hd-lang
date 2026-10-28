---
title: Values and bindings
---

# `:=` binds once, while `let` names a value that may be replaced

`:=` gives a computed value a name that cannot be reassigned. `let` declares a
variable that can be reassigned as a whole. `+0` is an `i32`; an unsuffixed
nonnegative literal such as `3` is a `usize` when nothing else sets its type.
Number widths never change silently, so conversions such as `i32(...)` and
`i64(...)` are written where they happen.

```hd
item_cents := +1200
box_count := 3
let quantity = +0
quantity = quantity + i32(box_count)
let total: i64 = i64(item_cents) * i64(quantity)
println("$quantity items cost $total cents")

# Replace `let quantity = +0` with `quantity := +0` and Run:
#     non-reassignable-binding: `quantity` is bound with `:=`; use `let` to reassign it
# Change `+0` to `0` and Run:
#     mixed-signedness: in operand: expected usize, found i32
# Delete `i64(item_cents)` and Run:
#     type-mismatch: in operand: expected i32, found i64
```

```edit
replace: let quantity = +0
with: quantity := +0
error: non-reassignable-binding: `quantity` is bound with `:=`; use `let` to reassign it
```

```output
3 items cost 3600 cents
```
