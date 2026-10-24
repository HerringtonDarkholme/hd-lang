# Positional Union Types: Stress Test

Design review, 2026-10-24. **Parked by the owner on 2026-10-24: hd does not need union types now.** The direction is kept for later: `A | B` is an anonymous enum, as a tuple is an anonymous struct. Order matters, `A | A` is a valid type, and nested unions stay nested. Variants are positional (`._0`, `._1`), and matching is positional. Construction is always explicit: you write `._0(x)` or `.map_err(Union._0)?`, with no injection by type, so no checker rule is added beyond the sugar. A one-payload variant is already a function value, which is what makes `map_err(Union._0)` work, and `?` supplies the missing member type from the enclosing result. Kotlin-style error sets were rejected because they add set semantics, normalization and type-test arms. Two questions remain for a future spec job: how `Error.source` behaves on a union, and how the nameable `Union` is defined for each number of members. This report changes no specification, fixture or compiler code.
It tests the [owner's positional candidate](https://github.com/HerringtonDarkholme/hd-lang/issues/804#issuecomment-6377970627) and [explicit alternative](https://github.com/HerringtonDarkholme/hd-lang/issues/804#issuecomment-6378059165).
The earlier Kotlin-style proposal in the issue body is superseded and remains rejected.

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Problems, Ranked](#problems-ranked)
- [Implementation And Rule Budget](#implementation-and-rule-budget)
- [Reproducing The Policy Port](#reproducing-the-policy-port)
- [Parse Log](#parse-log)
- [Owner Questions](#owner-questions)

## Surface Being Tested

A tuple is an anonymous product; this candidate makes a union an anonymous positional sum.
`A | B` behaves like an enum with payload variants `_0(A)` and `_1(B)`.
Order matters, duplicates survive, and generic arguments may themselves be unions or ordinary generic types.
No error-only restriction, type-set equality, normalization, subtyping or type-test pattern is assumed.

`A | B | C` has three variants.
`(A | B) | C` instead has two outer variants, with another enum inside `_0`.
Matching uses ordinary positional variants and exhaustiveness.
Flattening requires a programmer-written mapping; the report does not propose automatic flattening.

| Variant | Construction at an error boundary | Other contexts |
| --- | --- | --- |
| V1: unique static injection | `.Err(x)` and `?` may select the single member equal to the checked static type of `x` | No general member-to-union coercion is promised |
| V2: always explicit | Write `._k(x)` or `map_err(Union._k)` | Ordinary enum rules |

V1 checks generic bodies while type parameters remain opaque.
For `E | TimedOut`, an explicitly typed `TimedOut` selects `_1`, even when a caller supplies `E = TimedOut`.
A concrete `TimedOut | TimedOut` has two matching members and needs an explicit case.
Neither choice is reconsidered during monomorphization.

The owner proposes conditional `Eq`, `Debug`, `Display` and `Error` implementations.
Their existence does not settle their method bodies, particularly `Error.source`.
The ordinary enum controls explicitly derive or implement traits; they do not demonstrate anonymous built-in implementations.

Relevant existing rules are [unsupported unions](../../spec/lang/04-type-system.md#r-types.unsupported.no-unions),
[error conversion](../../spec/lang/05-expressions.md#error-conversion),
[variant function values](../../spec/lang/08-data-and-enums.md#variant-constructors-as-function-values),
[tuple templates](../../spec/lang/14-annotations.md#tuple-templates),
[error derivation](../../spec/lang/14-annotations.md#error-derivation) and
[error chains](../../spec/std/error.md#cause-chain).
The [policy proposal](POLICY_COMPOSITION.md) supplies the three fixed application programs.

## Method

Twelve cases translate published error-handling interfaces or exercise operations that those translations need.
Foreign examples are small adaptations, not complete library ports.
Rows, cancellation and source chains retain hd's behavior; similarities to another language are not semantic authority.

*Works* means the candidate expresses the operation without changing its intended result.
*Friction* means explicit adaptation, a named wrapper or a visible interface difference is required.
*Breaks* means the requested direct operation is unavailable or would give the wrong result.
Verdicts describe the candidate; current compiler outcomes are reported separately.

The compiler has no anonymous union syntax or V1 injection.
Controls declare ordinary `Union[A, B]` and `Flat[A, B, C]` enums, then parse, check and run their explicit lowerings.
V1-only spellings are parsed as named-enum probes and intentionally fail checking where injection would be needed.
This establishes existing capabilities and gaps, not implementation correctness for a future V1 checker.
A direct anonymous-type probe still rejects:

```text
@derive(Debug)
struct FetchError: pass
@derive(Debug)
struct ParseError: pass
fn run() -> Result[usize, FetchError | ParseError]: # hypothetical syntax
    .Err(._0(FetchError {}))
pub fn main() -> void $ Console:
    println(debug(run()))
```

The current diagnostic is `syntax-error: expected ',' or ']', found '|'`.

A constructor count includes a constructed positional variant or a positional constructor passed as a function value.
Pattern arms, declarations, type mentions and ordinary `Result.Ok`/`Err` do not count.
A `map_err` count includes each source call site, not dynamic invocations or generated functions.
Library code shared by several policy applications is counted once.

## Summary

| Case | Source or operation | V1 | V2 | Main finding |
| --- | --- | --- | --- | --- |
| C1 | thiserror-style HTTP and JSON enum | works | friction | Three explicit maps replace three propagations |
| C2 | anyhow-style context | friction | friction | A sum alone adds no operation/path context |
| C3 | Go wrapped errors | friction | friction | Conditional `Error.source` needs a specified body |
| C4 | Effect selected recovery | friction | friction | Explicit residual error signature and remapping |
| C5 | generic retry | works | works | Preserving `E` needs no union mapping |
| C6 | generic timeout | works | friction | Two construction sites; nested `E` stays nested |
| C7 | HTTP timeout, retry and fallback | friction | friction | Fallback needs an error adapter in both variants |
| C8 | stateful breaker | works | friction | Two construction sites, shared state unaffected |
| C9 | hedged requests | works | works | No extra error; cancellation behavior unchanged |
| C10 | nesting, ordering and `?` chains | friction | friction | No implicit flattening or permutation |
| C11 | duplicate and generic `TimedOut` | friction | friction | Concrete ambiguity differs from opaque checking |
| C12 | generic errors, derives and constructor inference | friction | friction | Generic errors exist; `?` context solves `B` today |

The controls preserve all three policy programs, including cancellation and total-budget assertions.
V2 adds seven constructor uses and five `map_err` calls across the independent HTTP/JSON and full-policy ports.
V1 removes those uses but adds checking obligations and a conversion-priority question.
The evidence favors testing V2 first, while keeping the owner’s positional semantics intact; this is advice, not an accepted design.

## Cases

### C1. A thiserror-style HTTP and JSON error

Rust's [thiserror](https://docs.rs/thiserror/latest/thiserror/) generates `From` and source forwarding from marked fields.
A named error also gives each failure a stable semantic name and message.
This adapted shape uses separate transport and decoder errors:

```rust
#[derive(Debug, thiserror::Error)]
enum LoadFailure {
    #[error("transport: {0}")]
    Transport(#[from] TransportFailure),
    #[error("decode: {0}")]
    Decode(#[from] DecodeFailure),
}
```

The hd baseline is [http-json.hd](../../spec/conformance/runtime/valid/http-json.hd).
Its application enum and two `From` implementations span 13 literal lines, 11 nonblank code lines, excluding the explanatory comment.
The function propagates send, status validation and JSON decoding separately.
A positional translation generates the same two payload choices but removes their semantic names:

```text
type CreateError = Union[HttpError, JsonError]
fn create!(name: string) -> Result[Created, CreateError] $ Http:
    body := encode(NewUser { name: name }).to_utf8()
    response := Request { method: Method.Post, url: "https://example.com/users", body: body }.send!().map_err(Union._0)?
    .Ok(response.ok().map_err(Union._0)?.json::[Created]().map_err(Union._1)?)
```

V1 removes the three maps; each remaining `?` has one statically matching member.
V2 adds three constructor values and three maps.
Both match JSON with `._1(error)` and a transport/status failure with `._0(error)`.
The named enum remains preferable when its custom message is part of the application interface.

The complete control retains the baseline POST headers and derives.
Both engines assert successful decoding, JSON failure, status-before-decoding and three received requests.
Output is `created 7; json _1; status _0; requests 3`.
**Verdict:** V1 works; V2 has explicit mapping friction, with no behavioral loss in this control.

### C2. anyhow-style context is another axis

[anyhow](https://docs.rs/anyhow/latest/anyhow/) supplies application context and downcasting over an erased error chain.
Its union of possible causes need not appear in each function signature.
This adapted call adds the operation being attempted:

```rust
let bytes = read_settings(path)
    .with_context(|| format!("loading settings at {path}"))?;
```

hd already has [Result.context](../../spec/std/error.md#error-context), returning `&dyn Error`:

```text
fn load() -> Result[usize, &dyn Error]:
    let result: Result[usize, DiskError] = .Err(DiskError {})
    result.context("read settings")
```

The runtime control prints `read settings`, then `source: disk full`.
A positional sum would retain exhaustive cases but would not create that extra context message.
Keeping static cases and a path requires an ordinary named wrapper containing the path and a source.
**Verdict:** friction in both variants; unions and contextual wrappers solve different problems.

### C3. Go's wrapped-error lookup

Go's [wrapped-error API](https://go.dev/blog/go1.13-errors) searches the entire chain through `errors.Is` and `errors.As`.
This adapted operation preserves a lower-level cause:

```go
return fmt.Errorf("reading account %s: %w", account, cause)
```

hd's `Error.find[T]` searches the root and successive sources using concrete downcasts.
These existing wrappers expose the issue without needing unions:

```text
@error("disk full")
struct DiskError: pass
@error(transparent)
struct Transparent[E]:
    error: E
@error("$error")
struct Wrapped[E]:
    @source
    error: E
```

For `DiskError` with no source, `Transparent[DiskError]` has chain depth 1 and `find[DiskError]` returns `.None`.
`Wrapped[DiskError]` has depth 2 and finds the member.
Both engines assert those results, consistent with [transparent source forwarding](../../spec/lang/14-annotations.md#r-annot.error.transparent.source).

A union that delegates `source()` transparently therefore cannot promise that `find` locates its active member.
Returning a reference to that member preserves lookup but repeats its displayed message in the default report.
This is an observable choice, not a new matching syntax requirement.
**Verdict:** unresolved for both variants until the conditional `Error` body is specified.

### C4. Effect's selected typed recovery

[Effect's expected-error channel](https://effect.website/docs/v4/error-management/expected-errors) accumulates error unions and removes handled tagged members through `catchTag`.
This adapted recovery handles transport failure but retains decoding failure:

```typescript
const recovered = request.pipe(
  Effect.catchTag("TransportFailure", () => Effect.succeed(cached)),
)
```

The hd translation writes its remaining error type and both result arms:

```text
fn recover(value: Result[usize, Union[string, i32]]) -> Result[usize, i32]:
    match value:
        .Ok(n) => .Ok(n)
        .Err(._0(_)) => .Ok(9)
        .Err(._1(error)) => .Err(error)
```

This generates an ordinary tag test; no runtime type test is needed.
The positional control asserts the recovered value is 9.
Adding a new case still changes each written signature or its shared alias.
**Verdict:** friction in both variants; neither supplies Effect's inferred set-union channel or automatic residual-type computation.

### C5. Generic retry preserves its error

The [current retry implementation](../../lib/std/task.hd) accepts a fallible attempt and returns the same `E`.
Retrying does not inherently introduce another error kind.
A policy may inspect a concrete union through an explicitly typed predicate:

```text
fn transient(error: Union[HttpError, TimedOut]) -> bool:
    match error:
        ._0(inner) => inner.is_transient()
        ._1(_) => true
```

The #783 port retains `Retry` and `SelectiveRetry[E]` unchanged.
Its two timed attempts still cancel before fallback.
A generic retry over arbitrary `E` cannot inspect an unknown member, so it keeps the existing predicate API.
**Verdict:** works for both; zero added constructors or maps inside retry.

### C6. Generic timeout adds one case

The existing `task.timeout!` returns `Result[T, TimedOut]` even when `T` itself is a `Result`.
A wrapper can separate the work's error from the deadline without a `From[TimedOut]` bound:

```text
fn timed![T, E, $R](work: fn!() -> Result[T, E] $ R) -> Result[T, Union[E, TimedOut]] $ R + Clock:
    match task.timeout!(ms(10), work=work):
        .Ok(result) => result.map_err(Union._0)
        .Err(error) => .Err(._1(error))
```

V2 uses one constructor value, one constructor call and one map.
V1 can instead use `.Ok(result?)` for the work result and `.Err(error)` for a deadline.
The parsed V1 model expands the work result into explicit arms to expose both injection sites.
That uses the two specified injection contexts; returning the inner `Result[T, E]` directly is not a promised coercion.
The full policy port implements this behavior in `TimeLimit.wrap`.

If `E` is already `HttpError | JsonError`, timeout returns `(HttpError | JsonError) | TimedOut`.
The outer error remains nested even when a flat spelling would be shorter.
**Verdict:** V1 works with explicit branch handling; V2 adds mapping friction.

### C7. Timeout, retry and fallback around HTTP

The source is [the complete lean #783 prototype](composing-work/policy-values.hd).
The port changes the timeout layer's associated `Wrapped` result to `Union[E, TimedOut]`.
Its concrete transient predicate handles `Union[HttpError, TimedOut]`, while the HTTP work still returns `HttpError`.
Policy values, tuple composition and the one `apply!` block remain unchanged.

```text
backup := fn!() -> Result[string, Union[HttpError, TimedOut]] $ Http:
    fetch!("http://backup/item").map_err(Union._0)
policy := (timeout(ms(50)), retry(Backoff { attempts: 2, initial: ms(1) }).when(transient))
    |> fallback(backup, _)
response := apply!(policy):
    fetch!("http://primary/item")
```

Fallback requires the same error type from work and backup.
A direct backup result is not an `.Err` or `?` injection site, so the shown map remains in V1 too.
Alternatively both versions can spell `.Ok(fetch!(url)?)`, moving the adaptation into a propagation context.
The count comparison fixes the direct-return spelling in both variants.

Both engines produce `http Result.Ok("200") requests=3`.
Assertions prove two primary replies were cancelled, none finished, and one backup request ran.
The outer-total-budget port also returns 201 after exactly two requests, without allowing another primary attempt.
**Verdict:** friction in both; error adapters remain a compositional obligation.

### C8. Stateful breaker

The same #783 prototype keeps a shared failure counter across applications.
Its new error is distinct from the work's `E`:

```text
fn guarded![T, E, $R](state: &mut Breaker, work: fn!() -> Result[T, E] $ R) -> Result[T, Union[E, CircuitOpen]] $ R:
    if state.failures >= state.limit:
        return .Err(._1(CircuitOpen {}))
    state.calls += 1
    result := work!()
    state.failures = if result.is_ok(): 0 else: state.failures + 1
    result.map_err(Union._0)
```

V1 replaces these two construction sites with ordinary result arms and injection.
The port removes the application `From[CircuitOpen]` implementation.
Four applications produce two HTTP errors, then two open-circuit errors; calls and failures both remain 2.
**Verdict:** V1 works; V2 adds two constructors and one map, with no state-lifetime difference.

### C9. Hedged requests

Hedging the same work introduces another execution but no new failure type.
The first completed branch still wins, even when its result is an error.
The #783 delay/race implementation and this application are unchanged:

```text
result := apply!(hedged(ms(10))):
    replica!(&mut state)
```

Both engines return `replica 2`, with `started=2 finished=1 closed=2`.
Assertions prove loser cleanup and shared state are preserved.
This is behavioral evidence for the ordinary enum port, not a latency benchmark or an implementation of anonymous union cancellation.
**Verdict:** works in both; zero added constructors or maps in hedging.

### C10. Nesting and order survive propagation

This ordinary-enum control spells out the only supported flattening operation:

```text
fn flatten[A, B, C](value: Union[Union[A, B], C]) -> Flat[A, B, C]:
    match value:
        ._0(inner) => match inner:
            ._0(a) => ._0(a)
            ._1(b) => ._1(b)
        ._1(c) => ._2(c)
```

Both engines map the nested second case to flat `_1(7)`.
Neither V1 nor V2 makes `Union[A, B]` assignable to `Union[B, A]`.
V1 can inject a complete inner union into an outer member of that exact type; it cannot distribute its alternatives.
V2 writes one additional `map_err(Union._0)` at that outer boundary.

Three intentional checker probes reject raw reordering, nested-to-flat conversion and member assignment to a union binding.
Their diagnostics are `type-mismatch`, as expected for ordinary enums.
A failed `?` into a union requires explicit mapping or the proposed V1 rule; existing `From` does not recursively flatten.
**Verdict:** friction in both; public aliases can contain churn, but cannot erase a changed positional contract.

### C11. Duplicate members and opaque generics

Two generic helpers can intentionally choose different cases with the same eventual payload type:

```text
fn expire[E](error: TimedOut) -> Union[E, TimedOut]: ._1(error)
fn carry[E](error: E) -> Union[E, TimedOut]: ._0(error)
```

With `E = TimedOut`, both engines preserve distinct tags and equality says the resulting values differ.
The V1 equivalents infer `_1` for the concrete `TimedOut` and `_0` for opaque `E` at checking time.
A non-generic `.Err(timed_out)` targeting `TimedOut | TimedOut` instead needs an explicit choice.
This difference is explicitly accepted by the owner; it is not grounds to reopen collapsing sets.

The ambiguity diagnostic must offer both valid choices, rather than silently preferring `_0`.
An unmatched static type must name the target members; blindly inserting `._0` may itself be ill-typed.
V2 exposes the choice at every source site.
**Verdict:** friction in both; V1 saves spelling while requiring stable checked elaboration and precise repairs.

### C12. Existing generic errors, derives and inference

A tracked `.hd`/Rust-source scan finds 133 `@error`-decorated declarations and 13 generic declarations.
Ten generic declarations are in positive cases; three are deliberate invalid marker cases.
There are no generic `@error` declarations in `lib/std` in this scan.
This is a source inventory, not a claim that every generic shape is legal.

Positive examples include `TaskError[E, T]`, transparent `Wrapped[P]`, interpolated `Named[T]`, `SaveError[E]` and `Generic[E]`.
See [runtime generic error bounds](../../spec/conformance/runtime/valid/a-generic-error-type-gets-derived-bounds.hd),
[typing derivation](../../spec/conformance/typing/valid/error-derivation.hd) and
[checker valid forms](../../crates/hd_check/tests/cases/checker/valid-forms-are-clean.hd).
The existing runtime example checks and runs on both engines, including source depth 2 and generic message interpolation.

A restriction to non-generic error declarations would therefore remove existing supported code.
The positional candidate needs no such restriction.
The duplicate/nested control also derives `Eq` and `Debug` through ordinary named-enum machinery.
This does not prove that an anonymous type automatically gets `Structure` or every user derivation.

The mandatory inference probe is a complete current program:

```text
@derive(Debug)
struct FetchError: pass
@derive(Debug)
struct ParseError: pass
@derive(Debug)
enum Union[A, B]:
    _0(A)
    _1(B)
fn fetch() -> Result[usize, FetchError]: .Err(FetchError {})
fn run() -> Result[usize, Union[FetchError, ParseError]]:
    value := fetch().map_err(Union._0)?
    .Ok(value)
pub fn main() -> void $ Console:
    println(debug(run()))
```

It checks with zero errors/warnings and prints `Result.Err(Union._0(FetchError {}))` on both engines.
The enclosing `?` supplies the target error type, solving the otherwise missing `B`.
A direct function-tail `fetch().map_err(Union._0)` also works with the written result signature.
This answers the owner's inference question positively for today's ordinary enum constructor.

Removing that context gives the intentional negative probe:

```text
fn unconstrained() -> void:
    wrapped := fetch().map_err(Union._0)
```

Its exact diagnostic is `cannot-infer-type: a type argument here has no solution; write it explicitly or give an expected type`.
A result annotation repairs it:

```text
let wrapped: Result[usize, Union[FetchError, ParseError]] = fetch().map_err(Union._0)
value := wrapped?
```

The complete annotated control checks and runs on both engines.
`Union::[FetchError, ParseError]._0` is not a working repair today: checking reports `unsupported: stage Body: a type with arguments used as a value is not supported`.
`map_err(._0)` separately reports `missing-contextual-enum-type: `._0` needs an expected enum type`.
The tested successful spelling is `map_err(Union._0)` with result context.

There remains a distinct anonymous-constructor question.
A declared `Union[A, B]` has a name, whereas anonymous sums of every arity need a family of nameable constructors.
The two-member probe cannot prove resolution for an anonymous flat three-member union.
**Verdict:** friction in both variants; expected-type inference works, but constructor-family and built-in trait rules still need specification.

## Comparison With Other Languages

| Mechanism | Cases and propagation | Context and inspection | Consequence for hd |
| --- | --- | --- | --- |
| thiserror named enum | Explicit nominal cases; marked fields generate `From` | Names, display text and source behavior remain explicit | Keep named enums for semantic application boundaries |
| anyhow | One erased error, generic `?` propagation | Context chain and dynamic downcasting | A positional sum does not replace context |
| Go wrapping | One interface; explicit wrapping | `Is`/`As` traverse the cause chain | Specify union source behavior before promising lookup |
| Effect typed errors | Structural union error channel; selected recovery | Tags guide recovery; handled cases leave the channel | hd retains written signatures and positional remapping |
| Named `Either[A, B]` in ordinary hd | Two positional variants, nested beyond two | Ordinary enum derives and hand-written delegation | Useful control; no anonymous syntax and no generic overlapping `From` pair |
| V1 positional union | Anonymous variants plus exact static injection at two contexts | Conditional trait bodies still need definition | Saves five maps in the two measured ports |
| V2 positional union | Anonymous variants, explicit construction | Same unresolved trait bodies | Smallest candidate requiring no injection rule |

These contrasts use [thiserror](https://docs.rs/thiserror/latest/thiserror/), [anyhow](https://docs.rs/anyhow/latest/anyhow/),
[Go's errors design](https://go.dev/blog/go1.13-errors) and [Effect](https://effect.website/docs/v4/error-management/expected-errors).
The comparison does not revive Kotlin's rejected set semantics or a variadic library union.
A generated named-wrapper annotation would retain names and existing derives, but cannot express generic `E | TimedOut` by itself.
That alternative remains a convenience feature requiring separate owner approval.

### Measured spelling changes

| Complete source, with fixed direct-return fallbacks | V1 constructors | V2 constructors | V1 maps | V2 maps | Added constructors / maps |
| --- | --- | --- | --- | --- | --- |
| HTTP/JSON `create!` body | 0 | 3 | 0 | 3 | 3 / 3 |
| Full #783 policy port, all three applications plus reuse/total-budget checks | 2 | 6 | 2 | 4 | 4 / 2 |
| Independent-program total | 2 | 9 | 2 | 7 | **7 / 5** |

The policy source shares one timeout and one breaker implementation across all applications.
Their explicit adaptations contribute four constructor uses and two maps.
Two fallback callbacks contribute two more constructor values and maps in both variants.
The transient predicate's patterns contribute no construction count.

The baseline application `Failure` enum and its three `From` impls occupy 14 literal lines, 11 nonblank code lines.
The port uses a one-line error alias and a two-case transient predicate; its ordinary `Union` declaration models future language sugar.
Named-wrapper messages and deliberate folding of deadline errors into application cases are not retained automatically.
These are source counts, not generated-code or performance measurements.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | U1 | Conditional `Error` does not define member visibility in the source chain | Silent behavioral difference | C2, C3, C12 |
| 2 | U2 | V1 must preserve ordinary propagation and freeze unique selection | Checker correctness requirement | C1, C5–C8, C10, C11 |
| 3 | U3 | Nameable anonymous constructor family remains unspecified | Blocks V2 spelling beyond the named control | C1, C6, C10, C12 |
| 4 | U4 | Nested error signatures and same-error fallback adapters remain | Explicit interface friction | C4, C6–C8, C10 |
| 5 | U5 | Automatic traits need bodies, ownership and derivation boundaries | Implementation scope | C3, C11, C12 |

### U1. Source-chain visibility

**Effect.** C3 proves that transparent forwarding can make the active error undiscoverable by its concrete type.
C2 still needs contextual wrappers, and C12 needs this behavior for generic members too.

**Candidates.** Should unions follow existing transparent errors, accepting the member lookup limitation?
Should they expose the active member as a source and accept duplicate default messages?
Should a richer inspection protocol wait until after the smaller union design?

### U2. Ordinary propagation and stable elaboration

**Effect.** Retry, fallback and callers routinely propagate an error whose type is already the destination union.
A rule that searches only payload members would reject that ordinary same-type propagation.
Generic selection also must not change when monomorphization makes two members equal.
Precedence against an existing `From[E]` implementation needs a defined place in the one-step conversion order.

**Candidates.** Should ordinary successful assignability remain first, with unique injection tried only after it fails?
Should V1 remain deferred until V2 establishes that five saved maps justify this rule?
The report assumes no general injection at assignments or direct `Result` returns.

### U3. Anonymous constructors

**Effect.** C12 proves expected context solves ordinary generic constructors, but not that the spelling `Union` resolves without a declaration.
Flat sums of different arities cannot all be one ordinary fixed-arity enum.

**Candidates.** Should `Union._k` be a compiler-recognized constructor family resolved from the expected sum type?
Should the first implementation require an explicit anonymous type spelling at constructor-value sites instead?
Any repair must retain the existing rule that contextual `.Variant` alone is not a function value.

### U4. Signature churn and adapters

**Effect.** C6 and C10 retain nested sums; C4 explicitly writes residual types; C7 adapts fallback errors.
V1 does not remove those obligations because its two injection contexts are narrower than general conversion.

**Candidates.** Should documentation recommend aliases and explicit boundary maps for these cases?
Should application-facing APIs retain named errors while generic policy internals use positional sums?
Automatic normalization remains rejected.

### U5. Traits and ownership

**Effect.** Eq must retain the tag even when payload types are equal, as C11's control asserts.
Display and Debug need separate forwarding/tag policies, while Error adds source-chain requirements.
Tuples currently use [tuple templates](../../spec/lang/09-traits.md#derived-tuple-implementations), not a universal built-in implementation for every trait.

**Candidates.** Should Debug expose the positional tag and Display delegate only the active payload?
Should anonymous sums initially expose only the four proposed traits, leaving custom derivation and persistence to named wrappers?
Should their ownership follow std-owned tuple constructors so aliases cannot create foreign-trait implementation rights?

## Implementation And Rule Budget

This is a bottom-up engineering allowance, not measured implementation size or timing.
There is no implemented union compiler to benchmark.
All additions below are language-tier mechanisms; an ordinary `Either` is a stdlib-tier alternative.
No specification rule or diagnostic ID changes in this job.

| Component | Existing mechanism to reuse | V2 allowance | Additional V1 allowance |
| --- | --- | --- | --- |
| Type grammar and parsing | Existing type parser, ordered members, parenthesis boundary | 60–110 Rust lines | none |
| Anonymous sum identity and wire form | One arity constructor, ordered type arguments; ordinary enum payload model | 120–220 lines | none |
| Constructor-family lookup and positional patterns | Existing variant function values and exhaustiveness | 80–160 lines | none |
| Lowering, layout and monomorphized instances | Existing enum tag/payload/drop paths | 50–110 routing lines; no new representation algorithm | 30–60 lines to retain checked injection tags |
| Conditional trait generation | Existing Eq/Debug/Display/Structure and Error methods | 150–260 lines, after method policies are decided | none |
| Error-boundary checking | Existing ordinary assignability and one-step conversion | none | 100–180 lines for two contexts, exact lookup and diagnostics |
| Tests and diagnostics | Current stage/conformance harnesses | 20–30 focused cases | 10–16 further generic/ambiguity/priority cases |

The estimate totals 460–860 production Rust lines for V2 and another 130–240 for V1.
It excludes user derivation templates, serialization policy, documentation and test source lines.
Anonymous identity and wire integration need review before these allowances become an implementation plan.
No performance win is claimed.

A unique selection can scan the `k` member types in O(k), with no combinatorial unification search.
Nested sums remain separate nodes and existing layout machinery handles each enum node.
The sugar implies no new runtime type test or heap box.
Payload storage and drop behavior remain those of its ordinary enum model.
A new ordered type-argument vector may instantiate another wrapper, just as a named generic enum does.
V1's selected constructor should lower identically to the corresponding explicit constructor; that is an invariant to test, not a measured fact.

Candidate rule accounting for a later approved spec pass:

| Proposed area | Atomic rules needed | Existing rules affected |
| --- | --- | --- |
| Anonymous type former | Ordered members; duplicate retention; flat chain; parenthesis nesting; unrestricted member types | `types.unsupported.no-unions` would need replacement with new IDs |
| Ordinary enum reuse | One payload per position; construction; positional matching; exhaustiveness; layout reuse | Existing enum rules referenced, not silently redefined |
| Trait surface | Conditional obligation and method behavior per proposed trait; ownership and Structure scope | New IDs after U1/U5 answers |
| V1 only | Eligible `.Err` boundary; eligible `?` boundary; exact equality; unique match; no match error; ambiguity error; opaque generic check; frozen tag; conversion priority | `expr.try.convert` ordering needs a new ID for its changed meaning |

V1 therefore costs **nine atomic checking rules**, even though it is one conceptual convenience.
V2 adds none of these nine, but still needs anonymous grammar, identity, constructor and trait rules.
The concrete syntax question for `Union` is not dismissed as free library code.
Retired IDs would be listed by the implementation commit; this report retires none.

## Reproducing The Policy Port

The following patch transforms the linked #783 source into the complete ordinary-enum V2 control.
Apply it to a scratch copy, not to production std.
The unchanged provider and main assertions are part of the executed program.
The control checks and runs on both engines using dev-profile tools.

```diff
--- policy-values.hd
+++ policy-explicit.hd
@@ -4 +3,0 @@
-use std.convert.From
@@ -9,0 +9,5 @@
+@derive(Debug, Eq)
+enum Union[A, B]:
+    _0(A)
+    _1(B)
+
@@ -37,5 +41,7 @@
-impl[T, E < From[TimedOut], $R] Layer[fn!() -> Result[T, E] $ R] for TimeLimit:
-    type Wrapped = fn!() -> Result[T, E] $ R + Clock
-    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
-        fn!() -> Result[T, E] $ R + Clock:
-            task.with_timeout!(self.limit, cleanup=self.cleanup, work=work)
+impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for TimeLimit:
+    type Wrapped = fn!() -> Result[T, Union[E, TimedOut]] $ R + Clock
+    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, Union[E, TimedOut]] $ R + Clock):
+        fn!() -> Result[T, Union[E, TimedOut]] $ R + Clock:
+            match task.timeout!(self.limit, cleanup=self.cleanup, work=work):
+                .Ok(result) => result.map_err(Union._0)
+                .Err(error) => .Err(._1(error))
@@ -82,0 +89 @@
+@derive(Debug, Eq)
@@ -95,4 +102,4 @@
-impl[T, E < From[CircuitOpen], $R] Layer[fn!() -> Result[T, E] $ R] for Guard:
-    type Wrapped = fn!() -> Result[T, E] $ R
-    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R):
-        fn!() -> Result[T, E] $ R:
+impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for Guard:
+    type Wrapped = fn!() -> Result[T, Union[E, CircuitOpen]] $ R
+    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, Union[E, CircuitOpen]] $ R):
+        fn!() -> Result[T, Union[E, CircuitOpen]] $ R:
@@ -100 +107 @@
-                return .Err(E::from(CircuitOpen {}))
+                return .Err(._1(CircuitOpen {}))
@@ -104 +111 @@
-            result
+            result.map_err(Union._0)
@@ -148,14 +155 @@
-@derive(Debug)
-enum Failure:
-    Http(HttpError)
-    Timeout
-    Open
-
-impl From[HttpError] for Failure:
-    fn from(error: HttpError) -> Failure: .Http(error)
-
-impl From[TimedOut] for Failure:
-    fn from(error: TimedOut) -> Failure: .Timeout
-
-impl From[CircuitOpen] for Failure:
-    fn from(error: CircuitOpen) -> Failure: .Open
+type Failure = Union[HttpError, TimedOut]
@@ -165,3 +159,2 @@
-        .Http(inner) => inner.is_transient()
-        .Timeout => true
-        .Open => false
+        ._0(inner) => inner.is_transient()
+        ._1(_) => true
@@ -185 +178 @@
-fn fetch!(url: string) -> Result[string, Failure] $ Http:
+fn fetch!(url: string) -> Result[string, HttpError] $ Http:
@@ -194 +187 @@
-fn replica!(state: &mut Replicas) -> Result[string, Failure] $ Http + Clock:
+fn replica!(state: &mut Replicas) -> Result[string, HttpError] $ Http + Clock:
@@ -221 +214 @@
-        backup := fn!() -> Result[string, Failure] $ Http: fetch!("http://backup/item")
+        backup := fn!() -> Result[string, Failure] $ Http: fetch!("http://backup/item").map_err(Union._0)
@@ -252 +245 @@
-            let value: Result[usize, Failure] = .Ok(11)
+            let value: Result[usize, HttpError] = .Ok(11)
@@ -258 +251 @@
-            let value: Result[string, Failure] = .Ok("reused")
+            let value: Result[string, HttpError] = .Ok("reused")
@@ -266 +259 @@
-        backup_again := fn!() -> Result[string, Failure] $ Http: fetch!("http://second-backup/item")
+        backup_again := fn!() -> Result[string, Failure] $ Http: fetch!("http://second-backup/item").map_err(Union._0)
```

## Parse Log

| Block | Subject | Parser outcome |
| --- | --- | --- |
| 1 | current anonymous syntax gap | expected hypothetical-syntax rejection at `|` |
| 2 | C1 HTTP/JSON body | clean |
| 3 | C2 context helper | clean |
| 4 | C3 transparent/source wrappers | clean |
| 5 | C4 selected recovery | clean |
| 6 | C5 transient predicate | clean |
| 7 | C6 timeout wrapper | clean |
| 8 | C7 HTTP policy application | clean |
| 9 | C8 breaker wrapper | clean |
| 10 | C9 hedge application | clean |
| 11 | C10 explicit flatten | clean |
| 12 | C11 generic positional helpers | clean |
| 13 | C12 complete inference control | clean |
| 14 | C12 unconstrained fragment | clean |
| 15 | C12 result annotation fragment | clean |

All 15 embedded hd blocks were parsed: 14 clean and one deliberate unsupported anonymous-type probe.
Fragments are syntax checks; their undeclared names are supplied by the linked baseline or complete ordinary-enum controls.
Eight complete controls also format, parse and check with zero errors/warnings, then run successfully on Node and Wasmtime.
The **16 successful engine executions** have pairwise identical output and include intermediate assertions for policy state and cleanup.

Ten additional negative controls retain their intended diagnostics:

| Probe | Expected current checker result |
| --- | --- |
| `contextual-constructor-gap` | `missing-contextual-enum-type` |
| `explicit-args` | `unsupported` |
| `fn-member-gap` | `type-mismatch` |
| `inline-union-gap` | `syntax-error` |
| `nested-flat-gap` | `type-mismatch` |
| `policy-injection-model` | `type-mismatch` |
| `raw-err-gap` | `type-mismatch` |
| `raw-try-gap` | `invalid-result-propagation` |
| `reorder-gap` | `type-mismatch` |
| `unconstrained` | `cannot-infer-type` |

The V1 model has four intended payload type mismatches because today’s named enum has no injection rule.
No negative result is treated as accepted anonymous behavior or a newly discovered production regression.
The repository inventory and 59 local links were checked; no complete test suite ran.

Tools are dev-profile frozen binaries whose compiler/std/host sources match this job’s base `5d8c1d92d`.
The base differs only in documentation from their compiler source revision `424c34c44`.
Exact commands, source hashes, formatted controls, negative outputs, inventory and gate logs are retained at
`/private/tmp/muse-804-union-1792821231924001000`.

## Owner Questions

1. **Source lookup changes silently.** Should union `Error.source` transparently skip the active member, or expose it as a source despite duplicate report text?
   C3's `Transparent[DiskError]` versus `Wrapped[DiskError]` demonstrates the observable choice.
2. **V1 saves five maps but adds nine checking rules.** Should the first design use V2?
   Alternatively, should V1 retain ordinary propagation first and freeze generic selection?
   C11's `expire[TimedOut]` must keep `_1` while a concrete duplicate needs an explicit case.
3. **The nameable family still needs a definition.** Should `Union._0` resolve from the expected anonymous sum, including its arity, as a compiler-recognized family?
   C12 proves the expected result can infer the missing member type once a constructor is resolved.
4. **Positional contracts remain visible.** Should named public errors and internal positional policy sums be the recommended boundary pattern?
   C7 needs a fallback adapter, and C10 needs an explicit nested-to-flat map.
5. **Conditional implementations need concrete behavior.** Should the initial surface use tag-sensitive Eq/Debug, active-member Display, and only the four proposed traits?
   Structure/custom derives and foreign-trait ownership would then need an explicit boundary before persistence APIs are promised.
