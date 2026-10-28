---
title: Concurrency
---

# `all!` waits for every task, `race!` keeps one winner, and a timeout limits waiting

`all!` starts independent suspensions and returns their values in order.
`race!` returns the first completed value and cancels the losers. `timeout`
sets how long `apply!` may wait. These combinators take a call written without
`!`, such as `first()`: it describes work that has not started yet, and the
combinator starts it. Writing `first!()` starts it immediately and produces a
string instead of work the combinator can start.

```hd
use std.task.{TimedOut, all, apply, race, timeout}
use std.time.{Clock, ms, sleep}

fn first!() -> string:
    "first"

fn second!() -> string:
    "second"

fn too_slow!() -> string $ Clock:
    sleep!(20ms)
    "late"

fn with_timeout!() -> Result[string, TimedOut] $ Clock:
    apply!(timeout(1ms)):
        let result: Result[string, TimedOut] = .Ok(too_slow!())
        result

pub fn main!() -> void $ Clock + Console:
    let (a, b) = all!(first(), second())
    println("all: $a, $b")
    println("race: ${race!(first(), second())}")
    match with_timeout!():
        .Ok(value) => println("timeout: $value")
        .Err(error) => println(error.to_string())

# Change `first()` inside `all!` to `first!()` and Run:
#     type-mismatch: in argument: expected a cold suspension, found string
# Change `first()` inside `race!` to `first!()` and Run:
#     type-mismatch: in argument: expected mut Suspend[?], found string
# Change `timeout(1ms)` to `timeout(1)` and Run:
#     type-mismatch: in argument: expected Duration, found an integer literal
```

```edit
replace:     let (a, b) = all!(first(), second())
with:     let (a, b) = all!(first!(), second())
error: type-mismatch: in argument: expected a cold suspension, found string
```

```output
all: first, second
race: first
timed out after 1ms
```
