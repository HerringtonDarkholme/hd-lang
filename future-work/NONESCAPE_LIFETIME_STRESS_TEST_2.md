# NonEscapable Types And Lifetimes: Adversarial Review 2

Status: design review, 2026-10-15; nothing here is accepted behavior.
This report changes no decision, design record, spec text, fixture, or compiler.
It reviews rounds 4 and 5 of [the design](NONESCAPE_LIFETIME_DESIGN.md),
including the owner clarification in commit `dfb53fe8`.

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Earlier Findings Rechecked](#earlier-findings-rechecked)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Problems, Ranked](#problems-ranked)
- [Evaluation By Dimension](#evaluation-by-dimension)
- [Parse Log](#parse-log)

## Surface Being Tested

This is a second adversarial review, following
[the twenty-case first review](NONESCAPE_LIFETIME_STRESS_TEST.md).
Migration is excluded, as the owner requested.
The owner decides the answers; this report supplies evidence and questions.

The exact decision set is D1–D8, the active Q1–Q16 answers, NE-S1,
NE-S2, NE-S5, NE-S6, NE-S7, NE-S10, NE-S11, OQ1, and OQ3.
Closing and cleanup checking are parked, including NE-S3, NE-S4, NE-S8,
and NE-S9. They are not failed requirements of this revision.

OQ1 is a substantive reduction of the guarantee, not a fix for resource
closing. Fresh handles reach `static`, and may escape through globals,
closures, or erasure, as the owner explicitly confirmed in `dfb53fe8`.
The creating body can keep an alias; no ownership transfer occurs.

The active guarantee is modular retention control. A borrowed parameter
or provider root can reach only the results and stores its contract names.
Fresh resources have no language-defined end; use after close remains a
runtime error. Scoped callback libraries can impose a stronger boundary.

Dependencies include both specification tiers:

| Area | Specification |
| --- | --- |
| Readonly views, nested mutation, polarity, newtypes | [Type System](../spec/lang/04-type-system.md#access-permission), especially `types.readonly.nested`, `types.polarity`, and `types.newtype.construct-ref` |
| Function constructors, captures, bound and unbound method values | [Functions](../spec/lang/07-functions.md#function-types-and-values), [Method References](../spec/lang/07-functions.md#method-references), and [Captures](../spec/lang/07-functions.md#captures) |
| Fields, recursive declarations, optional payloads | [Data and Enums](../spec/lang/08-data-and-enums.md) |
| Erasure, associated types, runtime recovery | [Traits](../spec/lang/09-traits.md#runtime-type-identity) |
| Provider values, contexts, cold construction, callable weakening | [Requirements and Suspension](../spec/lang/11-requirements-and-suspension.md#provider-values), especially `req.suspend.type.equivalent` and `req.bind.captured` |
| Library operations approximated below | [Collections](../spec/std/collections.md), [Task](../spec/std/task.md), [Net](../spec/std/net.md) |

The shared parser prelude models synchronous test handles. `open` is a
parser stub for an independent handle constructor; no example is executed.
Library shapes below are small approximations, not source translations
with a claim of equivalent implementations.

```text
trait NonEscapable
trait Socket < NonEscapable:
    fn sent(^self) -> usize # hypothetical syntax
type Conn = dyn Socket
fn open() -> Conn: panic("parser stub: supplies an independent handle")
```

## Method

Thirteen cases attack combinations of the revised rules. The emphasis is
ordinary providers, invariant storage behind joined slots, captured
setters, and the two representations of suspending callables.
Three controls separate intentional behavior from a hole.

| Verdict | Meaning |
| --- | --- |
| Works | The record supports a consistent derivation for this case. |
| Friction | Safe code faces a restriction or precision loss; no guarantee violation is alleged. |
| Breaks | Rules conflict, or an existing abstraction boundary lacks the rule needed to preserve its contract. |

A break is **not** a demonstrated compiler exploit. The proposed checker
does not exist. Each break identifies either conflicting rules, a missing
translation, or a conditional attack that a conservative checker must reject.

Every hd example is parsed unchanged and through a syntax-only projection.
The projection removes the proposed lifetime syntax and does not type-check
the result. Its exact transformations and diagnostics appear in the Parse Log.
The grammar sketch in the original design is not treated as an hd program.

Candidates are owner questions, with their costs classified by AGENTS.md:
syntax, semantic rules, compiler intrinsics, or ordinary core library code.
An ordinary library addition is preferred when it can preserve the guarantee.
No candidate is selected by this report.

## Summary

The proposal is substantially stronger than the previous version. Several
old contradictions have real repairs, and the optimizer section now makes
bounded claims. Parking closing removes an ill-supported ownership promise.
However, three abstraction boundaries still need resolution before a soundness claim.

| Case | Library shape | Verdict | Finding |
| --- | --- | --- | --- |
| R21 | Kotlin contextual service / Go context-carried callback | Breaks: rule conflict | Ordinary provider keys can carry nonstatic reaches but are not tracked inputs |
| R22 | Rust optional pair with interior mutable state | Breaks: conditional wrapper attack | Joining slots can widen an invariant mutable field |
| R23 | Rust tuple returning a mutable cell and an independent value | Breaks: boundary ambiguity | A single tuple reach cannot silently become each mutable component's reach |
| R24 | Go bound appender method / Rust captured setter | Breaks: missing translation | A bound store must become an environment store effect, including variance |
| R25 | Rust async closure environment | Breaks: conflicting frame formulas | The cold-call rule omits the called closure's environment |
| R26 | Rust future factory / hd callable weakening | Breaks: missing translation | Lowered constructor results need distinct `run` and `out` summaries |
| R27 | Rust scoped callback / Kotlin local resource API | Works | An explicit callback result bound can forbid the scoped argument's escape |
| R28 | Go closure assigning captured storage | Breaks: missing closure-boundary rule | A shared capture cell cannot remain a fresh flexible local inside the callback |
| R29 | Rust callable returning independent mutable storage | Friction | A mutable result makes the environment invariant even when independent |
| R30 | Generic `map` to sizes | Friction, accepted L9 | A generic result retains a dependency even after producing scalars |
| R31 | Go pooled buffer / identity helper | Works | Ordinary provenance now survives helper composition |
| R32 | Go callback registry with a newly opened handle | Works, intentional | Fresh resources may escape; a borrowed parameter's resource may not |
| R33 | Rust closure identity / forwarding factory | Breaks: optimizer wording | A bare `^fn` may still be retained through a named or elided result |

The highest-confidence new conflict is R21: ordinary types explicitly may
carry roots, yet provider tracking still uses the NonEscapable category.
R22–R23 test whether the new invariance restriction survives aggregation.
R24–R26 and R28 need exported translation and checking rules, not body inspection.

## Earlier Findings Rechecked

These statuses concern the earlier minimized attacks. A regression example
can fix that attack without proving every composition of the replacement rule.

| Earlier ID | Status in this revision | What remains |
| --- | --- | --- |
| NE-S1, C01–C03 | Direct getter, callable-result, and nested-list attacks are rejected by the new surface rule | Joining invariant components remains unaccounted for: R22–R23; setter effects need R24 |
| NE-S2, C04/C14 | Ordinary provenance is preserved, and optimizer escape edges are explicitly separate | The same ordinary provenance must survive providers: R21 |
| NE-S3 | Parked with closing | No active claim that cleanup targets are checked |
| NE-S4 | Closing method values are parked | Ordinary bound methods still need their store contracts translated: R24 |
| NE-S5, C08 | `run`/`out` split fixes the direct retained-frame/output example | Closure construction and constructor weakening: R25–R26 |
| NE-S6, C09 | NonEscapable provider keys and row parameters now count in callback elision | Ordinary provider keys are still absent: R21 |
| NE-S7, C13/C14 | Main overclaims are repaired; caller alias and liveness proofs are listed | No benchmark establishes payoff; callback contract checking must include captures: R28 |
| NE-S8 | Parked with closing | No active closing-authority claim |
| NE-S9 | Parked with closing; guarantee wording explicitly excludes disposal safety | Correctly reduced scope, not a successful proof of the old guarantee |
| NE-S10, C18 | Stored-field category calculation fixes the phantom example | Compute category and lifetime-carrying status separately; newtype coverage remains unstated |
| NE-S11 | Precision losses are explicit; scalar closure factories now work | Invariant aggregation is a soundness obligation, not merely accepted precision loss |

The old complexity objection is also addressed honestly: the stated bound
is now a common-case estimate, and repeated propagation is acknowledged.
Ordinary locality marks are no longer described as having no checking effect.

## Cases

### R21. An Ordinary Provider Can Carry A Borrowed Root

Original shape, approximating a service supplied from a contextual scope:

```go
type Reader struct { read func() int }
// A context can supply a Reader whose function closes over a resource.
func currentReader(ctx Context) Reader { return ctx.Reader() }
```

```text
trait Reader:
    fn read(^self) -> usize # hypothetical syntax

data CallbackReader:
    callback: fn() -> usize

impl Reader for CallbackReader:
    fn read(^self) -> usize: self.callback() # hypothetical syntax

fn current_reader() -> dyn Reader $ Reader:
    $.use(Reader)

let READERS: mut List[dyn Reader] = []

fn leak_through_provider(c: ^Conn) -> void: # hypothetical syntax
    p := CallbackReader { callback: fn() -> usize: c.sent() }
    r := $.with(Reader=p):
        current_reader()
    READERS.push(r)
```

`CallbackReader` is ordinary by `ne.data.category`: a function field does
not count. Its reach nevertheless includes `c`, as section 5.4 explicitly
requires. The non-root trait `Reader` can therefore receive a nonstatic provider.

`ne.provider.root` and `ne.sig.input` create tracked key roots only for
NonEscapable keys. `current_reader` consequently has no tracked input,
and `ne.elide.none` assigns its result `^{}`. `ne.call.fresh` then gives
the ordinary `dyn Reader` result an empty reach, allowing the final store.

But `ne.provider.use` says `$.use(Reader)` reaches `$Reader`. If that
root exists, the body cannot satisfy its elided `^{}` contract. If it
does not exist, returning the provider loses `c`. These are conflicting rules.

Lowering is just a provider argument returned as a trait value. No copy,
GC operation, or runtime closure allocation eliminates the retained resource.
No closing behavior is needed to violate the stated retention guarantee.

**Verdict: breaks, explicit rule conflict and conditional escape.** A
checker can reject the declaration or the local provider binding, but
neither is the presently stated rule for ordinary providers.

The same test applies to an ordinary provider that contains a frame,
an erased handle, or a callback holder inside `$.Context`. Tracking all
unknown row parameters does not fix an explicitly named ordinary key.

### R22. Optional Wrapping Joins An Invariant Slot

Original shape, approximating a Rust optional pair:

```rust
struct Pair<'a, 'b> {
    first: &'a RefCell<Vec<&'a Socket>>,
    second: &'b Socket,
}
// Option<Pair<'a, 'b>> preserves both lifetime parameters.
```

```text
data WritablePair[^a, ^b]: # hypothetical syntax
    first: ^{a} mut List[Conn] # hypothetical syntax
    second: ^{b} Conn # hypothetical syntax

impl NonEscapable for WritablePair

fn through_option(dst: ^mut List[Conn], c: ^Conn) -> void: # hypothetical syntax
    let wrapped: WritablePair? = .Some(WritablePair { first: dst, second: c })
    match wrapped:
        .Some(pair) => pair.first.push(pair.second)
        .None => pass
```

Before wrapping, slot `a` is `dst`, slot `b` is `c`, and slot `a` is
invariant because its field is mutable storage. `ne.data.wrapper` says
the optional joins both slots, and extraction gives every slot that union.
The projected mutable list would then have reach `{dst, c}`.

The push fits that widened reach, yet the helper's signature names no
store of `c` into `dst`. At runtime it modifies the caller's original
list, which retains `c` after the call. This is the old write-recovery
attack through optional payload extraction, without a written widening.

**Verdict: breaks, conditional attack against the claimed sound join.**
`ne.var.surface` can instead forbid joining the invariant slot. In that
reading this program is rejected while constructing or projecting the
optional; the rule that wrapping merely loses independence needs that qualification.

Keeping the original object's equality constraint can also reject it.
That is a rejection derivation the record needs to show across opaque
calls, rather than relying on seeing this literal locally.

### R23. A Tuple Can Hide The Same Widening

Original shape, approximating a Rust function returning two independently
borrowed components:

```rust
fn bundle<'a, 'b>(a: &'a RefCell<Vec<&'a Socket>>, b: &'b Socket)
    -> (&'a RefCell<Vec<&'a Socket>>, &'b Socket) { (a, b) }
```

```text
fn bundle(dst: ^mut List[Conn], c: ^Conn) -> ^{dst, c} (mut List[Conn], Conn): # hypothetical syntax
    (dst, c)

fn through_tuple(dst: ^mut List[Conn], c: ^Conn) -> void: # hypothetical syntax
    pair := bundle(dst, c)
    pair._0.push(pair._1)
```

Tuples have one reach, the union of their elements (`ne.data.tuple`).
The published result gives `{dst, c}` and no component relation.
If both projections use that one reach, the mutable list accepts `c`,
again concealing a store from the helper's signature.

**Verdict: breaks, missing aggregation/projection boundary.** A sound
reading rejects `bundle` because the mutable component cannot acquire
the union. A component-preserving representation could accept the tuple,
but that information is absent from the stated one-reach summary.

Local constant propagation is not a modular answer: `bundle` may live in
another file, and callers are explicitly forbidden to read its body.
The rule must explain a rejection or export the relation.

### R24. A Bound Appender Needs An Environment Store Effect

Original shape, approximating [Go method values](https://go.dev/ref/spec#Method_values):

```go
put := destination.Push
put(connection) // mutates the captured receiver
```

```text
fn apply_store( # hypothetical syntax
    put: ^fn(c: ^{put} Conn) -> void, # hypothetical syntax
    c: ^{put} Conn # hypothetical syntax
) -> void:
    put(c)

fn declared_store(dst: ^mut List[Conn], c: ^{dst} Conn) -> void: # hypothetical syntax
    apply_store(dst::push, c)

fn undeclared_store(dst: ^mut List[Conn], c: ^Conn) -> void: # hypothetical syntax
    put := dst::push
    put(c)
```

`List.push` has a `^{self}` input. Binding `self` does not eliminate
the retention effect: the function value stores into its environment's
captured receiver. The first helper supplies a place to name that effect.

The bound reference needs to translate `self` into an environment binder.
It must preserve that binder when stored locally, passed through another
callback type, or expressed using `Fn`. Existing `fn.ref.bound.type`
only removes the receiver and preserves the remaining ordinary parameter types.

`ne.var.fn` also checks only mutable results and results of returned
functions. A setter returns `void`, but its environment store effect
must prevent an environment widening that admits a new root. Input
effect comparison may reject that conversion; the record needs to state it.

**Verdict: breaks, missing translation and effect-subtyping obligation.**
The first call should keep its declared effect. The final body must be
rejected. No unconditional accepted exploit is alleged: full polarity
over the store effect may provide the necessary rejection.

The known inability to name an ambiguous result environment outside a
parameter (L6) does not specify this case: the result here is `void`,
and the dependency is on an input store.

### R25. A Cold Call Must Capture The Called Closure

Original shape, approximating an async closure whose output carries no resource:

```rust
let action = async || socket.ping().await;
let future = action(); // retains what the action needs, even with unit output
```

```text
fn cold_action(action: ^fn!() -> void) -> ^{} mut Suspend[void]: # hypothetical syntax
    action()

fn store_action(action: ^fn!() -> void) -> void: # hypothetical syntax
    let pending: mut Suspend[void] = cold_action(action)
    _ := pending
```

`ne.suspend.slots` says `run` includes captures. `ne.suspend.cold`
defines it as the union of arguments and construction-time providers,
omitting the function value's environment. This no-argument, empty-row
call therefore has empty `run` under the literal cold-call formula.

The action may hold a borrowed handle. Its cold frame must retain either
the closure value or the captures needed by its future execution. The
output being `void` changes neither fact.

**Verdict: breaks, conflicting formulas with a straightforward repair.**
With captures included, `cold_action` is rejected: its result promises
`^{}` but retains `action`. This is a narrow omission, not evidence
that the two-slot direction is wrong.

### R26. Lowered Constructors Need Both Frame Slots

Original shape, approximating a future factory with more retained inputs
than output dependencies:

```rust
async fn select<'a, 'b>(keep: &'a Socket, scratch: &'b Socket) -> &'a Socket {
    observe(scratch);
    keep
}
```

```text
fn select!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn: # hypothetical syntax
    _ := scratch.sent()
    keep

fn via_lowered!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn: # hypothetical syntax
    let make: fn(a: ^Conn, b: ^Conn) -> ^{a, b} mut Suspend[Conn] = select # hypothetical syntax
    let pending: mut Suspend[Conn] = make(keep, scratch)
    pending!()
```

The existing language converts `fn!(A) -> T` into its ordinary cold
constructor type. The revised design needs that conversion to export
`run={a,b}` and `out={a}`. One prefix bound on the constructor's
result applies to both slots (`ne.syntax.one-reach`).

The displayed annotation therefore loses output precision: both slots
become `{a,b}`, and `via_lowered!` cannot satisfy `{keep}`. Simply
copying the source's `{keep}` onto the constructor result would instead
drop `scratch` from `run`, an unsound retention summary.

**Verdict: breaks, missing conversion specification.** The example's
coarse annotation can legitimately be rejected or lose precision. The
problem is the claimed exact lowering has no specified two-slot target
or transformation in the revised function-constructor rules.

Explicit frame slot arguments may already be the intended representation.
The record should show that spelling, its binding scope, and its identity
with the `Fn`/`SuspendFn` constructor forms. A caller must not infer this
information from `select`'s body.

### R27. A Scoped Callback Can Forbid Its Argument's Escape

Original shape, approximating a scope-providing library:

```rust
with_socket(|socket| socket.sent()) // returns a count, not the borrowed socket
```

```text
fn with_conn[T](body: ^fn(c: ^Conn) -> ^{} T) -> ^{} T: # hypothetical syntax
    c := open()
    body(c)

fn good_scope() -> usize:
    with_conn(fn(c: ^Conn) -> usize: c.sent()) # hypothetical syntax

fn bad_scope() -> Conn:
    with_conn(fn(c: ^Conn) -> ^{c} Conn: c) # hypothetical syntax
```

Within the callback, `c` has a parameter root. Its implementation is
checked independently of the fact that `with_conn` supplies a fresh,
static-reaching handle. The `^{}` callback result excludes that root.

**Verdict: works.** `good_scope` fits; the callback supplied by
`bad_scope` has a dependency its expected type excludes. This is a
usable library boundary, provided function-value checking preserves the
result relation and the input's local contract.

The displayed library deliberately rejects results dependent on its
environment too. An API wishing to allow those can name `{body}`
instead; that still excludes the callback's own `c` input. No closing
guarantee is claimed by either version.

### R28. Captured Storage Is Not A Callback-Local Scratch Object

Original shape, approximating a Go closure that changes shared captured storage:

```go
var saved *Socket
put := func(x *Socket) { saved = x }
put(connection) // saved remains observable after put returns
```

```text
fn invoke_once(f: ^fn(x: ^Conn) -> void, c: ^Conn) -> void: # hypothetical syntax
    f(c)

fn captured_cell(c: ^Conn) -> ^{c} Conn?: # hypothetical syntax
    let saved: Conn? = .None
    invoke_once(fn(x: ^Conn) -> void: saved = .Some(x), c) # hypothetical syntax
    saved
```

The callback's input is bare `^`: its advertised contract is never kept.
The callback nevertheless stores it in a shared capture cell that survives
the call. Returning `saved` from the outer function is deliberately allowed;
the violation is the callback's falsely advertised non-retention.

`ne.local.captured` unions assignments to shared bindings, but does not
state when a captured cell stops being a flexible local reach variable.
`ne.heap.fixed` explicitly covers parameters' objects; the closure's
environment needs the corresponding boundary treatment.

**Verdict: breaks, missing closure checking obligation.** A sound checker
must reject this callback against the bare-input function type, or require
an environment store effect such as R24's. Treating the assignment as a
store into a fresh local cell is unsound for its `LocalParams` promise.

This is not a new demand for exclusive borrows or ownership. The shared
cell semantics already exist. What is missing is an explicit rejection
derivation using the function value's exported retention contract.

### R29. Mutable Outputs Can Be Independent Of The Environment

Original shape, approximating a Rust closure that observes captured state
but returns new independent storage:

```rust
let make = || { observe(&resource); Vec::<Socket>::new() };
```

```text
fn use_maker(make: ^fn() -> ^{} mut List[Conn], extra: ^Conn) -> void: # hypothetical syntax
    let wider: ^{make, extra} fn() -> ^{} mut List[Conn] = make # hypothetical syntax
    _ := wider()
```

The returned mutable list explicitly depends on nothing in the environment.
Widening the environment cannot relabel the result's reach: it remains
`^{}`. Nevertheless `ne.var.fn` makes the environment invariant whenever
the result exposes mutable storage, without testing that dependency.

**Verdict: friction.** The conservative rejection preserves soundness.
It is stricter than a dependence-sensitive polarity calculation, and the
claim that no slot is invariant without cause overstates its precision.
This is an evaluation of NE-S1's cost, not a decision to relax it.

### R30. Generic Scalar Results Remain Tied

Original shape, approximating Rust iterator mapping to owned counts:

```rust
let sizes: Vec<usize> = sockets.iter().map(|s| s.sent()).collect();
```

```text
fn map[T, U](xs: ^List[T], f: ^fn(x: ^T) -> ^{x, f} U) -> ^{xs, f} List[U]: # hypothetical syntax
    panic("parser stub: supplied library map")

fn sizes(xs: ^List[Conn]) -> ^{} List[usize]: # hypothetical syntax
    map(xs, fn(c: ^Conn) -> usize: c.sent()) # hypothetical syntax
```

The published `map` contract carries `xs` and the callback's environment
regardless of `U`. Instantiating `U=usize` does not remove the reach,
so the result cannot satisfy `sizes`'s `^{}` promise.

**Verdict: friction, already accepted as L9.** Directly building a fresh
list from scalar observations can avoid the generic contract's extra
dependency. No relaxation of the owner decision is proposed.

This is a common API composition cost, not an obscure allocation-site case.
It belongs in agent ergonomics and expressiveness evaluation even after
being documented as intentional.

### R31. Ordinary Identity Now Preserves Provenance

Original shape, approximating a Go helper returning an existing buffer:

```go
func identity(b []byte) []byte { return b }
```

```text
let BUFFERS: mut List[List[u8]] = []

fn identity(b: ^List[u8]) -> ^{b} List[u8]: # hypothetical syntax
    b

fn must_reject(b: ^List[u8]) -> void: # hypothetical syntax
    BUFFERS.push(identity(b))

fn fresh_is_allowed() -> void:
    BUFFERS.push(identity([1, 2]))
```

`must_reject` forwards its parameter root through the result relation,
so its global store fails. A fresh ordinary buffer has empty reach,
and the second store succeeds. The optimizer separately keeps the
identity edge when deciding which allocation escapes.

**Verdict: works.** This repairs the earlier NE-S2 counterexample and
the C14 optimization obligation without pretending that empty reach
means no object is retained.

### R32. Fresh Handle Escape Is An Accepted Boundary

Original shape, approximating a Go callback registry:

```go
socket := open()
register(func() int { return socket.Sent() })
// The local scope still has its alias; resource lifetime is a library concern.
```

```text
let READS: mut List[fn() -> usize] = []

fn register(read: fn() -> usize) -> void:
    READS.push(read)

fn fresh_registration() -> void:
    c := open()
    register(fn() -> usize: c.sent())

fn borrowed_registration(c: ^Conn) -> void: # hypothetical syntax
    register(fn() -> usize: c.sent())
```

The first closure captures a fresh root, which fits `static`; its type
is ordinary. The second captures a parameter root and fails the global
argument check. Both results follow the active rules without closing analysis.

**Verdict: works, intentional scope reduction.** OQ1 and the owner's
`dfb53fe8` clarification explicitly accept fresh escape through a callback,
erasure, or top-level storage. These routes are not reported as new holes.
Calling the design a prevention of resource escape without that qualification
would misdescribe its current guarantee.

### R33. A Bare Local Callback May Escape In The Result

Original shape, approximating ordinary Rust closure forwarding:

```rust
fn forward<F: Fn() -> usize>(f: F) -> F { f }
```

```text
fn forward(f: ^fn() -> usize) -> fn() -> usize: # hypothetical syntax
    f

fn escaped_callback(c: ^Conn) -> ^{c} fn() -> usize: # hypothetical syntax
    forward(fn() -> usize: c.sent())
```

The output of `forward` elides to `{f}`. Returning the original closure
is allowed: a bare local parameter is not kept anywhere its signature
does not name, and this signature names the result through elision.

`ne.opt.fact` and `ne.opt.escape` correctly export that result edge.
However, `ne.opt.closure` says a closure literal passed to a `^fn`
parameter is not stored by the callee and can be dropped after specialization.
A mark alone cannot establish that: the returned value still needs its captures.

**Verdict: breaks, optimizer wording rather than a demonstrated miscompile.**
An optimizer reading the complete `LocalParams` summary preserves the
closure here. The closure-specific rule must say "never-kept" rather
than simply "`^fn`", consistently with the neighboring rules.

Specialization may remove an environment if its own analysis proves that
safe. Merely having a specialized call-site instance is not proof that
an environment returned from that call is dead.

## Comparison With Other Languages

| Comparison | What it establishes | Limit of the analogy |
| --- | --- | --- |
| [Rust variance](https://doc.rust-lang.org/nomicon/subtyping.html) | Mutable storage is invariant in its stored lifetime; callable inputs reverse polarity | Returning a mutable component through a tuple or `Option` keeps its original type relation; Rust does not replace independent lifetimes with one union |
| [Rust `Future`](https://doc.rust-lang.org/std/future/trait.Future.html) | The output type is distinct from the future state used while polling | hd's explicit `run`/`out` slots are its own mechanism; the comparison does not prove the new lowering correct |
| [Go method values](https://go.dev/ref/spec#Method_values) | A bound method captures its evaluated receiver | Go supplies no static non-retention guarantee; hd must additionally translate the receiver's store effects |
| [Kotlin local lifetime notes](https://github.com/Kotlin/KEEP/blob/main/notes/0007-local-lifetimes.md) | Local lifetimes address retention in callbacks and containers | The notes do not validate hd's provider classification, slot erasure, or closure-cell checking |

hd is closer to a checked retention-contract system than a resource
ownership system after OQ1. That can be useful on its own terms. Neither
Rust's memory-safety proof nor Go's GC semantics transfers automatically
to the combination of shared mutable storage and erased reach relations here.

## Problems, Ranked

Severity describes the consequence if a permissive interpretation is
implemented. Missing translations are not proven accepted programs.
Paired wrapper cases count as one underlying problem, not separate discoveries.

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | NE-S6b | Ordinary provider values are omitted from key tracking | Critical contract conflict | R21 |
| 2 | NE-S1b | Joining slots can widen invariant components | Critical conditional store violation | R22, R23 |
| 3 | NE-S4b | Captured stores need exported effects and closure-boundary checks | High, modular contract and optimizer obligation | R24, R28 |
| 4 | NE-S5b | Cold closure construction omits its environment in one formula | High, direct rule conflict | R25 |
| 5 | NE-S5c | Callable weakening lacks a two-slot constructor contract | High, retention or precision failure if lowered naively | R26 |
| 6 | NE-S1c | Blanket mutable-result test overconstrains unrelated reaches | Moderate precision cost | R29 |
| 7 | NE-S11b | Generic scalar results retain accepted dependencies | Moderate ergonomic cost, already accepted | R30 |
| 8 | NE-S7b | Closure optimization wording ignores a permitted result edge | Moderate, explicit summary-rule conflict | R33 |

### NE-S6b. Ordinary Providers Need Provenance

**Effect.** R21 extends NE-S2 and NE-S6. The category is not a test
for whether a value can carry a borrowed root. Ordinary callback holders,
frames, and erased values now make that distinction explicit. A named
ordinary provider cannot safely disappear from its callee's retention summary.

**Candidates, for the owner.**

| Question | Cost | Consequence |
| --- | --- | --- |
| Should every provider key create a reach root and count for result elision? | One language semantic generalization | Preserves ordinary providers with local contents; more provider-bearing results need written bounds |
| Should a non-NonEscapable key accept only providers whose reach fits `static`? | One language semantic restriction | Existing key tracking can remain narrow, but ordinary local callback/frame providers are rejected |

An ordinary library wrapper cannot repair provenance lost inside
`$.use`; this is a language-tier boundary. Requiring all such service
traits to declare NonEscapable would change their category globally, including
implementations that hold nothing local, and is not a free library repair.

### NE-S1b. Joining Is A Variance Conversion Too

**Effect.** R22 and R23 recover a writable alias through aggregate
construction or projection. NE-S1 fixes explicit widening, but a rule
that replaces an invariant component's reach with a union performs the
same dangerous operation. The safe answer may be rejection, not extra precision.

**Candidates, for the owner.**

| Question | Cost | Consequence |
| --- | --- | --- |
| Should each aggregate join require every affected invariant component already to have that joined reach? | One language semantic restriction | Rejects R22/R23 when sources differ; existing slot-bearing holders can keep them separate |
| Should wrappers retain independent component slots instead of joining them? | A language semantic representation change | Preserves more safe programs, but extends the accepted one-reach wrapper decision |

The second candidate is listed only because invariance is new evidence
against an unconditional claim that joining is sound. It is not a request
to reopen the old precision preference without cause. Ordinary dedicated
slot-bearing holders are a cheap workaround under the first candidate.

### NE-S4b. A Closure's Environment Is A Retention Destination

**Effect.** R24 needs `self`-relative stores translated when the receiver
becomes a capture. R28 needs the same destination when checking a written
closure. Local input mode, store destinations, and environment polarity
must survive method references and function-type conversion.

**Candidates, for the owner.**

| Question | Cost | Consequence |
| --- | --- | --- |
| Should bound references and written closures share an environment store-effect rule, with captured objects/cells fixed at that boundary? | Language semantic translation and checking rules | Preserves never-kept contracts and makes setter environment variance depend on input effects too |
| Should callable forms whose environment stores cannot be represented be rejected, with a named data method as the workaround? | One language semantic restriction; ordinary library wrappers | Smaller callable surface; valid getter-only callbacks remain available |

No new ownership or closure mutation keyword is needed by either
candidate. The environment name already exists on parameter function types.
The unresolved part is its scope, translation, and negative occurrences,
especially for a reference first held in a local.

### NE-S5b. Include The Called Closure In Cold Frames

**Effect.** R25 has empty arguments and row, but a nonempty called
environment. Treating only arguments and providers as `run` drops the
one value needed to execute the computation. The broader slots rule
already says captures count.

**Candidate, for the owner.** Should `ne.suspend.cold` explicitly join
the called function value's environment as well as arguments and providers?
This is one language semantic clarification, using existing closure reach
and frame slots. It introduces no syntax, intrinsic, or library item.

### NE-S5c. Preserve Both Slots Through Callable Lowering

**Effect.** R26 crosses a representation equivalence the current language
already provides. A single result prefix cannot express differing `run`
and `out` bounds. Retaining all inputs is safe but loses the new output
precision; retaining only output inputs is an invalid frame contract.

**Candidates, for the owner.**

| Question | Cost | Consequence |
| --- | --- | --- |
| Should existing slot-argument notation express constructor results, with a specified transformation between `SuspendFn` and `Fn`? | Language semantic translation; syntax only if current slot applications exclude `Suspend` | Keeps the two-slot distinction through existing callable weakening |
| Should lowering to a coarse ordinary constructor deliberately join both slots and advertise the precision loss? | One language semantic restriction/qualification | Safe, but exact lowering and the current equivalence claims need qualification |

The `Fn` constructor's existing input tuple, output type, and row must
also encode local input modes and store/result binders. The design calls
out changing parameter names, but that alone does not specify the new
constructor identity. This waits on the same core representation answer.

### NE-S1c. Compute Variance Of Dependencies, Not Unrelated Results

**Effect.** R29 exposes a safe case rejected by the blanket mutable-result
test. The mutable result is independent of the environment being widened.
This is precision evidence, not a newly found resource leak.

**Candidates, for the owner.**

| Question | Cost | Consequence |
| --- | --- | --- |
| Should polarity follow only result/store relations that actually mention the reach being checked? | One language semantic refinement | Admits independent fresh mutable results while preserving invariance for related storage |
| Should this conservative test stay and be listed as an additional known limit? | Documentation only | Lowest implementation cost; the precision claim must acknowledge it |

### NE-S11b. Account For The Accepted Generic Cost

**Effect.** R30 shows L9 in ordinary mapping code. The soundness problem
is repaired by keeping the declared dependency; precision is intentionally
not recovered from the instantiated scalar result.

**Candidate, for the owner.** Should the guide show this diagnostic
and a fresh-result helper/loop alongside generic `map`? This is ordinary
library and documentation work, with no language change. The accepted
exact-substitution decision is not reopened.

### NE-S7b. Use The Full Retention Summary For Closures

**Effect.** R33 is legal under the active design and retains its
callback through the result. The local parameter mark permits that flow;
it does not by itself mean the callback can be discarded.

**Candidate, for the owner.** Should `ne.opt.closure` require a
never-kept callback, with no declared or elided result/store sources,
before using the non-retention license? This is a documentation clarification
of `ne.opt.fact`, with no new language mechanism or implementation claim.

## Evaluation By Dimension

These are the eleven substantive dimensions used in the first review;
the original migration dimension stays excluded at the owner's request.
"Improved" measures the revised guarantee, with its scope reduction explicit.

| Dimension | Earlier assessment | Revised assessment | Evidence still needed |
| --- | --- | --- | --- |
| Guarantee clarity | Closing, escaping, provenance, and authority were conflated | **Much better, narrower.** Parameter/key retention is the promise; disposal safety is expressly excluded | Fix ordinary-key wording in R21; keep fresh-versus-borrowed examples prominent |
| Soundness | Direct write recovery, ordinary laundering, and cleanup holes | **Improved, not established.** Direct attacks fixed; invariant aggregation and ordinary providers remain | Modular rejection derivations for R21–R24 and R28 |
| Precision | Silent loss through wrappers and root sites | **More honest, mixed capability.** Limits named; `run`/`out` improves direct frames; fresh roots make site precision irrelevant | Characterize rejection of invariant aggregates; R29 and accepted L9 |
| Practical expressiveness | Simple scopes worked; leases and context-dependent cleanup struggled | **Useful for retention contracts.** Scoped callbacks and escaping fresh handles work; automatic resource-scope safety is outside scope | Real scoped API implementations, callback stores, and ordinary local providers |
| Composability | Helpers, method values, frames, and provider callbacks exposed holes | **Partly repaired.** Ordinary helper composition works; new rules are still incomplete at aggregate/callable/provider boundaries | R21–R26 and R28 |
| Modular contracts | Signatures were promising but omitted necessary facts | **Stronger summaries, unfinished translations.** Ordinary results and frame output slots are exported | Environment stores, frame-constructor relations, and ordinary provider roots |
| Agent writability and human readability | Small syntax hid many different promises | **Clearer prose, still demanding code.** Bare `^` means a retention promise; users must understand category versus actual reach | Diagnostics that explain capture stores and invariant wrappers; no agent trial was run |
| Conceptual cost | Category, resource lifetime, provenance, closing, optimizer escape overloaded one story | **Reduced by parking closing.** Remaining category/reach distinction is real and necessary in current rules | Ensure category is not reused as a proxy for reach, as R21 does; assess fresh roots' now-inert role |
| Compiler feasibility and cost | Finite bitsets plausible; asserted bound unproved | **More credible.** Cleanup liveness removed; worklist estimate qualified | Finite interface representation for dependent callable modes/slots, recursive surface checks, and benchmarks |
| Optimization validity and payoff | Dependent results incorrectly licensed fusion/reuse | **Substantially better claims.** Caller alias/liveness and specialization prerequisites are explicit | Callback capture checking in R28; qualify `^fn` wording in R33; measured payoff remains absent |
| Specification readiness | Not ready | **Closer, still not ready for a complete sound system.** Several changes are substantive repairs, not just new examples | Settle the ranked core boundaries; annotation-handle operations remain explicitly deferred |

The conceptual improvement is important: the proposal no longer asks a
reach to prove who may close a resource. It still must distinguish
an instantiated type's category from the roots a particular value retains.
R21 shows why that distinction must apply consistently across every boundary.

The main review verdict is therefore **material improvement, incomplete
composition proof**. The old report should not be read as eleven unchanged
active failures. Nor do revised verdict tables establish soundness of
the combined rules without the missing boundary derivations.

## Parse Log

The parser was invoked as `scripts/hd-conformance.sh parse FILE` in an
isolated checkout of refreshed `origin/main` at `dfb53fe8`. The existing
parser binary was copied into that checkout. Blocks are parsed separately;
name resolution is not performed, so the prelude is not prepended.

Three passes were run on each of this report's fourteen `text` blocks:

1. **Raw:** unchanged proposed hd. All fourteen are rejected at lifetime syntax
   the current parser does not implement. Every such line is marked hypothetical.
2. **Markers removed:** remove `[^a, ^b]` slot declarations, `^{...}` bounds,
   and bare `^`, preserving line breaks and comments. Nine blocks parse;
   five reach the proposed named function-type parameters, also unimplemented.
3. **Structure:** additionally erase parameter names inside the function types
   following type-position colons. Names on executable closure literals stay.
   All fourteen blocks parse. This is a syntax check of surrounding structure,
   not validation of the removed lifetime or binder semantics.

The projection operates only before each line's comment. It does not give
abstract function signatures bodies, remove statements, or run examples.
`FILE` below replaces only the temporary absolute path. Diagnostics show
only the first parser message for each rejected block, verbatim otherwise.

| Block | Case | Raw first diagnostic | Markers-removed result | Structure result |
| --- | --- | --- | --- | --- |
| 1 | Prelude | `error: FILE:3:13: syntax-error: expected a parameter name, found '^'` | Parse | Parse |
| 2 | R21 | `error: FILE:2:13: syntax-error: expected a parameter name, found '^'` | Parse | Parse |
| 3 | R22 | `error: FILE:1:19: syntax-error: expected a generic parameter name, found '^'` | Parse | Parse |
| 4 | R23 | `error: FILE:1:16: syntax-error: expected a type, found '^'` | Parse | Parse |
| 5 | R24 | `error: FILE:2:10: syntax-error: expected a type, found '^'` | `error: FILE:2:14: syntax-error: expected ')', found ':'` | Parse |
| 6 | R25 | `error: FILE:1:24: syntax-error: expected a type, found '^'` | Parse | Parse |
| 7 | R26 | `error: FILE:1:18: syntax-error: expected a type, found '^'` | `error: FILE:6:19: syntax-error: expected ')', found ':'` | Parse |
| 8 | R27 | `error: FILE:1:23: syntax-error: expected a type, found '^'` | `error: FILE:1:27: syntax-error: expected ')', found ':'` | Parse |
| 9 | R28 | `error: FILE:1:19: syntax-error: expected a type, found '^'` | `error: FILE:1:23: syntax-error: expected ')', found ':'` | Parse |
| 10 | R29 | `error: FILE:1:20: syntax-error: expected a type, found '^'` | Parse | Parse |
| 11 | R30 | `error: FILE:1:18: syntax-error: expected a type, found '^'` | `error: FILE:1:34: syntax-error: expected ')', found ':'` | Parse |
| 12 | R31 | `error: FILE:3:16: syntax-error: expected a type, found '^'` | Parse | Parse |
| 13 | R32 | `error: FILE:10:29: syntax-error: expected a type, found '^'` | Parse | Parse |
| 14 | R33 | `error: FILE:1:15: syntax-error: expected a type, found '^'` | Parse | Parse |

The reviewed design's ninety-five `hd` blocks were also submitted unchanged
and with the marker-only projection. Eighteen parse raw; eighty parse after
projection. The fifteen remaining projection failures are listed below.
Signature-only sketches, named callback types, and parked `closing` forms
are distinguished from this report's executable structure checks.

The original's `text` fence containing a grammar production is a grammar
sketch, not an hd example, and is intentionally excluded from this count.

| Design block | Source line at `dfb53fe8` | Projection's first diagnostic |
| --- | --- | --- |
| 2 | 292 | `error: FILE:2:1: syntax-error: expected ':' and a function body, found a line ending` |
| 3 | 334 | `error: FILE:2:1: syntax-error: expected ':' and a function body, found a line ending` |
| 10 | 626 | `error: FILE:1:34: syntax-error: expected ')', found ':'` |
| 11 | 677 | `error: FILE:2:1: syntax-error: expected ':' and a function body, found a line ending` |
| 13 | 920 | `error: FILE:17:1: syntax-error: expected ':' and a function body, found a line ending` |
| 41 | 1596 | `error: FILE:10:1: syntax-error: expected ':' and a function body, found a line ending` |
| 49 | 1788 | `error: FILE:1:34: syntax-error: expected ')', found ':'` |
| 57 | 2023 | `error: FILE:11:1: syntax-error: expected ':' and a function body, found a line ending` |
| 68 | 2335 | `error: FILE:2:22: syntax-error: expected ':', found 'self'` |
| 69 | 2366 | `error: FILE:1:20: syntax-error: expected ':', found 'c'` |
| 71 | 2514 | `error: FILE:2:25: syntax-error: expected ':', found 'self'` |
| 72 | 2536 | `error: FILE:1:17: syntax-error: expected ':', found 'c'` |
| 78 | 2636 | `error: FILE:2:23: syntax-error: expected ':', found 'self'` |
| 92 | 2860 | `error: FILE:8:24: syntax-error: expected ':', found 'self'` |
| 93 | 2885 | `error: FILE:1:19: syntax-error: expected ':', found 'c'` |

Projection failures 2, 3, 11, 13, 41, and 57 include abstract free-function
signatures without bodies. Blocks 10 and 49 use named callback types.
Blocks 68, 69, 71, 72, 78, 92, and 93 contain parked `closing` syntax,
which this projection deliberately leaves in place. These failures do not
establish acceptance or rejection by the proposed lifetime checker.

### Repository Validation

Validation runs in the isolated checkout because the shared working tree
has concurrent work. `git fetch origin` refreshed the base; `git rebase
origin/main` there reported `HEAD is up to date`. Only this report and
its README link were copied over the base for the documentation checks.
The existing playground Wasm asset supplies the website build prerequisite.

- Parser: all fourteen report blocks checked in all three modes; all fourteen
  structural projections parse. No semantic checker or runtime claim follows.
- `bash spec/check.sh`: passed, including links, anchors, 5960 rule IDs,
  and 1396 diagnostic rows; eighty existing dead-citation warnings remain.
- `pnpm run website:build`: passed, building 79 pages and 265 grammar rules.
- No specification, design decision, compiler, or conformance fixture was changed.
  No commit or push was made.
