# Cancellation And Cleanup

When a task is cancelled, its `using` values must still be disposed, and a
`dispose!` may need to wait: a socket flushes, a peer acknowledges a close.
Inside `race!`, `all!` and `timeout!` this already works, because the
combinator waits for its cancelled children to finish disposing
([`flow.using.cancel.wait`](../../spec/lang/06-control-flow.md#r-flow.using.cancel.wait)).
A stored suspension is different: its `cancel()` returns at once
([`req.cancel.synchronous`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.synchronous)),
so nothing waits for a dispose that suspends, and today that dispose is left
where it stopped until the host closes the handle at exit. This proposal
gives every cancellation one shape: a graceful cancel that composes, and a
forced abandon that is always explicit.

Hd code below is proposed syntax that no compiler checks yet; `CleanupEnd`
is a placeholder name. Twelve library-shaped cases, taken from Kotlin,
Trio, asyncio, asyncpg, hyper, contextlib and websockets, were run against
an earlier draft. The rules on cleanup order, shared cleanup, deadlines and
fan-out below come from the places where that draft broke.

## The Model

Every suspension has two ways to stop, one graceful and one forced:

```text
end := job.cancel!()   # graceful: stop, run the sync cleanup, wait out the rest
job.abandon()          # forced: stop, run the sync cleanup, kill the rest
```

Both stop the job at its suspension point and unwind it. Everything in the
unwinding that does not suspend runs at once: every `defer` block, which
cannot suspend, and each `dispose!` up to its first real suspension point.
They differ only at those suspension points:

- `cancel!()` waits at each one until the dispose finishes, then returns
  how the cleanup ended, `.Finished` or `.Abandoned`, so a program can log
  an abrupt close. It is a suspending call, so only a `!` function makes
  it.
- `abandon()` kills each dispose at its suspension point and returns at
  once. Any function can call it.

Within one job, cleanup is sequential, in the order ordinary block exit
already uses
([`flow.using.order`](../../spec/lang/06-control-flow.md#r-flow.using.order)):
last in, first out. A dispose starts only when everything registered after
it has finished. Under `cancel!()`, an outer dispose therefore waits for
an inner one that is still flushing. Cleanups of different jobs do not wait
for each other; they run side by side.

Called without the bang, `job.cancel()` builds a suspension of the cancel,
as every bang function does: nothing happens until it is awaited, and the
job keeps its state until then. That is how cleanups run together:
`all!(a.cancel(), b.cancel())` cancels both and waits for both. A discarded
one is `discarded-must-use-value`, as any unused suspension is.

Two operations with different strengths keep the model simple. Cancelling
is graceful and composes: nothing in it ever drops cleanup. Abandoning is
forced, and only code that calls `abandon()` does it.

A runtime panic is outside both: it runs no pending `defer` or dispose, as
today ([`flow.defer.panic`](../../spec/lang/06-control-flow.md#r-flow.defer.panic)).

## One Cleanup Per Job

A job's cleanup is one piece of work, and the runtime runs it. A
`cancel!()` call starts it if it has not started, then waits for it. It
does not drive the cleanup itself. So several waiters can share one
cleanup:

```text
fn stop_twice!(job: mut Suspend[void]) -> (CleanupEnd, CleanupEnd):
    all!(job.cancel(), job.cancel())    # one cleanup, two waiters
```

The close handshake runs once, and both calls return the same
`CleanupEnd`. A `cancel!()` made after the cleanup ended returns how it
ended, `.Abandoned` included. A job that finished on its own has nothing
left to clean up, so cancelling it returns `.Finished`.

A job may already be disposing when the cancel arrives, because its body
reached `return`, `?` or the end of a `using` block. The cancel then takes
over that dispose: the close frame already sent stays sent, the dispose
goes on from where it is, and the cancel waits for it. Starting the
dispose again would send a second close, and dropping it would break the
graceful promise.

A cancelled waiter only stops waiting. The cleanup goes on, and the
waiter's own task folds it into its cleanup, as the next section says.

## Composition

**A cancelled parent never abandons a child's cleanup.** It folds the
child's cleanup into its own. Walk through `race!(a, b)` where `a` wins:

```text
t0  a and b run
t1  a finishes          race! calls b.cancel!(): b stops
t2  b's dispose! flushes its socket; race! waits
t3  the cleanup ends    race! returns a's value
```

If the race is itself cancelled at t2, say by an outer `timeout!`, the race
stops and its own cleanup includes finishing `b`'s. Had the outer cancel
abandoned `b`'s cleanup instead, every nested `using` would lose its
graceful close whenever an outer deadline fired.

**Every child is stopped before any is waited for.** When `race!` or `all!`
cancels its unfinished children, it starts each child's cleanup, in
argument order, and only then waits for all of them. The parent's own
cleanup runs after every child's. The order matters for a server shutdown:

```text
fn serve!(connections: mut Suspend[void], acceptor: mut Suspend[void]) -> void:
    _ := all!(connections, acceptor)
```

The connections' dispose waits until the acceptor stops accepting, and the
acceptor's `defer` is what stops it. A parent that waited out the
connections before cancelling the acceptor would never reach that `defer`.
Stopping both first lets the acceptor's `defer` run while the connections
wait. A dispose that waits for its own parent's `defer` still never ends,
because the parent cleans up last; that is a bug in the program, and no
order fixes it.

**Cancelling a cancel adds nothing.** If the task running `job.cancel!()`
is itself cancelled, the cleanup in progress goes on as part of the outer
task's cleanup: the same cleanup continues, and no new one is made.

## Abandoning

`abandon()` runs in the caller, synchronously. It unwinds the job's stack
once, innermost frame first, as one last-in-first-out list:

1. Every `defer` registered so far runs once, in reverse order of
   registration, including the `defer` blocks inside a `dispose!` that is
   already running. A `defer` written after a suspension point the job
   never reached was never registered and does not run.
2. A `dispose!` that was waiting is killed at its suspension point, after
   its own registered `defer` blocks run. A pending host operation, such as
   the flush it waits on, is cancelled, and the host closes that handle.
3. A `using` value whose dispose has not started yet starts it now; the
   dispose runs to its first suspension point and is killed there in the
   same way.
4. If a `cancel!()` of the same job is waiting, it returns `.Abandoned`,
   and so does every later one.

Mixed `defer` and `using` blocks keep their written order under
abandonment:

```text
fn ordered!(res: mut Release, log: mut List[string], pause: fn!() -> void) -> void:
    defer: log.push("outer")
    using r = res
    defer: log.push("tail")
    pause!()
```

Abandoned at `pause!()`, the log reads `tail`, then whatever `r`'s dispose
records before its first wait, including that dispose's own `defer`
blocks, then `outer`.

**Abandonment releases what synchronous code releases.** Killing a wait
closes the handle that the wait was on, and nothing else. A dispose may be
waiting on something that is not a handle at all. A pool, for example,
first waits for its borrowed connections to come back, then closes its
idle sockets. Killing that wait identifies no socket. The host cannot tell
which handles the pool owns either; closing every handle reachable from
it would also close connections it only borrowed.

So a dispose that holds handles across a wait registers a synchronous
fallback before it waits:

```text
impl Disposable for Pool:
    fn dispose!(mut self) -> void:
        defer: self.close_idle()       # runs on graceful end and on abandon
        self.wait_released!()
```

The std wrappers of host handles do this. A handle that a dispose forgot to
cover is still open when the case ends, and `hd test` reports it as
leaked, so the missing fallback shows up in the first test that abandons
the resource.

Tasks run cooperatively on one thread, so all of this finishes inside the
`abandon()` call before any waiter runs again. Abandoning a job that
already ended, or abandoning it twice, does nothing.

## Deadlines

`timeout!` cancels gracefully: when the limit passes, its work is cancelled
and `timeout!` waits for the cleanup, which can take longer than the limit.
A bound on the cleanup is a parameter: `timeout!` runs `cancel!()` under the
bound and calls `abandon()` when it passes.

```text
timeout!(5s, cleanup = 1s): fetch_all!()   # cancel at 5 s; abandon what is left 1 s later
```

The bound is part of the cleanup it bounds, not an ordinary child that
cancellation can stop. It is one fixed deadline, set when the work is first
cancelled, by the limit or from outside:

```text
t0    timeout! starts its work
t5    the limit passes: the work is cancelled; the deadline is set to t6
t5.2  the host cancels the whole request: the cleanup goes on, deadline still t6
t6    the close has not finished: timeout! abandons it
```

If the outside cancel came first, at t3, the deadline would be t4. A later
cancel never moves the deadline, so cancelling again and again cannot
stretch the bound.

Racing does not bound a cleanup: `race!(job.cancel(), sleep!(1s))` cancels
the losing cancel, which only continues inside the race's own cleanup, and
the race then waits for it. A hard bound comes only from an explicit
`abandon()`, which the parameter performs.

## Seeing An Abrupt Close

Three places learn that a cleanup was abandoned:

- A direct `cancel!()` caller gets `.Abandoned`.
- The code that calls `abandon()` knows it did, and logs it there if it
  cares.
- `timeout!` is such a caller, so its error says how the cleanup ended:
  `TimedOut` gains a `cleanup: CleanupEnd` field next to `limit`.

`race!` and `all!` keep their result types. A loser whose cleanup was cut
short by an outer abandon shows up at that outer abandon, not in the
race's value.

## Nothing To Forget

`cancel!()` waits inside the call, so no cleanup outlives the code that
asked for it. A cold `job.cancel()` that is built and never awaited never
starts, and the compiler reports it as an unused suspension. One that is
explicitly discarded with `_ :=` leaves the job in its state. If that job
holds host handles, they are closed when the program exits and `hd test`
fails the case, so the missing `await` shows up in the first test run. A
job holding only in-memory resources leaves nothing for the host to see;
a test of such a resource asserts its state itself.

Handles closed by an explicit `abandon()` are intended. `hd test` lists them
as abandoned, not leaked, and does not fail the case for them.

## How Other Languages Do It

Languages whose cancellation interrupts at suspension points all protect
cleanup that waits, and all wait for it at a structured point:

| Language | Cleanup that waits | Who waits |
| --- | --- | --- |
| Kotlin | `withContext(NonCancellable)` in `finally` | `cancelAndJoin()` |
| Trio | `CancelScope(shield=True)`, usually with a deadline inside | the nursery |
| Python asyncio | `await` in `__aexit__` and `finally`; `asyncio.shield` | awaiting the cancelled task |
| MoonBit | no async in `defer`; `protect_from_cancel` as an escape hatch | `with_task_group` |

Where cancellation is cooperative, as in JavaScript (`await using`), C#
(`DisposeAsync`) and Go (`defer`), cleanup that waits simply runs. Swift's
`defer` cannot `await`, and Rust has no async drop. This design takes the
shield from Kotlin and Trio, built in for `dispose!`; Kotlin's
`cancelAndJoin` as `cancel!()`; Trio's shielded deadline as `timeout!`'s
`cleanup` parameter; and asyncpg's split between a graceful `close()` and
a synchronous `terminate()` as `cancel!()` and `abandon()`.

## What Changes

- The synchronous `Suspend.cancel()`
  ([`req.cancel.synchronous`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.synchronous))
  becomes two operations: the suspending `cancel!()`, which returns a
  `CleanupEnd`, and the synchronous `abandon()`.
- The runtime keeps one cleanup per job, with any number of waiters and
  the final `CleanupEnd` kept for later calls.
- A new std enum `CleanupEnd` (`.Finished`, `.Abandoned`) in `std.task`.
- `timeout!` gains an optional `cleanup: Duration` parameter, and
  `TimedOut` a `cleanup: CleanupEnd` field.
- `race!` and `all!` keep their results; their rules are restated with
  `cancel!()`, and they start every child's cleanup before waiting for
  any.
- Std wrappers of host handles register a synchronous fallback before
  their dispose waits.
- The host lists abandoned handles separately from leaked ones in
  `hd test`.
- A host's cancellation of the entry, the runtime scenarios' `hd.cancel`,
  follows the same rules with the host as the waiter.

## Open Points

1. The names `abandon`, `CleanupEnd` and the `cleanup` parameter of
   `timeout!`.
2. Whether the host's cancellation of the entry should wait for the
   entry's cleanup without a bound, or with a default one. With the
   deadline rule above, a default would behave as `timeout!`'s `cleanup`
   does: set once at the first cancel, never moved.
