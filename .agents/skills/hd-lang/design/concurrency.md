# Concurrency: `!`, Policies, Channels And Threads

A function that can wait has a name ending in `!`. Waiting work runs
together with `join!`, `race!` and `map_join!`, is wrapped in a `std.task`
policy for retries and deadlines, and goes to a thread when it is CPU
work. [concurrent-work](../scenarios/concurrent-work.md) is a complete
program that uses them.

## When

| The problem | Use |
| --- | --- |
| retry a failing call | `apply!(retry(Backoff { .. }))`, never a `for` loop with `sleep!` |
| give up after a deadline | `apply!(timeout(d))`, or `with_timeout(d, policy)` around a retry |
| use a backup when the work fails | `fallback(backup, policy)` |
| stop calling a service that keeps failing | `guarded(state)` with a `Breaker` |
| answer slow calls with a second copy | `hedged(delay)` |
| several independent waits | `join!` for a few, `join!(calls...)` for a list |
| many waits, at most N at a time | `map_join!(items, N, work)` |
| the first answer wins | `race!` |
| hand items from one task to another | `channel` |
| CPU work in parallel | `std.thread`: `scope`, or `background!` from a task |
| test any of this without waiting | install `ManualClock` ([testing](testing.md)) |

## Bang functions, `join!` and `race!`

Every point where a program can pause is visible: the function is `name!`
and every call is written `name!(...)`. The mark travels up the call chain.
`join!` returns the values of several calls in order. `race!` returns the
first to finish and cancels the rest. `map_join!(items, limit, work)` runs
`work` on every item, at most `limit` at a time, and returns the values in
item order. Their call arguments are written **without** `!`: `fetch("a")`
describes work that has not started, and `join!` starts it.

```hd
use std.task.{join, map_join, race}
use std.time.{Clock, ms, sleep}

fn fetch!(name: string, delay: i64) -> string $ Clock:
    sleep!(ms(delay))                       # `1ms` is a Duration; `ms` is also a constructor
    "$name done"

pub fn main!() -> void $ Clock + Console:
    let (a, b) = join!(fetch("user", 20), fetch("orders", 10))
    println("$a, $b")
    calls := [fetch("x", 3), fetch("y", 1)]
    println(debug(join!(calls...)))         # a list of calls, spread with `...`
    names := map_join!(["p", "q", "r"], 2, fn!(name: string) -> string $ Clock: fetch!(name, 5))
    println(debug(names))
    println(race!(fetch("slow", 30), fetch("fast", 5)))
```

```output
user done, orders done
["x done", "y done"]
["p done", "q done", "r done"]
fast done
```

- Only a `!` function may make a bang call. Elsewhere the error is
  `bang-call-outside-suspension`: add `!` to the enclosing function and to
  every call of it, up to `main!`. A script may make bang calls at top level.
- Import a bang function by its plain name, `use std.fs.read_text`, and
  call it as `read_text!(...)`. A closure that makes bang calls is `fn!(..)`.
- Writing `join!(fetch!("user", 20), ..)` is a `type-mismatch`: the bang
  call already ran and gave a string. A fixed count gives a tuple, a
  spread list gives a list.
- Duration literals use `ms`, `s`, `min`, `h`; constructors are `ms`, `s`, `minutes`, `h`.
- Tasks take turns: one runs until its next `!` call, then another may run.
  Two tasks never change a value at the same instant, so tasks can share a
  `box` without a lock.
- A trait method may be a bang method, `fn lookup!(&self, ..)`; its
  implementations and calls carry the `!` too.

## Policies: retry, timeout, fallback

`apply!(policy):` runs the indented block under a policy from `std.task`.
The block returns a `Result`. Its error type needs `From[TimedOut]` to
report a deadline: write it as an `@error` variant with `@from`
([errors](errors.md)). Retries and deadlines need `$ Clock`, even with a
zero delay.

| Policy | Does |
| --- | --- |
| `retry(Backoff { attempts, initial, factor, max })` | tries again on `.Err`, sleeping `initial`, then times `factor`, capped at `max` |
| `retry(..).when(pred)` | retries only the errors `pred(error)` accepts |
| `timeout(limit)` | turns a late finish into `.Err(TimedOut)` |
| `with_timeout(limit, policy)` | one deadline around every attempt of `policy` |
| `fallback(backup, policy)` | calls `backup()` when `policy` still fails |
| `guarded(state)` | circuit breaker: refuses work once a caller-owned `Breaker` has `limit` consecutive failures; no timer, so reset `failures` yourself |
| `hedged(delay)` | starts a second copy if the first is not done after `delay`; the first result wins and the loser is cancelled |

`Backoff` defaults are 3 attempts, `100ms`, factor 2, cap `10s`. Zero
attempts still makes one. Policies compose with `|>`, from the work
outward. A refused `guarded` call becomes `From[CircuitOpen]::from(..)` of
your error type, and `hedged` needs `$ Clock`. The program below retries
only a busy answer under one deadline, falls back to a cached value, and
trips a breaker.

```hd
use std.convert.From
use std.task.{Backoff, Breaker, CircuitOpen, TimedOut, apply, fallback, guarded, retry, timeout, with_timeout}
use std.time.{Clock, ManualClock, Timestamp, ms, s, sleep}

@error
@derive(Eq, Debug)
enum CallError:
    @error("busy")
    Busy
    @error("refused")
    Refused
    @error("too slow")
    Slow(@from error: TimedOut)
    @error("circuit open")
    Open

impl From[CircuitOpen] for CallError:           # `CircuitOpen` has no `Error` of its own
    fn from(error: CircuitOpen) -> CallError: .Open

fn is_busy(error: CallError) -> bool:
    error == .Busy

fn ask!(replies: *mut List[Result[string, CallError]]) -> Result[string, CallError]:
    replies.remove(0)

fn call!(replies: *mut List[Result[string, CallError]]) -> Result[string, CallError] $ Clock:
    policy := retry(Backoff { attempts: 5, initial: 10ms }).when(is_busy) |> with_timeout(1s, _)
    apply!(fallback(cached, policy)):
        ask!(replies)

fn cached!() -> Result[string, CallError]:
    .Ok("cached")

fn late!() -> Result[string, CallError] $ Clock:
    apply!(timeout(5ms)):
        sleep!(50ms)
        let result: Result[string, CallError] = .Ok("late")
        result

tests:
    fn clock() -> *mut ManualClock:
        ManualClock::new(Timestamp::from_unix_milliseconds(0))

    it("retries a busy answer, then falls back on a refusal"):
        let replies: *mut List[Result[string, CallError]] = box([.Err(.Busy), .Err(.Refused), .Ok("x")])
        $.with(Clock=clock()):
            assert_equal(call!(replies), .Ok("cached"))
        assert_equal(replies.len(), 1)

    it("stops calling after two failures"):
        let replies: *mut List[Result[string, CallError]] = box([.Err(.Refused), .Err(.Refused), .Ok("x"), .Ok("y")])
        state := box(Breaker { failures: 0, limit: 2, calls: 0 })
        guard := guarded(state)
        for _ in 0..4:
            _ := apply!(guard):
                ask!(replies)
        assert_equal(state.calls, 2)
        assert_equal(replies.len(), 2)

    it("reports a deadline"):
        $.with(Clock=clock()):
            assert_equal(late!().map_err(fn(error): error.to_string()), .Err("too slow"))
```

Install `ManualClock` in tests: its time moves only when every task is
waiting, so sleeps and deadlines cost no real time. `timeout(limit, cleanup)`
also takes a time allowed for the cancelled work's cleanup.

## Cancellation and `block_on`

`race!`, a `timeout` and a lost `hedged` copy cancel the work that did not
win. The cancelled work stops at its next wait, and its `defer` blocks run
before the winner's value returns.

- A stored suspension, `job := fetch("a")`, has `job.cancel!()`, which waits
  for its cleanup and returns a `CleanupEnd` (`.Finished` or `.Abandoned`),
  and `job.abandon()`, which unwinds it at once and cuts each `dispose!`
  short.
- `block_on(suspension)` drives one from code that does not wait. It returns
  `.Completed(value)`, `.Cancelled` or `.Abandoned`. Use it only at the edge
  of synchronous code.
- A program whose only pending waits are on each other panics with
  `deadlock`.
- A signal wait is cancelled by racing it: see
  [concurrent-work](../scenarios/concurrent-work.md#stop-on-ctrl-c).

## Channels

`channel(capacity)` gives a `Sender[T]` and a `Receiver[T]`; copies of a
handle use the same channel. `send!` waits while it is full. `recv!` waits
while it is empty and open, and gives `.None` once it is closed and
drained. `close()` ends it. Annotate the pair: `channel(2)` alone cannot
infer the item type.

```hd
use std.task.{Receiver, Sender, channel, join}

fn produce!(out: Sender[i32]) -> void:
    for item in 0..3:
        out.send!(item)
    out.close()                                 # without it, `consume!` waits forever

fn consume!(input: Receiver[i32]) -> i32:
    let sum = 0
    while true:
        match input.recv!():
            .Some(item) => sum += item
            .None => break                      # closed and drained
    sum

pub fn main!() -> void $ Console:
    let (tx, rx): (Sender[i32], Receiver[i32]) = channel(2)
    let (_, total) = join!(produce(tx), consume(rx))
    println("$total")
```

```output
3
```

A pool of workers is `map_join!` over the worker count, every worker
receiving from one shared `Receiver`.

## Threads and sync

Tasks share one thread. CPU work that should run in parallel goes to
`std.thread`: `scope` and `s.spawn` from code that does not wait,
`background!` from a task.

`scope` waits for every thread it started. `s.spawn` returns a `Handle`,
and `handle.join()` gives the closure's value. A thread that panics makes
`join` (or `scope`) panic with `thread-stopped`. `background!` suspends the
calling task, not its thread, so other tasks go on. Cancelling it returns at
once while the thread finishes and drops its value.

`string`, `List`, `Map` and `Set` are not `Sync`: a thread cannot borrow
them, and `Arc` of one is not `Send`. Pass each thread its own copy, or
share a struct of numbers. A closure given to `background!` captures by
value, and an `Arc` is moved, so clone it into a new name first.

```hd
use std.sync.Arc
use std.task.map_join
use std.thread.{Scope, background, scope}

struct Settings:
    scale: i64
    offset: i64

fn both(settings: &Settings) -> i64:
    scope(fn(s: &mut Scope) -> i64:
        left := s.spawn(fn() -> i64: settings.scale * 2)
        right := s.spawn(fn() -> i64: settings.offset * 2)
        left.join() + right.join()
    )

fn scaled!(settings: &Arc[Settings], items: List[i64]) -> List[i64]:
    map_join!(items, 4, fn!(item: i64) -> i64:
        mine := settings.clone()
        background!(fn() -> i64: item * mine.get().scale + mine.get().offset)
    )

pub fn main!() -> void $ Console:
    println("${both(&Settings { scale: 10, offset: 1 })}")
    settings := Arc::new(Settings { scale: 10, offset: 1 })
    println(debug(scaled!(&settings, [1, 2, 3])))
```

```output
22
[11, 21, 31]
```

`Arc` also has `get_mut` (when no other holder exists), `try_unwrap`,
`ptr_eq` and `downgrade` to a `Weak`. Run `hd doc std.sync` in a package.
