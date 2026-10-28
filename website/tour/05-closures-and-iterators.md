---
title: Closures and iterators
---

# A closure can capture a value, and an iterator waits for a terminal operation

A closure is an unnamed `fn` value. It may read a surrounding binding, as the
filter reads `threshold`. `iter()` begins a lazy chain: `filter` and `map`
describe work, while the terminal `sum()` pulls values through the chain.
The closure parameter and result types are checked like any function.

```hd
amounts := [+800, +1200, +2500]
threshold := +1000
fees := amounts
    .iter()
    .filter(fn(amount: i32) -> bool: amount >= threshold)
    .map(fn(amount: i32) -> i32: amount / 10)
let total: i32 = fees.sum()
println("fees: $total cents")

# Change the filter result type from bool to i32 and Run:
#     type-mismatch: in argument: expected fn(i32) -> bool, found fn(i32) -> i32
```

```edit
replace:     .filter(fn(amount: i32) -> bool: amount >= threshold)
with:     .filter(fn(amount: i32) -> i32: amount)
error: type-mismatch: in argument: expected fn(i32) -> bool, found fn(i32) -> i32
```

```output
fees: 370 cents
```
