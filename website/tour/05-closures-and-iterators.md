---
title: Closures and iterators
---

# Closures and iterators

The program on the right adds up fees on large amounts and counts when the
work runs.

## Closures

```hd example
fees := amounts
    .iter()
    .filter(fn(amount: i32) -> bool: amount >= threshold)
```

A closure is an **unnamed `fn` value**. It can read nearby bindings, as this
one reads `threshold`. Its parameter and result types are checked like any
function's.

## Lazy iterators

```hd example
before := calls
let total: i32 = fees.sum()
```

`iter()` starts a **lazy chain**. `filter` and `map` only describe work, and
`sum()` pulls values through it. So `calls` is 0 before `sum()` and 2 after.

## Tuples

```hd example
let (before, after, total) = measure_fees([+800, +1200, +2500], +1000)
```

`measure_fees` returns **three values** as one tuple, `(i32, i32, i32)`.
`let (...) = ...` unpacks it, and `._0` reads a single position.

**Run it.** Two amounts reach 1000, and their fees are 120 and 250. So it
prints `map calls before sum: 0; after: 2; fees: 370 cents`.

```hd
fn measure_fees(amounts: List[i32], threshold: i32) -> (i32, i32, i32):
    let calls = +0
    fees := amounts
        .iter()
        .filter(fn(amount: i32) -> bool: amount >= threshold)
        .map(fn(amount: i32) -> i32:
            calls += 1
            amount / 10
        )
    before := calls
    let total: i32 = fees.sum()
    (before, calls, total)

let (before, after, total) = measure_fees([+800, +1200, +2500], +1000)
println("map calls before sum: $before; after: $after; fees: $total cents")
```

```edit
replace:         .filter(fn(amount: i32) -> bool: amount >= threshold)
with:         .filter(fn(amount: i32) -> i32: amount)
error: type-mismatch: in argument: expected fn(i32) -> bool, found fn(i32) -> i32
```

```output
map calls before sum: 0; after: 2; fees: 370 cents
```
