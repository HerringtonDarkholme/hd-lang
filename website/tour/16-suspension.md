---
title: Suspension and all!
---

# A function that can wait says so with `!`, and so does every call to it

A call that waits, for a timer, a disk, or a server, is a point where other
work can run and the world can change. In hd such a function's name ends in
`!`, and so does each call that waits for it, so you can spot every pause
point by reading. Like `?`, the mark travels up: a function that makes a `!`
call must be a `!` function itself. Writing the call without `!`, as in
`fetch_orders(user)`, does not run it yet; `all!` takes such calls, runs them
as tasks, and waits until all are done. The clock is provided, so a test can
use a fake one.

```hd
fn dashboard!(user: string) -> string $ Clock:
    let (orders, unpaid) = all!(fetch_orders(user), fetch_unpaid(user))  # ← wait for both
    "$user: $orders orders, $unpaid unpaid invoices"

pub fn main!() -> void $ Console:
    let clock: mut ManualClock = ManualClock::new(Timestamp::from_unix_milliseconds(0))
    $.with(Clock=clock):
        println(dashboard!("ada"))

# Delete the `!` from `fn dashboard!` and Run. A function that waits
# can't pretend it doesn't:
#     bang-call-outside-suspension: a bang call requires a suspending driver context

# ── plumbing ──
use std.task.all
use std.time.{Clock, ManualClock, Timestamp, ms}

# Stand-ins for two services: each waits on the clock it is given.
fn fetch_orders!(user: string) -> i32 $ Clock:
    $.use(Clock).sleep!(120ms)
    3

fn fetch_unpaid!(user: string) -> i32 $ Clock:
    $.use(Clock).sleep!(80ms)
    1
```

```edit
replace: fn dashboard!(user: string) -> string $ Clock:
with: fn dashboard(user: string) -> string $ Clock:
error: bang-call-outside-suspension: a bang call requires a suspending driver context
```

```output
ada: 3 orders, 1 unpaid invoices
```
