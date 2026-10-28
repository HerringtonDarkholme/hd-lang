---
title: Values and bindings
---

# `:=` binds once, while `let` names a value that may be replaced

`:=` gives a computed value a stable name. `let` declares a variable that can
be reassigned as a whole. A signed integer literal such as `+2` is an `i32`;
an unsuffixed nonnegative literal such as `2` is a `usize` when nothing else
sets its type. Number widths never change silently, so the total uses the
written conversion `i64(...)`. Numeric literals may group digits with `_`.
Text-to-number parsing returns a `Result`, introduced in lesson 10, and string
ranges count UTF-8 bytes; the guide collects the full string surface.

```hd
item_cents := +1200  # ← fixed i32 binding
let quantity = +2    # ← reassignable i32 variable
quantity = quantity + 1
let total: i64 = i64(item_cents) * i64(quantity)
println("$quantity items cost $total cents")

# Replace `let quantity = +2` with `quantity := +2` and Run:
#     non-reassignable-binding: `quantity` is bound with `:=`; use `let` to reassign it
```

```edit
replace: let quantity = +2    # ← reassignable i32 variable
with: quantity := +2
error: non-reassignable-binding: `quantity` is bound with `:=`; use `let` to reassign it
```

```output
3 items cost 3600 cents
```
