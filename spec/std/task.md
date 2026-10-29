# Task

Status: standard library specification draft.

This chapter defines the part of `std.task` that `lib/std` writes in
ordinary hd over the language tier:

- the `Layer` trait, unit and binary tuple implementations, and `apply!`;
- the `retry` builder, its `Retry` and `SelectiveRetry` policies, and `Backoff`;
- the `timeout` and `with_timeout` builders, their `TimeLimit` policy, and `TimedOut`;
- the `Fallback`, `Guard`, and `Hedge` policies and their builders;
- the `Breaker` state and `CircuitOpen` marker;
- the `map_all!` combinator;
- the `CleanupEnd` enum, which `cancel!()` returns;
- the `BlockOn` enum, which `block_on` returns.

The language tier keeps the `Suspend` protocol, `block_on`, and the
polling combinators `all!` and `race!`, which are compiler intrinsics
([Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators)).

## Retry

`retry` constructs a reusable retry configuration; `Retry.when` selects a
predicate for one error type:

```text
pub struct Retry:
    backoff: Backoff

pub struct SelectiveRetry[E]:
    backoff: Backoff
    when: fn(E) -> bool

pub fn retry(backoff: Backoff) -> Retry
```

1. r[std-task.policy.retry.decl] `std.task` declares the public configuration type `Retry` with a private `backoff: Backoff` field.
2. r[std-task.policy.selective-retry.decl] `SelectiveRetry[E]` is public and has private fields `backoff: Backoff` and `when: fn(E) -> bool`.
3. r[std-task.policy.retry.when] `Retry.when[E](self, predicate: fn(E) -> bool) -> SelectiveRetry[E]` stores that predicate with the retry configuration.
4. r[std-task.policy.retry.layer] `Retry` implements `Layer[fn!() -> Result[T, E] $ R]` with `Wrapped = fn!() -> Result[T, E] $ R + Clock`.
5. r[std-task.policy.selective-retry.layer] `SelectiveRetry[E]` implements `Layer[fn!() -> Result[T, E] $ R]` with `Wrapped = fn!() -> Result[T, E] $ R + Clock`.
6. r[std-task.policy.retry.builder] `retry(backoff: Backoff) -> Retry` constructs a retry configuration.
7. r[std-task.policy.retry.loop] An applied retry policy calls work sequentially at most `backoff.attempts` times, returning the first `.Ok` without another call.
8. r[std-task.policy.retry.last-error] When every attempt returns `.Err`, an applied retry policy returns the last error.
9. r[std-task.policy.retry.zero-once] An `attempts` of zero counts as one, so an applied retry policy always calls work at least once.
10. r[std-task.policy.retry.predicate] After `.Err(error)`, a `SelectiveRetry` application retries only when its predicate returns `true`.
11. r[std-task.policy.retry.all-errors] A `Retry` application retries every error until its attempt limit.
12. r[std-task.policy.retry.when-plain] A selective predicate is an ordinary function, takes the error, and runs at most once for each `.Err`.
13. r[std-task.policy.retry.cancel] Cancelling an applied retry policy cancels its active attempt or sleep and starts no further attempt.
14. r[std-task.policy.retry.clock] Executing either retry policy requires `Clock`, including when `backoff.initial` is `0ms`.

```text
use std.task.{Backoff, apply, retry}
use std.time.Clock

struct Server:
    busy_for: i32

fn ask!(server: &mut Server) -> Result[string, string]:
    if server.busy_for > 0:
        server.busy_for = server.busy_for - 1
        return .Err("busy")
    .Ok("ready")

fn connect!(server: *mut Server) -> Result[string, string] $ Clock:
    apply!(retry(Backoff { initial: 0ms })):
        ask!(server)
```

With `busy_for` at 2, `connect!` returns `.Ok("ready")` on the third call.
With `busy_for` at 3, it returns `.Err("busy")` after three calls.
With `attempts` at 0, an applied retry policy would make one call.

```text
use std.task.{Backoff, apply, retry}
use std.time.Clock

fn is_busy(error: string) -> bool:
    error == "busy"

fn connect!(replies: *mut List[Result[string, string]]) -> Result[string, string] $ Clock:
    apply!(retry(Backoff { attempts: 5, initial: 0ms }).when(is_busy)):
        replies.remove_at(0)
```

With replies `.Err("busy")`, `.Err("refused")`, `.Ok("ready")`, `connect!` returns
`.Err("refused")` after two calls: `is_busy` rejects the second error, so a third
call never starts.

The row parameter `R` lets an attempt require providers. An attempt that
calls `println` has the row `Console`, and applying its retry policy then
requires `Console` and `Clock`.

> **Why.** Configuration and execution are separate ordinary library code.
> One Backoff configures immediate and delayed attempts, both using Clock.
> Zero attempts still makes one attempt, because a result needs an attempt.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Suspending Closures And Clauses](../lang/07-functions.md#suspending-closures-and-clauses).

## Retry With Backoff

`Backoff` configures the retry policies' attempt count and delays:

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
4. r[std-task.policy.retry.sleep] An applied retry policy calls the covering `Clock.sleep!` between attempts, with no sleep after the final attempt.
5. r[std-task.retry.policy.delay.capped] The first delay is `initial` and later delays multiply the previous delay by `factor`, each capped at `max` without overflow.
6. r[std-task.policy.retry.when-sleep] A `SelectiveRetry` application sleeps only before another attempt that its predicate allowed.

```text
use std.task.{Backoff, apply, retry}
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
    apply!(retry(policy)):
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

`timeout` constructs a reusable deadline configuration for Result-returning
work:

```text
pub struct TimedOut:
    pub limit: Duration
    pub cleanup: CleanupEnd

pub struct TimeLimit:
    limit: Duration
    cleanup: Duration? = .None

pub fn timeout(limit: Duration, cleanup: Duration? = .None) -> TimeLimit
```

1. r[std-task.policy.time-limit.decl] `std.task` declares the public configuration type `TimeLimit` with private fields `limit: Duration` and `cleanup: Duration?`.
2. r[std-task.policy.time-limit.layer] `TimeLimit` implements `Layer[fn!() -> Result[T, E] $ R]` with `Wrapped = fn!() -> Result[T, E] $ R + Clock` when `E < From[TimedOut]`.
3. r[std-task.policy.timed-out.from-self] `TimedOut` implements `From[TimedOut]` by returning its argument unchanged.
4. r[std-task.timeout.fields-value] `std.task` declares the struct value `TimedOut` with the public fields `limit`, a `Duration`, and `cleanup`, a [`CleanupEnd`](#cleanup-end). `TimedOut` implements `Eq`, `Debug`, `Display`, and `Error`.
5. r[std-task.timeout.display] The `Display` text of a `TimedOut` is `timed out after ` followed by the `Display` text of `limit`. When `cleanup` is `.Abandoned`, the text ends with `, cleanup abandoned`.
6. r[std-task.policy.timeout.builder] `timeout(limit: Duration, cleanup: Duration? = .None) -> TimeLimit` constructs a deadline configuration.
7. r[std-task.policy.time-limit.race] A `TimeLimit` execution races the work against the covering `Clock.sleep!(limit)`, with the work polled first.
8. r[std-task.policy.time-limit.completed] When work completes first, a `TimeLimit` execution returns its Result unchanged.
9. r[std-task.policy.time-limit.expired] When the deadline wins, a `TimeLimit` execution returns `.Err(E::from(TimedOut { limit: limit, cleanup: end }))`, with `end` set to the work's cleanup outcome.
10. r[std-task.timeout.tie] A `work` that completes on its first poll wins even when `limit` is 0, since `race!` polls its children in order.
11. r[std-task.policy.time-limit.cancel-loser] A `TimeLimit` execution cancels the losing child and waits for its cleanup before returning.
12. r[std-task.policy.time-limit.cancel] Cancelling a `TimeLimit` execution cancels its work and deadline children.

```text
use std.task.{TimedOut, apply, timeout}
use std.time.{Clock, s, sleep}

fn lookup!() -> string $ Clock:
    sleep!(5s)
    "value"

fn lookup_in_time!() -> Result[string, TimedOut] $ Clock:
    apply!(timeout(1s)):
        let result: Result[string, TimedOut] = .Ok(lookup!())
        result
```

On a real clock, `lookup_in_time!` returns
`.Err(TimedOut { limit: 1s, cleanup: .Finished })` after one second, and the
5 second sleep is cancelled. On a `ManualClock`, every sleep completes at once,
so `lookup!` finishes first and the result is `.Ok("value")`.

> **Why.** The policy uses the existing race and cancellation rules.
> Work is polled first, so immediately completing work wins a zero-limit tie.
> The timeout error converts through the work's declared error type.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Clock Helpers](time.md#clock-helpers), [Manual Clock](time.md#manual-clock).

### Cleanup Bound

A `cleanup` argument bounds how long a `TimeLimit` execution waits for the cleanup of
`work` once `work` is cancelled:

```text
use std.ops.AsyncDisposable
use std.task.{TimedOut, apply, timeout}
use std.time.{Clock, s}

struct Upload:
    log: *mut List[string]
    ack: mut Suspend[void]

impl AsyncDisposable for Upload:
    fn dispose!(&mut self) -> void:
        defer: self.log.push("upload closed")
        let mut ack = self.ack
        ack!()                          # waits for the peer
        self.log.push("upload acknowledged")

fn send!(log: *mut List[string], reply: mut Suspend[string], ack: mut Suspend[void]) -> Result[string, TimedOut] $ Clock:
    apply!(timeout(5s, cleanup=1s)):
        using! upload = &mut Upload { log: log, ack: ack }
        let result: Result[string, TimedOut] = .Ok(reply!())
        result
```

When neither `reply` nor `ack` completes, the limit cancels the work at 5
seconds, and its dispose waits for `ack`. At 6 seconds the policy abandons the work. The log reads `upload closed`, and `send!` returns
`.Err(TimedOut { limit: 5s, cleanup: .Abandoned })`, whose text is
`timed out after 5s, cleanup abandoned`. When `ack` completes in time, the log
also holds `upload acknowledged`, and `cleanup` is `.Finished`.

1. r[std-task.policy.time-limit.cleanup-unbounded] Without `cleanup`, a `TimeLimit` execution waits for the whole cleanup of cancelled work, however long it takes.
2. r[std-task.policy.time-limit.cleanup-deadline] With `cleanup`, the work has a cleanup deadline measured from its first cancellation, whether caused by expiry or cancellation of the execution.
3. r[std-task.policy.time-limit.cleanup-fixed] Later cancellation of the `TimeLimit` execution never moves its cleanup deadline.
4. r[std-task.policy.time-limit.cleanup-outlives] Cancellation of a `TimeLimit` execution does not stop its cleanup bound.
5. r[std-task.policy.time-limit.cleanup-abandon] When its cleanup deadline passes before cleanup ends, the `TimeLimit` execution calls `abandon()` on the work.
6. r[std-task.timeout.cleanup-cooperative] The deadline is cooperative. `abandon()` runs at the first point at or after the deadline where the cleanup waits, so synchronous code is never interrupted.
7. r[std-task.timeout.cleanup-ended] A cleanup that ends before `abandon()` runs is not abandoned.
8. r[std-task.timeout.cleanup-zero] A `cleanup` of 0 abandons the cleanup at its first wait, so `limit` then bounds all of the waiting.
9. r[std-task.timeout.cleanup-end] `TimedOut.cleanup` is `.Abandoned` when the deadline cut the cleanup short, or when another part of it was cut short, by [`std-task.cleanup-end.cut-short`](#r-std-task.cleanup-end.cut-short). Otherwise it is `.Finished`.

The deadline is set once, at the first cancel:

| Time | Event |
| --- | --- |
| t0 | `apply!(timeout(5s, cleanup=1s))` starts its work. |
| t5 | The limit passes. The work is cancelled, and the deadline is t6. |
| t5.2 | The caller cancels the execution. The cleanup goes on, and the deadline stays t6. |
| t6 | The cleanup has not ended, so the policy abandons the work. |

Had the caller cancelled the execution at t3 instead, the deadline would be t4.

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
6. r[std-task.map-all.zero-limit] A `limit` of zero counts as one, so calls run one after another.
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
> lock. A `limit` of 0 counts as 1, as for retry attempts.

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
4. r[std-task.cleanup-end.best-effort.using-split] `.Finished` does not mean that every close succeeded: a `dispose!` returns `void`, by [`flow.using.best-effort.v2`](../lang/06-control-flow.md#r-flow.using.best-effort.v2).
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
> `Result`, as [`flow.using.best-effort.v2`](../lang/06-control-flow.md#r-flow.using.best-effort.v2)
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

`with_timeout` places a deadline outside an inner policy through a binary
tuple; applying the tuple keeps the work's error type:

```text
pub fn with_timeout[I](limit: Duration, inner: I, cleanup: Duration? = .None) -> (I, TimeLimit)
```

1. r[std-task.policy.builders.requirement-free] `timeout`, `with_timeout`, `retry`, and `Retry.when` are ordinary functions with empty requirement rows.
2. r[std-task.policy.builders.configuration] Constructing these configurations neither starts work nor acquires a provider.
3. r[std-task.policy.execution.apply] `apply!` is the public terminal for executing these policies.
4. r[std-task.policy.execution.private] The execution atoms underlying these policies are private implementation details.
5. r[std-task.policy.time-limit.work-cancelled] External cancellation of work propagates cancellation through its `TimeLimit` execution rather than returning a timeout error.
6. r[std-task.policy.with-timeout.builder] `with_timeout[I](limit: Duration, inner: I, cleanup: Duration? = .None) -> (I, TimeLimit)` returns `(inner, timeout(limit, cleanup=cleanup))`.
7. r[std-task.policy.time-limit.bound] The `TimeLimit` implementation requires `E < std.convert.From[TimedOut]`. Error: `unsatisfied-trait-bound`.
8. r[std-task.policy.time-limit.flat] Nested `TimeLimit` applications preserve the work's Result type.
9. r[std-task.policy.time-limit.clock] Executing a `TimeLimit` requires `Clock` in addition to the work's requirement row.

> **Why.** A time policy preserves one statically typed error channel.
> Plain work is lifted into a typed Result before applying the policy.
> The identity conversion lets that error remain `TimedOut`.

## Policy Layers

`Layer` describes work transformations and their resulting types.
The foundation uses ordinary unit and binary tuple values.

```text
use std.task.apply

pub fn answer!() -> usize:
    apply!(()):
        42
```

`answer!()` returns `42` without requiring `Clock`.

### Layer

1. r[std-task.layer.decl] `std.task` declares the public trait `Layer[W]`.
2. r[std-task.layer.wrapped] `Layer[W]` declares the associated type `Wrapped`.
3. r[std-task.layer.wrap] Its ordinary method is `wrap(self, work: W) -> Self::Wrapped`.

### Unit Layer

1. r[std-task.layer.unit.impl] `()` implements `Layer[W]` for every work type `W`, with `Wrapped = W`.
2. r[std-task.layer.unit.identity] The unit implementation's `wrap` returns its `work` argument unchanged.

### Tuple Layers

1. r[std-task.layer.tuple.impl] `(I, O)` implements `Layer[W]` when `I` implements `Layer[W]` and `O` implements `Layer[I::Wrapped]`.
2. r[std-task.layer.tuple.wrapped] That tuple implementation binds `Wrapped` to `O::Wrapped`.
3. r[std-task.layer.tuple.order] Its `wrap` returns `self._1.wrap(self._0.wrap(work))`, applying the first component's wrapper before the second component's wrapper.

```text
use std.task.Layer

struct Prefix:
    text: string

impl Layer[string] for Prefix:
    type Wrapped = string
    fn wrap(self, work: string) -> string:
        self.text + work

pub fn wrapped() -> string:
    (Prefix { text: "first:" }, Prefix { text: "second:" }).wrap("work")
```

`wrapped()` returns `"second:first:work"`: the first component wraps before
the second component.

### Apply

1. r[std-task.apply.decl] `std.task` declares the public function `apply![T, U, $R, $S, P < Layer[fn!() -> T $ R, Wrapped = fn!() -> U $ S]](policy: P, work: fn!() -> T $ R) -> U $ S`.
2. r[std-task.apply.wrap-once] When executed, `apply!` calls `policy.wrap(work)` exactly once.
3. r[std-task.apply.invoke-once] It then bang-calls the returned function exactly once.
4. r[std-task.apply.result] It returns that function call's value unchanged.
5. r[std-task.apply.row] Its declared requirement row is the wrapped function's row `S`, without an added `Clock` requirement.

> **Why.** The associated output permits different input and output types
> and requirement rows. Unit preserves work, while a binary tuple composes
> two ordinary implementations without a separate stack type.

See also: [Associated Types](../lang/09-traits.md#associated-types),
[Associated Type Bindings](../lang/09-traits.md#associated-type-bindings),
[Requirement Polymorphism](../lang/11-requirements-and-suspension.md#requirement-polymorphism),
[Trailing Blocks](../lang/07-functions.md#r-fn.trailing.form).

## Fallback

`fallback` places a backup operation outside an inner policy.

```text
use std.task.{apply, fallback}

fn backup!() -> Result[string, string]:
    .Ok("backup")

pub fn read!() -> Result[string, string]:
    apply!(fallback(backup, ())):
        let failed: Result[string, string] = .Err("offline")
        failed
```

`read!()` returns `.Ok("backup")` without requiring `Clock`.
The unit inner policy makes fallback the only wrapper.

1. r[std-task.policy.fallback.decl] `std.task` declares public `Fallback[F]` with a private field `other: F`.
2. r[std-task.policy.fallback.builder] `fallback[F, I](other: F, inner: I) -> (I, Fallback[F])` returns `(inner, Fallback { other: other })`.
3. r[std-task.policy.fallback.layer] `Fallback[fn!() -> Result[T, E] $ S]` implements `Layer[fn!() -> Result[T, E] $ R]`, with `Wrapped = fn!() -> Result[T, E] $ R + S`.
4. r[std-task.policy.fallback.completed] A fallback application invokes work once, returning any `.Ok` unchanged without invoking the backup.
5. r[std-task.policy.fallback.backup] After work returns `.Err`, a fallback application invokes its backup once and returns that Result unchanged.
6. r[std-task.policy.fallback.cancel] Cancelling a fallback application cancels its active child; cancellation of work does not invoke the backup.
7. r[std-task.policy.fallback.requirement-free] `fallback` is an ordinary function with an empty requirement row; it neither invokes work nor acquires providers.

> **Why.** The concrete backup function retains its result, error and
> requirement types. A backup with another error type needs an explicit
> conversion into the shared error type.

See also: [Policy Layers](#policy-layers), [Unit Layer](#unit-layer).

## Circuit Breaker

`Guard` refuses work after a caller-owned `Breaker` reaches its failure limit.

```text
use std.convert.From
use std.task.{Breaker, CircuitOpen, apply, guarded}

enum Failure:
    Offline
    Open

impl From[CircuitOpen] for Failure:
    fn from(error: CircuitOpen) -> Failure: .Open

pub fn guard_counts!() -> (usize, usize):
    state := &mut Breaker { failures: 0, limit: 2, calls: 0 }
    guard := guarded(state)
    for _i in 0..4:
        _ := apply!(guard):
            let failed: Result[string, Failure] = .Err(.Offline)
            failed
    (state.calls, state.failures)
```

`guard_counts!()` returns `(2, 2)`: two failures open the breaker,
and two later applications refuse work.

1. r[std-task.policy.circuit-open.decl] `std.task` declares the public empty struct `CircuitOpen`.
2. r[std-task.policy.breaker.decl] `std.task` declares public `Breaker` with public fields `failures: usize`, `limit: usize`, and `calls: usize`.
3. r[std-task.policy.guard.decl] `std.task` declares public `Guard` with a private field `state: *mut Breaker`.
4. r[std-task.policy.guard.builder] `guarded(state: *mut Breaker) -> Guard` returns a configuration holding that counted reference.
5. r[std-task.policy.guard.layer] `Guard` implements `Layer[fn!() -> Result[T, E] $ R]`, with `Wrapped = fn!() -> Result[T, E] $ R`, when `E < From[CircuitOpen]`.
6. r[std-task.policy.guard.bound] The guard's error type must implement `std.convert.From[CircuitOpen]`. Error: `unsatisfied-trait-bound`.
7. r[std-task.policy.guard.refuse] When `failures >= limit`, a guard application returns `.Err(E::from(CircuitOpen {}))` without invoking work or changing its counters.
8. r[std-task.policy.guard.calls] Otherwise, a guard application increments `calls` before invoking work once.
9. r[std-task.policy.guard.success] When work returns `.Ok`, its guard resets `failures` to zero.
10. r[std-task.policy.guard.failure] When work returns `.Err`, its guard increments `failures` once.
11. r[std-task.policy.guard.result] A guard application returns the work's completed Result unchanged.
12. r[std-task.policy.guard.shared] Copies of a `Guard` retain the same counted `Breaker` reference.
13. r[std-task.policy.guard.cancel] Cancellation before work returns does not perform the guard's completed-result update of `failures`.
14. r[std-task.policy.guard.requirement-free] `guarded` is an ordinary function with an empty requirement row; it neither invokes work nor acquires providers.

> **Why.** The caller owns and can inspect or change all breaker state.
> A counted reference keeps that state across applications. The minimal
> breaker has no timer, automatic reopening or half-open probe.

See also: [Policy Layers](#policy-layers),
[Reference Types](../lang/13-values-and-references.md#reference-types).

## Hedging

`Hedge` races work against a delayed second invocation of the same work.

```text
use std.task.{apply, hedged}
use std.time.Clock

pub fn quick!() -> Result[string, string] $ Clock:
    apply!(hedged(10ms)):
        let ready: Result[string, string] = .Ok("ready")
        ready
```

`quick!()` returns `.Ok("ready")` on the first poll, before a second
invocation starts. Its declared row still requires `Clock`.

1. r[std-task.policy.hedge.decl] `std.task` declares public `Hedge` with a private field `delay: Duration`.
2. r[std-task.policy.hedge.builder] `hedged(delay: Duration) -> Hedge` constructs a configuration holding that delay.
3. r[std-task.policy.hedge.layer] `Hedge` implements `Layer[fn!() -> Result[T, E] $ R]`, with `Wrapped = fn!() -> Result[T, E] $ R + Clock`.
4. r[std-task.policy.hedge.race] A hedge application races immediate work against a child that calls the covering `Clock.sleep!(delay)` before invoking fresh work.
5. r[std-task.policy.hedge.first-result] The first completed Result wins, whether `.Ok` or `.Err`.
6. r[std-task.policy.hedge.unneeded-copy] If immediate work completes before the delayed child invokes work, the second invocation never starts.
7. r[std-task.policy.hedge.tie] The immediate child is polled first, so immediately completing work wins even when `delay` is zero.
8. r[std-task.policy.hedge.cleanup] A hedge application cancels its losing child and waits for its cleanup under the existing `race!` rules.
9. r[std-task.policy.hedge.cancel] Cancelling a hedge application cancels both children.
10. r[std-task.policy.hedge.requirement-free] `hedged` is an ordinary function with an empty requirement row; it neither invokes work nor acquires providers.

> **Why.** Each invocation creates fresh work. The existing structured
> race joins the loser cleanup; no detached operation or new cleanup
> bound is needed.

See also: [Standard Combinators](../lang/11-requirements-and-suspension.md#standard-combinators),
[Clock](time.md#clock).
