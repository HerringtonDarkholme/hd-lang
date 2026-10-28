---
title: Functions
---

# Functions say what they take and return, `if` chooses a value, and pipes show steps

A function writes each parameter type and its result type. The last expression
is its result. `if` may choose a value when both branches produce the same
type. A pipe feeds one result into the next step, and `_` marks the argument
slot that receives it. The pure functions return values; only the script's
top-level line prints.

```hd
fn shipping_cents(weight_grams: i32, express: bool) -> i32:
    rate := if express: +40 else: +20
    weight_grams
        |> charge(_, rate)
        |> add_fee(_, 99)

fn charge(weight_grams: i32, rate: i32) -> i32:
    weight_grams / 100 * rate

fn add_fee(cents: i32, fee: i32) -> i32:
    cents + fee

println(shipping_cents(1200, true))

# Change `charge(_, rate)` to `charge(rate)` and Run:
#     pipe-step-needs-placeholder: write `_` where the piped value goes
```

```edit
replace:         |> charge(_, rate)
with:         |> charge(rate)
error: pipe-step-needs-placeholder: write `_` where the piped value goes
```

```output
579
```
