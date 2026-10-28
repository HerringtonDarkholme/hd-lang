---
title: Concurrency
---

# `all!` waits for every task, `race!` keeps one winner, and a timeout is a policy

`all!` starts independent suspensions and returns their values in order.
`race!` returns the first completed value and cancels the losers. `timeout`
builds a deadline policy; `apply!` runs one result-producing operation under
it. These combinators take cold suspensions such as `first()`, not already
awaited values such as `first!()`. Because the clock is a requirement, tests
may install a manual provider instead of waiting for real time.

```hd
use std.task.{TimedOut, all, apply, race, timeout}
use std.time.{Clock, ms}

fn first!() -> string:
    "first"

fn second!() -> string:
    "second"

fn in_time!() -> Result[string, TimedOut] $ Clock:
    apply!(timeout(1ms)):
        let result: Result[string, TimedOut] = .Ok(first!())
        result

pub fn main!() -> void $ Clock + Console:
    let (a, b) = all!(first(), second())
    println("all: $a, $b")
    println("race: ${race!(first(), second())}")
    match in_time!():
        .Ok(value) => println("timeout: $value")
        .Err(error) => println(error.to_string())

# Change `first()` inside `all!` to `first!()` and Run:
#     type-mismatch:
```

```edit
replace:     let (a, b) = all!(first(), second())
with:     let (a, b) = all!(first!(), second())
error: type-mismatch:
```

```output
all: first, second
race: first
timeout: first
```
