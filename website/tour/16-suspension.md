---
title: Suspension
---

# A function that may wait ends in `!`, and every waiting call shows `!`

A timer, file, or server may suspend while other work runs. A suspending
function ends in `!`, and so does every call that waits for it. The marker
travels up to `main!`, making every pause point visible. This entry point names
the clock and console it uses directly as `$ Clock + Console`.

```hd
use std.time.{Clock, ms, sleep}

fn ready!() -> string $ Clock:
    sleep!(1ms)
    "ready"

pub fn main!() -> void $ Clock + Console:
    println(ready!())

# Delete `!` from the declaration of `ready!` and Run:
#     bang-call-outside-suspension: a bang call needs a suspending function or closure
```

```edit
replace: fn ready!() -> string $ Clock:
with: fn ready() -> string $ Clock:
error: bang-call-outside-suspension: a bang call needs a suspending function or closure
```

```output
ready
```
