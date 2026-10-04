---
title: Functions
---

# A public signature is a contract, checked on both sides

A public function spells out its parameter types and its result type. Inside
the body, types are inferred: `base` is an `i32` without saying so. Because the
contract is written down, a mistake in the body is reported in the body, not at
every caller, and a caller that passes the wrong type is stopped at the call.
Parameters may have defaults, and a named argument such as `express=true` says
what a bare `true` would hide.

```hd
pub fn shipping_cents(weight_grams: i32, express: bool = false) -> i32:
    base := 499 + weight_grams / 100 * 25  # ← inferred: i32
    if express: base * 2 else: base

pub fn main() -> void $ Console:
    println(shipping_cents(1200))
    println(shipping_cents(1200, express=true))

# Make the body's last line `"free"` and Run. The error is
# in the body, not at the callers:
#     type-mismatch: expected i32, found string
```

```edit
replace:     if express: base * 2 else: base
with:     "free"
error: type-mismatch: expected i32, found string
```

```output
799
1598
```
