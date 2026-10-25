# Task

Status: standard library specification draft.

This chapter defines the part of `std.task` that `lib/std` writes in
ordinary hd over the language tier:

- the `retry!` combinator and its `Backoff` policy;
- the `timeout!` and `with_timeout!` combinators and their `TimedOut` error;
- the `map_all!` combinator;
- the `CleanupEnd` enum, which `cancel!()` returns;
- the `BlockOn` enum, which `block_on` returns.

The language tier keeps the `Suspend` protocol, `block_on`, and the
polling combinators `all!` and `race!`, which are compiler intrinsics
([Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators)).

## Retry

`retry!` calls a suspending attempt until it succeeds, fails with an error
that `when` rejects, or runs out of tries:

```text
pub fn retry![T, E, $R](policy: Backoff, when: fn(E) -> bool = fn(error: E): true, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock
```

1. r[std-task.retry.policy.decl] `std.task` declares the ordinary `retry!` function with the signature above.
2. r[std-task.retry.policy.loop] `retry!` calls `attempt` sequentially at most `policy.attempts` times, returning the first `.Ok` without another call.
3. r[std-task.retry.policy.last-error] When every attempt returns `.Err`, `retry!` returns the last error.
4. r[std-task.retry.policy.zero-once] An `attempts` of 0 counts as 1, so `retry!` always calls `attempt` at least once.
5. r[std-task.combinator.retry.when] After a call that returns `.Err(error)`, `retry!` makes another call only when `when(error)` is `true`. When it is `false`, `retry!` returns that `.Err` at once.
6. r[std-task.retry.policy.when-default] Omitting `when` retries every error, using `fn(error: E): true`.
7. r[std-task.combinator.retry.when-plain] `when` is an ordinary function: it takes the error and does not suspend, and it runs at most once for each `.Err`.
8. r[std-task.retry.policy.cancel] Cancelling `retry!` cancels the active attempt or sleep and starts no further attempt.
9. r[std-task.retry.policy.clock] Every `retry!` call requires `Clock`, including a policy whose `initial` is `0ms`.

```text
use std.task.{Backoff, retry}
use std.time.Clock

struct Server:
    busy_for: i32

fn ask!(server: &mut Server) -> Result[string, string]:
    if server.busy_for > 0:
        server.busy_for = server.busy_for - 1
        return .Err("busy")
    .Ok("ready")

fn connect!(server: *mut Server) -> Result[string, string] $ Clock:
    retry!(Backoff { initial: 0ms }):
        ask!(server)
```

With `busy_for` at 2, `connect!` returns `.Ok("ready")` on the third call.
With `busy_for` at 3, it returns `.Err("busy")` after three calls.
With `attempts` at 0, `retry!` would make one call.

```text
use std.task.{Backoff, retry}
use std.time.Clock

fn is_busy(error: string) -> bool:
    error == "busy"

fn connect!(replies: *mut List[Result[string, string]]) -> Result[string, string] $ Clock:
    retry!(Backoff { attempts: 5, initial: 0ms }, when=is_busy):
        replies.remove_at(0)
```

With replies `.Err("busy")`, `.Err("refused")`, `.Ok("ready")`, `connect!` returns
`.Err("refused")` after two calls: `is_busy` rejects the second error, so a third
call never starts.

The row parameter `R` lets an attempt require providers. An attempt that
calls `println` has the row `Console`, and that `retry!` call then
requires `Console` and `Clock`.

> **Why.** A retry over a `fn!` attempt is a loop that `lib/std` can write
> in plain hd, so it needs no compiler support. One policy configures immediate and
> delayed attempts, both using `Clock`. An `attempts` of 0 still makes one attempt,
> because with no attempt there is no `Result` to return.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Suspending Closures And Clauses](../lang/07-functions.md#suspending-closures-and-clauses).

## Retry With Backoff

`Backoff` configures `retry!`'s attempt count and delays:

```text
pub struct Backoff:
    pub attempts: usize = 3
    pub initial: Duration = 100ms
    pub factor: u32 = 2
    pub max: Duration = 10s
```

1. r[std-task.backoff.decl.defaults] `std.task` declares `Backoff` with the four public fields, types and defaults above.
2. r[std-task.backoff.value-copy] A `Backoff` is a value whose copy is independent of the original.
3. r[std-task.backoff.eq] `Backoff` implements `Eq` by comparing all four fields.
4. r[std-task.retry.policy.sleep] Between attempts, `retry!` calls the covering `Clock.sleep!`, with no sleep after the final attempt.
5. r[std-task.retry.policy.delay.capped] The first delay is `initial` and later delays multiply the previous delay by `factor`, each capped at `max` without overflow.
6. r[std-task.retry.policy.when-sleep] `retry!` sleeps only before another attempt that `when` allowed.

```text
use std.task.{Backoff, retry}
use std.time.{Clock, ms, s}

struct Server:
    busy_for: i32

fn ask!(server: &mut Server) -> Result[string, string]:
    if server.busy_for > 0:
        server.busy_for = server.busy_for - 1
        return .Err("busy")
    .Ok("ready")

fn connect!(server: *mut Server) -> Result[string, string] $ Clock:
    policy := Backoff { attempts: 5, initial: 100ms, factor: 2, max: 5s }
    retry!(policy):
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

`all_list!` is the private engine of `map_all!`.
Public callers use `map_all!` with the input length as the concurrency limit:

```text
use std.task.map_all

fn square!(value: i32) -> i32:
    value * value

fn squares!() -> List[i32]:
    map_all!([3, 1, 2], 3, square)
```

1. r[std-task.map-all.private-engine] `std.task.all_list` is private and cannot be imported by a caller. Error: `private-import`.

`squares!` returns `[9, 1, 4]`, in input order.

> **Why.** `map_all!` accepts items directly, so callers need not construct
> a list of closures. Its private engine drives one flat list of cold
> workers and retains results until every worker finishes. Waker readiness
> routes later polls to unfinished workers without scanning unwoken ones.

See also: [Map All](#map-all),
[Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators).

## Timeout

`timeout!` runs a suspending task and gives up when a limit passes first:

```text
pub struct TimedOut:
    pub limit: Duration
    pub cleanup: CleanupEnd

pub fn timeout![T, $R](limit: Duration, cleanup: Duration? = .None, work: fn!() -> T $ R) -> Result[T, TimedOut] $ R + Clock
```

1. r[std-task.timeout.fields-value] `std.task` declares the struct value `TimedOut` with the public fields `limit`, a `Duration`, and `cleanup`, a [`CleanupEnd`](#cleanup-end). `TimedOut` implements `Eq`, `Debug`, `Display`, and `Error`.
2. r[std-task.timeout.display] The `Display` text of a `TimedOut` is `timed out after ` followed by the `Display` text of `limit`. When `cleanup` is `.Abandoned`, the text ends with `, cleanup abandoned`.
3. r[std-task.timeout.decl-cleanup] `std.task` declares `timeout!` with the signature above, as an ordinary `fn!` function, not an intrinsic. Code imports both, as in `use std.task.{TimedOut, timeout}`.
4. r[std-task.timeout.race] `timeout!` is a [`race!`](../lang/11-requirements-and-suspension.md#standard-combinators) of two children, in this order: `work`, called once without `!`, and a call of `sleep!(limit)` on the `Clock` provider that covers it.
5. r[std-task.timeout.ok] When `work` completes first, `timeout!` returns `.Ok` of its value.
6. r[std-task.timeout.err-cleanup] When the sleep completes first, `timeout!` returns `.Err(TimedOut { limit: limit, cleanup: end })`, where `end` says how the cleanup of `work` ended.
7. r[std-task.timeout.tie] A `work` that completes on its first poll wins even when `limit` is 0, since `race!` polls its children in order.
8. r[std-task.timeout.cancel-loser] The losing child is cancelled before `timeout!` returns, as [`req.combinator.race-losers-disposed`](../lang/11-requirements-and-suspension.md#r-req.combinator.race-losers-disposed) states, so a timed-out `work` runs its registered `defer` suites and starts nothing more.
9. r[std-task.timeout.cancel] Cancelling `timeout!` cancels both children, as [`req.combinator.cancel-children`](../lang/11-requirements-and-suspension.md#r-req.combinator.cancel-children) states.

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

On a real clock, `lookup_in_time!` returns
`.Err(TimedOut { limit: 1s, cleanup: .Finished })` after one second, and the
5 second sleep is cancelled. On a `ManualClock`, every sleep completes at once,
so `lookup!` finishes first and the result is `.Ok("value")`.

> **Why.** Go's `context.WithTimeout` and Rust's `tokio::time::timeout` also
> return an error value when the limit passes first, and drop or cancel the
> work. `timeout!` is a race against a sleep, which `lib/std` writes in
> hd. Because `race!` needs children of one type, the helpers wrap the
> work's value and the sleep's end in one private enum. Work first, then the
> sleep, so a deterministic provider such as `ManualClock`, whose sleeps never
> wait, never reports a timeout for work that can finish without suspending.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Clock Helpers](time.md#clock-helpers), [Manual Clock](time.md#manual-clock).

### Cleanup Bound

A `cleanup` argument bounds how long `timeout!` waits for the cleanup of
`work` once `work` is cancelled:

```text
use std.ops.Disposable
use std.task.{TimedOut, timeout}
use std.time.{Clock, s}

struct Upload:
    log: *mut List[string]
    ack: mut Suspend[void]

impl Disposable for Upload:
    fn dispose!(&mut self) -> void:
        defer: self.log.push("upload closed")
        let mut ack = self.ack
        ack!()                          # waits for the peer
        self.log.push("upload acknowledged")

fn send!(log: *mut List[string], reply: mut Suspend[string], ack: mut Suspend[void]) -> Result[string, TimedOut] $ Clock:
    timeout!(5s, cleanup=1s):
        using upload = &mut Upload { log: log, ack: ack }
        reply!()
```

When neither `reply` nor `ack` completes, the limit cancels the work at 5
seconds, and its dispose waits for `ack`. At 6 seconds `timeout!` abandons the
work. The log reads `upload closed`, and `send!` returns
`.Err(TimedOut { limit: 5s, cleanup: .Abandoned })`, whose text is
`timed out after 5s, cleanup abandoned`. When `ack` completes in time, the log
also holds `upload acknowledged`, and `cleanup` is `.Finished`.

1. r[std-task.timeout.cleanup-unbounded] Without `cleanup`, `timeout!` waits for the whole cleanup of a cancelled `work`, however long it takes.
2. r[std-task.timeout.cleanup-deadline] With `cleanup`, the cleanup of `work` has a deadline: `cleanup` after `work` is first cancelled, by the limit or by a cancel of `timeout!`.
3. r[std-task.timeout.cleanup-fixed] A later cancel of `timeout!` never moves the deadline.
4. r[std-task.timeout.cleanup-outlives] Cancelling `timeout!` does not stop the bound. The cleanup of `work` goes on toward the same deadline, as part of the cleanup of `timeout!`.
5. r[std-task.timeout.cleanup-abandon] When the deadline passes before the cleanup of `work` has ended, `timeout!` calls [`abandon()`](../lang/11-requirements-and-suspension.md#r-req.abandon.unwind) on `work`.
6. r[std-task.timeout.cleanup-cooperative] The deadline is cooperative. `abandon()` runs at the first point at or after the deadline where the cleanup waits, so synchronous code is never interrupted.
7. r[std-task.timeout.cleanup-ended] A cleanup that ends before `abandon()` runs is not abandoned.
8. r[std-task.timeout.cleanup-zero] A `cleanup` of 0 abandons the cleanup at its first wait, so `limit` then bounds all of the waiting.
9. r[std-task.timeout.cleanup-end] `TimedOut.cleanup` is `.Abandoned` when the deadline cut the cleanup short, or when another part of it was cut short, by [`std-task.cleanup-end.cut-short`](#r-std-task.cleanup-end.cut-short). Otherwise it is `.Finished`.

The deadline is set once, at the first cancel:

| Time | Event |
| --- | --- |
| t0 | `timeout!(5s, cleanup=1s)` starts its work. |
| t5 | The limit passes. The work is cancelled, and the deadline is t6. |
| t5.2 | The caller cancels `timeout!`. The cleanup goes on, and the deadline stays t6. |
| t6 | The cleanup has not ended, so `timeout!` abandons the work. |

Had the caller cancelled `timeout!` at t3 instead, the deadline would be t4.

> **Why.** A graceful cancel never drops cleanup, so only an explicit
> `abandon()` ends a cleanup early, and the `cleanup` argument makes that
> call. Racing a cancel against a sleep would not bound it, since `race!`
> cancels the losing cancel, which only continues inside the race's own
> cleanup. The deadline is fixed at the first cancel, so cancelling again
> and again cannot stretch the bound.

> **Note.** The bound limits waiting, not computation. A dispose that
> computes for two seconds before its first wait overruns a one-second
> bound.

See also: [Cleanup End](#cleanup-end),
[Abandoning](../lang/11-requirements-and-suspension.md#abandoning).

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
6. r[std-task.map-all.limit-zero.attempts] A `limit` of 0 counts as 1, so the calls run one after another, as `retry!` counts an `attempts` of 0.
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
> bound fan-out the same way. Setting `limit` to `items.len()` starts every
> item together. Smaller limits use a sliding window of worker closures
> driven by a private flat engine, each taking the next unstarted index.
>
> Bang calls are the only yield points, so the shared index needs no
> lock. A `limit` of 0 counts as 1, not as a panic, as for `retry!`.

See also: [All List](#all-list), [Cooperative Scheduling](../lang/11-requirements-and-suspension.md#cooperative-scheduling).

## Cleanup End

`CleanupEnd` says how the cleanup of a cancelled suspension ended, and
[`cancel!()`](../lang/11-requirements-and-suspension.md#cancellation)
returns it:

```text
pub enum CleanupEnd:
    Finished
    Abandoned
```

1. r[std-task.cleanup-end.decl] `std.task` declares the enum `CleanupEnd` with the two variants above. `CleanupEnd` implements `Eq` and `Debug`. Code imports it with `use std.task.CleanupEnd`.
2. r[std-task.cleanup-end.finished] `.Finished` means the whole cleanup ran to its end, with nothing in it cut short.
3. r[std-task.cleanup-end.cut-short] `.Abandoned` means a part of the cleanup was cut short: by [`abandon()`](../lang/11-requirements-and-suspension.md#r-req.abandon.result), or by a dispose whose awaited suspension was cancelled elsewhere, by [`req.abandon.dependency.result`](../lang/11-requirements-and-suspension.md#r-req.abandon.dependency.result).
4. r[std-task.cleanup-end.best-effort] `.Finished` does not mean that every close succeeded: a `dispose!` returns `void`, by [`flow.using.best-effort`](../lang/06-control-flow.md#r-flow.using.best-effort).
5. r[std-task.cleanup-end.debug] The `Debug` text of each variant is `CleanupEnd.` followed by the variant's name, as in `CleanupEnd.Finished`.

```text
use std.task.CleanupEnd

fn stop!(job: mut Suspend[string]) -> string:
    end := job.cancel!()
    if end == CleanupEnd.Finished: "closed" else: "cut short"
```

`stop!` returns `"closed"` once the cleanup of `job` has run to its end.

> **Why.** A `void` dispose cannot report a failed close, but a program
> can still log a close that was cut short. Code that must know that a
> close protocol succeeded calls an explicit close that returns a
> `Result`, as [`flow.using.best-effort`](../lang/06-control-flow.md#r-flow.using.best-effort)
> advises.

See also: [Cancellation](../lang/11-requirements-and-suspension.md#cancellation).

## Block On

`BlockOn[T]` says how a
[`block_on`](../lang/11-requirements-and-suspension.md#driving-a-stored-suspension)
drive ended, and `block_on` returns it:

```text
pub enum BlockOn[T]:
    Completed(T)
    Cancelled
    Abandoned
```

1. r[std-task.block-on.decl] `std.task` declares the generic enum `BlockOn[T]` with the three variants above. Code imports it with `use std.task.BlockOn`.
2. r[std-task.block-on.completed] `.Completed(value)` means the suspension completed with `value`, by [`req.drive.block-on.completed`](../lang/11-requirements-and-suspension.md#r-req.drive.block-on.completed).
3. r[std-task.block-on.cancelled] `.Cancelled` means the suspension was cancelled and its cleanup ran to its end, as [`CleanupEnd.Finished`](#r-std-task.cleanup-end.finished) means.
4. r[std-task.block-on.abandoned] `.Abandoned` means the suspension was cancelled and a part of its cleanup was cut short, as [`CleanupEnd.Abandoned`](#r-std-task.cleanup-end.cut-short) means.

```text
use std.task.block_on

fn fetch!(id: i32) -> string:
    "user ${id}"

fn name_of(id: i32) -> string:
    match block_on(fetch(id)):
        .Completed(name) => name
        .Cancelled => "cancelled"        # its cleanup ran to its end
        .Abandoned => "cut short"        # its cleanup was cut short
```

`name_of(7)` returns `"user 7"`: nothing else holds the suspension, so
nothing can cancel it.

> **Why.** Synchronous code that drives a suspension cannot be cancelled
> itself, so the result names every way the drive can end. A caller that
> holds the only reference matches the other two variants with `pass` or
> a default, as `println` does.

See also: [Driving A Stored Suspension](../lang/11-requirements-and-suspension.md#driving-a-stored-suspension),
[Cleanup End](#cleanup-end).


## Result Timeouts

`with_timeout!` composes a deadline with Result-returning work while
keeping the operation's own error type:

```text
pub fn with_timeout![T, E < From[TimedOut], $R](limit: Duration, cleanup: Duration? = .None, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock
```

1. r[std-task.with-timeout.decl] `std.task` declares `with_timeout!` with the signature above.
2. r[std-task.with-timeout.bound] The work's error type implements `std.convert.From[TimedOut]`. Error: `unsatisfied-trait-bound`.
3. r[std-task.with-timeout.completed] When the work completes first, `with_timeout!` returns its Result unchanged.
4. r[std-task.with-timeout.expired] When the deadline wins, `with_timeout!` converts the `TimedOut` through `E::from` and returns `.Err` of that value.
5. r[std-task.with-timeout.cleanup] `with_timeout!` uses `timeout!` with the same limit, cleanup bound, and work.
6. r[std-task.with-timeout.flat] Nesting `with_timeout!` preserves the work's Result type.
7. r[std-task.with-timeout.clock] `with_timeout!` requires `Clock` in addition to the work's requirement row.

> **Why.** `timeout!` still accepts any work and keeps its deadline error
> separate. Result-returning work opts into a conversion explicitly, so
> retries and nested deadlines keep one statically typed error channel.
