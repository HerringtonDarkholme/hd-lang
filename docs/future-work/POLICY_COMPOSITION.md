# Composing Time And Failure Policies

Status: decided 2026-10-24 (#783, owner); domain: standard library API (std.task). #845 implements the foundation and timeout/retry builders; fallback, breaker, hedge and measured guardrails remain later slices.
It answers [#783](https://github.com/HerringtonDarkholme/hd-lang/issues/783) and replaces the earlier version of this file and Composing Work.
The owner selected policy values, pipes and one apply block on 2026-10-23.
The decision: `apply!` is the only public way to apply a policy; `timeout!`, `with_timeout!` and `retry!` become private building blocks in `std.task`. The implementation follows the phasing below, with the std spec rules and fixtures in the same job. No language change is needed.
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

Before #845, hd exported `timeout!`, `with_timeout!` and `retry!` as bang functions that take the work last ([std.task](../../spec/std/task.md#timeout)).
A trailing block supplies the work, so U1 is a staircase of three nested blocks.
That runs, and the owner accepted it as the interim shape on 2026-10-22.
There is no fallback, breaker or hedge in std yet.
Policy values run today using generic trait implementations and tuples.
The separately approved inferred-apply correction is merged; the public library surface is approved.

## State of the art

The comparison fixes eight responsibilities: repeatable work, composition order, typed errors, cancellation, persistent state, retry selection, capability requirements and extension.
Bulkheads and rate limiting need a semaphore or queue beyond this proposal.
They can later use the same Layer interface, rather than gaining a separate composition system.

**Effect v4.** An Effect describes an operation, with result, failure and service types.
`request.pipe(Effect.retry(schedule), Effect.timeout(duration))` describes retries inside one deadline.
A Schedule decides when another attempt starts and when attempts stop; it does not persist executions.
The [schedule cookbook](https://effect.website/docs/v4/scheduling/cookbook) distinguishes limiting retries from interrupting an already running attempt.

[Retrying](https://effect.website/docs/v4/error-management/retrying) supports conditions and schedules; [timeouts](https://effect.website/docs/v4/error-management/timing-out) interrupt the operation and add a timeout failure.
The implementation's [dual](https://github.com/Effect-TS/effect-smol/blob/main/packages/effect/src/Function.ts) dispatches data-first and data-last forms by arity or a predicate.
That dispatch cannot solve hd's problem alone: a bang function runs now, and its plain call builds a one-shot suspension.
We take explicit order and independent policy values, while keeping hd's existing static From conversions and visible Clock requirement.

**Tower.** A [Layer](https://docs.rs/tower/latest/tower/trait.Layer.html) turns a service into an associated Service type.
The generic implementation remains reusable for different input services, as hd's Layer implementations remain reusable for different work types.
Tower supports tuple layers too, so a separately named Stack is unnecessary here.

[ServiceBuilder](https://docs.rs/tower/latest/tower/builder/struct.ServiceBuilder.html#order) adds the first listed layer outermost.
This proposal instead defines each pipe wrapper as outermost; the difference must be visible in examples.
Tower services also have readiness and request protocols; a zero-argument hd work closure has neither.
We borrow the associated-type mechanism, without importing readiness, boxed errors or a new service protocol.

**Polly and resilience4j.** Polly separates reusable [pipeline configuration](https://www.pollydocs.org/pipelines/) from executing a callback.
The first strategy added is outermost, and context state can be supplied without allocating a capturing lambda.
That separation motivates storing a breaker reference in configuration and applying it repeatedly.
It does not supply hd's typed error or row rules: Polly operates through its own outcome and exception model.

[resilience4j](https://resilience4j.readme.io/docs/getting-started) decorates ordinary functions with retries, breakers and other policies.
Its composition can be user-defined rather than selected from one fixed retry/timeout record.
Its Java functional interfaces and exceptions also avoid hd's varying error and requirement-row types.
We keep extensible decoration, but cannot copy an exception-based signature into Result-based hd.

**Kotlin and tokio.** Kotlin's [withTimeout](https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-core/kotlinx.coroutines/with-timeout.html) executes a suspending block and cancels it on expiry.
Its cancellation exception is not a Result error.
Tokio's [timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html) wraps one future, returning its value or Elapsed.
Dropping the timeout cancels that future; it cannot make an already consumed future repeatable.

hd therefore retains `fn!() -> Result[T, E] $ R` as repeatable work.
It builds fresh cold suspensions only inside each execution, as race and timeout already require.
Neither a stored Suspend nor a new plain-call rule is needed.

**Go and MoonBit.** Go's [context](https://pkg.go.dev/context) propagates scope cancellation and deadlines through derived contexts.
A context is not a generic decorator of one expression and does not encode errors in a static sum.
It informs the cancellation boundary, rather than the public composition syntax.

[MoonBit async](https://mooncakes.io/docs/moonbitlang/async) provides callback-based timeout and retry functions, including retry conditions and bounded retry counts.
Those are close implementation atoms for hd's existing timeout and retry functions.
Our builders instead return configuration, allowing one block around several independently configured policies.
Retries still need application knowledge of idempotency; no library can infer that from a work closure.

## Semantics the shapes rely on

**Work.** A suspending closure `fn!() -> Result[T, E] $ R` runs its body on every call, and its row keys are resolved at each call ([`req.with.dynamic`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.dynamic)).
A plain call `f(x)` of a bang function builds a cold `mut Suspend[T]` instead ([`req.suspend.type.plain-call`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.type.plain-call)).
That suspension binds its providers when it is built ([`req.bind.construction`](../../spec/lang/11-requirements-and-suspension.md#r-req.bind.construction)) and can be driven once.
A naive design that retries a cold suspension drives a finished one; the counterexample is `apply!(retry(b), work=fetch(url))`, a `type-mismatch` today.

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
The previous bang with_timeout had a defaulted cleanup parameter before work.
The proposed plain builder instead takes inner second, so `with_timeout(ms(50), _)` places the policy correctly.

**Concurrency and cleanup.** `race!` returns the first child's value and cancels the losers, waiting for their disposes ([`req.combinator.race-losers-disposed`](../../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-losers-disposed)).
A TimeLimit execution races the work against a sleep, work first, so work that finishes on its first poll wins a tie ([std.task timeout](../../spec/std/task.md#timeout)).
A retry policy sleeps only before another allowed attempt and requires `Clock` ([`std-task.policy.retry.clock`](../../spec/std/task.md#r-std-task.policy.retry.clock)).
The recommended policy implementations reuse these atoms privately, preserving their cancellation semantics.

**Ownership.** Values copy; `&mut` references are shared and not exclusive.
A breaker state passed as `&mut Breaker` keeps its counts across calls, and hedged copies share the caller's `&mut` state.
A copied policy configuration never shares state unless it holds a reference.

## Shape 1: policy values and one apply block

The owner selected this mechanism, after rejecting the verbose Policy/then/Stack version.
A policy is a configuration value implementing `Layer[W]`.
Ordinary tuples compose layers, plain functions construct their configurations, and one apply block supplies the operation.
No provider is captured merely by constructing a timeout or retry policy.

### Std API signatures

The complete reference implementation is [policy-values.hd](composing-work/policy-values.hd).
These are its mechanism definitions, with bodies rather than non-parsing standalone signatures:

```text
trait Layer[W]:
    type Wrapped
    fn wrap(self, work: W) -> Self::Wrapped

impl[W, I < Layer[W], O < Layer[I::Wrapped]] Layer[W] for (I, O):
    type Wrapped = O::Wrapped
    fn wrap(self, work: W) -> O::Wrapped:
        self._1.wrap(self._0.wrap(work))

fn apply![T, U, $R, $S, P < Layer[fn!() -> T $ R, Wrapped = fn!() -> U $ S]](policy: P, work: fn!() -> T $ R) -> U $ S:
    wrapped := policy.wrap(work)
    wrapped!()
```

All proposed public signatures appear below; W denotes an ordinary type parameter, including a function type.
The table is signature notation, not a second source file.
The linked implementation supplies every body.

| Item | Complete signature or fields |
| --- | --- |
| Layer | `pub trait Layer[W]`, associated `type Wrapped`, `fn wrap(self, work: W) -> Self::Wrapped` |
| unit Layer | `impl[W] Layer[W] for ()`, with `Wrapped = W`, preserving the work |
| tuple Layer | the generic `(I, O)` implementation above; inner wraps first, outer wraps second |
| apply | `pub fn apply![T, U, $R, $S, P < Layer[fn!() -> T $ R, Wrapped = fn!() -> U $ S]](policy: P, work: fn!() -> T $ R) -> U $ S` |
| TimeLimit | fields `limit: Duration`, `cleanup: Duration?` |
| timeout | `pub fn timeout(limit: Duration, cleanup: Duration? = .None) -> TimeLimit` |
| with_timeout | `pub fn with_timeout[I](limit: Duration, inner: I, cleanup: Duration? = .None) -> (I, TimeLimit)` |
| Retry | field `backoff: Backoff` |
| retry | `pub fn retry(backoff: Backoff) -> Retry` |
| Retry.when | `pub fn when[E](self, predicate: fn(E) -> bool) -> SelectiveRetry[E]` |
| SelectiveRetry[E] | fields `backoff: Backoff`, `when: fn(E) -> bool` |
| Fallback[F] | field `other: F`, retaining its concrete function type and row |
| fallback | `pub fn fallback[F, I](other: F, inner: I) -> (I, Fallback[F])` |
| Breaker | fields `failures: usize`, `limit: usize`, `calls: usize`; mutable caller-owned state |
| CircuitOpen | `pub struct CircuitOpen: pass` |
| Guard | field `state: &mut Breaker` |
| guarded | `pub fn guarded(state: &mut Breaker) -> Guard` |
| Hedge | field `delay: Duration` |
| hedged | `pub fn hedged(delay: Duration) -> Hedge` |

Configuration fields are private in the proposed public API, except Breaker's observable state.
The local prototype omits pub on these names because it is a single-file program.
The zero-argument function type inside F is not a row parameter on a data declaration.
Builders are requirement-free ordinary functions.

| Layer implementation | Wrapped type and bound |
| --- | --- |
| TimeLimit | `fn!() -> Result[T, E] $ R + Clock`, with `E < From[TimedOut]` |
| Retry | `fn!() -> Result[T, E] $ R + Clock`, retry every Err up to Backoff.attempts |
| SelectiveRetry[E] | same result and row, but retry only when its typed predicate accepts E |
| Fallback[fn!() -> Result[T, E] $ S] | `fn!() -> Result[T, E] $ R + S` |
| Guard | `fn!() -> Result[T, E] $ R`, with `E < From[CircuitOpen]` |
| Hedge | `fn!() -> Result[T, E] $ R + Clock` |

Retry's error type is fixed only when a predicate is supplied or work is applied.
Thus `retry(backoff)` remains reusable across result and error types.
The `.when` builder captures a concrete `fn(E) -> bool`; it does not require polymorphic closures.

The exact requested pipe form has one apply block:

```text
policy := retry(backoff) |> with_timeout(ms(50), _) |> fallback(backup, _)
response := apply!(policy):
    fetch!(url)
```

Here the timeout surrounds all retries, and fallback surrounds the timeout.
A per-attempt deadline has the opposite first pair: `(timeout(ms(50)), retry(backoff))`, then the same fallback step.
A tuple is the composition atom, so a user policy joins any composition without registration or another trait.
This also exposes ordering when there is no dedicated wrapper builder.

A single timeout uses `apply!(timeout(ms(50))):`, with the same trailing work block.
There is no proposed public timeout bang function alongside it.
The implementation keeps today's timeout/retry algorithms as private building blocks, renamed internally where their names conflict with builders.
The migrated executable prototypes delegate through the public `task.apply!`, `task.timeout` and `task.retry` APIs. Their local policy types still illustrate the compared designs; they are not extra public std APIs.

### Prototypes

The lean source is policy-values.hd.
It ports the timed HTTP request with retry and fallback, the stateful breaker, and the hedged request.
Additional controls and measured artifacts are listed in the final parse log.

#### The compiler gap

The inferred associated-output row bug was fixed in [1f9668271](https://github.com/HerringtonDarkholme/hd-lang/commit/1f9668271fcdd12bfa767fb786b825dbf389cf81), before this rewrite.
At the old baseline, `shape1-gap.hd` passed checking but both engines trapped at invocation with `internal error: unreachable`.
Binding checks now solve nested function rows and compare them invariantly before unresolved call rows default to empty.
The correction changed eight production Rust lines; its thirteen benchmark modules stayed byte-identical, totaling 332,211 bytes.

The current reduced repro prints `hello` on both engines.
The complete lean port now runs with inferred `apply!(policy):`; its explicit-row sibling remains a diagnostic control.
The original evidence and the focused compiler regression are linked in [the prerequisite report](https://github.com/HerringtonDarkholme/hd-lang/issues/783#issuecomment-6379809640).

The runtime regression uses inferred calls, rather than substituting explicit row arguments.
No new language rule was necessary.
The separate returned-function row-inference gap found by #818 is described below; it does not block these configuration builders.

### A user-defined policy

Trace is one configuration struct and one Layer implementation.
It has no Policy marker, registration, then method or operator implementation.
It adds Console and no Clock:

```text
struct Trace:
    label: string

impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for Trace:
    type Wrapped = fn!() -> Result[T, E] $ R + Console
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Console):
        fn!() -> Result[T, E] $ R + Console:
            result := work!()
            println("trace ${self.label}: ${if result.is_ok(): "ok" else: "failed"}")
            result
```

Trace is **9 nonblank, noncomment lines**, or **11 literal lines** from struct through impl.
The source keeps no marker trait, registration, wrapper method, or extra composition type.
Its verbosity comes from the configuration field, Layer projection, written function types, and repeatable closure body.

The tuple `(policy, Trace { label: "request" })` applies tracing outside policy.
That construction works for a policy std did not anticipate.
A pure work function under Trace requires Console alone; capability checks must reject accidental Clock use there.

### How errors and rows type-check

Fix `fetch!: fn!(string) -> Result[string, Failure] $ Http` and a backup with the same error and row.
For the per-attempt tuple, the input is `W0 = fn!() -> Result[string, Failure] $ Http`.
TimeLimit requires `Failure < From[TimedOut]` and produces W1 with Http + Clock.

Retry's Layer implementation accepts W1, producing W2 with Http + Clock + Clock, which is the set Http + Clock.
Fallback's function adds Http, leaving W3 with Http + Clock.
Apply solves U as Result[string, Failure] and S as Http + Clock through Layer.Wrapped.
The containing function must provide those requirements; builders themselves require none.

A guard adds the bound `Failure < From[CircuitOpen]` but no row.
An HTTP error reaches Failure through its explicit From[HttpError] implementation.
No union, any Error channel, type-name dispatch or implicit error erasure is used.

A policy that changes the output type can choose another Wrapped result, because apply has separate T and U.
A fallback returning a different error needs an explicit conversion before it can satisfy this implementation.
A missing From bound is a checked error, even if the current diagnostic names an unsatisfied Layer rather than the underlying bound.

### Cost per run

Construction copies configuration and references; apply recursively builds the Layer closures, then starts the final work once.
Retry invokes its inner work afresh; hedge starts two fresh children when the first has not completed.
The measured complete sequence below includes ten apply operations, provider setup, output formatting, and cancellation.
It is not a count of allocations intrinsic to one Layer or one application.

### The three programs

The HTTP program makes two deadline-limited primary attempts and then one backup call.
It asserts three recorded requests and zero completed slow replies, so a late successful response cannot masquerade as cancellation.
The fallback runs outside retry, once after the retrying operation fails.

The breaker retains `&mut Breaker` across four applications.
Its first two operations fail, then two are refused without calling Http.
It is a deliberately minimal consecutive-failure breaker; timed reopening and half-open probing are not implemented or claimed.

The hedge starts one operation, then another after its configured delay.
The fast replica wins; the source asserts two starts, one completion and two defer executions.
Cancellation and cleanup use race's existing structured behavior, rather than a detached task.

## Shape 2: one bang function, run now or built as work

This heading remains for existing links, but this option is rejected.
The owner rejected both the new plain-call rule and the nested-block substitute on 2026-10-23.
The former changed language semantics; the latter added indentation for every policy.
Neither is a fallback recommendation when apply's compiler bug is inconvenient.

### Std API signatures

The historical bang functions took repeatable work last.
Their full definitions remain in [shape2.hd](composing-work/shape2.hd), as evidence for the rejected option.
They are not a second proposed public policy surface.

### Prototypes

shape2.hd is historical nested-block code.
[shape2-pipe.hd](composing-work/shape2-pipe.hd) writes plain calls that today's language does not turn into repeatable work.

#### The missing feature

[shape2-gap.hd](composing-work/shape2-gap.hd) asks twice to take `hello()`, whose actual type is a cold Suspend.
The type mismatch is correct under today's language.
No compiler change is requested for it, and no deferred thunk adapter is presented as the rejected combinator form.

### A user-defined policy

A bang function taking work last is small, but it does not become a pipeable policy by omitting the bang.
An explicit closure can defer it, at the cost the owner rejected.

### How errors and rows type-check

The existing bang functions preserve Result and grow rows explicitly.
That typing remains useful for private implementation atoms in shape 1.
It does not establish that a cold suspension is reusable work.

### Cost per run

Historical shape-2 allocation numbers predate the current compiler and do not select the mechanism.
New measurements below describe only the exact current artifacts they exercise.

### The three programs

Nested timeout/retry/fallback, a breaker function and a hedging function were runnable evidence for this alternative.
They do not meet the single-block requirement.
The recommended ports use policy configurations and one apply block instead.

## Shape 3: policies as provider middleware

The owner ruled this out as the operation-level composition mechanism.
A requirement is injected per scope; a different strategy for one call needs a scope around that call.
Provider middleware remains useful for default strategies, and only that use is retained.

### Std API signatures

There is no proposed std surface for shape 3.
[shape3.hd](composing-work/shape3.hd) preserves the historical Http wrappers, with Clock captured where needed.

### Prototypes

The historical source demonstrates configuration for all Http calls under one provider scope.
It does not demonstrate attaching a policy value to an arbitrary operation.

### A user-defined policy

A provider wrapper implements the relevant provider trait for each protocol it supports.
That is more duplication than one generic Layer implementation, and does not cover work without provider calls.

### How errors and rows type-check

Http fixes HttpError at its boundary, so an open breaker must be translated into that domain error.
A wrapper's captured Clock is a provider reference, unlike the requirement-free policy configuration recommended here.

### Cost per run

Its historical measurements are archived, not current selection evidence.
The missing operation-level boundary decides its rejection regardless of allocation counts.

### The three programs

The provider prototypes apply their strategy to every matching request in a scope.
They cannot independently decorate two arbitrary operations in that scope.
The second operation needs another scope or a different application mechanism.

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
Apply already separates T and U, so an explicitly written Layer could return Either too.
That does not change the current From constraint or require union types.

## The three shapes on the fixed programs

The fixed programs expose the difference in mechanisms.
Policy values apply to one chosen operation, with one block and persistent configuration reusable across work types.
Bang functions need nested blocks, while provider middleware changes a scope rather than that chosen operation.

| Requirement | Lean policy values | Rejected bang/plain-call design | Provider defaults |
| --- | --- | --- | --- |
| one block for any number of policies | yes | no with today's language | one provider scope, not an operation boundary |
| new language rule | none | required for automatic repeatable work | none |
| custom policy | struct plus Layer impl | bang function plus explicit work closure | provider-specific impl |
| U1 timed HTTP retry and fallback | one configured tuple tree | nested blocks | per-scope Http strategy |
| U2 breaker with typed refusal | From[CircuitOpen] | From[CircuitOpen] | must fit provider error |
| U3 hedge including defer cleanup | existing race atom | existing race atom | provider call only |
| untimed work has no Clock | yes | yes | provider-specific |

## Architecture of the recommended design

The public implementation belongs in ordinary hd in std.task, with rules in the stdlib tier.
It needs no parser syntax, checker intrinsic, new TIR instruction, host ABI or compiler cache.
The associated-type row prerequisite has already been fixed and tested separately.

**Data structures.** TimeLimit stores duration and optional cleanup duration.
Retry stores Backoff; SelectiveRetry stores Backoff and a typed predicate.
Fallback stores its concrete backup closure; Guard stores a shared mutable Breaker reference; Hedge stores its delay.
A tuple stores two configurations; it has no runtime policy tag or heterogeneous List.

**Data flow.** Plain builders evaluate configuration once, in ordinary expression order.
Apply receives the trailing block as work, recursively wraps it through the tuple, then calls the final closure once.
Retry and hedge call that repeatable work again when their algorithms require it.
Each execution constructs fresh suspension frames and resolves the requirements at execution.

The implementation algorithm is ordinary hd; the full file is authoritative.
This outline names the distinct phases, including the cancellation boundary:

```rust
fn construct_limit(limit, cleanup) { TimeLimit { limit, cleanup } }
fn construct_retry(backoff) { Retry { backoff } }
fn construct_fallback(other, inner) { (inner, Fallback { other }) }
fn wrap_pair((inner, outer), work) {
    let first = inner.wrap(work);
    outer.wrap(first)
}
fn wrap_timeout(config, work) {
    // Return work, not a started task or a captured Clock provider.
    move || async {
        // The private existing timeout atom creates a fresh child.
        // It races the child with a sleep, then waits for loser cleanup.
        private_with_timeout(config.limit, config.cleanup, work).await
    }
}
fn wrap_retry(config, work) {
    move || async { private_retry(config.backoff, work).await }
}
fn wrap_fallback(config, work) {
    move || async {
        match work().await {
            Ok(value) => Ok(value),
            Err(_) => config.other().await,
        }
    }
}
async fn apply(config, work) { config.wrap(work)().await }
```

The Rust-like pseudo-code does not specify Rust's moves or async runtime for hd.
The hd reference uses value copies, fn! closures, requirements and existing bang atoms.
There is no cache of an operation's result: reusing configuration must not memoize work or an error.
Compiler incremental behavior remains ordinary signatures, implementation bodies and mono instantiations.

Testing follows observable operations: actual timeout cancellation, refused breaker calls, completed hedge requests and closed scopes.
Compile-negative controls check missing From and missing capabilities.
The inferred apply call must run before the API can ship; an explicitly typed control cannot satisfy that test.

## Extension points

A user supplies one configuration type and one Layer implementation for its accepted work types.
Its Wrapped associated type declares new result and row requirements.
Tuples compose it with std or another user's layer, without registration, a Policy marker or a blanket operator impl.

References inside configuration intentionally share state; ordinary copied configuration values do not imply shared state.
A breaker copied by value would reset state between applications, so its constructor requires a reference.
A user layer can inspect or transform work only with the authority visible in its signature and captured references.

The minimal breaker is not a full half-open circuit-breaker framework.
Later state machines, jitter, tracing spans and rate limiting need their own focused API decisions.
They can retain this composition mechanism; no new language extension point is needed.

## Checklists for the recommended design

### Language feature

There is no proposed language feature and no rule ID retirement.
Existing rules cover tuple fields, generic impls, associated outputs, use-site inference, pipes, trailing blocks and cancellation.
The plain-call and call-operator ideas remain rejected or explicitly postponed.

Each pipe step still needs `_`, under `expr.pipe.bare.needs-placeholder`.
Writing `policy!:` would need a call-operator trait or generic function values.
Both are possible future rules recorded as gaps from the ideal, not proposals in this job.

The [#818 stress test](https://github.com/HerringtonDarkholme/hd-lang/blob/b5fe14a15ae610d0e91a4bf4428c027aae101444/docs/future-work/PIPE_STEPS.md) checks a possible zero-placeholder value-step rule separately.
Its curried factories infer inner configuration types and run all three ports through the expected unary function type.
The fixed bodies save three placeholders and no lines; the two factory implementations add two lines.
The uniform rule also changes existing method/default semantics and needs a final-fn! weakening guard.

This rewrite therefore keeps today's explicit `_` spelling; adopting #818 would be a later owner-approved language change.
It is not an excuse to alter the selected policy mechanism now.
The separate nested nonempty returned-function row gap does not affect these configuration-only builders.

The soundness condition is ordinary type soundness: a Layer must return its declared Wrapped type and row.
An accepted inferred apply call must receive every provider required by its associated output.
That invariant is the compiler gap's regression target.

### Standard library API

The layer is plain hd, not an intrinsic.
The ergonomic path is builders, pipe and one apply block; tuples and Layer are the extension atoms.
There is no fixed Policy record enumerating accepted combinations, and no resilient shortcut hiding policy order.

Errors remain statically named, with explicit From conversions.
Receivers take configurations by value; persistent mutable state is referenced explicitly.
Apply is the one terminal operation, and former public timeout bang functions become private building blocks.

The implementation must retain work-first timeout ties, capped backoff, at-least-one retry attempt and loser cleanup.
Native and Node tests must agree with the spec, rather than treating either host as semantic authority.
Capabilities remain visible in Wrapped and the apply call, and a builder acquires none implicitly.

## Cost

### Measured per run

The exact inferred-call reference package was built in the dev profile for Node at source baseline `6c0dc2a31`.
The compiler sources match the frozen `424c34c44` compiler; only documentation changed between those bases.
The unmodified module is **569,376 bytes**, with **862 defined functions**.
Its measurement-only instrumented copy is **593,453 bytes**; that size is not a shipped-code claim.

| Complete port-sequence repetitions | Guest allocation calls | Defined Wasm function entries | Runtime entries, subset |
| --- | --- | --- | --- |
| 0 | 4 | 268 | 255 |
| 1 | 473 | 35,218 | 34,207 |
| 5 | 2,286 | 171,631 | 166,532 |
| 9 | 4,099 | 307,991 | 298,804 |

The extra sequences have **453.25 allocation calls per sequence** on both the 1→5 and 5→9 intervals.
Defined-function entries average **34,103.25** and **34,090.00** per sequence on those intervals.
Fractional slopes describe several executions, not a fraction of one allocation.
Timers can change poll counts; these function-entry counts are observations, not semantic constants or latency measurements.

[measure-values.mjs](composing-work/measure-values.mjs) exports a counter for every defined function and the existing allocator metadata address.
It reads allocation-call total at heap-metadata offset 244, verified against the current `note_allocation` emitter.
It does not count imported host calls, host allocations, Wasm instructions, or maximum live bytes.
Every count uses a fresh instance; the zero-repetition run exposes fixed startup work.
Instrumentation adds globals and entry increments without changing function indices; its output matches the original asserted program.

The command is `node docs/future-work/composing-work/measure-values.mjs PATH_TO_HD WORK_PACKAGE`.
Set `HD_CACHE` and `TMPDIR` inside the isolated worktree; the script uses no release build.
The complete port sequence covers U1–U5, policy reuse and a total retry budget.
Native execution validates the uninstrumented behavior; the allocation/function-entry observations above use Node only.

Old tables from compiler 2b0db8db4 are superseded rather than copied as current measurements.
No latency or speed claim follows from counting allocations and function entries.
Performance does not decide the chosen composition form.

### Bottom-up budget

The historical component counts below are pinned to [policy-values.hd at the #845 migration base](https://github.com/HerringtonDarkholme/hd-lang/blob/8b263e56d5657a52678161fe79c1c166c1c0ffbc/docs/future-work/composing-work/policy-values.hd). The executable [current copy](composing-work/policy-values.hd) has been migrated to public policy calls and current counted-reference annotations. That caller migration does not redefine the measured production-code allowance.
“Code” excludes blank and comment-only lines; literal spans retain internal blanks/comments and exclude separators after the component.
The whole prototype is **275 literal lines / 203 code lines**, including imports, the fake provider, assertions and examples.

| Component | Measured code lines | Literal span | Existing algorithm replaced | Implementation allowance |
| --- | --- | --- | --- | --- |
| Layer, unit, tuple, apply | 13 | 17 | 0; existing timeout/retry/race atoms reused | 13 code lines |
| TimeLimit | 8 | 11 | 0; existing timeout/retry/race atoms reused | 8 code lines |
| Retry and predicate | 18 | 25 | 0; existing timeout/retry/race atoms reused | 18 code lines |
| Fallback | 9 | 12 | 0; existing timeout/retry/race atoms reused | 9 code lines |
| Breaker and Guard | 17 | 23 | 0; existing timeout/retry/race atoms reused | 17 code lines |
| Hedge | 10 | 14 | 0; existing timeout/retry/race atoms reused | 10 code lines |
| Builders | 9 | 10 | 0; existing timeout/retry/race atoms reused | 9 code lines |
| Trace | 9 | 11 | 0; existing timeout/retry/race atoms reused | documentation/example only |
| Port support and bodies | 105 | 130 | 0; existing timeout/retry/race atoms reused | documentation/example only |

The proposed library mechanism and policy components total **84 code lines**; the user Trace example adds nine.
This is a reference-derived implementation allowance, excluding documentation and tests, rather than a Rust compiler budget.
The first migration changes three public bang declaration headers to private atom names and updates their existing callers.
It retains their algorithms; it does not delete the cancellation implementation to meet a line target.
Caller count and migration edits must be inventoried on the implementation base before that slice starts.

No new compiler code or web compiler payload is required for the library design.
The prerequisite compiler fix separately measured web Wasm at **5,700,644 raw / 1,908,698 gzip bytes** on its pinned revision.
That is historical prerequisite evidence, not a fresh current-web measurement or proof about a future std migration.
The user-program module size above is historical prototype evidence; current implementation compile-time and latency deltas are not established by that measurement.

The proposal requires no production Rust or web compiler source change.
The separately merged compiler correction already passed its web build and smoke gates.
The 2 MB web gzip cap cannot be inferred from a Rust line estimate.
Stop each implementation slice if its additions exceed 1.5 times the corresponding measured prototype component.

## Stress test

The six fixed use cases are exercised by the full ports and the clock-free Trace control.
Eleven additional focused programs check boundaries and extension behavior; six are positive controls and five are intentional rejections.

| Focused case | Verdict | Current evidence |
| --- | --- | --- |
| `pure-no-clock` | works | Only Console; both engines print a successful traced result. |
| `unit-policy` | works | The unit Layer leaves work unchanged; both engines return Ok(9). |
| `from-required` | friction | Missing From[TimedOut] rejects the TimeLimit Layer bound. |
| `clock-required` | works | The missing Clock provider is a checked missing-requirement error. |
| `console-required` | works | Trace cannot be applied without its Console requirement. |
| `cold-not-work` | works | A cold Suspend fails the repeatable fn! parameter; secondary diagnostics remain noisy. |
| `plain-result-boundary` | friction | TimeLimit currently accepts Result-returning work, not a plain usize body. |
| `policy-output-transform` | works | A user Layer changes Result work to string output; both engines print rendered. |
| `fallback-unit` | works | fallback(backup, ()) works as the first or only policy. |
| `copied-breaker` | works | Copied Guard values share referenced state; only one underlying call runs. |
| `nested-budgets` | works | A 5 ms total budget surrounds retries of 50 ms attempts; both engines cancel the first slow attempt and return the backup. |

The plain-value boundary is deliberate in the proposed surface: wrap a plain result in a typed `Result` when timing it.
A blanket TimeLimit implementation for every work type would overlap the Result-specific implementation; this proposal does not add specialization.
An initial fallback uses the unit Layer as its inner configuration; no duplicate standalone fallback API is necessary.
Output-changing user layers work because apply has separate input and output type parameters.

The nested-budget control combines a per-attempt deadline and an outer total deadline, rather than testing those only separately.
It asserts one cancelled primary attempt, one backup request and no completed slow response.
The counters test cooperative timeout cancellation and hedge disposal, not a synchronous host call that never yields.
A complete production fixture set still needs cleanup-bound zero/expiry cases and work-first timeout ties from the existing task suite.
Those existing behaviors must survive the private-atom migration; this documentation job does not declare them newly tested.

The library does not promise cancellation of a synchronous host call that never yields.
It preserves the existing cooperative suspension boundary.
Retries of non-idempotent work remain the caller's responsibility; transient error selection cannot make an operation idempotent.

## Phasing

Each slice targets about an hour of agent work and has a concrete observable test.
The prerequisite associated-output row correction is already merged, with inferred-call, explicit, empty-row and invariant-rejection coverage.
Keep that runtime regression in both-engine focused checks; do not reopen the compiler fix as a library phase.

Second, introduce Layer, tuple composition and apply with stdlib rules and fixtures.
The test applies a user layer to two result types and proves an untimed policy needs no Clock.
Third, expose the timeout/retry builders while making their bang implementations private and migrating every existing caller.

Fourth, add fallback, the minimal breaker and hedging with their typed errors and complete signatures.
Their tests are U1 through U3, including actual cancellation counters and loser defer execution.
Finally, update guides and add measured allocation/call guardrails from the current prototype, without making historical counts semantic requirements.

No phase changes the meaning of plain calls or adds an operator trait.
Each production slice must ship its stdlib rules and conformance cases with the implementation.

## Recommendation and approvals

**Recommendation:** the owner's lean policy-value mechanism, using Layer, tuples, plain builders and one apply block.
It gives up the four-line bang-only custom policy, because automatic reusable plain calls and nested application blocks were rejected.
It also gives up provider middleware as an operation-level mechanism; providers may still carry a default strategy.

Approved (owner, 2026-10-24): the std.task surface, and migration of the public bang timeout/retry APIs into private implementation atoms.
The source and exact signatures above are the reference.
From[TimedOut] remains the current constraint; adopting Either or unions is not requested.

The inferred apply correction is complete under existing rules, rather than an API limitation to normalize.
No approval for a new plain-call rule, policy call syntax, generic function value or bare pipe step is sought.

## Prototypes, sources and parse log

| Source | Parse | Check | Execution |
| --- | --- | --- | --- |
| `policy-values.hd` | clean | clean, zero warnings | Node and Wasmtime pass |
| `policy-values-controls.hd` | clean | clean, zero warnings | Node and Wasmtime pass |
| `policy-values-cost.hd` | clean | clean, zero warnings | Node and Wasmtime pass |
| `shape2.hd` | clean | clean, zero warnings | Node and Wasmtime pass |
| `shape2-pipe.hd` | clean | expected rejection of cold Suspend as work | not run after intended rejection |
| `shape2-gap.hd` | clean | expected rejection of cold Suspend as work | not run after intended rejection |
| `shape3.hd` | clean | clean, zero warnings | Node and Wasmtime pass |
| `either-errors.hd` | clean | clean, zero warnings | Node and Wasmtime pass |

The three embedded hd blocks parse cleanly; they are mechanism definitions, the user spelling, and the Trace implementation.
The linked complete policy-values program supplies the missing context for those excerpts.
All eleven focused stress programs parse; six check and run on both engines, while five reject for the documented boundary.
The resulting **24 successful ordinary engine runs** have exact Node/native stdout parity per source.
Four additional instrumented Node executions cover 0, 1, 5 and 9 full sequences, retaining all counts and stdout.

No full test suite or production compiler/std change is part of this rewrite.

Sources are the primary links in State of the art, rechecked on 2026-10-24.
The owner's final direction is the 2026-10-24 03:30 UTC reply on #783.
Earlier shape-2 approvals in the historical version of this file are superseded.
