---
title: Suspension
---

# Suspension

The program on the right waits one millisecond, then prints.

## Suspending functions

```hd example
fn ready!() -> string $ Clock:
    sleep!(1ms)
    "ready"
```

A timer, file or server may make a function **wait** while other work runs.
Such a function ends in `!`, and so does every call that waits, like
`sleep!(1ms)`.

## Up to `main!`

```hd example
pub fn main!() -> void $ Clock + Console:
    println(ready!())
```

The `!` travels up to **`main!`**, so every pause point is visible. `main!`
names the clock and console it uses as `$ Clock + Console`.

**Run it.** `ready!` sleeps 1 ms and returns, so it prints `ready`.

```hd
use std.time.{Clock, ms, sleep}

fn ready!() -> string $ Clock:
    sleep!(1ms)
    "ready"

pub fn main!() -> void $ Clock + Console:
    println(ready!())
```

```edit
replace: fn ready!() -> string $ Clock:
with: fn ready() -> string $ Clock:
error: bang-call-outside-suspension: a bang call needs a suspending function or closure
```

```output
ready
```
