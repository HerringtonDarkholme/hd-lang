# Task

Status: standard library specification draft.

This chapter defines the part of `std.task` that `lib/std` writes in
ordinary hd over the language tier:

- the `retry!` combinator;
- the `Backoff` policy and the `retry_with!` combinator;
- the `all_list!` combinator.

The language tier keeps the `Suspend` protocol, `block_on`, and the
polling combinators `all!` and `race!`, which are compiler intrinsics
([Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators)).

## Retry

`retry!` calls a suspending attempt until it succeeds or runs out of
tries:

```text
pub fn retry![T, E, $R](times: usize, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R
```

1. r[std-task.combinator.retry.decl-usize] `std.task` declares `retry!` with the signature above, whose `times` is a `usize`, as an ordinary `fn!` function, not an intrinsic. Code imports it with `use std.task.retry`.
2. r[std-task.combinator.retry.loop] `retry!` calls `attempt` at most `times` times, one call after another, and returns the first `.Ok` result without another call.
3. r[std-task.combinator.retry.last-error] When every one of the `times` calls returns `.Err`, `retry!` returns the last `.Err`.
4. r[std-task.combinator.retry.zero-once] A `times` of 0 counts as 1, so `retry!` calls `attempt` once and returns its result.
5. r[std-task.combinator.retry.cancel] Cancellation follows the ordinary rules of [Cancellation](../lang/11-requirements-and-suspension.md#cancellation), since `retry!` is a loop of bang calls. Cancelling it cancels the active attempt, and no further attempt starts.

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
With `times` at 0, `retry!` would make one call.

The row parameter `R` lets an attempt require providers. An attempt that
calls `println` has the row `Console`, and that `retry!` call then
requires `Console` too.

> **Why.** A retry over a `fn!` attempt is a loop that `lib/std` can write
> in plain hd, so it needs no compiler support. It has no delay between
> attempts, so it needs no clock. A `times` of 0 still makes one attempt,
> because with no attempt there is no `Result` to return.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Suspending Closures And Clauses](../lang/07-functions.md#suspending-closures-and-clauses).

## Retry With Backoff

`retry_with!` retries as `retry!` does, and waits on the `Clock` between
attempts:

```text
pub data Backoff:
    pub attempts: usize
    pub initial: Duration
    pub factor: i32
    pub max: Duration

pub fn retry_with![T, E, $R](backoff: Backoff, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock
```

1. r[std-task.backoff.decl-usize] `std.task` declares the data type `Backoff` with the four public fields above, where `attempts` is a `usize`. `Backoff` implements `Eq`.
2. r[std-task.retry-with.decl] `std.task` declares `retry_with!` with the signature above, as an ordinary `fn!` function. Code imports both, as in `use std.task.{Backoff, retry_with}`.
3. r[std-task.retry-with.loop] `retry_with!` calls `attempt` at most `backoff.attempts` times, one call after another, and returns the first `.Ok` result without another call.
4. r[std-task.retry-with.last-error] When every call returns `.Err`, `retry_with!` returns the last `.Err`.
5. r[std-task.retry-with.zero-once] An `attempts` of 0 counts as 1, as for `retry!`.
6. r[std-task.retry-with.sleep] Between two calls, `retry_with!` calls `sleep!` on the `Clock` provider that covers it. It does not sleep after the last call.
7. r[std-task.retry-with.delay] The first delay is `initial`. Each later delay is the one before it times `factor`, and a delay above `max` is `max` instead.
8. r[std-task.retry-with.cancel] Cancellation follows [`std-task.combinator.retry.cancel`](#r-std-task.combinator.retry.cancel): cancelling `retry_with!` cancels the active attempt or sleep, and nothing further starts.

```text
use std.task.{Backoff, retry_with}
use std.time.{Clock, ms, s}

data Server:
    busy_for: i32

fn ask!(server: mut Server) -> Result[string, string]:
    if server.busy_for > 0:
        server.busy_for = server.busy_for - 1
        return .Err("busy")
    .Ok("ready")

fn connect!(server: mut Server) -> Result[string, string] $ Clock:
    policy := Backoff { attempts: 5, initial: 100ms, factor: 2, max: 5s }
    retry_with!(policy):
        ask!(server)
```

With `busy_for` at 3, `connect!` sleeps 100, 200, and 400 milliseconds,
and the fourth call returns `.Ok("ready")`. With `busy_for` at 5, it also
sleeps 800 milliseconds and returns `.Err("busy")` after five calls.

> **Why.** `Backoff` is the option set of Deno's `retry` in
> [`@std/async`](https://jsr.io/@std/async): a count, a first delay, a
> multiplier, and a cap. The delays go through the `Clock` requirement,
> so a test provider makes them instant and observable.

See also: [Retry](#retry), [Clock](time.md#clock).

## All List

`all_list!` awaits a list of suspending tasks together and returns their
results in a list:

```text
pub fn all_list![T, $R](tasks: List[fn!() -> T $ R]) -> List[T] $ R
```

1. r[std-task.all-list] `std.task` declares `all_list!` with the signature above, as an ordinary `fn!` function, not an intrinsic. Code imports it with `use std.task.all_list`.
2. r[std-task.all-list.children] `all_list!` calls each task once, without `!`, in input order, and awaits the cold suspensions as `all!` awaits its children.
3. r[std-task.all-list.order] The result holds each task's value at the task's index in `tasks`, whatever order the tasks complete in.
4. r[std-task.all-list.empty] An empty `tasks` gives an empty list.
5. r[std-task.all-list.like-all] Scheduling, cancellation, failure, and panics follow the rules of `all!`: [`req.schedule.all-unfinished`](../lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished), [`req.schedule.all-completed`](../lang/11-requirements-and-suspension.md#r-req.schedule.all-completed), and [`req.combinator.cancel-children`](../lang/11-requirements-and-suspension.md#r-req.combinator.cancel-children).

```text
use std.task.all_list

fn square!(value: i32) -> i32:
    value * value

fn squares!() -> List[i32]:
    all_list!([fn!() -> i32: square!(3), fn!() -> i32: square!(1), fn!() -> i32: square!(2)])
```

`squares!` returns `[9, 1, 4]`: the results are in input order.

> **Why.** `all!` takes a fixed number of children of any types and gives
> a tuple. A list of tasks of one type, whose length is known only at run
> time, needs a list result. `lib/std` writes `all_list!` as nested `all!`
> calls, so it needs no new intrinsic, and `all!` stays fixed-arity.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Cooperative Scheduling](../lang/11-requirements-and-suspension.md#cooperative-scheduling).
