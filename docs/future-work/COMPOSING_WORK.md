# Composing Work: Timeouts, Retries and Policies

Status: proposal (2026-10-22), not decided. Every program below was
checked and run with `hd` on main after #755.

## The problem

Real I/O code needs several policies around one operation: a deadline
per attempt, retries with backoff, a total budget, a fallback, a
circuit breaker, logging or tracing. Today std has `timeout!`,
`with_timeout!` (#755) and `retry!`, each a trailing-block function.
Two of them nest, three make a staircase, and nothing composes a policy
std did not foresee. A `Policy` struct with one block (prototype B
below) fixes the staircase but composes only the policies its fields
name.

The goal: one mechanism that composes any policy, including ones users
write, with errors and requirements kept static.

## How Effect does it

An `Effect<A, E, R>` is a description of work, not running work. It can
be run any number of times. Every operator takes an effect and returns
an effect, so a pipe composes them:

```ts
const getTodo = (id: number) => HttpClient.get(`/todos/${id}`).pipe(
  Effect.flatMap(HttpClientResponse.schemaBodyJson(Todo)),
  Effect.timeout("1 second"),
  Effect.retry(Schedule.exponential("100 millis").pipe(Schedule.intersect(Schedule.recurs(3)))),
  Effect.catchTag("ResponseError", () => Effect.succeed(fallbackTodo)))
```

`retry` can re-run what it receives because it is a description. The
order of the steps is the order of wrapping: `timeout` before `retry`
gives each attempt a deadline; after it, one deadline for all attempts.
User operators are ordinary functions of the same shape, so they
compose like std's. Effect v4 (beta, 2026) keeps this model unchanged:
the runtime is rewritten and smaller, `Context.Tag` becomes
`Context.Service`, `catchAll` becomes `catch`, and `retry` takes an
options form (`Effect.retry({ times: 3 })`), but composition is still
operators over effect values.

Two more Effect facts matter here. Effect's timeout widens the error to
`E | TimeoutException`, a union hd does not have (and will not add now).
And Effect's `Clock` is a default service, absent from `R`; hd keeps
requirements visible.

## hd's counterpart of an effect value

hd runs work directly: `get!(url)` runs now. A cold suspension
(`get(url)` without `!`) is not a description either: it runs once. The
re-runnable description already exists in hd: a suspending closure,

```hd
fetch := fn!(): get!(url)          # fn!() -> Result[Response, HttpError] $ Http
```

It runs each time it is called (`fetch!()`), its type carries its
result, its error and its requirement row, and it is an ordinary value.
That is all Effect's model needs.

## Approach A: combinators over work (recommended)

A combinator is an ordinary function that takes work and returns work.
Nothing runs until the end:

```hd
## Each run of the returned work gets `limit`; a timeout becomes E.
pub fn timed[T, E < From[TimedOut], $R](limit: Duration, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
    fn!() -> Result[T, E] $ R + Clock:
        with_timeout!(limit, work=work)

## The returned work retries `work` under `policy` while `when` accepts the error.
pub fn retried[T, E, $R](policy: Backoff, when: fn(E) -> bool, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
    fn!() -> Result[T, E] $ R + Clock:
        retry!(policy, when=when, attempt=work)

## Runs `other` when `work` fails.
pub fn or_else[T, E, $R, $S](other: fn!() -> Result[T, E] $ S, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + S):
    fn!() -> Result[T, E] $ R + S:
        match work!():
            .Ok(value) => .Ok(value)
            .Err(_) => other!()
```

Every combinator takes the work as its **last** parameter, like
`timeout!` and `retry!` do. That gives two spellings of one function,
with no second API: a pipe step with `_` in the last slot, and a
trailing block that supplies the work directly:

```hd
slow := timed(5s):          # the block is the work; nothing runs yet
    get!(url)
response := slow!()         # runs it; Result[Response, HttpError]
```

A pipe composes them, and `|> _!()` runs the result:

```hd
backup := fn!(): secondary!(url)
r := (fn!(): primary!(url))
    |> timed(20ms, _)                                    # each attempt: 20 ms
    |> retried(Backoff { attempts: 2 }, is_timeout, _)
    |> or_else(backup, _)                                # then the other source
    |> _!()
```

Run: `primary` twice (both time out), then `secondary`, giving
`Ok("secondary")`.

User combinators are the same kind of thing. A circuit breaker that
keeps state across calls:

```hd
struct CircuitOpen: pass

struct Breaker:
    failures: usize
    limit: usize

fn guarded[T, E < From[CircuitOpen], $R](breaker: &mut Breaker, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R):
    fn!() -> Result[T, E] $ R:
        if breaker.failures >= breaker.limit:
            return .Err(E::from(CircuitOpen {}))
        result := work!()
        breaker.failures = if result.is_ok(): 0 else: breaker.failures + 1
        result

let mut breaker = Breaker { failures: 0, limit: 2 }
call := (fn!(): down!())
    |> guarded(&mut breaker, _)
# call!() four times: Down, Down, Open, Open (the work is not run once open)
```

Concurrency composes the same way. A hedged request starts a second copy
of the work if the first has not answered after a delay, and keeps
whichever finishes first:

```hd
pub fn hedged[T, E, $R](delay: Duration, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
    fn!() -> Result[T, E] $ R + Clock:
        race!(work(), late_copy(work, delay))

fn late_copy![T, E, $R](work: fn!() -> Result[T, E] $ R, delay: Duration) -> Result[T, E] $ R + Clock:
    sleep!(delay)
    work!()
```

With a slow first replica (200 ms) and a fast second one (10 ms),
`hedged(30ms, _)` answers from replica 2; `race!` cancels the slow copy.

And the Effect `getTodo` above, ported:

```hd
fn get_todo(id: u32) -> (fn!() -> Result[Todo, HttpError] $ Http + Clock):
    (fn!() -> Result[Todo, HttpError] $ Http: get!("/todos/$id")?.json::[Todo]())
        |> timed(1s, _)
        |> retried(Backoff { attempts: 4, initial: 100ms }, HttpError.is_transient, _)
        |> recovered(missing_todo, _)
```

`get_todo(id)` returns the composed work; `get_todo(1)!()` runs it.
Building it once and calling it many times is the natural use.

What this gives:

- **Anything composes.** Std's combinators have no privileges. A
  breaker, a fallback, a tracer, a rate limiter, a hedged request are
  each one function a user can write.
- **Order is explicit.** `timed` then `retried` is a deadline per
  attempt; `retried` then `timed` is one budget. The pipe reads in the
  order the policies apply, from the inside out.
- **Statically typed.** The error stays `E` through every step.
  `timed` converts a timeout through `From[TimedOut]` (as #755 does);
  `guarded` through `From[CircuitOpen]`. No union, no erased error.
- **Requirements compose.** Each step adds to the row: `timed` adds
  `Clock`, a tracer adds `Console`, `or_else` joins two rows
  (`$ R + S`). The checker already handles all three (verified).
- **No language change.** Everything above runs on today's compiler.

What it costs and where it rubs (all found by the prototypes):

1. **One heap closure per step**, made once when the pipeline is built,
   and one indirect call per step per run. Nothing next to a network
   call; a loop that builds a pipeline per element should build it once
   outside.
2. **The pipe head needs parentheses** when it is an inline closure and
   the chain continues on `|>` lines: a same-line closure body is still
   open at the line end (`syntax-error`). `(fn!(): get!(url))` works.
3. **An inline closure cannot precede `_`.** A same-line closure
   argument runs to the end of its line, so `retried(policy, fn(e): …, _)`
   swallows the `, _`. Pass a named function or a method
   (`HttpError.is_transient`), or bind the closure first. Work-last is
   still right: it is what lets a trailing block supply the work.
4. **No multi-line closures in a step** (`expr.pipe.single-line`), so a
   recovery handler with a `match` is a named function.
5. **A closure that uses `?` needs its result type written**
   (`fn!() -> Result[Todo, HttpError] $ Http: …`); without it the
   checker reports `?` on an unknown type.
6. **Running looks unusual**: `|> _!()`, or bind and call (`f!()`).

Items 2 to 6 are small. Item 2 could be lifted by letting a same-line
closure body end before a leading `|>` line; that is a grammar change
and not needed to adopt A.

## Approach B: one block with a policy struct

```hd
response := resilient!(Policy { retry: Backoff { attempts: 3 }, attempt: .Some(5s), total: .Some(20s) }, when: HttpError.is_transient):
    get!(url)
```

Runs on today's compiler (prototype `resilient.hd`). One block, no
staircase, a fixed and correct order. But it composes only what
`Policy` names: no fallback, no breaker, no user policy, and every new
policy is a std change. Effect's experience is the counter-argument:
schedules and operators compose because they are values and functions,
not fields.

## Approach C: policies as provider decorators

hd already has a composition point for capabilities: providers. A
policy can be a provider that wraps another provider of the same trait,
like Tower layers in Rust, Effect layers and Go's `RoundTripper`:

```hd
struct Timed:
    inner: &mut dyn Http
    clock: &mut dyn Clock
    limit: Duration

impl Http for Timed:
    fn send!(&mut self, request: Request) -> Result[Response, HttpError]:
        inner := self.inner
        limit := self.limit
        $.with(Clock=self.clock):
            with_timeout!(limit):
                inner.send!(request)

# configured once, at the edge:
$.with(Http=&mut logged):        # logged wraps retried wraps timed wraps the host
    fetch_status!(url)           # application code: `$ Http` only
```

Run with a fake server whose first two sends take 100 ms: two attempts
time out at 20 ms, the third succeeds, the logger prints one line.

What C does best: application code stays `$ Http` with no `Clock` in
its row (the decorator holds its own clock), the policy is
configuration chosen at the edge, and stateful policies (breakers, rate
limits, caches) live naturally in the decorator. What it cannot do: it
applies only to operations behind a provider trait (`Http`, `Net`,
`Process`), not to arbitrary work, and per-call policies need nested
`$.with` blocks.

C is not an alternative to A: a decorator's method is a natural place to
use A's combinators. It is a pattern for the docs and std's test
providers, and needs no new std API beyond A.

## Approaches considered and left

- **Stacked trailing blocks** (`retry!(…), timeout!(…):`). Rejected by
  the owner.
- **Union error types** (`E | TimedOut`), as Effect and Kotlin's
  proposed rich errors do. Flat and typed, but matching and generics
  over unions are a large language feature. Not now (owner).
- **An open error channel**, as MoonBit's `raise Error`. Flat but loses
  the static error type. Ruled out (owner): errors must be statically
  handled.
- **Algebraic effect handlers** intercepting a row's operations (a
  handler for `Http` that retries every `send!` below it). C already
  gives this for provider traits without a new language feature.
- **Function decorators** (`@policy(timed(5s), retried(…)) fn fetch!…`).
  A language feature whose only gain over A is syntax at the
  definition; A's `fetch := (fn!(): …) |> timed(…)` already names the
  composed work.

## Recommendation

1. **A is the composition mechanism.** Add `timed`, `retried`,
   `or_else` and `recovered` to std.task as ordinary functions, work
   first, with spec rules and conformance cases from the programs
   above. More combinators (`tapped`, `hedged`, `limited`) come when a
   real program needs them.
2. **The block forms stay as the one-shot spelling** of the same
   operation: `timeout!` (any work, `Result[T, TimedOut]`),
   `with_timeout!` (flat through `From`), `retry!`. Each block form is
   defined as its combinator applied and run, so there is one
   implementation and the docs say which is which: a block for one
   policy at one call site, a pipe when policies combine or the work is
   reused. (Effect's two shapes, data-first and pipeable, come from one
   `dual` definition dispatched on arity; hd's `_` placeholder gives the
   same two shapes from one function. Effect has no run-now block form;
   with work-last, `timed(d):` plus a body already covers it, so the
   block forms could also be dropped.)
3. **C is documented as a pattern** in the std.http and std.net
   chapters, with `Timed` and `Retried` decorators written using A.
4. **Grammar follow-up, optional:** let a same-line closure end before
   a leading `|>` line, so the pipe head needs no parentheses.

If the owner prefers a single spelling over the block forms, the cost
is that a one-off timeout is `slow := timed(5s):` with the body below and
then `slow!()`, instead of one `timeout!` block that runs at once.

## Prototypes

Runnable with `hd FILE` from the repository root, in
`docs/future-work/composing-work/`:

- `combinators.hd`: A, with timeout, retry, fallback and a circuit breaker.
- `effect_port.hd`: A, the Effect `getTodo` port against `ScriptedHttp`.
- `hedged.hd`: A, a hedged request over `race!`.
- `layers.hd`: C, timeout, retry and logging as `Http` decorators.
- `resilient.hd`: B, one block with a `Policy` struct.
