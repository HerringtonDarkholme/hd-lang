# Task

Status: standard library specification draft.

This chapter defines the part of `std.task` that `lib/std` writes in
ordinary hd over the language tier:

- the `retry!` combinator.

The language tier keeps the `Suspend` protocol, `block_on`, and the
polling combinators `all!` and `race!`, which are compiler intrinsics
([Standard Combinators](../11-requirements-and-suspension.md#standard-combinators)).

## Retry

`retry!` calls a suspending attempt until it succeeds or runs out of
tries:

```text
pub fn retry![T, E](times: i32, attempt: fn!() -> Result[T, E]) -> Result[T, E]
```

1. r[std-task.combinator.retry] `std.task` declares `retry!` with the signature above, as an ordinary `fn!` function, not an intrinsic. Code imports it with `use std.task.retry`.
2. r[std-task.combinator.retry.loop] `retry!` calls `attempt` at most `times` times, one call after another, and returns the first `.Ok` result without another call.
3. r[std-task.combinator.retry.last-error] When every one of the `times` calls returns `.Err`, `retry!` returns the last `.Err`.
4. r[std-task.combinator.retry.cancel] Cancellation follows the ordinary rules of [Cancellation](../11-requirements-and-suspension.md#cancellation), since `retry!` is a loop of bang calls. Cancelling it cancels the active attempt, and no further attempt starts.

```text
use std.task.retry

data Server:
    busy_for: i32

fn ask!(server: mut Server) -> Result[string, string]:
    if server.busy_for > 0:
        server.busy_for = server.busy_for - 1
        return .Err("busy")
    .Ok("ready")

fn connect!(server: mut Server) -> Result[string, string]:
    retry!(3):
        ask!(server)
```

With `busy_for` at 2, `connect!` returns `.Ok("ready")` on the third call.
With `busy_for` at 3, it returns `.Err("busy")` after three calls.

> **Why.** A retry over a `fn!` attempt is a loop that `lib/std` can write
> in plain hd, so it needs no compiler support. It has no delay between
> attempts, so it needs no clock.

See also: [Standard Combinators](../11-requirements-and-suspension.md#standard-combinators),
[Suspending Closures And Clauses](../07-functions.md#suspending-closures-and-clauses).
