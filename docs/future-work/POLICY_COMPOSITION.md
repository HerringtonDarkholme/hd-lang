# Composing Time And Failure Policies

Status: design proposal, 2026-10-23; domain: standard library API (std.task) and one language feature (spec/lang/11); not accepted.
It answers [#783](https://github.com/HerringtonDarkholme/hd-lang/issues/783) and replaces the earlier version of this file and Composing Work.
Nothing here is accepted behavior or specification.
Under review: [`std.task`](../../spec/std/task.md) (`timeout!`, `with_timeout!`, `retry!`) and [suspending functions](../../spec/lang/11-requirements-and-suspension.md#suspending-functions).

## Contents

- [Problem, users and use cases](#problem-users-and-use-cases)
- [State of the art](#state-of-the-art)
- [Semantics the shapes rely on](#semantics-the-shapes-rely-on)
- [Shape 1: policy values and one apply block](#shape-1-policy-values-and-one-apply-block)
- [Shape 2: one bang function, run now or built as work](#shape-2-one-bang-function-run-now-or-built-as-work)
- [Shape 3: policies as provider middleware](#shape-3-policies-as-provider-middleware)
- [Either instead of From](#either-instead-of-from)
- [The three shapes on the fixed programs](#the-three-shapes-on-the-fixed-programs)
- [Architecture of the recommended design](#architecture-of-the-recommended-design)
- [Extension points](#extension-points)
- [Checklists for the recommended design](#checklists-for-the-recommended-design)
- [Cost](#cost)
- [Stress test](#stress-test)
- [Phasing](#phasing)
- [Recommendation and approvals](#recommendation-and-approvals)
- [Prototypes, sources and parse log](#prototypes-sources-and-parse-log)

## Problem, users and use cases

Every program that talks to a network needs several policies around one operation.
The usual set is a deadline, retry with backoff, a fallback source, a circuit breaker, hedging and tracing.
Users also write their own, such as a rate limiter or a metric.
std must offer one way to compose them, keeping errors static and requirement rows honest.

The users are application authors, mostly coding agents, and the readers who review their code.
Library authors write policies that std did not foresee.
Every shape below is judged on the same six use cases.

| # | Use case | What must hold |
| --- | --- | --- |
| U1 | Timeout per attempt, two attempts with 1 ms backoff, then a backup source, around an HTTP GET | each attempt is cancelled at its deadline; the backup runs once |
| U2 | A stateful circuit breaker over a failing endpoint, called four times | two calls run, two are refused without running the work |
| U3 | A hedged request: a second copy after 10 ms, the first answer wins | the loser is cancelled and its `defer` runs |
| U4 | A user-written tracing policy | written without any std change; adds `Console` only |
| U5 | Code that uses only non-time policies | its row has no `Clock` |
| U6 | A total budget around per-attempt deadlines | expressed by order, with no special policy |

Today hd has `timeout!`, `with_timeout!` and `retry!` as bang functions that take the work last ([std.task](../../spec/std/task.md#timeout)).
A trailing block supplies the work, so U1 is a staircase of three nested blocks.
That runs, and the owner accepted it as the interim shape on 2026-10-22.
There is no fallback, breaker or hedge in std, and no way to build composed work for a pipe.

## State of the art

Every resilience library answers the same eight questions.
The table lists them; the prose after it says how the leading libraries answer each.
Each item is covered by the designs below or excluded with a reason.

| Question | Covered by |
| --- | --- |
| What is the unit of work, and can it run again? | a re-runnable `fn!() -> Result[T, E] $ R` in every shape |
| How do policies compose, and who sets the order? | shape-specific; order is always explicit |
| Where does a timeout's error go? | `E < From[TimedOut]`, as `with_timeout!` does; Either weighed separately |
| How are deadlines and cancellation propagated? | `race!` cancels losers and waits for their cleanup |
| Where does policy state live (breaker, rate limit)? | a `&mut` state value the caller owns |
| How does retry decide what is transient? | a `when: fn(E) -> bool` predicate |
| How are time and other services supplied? | requirement rows; `Clock` only where time is used |
| Bulkheads and rate limiting | excluded: they need a semaphore type std does not have; they are user policies here |

**Effect (TypeScript).** An `Effect<A, E, R>` is a description of work, so `Effect.retry` can run it again ([retrying](https://effect.website/docs/error-management/retrying/)).
Policies are operators from effect to effect, composed in a `pipe`; order of the pipe is order of wrapping.
`Effect.timeout` widens the error with a `TimeoutException`, and `timeoutFail` substitutes a user error ([timing out](https://effect.website/docs/error-management/timing-out/)).
That widening is a union, which hd does not have; `timeoutFail` is the closest relative of hd's `From[TimedOut]`.

Effect gets data-first and pipeable forms from one definition with `dual` ([Function.ts](https://github.com/Effect-TS/effect/blob/main/packages/effect/src/Function.ts)).
`dual` checks the argument count, or a predicate, at each call and returns a curried function when `self` is missing.
It solves argument placement only, because every Effect call already returns a description.
hd's problem is different: `f!(x)` runs now, so the combinator form must also defer.

Effect's limits are the erased-at-runtime union error and services such as `Clock` that are defaults rather than visible in `R`.
Both conflict with the issue's constraints.
Its strength, operators that are plain functions anyone can write, is what shape 2 keeps.

**Tower (Rust).** Work is a `Service<Request>`; a `Layer` maps one service to another ([Layer](https://docs.rs/tower/latest/tower/trait.Layer.html)).
`ServiceBuilder` stacks layers, and "layers that are added first will be called with the request first" ([ServiceBuilder](https://docs.rs/tower/latest/tower/builder/struct.ServiceBuilder.html)).
Its types are static: each layer's output is an associated type, which is shape 1's model.
Its limit is that work must be a service with one request type; errors are usually boxed (`BoxError`) once layers mix error types.

**Polly (.NET) and resilience4j (Java).** Polly builds a `ResiliencePipeline` with `AddRetry`, `AddTimeout` and others ([pipelines](https://www.pollydocs.org/pipelines/)).
The first strategy added is the outer one, so a timeout added before retries is "overarching all retry attempts".
resilience4j chains `Decorators.ofSupplier(supplier).withCircuitBreaker(cb).withRetry(retry).decorate()` ([getting started](https://resilience4j.readme.io/docs/getting-started)).
Both are value pipelines applied to work later, like shape 1, and both report errors as exceptions, which hd rules out.

**Kotlin, tokio, Go and MoonBit.** Kotlin's `withTimeout` runs a suspending block now and cancels it with `TimeoutCancellationException` ([withTimeout](https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-core/kotlinx.coroutines/with-timeout.html)).
tokio's `timeout` wraps one future and returns `Result<T, Elapsed>`; the future is consumed, so it cannot be retried ([timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html)).
Go passes a `context.Context` whose deadline cancels every derived context; `ctx.Err()` is `DeadlineExceeded` ([context](https://pkg.go.dev/context)).
MoonBit's `with_timeout(time, f, error?)` and `retry(strategy, max_retry?, fatal_error?, f)` take the work last, like hd today ([moonbitlang/async](https://mooncakes.io/docs/moonbitlang/async)).

The lesson from tokio is the one that matters most for hd.
A cold future, like hd's plain call, runs once; retrying needs something that builds a fresh attempt each time.
Kotlin and MoonBit are hd's block form; Effect, Polly and Tower are the combinator and value forms.

## Semantics the shapes rely on

**Work.** A suspending closure `fn!() -> Result[T, E] $ R` runs its body on every call, and its row keys are resolved at each call ([`req.with.dynamic`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.dynamic)).
A plain call `f(x)` of a bang function builds a cold `mut Suspend[T]` instead ([`req.suspend.type.plain-call`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.type.plain-call)).
That suspension binds its providers when it is built ([`req.bind.construction`](../../spec/lang/11-requirements-and-suspension.md#r-req.bind.construction)) and can be driven once.
A naive design that retries a cold suspension drives a finished one; the counterexample is `retry!(b, attempt=fetch(url))`, a `type-mismatch` today.

**Rows.** Rows are sets, so `$ Http + Clock + Clock` is `$ Http + Clock` ([`req.row.set.duplicate`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.set.duplicate)).
A struct, enum or trait declares no row parameter ([`req.row.param.no-data`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.param.no-data)).
So a policy value that stores work stores it behind a type parameter `F`, never `$S`.
A row-polymorphic body is compiled once and receives its row's providers as one bundle ([`req.poly.one-body`](../../spec/lang/11-requirements-and-suspension.md#r-req.poly.one-body)).

**Generics.** Closures are monomorphic, so a policy that works for every `T` and `E` cannot be a closure.
An impl can be generic, though: `impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for Trace` covers every work type.
Its associated type names the result ([`trait.assoc.declare`](../../spec/lang/09-traits.md#r-trait.assoc.declare)), and a bound can fix that projection ([`trait.assoc.bound`](../../spec/lang/09-traits.md#r-trait.assoc.bound)).

A trait method's own generics must match the trait's bounds exactly ([`trait.impl.generics.fixed-bounds`](../../spec/lang/09-traits.md#r-trait.impl.generics.fixed-bounds)).
So one generic method cannot carry `E < From[TimedOut]` for one policy and no bound for another.

**Calls and pipes.** A trailing block supplies only the last parameter ([`fn.trailing.one`](../../spec/lang/07-functions.md#r-fn.trailing.one)).
A pipe step's `_` is a value bound before the step runs ([`expr.pipe.slot.meaning`](../../spec/lang/05-expressions.md#r-expr.pipe.slot.meaning)).
`_` inside a closure in a step is an error ([`expr.pipe.slot.closure`](../../spec/lang/05-expressions.md#r-expr.pipe.slot.closure)), and `f(_, a)` is not a partial application ([`expr.pipe.no-partial`](../../spec/lang/05-expressions.md#r-expr.pipe.no-partial)).
A positional `_` fills the next positional parameter, including a defaulted one, so `with_timeout(ms(50), _)` puts the work in `cleanup`.

**Concurrency and cleanup.** `race!` returns the first child's value and cancels the losers, waiting for their disposes ([`req.combinator.race-losers-disposed`](../../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-losers-disposed)).
`timeout!` races the work against a sleep, work first, so work that finishes on its first poll wins a tie ([std.task timeout](../../spec/std/task.md#timeout)).
`retry!` sleeps only before another allowed attempt and requires `Clock` ([`std-task.retry.policy.clock`](../../spec/std/task.md#r-std-task.retry.policy.clock)).
Every shape builds its policies from these three, so cancellation behaves the same in all of them.

**Ownership.** Values copy; `&mut` references are shared and not exclusive.
A breaker state passed as `&mut Breaker` keeps its counts across calls, and hedged copies share the caller's `&mut` state.
A copied policy configuration never shares state unless it holds a reference.

## Shape 1: policy values and one apply block

### Std API signatures

A policy is a value whose type implements `Layer[W]` for the work types it accepts.
`Policy` is a one-method trait that gives every policy `then`, the composition form.
`a.then(b)` runs `a` inside `b`, so a chain reads from the work outward, as Polly and Tower order theirs.
This is the complete proposed surface of shape 1 in `std.task`.

```text
pub trait Policy:
    fn then[O](self, outer: O) -> Stack[Self, O]:
        Stack { inner: self, outer: outer }

pub trait Layer[W] < Policy:
    type Wrapped
    fn wrap(self, work: W) -> Self::Wrapped

pub struct Stack[I, O]:
    inner: I
    outer: O

impl[I, O] Policy for Stack[I, O]
impl[W, I < Layer[W], O < Layer[I::Wrapped]] Layer[W] for Stack[I, O]

pub fn apply![T, U, $R, $S, P < Layer[fn!() -> T $ R, Wrapped = fn!() -> U $ S]](policy: P, work: fn!() -> T $ R) -> U $ S

pub struct TimeLimit:
    pub limit: Duration
    pub cleanup: Duration? = .None

pub struct Retry[E]:
    pub backoff: Backoff
    pub when: fn(E) -> bool

pub struct Fallback[F]:
    pub other: F

pub struct CircuitOpen: pass

pub struct Breaker:
    pub failures: usize
    pub limit: usize
    pub calls: usize

pub struct Guard:
    pub state: &mut Breaker

pub struct Hedge:
    pub delay: Duration
```

Each policy struct has `impl Policy` and one generic `Layer` impl.

| Policy | Layer impl | `Wrapped` |
| --- | --- | --- |
| `TimeLimit` | `impl[T, E < From[TimedOut], $R] Layer[fn!() -> Result[T, E] $ R]` | `fn!() -> Result[T, E] $ R + Clock` |
| `Retry[E]` | `impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R]` | `fn!() -> Result[T, E] $ R + Clock` |
| `Fallback[fn!() -> Result[T, E] $ S]` | `impl[T, E, $R, $S] Layer[fn!() -> Result[T, E] $ R]` | `fn!() -> Result[T, E] $ R + S` |
| `Guard` | `impl[T, E < From[CircuitOpen], $R] Layer[fn!() -> Result[T, E] $ R]` | `fn!() -> Result[T, E] $ R` |
| `Hedge` | `impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R]` | `fn!() -> Result[T, E] $ R + Clock` |

`apply!` takes `U` apart from `T`, so a policy may change the result type, as the Either variant below does.
`Fallback` stores its backup as `F` because a struct cannot declare `$S`.
`Retry[E]` fixes `E` through its predicate, while `T` and `R` stay open, so one `Retry` value serves every result type with that error.

`then` lives on its own trait. As a default method of `Layer[W]`, the call `TimeLimit { ... }.then(...)` cannot pick one instantiation.
That fails with `unsatisfied-trait-bound`.

An operator was tried and rejected.
`impl[P < Policy, O] Add[O] for P` is `bare-parameter-impl-target` ([`trait.target.bare-parameter`](../../spec/lang/09-traits.md#r-trait.target.bare-parameter)).
An operator would need a per-policy `impl Add`, which every user policy would repeat.
`then` costs a user policy one line, `impl Policy for Trace`.

### Prototypes

[shape1.hd](composing-work/shape1.hd) holds the whole surface, a user policy and the three programs written with `apply!(policy):` blocks.
It passes `hd check`, and `hd FILE` traps in `apply!`:

```console
internal error: unreachable
    shape1.hd:39 shape1.apply
    shape1.hd:198 shape1.traced
    shape1.hd:202 shape1.main
```

[shape1-controls.hd](composing-work/shape1-controls.hd) is the same file with `apply!`'s two lines written at each call site, where every type is concrete.
It runs and prints the same results as shape 2 below, so every policy, `then`, `Stack` and the row typing work today.
That file is a test of the gap, not the proposed user code.

#### The compiler gap

This is a bug in today's use-site row inference, not a missing language feature.
[shape1-gap.hd](composing-work/shape1-gap.hd) reduces it to 17 lines, with no policy, suspension or generic impl:

```text
trait Make:
    type Out
    fn make(self) -> Self::Out

struct Hello: pass

impl Make for Hello:
    type Out = fn() -> void $ Console
    fn make(self) -> (fn() -> void $ Console):
        fn() -> void $ Console: println("hello")

fn call[$S, P < Make[Out = fn() -> void $ S]](maker: P) -> void $ S:
    f := maker.make()
    f()

pub fn main() -> void $ Console:
    call(Hello {})
```

`hd check` passes, and `hd FILE` reports:

```console
internal error: unreachable
    shape1-gap.hd:20 shape1-gap.call
    shape1-gap.hd:23 shape1-gap.main
```

Four controls locate it.

| Change to the repro | Result |
| --- | --- |
| explicit row at the call, `call::[$ Console, Hello](Hello {})` | prints `hello` |
| empty row, `Out = fn() -> void` | runs |
| a type parameter fixed the same way, `Out = fn() -> T`, returning `T` | prints the value |
| a `get` that returns `f`, which `main` calls at its concrete type | still traps |

So `S` is solved while checking, through the binding `Out = fn() -> void $ S`, but the call's instantiation does not carry it.
The likely mechanism is [`req.poly.one-body`](../../spec/lang/11-requirements-and-suspension.md#r-req.poly.one-body): the caller passes `S`'s providers as one bundle, and that bundle lacks `Console`.

The fix belongs in the checker's call instantiation.
A row solved through an associated-type binding must reach the call's row arguments, as a type parameter already does.
Its test is shape1-gap.hd as a runtime conformance case, plus the four controls.
The trait-solver fix da5a1f75a repaired the same kind of loss for row parameters in impl heads.

### A user-defined policy

A user policy is a struct, `impl Policy`, and one `Layer` impl, ten lines in all.
The impl names what it adds to the row; here that is `Console` and nothing else.

```text
struct Trace:
    label: string

impl Policy for Trace

impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for Trace:
    type Wrapped = fn!() -> Result[T, E] $ R + Console
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Console):
        fn!() -> Result[T, E] $ R + Console:
            result := work!()
            println("trace ${self.label}: ${if result.is_ok(): "ok" else: "failed"}")
            result

fn traced!() -> Result[usize, string] $ Console:
    apply!(Trace { label: "pure" }):
        count!()
```

`traced!` checks with `$ Console` alone, which is U5.
A policy that needs a bound states it on its impl, as `Guard` states `E < From[CircuitOpen]`.

### How errors and rows type-check

The U1 pipeline is the worked example: `fetch!` returns `Result[string, Failure] $ Http`.

```text
backup := fn!() -> Result[string, Failure] $ Http: fetch!("http://backup/item")
policy := TimeLimit { limit: ms(50) }
    .then(Retry { backoff: Backoff { attempts: 2, initial: ms(1) }, when: transient })
    .then(Fallback { other: backup })
response := apply!(policy):
    fetch!("http://primary/item")
```

1. `policy` has type `Stack[Stack[TimeLimit, Retry[Failure]], Fallback[fn!() -> Result[string, Failure] $ Http]]`. `Retry`'s `E` is `Failure` from `transient: fn(Failure) -> bool`.
2. The trailing block is the work `W0 = fn!() -> Result[string, Failure] $ Http`, so `T = Result[string, Failure]` and `R = Http`.
3. `apply!` needs `P < Layer[W0, Wrapped = fn!() -> U $ S]`. The outer `Stack` impl needs the inner `Stack < Layer[W0]`, which needs `TimeLimit < Layer[W0]`.
4. `TimeLimit`'s impl matches with `T = string`, `E = Failure`, `R = Http`, and requires `Failure < From[TimedOut]`, which the program implements. `W1 = fn!() -> Result[string, Failure] $ Http + Clock`.
5. `Retry[Failure] < Layer[W1]` matches with `R = Http + Clock`. Its `Wrapped` is `$ Http + Clock + Clock`, which is the set `$ Http + Clock`. That is `W2`.
6. `Fallback[fn!() -> Result[string, Failure] $ Http] < Layer[W2]` matches with `R = Http + Clock`, `S = Http`. `W3 = fn!() -> Result[string, Failure] $ Http + Clock`.
7. So `U = Result[string, Failure]` and `S = Http + Clock`. The enclosing `main!` has `$ Console + Clock` and an `Http` provider scope, so the call checks.

Without `impl From[TimedOut] for Failure`, step 4 fails, but the report is worse than shape 2's.
The checker says `TimeLimit does not implement Layer[fn!() -> Result[usize, Plain]]`, plus two `cannot-infer-type` errors, and never names `From[TimedOut]`.
Shape 2 reports `Plain does not implement From[TimedOut]` alone.

### Cost per run

Measured allocations and Wasm function entries per run are in [Cost](#cost).
Shape 1 runs there as the controls do (wrap, then call), because `apply!` traps.
`apply!` adds one call frame and nothing else, so the control is its cost less one frame per application.
A composed `then` chain is a struct of structs, built with no allocation; each `wrap` allocates one closure per layer.

### The three programs

All three are in shape1.hd, written with `apply!`; each prints the expected line in shape1-controls.hd.

```text
let mut breaker = Breaker { failures: 0, limit: 2, calls: 0 }
guard := Guard { state: &mut breaker }
for i in 0..4:
    outcome := apply!(guard):
        fetch!("http://down/item")
    println("breaker $i ${debug(outcome)}")

let mut state = Replicas { started: 0, finished: 0, closed: 0 }
result := apply!(Hedge { delay: ms(10) }):
    replica!(&mut state)
```

A total budget around per-attempt deadlines (U6) is one more `then`; this composition was checked and run in the control form.

```text
policy := TimeLimit { limit: ms(50) }
    .then(Retry { backoff: Backoff { attempts: 3, initial: ms(1) }, when: transient })
    .then(TimeLimit { limit: ms(200) })
    .then(Trace { label: "budget" })
```

Shape 1 has one cost the others lack under "one way to do each thing".
`TimeLimit` and `Retry` are written over `with_timeout!` and `retry!`, which stay in std.
Either those become private, and every one-off timeout becomes `apply!(TimeLimit { limit: ms(50) }):`, or std keeps two spellings of each policy.

## Shape 2: one bang function, run now or built as work

### Std API signatures

Each policy is one bang function that takes the work last, named `work`.
`with_timeout!` and `retry!` already have this form; shape 2 adds three functions and renames one parameter.
This is the complete surface of shape 2 in `std.task`.

```text
pub fn timeout![T, $R](limit: Duration, cleanup: Duration? = .None, work: fn!() -> T $ R) -> Result[T, TimedOut] $ R + Clock
pub fn with_timeout![T, E < From[TimedOut], $R](limit: Duration, cleanup: Duration? = .None, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock
pub fn retry![T, E, $R](policy: Backoff, when: fn(E) -> bool = fn(error: E): true, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock
pub fn fallback![T, E, $R, $S](other: fn!() -> Result[T, E] $ S, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + S
pub fn guarded![T, E < From[CircuitOpen], $R](breaker: &mut Breaker, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R
pub fn hedged![T, E, $R](delay: Duration, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock

pub struct CircuitOpen: pass

pub struct Breaker:
    pub failures: usize
    pub limit: usize
    pub calls: usize
```

`retry!`'s last parameter is `attempt` today; the rename to `work` gives every pipe step the same `work=_`.
No file in lib, spec or website passes `attempt=` today, so the rename touches only the signature and the spec text.
`fallback!` joins the two rows; `guarded!` adds no row; `hedged!` adds `Clock` for its delay.

The two forms come from that one definition.
The block form is the bang call, and runs now: `with_timeout!(ms(50)):` with the work below.
The combinator form is the plain call in a pipe step: `with_timeout(ms(50), work=_)`, which returns work.
There is no `timed`, `retried` or `deferred` function, and no second definition to keep in step.

### Prototypes

[shape2.hd](composing-work/shape2.hd) holds the additions, a user policy and the three programs in block form.
It runs today and prints:

```console
trace pure: ok
pure Result.Ok(9)
http Result.Ok("200") requests=3
breaker 0 Result.Err(Failure.Http(HttpError.Connect(message="down")))
breaker 1 Result.Err(Failure.Http(HttpError.Connect(message="down")))
breaker 2 Result.Err(Failure.Open)
breaker 3 Result.Err(Failure.Open)
breaker calls=2 failures=2
hedge Result.Ok("replica 2") started=2 finished=1 closed=2
```

The program asserts the counts as well: two primary replies cancelled before they finish, two breaker calls, one hedge completion and two closed scopes.

#### The missing feature

The combinator form needs one language change: **a plain call of a bang function builds re-runnable work.**
Today it builds a one-shot cold `mut Suspend[T]`, which a policy cannot run twice.
[shape2-gap.hd](composing-work/shape2-gap.hd) is the minimal repro; under the proposed rule it prints `hello` twice.

```text
fn twice![T, $R](work: fn!() -> T $ R) -> T $ R:
    work!()
    work!()

fn hello!() -> void $ Console: println("hello")

pub fn main!() -> void $ Console:
    twice!(hello())
```

```console
error: shape2-gap.hd:13:12: type-mismatch: in argument: expected fn!() -> ?, found mut Suspend[()]
```

[shape2-pipe.hd](composing-work/shape2-pipe.hd) writes the three programs in combinator form, with shape2.hd's functions unchanged.
`hd check` reports eight errors, and each is this feature.
Seven are the same `type-mismatch` at a plain call; the eighth, `not-writable` at `call!()`, is the readonly `Suspend` that `call := ...` binds today.

The proposed rule, in full:

1. A plain call `f(args)` of a suspending function or function value has type `fn!() -> T $ R`, where `T` and `R` are the bang call's result and row.
2. Its arguments, defaults included, are evaluated once, left to right, when the call is evaluated; each call of the work runs `f!` with them.
3. Building the work needs no provider; calling it resolves the row at that call, as for any closure ([`req.with.dynamic`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.dynamic)).
4. Where `mut Suspend[T]` or `Suspend[T]` is expected, the plain call builds the cold suspension as today, with construction-time binding. This covers `let s: mut Suspend[T] = f()`, `race!` arguments, `all!` and `block_on`.
5. A plain call that would capture a `mut Suspend` argument as work is an error, because the work could not run twice. Error: `one-shot-capture`.
6. Work that is built and never used keeps today's `discarded-must-use-value` error.

The no-feature alternatives were tried and set aside.
A closure in a step, `|> fn!(): with_timeout!(ms(50), work=_)`, is `pipe-placeholder-in-closure`.
A bound closure works, but spells every step's full type and is a thunk, not the policy's combinator form.
A generic `deferred(f, args...)` adapter works too; the last version of this design used it, and the owner rejected it.

### A user-defined policy

A user policy is one bang function, four lines, with nothing else to implement.
Both forms follow from it: `trace!("pure"):` runs now, and `trace("pure", work=_)` builds work.

```text
fn trace![T, E, $R](label: string, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Console:
    result := work!()
    println("trace $label: ${if result.is_ok(): "ok" else: "failed"}")
    result

fn traced!() -> Result[usize, string] $ Console:
    trace!("pure"):
        count!()

fn traced_later!() -> Result[usize, string] $ Console:
    count() |> trace("pure", work=_) |> _!()
```

`traced!` runs today; `traced_later!` needs the plain-call rule.
Both check with `$ Console` alone, which is U5.

### How errors and rows type-check

The U1 pipeline in combinator form, with the proposed rule:

```text
response := fetch("http://primary/item")
    |> with_timeout(ms(50), work=_)
    |> retry(Backoff { attempts: 2, initial: ms(1) }, when=transient, work=_)
    |> fallback(fetch("http://backup/item"), work=_)
    |> _!()
```

1. `fetch("http://primary/item")` is work `W0 = fn!() -> Result[string, Failure] $ Http` by rule 1.
2. `with_timeout(ms(50), work=_)` binds `work: fn!() -> Result[T, E] $ R` to `W0`: `T = string`, `E = Failure`, `R = Http`. Its bound `Failure < From[TimedOut]` holds. By rule 1 the step is `W1 = fn!() -> Result[string, Failure] $ Http + Clock`.
3. `retry(...)` binds `E = Failure` from both `when` and `W1`, and `R = Http + Clock`. Its result row `$ Http + Clock + Clock` is the set `$ Http + Clock`: `W2`.
4. `fallback(fetch("http://backup/item"), work=_)`: `other` is `fn!() -> Result[string, Failure] $ Http`, so `S = Http`; `work` is `W2`, so `R = Http + Clock`. `W3 = fn!() -> Result[string, Failure] $ Http + Clock`.
5. `_!()` calls `W3`, which needs `$ Http + Clock` at that point; the enclosing scope provides both.

The block form types the same way, inside out, because each trailing block is the `work` argument of the call around it.
No step names a type or a row, and every inference is use-site inference.
A missing `From[TimedOut]` reports `Failure does not implement From[TimedOut]` at the `with_timeout` step.

### Cost per run

Measured in [Cost](#cost), for the block form and for the closures the plain-call rule builds.
The block form allocates one closure per trailing block, as today.
The combinator form allocates one closure per plain call when the pipeline is built; a pipeline built once and run many times pays that once.

### The three programs

U1 in block form runs today (shape2.hd), and in combinator form needs the rule (shape2-pipe.hd):

```text
backup := fn!() -> Result[string, Failure] $ Http: fetch!("http://backup/item")
response := fallback!(backup):
    retry!(Backoff { attempts: 2, initial: ms(1) }, when=transient):
        with_timeout!(ms(50)):
            fetch!("http://primary/item")
```

With the rule, the block form's `backup` closure becomes `fallback!(fetch("http://backup/item")):` too.
U2 and U3 in combinator form; the breaker builds one work value and calls it four times:

```text
let mut breaker = Breaker { failures: 0, limit: 2, calls: 0 }
call := fetch("http://down/item") |> guarded(&mut breaker, work=_)
for i in 0..4:
    println("breaker $i ${debug(call!())}")

let mut state = Replicas { started: 0, finished: 0, closed: 0 }
result := replica(&mut state) |> hedged(ms(10), work=_) |> _!()
```

U6 is one more step after `retry`: `|> with_timeout(ms(200), work=_)`.
The block form nests one block per line, each the `work` of the call around it; no shape needs stacked blocks such as `retry!(…), timeout!(…):`.

## Shape 3: policies as provider middleware

### Std API signatures

Tower's model in hd is a provider that wraps another provider of the same trait.
Each policy is an `Http` implementation holding the provider it wraps, plus the providers it uses itself.
A builder function per policy captures `Clock` where the stack is built ([`req.with.lexical`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.lexical)).

```text
pub fn timed(inner: &mut dyn Http, limit: Duration) -> &mut dyn Http $ Clock
pub fn retried(inner: &mut dyn Http, backoff: Backoff) -> &mut dyn Http $ Clock
pub fn falls_back(inner: &mut dyn Http, from: string, to: string) -> &mut dyn Http
pub fn broken_by(inner: &mut dyn Http, state: &mut Breaker) -> &mut dyn Http
pub fn hedged(inner: &mut dyn Http, delay: Duration) -> &mut dyn Http $ Clock
```

The same five functions are needed again for `Net`, `Process`, `Fs` and every other provider trait.
The `Http` trait fixes the error type, so the breaker spells an open circuit as `HttpError.Other("circuit open")`.

### Prototypes

[shape3.hd](composing-work/shape3.hd) runs today and prints the U1, U2 and U3 results, with the same assertions.
A pipe composes the stack, and one `$.with` block applies it:

```text
stack := &mut delayed_http
    |> timed(_, ms(50))
    |> retried(_, Backoff { attempts: 2, initial: ms(1) })
    |> falls_back(_, "http://primary/", "http://backup/")
$.with(Http=stack):
    response := fetch!("http://primary/item")
```

### A user-defined policy

A user policy is a struct and an `impl Http`, about twelve lines, for each provider trait it applies to.
A tracer for `Http` and `Net` is two such impls.
It cannot wrap work that is not a provider call, such as a function that reads a file and then sends a request.

### How errors and rows type-check

`fetch!` keeps `$ Http`, and no policy appears in its row or its error type.
`timed`'s own row has `Clock`, at the place that builds the stack, so `Clock` appears only where time is configured.
Errors cannot grow: a timeout must already be an `HttpError` (it is, `HttpError.Timeout`), and a breaker has no static error of its own.
Rows cannot grow either, so a policy that needs `Console` must capture it lexically like `Clock`.

### Cost per run

Measured in [Cost](#cost).
Each request through the stack makes one `&mut dyn Http` call per layer, enters one `$.with` per timed layer, and copies the `Request` value.

### The three programs

All three run in shape3.hd.
The hedge sends the same request twice; the breaker's refusals print as `HttpError.Other(message="circuit open")`.
Per-call policies need a different stack and another `$.with`, so a total budget around one call is a second nested block.

## Either instead of From

A policy that adds an error could return `Result[T, Either[E, Added]]` with a std `Either[A, B]` enum, instead of requiring `E < From[Added]`.
[either-errors.hd](composing-work/either-errors.hd) writes the U1 pipeline that way, in shape 2's block form, and runs today.

| | `E < From[TimedOut]` | `Either[E, TimedOut]` |
| --- | --- | --- |
| glue code | one `impl From` per domain error and policy error | none |
| the caller's match after timeout and breaker | `.Err(.Timeout)` | `.Err(.Left(.Right(_)))` |
| changing policy order | no change to matches | every match path changes |
| `retry`'s `when` | classifies `Failure` | classifies `Either[HttpError, TimedOut]` |
| fallback of work with a different error | same `E`, no lift | backup lifted by hand to `Either[HttpError, TimedOut]` |
| later move to unions (#804) | signatures change only | signatures and every match change |

The prototype shows each row of that table.
Its `transient` must handle `.Right(_)`, and its backup needs a hand-written lift before `fallback!` accepts it.
A two-policy error is `.Err(.Left(.Left(.Connect(_))))` for the original failure.

Either removes the glue but moves the cost to every caller and every reorder.
From keeps one named error through a long chain, which is what a reviewer reads.
Recommendation: keep `From`, as the constraint states; Either is not proposed for std.
Both work with every shape: `apply!` already separates `T` and `U`, and shape 2's functions may return any error type.

## The three shapes on the fixed programs

| | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- |
| U1 timeout, retry, fallback | `then` chain, one `apply!` | nested blocks, or a pipe | provider stack |
| U2 breaker | `apply!(guard):` | `guarded!(&mut b):`, or reusable work | provider; error is a string case |
| U3 hedge | `apply!(Hedge {...}):` | `hedged!(ms(10)):` | provider; same request twice |
| U4 user policy | struct, `impl Policy`, `impl Layer` (10 lines) | one bang function (4 lines) | struct and `impl Http` per trait (about 12 lines each) |
| U5 no Clock without time | yes | yes | yes |
| U6 total budget | one more `then` | one more step or block | another `$.with` |
| runs today | checks; `apply!` traps (compiler bug) | block form yes; combinator form needs the rule | yes |
| language change | none (a bug fix) | plain call builds work | none |
| one definition per policy | no: struct beside `with_timeout!`, or the bang functions go private | yes | no: one per provider trait |

The table summarizes the sections above; the argument for each row is there.

## Architecture of the recommended design

The recommendation is shape 2 with the plain-call rule, so this section is that rule's implementation.

**Where it lives.** The checker's two plain-call sites in `crates/hd_check/src/call.rs`.
One is `emit_call` for a call of a known item, and one is the function-value path that emits `Tag::CallValue` for a cold call.
Both already know the callee suspends, the instantiated result `T`, the row `R` and the expected type.
No new TIR instruction, mono rule or backend change is needed.

**Data.** No new data structure.
The checker reads the expected type it already has (`want`) and the argument refs it already built, defaults included.

**Data flow.**

1. Check the callee and arguments as today.
2. If the call is a bang call, or the callee does not suspend, stop: today's path.
3. If the expected type is `Suspend[_]` or `mut Suspend[_]`, or the call is an argument of `all!`, emit the cold suspension: today's path.
4. Otherwise, report `one-shot-capture` for any argument of type `mut Suspend[_]`.
5. Emit work: a call of the std helper `std.function.deferred(f, args...)`, which returns `fn!() -> T $ R`. `f` is the callee as a function value, with a method's receiver as its first argument.
6. Record the result type `fn!() -> T $ R`; do not check `R` against the caller's row ([`req.bind.caller`](../../spec/lang/11-requirements-and-suspension.md#r-req.bind.caller) no longer applies to work).

The helper is three lines of hd, already proven in the last version's prototype with full use-site inference.
It stays private to std: users never write it, and its name does not appear in diagnostics.

```text
fn deferred[Args < Tuple, O, $R](f: SuspendFn[Args, O, $ R], args...: Args) -> (fn!() -> O $ R):
    fn!() -> O $ R:
        f!(args...)
```

**Pseudo-code** for the branch, in the checker's style:

```rust
fn plain_suspending_call(&mut self, c: &Callee, refs: &[Ref], ret: Ty, row: Row, want: Option<Ty>, n: Node) -> (Ref, Ty) {
    // Rule 4: a Suspend slot keeps the cold suspension and its binding.
    if want.is_some_and(|w| self.is_suspend_type(w)) || self.in_all_argument {
        return self.emit_cold_call(c, refs, ret, n); // today's code
    }
    // Rule 5: work runs again, so it cannot hold a one-shot argument.
    for (i, r) in refs.iter().enumerate() {
        if self.is_suspend_type(self.ty_of(*r)) {
            self.err(n, "one-shot-capture",
                format!("argument {} is a one-shot suspension; work built from this call could not run twice", i + 1));
        }
    }
    // Rule 1-3: the callee as a value, then `deferred(f, args...)`.
    let f = self.callee_value(c, n);           // fn item, closure, or method with receiver first
    let work_ty = self.pool().suspend_fn(&[], ret, row); // fn!() -> T $ R
    let helper = self.std_item("std.function.deferred");
    let call = self.emit_item_call(helper, &[f].iter().chain(refs).copied().collect::<Vec<_>>(), work_ty, n);
    // Rule 6: must-use stays on the value.
    self.mark_must_use(call);
    (call, work_ty)
}
```

**Caching and incremental behaviour.** None of its own: the branch is local to one call expression.

**Testing.** Conformance cases, each a runtime fixture:

| Case | Shows |
| --- | --- |
| `twice!(hello())` prints twice | rule 1, 2 |
| a plain call under a `$.with` that the call site does not cover, called inside one | rule 3 |
| `let s: mut Suspend[i32] = $.with(...): read()` keeps the construction provider | rule 4 (the existing construction-provider-capture fixture) |
| `race!(work(), other())` unchanged | rule 4 |
| `twice!(consume(job))` with `job: mut Suspend[i32]` | rule 5, `one-shot-capture` |
| `pending := work()` unused | rule 6, `discarded-must-use-value` |

## Extension points

Users extend shape 2 by writing a bang function that takes `work` last, as `trace!` does.
Its bounds and its row are ordinary signature text, so a reader sees what a policy needs from the declaration alone.
There is no registration, trait or capability involved, so a user policy has no more authority than its row grants.

A policy that needs state takes a `&mut` state parameter, as `guarded!` takes `&mut Breaker`.
A policy over a provider trait is still possible as middleware (shape 3), and std's test providers may document it.
That pattern adds no std mechanism, and it is not a second way to compose policies around work.

## Checklists for the recommended design

The recommended design touches two domains: a language feature in spec/lang/11 and std.task API.

### Language feature

| Item | Answer |
| --- | --- |
| Grammar | none: `f(args)` already parses everywhere; the formatter is unchanged |
| Static semantics | rules 1 to 6; the work's type is the bang call's result and row; use-site inference only |
| Dynamic semantics | arguments evaluate once, left to right, at the plain call; a panic there happens before any run |
| Soundness | below |
| Diagnostics | `one-shot-capture` (new); `discarded-must-use-value` and `type-mismatch` as today |
| Agent writing and reading | `!` means run now, no `!` means work; a `Suspend` slot is the only exception, and it is written as a type |
| Spec and tests | the four retired and six new rules in spec/lang/11; the five cases in the testing table |
| No-feature alternative | blocks only, as today; or bound closures; or a public `deferred`, which the owner rejected |

The interactions that could go wrong, each with the program that shows it:

| Feature | Program | Behaviour under the rule |
| --- | --- | --- |
| captures and `&mut` | `call := fetch(url) \|> guarded(&mut breaker, work=_)` | the reference is captured; state persists across runs, as with a closure |
| provider scopes | `w := fetch(url)` outside a `$.with(Http=...)`, then `w!()` inside | the provider at the call is used (rule 3) |
| construction binding | `let s: mut Suspend[i32] = $.with(Counter=a): read()` | today's binding to `a` (rule 4) |
| `race!` and `all!` | `race!(work(), late_copy(work, d))` | cold suspensions, unchanged (rule 4) |
| one-shot arguments | `twice!(consume(job))` with `job: mut Suspend[i32]` | `one-shot-capture` (rule 5) |
| must-use | `fetch(url)` as a statement | `discarded-must-use-value`, as today |
| generics | `id(fetch(url))` with `fn id[T](x: T) -> T` | `T` is the work type; no `Suspend` is expected, so it is work |
| methods | `client.send(request) \|> retry(b, work=_)` | the receiver is captured like an argument |
| `defer` and cleanup | a work whose body has `defer:` | runs on every run of the work, as for any bang call |
| ownership passes and the inliner | `deferred`'s closure | an ordinary closure; no new instruction reaches them |

Soundness: a plain call either builds today's cold suspension or an ordinary closure that calls `f!`.
Both are existing, checked constructs, so the rule adds no new runtime state.
Rule 5 keeps a one-shot value out of anything that runs twice.
Rule 3 keeps rows honest: the work's type carries `R`, and every run is checked where it happens.

### Standard library API

| Item | Answer |
| --- | --- |
| Layer | plain hd in lib/std/task.hd; no intrinsic; `deferred` is private hd in std.function |
| Design bar (#350) | one function per policy, composed freely; no combined `resilient!` shortcut; strongly typed errors through `From` |
| Signatures | complete in shape 2's API section, with bounds, rows, receivers and bang suspension |
| Naming | verb or participle names like `retry!` and `with_timeout!`; the work parameter is `work` everywhere |
| Performance | measured in Cost; one closure per built step; no per-run allocation beyond the policy's own frames |
| Security | building work needs no provider; running it needs its row; a policy gains no capability it does not name |
| Tests | spec/std/task.md fixtures per rule, plus the three programs as runtime cases |
| Prior art | State of the art; Effect's operators and MoonBit's work-last functions are the closest |

## Cost

### Measured per run

[measure.mjs](composing-work/measure.mjs) builds [cost.hd](composing-work/cost.hd) with `hd build --release --engine node` and rewrites the module.
Every defined function gets an exported counter that its first instruction increments.
The allocator's block total, at offset 244 of its metadata (`GIVEN` in [alloc.hd](../../lib/std/rt/alloc.hd)), is read after the run.
Each variant runs in a fresh instance 4, 20 and 36 times; the per-run cost is the slope, and the two intervals agree within 2%.

The fake `Http` provider answers at once and records nothing, so its cost is constant.
The `fail` path refuses two primary attempts 1 ms apart, then the backup answers: three requests.
The `ok` path succeeds on the first attempt: one request.
Engine: Node 24 (V8), compiler at 2b0db8db4; wasmtime is not in this build.

U1, the `fail` path:

| Variant | Allocations | Function entries | Entries outside the allocator and RC runtime |
| --- | --- | --- | --- |
| the work alone (three requests, one sleep, no policy) | 32 | 1666 | 61 |
| shape 1, wrapped per application | 61 | 3780 | 130 |
| shape 1, wrapped once, run many times | 56 | 3658 | 118 |
| shape 2, blocks | 60 | 3423 | 122 |
| shape 2, pipe built per run | 61 | 3808 | 126 |
| shape 2, pipe built once | 57 | 3699 | 125 |
| shape 3, provider stack | 76 | 5461 | 153 |

U1, the `ok` path:

| Variant | Allocations | Function entries | Entries outside the allocator and RC runtime |
| --- | --- | --- | --- |
| the work alone | 13 | 629 | 21 |
| shape 1, wrapped per application | 31 | 1553 | 62 |
| shape 1, wrapped once | 27 | 1368 | 52 |
| shape 2, blocks | 28 | 1337 | 52 |
| shape 2, pipe built per run | 31 | 1477 | 56 |
| shape 2, pipe built once | 27 | 1361 | 55 |
| shape 3, provider stack | 36 | 2181 | 57 |

U2 and U3, the `ok` path (the breaker closed; the first hedge copy answers at once):

| Variant | Allocations | Function entries |
| --- | --- | --- |
| the work alone | 13 | 580 |
| breaker, shape 1 | 19 | 857 |
| breaker, shape 2 blocks | 16 | 738 |
| breaker, shape 2 work built once | 17 | 816 |
| breaker, shape 3 | 14 | 735 |
| hedge, shape 1 | 24 | 1034 |
| hedge, shape 2 blocks | 20 | 884 |
| hedge, shape 3 | 18 | 967 |

The allocator and reference counting make about 96% of all entries, so allocations decide the call count.
The three U1 policies cost 15 allocations on the `ok` path in block form.
Shapes 1 and 2 differ by at most three allocations per run.

Building a composition once removes four allocations per run in both shapes.
Shape 3 costs most on U1 but least on U2 and U3.

The measured shape-2 pipe is the closures the rule builds, written by hand in cost.hd.
The implementation through `deferred` must not exceed those counts; phase 2's test checks it.
None of this is a latency claim: one real request costs far more than 1500 function entries.

### Bottom-up budget

| Component | Replaces | Adds | Basis |
| --- | --- | --- | --- |
| spec/lang/11, plain call | 4 rules (`req.suspend.type.plain-call`, `req.suspend.type.readonly-binding`, `req.bind.caller` scope, `req.suspend.cold`) | 6 rules, one error code | the rule list above |
| checker branch | 0 | 60 to 100 Rust lines in call.rs | two call sites, the pseudo-code above (about 30 lines each), a diagnostic |
| std helper `deferred` | 0 | 3 hd lines | the earlier prototype, measured |
| conformance | edits to 2 fixtures (unused-cold-suspension, stored-suspension-single-drive) | 5 runtime cases | the testing table |
| std.task additions | 0 | 21 hd lines (`fallback!`, `guarded!`, `hedged!`, `Breaker`, `CircuitOpen`) | shape2.hd part 1, counted |
| spec/std/task.md | `attempt` in 6 retry rules | about 15 rules for three functions | one rule per sentence, as the timeout section |
| shape-1 bug fix (independent) | 0 | 20 to 80 Rust lines and one runtime case | the comparable solver fix da5a1f75a changed 77 Rust lines |

Compile time: the branch is constant work per plain call, with no superlinear step.
Emitted Wasm: a program pays only for the policies it calls; `deferred` is one small generic body per instantiation.
The web compiler's gzip size is not measured here; a 100-line checker branch is far below the 2 MB cap, and phase 2 measures it.
Stop rule: a commit that runs past 1.5 times its line share stops for review.

For comparison, shape 1's std surface is 74 hd lines beside the existing bang functions, and shape 3's is 74 lines for `Http` alone.

## Stress test

Each case was run or checked on today's compiler unless it needs the proposed rule.

1. **One policy value, two result types (shape 1).** `TimeLimit {...}.then(Retry {...})` wrapped a `Result[string, Failure]` and a `Result[usize, Failure]` work; both ran. The generic impl, not a closure, makes this work.
2. **A missing `From` (shape 1).** The checker names `Layer`, not `From[TimedOut]`, and adds two `cannot-infer-type` errors. Shape 2 names the bound. If shape 1 is chosen, an unmet impl bound must be reported by name.
3. **A positional `_` (shape 2).** `with_timeout(ms(50), _)` puts the work in `cleanup` and reports `argument-count`. The answer is `work=_` and the rename of `retry!`'s `attempt`.
4. **Operator composition (shape 1).** A blanket `impl[P < Policy, O] Add[O] for P` is `bare-parameter-impl-target`. The answer is `then`.
5. **`then` on `Layer[W]` (shape 1).** `unsatisfied-trait-bound`, because the receiver implements `Layer` at infinitely many instantiations. The answer is the separate `Policy` trait.
6. **A deadline that must cancel a pending reply.** The fake provider holds primary replies for 500 ms. Every shape reports zero finished primary replies, so the timeout cancels rather than waits.
7. **The hedge loser's cleanup.** `closed=2` in every shape: `race!` waits for the loser's `defer`.
8. **Order mistakes.** `retry` outside `with_timeout` is a deadline per attempt; inside is one budget. Both type-check; only order tells them apart, as in Polly. This is documented, not checked.
9. **Retrying non-idempotent work.** No shape can tell a POST from a GET. The `when` predicate and the caller decide; this matches Effect, Polly and MoonBit.
10. **A one-shot argument under the rule.** `twice!(consume(job))` with `job: mut Suspend[i32]` would drive `job` twice; rule 5 rejects it at the plain call.
11. **A plain call that must stay a suspension.** `race!(work(), late_copy(work, d))` inside `hedged!` and `timeout!`'s `let job: mut Suspend[T] = work()` are Suspend slots, so rule 4 keeps them.
12. **Shape 3 on non-provider work.** A unit of work that reads a file and then sends a request cannot be wrapped; only its `send!` can.
13. **Real library code.** Effect's `getTodo` (timeout, exponential retry, `catchTag` fallback) maps onto the U1 pipeline step for step. resilience4j's bulkhead and rate limiter have no std counterpart and stay user policies.

Cases 2 and 3 changed the design: the rename of `attempt`, and the diagnostic requirement if shape 1 were chosen.

## Phasing

Each step is green on its own and about one hour of agent work.

1. **Spec the plain-call rule.** Edit spec/lang/11 as listed, add the error code, add the five runtime cases. Test: `bash spec/check.sh`; the cases are known failures until step 2.
2. **Checker branch.** Implement the pseudo-code and the private `deferred`. Test: the five cases pass, shape2-gap.hd prints twice, shape2-pipe.hd checks and prints shape2.hd's output; cost.hd's pipe counts do not grow.
3. **std.task additions.** `fallback!`, `guarded!`, `Breaker`, `CircuitOpen`, `hedged!`, and `attempt` renamed to `work`, with spec/std/task.md rules and fixtures. Test: shape2.hd and shape2-pipe.hd run with the std versions.
4. **Guide and examples.** The three programs in the std.task guide, block and pipe forms. Test: the website build.

Independently, the shape-1 compiler bug goes to the compiler lane, with shape1-gap.hd as its runtime case.
It is a bug whatever shape is chosen.

## Recommendation and approvals

**Recommendation:** shape 2, with the plain-call rule.
Each policy is one bang function; `policy!(...):` runs now and `policy(..., work=_)` builds work.

It gives a four-line user policy, the clearest error reports and the lowest measured cost in block form.
No policy has a second definition.
It gives up shape 1's reusable policy values: a composition is reused as built work, not as a configuration applied to different work.

The next best is shape 1, which needs no language change, only the row-inference bug fixed.
It costs a second spelling of every policy, ten lines per user policy, and a diagnostic fix for unmet impl bounds.
Shape 3 is a pattern for provider-backed operations, not a composition mechanism for std.

What needs the owner's approval:

1. **The language change.** A plain call of a bang function builds re-runnable work `fn!() -> T $ R`, except where a `Suspend` is expected. That is rules 1 to 6 above, with the error `one-shot-capture`. Cost: six spec rules, a 60 to 100 line checker branch, two fixture edits.
2. **The std.task additions.** `fallback!`, `guarded!` with `Breaker` and `CircuitOpen`, and `hedged!`, with the signatures above.
3. **The rename** of `retry!`'s `attempt` parameter to `work`.

Keeping `From[TimedOut]` over Either follows the existing constraint and needs no new decision.
The shape-1 bug fix needs no approval: it makes the compiler accept what the spec already allows.

## Prototypes, sources and parse log

Prototypes, run with `hd FILE` (Node engine) from the repository root at 2b0db8db4:

| File | Expected result |
| --- | --- |
| [shape1.hd](composing-work/shape1.hd) | `hd check` passes; `hd FILE` traps `internal error: unreachable` in `apply!` (the compiler gap) |
| [shape1-gap.hd](composing-work/shape1-gap.hd) | the gap's minimal repro: traps the same way |
| [shape1-controls.hd](composing-work/shape1-controls.hd) | runs; the three programs with `apply!` written at the call site |
| [shape2.hd](composing-work/shape2.hd) | runs; the three programs in block form |
| [shape2-pipe.hd](composing-work/shape2-pipe.hd) | the missing feature: `hd check` reports eight errors, all at plain calls |
| [shape2-gap.hd](composing-work/shape2-gap.hd) | the missing feature's minimal repro: one `type-mismatch` |
| [shape3.hd](composing-work/shape3.hd) | runs; the three programs as provider middleware |
| [either-errors.hd](composing-work/either-errors.hd) | runs; U1 with Either errors |
| [cost.hd](composing-work/cost.hd) | runs, for example `hd cost.hd -- http-s2-block fail 3` |
| [measure.mjs](composing-work/measure.mjs) | `node docs/future-work/composing-work/measure.mjs target/debug/hd` prints the cost tables |

Parse log: each `text` block was parsed with `scripts/hd-conformance.sh parse` on 2026-10-23.
Parsing checks syntax only; the type-checking claims come from the prototype files above.

| Block | Content | Result |
| --- | --- | --- |
| 1 | shape 1 std surface | `syntax-error` at `apply!`: a signature without a body; the full definitions parse in shape1.hd |
| 2 | the shape-1 gap repro | parses |
| 3 | `Trace` user policy | parses |
| 4 | U1 with `apply!` | parses |
| 5 | U2 and U3 with `apply!` | parses |
| 6 | U6 total budget | parses |
| 7 | shape 2 signatures | `syntax-error` per line: signatures without bodies; the bodies are in lib/std/task.hd and shape2.hd |
| 8 | the shape-2 missing-feature repro | parses |
| 9 | `trace!` user policy | parses |
| 10 | U1 pipe | parses (it needs the rule to type-check) |
| 11 | U1 blocks | parses |
| 12 | U2 and U3 pipes | parses (they need the rule to type-check) |
| 13 | shape 3 signatures | `syntax-error` per line: signatures without bodies; the bodies are in shape3.hd |
| 14 | shape 3 stack | parses |
| 15 | `deferred` | parses |

The `rust` block is pseudo-code and is not compiled.

Sources: the links in [State of the art](#state-of-the-art), read on 2026-10-23.
The issue comments on #783 hold the owner's review of the earlier version and the compiler update this one relies on.
