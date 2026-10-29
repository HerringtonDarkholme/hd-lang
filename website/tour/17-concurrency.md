---
title: Concurrency
---

# Concurrency

The program on the right runs two tasks together, races them, and times out a
slow one.

## `all!`

```hd example
let (a, b) = all!(first(), second())
```

`all!` starts independent tasks and returns **their values in order**. Its
arguments have no `!`. `first()` describes work that has not started, and
`all!` starts it. `first!()` would run at once and pass a string instead.

## `race!`

```hd example
println("race: ${race!(first(), second())}")
```

`race!` returns the **first value** to complete and cancels the others. It
takes calls without `!` too.

## Timeouts

```hd example
apply!(timeout(1ms)):
    let result: Result[string, TimedOut] = .Ok(too_slow!())
    result
```

`timeout(1ms)` limits how long the `apply!` block **may wait**. If time runs
out, the block gives `.Err` with a `TimedOut`. The limit is a `Duration`, so a
bare `1` is an error.

**Run it.** It prints `all: first, second` and `race: first`. `too_slow!`
sleeps 20 ms against a 1 ms limit, so the last line is `timed out after 1ms`.

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
