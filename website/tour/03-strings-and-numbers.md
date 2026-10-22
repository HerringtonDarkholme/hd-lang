---
title: Strings and numbers
---

# A number never changes type on its own, so every conversion is in the code

A number type names its width: `i32`, `i64`, `u8`, `f64`, and `usize` for
lengths and indices. hd never converts between them for you, not even from
`i32` to the wider `i64`, so `i64(quantity)` shows each place a value
changes type. A literal takes its type from its use, and `_` groups digits,
as in `1_250`. Text that should be a number is parsed, and parsing can fail,
so `text.parse::[i32]()` gives a `Result`. A string interpolates `$name` and
`${expression}`; a `$` before anything else is plain text, so `$${...}`
prints a dollar sign before the value. A range index slices: `entry[0..3]`
is the first three bytes, and `entry[4..]` the rest from byte 4.

```hd
fn order_line(entry: string, unit_cents: i64) -> string:
    sku := entry[0..3]  # ← a range index slices: bytes 0, 1, 2
    quantity := (entry[4..]).parse::[i32]().unwrap_or(1)  # ← text to number, which may fail
    total := unit_cents * i64(quantity)  # ← i32 to i64: you write the widening
    cents := (total % 100).to_string().pad_start(2, '0')
    "$sku x$quantity: $${total / 100}.$cents"

pub fn main() -> void $ Console:
    println(order_line("LMP 3", 1_250))  # ← `_` groups digits
    println(order_line("BLB 12", 75))
    println(order_line("CBL x", 1_999))

# Change `i64(quantity)` to `quantity` and Run. hd never converts
# a number for you, not even to a wider type:
#     type-mismatch: in operand: expected i64, found i32

# ── plumbing ──
```

```edit
replace:     total := unit_cents * i64(quantity)  # ← i32 to i64: you write the widening
with:     total := unit_cents * quantity
error: type-mismatch: in operand: expected i64, found i32
```

```output
LMP x3: $37.50
BLB x12: $9.00
CBL x1: $19.99
```
