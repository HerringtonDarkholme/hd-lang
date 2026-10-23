# Composing policies around repeatable work

Status: design proposal, 2026-10-23; nothing here is accepted behavior or specification.
This supersedes the recommendation in [Composing Work](COMPOSING_WORK.md), answering [#783](https://github.com/HerringtonDarkholme/hd-lang/issues/783).
The owner requested exactly two shapes, full ports and one recommendation, without std or compiler edits.
The [owner's follow-up](https://github.com/HerringtonDarkholme/hd-lang/issues/783#issuecomment-6371482714) permits shape 1's specified API plus its demonstrated compiler gap.

## The common contract

Work is a re-runnable `fn!() -> Result[T, E] $ R`, not an already-created suspension.
Policies may retry or hedge that function, so captured arguments and external effects must support those repeated attempts.
The caller chooses wrapper order explicitly; no closed configuration struct lists the only policies that may compose.
Timeouts convert through `E < From[TimedOut]`; breaker refusal similarly uses `E < From[CircuitOpen]`.

Keep errors static, avoid stacked trailing blocks, and add `Clock` only where code uses time.
The constraints come from the issue and [#755's timeout API](../../spec/std/task.md#timeout).
Relevant language mechanisms are [function constructors](../../spec/lang/07-functions.md#function-type-constructors), [associated types](../../spec/lang/09-traits.md#associated-types), [rows](../../spec/lang/11-requirements-and-suspension.md#requirement-rows), and [variadic tuples](../../spec/lang/12-variadic-generics.md).
No union, erased error channel or new syntax is assumed.

The complete API definitions and application ports are in [shape1.hd](composing-work/shape1.hd) and [shape2.hd](composing-work/shape2.hd).
The former is blocked at generic composition; the latter checks and runs on today's main compiler on both engines.
This distinction applies throughout: source for a proposed API is not proof that its blocked application runs.

## Precedents and what hd should borrow

[Effect v4](https://effect.website/docs/v4/error-management/retrying) wraps effect descriptions and makes retry schedules independent values.
Its [schedule model](https://effect.website/docs/v4/scheduling/using-schedules) separates retry after failure from repetition after success.
Borrow the description-of-work boundary and explicit wrapper order, not its union error channel or default service assumptions.

Effect's [Function.dual](https://github.com/Effect-TS/effect/blob/main/packages/effect/src/Function.ts) exposes direct and pipe-friendly calls from one implementation, selected by arity or a predicate.
That solves argument placement, not hd's run-now versus build-repeatable-work distinction.
hd already has `_` for pipe placement; the generic lift below handles staging without an arity-overload mechanism.

[Tower's Layer](https://tower-rs.github.io/tower/tower/trait.Layer.html) maps one service into another through an associated output type.
[ServiceBuilder](https://docs.rs/tower/latest/tower/struct.ServiceBuilder.html) stores a typed stack of layers.
This is shape 1's useful precedent: configurations remain values, while applying them chooses concrete work types.

[Polly](https://www.pollydocs.org/pipelines/index.html) explicitly distinguishes inner and outer resilience strategies.
[Kotlin withTimeout](https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-core/kotlinx.coroutines/with-timeout.html) runs a suspending block within a cancellation scope.
[Tokio timeout](https://docs.rs/tokio/latest/tokio/time/struct.Timeout.html) instead wraps a future value; that does not itself make the future repeatable.

[Go context](https://pkg.go.dev/context) propagates cancellation and deadlines through explicit parameters.
[MoonBit async](https://docs.moonbitlang.com/en/latest/language/async-experimental.html) provides timeout/retry combinators over its cancellation machinery.
These references support using existing suspension and cancellation rather than adding a separate policy scheduler to hd.
They do not establish that hd should copy another language's error or capability model.

## Shape 1: policy values, one apply block

A policy implements `Layer[W]`, where `W` is the entire input work type.
Its associated `Wrapped` type describes the resulting work, including its exact result and row.
`TimeLimit` is a configuration value whose implementations work across `T`, convertible errors and requirement rows.
It is not a monomorphic closure pretending to be a universally applicable policy.

```text
trait Layer[W]:
    type Wrapped
    fn wrap(self, work: W) -> Self::Wrapped

struct Stack[I, O]:
    inner: I
    outer: O

impl[W, I < Layer[W], O < Layer[I::Wrapped]] Layer[W] for Stack[I, O]:
    type Wrapped = O::Wrapped
    fn wrap(self, work: W) -> O::Wrapped:
        self.outer.wrap(self.inner.wrap(work))
```

The proposed terminal signature is:
`apply![T, E, $R, $S, P < Layer[fn!() -> Result[T, E] $ R, Wrapped = fn!() -> Result[T, E] $ S]](policy: P, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ S`.
It builds the wrapped work and invokes it once; a trailing block supplies only `work`.
Nested `Stack` values compose any qualifying user layer, without enumerating policies in a std struct or enum.

`TimeLimit.wrap` maps `fn!() -> Result[T, E] $ R` to `fn!() -> Result[T, E] $ R + Clock`, requiring `E < From[TimedOut]`.
`Retries[E]` adds `Clock` for backoff, while `Fallback[F]` joins the primary and backup rows without adding time authority.
`Guard` retains a reference to breaker state, keeps the work's row, and requires `E < From[CircuitOpen]`.
`Hedge` adds `Clock`; a user-written `Trace` adds only `Console`.

The prototype contains the full declarations, implementations and ports for all six layers.
`Fallback` stores its callback as a type parameter `F`; current hd does not permit data types to declare row parameters directly.
Returned function types in method signatures need parentheses, so their rows belong to the returned function rather than `wrap`.
Neither restriction requires a new API mechanism.

The HTTP port puts `TimeLimit` inside `Retries`, then `Fallback` outside both.
Each attempt has its own deadline; moving another `TimeLimit` outside that stack instead gives a total budget.
The breaker port reuses one referenced state across applications; the hedge port races fresh invocations and cancels the loser.
All three ports are written in `shape1.hd`, but its generic stack currently prevents execution.

### The precise compiler gap

[shape1-gap.hd](composing-work/shape1-gap.hd) reduces the failure to two generic layers, with no HTTP request or resilience implementation.
The inner layer adds `Clock`, the outer preserves its input work, and the work already carries `$ Http`.
Checking the composed stack fails with:

```console
in inferred argument: the function's row lists `Http`, which the expected row does not
```

The [checker controls](composing-work/shape1-controls.hd) check the same generic stack with an empty input row.
They also directly nest the two layers with the nonempty row; both expressions pass checking.
A trait-qualified call and explicit application rows do not make generic stack composition succeed.
An explicit input/output-trait alternative runs into current impl-head restrictions rather than providing an unrestricted replacement.

**This is a bug in today's row inference through an associated-type output, not a missing language feature.**
The current rules already describe row-bearing function types, associated projections and generic implementations.
A compiler fix must retain the substituted input row while normalizing the inner output and selecting the outer layer implementation.
It needs regression coverage for nonempty rows through nested projections, alongside empty-row and direct-call controls.

Fixing it should not weaken row checking, broaden coherence, introduce polymorphic closures or add time to every work signature.
The exact repro, diagnostic and controls are retained in the [issue report](https://github.com/HerringtonDarkholme/hd-lang/issues/783#issuecomment-6371456214).
No compiler edit is included here, and the proposal does not assume the fix has landed.
Shape 1 remains a complete proposed surface with an explicit implementation prerequisite.

Checking is not execution: the combined control executable also fails in Node-target lowering:

```console
error: unsupported: stage Link: values of type `{poison}` in the linear backend is not supported
```

Neither control is claimed to run successfully.
An eventual fix needs end-to-end execution coverage, not merely successful checking; this note does not diagnose that additional lowering failure.

## Shape 2: one bang API, one generic lift

Define each policy once as a bang function taking repeatable work last.
The direct block calls it with `!`, and therefore runs now.
For a pipeline, one ordinary variadic utility captures that same function and its arguments, returning repeatable work:

```text
use std.function.{SuspendFn, Tuple}

fn deferred[Args < Tuple, O, $R](f: SuspendFn[Args, O, $ R], args...: Args) -> (fn!() -> O $ R):
    fn!() -> O $ R:
        f!(args...)
```

This uses chapter 12's argument tuple and the existing `SuspendFn` constructor.
It has no arity dispatch or per-policy lift implementation.
Unlike calling a bang function without `!`, invoking the returned function constructs and runs a fresh policy call every time.
Capturing an already-built `Suspend` as an argument would still capture a one-shot object; the adapter cannot make that object renewable.

Use existing `with_timeout!` and `retry!` directly, plus these ordinary policy signatures:

- `fallback![T, E, $R, $S](other: fn!() -> Result[T, E] $ S, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + S`.
- `guarded![T, E < From[CircuitOpen], $R](state: &mut Breaker, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R`.
- `hedged![T, E, $R](delay: Duration, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock`.
- `trace![T, E, $R](label: string, work: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Console`.

There is no second `timed`, `retried` or `guarded` combinator to maintain.
For example, the real HTTP port pipes work through `deferred(with_timeout, ms(50), .None, _)`, then deferred retry and fallback.
All type and row arguments infer in that program; no explicit generic lists are needed.
The function value retains all formal parameters, so the timeout's defaulted cleanup argument is supplied as `.None` in this form.

The block form remains `with_timeout!(limit):` followed by the work.
The combinator form is more verbose because it explicitly says to defer the call; that is the price of keeping run-now and build-work distinct.
The adapter is the same for user-written policies, including the stateful breaker, hedge and tracer in the prototype.
No special policy trait, compiler registration, new grammar or bang-call rule is required.

### Static errors and requirements

The full HTTP port uses `Failure`, with distinct HTTP, timeout and open-circuit cases.
Only the policies which introduce those failures require their corresponding `From` conversion.
Retry and fallback keep `E`; fallback deliberately chooses the backup result rather than synthesizing an aggregate error.
The hedge returns the first completion, including an error, rather than waiting for the first success.

Backoff, time limits and delayed hedging add `Clock`.
The breaker and fallback do not; the pure tracing port checks with only `Console` and uses an error type without timeout conversion.
Rows travel through the lifted function's concrete type, not an erased runtime environment.
Configured references preserve shared state across calls; copying the captured argument tuple does not roll back mutations between attempts.

### The programs that actually run

`shape2.hd` includes a standalone timeout and all three requested application ports.
Its fake HTTP provider records requests, then delays primary replies through a captured real clock.
Two per-attempt deadlines expire, retry exhausts its two total attempts, and the third request reaches the fallback provider.
This exercises HTTP timeout cancellation, rather than merely wrapping immediately available fake responses.
Assertions require zero completed primary replies and exactly three recorded requests, distinguishing cancellation from a slow failure followed by fallback.

The stateful breaker allows two failed HTTP calls, then refuses two more without invoking the work.
The hedged request starts a slow replica and a delayed fast replica; the fast replica performs the successful HTTP request.
The result reports two starts, one completion and two closed scopes, including cancellation cleanup of the loser.
Assertions also enforce the breaker and hedge counters, rather than merely printing them.
The fake providers use no external network.

On both engines, the observed results include:

```console
http Result.Ok("200") requests=3
breaker calls=2 failures=2
hedge Result.Ok("replica 2") started=2 finished=1 closed=2
```

These are functional results, not latency measurements or a scheduler fairness claim.
The complete output and exact commands are preserved with source hashes in the delivery report.
The prototype breaker uses a simple failure threshold and explicit mutable state; it does not claim a timed half-open recovery policy.
Such a policy can use the same API while adding `Clock` only to that implementation.

## The Either option

[either-errors.hd](composing-work/either-errors.hd) implements a local `Either[A, B]` enum; it is not a new std item.
Its timeout returns `Result[T, Either[E, TimedOut]]` without requiring `E < From[TimedOut]`.
A second policy adds `CircuitOpen`, producing `Either[Either[E, TimedOut], CircuitOpen]`.
Both engines run the original-error, timeout and open-circuit paths, including a non-time guard with no `Clock` row.

This removes per-domain conversion glue and preserves every error variant statically.
Its cost is positional matching: the demonstrated timeout is `.Err(.Left(.Right(_)))`, and changing wrapper order changes that path.
Each error-adding policy introduces another nesting level; `?` does not automatically flatten it into the caller's preferred domain error.
That is a visible API tradeoff, not a substitute for an erased or open error channel.

The choice is independent of the composition mechanism.
Shape 2's adapter already permits a policy's output error to differ from its input error.
Shape 1's layer output can do the same, but the shown `apply!` signature would need separate input and output error parameters.
The stack representation itself need not change.

Recommend keeping `From[X]` for this proposal, as the current constraint requires, while retaining the runnable Either comparison for the owner.
It gives named domain matches and an unchanged error through a long chain, at the cost of explicit conversion implementations.
Adopting Either instead would change policy signatures and caller matches; it is not a prerequisite for either composition shape.

## Construction and run costs

This is a source-level construction/call model, not measured allocator-call counts.
Optimizations may inline calls or remove closure environments; captures may themselves clone counted values.
Neither mechanism promises allocation-free execution, and this note reports no speed or size improvement.

For `k` layers, shape 1 stores a typed configuration tree, with no required heap allocation for the tree alone.
`apply!` walks it, constructing `k` wrapper-closure values, then invokes the resulting nested work.
Without elimination, that can retain up to `k` closure environments per application; caller work and fallback closures are additional.
There are `k` layer-building calls per application, then wrapper calls along each executed path.

For `k` deferred steps, shape 2 constructs `k` adapter closures when the pipeline is built.
Reusing that pipeline does not reconstruct those adapter closures for every run.
Each visited step adds an adapter invocation and its underlying policy invocation; a direct block has no deferred-adapter layer.
Policy invocations still construct their ordinary suspension state, subject to compiler lowering and elimination.

Retries multiply inner invocations by the actual attempt count; hedging can start two copies and must dispose the loser.
Timeout and race state, input captures and host operations add their existing runtime costs under either shape.
Final allocation numbers need an allocator probe on the resulting Wasm; source closure counts cannot truthfully be reported as measured heap allocations.
The runnable prototypes establish behavior and type-checking, not that unperformed measurement.

## Recommendation

Choose shape 2: ordinary bang policies plus the single `deferred` utility.
It meets the one-definition requirement, runs the requested ports on today's compiler, preserves static errors and rows, and requires no language change.
It also preserves a clear difference between running a block and constructing repeatable work.
Its principal cost is visible adapter syntax and an extra call layer before optimization.

Shape 1 is the next-best surface when reusable, operation-independent policy configurations are more valuable than that simplicity.
It needs the demonstrated row-inference bug fixed before it can claim arbitrary work composition on today's compiler.
A fixed-field resilience struct, provider-only decorators or a monomorphic specialized stack would narrow the requested design, so none replaces these two shapes.
There is no better third mechanism established by this work.

Approval is needed for the std utility's name and contract, eventual policy APIs, and whether Either should replace the current conversion rule.
The recommendation itself does not approve a std change or allocate specification rule IDs.
Ordinary policy APIs would be stdlib-tier; any CLI behavior belongs in the CLI tier.
The shape-1 fix implements existing language rules rather than adding a new mechanism.

## Validation and limits

The compiler is built from main commit `dfe4e2a6f4b7be028c3962c5c5e9707a14407bab` in an isolated worktree, with scratch `HD_CACHE` paths.
`shape2.hd` and `either-errors.hd` check and run on Node and Wasmtime.
`shape1-controls.hd` passes checking but fails during lowering, as recorded above.
`shape1-gap.hd` is a deliberately rejected check reproducer; `shape1.hd` is the full blocked design prototype, not a successful run.
The older prototypes under `composing-work/` belong to the superseded proposal and are not evidence for this one.

Code excerpts parse; the plain diagnostic and output fences above are text evidence, not hd programs.
Actual writing mistakes and exact compiler messages are appended to [the writing log](../audit/hd-writing-log.md).
No std, compiler, spec or conformance-fixture file changes accompany this proposal, and no full local suite was run.
