# Task

Status: standard library specification draft.

This chapter defines the part of `std.task` that `lib/std` writes in
ordinary hd over the language tier:

- the `retry!` combinator;
- the `Backoff` policy and the `retry_with!` combinator;
- the `all_list!` combinator;
- the `timeout!` combinator and its `TimedOut` error;
- the `map_all!` combinator.

The language tier keeps the `Suspend` protocol, `block_on`, and the
polling combinators `all!` and `race!`, which are compiler intrinsics
([Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators)).

## Retry

`retry!` calls a suspending attempt until it succeeds, fails with an error
that `when` rejects, or runs out of tries:

```text
pub fn retry![T, E, $R](times: usize, when: fn(E) -> bool = fn(error: E): true, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R
```

1. r[std-task.combinator.retry.decl-when] `std.task` declares `retry!` with the signature above, whose `times` is a `usize` and whose `when` has a default, as an ordinary `fn!` function, not an intrinsic. Code imports it with `use std.task.retry`.
2. r[std-task.combinator.retry.loop] `retry!` calls `attempt` at most `times` times, one call after another, and returns the first `.Ok` result without another call.
3. r[std-task.combinator.retry.last-error] When every one of the `times` calls returns `.Err`, `retry!` returns the last `.Err`.
4. r[std-task.combinator.retry.zero-once] A `times` of 0 counts as 1, so `retry!` calls `attempt` once and returns its result.
5. r[std-task.combinator.retry.when] After a call that returns `.Err(error)`, `retry!` makes another call only when `when(error)` is `true`. When it is `false`, `retry!` returns that `.Err` at once.
6. r[std-task.combinator.retry.when-default] A call without `when` retries every `.Err`, so `when` is `fn(error: E): true`. Code passes `when` by name, as in `retry!(3, when=is_busy)`.
7. r[std-task.combinator.retry.when-plain] `when` is an ordinary function: it takes the error and does not suspend, and it runs at most once for each `.Err`.
8. r[std-task.combinator.retry.cancel] Cancellation follows the ordinary rules of [Cancellation](../lang/11-requirements-and-suspension.md#cancellation), since `retry!` is a loop of bang calls. Cancelling it cancels the active attempt, and no further attempt starts.

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

```text
use std.task.retry

fn is_busy(error: string) -> bool:
    error == "busy"

fn connect!(replies: mut List[Result[string, string]]) -> Result[string, string]:
    retry!(5, when=is_busy):
        replies.remove_at(0)
```

With replies `.Err("busy")`, `.Err("refused")`, `.Ok("ready")`, `connect!` returns
`.Err("refused")` after two calls: `is_busy` rejects the second error, so a third
call never starts.

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

`retry_with!` retries as `retry!` does, with the same `when` predicate,
and waits on the `Clock` between attempts:

```text
pub data Backoff:
    pub attempts: usize
    pub initial: Duration
    pub factor: i32
    pub max: Duration

pub fn retry_with![T, E, $R](backoff: Backoff, when: fn(E) -> bool = fn(error: E): true, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock
```

1. r[std-task.backoff.decl-usize] `std.task` declares the data type `Backoff` with the four public fields above, where `attempts` is a `usize`. `Backoff` implements `Eq`.
2. r[std-task.retry-with.decl-when] `std.task` declares `retry_with!` with the signature above, as an ordinary `fn!` function. Code imports both, as in `use std.task.{Backoff, retry_with}`.
3. r[std-task.retry-with.loop] `retry_with!` calls `attempt` at most `backoff.attempts` times, one call after another, and returns the first `.Ok` result without another call.
4. r[std-task.retry-with.last-error] When every call returns `.Err`, `retry_with!` returns the last `.Err`.
5. r[std-task.retry-with.zero-once] An `attempts` of 0 counts as 1, as for `retry!`.
6. r[std-task.retry-with.sleep] Between two calls, `retry_with!` calls `sleep!` on the `Clock` provider that covers it. It does not sleep after the last call.
7. r[std-task.retry-with.delay] The first delay is `initial`. Each later delay is the one before it times `factor`, and a delay above `max` is `max` instead.
8. r[std-task.retry-with.when] `retry_with!` applies `when` as [`std-task.combinator.retry.when`](#r-std-task.combinator.retry.when), [`.when-default`](#r-std-task.combinator.retry.when-default), and [`.when-plain`](#r-std-task.combinator.retry.when-plain) state, and sleeps only before a call that `when` allowed.
9. r[std-task.retry-with.cancel] Cancellation follows [`std-task.combinator.retry.cancel`](#r-std-task.combinator.retry.cancel): cancelling `retry_with!` cancels the active attempt or sleep, and nothing further starts.

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

## Timeout

`timeout!` runs a suspending task and gives up when a limit passes first:

```text
pub data TimedOut:
    pub limit: Duration

pub fn timeout![T, $R](limit: Duration, work: fn!() -> T $ R) -> Result[T, TimedOut] $ R + Clock
```

1. r[std-task.timeout.type] `std.task` declares the data type `TimedOut` with the public field `limit`, a `Duration`. `TimedOut` implements `Eq`, `Debug`, `Display`, and `Error`, and its `Display` text is `timed out after ` followed by the `Display` text of `limit`.
2. r[std-task.timeout.decl] `std.task` declares `timeout!` with the signature above, as an ordinary `fn!` function, not an intrinsic. Code imports both, as in `use std.task.{TimedOut, timeout}`.
3. r[std-task.timeout.race] `timeout!` is a [`race!`](../lang/11-requirements-and-suspension.md#standard-combinators) of two children, in this order: `work`, called once without `!`, and a call of `sleep!(limit)` on the `Clock` provider that covers it.
4. r[std-task.timeout.ok] When `work` completes first, `timeout!` returns `.Ok` of its value.
5. r[std-task.timeout.err] When the sleep completes first, `timeout!` returns `.Err(TimedOut { limit: limit })`.
6. r[std-task.timeout.tie] A `work` that completes on its first poll wins even when `limit` is 0, since `race!` polls its children in order.
7. r[std-task.timeout.cancel-loser] The losing child is cancelled before `timeout!` returns, as [`req.combinator.race-losers-disposed`](../lang/11-requirements-and-suspension.md#r-req.combinator.race-losers-disposed) states, so a timed-out `work` runs its registered `defer` suites and starts nothing more.
8. r[std-task.timeout.cancel] Cancelling `timeout!` cancels both children, as [`req.combinator.cancel-children`](../lang/11-requirements-and-suspension.md#r-req.combinator.cancel-children) states.

```text
use std.task.{TimedOut, timeout}
use std.time.{Clock, s, sleep}

fn lookup!() -> string $ Clock:
    sleep!(5s)
    "value"

fn lookup_in_time!() -> Result[string, TimedOut] $ Clock:
    timeout!(1s):
        lookup!()
```

On a real clock, `lookup_in_time!` returns `.Err(TimedOut { limit: 1s })` after
one second, and the 5 second sleep is cancelled. On a `ManualClock`, every sleep
completes at once, so `lookup!` finishes first and the result is
`.Ok("value")`.

> **Why.** Go's `context.WithTimeout` and Rust's `tokio::time::timeout` also
> return an error value when the limit passes first, and drop or cancel the
> work. `timeout!` is a race against a sleep, so `lib/std` needs no new
> intrinsic. Because `race!` needs children of one type, the helper wraps the
> work's value in `.Ok` and the sleep's result in `.Err`. Work first, then the
> sleep, so a deterministic provider such as `ManualClock`, whose sleeps never
> wait, never reports a timeout for work that can finish without suspending.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Clock Helpers](time.md#clock-helpers), [Manual Clock](time.md#manual-clock).

## Map All

`map_all!` runs a suspending function over a list with a bound on how many
calls are in flight:

```text
pub fn map_all![T, U, $R](items: List[T], limit: usize, work: fn!(T) -> U $ R) -> List[U] $ R
```

1. r[std-task.map-all.decl] `std.task` declares `map_all!` with the signature above, as an ordinary `fn!` function, not an intrinsic. Code imports it with `use std.task.map_all`.
2. r[std-task.map-all.order] The result holds `work(item)` for each item at the item's index in `items`, whatever order the calls complete in.
3. r[std-task.map-all.start-order] `work` is called once for each item. Calls start in item order: no call starts before the call for every earlier item has started.
4. r[std-task.map-all.limit] At most `limit` calls of `work` are in flight at once. A call is in flight from its start until it completes.
5. r[std-task.map-all.window] When one call completes and an item has not started, the next item in order starts, so the number in flight stays at `limit` until the items run out.
6. r[std-task.map-all.limit-zero] A `limit` of 0 counts as 1, so the calls run one after another, as `retry!` counts a `times` of 0.
7. r[std-task.map-all.empty] An empty `items` gives an empty list and calls `work` never.
8. r[std-task.map-all.like-all] Scheduling, cancellation, failure, and panics follow the rules of `all!`: a panic in one call propagates, and cancelling `map_all!` cancels every call in flight and starts no more ([`req.combinator.cancel-children`](../lang/11-requirements-and-suspension.md#r-req.combinator.cancel-children)).

```text
use std.task.map_all
use std.time.{Clock, ms, sleep}

fn fetch!(id: i32) -> string $ Clock:
    sleep!(ms(100))
    "item ${id}"

fn fetch_all!() -> List[string] $ Clock:
    map_all!([1, 2, 3, 4, 5], 2, fn!(id: i32) -> string $ Clock: fetch!(id))
```

`fetch_all!` returns `["item 1", ..., "item 5"]` in that order. On a real clock
it runs two calls at a time, so it takes about 300 milliseconds; the third call
starts as soon as the first completes.

> **Why.** Effect's `forEach` with `concurrency` and Go's `errgroup.SetLimit`
> bound fan-out the same way. `all_list!` starts every task at once, which a
> list of a thousand requests cannot afford. `lib/std` writes `map_all!` as
> `limit` worker closures run by one `all_list!`, each taking the next unstarted
> index. Bang calls are the only yield points, so the shared index needs no
> lock. A `limit` of 0 counts as 1, not as a panic, as for `retry!`.

See also: [All List](#all-list), [Cooperative Scheduling](../lang/11-requirements-and-suspension.md#cooperative-scheduling).
