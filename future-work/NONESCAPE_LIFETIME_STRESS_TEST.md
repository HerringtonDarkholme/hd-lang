# NonEscapable Types And Lifetimes: Adversarial Stress Test

Status: design review, 2026-10-14; nothing here is accepted behavior. This
report changes no decision, design record, spec text, fixture, or compiler.
It tests [the design record](NONESCAPE_LIFETIME_DESIGN.md), including
D1–D8, Q1–Q16, and its separately identified agent details.

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Problems, Ranked](#problems-ranked)
- [Evaluation By Dimension](#evaluation-by-dimension)
- [Parse Log](#parse-log)

## Surface Being Tested

The owner asked for an adversarial stress test. Migration and compatibility
are excluded: hd is the owner's toy language. This review tries to defeat
the guarantees, rather than confirm the existing examples.

The assumed surface is the record's current notation, category rules,
root sets, one-reach containers, optional declared slots, parameter
locality, signature checking, closing authority, cleanup blocks, and
structured suspension. Early close remains a runtime matter. There are
no moves, exclusive borrows, Drop, `using`, or detached spawn.

The language dependencies are:

| Area | Specification under review |
| --- | --- |
| Views, shallow readonly access, variance, identity | [Type System](../spec/lang/04-type-system.md#access-permission), especially `types.readonly.nested`, `types.readonly.results`, and `types.polarity.mut` |
| Cleanup registration, storage, results, suite restrictions | [Deferred Cleanup](../spec/lang/06-control-flow.md#deferred-cleanup) |
| Function constructors, function values, method references, captures | [Functions](../spec/lang/07-functions.md#function-types-and-values), [Method References](../spec/lang/07-functions.md#method-references), and [Captures](../spec/lang/07-functions.md#captures) |
| Fields, generic data, wrappers, GC | [Data and Enums](../spec/lang/08-data-and-enums.md#data-declarations) |
| Trait values and runtime recovery | [Traits](../spec/lang/09-traits.md#runtime-type-identity) |
| Providers, frames, polling, cancellation, `all!`, `race!` | [Requirements and Suspension](../spec/lang/11-requirements-and-suspension.md#suspendt-protocol) |
| Field handles and construction | [Annotation handles](../spec/lang/14-annotations.md#typed-derivation) |
| Concrete library behavior | [Net](../spec/std/net.md#socket-handles), [Task](../spec/std/task.md), [Iter](../spec/std/iter.md), and [Collections](../spec/std/collections.md) |

The socket prelude abbreviates real resource errors. It models synchronous
test handles when cleanup sequencing is the subject, avoiding suspension
as an unrelated cause of rejection. `open()` returns a new independent
handle. Its parser stub stands for a harness constructor and is never run.

```text
trait NonEscapable
trait Socket < NonEscapable:
    fn sent(^self) -> usize # hypothetical syntax
    fn close(closing self) -> void # hypothetical syntax
type Conn = dyn Socket
fn open() -> Conn: panic("harness supplies this body")
```

Every case is an API-shape approximation, not a port of the named library.
Each case supplies its additional declarations locally. Signature-only
declarations stand for implementations obeying those signatures.

## Method

Twenty cases attack abstraction boundaries, cleanup sequencing, ordinary
object locality, resource protocols, and claimed optimizer facts.
Positive controls test where an apparent problem is an accepted limitation.
The prior record's X1–X7 are not presented as newly discovered problems.

| Verdict | Meaning |
| --- | --- |
| Works | The record supplies a consistent account of the case. |
| Friction | It rejects useful safe code or requires a workaround. |
| Breaks | Explicit rules conflict, information needed at a boundary is absent, or a permissive reading admits a guarantee violation. |

“Breaks” does not mean the current compiler accepted and executed the
program. It has no implementation of this design. Each case distinguishes
an explicit contradiction, a conditional counterexample, and a missing rule.

The most useful adversarial transformation is to keep runtime behavior
unchanged while changing its surface: direct field to trait getter,
closure to method reference, immediate bang call to stored frame, or
direct value to identity helper. A guarantee should survive these changes.

Syntax is checked separately from semantics. Each `text` block is submitted
unchanged to the parser. A second, mechanical projection removes the
unimplemented lifetime syntax, checking the remaining hd structure.
Neither pass proves typing, lifetime correctness, or runtime behavior.

No annotation or line-count comparison is used to rank options. Candidate
costs list syntax changes, semantic changes, intrinsics, and library additions.
The owner's cost order applies, with the fewest changes breaking ties.

## Summary

| Case | Library shape | Verdict | Main issue |
| --- | --- | --- | --- |
| C01 | Rust `Cell`/`RefCell` accessor, trait abstraction | Breaks | A mutable getter defeats the proposed `dyn` covariance test |
| C02 | Rust closure returning captured state | Breaks | Covariant closure reach can relabel an invariant mutable result |
| C03 | Rust `Vec` of mutable cells | Breaks | Readonly outer access does not prevent writes through nested values |
| C04 | Rust identity/helper composition | Breaks | Ordinary result erasure conflicts with locality enforcement |
| C05 | Go bound method value, cleanup callback | Breaks | Closing through method references has no specified treatment |
| C06 | Go deferred closure over reassigned storage | Breaks | Block-exit reach alone does not describe cleanup execution |
| C07 | Go deferred cleanup followed by observation | Breaks | Same-suite reads are outside the stated defer-order check |
| C08 | Rust `Future::Output`, task combinators | Breaks | Frame retention and output dependence have been collapsed |
| C09 | Kotlin higher-order locality, hd providers | Breaks | Function-type elision omits provider inputs in its explicit list |
| C10 | Go `database/sql.Conn.Close` lease | Friction | Closing authority includes dependencies the closer does not close |
| C11 | Rust `Option` around a two-source holder | Friction | Optional wrapping destroys independent slots |
| C12 | Rust `Vec::clear` reuse | Friction | Historical reach survives after all references are removed |
| C13 | Rust stateful iterator aliases | Breaks | A dependent result does not prove a unique pipeline |
| C14 | Go pooled buffers, dependent ordinary results | Works, with a proof obligation | Optimizer escape edges must survive empty resource reaches |
| C15 | Rust loop allocations | Friction | Distinct evaluations at one site share a cleanup root |
| C16 | hd `race!`, scoped child cancellation | Works | Cancellation ordering supports the stated scoped case |
| C17 | Go explicit `Close`, delegated cleanup | Works, with a guarantee boundary | Early and delegated closing still allow disposed aliases |
| C18 | Phantom metadata / hd `Field[S, F]` | Breaks | Category-by-argument conflicts with category-by-stored-field |
| C19 | Rust `Any` classifier | Friction | A local erased parameter cannot downcast even an ordinary value |
| C20 | Go callback factory capturing only a scalar | Friction | Fresh-result elision creates a local root despite no resource capture |

The highest-risk weakness is recoverable write authority after reach
widening. C01–C03 extend the existing X6 attack without any downcast.
Three further boundaries need complete accounts: ordinary provenance,
cleanup execution, and function-value closing behavior.

The existing examples establish useful intended behavior, but cannot
establish soundness. Several “works” claims depend on distinguishing facts
that the one-reach representation currently merges or discards.

## Cases

### C01. A Trait Getter Replaces The Downcast

Original shape, approximating [Rust cells](https://doc.rust-lang.org/std/cell/struct.Cell.html):

```rust
trait Window<'a> {
    fn items(&mut self) -> &mut Vec<&'a Socket>;
}
```

```text
trait Window:
    fn items(^mut self) -> ^{self} mut List[Conn] # hypothetical syntax

fn insert_hidden(w: ^mut dyn Window, c: ^Conn) -> void: # hypothetical syntax
    let wide: ^{w, c} mut dyn Window = w # hypothetical syntax
    wide.items().push(c)
```

`Window` has no parameter marked `^{self}`. `ne.var.dyn` therefore
permits reach covariance, including through mutable trait views.
`ne.call.result` gives `items()` the widened reach. The store fits that
returned list's reach without requiring a downcast.

At runtime the getter returns the original backing list. The call can put
`c` into a holder the original caller regards as independent of `c`.
The record's static-only downcast rule never runs. Ordinary lowering is a
trait-table call followed by `List.push`; neither operation repairs reach metadata.

**Verdict: breaks, conditional soundness counterexample.** The blanket
signature-store check must reject this body, or the covariance rule must
reject the conversion. The record does not explain how an opaque getter
exports the invariant origin needed to make either rejection modular.

An implementation that retains invariant object constraints may reject it.
That is a necessary reconciliation with `ne.var.dyn`, not evidence that
the explicitly permitted conversion is harmless. Getter results and
callable results belong in the variance analysis too.

### C02. A Closure Returns Mutable Captured State

Original shape, approximating a Rust closure factory:

```rust
let get = move || shared_cell.clone();
get().borrow_mut().push(short_lived);
```

```text
fn put_through_getter( # hypothetical syntax
    get: ^fn() -> mut List[Conn], # hypothetical syntax
    c: ^Conn # hypothetical syntax
) -> void:
    let wide: ^{get, c} fn() -> mut List[Conn] = get # hypothetical syntax
    wide().push(c)
```

The callback result elides to its environment. `ne.var.fn` makes the
function's own reach covariant because the function value has no `mut`
permission. The widened environment then becomes the mutable result's reach.

No access upgrade occurs. The original callback already returns a mutable
list. A call of the original function and a call of the widened function
can therefore yield aliases of one list with different invariant reaches.
Lowering still calls the same closure with the same environment.

**Verdict: breaks, conditional soundness counterexample.** A function
value lacking a permission modifier does not mean that calling it lacks
write authority. `fn.capture.result-not-weakened` explicitly preserves a
declared mutable result. C01's problem exists without a trait or downcast.

### C03. A Readonly Container Contains Mutable Children

Original shape, approximating Rust nested mutable cells:

```rust
let rows: Vec<Cell<Option<&Socket>>> = make_rows();
rows[0].set(Some(short_lived));
```

```text
fn put_through_row(rows: ^List[mut List[Conn]], c: ^Conn) -> void: # hypothetical syntax
    let wide: ^{rows, c} List[mut List[Conn]] = rows # hypothetical syntax
    wide[0].push(c)
```

`ne.var.generic` declares a generic container's one reach covariant
through a readonly view, with nothing left to compose. But
`types.readonly.nested` preserves a mutable element's permission.
Extraction gives a mutable list at the outer container's widened reach.

**Verdict: breaks, conditional soundness counterexample.** The explicit
readonly covariance rule conflicts with an invariant reachable child.
An opaque helper returning that child has the same problem as C01.
Lowering is ordinary indexing and mutation of the original inner list.

The issue is independent of strong updates. No overwrite needs to remove
a root. A single store exploits a reach conversion before any cleanup.

This caller makes the lifetime consequence concrete:

```text
fn nested_witness() -> usize:
    let inner: mut List[Conn] = []
    let rows: List[mut List[Conn]] = [inner]
    if true:
        c := open()
        defer: c.close()
        put_through_row(rows, c)
    inner[0].sent()
```

If the helper passes its signature check, its signature exports no store
of `c` into `rows`. The caller therefore keeps no dependency on `c`.
At runtime the final read reaches the handle closed by the inner block.
Rejecting the helper is essential; caller liveness alone cannot repair it.

### C04. Ordinary Locality Through An Identity Helper

Original shape, approximating a Rust borrowed-buffer helper:

```rust
fn identity<T>(x: T) -> T { x }
cache.push(identity(buffer));
```

```text
let CACHE: mut List[List[u8]] = []

fn identity_bytes(b: ^List[u8]) -> ^{b} List[u8]: # hypothetical syntax
    b

fn hidden_retention(b: ^List[u8]) -> void: # hypothetical syntax
    CACHE.push(identity_bytes(b))
```

The direct `CACHE.push(b)` is forbidden in the record's N3 example.
`identity_bytes` is allowed by `ne.param.ordinary-result`. Yet that rule
says the caller tracks nothing, and `ne.fntype.carry-only` reinforces
discarding reaches for ordinary results. The returned value still denotes
the same buffer at runtime.

**Verdict: breaks, explicit representation/contract conflict.** If “tracks
nothing” drops `b`'s locality provenance inside this caller, the helper
launders the forbidden store. If `ne.body.local` preserves the provenance,
the checker must track more than resource-carrying types and results.

This is not use-after-close: there is no resource in the example. It
breaks the general `^` contract and invalidates optimizer facts built on
that contract. A generic identity helper and ordinary wrapper fields need
the same audit.

### C05. A Bound Closing Method Is A Cleanup Callback

Original shape, approximating a [Go method value](https://go.dev/ref/spec#Method_values):

```go
cleanup := conn.Close
defer cleanup()
return conn
```

```text
fn cleanup_for(c: ^Conn) -> fn() -> void: # hypothetical syntax
    c::close

fn return_closed() -> Conn:
    c := open()
    cleanup := cleanup_for(c)
    defer: cleanup()
    c
```

`fn.ref.bound.capture` captures the receiver at reference creation.
`fn.ref.bound.type` removes the receiver from the callable inputs.
The generated function's visible signature is `fn() -> void`, with a
reach from `c`, but no closing parameter.

The record's X4 rejects a handwritten cleanup closure. This form has no
handwritten closure body and no closing call at the registration site.
The bound reference calls the same close operation during cleanup.
The saved return operand then contains the closed handle.

**Verdict: breaks, missing lowering and closing rule.** Checking the
generated reference as a closure would safely reject it. The record must
say that this check occurs; checking only source closure bodies misses it.
An unbound reference such as `Socket::close` also needs closing authority
preserved in its function type, or a specified rejection.

### C06. One Cleanup Suite Changes Another Suite's Target

Original shape, approximating [Go deferred closures](https://go.dev/ref/spec#Defer_statements):

```go
defer func() { current.Close() }()
defer func() { current = second }()
return second
```

```text
fn changed_target() -> Conn:
    let current = open()
    second := open()
    defer: current.close()
    defer: current = second
    second
```

At body exit, `current` holds the first handle. The second suite runs
first, changes `current`, and the first suite closes `second`.
The return operand was saved before either suite ran.

Algorithm 12.2 takes the closing argument's reach at each block exit.
If that means the ordinary body exit state, it marks the wrong root.
The returned second handle appears free and escapes while actually closed.

**Verdict: breaks, incomplete operational algorithm.** The relevant state
is at the closing statement during cleanup execution, after later suites.
It is not merely the state on entry to cleanup. A whole-scope captured-cell
union may reject this case, but also defeats the record's precise X3 claim.

The same problem occurs within one suite that reassigns `current` before
closing it. Branches, nested calls, and several exits require cleanup CFG
states, rather than a single exit snapshot. This does not require heap
strong updates: `current` is a local binding.

### C07. A Read Follows A Close Within The Same Suite

Original shape, approximating a cleanup-and-log pattern:

```go
defer func() { conn.Close(); observe(conn) }()
```

```text
fn observe_after_cleanup() -> void:
    c := open()
    defer:
        c.close()
        _ := c.sent()
```

`ne.cleanup.defer-order` checks earlier-registered suites. It says nothing
about later reads in the same suite. The read occurs before the owning
block exits, so `ne.outlive.check` alone does not reject it.

**Verdict: breaks, guarantee ambiguity.** The narrow promise that no alias
outlives the block permits this disposed read. The explanatory promise that
aliases die before cleanup closes them does not. No heap or return escape
is involved. Lowering faithfully executes close, then observation.

This case needs either an explicitly narrow guarantee or cleanup-statement
liveness. Early-close runtime behavior alone does not explain it, because
the close is written directly in a `defer`.

### C08. A Frame Retains More Than Its Output Depends On

Original shape, approximating [Rust futures](https://doc.rust-lang.org/std/future/trait.Future.html):

```rust
async fn select<'a>(keep: &'a Socket, scratch: &Socket) -> &'a Socket {
    observe(scratch).await;
    keep
}
```

```text
use std.task.all

fn select!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn: # hypothetical syntax
    _ := scratch.sent()
    keep

fn via_frame!(keep: ^Conn) -> ^{keep} Conn: # hypothetical syntax
    if true:
        scratch := open()
        defer: scratch.close()
        let (out,) = all!(select(keep, scratch))
        return out
    keep
```

The frame retains both inputs. Its result depends only on `keep`.
A direct `select!(keep, scratch)` preserves that distinction through
`ne.call.result`. But `Suspend[Conn]` has one reach, and no lifetime
inside its type argument records the output's subset.

**Verdict: breaks, missing result relation at frame erasure.** If polling
or `all!` gives `out` the frame's whole reach, safe code is rejected.
If it ignores the frame reach, a result that actually returns `scratch`
can escape its cleanup block. A signature-specific result relation must
survive storage and abstraction, or the conservative loss must be explicit.

Keeping a relation only for direct syntax is insufficient. A helper taking
`^mut Suspend[Conn]` must also know what reach its completed output has.
The sealed protocol permits a compiler solution, but its representation
and typing rules are not in the design. The tuple wrapper adds another
union boundary once several child results are combined.

### C09. A Callback Obtains Its Result From A Provider

Original shape, approximating a localized higher-order callback:

```kotlin
fun invoke(local callback: () -> Resource): Resource = callback()
```

```text
trait Pool < NonEscapable:
    fn get(^self) -> Conn # hypothetical syntax

fn from_pool() -> Conn $ Pool:
    $.use(Pool).get()

fn invoke(f: ^fn() -> Conn $ Pool) -> ^{$Pool} Conn $ Pool: # hypothetical syntax
    f()
```

`ne.sig.input` treats a NonEscapable provider key as a tracked input.
`ne.fntype.row` permits a callback result to reach its call-time provider.
But `ne.fntype.elide` lists callback inputs and environment without the
provider key. With no callback parameters it selects `f`'s environment.

The named function has an empty environment and a provider-dependent
result. Converting it to this callback type cannot erase that dependence.

**Verdict: breaks, conflicting elision accounts.** Applying function
elision consistently requires the provider to participate. That makes
this callback's omitted result ambiguous, unlike the written rule's answer.
If the implementation instead follows the environment-only answer, it must
reject `from_pool` or lose the provider dependence.

The record's generic `retry!` and `all_list!` examples have the same
question when `$R` can contain resource providers. Generated frame
providers, call-time closure providers, and callback output bounds need
separate substitutions. [Kotlin's notes](https://github.com/Kotlin/KEEP/blob/main/notes/0007-local-lifetimes.md)
support treating locality at higher-order boundaries as part of typing.

### C10. A Lease Needs Its Pool But Does Not Close It

Original shape, approximating [Go SQL connection leases](https://pkg.go.dev/database/sql#Conn.Close):

```go
conn, _ := pool.Conn(ctx)
defer conn.Close() // returns the connection to the pool
```

```text
data Lease:
    conn: Conn
    pool: dyn Pool
impl NonEscapable for Lease

impl Lease:
    fn release(closing self) -> void: # hypothetical syntax
        self.conn.close()

fn lease(c: ^Conn, pool: ^dyn Pool) -> ^{c, pool} Lease: # hypothetical syntax
    Lease { conn: c, pool: pool }

fn use_lease(pool: ^dyn Pool) -> void: # hypothetical syntax
    c := open()
    l := lease(c, pool)
    defer: l.release()
```

This minimal model closes the connection and retains pool metadata.
A real SQL release returns the connection to its pool instead.
Neither operation should close the pool itself.

`ne.close.owned` requires ownership of every root in the lease's reach.
The pool root is borrowed, so this cleanup is rejected. Even separate
declared slots do not make a closing call on the whole lease own the pool.

**Verdict: friction, by an owner-decided rule.** Reach describes retained
dependencies, not the close operation's target set. Treating both sets as
identical excludes wrappers with borrowed cleanup context. Closing only
the connection manually works here, but loses the intended wrapper API.

This is new use-case evidence about Q3, not a request to add moves or RAII.
The report does not silently reverse the decision.

### C11. Optional Wrapping Destroys Independent Slots

Original shape, approximating a Rust optional pair:

```rust
struct Pair<'a, 'b> { first: &'a Socket, second: &'b Socket }
fn first<'a, 'b>(p: Option<Pair<'a, 'b>>) -> &'a Socket { p.unwrap().first }
```

```text
data Pair[^a, ^b]: # hypothetical syntax
    first: ^{a} Conn # hypothetical syntax
    second: ^{b} Conn # hypothetical syntax
impl NonEscapable for Pair

fn first_optional(p: ^Pair?) -> ^{p} Conn: # hypothetical syntax
    match p:
        .Some(pair) => pair.first
        .None => panic("missing pair")
```

`ne.data.wrapper` joins both slots in `Option[Pair]`. Unwrapping restores
both at the union. The first field can no longer be kept independently
of the second, despite the declaration explicitly recording independence.

**Verdict: friction, accepted L5.** This is not an unsoundness claim.
The practical cost includes nullable sessions, fallible constructors,
collections of sessions, and recursive optional links. Declared slots
help only while the value stays outside ordinary generic wrappers.

The workaround is a dedicated holder with separately declared slots.
It introduces extra API types where an ordinary optional would suffice.

### C12. An Empty Container Remains Contaminated

Original shape, approximating [Rust vector clearing](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.clear):

```rust
items.push(short_lived);
items.clear();
use_empty_container(items);
```

```text
fn reuse_list() -> usize:
    let items: mut List[Conn] = []
    if true:
        c := open()
        defer: c.close()
        items.push(c)
        items.clear()
    items.len()
```

The list has no runtime alias to `c` after clearing. Its historical reach
still contains `c`, and reading its length keeps that reach live.
Reusing the empty list is rejected.

**Verdict: friction, accepted L1.** This affects scratch collections,
work queues, optional frame fields, and registries reset between phases.
A fresh list per cleanup scope works. No strong-update change is proposed:
the owner already decided not to add it now.

### C13. A Dependent Iterator Result Is Not A Unique Pipeline

Original shape, approximating [Rust stateful iterator adapters](https://doc.rust-lang.org/std/iter/trait.Iterator.html):

```rust
let mapped = shared_source.map(transform);
shared_source.next();
mapped.collect::<Vec<_>>();
```

```text
fn shared_pipeline(xs: List[usize]) -> List[usize]:
    let source = xs.iter()
    let mapped = source.map(fn(n): n + 1)
    _ := source.next()
    mapped.collect()
```

The source and adapter share iterator state. Advancing the source changes
what the adapter will produce. Under hd's shared mutable aliases this
requires no illicit permission upgrade.

`ne.opt.fusion` infers that an intermediate iterator is reachable only
through the next stage from the result's dependence on `self` and callback.
But that dependence is an upper bound on what the result retains. It says
nothing about other aliases that the caller already holds.

**Verdict: breaks, invalid optimizer implication.** The program itself is
expressible and safe. Fusion must preserve the extra advance and every
observable callback invocation. A caller alias/use analysis can justify
fusion of some chains; the signature alone cannot.

The runtime lowering must keep shared iterator state unless a separate
proof eliminates it. This is distinct from resource lifetime soundness.

### C14. An Ordinary Dependent Result Escapes

Original shape, approximating [Go pooled buffers](https://pkg.go.dev/sync#Pool):

```go
saved = append(saved, identity(buffer))
```

```text
fn retain_examples() -> List[List[u8]]:
    let saved: mut List[List[u8]] = []
    for _ in 0..2:
        let b: mut List[u8] = []
        saved.push(identity_bytes(b))
    saved
```

`identity_bytes` from C04 may return its input because its result names
the parameter. The returned buffer escapes through `saved`, so reusing
one allocation for both loop iterations would change observable aliasing.

**Verdict: works, with an optimizer representation obligation.** The
signature's result edge lets `ne.opt.escape` notice this escape even
though the buffer has no resource reach. The optimizer must keep that
edge in its ordinary object graph. An empty resource bitset cannot mean
that the object is unreachable or dead.

This positive control distinguishes a useful locality contract from
the erroneous claim that resource reach alone proves object death.
It also gives a small negative test for allocation reuse.

### C15. Distinct Evaluations At One Root Site

Original shape, approximating independent Rust allocations in a loop:

```rust
for index in 0..2 {
    let socket = open();
    if index == 0 { keep = Some(socket); } else { close(socket); }
}
```

```text
fn distinct_instances() -> Conn?:
    let keep: Conn? = .None
    for index in 0..2:
        c := open()
        if index == 0:
            keep = .Some(c)
        else:
            defer: c.close()
    keep
```

Only the second runtime handle is closed. The first is returned open.
`ne.root.site` assigns both evaluations one root, so the first appears
to be closed by the second evaluation's cleanup.

**Verdict: friction.** The finite site abstraction is conservative, but
it does not track handle identity exactly. This is broader than the
record's feeder example: the retained object is never fed back into the
operation on the next handle. Unrolling into two distinct sites avoids it.

### C16. A Child Is Cancelled Before Parent Cleanup

Original shape, approximating scoped cancellation in task libraries:

```rust
let answer = race(read(first), read(second)).await;
// the losing child has stopped before the scope releases either socket
```

```text
use std.task.race
fn read!(c: ^Conn) -> usize: # hypothetical syntax
    c.sent()

fn scoped_race!() -> usize:
    a := open()
    b := open()
    defer: a.close()
    defer: b.close()
    race!(read(a), read(b))
```

**Verdict: works.** `req.combinator.race-losers` stops losing children
before returning. Parent cancellation cleans children before parent
defer suites. The scalar result carries neither socket.

The successful proof depends on the cancellation contract, not solely on
the frame reach. Host operations must obey their abort/cleanup contracts.
Raw abandonment remains outside this guarantee, as already specified.

### C17. Early And Delegated Close Preserve Disposed Aliases

Original shape, approximating explicit Go closing:

```go
other := conn
conn.Close()
use(other) // runtime error
```

```text
fn finish(closing c: Conn) -> void: # hypothetical syntax
    defer: c.close()

fn disposed_alias() -> usize:
    c := open()
    other := c
    finish(c)
    other.sent()
```

The caller's invocation is outside a `defer`. `ne.close.early` therefore
does not establish a cleanup block there. The callee closes its parameter
at its own body exit, and the caller subsequently uses an alias.

**Verdict: works under the early-close decision, with a guarantee boundary.**
The design does not make handles generally safe from disposed use.
Delegating a close to a function containing `defer` also leaves aliases
usable statically. The unrestricted phrase “no alias of a value that a
defer closes outlives its block” needs this boundary stated explicitly.

No affine transfer, double-close prevention, or cleanup cardinality follows
from `closing`. Those guarantees should not be inferred from its spelling.

### C18. Phantom Metadata Is Not A Holder

Original shape, approximating a typed field descriptor:

```rust
struct Field<S, F> { metadata: usize, marker: PhantomData<fn(S) -> F> }
```

```text
data Tag[T]:
    number: usize

fn tag_number(t: Tag[Conn]) -> usize:
    t.number
```

`ne.data.generic` conditions the category on a field mentioning the
parameter. `Tag` stores no `T`, so it stores no handle. But
`ne.param.nonescapable` also describes any application with a NonEscapable
argument as NonEscapable as written. That classifies `Tag[Conn]` differently.

**Verdict: breaks, conflicting category definitions.** The actual
`Field[S, F]` has an explicit no-reach rule and stores neither `S` nor `F`.
`Field[Session, Conn]` needs a consistent classification too. Checking
argument syntax alone cannot substitute for instantiated holder structure.

The analogous field-descriptor API should generate metadata operations,
not create a resource root. No speculative new language feature is needed
to state the counterexample: hd already has these handles.

### C19. A Local Classifier Cannot Downcast Ordinary Values

Original shape, approximating Rust type inspection:

```rust
fn is_integer(value: &dyn Any) -> bool { value.is::<i32>() }
```

```text
use std.inspect.{Inspectable, downcast_val}

fn is_integer(value: ^dyn Inspectable) -> bool: # hypothetical syntax
    match downcast_val::[i32](value):
        .Some(_) => true
        .None => false
```

The parameter's abstract root is not static, even when the caller passes
an ordinary scalar that has no resource reach. `ne.erase.downcast`
rejects the function body independently of that caller.

**Verdict: friction, owner-decided Q6.** The classifier can take an
unmarked global parameter instead, accepting static/ordinary arguments.
It cannot accept arbitrary local inspectable values and merely inspect
their runtime variant. Comparing `TypeId` without recovering the value
can implement this boolean case, but does not recover a typed payload.

The cost of the blanket rule reaches beyond rare handle downcasts.
It changes reusable local inspection helpers. No narrower downcast rule
is proposed; the owner already rejected that alternative.

### C20. A Scalar-Only Closure Factory Gets A Local Root

Original shape, approximating a Go callback factory:

```go
func counter(n int) func() int { return func() int { return n } }
callbacks = append(callbacks, counter(3))
```

```text
let COUNTERS: mut List[fn() -> usize] = []

fn counter(n: usize) -> fn() -> usize:
    fn() -> usize: n

fn install(f: fn() -> usize) -> void:
    COUNTERS.push(f)

fn install_counter() -> void:
    install(counter(3))

fn static_counter(n: usize) -> ^{static} fn() -> usize: # hypothetical syntax
    fn() -> usize: n
```

The closure retains only an ordinary scalar. It has no handle whose
cleanup could constrain it. Nevertheless, a function result is lifetime-carrying,
and no tracked input makes `counter` elide to `^{}`.
`ne.call.fresh` mints a local root at `counter(3)`.

**Verdict: friction, under the stated fresh-result rules.** `install` is
global, so the minted root causes rejection. The explicitly static factory
works. An implementation that gives the first factory an empty reach must
explain why the signature's unconditional fresh-root rule is not applied.

This is an inference/default precision test, rather than a missing `^`
on a resource-using callback API. The record's M3 scalar factory works
locally, but its global callback use was not tested. Fresh resource identity
and a fresh ordinary closure environment need distinct accounts.

## Comparison With Other Languages

These comparisons identify missing obligations; they do not propose
adopting another language wholesale.

| Case | Relevant comparison | Consequence for hd |
| --- | --- | --- |
| C01–C03 | [Rust variance](https://doc.rust-lang.org/nomicon/subtyping.html) keeps mutable referent types invariant and treats interior-mutability containers separately. | An outer readonly or callable form cannot alone justify widening every lifetime used by recovered mutable storage. |
| C02, C08 | A [Rust future](https://doc.rust-lang.org/std/future/trait.Future.html) has an explicit associated output type; callable output types likewise stand apart from the callable's own retention. | Frame/environment validity and output origin require separate facts, even when one syntax summarizes common cases. |
| C04, C09 | [Kotlin locality notes](https://github.com/Kotlin/KEEP/blob/main/notes/0007-local-lifetimes.md) treat local parameters as non-global within typing and examine callback argument escape separately from callback escape. | A locally used function value does not automatically use its arguments locally; ordinary provenance must survive composition. |
| C05, C06 | [Go method values](https://go.dev/ref/spec#Method_values) fix the receiver when created; [defer](https://go.dev/ref/spec#Defer_statements) evaluates direct call arguments when registered. Deferred closures can instead observe changed variables. | hd must distinguish bound references, captured lexical storage, and the order of operations during cleanup. |
| C10 | [Go SQL `Conn.Close`](https://pkg.go.dev/database/sql#Conn.Close) releases a lease back to its pool. | Retaining a pool is not authority to close that pool. |
| C11 | Rust's `Option<Pair<'a, 'b>>` retains the pair's distinct type arguments. | hd's slot union on wrapping is a deliberate expressiveness loss, not a universal property of optionals. |
| C13 | [Rust iterators](https://doc.rust-lang.org/std/iter/) are lazy, and [stateful mapping](https://doc.rust-lang.org/std/iter/struct.Map.html) can expose traversal order. | Fusion needs an observational-equivalence argument, including aliases, state, and callback invocation order. |
| RTTI boundary | [Rust `Any`](https://doc.rust-lang.org/std/any/trait.Any.html) requires the underlying type to be `'static`. An ordinary reference to such a value may itself be short-lived. | hd instead tests the erased value's reach. Calling this “the whole Rust rule” hides a distinction between type contents and reference duration. |

## Problems, Ranked

Ranks combine breadth, silent failure risk, and confidence. Three variance
cases count as one structural problem. Accepted precision losses are
ranked below guarantee and contract failures.

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | NE-S1 | Widening followed by recovered invariant write authority | Critical soundness obligation | C01, C02, C03 |
| 2 | NE-S2 | Ordinary object provenance disappears at typed boundaries | Critical contract/optimizer obligation | C04, C14 |
| 3 | NE-S3 | Cleanup needs execution states, not just exit snapshots | Critical target-tracking gap | C06; related C07 |
| 4 | NE-S4 | Closing authority through function and method values is unspecified | High lowering gap | C05 |
| 5 | NE-S5 | A frame's retention cannot specify its output origin | High composability/precision gap | C08 |
| 6 | NE-S6 | Function-type provider elision conflicts with function elision | High signature inconsistency | C09 |
| 7 | NE-S7 | Dependent results do not prove exclusive iterator reachability | High optimizer overclaim | C13 |
| 8 | NE-S8 | Closing authority is conflated with all retained dependencies | Significant API restriction | C10 |
| 9 | NE-S9 | Cleanup guarantee has several non-equivalent formulations | Significant contract ambiguity | C07, C17 |
| 10 | NE-S10 | Generic category definitions disagree on phantom arguments | Specification inconsistency | C18 |
| 11 | NE-S11 | Finite reach approximations lose useful independence | Accepted or conservative precision cost | C11, C12, C15, C20 |

Every variance, locality, closing, elision, or frame-protocol candidate
belongs to the language tier. An ordinary field-level cleanup helper
belongs to the stdlib tier if it requires no compiler knowledge of its name.
No candidate changes the CLI tier.

### NE-S1. Recovered Write Authority

**Effect.** C01–C03 are the same attack through traits, closures, and
nested containers. The static-only downcast restriction addresses X6's
specific route, but does not establish a general variance theorem.

**Candidates, as owner questions.** A widened outer view must not change
the invariant origin of mutable storage recovered through that view.
Should variance follow the full exposed authority surface, or should the
checker retain an invariant origin separately from a widened use bound?

| Candidate | Syntax | Semantic changes | Intrinsics | Library additions | Cost and consequence |
| --- | --- | --- | --- | --- | --- |
| Extend the existing polarity analysis across getters, callable results, and nested mutable elements | 0 | 1 general variance rule | 0 | 0 | Reuses current variance; may conservatively reject otherwise useful views |
| Preserve invariant storage origins while widening a value's use bound | 0 assumed | 1 origin distinction, plus propagation rules | 0 | 0 | More representation and proof work; keeps useful covariance where origins remain fixed |

The first has fewer mechanisms. The evidence does not yet prove that it
accepts every required higher-order case. Neither candidate is chosen.

### NE-S2. Ordinary Provenance

**Effect.** C04 distinguishes resource lifetime from an ordinary object's
non-retention contract. C14 shows that optimizer escape edges remain
necessary when resource reaches are empty.

**Candidates, as owner questions.** Should the checker retain ordinary
parameter provenance through every value boundary, erasing only resource
constraints at callers? Or should ordinary locality be checked by an
explicit separate escape graph while using the same `^` syntax?

| Candidate | Changes | Costliest kind |
| --- | --- | --- |
| Preserve ordinary provenance with the existing root-flow machinery | 0 syntax, 1 general propagation rule, 0 intrinsics, 0 library additions | Semantic change |
| Use ordinary object escape edges and resource reaches as two analyses sharing the annotations | 0 syntax, 1 separate analysis contract, 0 intrinsics, 0 library additions | Semantic change |

Both must reject `hidden_retention`, preserve C14's escape, and allow
returning an unrelated freshly computed ordinary value.

### NE-S3. Cleanup Execution States

**Effect.** C06 changes the actual closed root after the body has finished.
C07 requires a decision about observations within the suite itself.
Whole-scope unions avoid some holes by rejecting the record's precision claims.

**Candidates, as owner questions.** Should cleanup suites be part of the
ordinary CFG in their actual LIFO order? Or should captured cleanup targets
be restricted so their values cannot change during cleanup execution?

| Candidate | Changes | Costliest kind |
| --- | --- | --- |
| Analyze the ordered cleanup CFG, including saved results and captured storage | 0 syntax, 1 general execution-state rule, 0 intrinsics, 0 library additions | Semantic change |
| Restrict target-changing cleanup interactions | 0 syntax, at least 1 restriction, 0 intrinsics, 0 library additions | Semantic exception/restriction |

The first reuses the existing execution semantics. The second reduces
checker work by rejecting valid patterns; its precision cost needs measuring.

### NE-S4. Closing Function Values

**Effect.** C05 rebuilds X4 using already specified method references.
Unbound references can additionally lose an input's closing mode when
converted to an ordinary function type.

**Candidates, as owner questions.** Should every generated bound method
reference undergo the capture-closing check, and unbound closing references
be rejected when their mode cannot be represented? Or should function
types preserve a closing input mode explicitly?

| Candidate | Changes | Costliest kind |
| --- | --- | --- |
| Apply existing capture rules to generated references and reject mode-erasing conversions | 0 syntax, 1 general preservation rule, 0 intrinsics, 0 library additions | Semantic change |
| Preserve closing modes in function types | Possible grammar extension, 1 mode-preservation rule, 0 intrinsics, 0 library additions | Syntax if the extension is needed |

The first is cheaper under the owner's order. This asks how existing
closing authority survives abstraction, not whether to add a new cleanup feature.

### NE-S5. Frame Retention And Output

**Effect.** C08 has a safe subset-dependent result and a larger frame
retention set. The same erased `Suspend[Conn]` shape can represent a frame
that returns either input. Storage cannot discard that distinction while
still claiming exact results.

**Candidates, as owner questions.** Should generated frames expose separate
retention and output slots through the existing slot machinery? Or should
the erased protocol explicitly return the entire frame reach and document
the loss, while direct bang calls keep the declared precision?

| Candidate | Changes | Costliest kind |
| --- | --- | --- |
| Reuse declared slots for frame retention and completed output | Slot/type surface may need syntax; 1 representation rule plus combinator rules; sealed compiler protocol changes | Syntax if required, otherwise compiler-known protocol |
| Adopt an explicit conservative erased-frame output rule | 0 syntax, 1 general erasure rule, 0 new intrinsic names, 0 library additions | Semantic change |

The second conflicts with a blanket claim of exact subset tracking.
The owner would need to bound that requirement explicitly; it cannot be
quietly accepted as an implementation detail.

### NE-S6. Provider Elision

**Effect.** C09 gives the same callback two different inferred bounds.
Row-polymorphic task helpers compound the mismatch. Provider substitution
cannot be recovered from the closure's empty environment.

**Candidates, as owner questions.** Should function-type elision include
NonEscapable keys and row parameters exactly as function elision does?
That uses the decided rule rather than adding another callback default.
It requires 0 syntax changes, 1 elision clarification, 0 intrinsics, and
0 library additions; ambiguous callback outputs then need written bounds.

### NE-S7. Optimizer Proof Boundaries

**Effect.** C13 refutes the inference from “the result retains the source”
to “only the result retains the source.” C14 distinguishes absence of
resource roots from absence of ordinary object escape.

**Candidates, as owner questions.** Should `LocalParams` be documented only
as a checked bound on callee retention, with caller alias/liveness and
effect proofs still required for each optimization? Or should each
optimization claim include its additional preconditions explicitly?

Both are documentation of existing proof obligations: 0 syntax, 0 new
semantic exceptions, 0 intrinsics, and 0 library additions. No language
feature is needed to stop overstating what the annotations establish.

### NE-S8. Closing A Dependency Is Not Closing A Resource

**Effect.** C10 excludes a resource wrapper that retains borrowed context.
The all-roots rule is owner-decided; this is fresh evidence of its cost.

**Candidates, as owner questions.** Should the whole-reach closing rule
remain, with users explicitly closing only owned fields? Or should a closer
describe a subset of roots it ends, separately from roots it merely needs?

| Candidate | Changes | Costliest kind |
| --- | --- | --- |
| Keep the rule and expose field-level cleanup APIs | Ordinary library APIs; 0 language changes | Library addition |
| Distinguish closing targets from retained context | May need a closing-footprint syntax; at least 1 semantic distinction | Syntax if required |

The first is cheaper but does not preserve an encapsulated lease API.
The second needs a new owner decision justified by that use case.

### NE-S9. Guarantee Wording

**Effect.** C07 and C17 separate three promises: no alias past the owning
block, no use after a deferred closing statement, and no disposed use
across delegated cleanup. The record currently uses wording from more
than one promise while explicitly excluding early-close checking.

**Candidates, as owner questions.** Should the promise name only direct
registered cleanup blocks and their exit liveness? Or should it also cover
reads after a deferred close within cleanup execution?

The first is a wording clarification with no new mechanism. The second
needs the C07 execution check, reusing NE-S3. Neither implies a general
runtime-state or exactly-once-close guarantee.

### NE-S10. Category Calculation

**Effect.** C18 shows the category cannot be calculated both from every
argument's spelling and only from fields that retain those arguments.
The existing field-handle exception makes the discrepancy observable.

**Candidates, as owner questions.** Should “NonEscapable as written” mean
the instantiated structural category, computed from retained fields,
instead of an unconditional test of all type arguments?

This is 0 syntax changes, 1 definition clarification, 0 intrinsics, and
0 library additions. It aligns with the owner's field-based Q2 rule.
Recursive declarations need a termination/fixed-point account too.

### NE-S11. Precision Boundaries

**Effect.** C11 loses slot independence through wrapping; C12 retains
historical roots after clearing; C15 merges distinct dynamic instances.
C20 creates a caller root for an environment without any resource capture.
These are different causes and should not all be called inference precision.

**Candidates, as owner questions.** Should the accepted limits be stated
as explicit boundaries of the exactness requirement, with a representative
real-program case for each? Or should exactness mean only exact substitution
of the published signature after those abstractions have already lost information?

This is a requirement clarification, not a proposed strong-update feature.
Q9 and Q15 remain decided. No migration work is included.

## Evaluation By Dimension

| Dimension | Assessment | Evidence needed next |
| --- | --- | --- |
| Guarantee clarity | Several inequivalent guarantees are currently conflated | Decide the direct/delegated and intra-cleanup boundaries in C07/C17 |
| Soundness | Three write-recovery routes need a general variance proof; cleanup/reference lowering is incomplete | Show rejection derivations for C01–C06 using only exported signatures |
| Precision | Losses are broader than isolated corner cases | Measure wrappers, reusable scratch containers, and repeated-site allocations against C11/C12/C15 |
| Practical expressiveness | Straight resource scopes work; context-dependent cleanup wrappers are awkward | Try encapsulated leases, transactions with borrowed context, and stateful callback factories |
| Composability | Method values, erased frames, and provider callbacks expose missing boundaries | Specify C05/C08/C09 before treating individual successful examples as evidence of composition |
| Modular contracts | Written signatures are a good foundation, but some necessary origins/effects are absent | Define what a mutable getter, closing reference, and stored frame exports |
| Agent writability and human readability | The short common mark hides substantial semantic distinctions | Test diagnoses on these minimized cases after a checker exists |
| Conceptual cost | Category, resource reach, ordinary provenance, closing authority, and optimizer escape are not interchangeable | Make each fact's role explicit; reuse one notation without claiming one bitset proves them all |
| Compiler feasibility and cost | Finite monotone constraints are implementable; the asserted linear complexity is unproved | Bound fixed-point work, cleanup CFG construction, recursive category analysis, and alias/origin propagation |
| Optimization validity and payoff | Non-retention summaries help, but several claimed licenses are too strong | Preserve ordinary escape graphs; prove caller alias and effect conditions for fusion/reuse |
| Specification readiness | Not ready to apply as a complete system | Resolve conflicting definitions and supply semantic rejection/acceptance derivations, not only parse results |

### Checker Cost Needs A Real Bound

A one-word reach makes one union cheap. It does not prove that each edge
is visited once. In a monotone worklist, a destination can acquire roots
incrementally and cause repeated propagation. SCC scheduling or delta
propagation can help, but their bounds must be stated.

Backward liveness on cyclic CFGs also needs a fixed point. Calling it
“one backward bitvector pass” is not an algorithm for loops. Cleanup suites
add ordered execution edges, saved operands, and possible storage updates.
Captured-cell union and ordinary provenance introduce further variables.

The `O((N + E) · ceil(R/64))` claim therefore lacks a derivation.
It may describe a common-case estimate, but it is not established as a
worst-case bound by the listed algorithm. No benchmark was run here:
the proposed checker does not exist.

### Existing Optimizer Support Is Uneven

The reviewed `crates/hd_opt/src/passes.rs` registers `escape` and
`scalar_replace` with `Instance(not_yet)`. `value_flow`, `closure_spec`,
and `devirt_known` have implemented entry points. This is a concrete limit
on the record's description of optimizations that currently prove these facts.

The language claim and the implementation claim should be separated.
A signature can support future escape analysis without an existing escape
pass already establishing the same fact. No performance benefit has been
measured by this review.

### Diagnostics And Agent Evidence

No lifetime diagnostic has been emitted by a checker here. The record's
example comments are proposed messages, not evidence that an agent can
repair these programs. Parse rejection at `^` tests the parser's current
surface, not the proposed diagnostic experience.

Useful future diagnostic tests should expose the complete causal path:
the widened value, recovered mutable origin, actual store, and later use.
Cleanup diagnostics should identify the actual target at the closing
statement, including which suite changed it. A fix-it must not merely
add an annotation that changes the API's intended guarantee.

Two annotations deserve explicit prose rather than shorthand slogans.
Bare `^` is conditionally retained when a result names it; “never kept”
must be read together with the entire signature. `closing` grants
authority but promises neither single close nor consumed aliases.

C19 also makes the RTTI restriction visible in ordinary inspection code.
Its workaround is straightforward for boolean type tests, but not for
typed local payload recovery. This cost is accepted, rather than counted
as a new soundness defect.

### Ordinary Forwarding Is Not Optimizer-Only

A parameter `text: ^string` has a root in its body. Forwarding it to
an ordinary helper with an unmarked `text: string` parameter fails
`ne.body.global-arg`, even if the helper only computes its length.
Marking the helper local then requires a local `string.len` contract too.

This is consistent with parameter locality, but contradicts the record's
claim that ordinary-type marks affect no checker result. It concerns API
composition, not migration. Erasing ordinary roots to avoid these checks
would instead admit C04's forbidden retention.

### Additional Specification Boundaries

- `Fn[Args, O, $R]` is currently identical to function syntax. Named
  dependent inputs and closing modes need an account in that constructor,
  including tuple/rest input forms.
- Slot-preserving reflection cannot rely on `Field[S, F]` alone to encode
  an instance's slot paths unless the compiler exports the member relation.
  The record explicitly gives `h.get(x)` special knowledge; generated
  templates and `build` need the same precise substitution.
- Generic category computation must handle phantom arguments, nested
  instantiated holders, associated type projections, and recursive fields.
  “One pass over the declaration” does not settle recursive dependencies.
- A fresh result introduces a root at the call even when it is an ordinary
  closure capturing only scalars. The checker should state when empty
  retention, fresh resource identity, and caller-created locality tokens
  differ, rather than using `{}` for all three without an account.

These are coverage gaps to resolve after the core obligations above.
They do not authorize adding any advanced feature.

## Parse Log

All 22 report blocks were parsed unchanged. Eight parse successfully.
Fourteen are rejected at proposed lifetime/closing syntax. All 22
mechanical projections parse successfully. This is syntax evidence only.

The projection removes lifetime prefixes, slot declarations, and `closing`
from code before comments. It preserves line breaks and all executable
body structure. It does not claim semantic equivalence or type correctness.

The same audit covers the design record's 61 `hd` blocks. Eight raw
blocks parse; 55 projections parse. Six projected fragments still contain
signature-only free functions or named callback inputs outside the current
grammar. These are schematic/unsupported fragments, not additional lifetime findings.

### Report Blocks

| Block | Case | Raw parser result, first diagnostic | Projection |
| --- | --- | --- | --- |
| 1 | Prelude | `error: FILE:3:13: syntax-error: expected a parameter name, found '^'` | clean |
| 2 | C01 | `error: FILE:2:14: syntax-error: expected a parameter name, found '^'` | clean |
| 3 | C02 | `error: FILE:2:10: syntax-error: expected a type, found '^'` | clean |
| 4 | C03 | `error: FILE:1:26: syntax-error: expected a type, found '^'` | clean |
| 5 | C03 caller | clean | clean |
| 6 | C04 | `error: FILE:3:22: syntax-error: expected a type, found '^'` | clean |
| 7 | C05 | `error: FILE:1:19: syntax-error: expected a type, found '^'` | clean |
| 8 | C06 | clean | clean |
| 9 | C07 | clean | clean |
| 10 | C08 | `error: FILE:3:18: syntax-error: expected a type, found '^'` | clean |
| 11 | C09 | `error: FILE:2:12: syntax-error: expected a parameter name, found '^'` | clean |
| 12 | C10 | `error: FILE:7:24: syntax-error: expected ':', found 'self'` | clean |
| 13 | C11 | `error: FILE:1:11: syntax-error: expected a generic parameter name, found '^'` | clean |
| 14 | C12 | clean | clean |
| 15 | C13 | clean | clean |
| 16 | C14 | clean | clean |
| 17 | C15 | clean | clean |
| 18 | C16 | `error: FILE:2:13: syntax-error: expected a type, found '^'` | clean |
| 19 | C17 | `error: FILE:1:19: syntax-error: expected ':', found 'c'` | clean |
| 20 | C18 | clean | clean |
| 21 | C19 | `error: FILE:3:22: syntax-error: expected a type, found '^'` | clean |
| 22 | C20 | `error: FILE:12:32: syntax-error: expected a type, found '^'` | clean |

### Design Record Blocks

Block numbers count only `hd` fences in source order. The notation grammar
is an EBNF fragment, not an hd program, and is not included. Diagnostic
paths are normalized to `FILE`; the first diagnostic is otherwise verbatim.

| Block | Section | Raw | Projection, first diagnostic when rejected |
| --- | --- | --- | --- |
| 1 | 2. Core Model | `error: FILE:1:14: syntax-error: expected a type, found '^'` | clean |
| 2 | Outlives | clean | clean |
| 3 | 3. Notation | `error: FILE:1:14: syntax-error: expected a type, found '^'` | `error: FILE:2:1: syntax-error: expected ':' and a function body, found a line ending` |
| 4 | 4. Signatures And Elision | `error: FILE:1:15: syntax-error: expected a type, found '^'` | `error: FILE:2:1: syntax-error: expected ':' and a function body, found a line ending` |
| 5 | 4.1 Parameter Marks | `error: FILE:4:21: syntax-error: expected a type, found '^'` | clean |
| 6 | 5.1 The Category | clean | clean |
| 7 | 5.2 Declared Slots | `error: FILE:1:14: syntax-error: expected a generic parameter name, found '^'` | clean |
| 8 | 6. Closing | `error: FILE:2:22: syntax-error: expected ':', found 'self'` | clean |
| 9 | 6. Closing | `error: FILE:1:20: syntax-error: expected ':', found 'c'` | clean |
| 10 | 8.1 Generic Substitution | `error: FILE:2:13: syntax-error: expected a parameter name, found '^'` | clean |
| 11 | 8.2 Function Types | `error: FILE:1:18: syntax-error: expected a type, found '^'` | `error: FILE:1:34: syntax-error: expected ')', found ':'` |
| 12 | 12.6 What The Optimizer Gets | `error: FILE:1:14: syntax-error: expected a type, found '^'` | clean |
| 13 | 13. Stress-Test Suite | `error: FILE:4:14: syntax-error: expected a parameter name, found '^'` | `error: FILE:17:1: syntax-error: expected ':' and a function body, found a line ending` |
| 14 | 13. Stress-Test Suite | `error: FILE:2:13: syntax-error: expected a parameter name, found '^'` | clean |
| 15 | R1. `longest` | `error: FILE:1:14: syntax-error: expected a type, found '^'` | clean |
| 16 | R2. A Dangling Reference | clean | clean |
| 17 | R3. `ImportantExcerpt` | `error: FILE:6:15: syntax-error: expected a type, found '^'` | clean |
| 18 | R4. `Parser<'c, 's>` | `error: FILE:7:13: syntax-error: expected a parameter name, found '^'` | clean |
| 19 | R5. A Slice Iterator | `error: FILE:8:13: syntax-error: expected a parameter name, found '^'` | clean |
| 20 | R6. Evil Feeder | `error: FILE:4:18: syntax-error: expected a type, found '^'` | clean |
| 21 | R7. `thread::spawn` Versus Structured Children | `error: FILE:3:16: syntax-error: expected a type, found '^'` | clean |
| 22 | R8. Reassignment | `error: FILE:1:12: syntax-error: expected a type, found '^'` | clean |
| 23 | R9. `accept` | clean | clean |
| 24 | R10. A Borrow Across A Suspension | `error: FILE:3:13: syntax-error: expected a type, found '^'` | clean |
| 25 | R11. `std::thread::scope` | `error: FILE:3:18: syntax-error: expected a type, found '^'` | clean |
| 26 | R12. A Lifetime-Generic Method | `error: FILE:2:14: syntax-error: expected a parameter name, found '^'` | clean |
| 27 | R13. A `HashMap` Of Handles | `error: FILE:1:17: syntax-error: expected a type, found '^'` | clean |
| 28 | R14. `Option<&T>` With `?` | `error: FILE:1:21: syntax-error: expected a type, found '^'` | clean |
| 29 | R15. A Self-Referential Struct | `error: FILE:12:14: syntax-error: expected a type, found '^'` | clean |
| 30 | S1. A View Over A Handle | `error: FILE:2:14: syntax-error: expected a parameter name, found '^'` | clean |
| 31 | S2. A Mutating Store | `error: FILE:7:14: syntax-error: expected a parameter name, found '^'` | clean |
| 32 | S3. A Cleanup Wrapper | `error: FILE:2:25: syntax-error: expected ':', found 'self'` | clean |
| 33 | S4. `MutableSpan` And `@_lifetime(&self)` | `error: FILE:8:15: syntax-error: expected a parameter name, found '^'` | clean |
| 34 | S5. Conditional `Escapable` | `error: FILE:19:21: syntax-error: expected a type, found '^'` | clean |
| 35 | S6. A Non-Escaping Closure Called Later In The Call | `error: FILE:1:33: syntax-error: expected a type, found '^'` | clean |
| 36 | M3. Capturing Versus Escaping Closures | `error: FILE:1:15: syntax-error: expected a type, found '^'` | clean |
| 37 | T1. Independent Slots | `error: FILE:1:17: syntax-error: expected a type, found '^'` | clean |
| 38 | T2. Swapping Slots | `error: FILE:1:15: syntax-error: expected a type, found '^'` | clean |
| 39 | D1. A Handle In A `Result` Through `?` | `error: FILE:6:14: syntax-error: expected a type, found '^'` | clean |
| 40 | D2. A Cold Suspension In A Field | `error: FILE:4:12: syntax-error: expected a type, found '^'` | clean |
| 41 | D3. `race!` And Cancellation With A `defer` | `error: FILE:3:16: syntax-error: expected a type, found '^'` | clean |
| 42 | D4. A NonEscapable Provider | `error: FILE:2:12: syntax-error: expected a parameter name, found '^'` | `error: FILE:10:1: syntax-error: expected ':' and a function body, found a line ending` |
| 43 | D5. `Map[string, Conn]` In A Holder | `error: FILE:7:12: syntax-error: expected a parameter name, found '^'` | clean |
| 44 | D6. A `dyn Conn` List Mixing Lifetimes | `error: FILE:6:14: syntax-error: expected a parameter name, found '^'` | clean |
| 45 | D7. Recursive Data Of Handles | `error: FILE:7:10: syntax-error: expected a generic parameter name, found '^'` | clean |
| 46 | D8. A Callback Registry | `error: FILE:8:14: syntax-error: expected a type, found '^'` | clean |
| 47 | N1. An Unmarked NonEscapable Parameter | `error: FILE:4:12: syntax-error: expected a type, found '^'` | clean |
| 48 | N2. Generic And `dyn` Opt-In | `error: FILE:2:18: syntax-error: expected a type, found '^'` | clean |
| 49 | N3. General Locality | `error: FILE:3:17: syntax-error: expected a type, found '^'` | clean |
| 50 | N4. Function Types | `error: FILE:1:18: syntax-error: expected a type, found '^'` | `error: FILE:1:34: syntax-error: expected ')', found ':'` |
| 51 | N5. `closing` Implies A Local Parameter | `error: FILE:1:17: syntax-error: expected ':', found 'c'` | clean |
| 52 | N6. A Concrete Holder Declares The Category | clean | clean |
| 53 | X1. A Late Cleanup Block | `error: FILE:1:14: syntax-error: expected a type, found '^'` | clean |
| 54 | X2. Defer Order | `error: FILE:1:14: syntax-error: expected a type, found '^'` | clean |
| 55 | X3. A `defer` Reads The Binding At Exit | clean | clean |
| 56 | X4. A Cleanup Closure | clean | clean |
| 57 | X5. Closing A Borrowed Handle Through A Local | `error: FILE:1:19: syntax-error: expected a type, found '^'` | clean |
| 58 | X6. Widening Through A Downcast | `error: FILE:8:15: syntax-error: expected a type, found '^'` | clean |
| 59 | X7. A Suspending Close In A `defer` | `error: FILE:2:23: syntax-error: expected ':', found 'self'` | clean |
| 60 | Appendix A. Why A Category And Lifetimes | `error: FILE:1:41: syntax-error: expected a type, found '^'` | clean |
| 61 | Appendix A. Why A Category And Lifetimes | clean | clean |

### Repository Validation

`bash spec/check.sh` passed. `pnpm run website:build` passed, producing
79 pages. Validation used an isolated checkout at `origin/main`
(`cf3002b3`) plus this report and its README entry. The existing generated
playground Wasm asset was copied into that checkout for the website build.

The user’s active runtime edits were excluded. No lifetime checker exists,
so no case is represented as having passed lifetime type checking.
