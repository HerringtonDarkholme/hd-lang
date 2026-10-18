# Cancellation And Cleanup

When a task is cancelled, its `using` values must still be disposed, and a
`dispose!` may need to wait: a socket flushes, a peer acknowledges a close.
Inside `race!`, `all!` and `timeout!` this already works, because the
combinator waits for its cancelled children to finish disposing
([`flow.using.cancel.wait`](../../spec/lang/06-control-flow.md#r-flow.using.cancel.wait)).
A stored suspension was different: its `cancel()` returned at once,
so nothing waited for a dispose that suspends, and that dispose was left
where it stopped until the host closed the handle at exit. This proposal
gives every cancellation one shape: a graceful cancel that composes, and a
forced abandon that is always explicit.

Hd code below is proposed syntax that no compiler checks yet. Twelve library-shaped cases, taken from Kotlin,
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
  an abrupt close. `.Finished` means the whole cleanup ran to its end,
  including every child cleanup folded into it, with nothing abandoned;
  if any part was abandoned, the result is `.Abandoned`. It does not mean
  every close succeeded: a dispose returns `void` and does its best, so
  code that must know a close protocol succeeded calls an explicit close
  that returns a `Result`. It is a suspending call, so only a `!`
  function makes it.
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
left to clean up, so cancelling it returns how its own cleanup ended:
`.Finished`, unless a dispose of its was cut short (below).

A job may already be disposing when the cancel arrives, because its body
reached `return`, `?` or the end of a `using` block. The cancel then takes
over that dispose: the close frame already sent stays sent, the dispose
goes on from where it is, and the cancel waits for it. Starting the
dispose again would send a second close, and dropping it would break the
graceful promise.

A cancelled waiter only stops waiting. The cleanup goes on, and the
waiter's own task folds it into its cleanup, as the next section says. If
that task is later abandoned, the folded cleanup is abandoned with it, and
every other waiter of that cleanup gets `.Abandoned` too. Any code holding
the job could abandon it directly, so a fellow waiter loses nothing it
could rely on, and the bound on the abandoning task really ends the
cleanup it was waiting for.

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

**Awaiting a cancelled job cancels the awaiter.** One frame may be waiting
for a job's value while another cancels the job:

```text
fn supervise!(job: mut Suspend[void]) -> void $ Clock:
    _ := all!(job, stop_later(job))     # stop_later cancels job after 1 s
```

The value `all!` waits for will never come. So the frame awaiting it is
cancelled too, as asyncio raises `CancelledError` at such an `await`:
`all!` stops, and its cleanup, which includes the job's, unwinds as any
cancellation does. No value is invented and nothing panics.

A `dispose!` is shielded from the cancellation of its own task, but no
shield can produce a value that will never arrive. So when a dispose
awaits a job that was cancelled elsewhere (a shared acknowledgement it
needs before sending its close frame, say), that dispose is unwound from
the await exactly as `abandon()` would unwind it: its registered `defer`
blocks run, the code after the await is skipped, and its cleanup ends
`.Abandoned`, which carries up as any abandonment does. Cancelling the
task itself still never drops cleanup; here a dependency of the cleanup
failed, and `.Abandoned` says the close did not finish. This is the one
way a cleanup ends early without an explicit `abandon()`. If the dispose
ran after an ordinary return, the job still completed: its value is
delivered, and only its kept cleanup result is `.Abandoned`. Sharing a
suspension this way is rare under structured concurrency, so nothing more
reports it.

`block_on` is the one driver whose caller cannot be cancelled: it is
synchronous code expecting a value. So `block_on` returns a `BlockOn[T]`
that names all three ways the drive can end:

```text
match block_on(call):
    .Completed(value) => use(value)
    .Cancelled => pass        # cancelled; its cleanup ran to its end
    .Abandoned => pass        # cancelled; its cleanup was cut short
```

The suspension can only be cancelled by code running inside the same
drive that holds another reference to it; `block_on` then drives the
cleanup to its end, or to its abandonment, before it returns.

**A resource a combinator has already won has no owner yet.** In
`using c = race!(open_a(), open_b())`, `race!` holds the winner while it
waits for the loser's cleanup; if the parent is cancelled during that
wait, the winner was never bound by `using`, so nothing disposes it.
`all!` has the same gap for a value that completed while a sibling is
still pending. Bind each resource with `using` inside the child that
opens it and return plain values. Under the planned counted object model
the gap is at least loud: a `Disposable` whose count reaches zero without
being disposed is reported as a leak at once, with its site.

```text
fn with_connection![T](open: fn!() -> mut Connection, work: fn!(mut Connection) -> T) -> T:
    using connection = open!()
    work!(connection)
```

Acquiring resources one after another with `using` avoids the gap too, at
the cost of concurrency.

## Abandoning

`abandon()` runs in the caller, synchronously. It unwinds the job's stack
once, innermost frame first, as one last-in-first-out list:

1. Every `defer` registered so far runs once, in reverse order of
   registration, including the `defer` blocks inside a `dispose!` that is
   already running. A `defer` written after a suspension point the job
   never reached was never registered and does not run.
2. A `dispose!` that was waiting is killed at its suspension point, after
   its own registered `defer` blocks run. A pending host operation, such as
   the read or flush it waits on, is aborted as that operation's own
   cancellation rule says (a cancelled read keeps its unread bytes). The
   handle itself stays open.
3. A `using` value whose dispose has not started yet starts it now; the
   dispose runs to its first suspension point and is killed there in the
   same way.
4. Every cleanup folded into the job is abandoned the same way: the
   cleanups of children that `race!` or `all!` had started, and a shared
   cleanup the job was waiting on. Abandonment follows exactly the links
   that cancellation folds.
5. If a `cancel!()` of the same job is waiting, it returns `.Abandoned`,
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
aborts the operation it waited on and closes nothing. The handle a dispose
waits on may not even be its own: a logical channel may wait for an
acknowledgement on a control socket that other channels share, and
closing it would cut them all off. A dispose may also be waiting on
something that is not a handle at all. A pool, for example,
first waits for its borrowed connections to come back, then closes its
idle sockets. Killing that wait identifies no socket. The host cannot tell
which handles the pool owns either; closing every handle reachable from
it would also close connections it only borrowed.

So the code that owns a handle decides what is closed: a dispose that holds
handles across a wait registers a synchronous fallback before it waits:

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

The bound is cooperative, like everything on one thread. The deadline
calls `abandon()` at the first moment at or after it when the thread is
free, which is the next suspension point. Synchronous code is never
interrupted: a dispose that computes for two seconds before its first wait
overruns a one-second bound, and `abandon()` itself still runs every
registered `defer`, however long each takes. So the bound limits waiting,
not computation. The one hard wall-clock limit is the host's time limit,
which panics and, like any panic, runs no pending cleanup.

Racing does not bound a cleanup: `race!(job.cancel(), sleep!(1s))` cancels
the losing cancel, which only continues inside the race's own cleanup, and
the race then waits for it. Only an explicit `abandon()` ends a cleanup
early, and the parameter performs one.

The `cleanup` bound is extra time after the work is cancelled, not a total
budget. A close that was already waiting when the limit passed keeps the
time it has used and then gets the whole bound. A total limit on waiting
is `cleanup = 0s`: cancel at the limit, then abandon whatever still waits.

```text
timeout!(1s, cleanup = 0s): close!()   # waits at most 1 s in all, cooperatively
```

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

Because `.Abandoned` carries up through folded cleanups, a parent reports
it when any child's cleanup was cut short, even if the parent and its
other children finished gracefully. With structured concurrency this is
rare: a child is normally abandoned only through its parent. The rule
keeps `TimedOut.cleanup` from saying `.Finished` over a forced close when
it does happen.

## Nothing To Forget

`cancel!()` waits inside the call, so no cleanup outlives the code that
asked for it. A cold `job.cancel()` that is built and never awaited never
starts, and the compiler reports it as an unused suspension. One that is
explicitly discarded with `_ :=` leaves the job in its state. If that job
holds host handles, they are closed when the program exits and `hd test`
fails the case, so the missing `await` shows up in the first test run. A
job holding only in-memory resources leaves nothing for the host to see;
a test of such a resource asserts its state itself.

`abandon()` itself closes no handle. Handles that their owners' fallbacks
close during an abandon are closed as usual, and a handle still open when
a case ends is leaked, abandoned or not, so an abandon never hides a
missing fallback.

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

- The synchronous `Suspend.cancel()` becomes two operations: the suspending `cancel!()`, which returns a
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
- `abandon()` aborts pending host operations by their own cancellation
  rules and closes no handle; owners close handles through their
  fallbacks.
- Awaiting the value of a job that another frame cancelled cancels the
  awaiter; inside a `dispose!` it unwinds that dispose as an abandon
  would, with `.Abandoned`.
- `block_on` returns a new std enum `BlockOn[T]` in `std.task`
  (`.Completed(T)`, `.Cancelled`, `.Abandoned`). `println` and `eprintln`
  hold the only reference to their write, so it cannot be cancelled; they
  ignore the other two variants. A custom provider whose `write_line!`
  cancels its own work loses that line; that is the provider's bug.
- A host's cancellation of the entry (Ctrl-C, the runtime scenarios'
  `hd.cancel`) follows the same rules with the host as the waiter. The
  host waits with a default cleanup bound, a host setting that behaves as
  `timeout!`'s `cleanup` does: set once at the first cancel, never moved,
  then `abandon()`. A second Ctrl-C abandons at once.
