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
is a placeholder name.

## The Model

Every suspension has two ways to stop, one graceful and one forced:

```text
end := job.cancel!()   # graceful: stop, run the sync cleanup, wait out the rest
job.abandon()          # forced: stop, run the sync cleanup, kill the rest
```

Both stop the job at its suspension point and run, at once, everything in
the unwinding that does not suspend: every `defer` block, which cannot
suspend, and each `dispose!` up to its first real suspension point. They
differ only at those suspension points:

- `cancel!()` waits at each one until the dispose finishes, then returns
  how the cleanup ended, `.Finished` or `.Abandoned`, so a program can log
  an abrupt close. It is a suspending call, so only a `!` function makes
  it.
- `abandon()` kills each dispose at its suspension point and returns at
  once. Any function can call it.

Called without the bang, `job.cancel()` builds a suspension of the cancel,
as every bang function does: nothing happens until it is awaited, and the
job keeps its state until then. That is how cleanups run together:
`all!([a.cancel(), b.cancel()])` cancels both and waits for both. A
discarded one is `discarded-must-use-value`, as any unused suspension is.

Two operations with different strengths keep the model simple. Cancelling
is graceful and composes: nothing in it ever drops cleanup. Abandoning is
forced, and only code that calls `abandon()` does it.

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
abandoned `b`'s cleanup instead, every nested `using` would lose its graceful close
whenever an outer deadline fired. `all!` does the same for every child.

**Cancelling a cancel adds nothing.** If the task running `job.cancel!()`
is itself cancelled, the cleanup in progress goes on as part of the outer
task's cleanup: the same cleanup continues, and no new one is made.
Cancelling a job twice waits for the same cleanup. A dispose that has
`using` or `defer` blocks of its own runs them inside the same cleanup;
they finish with it or are abandoned with it.

## Abandoning

`abandon()` runs in the caller, synchronously:

1. The job stops at its suspension point, as `cancel!()` would stop it.
2. Its synchronous cleanup runs: every `defer` block. A `dispose!` runs up
   to its first suspension point and is killed there: a pending host
   operation, such as the flush it waits on, is cancelled, and the host
   closes that handle at once. No `defer` is ever skipped, only waiting
   work.
3. If a `cancel!()` of the same job is waiting, it returns `.Abandoned`.

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

Racing does not bound it: `race!(job.cancel(), sleep!(1s))` cancels the
losing cancel, which only continues inside the race's own cleanup, and the
race then waits for it. A hard bound comes only from an explicit
`abandon()`, which the parameter performs.

## Nothing To Forget

`cancel!()` waits inside the call, so no cleanup outlives the code that
asked for it. A cold `job.cancel()` that is built and never awaited never
starts: the job keeps its state, and if it holds handles, they are closed
when the program exits and `hd test` fails the case, so the missing
`await` shows up in the first test run.

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
`cancelAndJoin` as `cancel!()`; and Trio's advice to bound a
shield with a deadline as `timeout!`'s `cleanup` parameter.

## What Changes

- The synchronous `Suspend.cancel()`
  ([`req.cancel.synchronous`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.synchronous))
  becomes two operations: the suspending `cancel!()`, which returns a
  `CleanupEnd`, and the synchronous `abandon()`.
- A new std enum `CleanupEnd` (`.Finished`, `.Abandoned`) in `std.task`.
- `timeout!` gains an optional `cleanup: Duration` parameter.
- `race!` and `all!` keep their behavior; their rules are restated with
  `cancel!()`.
- The host lists abandoned handles separately from leaked ones in
  `hd test`.
- A host's cancellation of the entry, the runtime scenarios' `hd.cancel`,
  follows the same rules with the host as the waiter.

## Open Points

1. The names `abandon`, `CleanupEnd` and the `cleanup` parameter of
   `timeout!`.
2. Whether the host's cancellation of the entry should wait for the
   entry's cleanup without a bound, or with a default one.
