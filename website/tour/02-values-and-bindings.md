---
title: Values and bindings
---

# Values and bindings

The program on the right counts boxes and prices them.

## Number literals

```hd example
item_cents := +1200
box_count := 3
```

`+1200` is an `i32`. A plain literal such as `3` is a **`usize`** when
nothing else sets its type. That is why `quantity` starts at `+0`: as `0`, it
would be a `usize`, and adding an `i32` would not check.

## `:=` and `let`

```hd example
let quantity = +0
quantity = quantity + i32(box_count)
```

`:=` names a value that **cannot be reassigned**. `let` declares a variable
that can be replaced as a whole.

## Conversions

```hd example
let total: i64 = i64(item_cents) * i64(quantity)
```

Number widths **never change silently**. You write each conversion, such as
`i32(...)` or `i64(...)`, where it happens. Without `i64(item_cents)`, the
product mixes `i32` and `i64` and does not check.

## Names in strings

```hd example
println("$quantity items cost $total cents")
```

`$name` puts **one binding** in a string without the braces.

**Run it.** Three boxes at 1200 cents make 3600, so it prints
`3 items cost 3600 cents`.

```hd
item_cents := +1200
box_count := 3
let quantity = +0
quantity = quantity + i32(box_count)
let total: i64 = i64(item_cents) * i64(quantity)
println("$quantity items cost $total cents")
```

```edit
replace: let quantity = +0
with: quantity := +0
error: non-reassignable-binding: `quantity` is bound with `:=`; use `let` to reassign it
```

```output
3 items cost 3600 cents
```
