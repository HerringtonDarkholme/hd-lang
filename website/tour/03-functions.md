---
title: Functions
---

# Functions

The program on the right works out a shipping price in three small steps.

## Parameters and results

```hd example
fn add_fee(cents: i32, fee: i32) -> i32:
    cents + fee
```

Every parameter and the result have a **type**. The **last expression** is
the result, so there is no `return`.

## `if` gives a value

```hd example
rate := if express: +40 else: +20
```

`if` picks **one of two values**, and both must have the same type.

## Pipes

```hd example
weight_grams
    |> charge(_, rate)
    |> add_fee(_, 99)
```

`|>` passes each result to the next step, and `_` marks **where it goes**.
This pipe means `add_fee(charge(weight_grams, rate), 99)`.

**Run it.** 1200 grams by express costs `12 * 40 + 99`, so it prints `579`.

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
```

```edit
replace:         |> charge(_, rate)
with:         |> charge(rate)
error: pipe-step-needs-placeholder: write `_` where the piped value goes
```

```output
579
```
