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

Hd code below is proposed syntax that no compiler checks yet; `Cleanup`
and `CleanupEnd` are placeholder names.

## The Model

Cancelling a job stops it at its suspension point and hands back its
cleanup:

```text
c: Cleanup := job.cancel()      # synchronous: the job stops where it waited
end := c.await!()               # waits until every dispose has finished
c.abandon()                     # synchronous and final: stop what is left
```

`cancel()` first runs, at once, everything in the unwinding that does not
suspend: every `defer` block, which cannot suspend, and each `dispose!` up
to its first real suspension point. What is left, the waiting part of
graceful closes, is the `Cleanup`.

A `Cleanup` has two operations and no `cancel()`:

- `await!()` waits for the rest of the cleanup and returns how it ended,
  `.Finished` or `.Abandoned`, so a program can log an abrupt close.
- `abandon()` stops it now. It returns at once.

Two operations with different strengths keep the model simple. Cancelling
is graceful and composes: nothing in it ever drops cleanup. Abandoning is
forced, and only code that names a `Cleanup` and calls `abandon()` does it.

## Composition

**A cancelled parent never abandons a child's cleanup.** It folds the
child's cleanup into its own. Walk through `race!(a, b)` where `a` wins:

```text
t0  a and b run
t1  a finishes          race! calls b.cancel(): b stops, its cleanup is cb
t2  race! awaits cb     b's dispose! flushes its socket
t3  cb ends             race! returns a's value
```

If the race is itself cancelled at t2, say by an outer `timeout!`, the race
stops and its own cleanup includes finishing `cb`. Had the outer cancel
abandoned `cb` instead, every nested `using` would lose its graceful close
whenever an outer deadline fired. `all!` does the same for every child.

**There is one cleanup per cancelled job.** Waiting on a cleanup holds no
resources of its own, only responsibility for it. Cancelling the wait hands
that responsibility back, so cancelling a stored wait on `c` returns `c`
itself, the same object with the same pending disposes:

```text
c := job.cancel()
w := c.await            # a stored suspension of the wait
w.cancel()              # returns c again
```

Many waits on one cleanup are views of it: when it ends, they all
complete, and `abandon()` through any of them ends the one cleanup. A
dispose that has `using` or `defer` blocks of its own runs them inside the
same cleanup; they finish with it or are abandoned with it.

## Abandoning

`abandon()` runs in the caller, synchronously:

1. The cleanup is marked abandoned. A pending host operation, such as the
   flush a dispose waits on, is cancelled, and the host closes that handle
   at once. Disposes that have not started are skipped and their handles
   closed the same way.
2. The stopped frames still run their synchronous parts on the way out:
   their `defer` blocks run. A `dispose!` reached during this unwinding that
   would suspend does not wait; its handle is closed by the host. No
   `defer` is ever skipped, only waiting work.
3. Every `await!()` on the cleanup returns `.Abandoned`.

Tasks run cooperatively on one thread, so all of this finishes inside the
`abandon()` call before any waiter runs again. Abandoning a cleanup that
already ended, or abandoning it twice, does nothing, and its waits return
how it actually ended.

## Deadlines

`timeout!` cancels gracefully: when the limit passes, its work is cancelled
and `timeout!` waits for the cleanup, which can take longer than the limit.
A bound on the cleanup is a parameter, since only `timeout!` holds its own
cleanup:

```text
timeout!(5s, cleanup = 1s): fetch_all!()   # cancel at 5 s; abandon what is left 1 s later
```

Without the parameter, a program cannot force the bound from outside:
racing `c.await!()` against `sleep!` would only fold `c` into the race's
cleanup.

## A Cleanup Nobody Awaits

Only an explicit `cancel()` on a stored suspension gives user code a
`Cleanup`; the combinators, and a cancelled parent, always take care of
their children's. Two rules keep a forgotten one nearly harmless:

- A `Cleanup` must be used. Writing `job.cancel()` as a bare statement is
  `discarded-must-use-value`, with fix-its to await or to abandon it.
- What a forgotten cleanup loses is only the waiting part of graceful
  closes, because `cancel()` already ran everything that does not suspend.
  hd has no finalizers, so the runtime cannot notice when a `Cleanup`
  becomes unreachable. The handles it still holds are closed when the
  program exits, and `hd test` fails a case that leaves them open, so a
  forgotten `await` shows up in the first test run.

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
`cancelAndJoin` as `cancel()` then `await!()`; and Trio's advice to bound a
shield with a deadline as `timeout!`'s `cleanup` parameter.

## What Changes

- `Suspend.cancel()` returns a `Cleanup` instead of `void`, and runs the
  non-suspending part of the unwinding before it returns.
- New std types `Cleanup` and `CleanupEnd`, in `std.task`.
- `timeout!` gains an optional `cleanup: Duration` parameter.
- `race!` and `all!` keep their behavior; their rules are restated on
  `Cleanup`.
- The host lists abandoned handles separately from leaked ones in
  `hd test`.
- A host's cancellation of the entry, the runtime scenarios' `hd.cancel`,
  follows the same rules with the host as the waiter.

## Open Points

1. The names `Cleanup`, `CleanupEnd` and the `cleanup` parameter of
   `timeout!`.
2. Whether the host's cancellation of the entry should wait for the
   entry's cleanup without a bound, or with a default one.
