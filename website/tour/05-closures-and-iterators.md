---
title: Closures and iterators
---

# A closure can capture a value, and an iterator waits until a result asks for values

A closure is an unnamed `fn` value. It may read a surrounding binding, as the
filter reads `threshold`. `iter()` begins a lazy chain: `filter` and `map`
describe work, while `sum()` pulls values through it. The call count is zero
before `sum()` and two afterwards, showing exactly when the closures run. A
closure's parameter and result types are checked like any function.

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

# Change the filter result type from bool to i32 and Run:
#     type-mismatch: in argument: expected fn(i32) -> bool, found fn(i32) -> i32
```

```edit
replace:         .filter(fn(amount: i32) -> bool: amount >= threshold)
with:         .filter(fn(amount: i32) -> i32: amount)
error: type-mismatch: in argument: expected fn(i32) -> bool, found fn(i32) -> i32
```

```output
map calls before sum: 0; after: 2; fees: 370 cents
```
