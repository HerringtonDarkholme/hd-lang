# NonEscapable Types And Lifetimes: Adversarial Review 3

Status: design review, 2026-10-15; nothing here is accepted behavior.
This changes no decision, design record, spec text, or compiler code.
It reviews [round 6 of the design](NONESCAPE_LIFETIME_DESIGN.md), commit
`6fcce095962d2d8d0f9aea1584e85d3c1cbd3782`, against
[review 2](NONESCAPE_LIFETIME_STRESS_TEST_2.md).

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
- [Repository Validation](#repository-validation)

## Surface Being Tested

The review includes every binding round-6 decision, especially NE-S1b's
independent generic components, NE-S4b's environment stores, and NE-S5c's
callable weakening. It also checks the explicitly unaccepted round-6
agent details as proposed rules. A conflict with one of those details
does not invalidate the owner's underlying decision.

| Assumption | Active rules |
| --- | --- |
| Every marked parameter has provenance, including ordinary buffers | `ne.param.any-type`, `ne.body.local` |
| Result-elision inputs exclude concrete types that carry nothing | `ne.sig.carrying`, `ne.sig.input` |
| A container's own root differs from its component roots | `ne.data.component-locality` |
| Prefix bounds cover every component and slot | `ne.syntax.one-reach`, `ne.data.component-bound` |
| A name without a path denotes the union of its reaches | `ne.data.whole`, `ne.generic.abstract` |
| Mutable components cannot widen | `ne.var.surface`, `ne.var.mut-data`, `ne.var.generic` |
| Stores through a callable target every captured object and binding | `ne.fntype.env-store` |
| A direct cold frame retains every argument and its called environment | `ne.suspend.cold` |
| Weakening puts every tracked input in `run` | `ne.suspend.weaken` |
| Ordinary-key providers must fit static | `ne.provider.ordinary` |
| Fresh resources may escape, and closing is outside the guarantee | OQ1; [parked closing](NONESCAPE_LIFETIME_DESIGN.md#parked-closing-and-cleanup) |

The language-tier dependencies are
[type identity and variance](../spec/lang/04-type-system.md),
[iteration protocols](../spec/lang/06-control-flow.md#iteration-protocols),
[function constructors](../spec/lang/07-functions.md#function-type-constructors),
[method references](../spec/lang/07-functions.md#method-references),
[captures](../spec/lang/07-functions.md#captures),
[data](../spec/lang/08-data-and-enums.md), and
[suspension](../spec/lang/11-requirements-and-suspension.md).
The stdlib dependencies are
[collections](../spec/std/collections.md),
[iterators](../spec/std/iter.md),
[optionals](../spec/std/option.md), and
[tasks](../spec/std/task.md).
The [actual iterator representation](../lib/std/iter.hd) is evidence in R42.
No CLI design change is needed.

The examples approximate library shapes. `Conn` and `open` use this
parser prelude; their behavior follows the design's simplified socket
model. Actual networking errors and closing are irrelevant to these cases.

```text
trait NonEscapable
trait Socket < NonEscapable:
    fn sent(^self) -> usize # hypothetical syntax
type Conn = dyn Socket
fn open() -> Conn: panic("parser stub")
```

`List.push` and `List.get` use the exact proposed signatures:
`fn push(^mut self, v: ^{self.T} T)` and
`fn get(^self, index: usize) -> ^{self.T} T?`.
No body inference repairs an exported contract.

## Method

Twelve cases target the replacements for the previous fixes, rather
than repeating the same rejected programs. Three positive controls
test providers, frame outputs, and scoped callbacks. Migration remains
excluded, as the owner requested.

| Verdict | Meaning |
| --- | --- |
| Works | A consistent derivation supports the case, including its intended rejection where applicable |
| Friction | Sound rejection or precision loss blocks useful code |
| Breaks | Explicit rules or claimed examples conflict, or a required translation is absent |

Breaks does not mean an implemented compiler accepts an exploit. The
lifetime checker does not exist. This report distinguishes a direct
rule conflict, a missing representation, and a conservative rejection.
Problems shared by several cases are counted once.

All hd examples are parsed raw and through a syntax-only projection.
Only proposed lifetime syntax and names in function types are removed;
the executable operations stay. Parser acceptance proves syntax only.
It proves neither proposed typing nor resource safety.

For costs, a language semantic rule includes a changed rule or exception.
New syntax is more expensive than semantic changes, then compiler-known
intrinsics, then ordinary core-library additions. The candidate tables
list costs and leave every choice to the owner.

## Summary

Round 6 repairs the old minimized attacks with meaningful rules.
The remaining trouble is the interface between independent reach
components and contracts that still export only one set. One new cold
constructor translation also drops ordinary parameter provenance.

| Case | Library shape | Verdict | Finding |
| --- | --- | --- | --- |
| R34 | Rust future constructor over an ordinary buffer | Breaks: direct rule conflict | Weakening retains elision inputs instead of all marked arguments |
| R35 | Rust `identity` | Friction | A single result bound flattens a mutable container's independent reaches |
| R36 | Rust struct containing a mutable vector and a handle | Breaks: claimed workaround fails | A declared slot around a generic field still flattens that field |
| R37 | Rust vector lookup of a two-lifetime record | Breaks: rules conflict | `get`'s written bound conflicts with preservation during extraction |
| R38 | Rust vector of independent mutable cells | Friction | Sharing an element component still requires invariant aggregation checks |
| R39 | Go bound getter / Python partial application | Breaks: missing translation | Binding removes `self`, but component-dependent results need its path |
| R40 | Go callback storing while reading metadata | Friction | The environment store permission targets unrelated captures too |
| R41 | hd `Fn` constructor, compared with Rust callable bounds | Breaks: unfinished identity | Constructor arguments do not encode modes and dependent binders |
| R42 | Rust `iter::from_fn`, actual hd iterator | Breaks: proposed std contract fails | A generated item comes from the stored closure, not an independent payload component |
| R43 | Rust future output composition / hd `all!` | Works | Direct frame output slots survive local tuple projection |
| R44 | Go context service | Works | Ordinary scoped providers reject; tracked keys preserve provenance |
| R45 | Rust scoped callback | Works | Fresh construction does not bypass checking the callback's parameter |

The most urgent correction is R34. The largest design question is
R35–R37: preserving a reach tree locally does not preserve it across a
signature that replaces the tree with one set. R39 and R42 show that
callable outputs also need a place in that account.

## Earlier Findings Rechecked

| Review-2 ID and cases | Round-6 status | Remaining boundary |
| --- | --- | --- |
| NE-S6b, R21 | Fixed as chosen: ordinary provider must fit static | R44 confirms both ordinary rejection and tracked-key behavior |
| NE-S1b, R22–R23 | Local tuple/optional attacks reject; written tuple return rejects | The advertised slot-bearing fallback is itself too coarse: R36; generic reads: R37 |
| NE-S4b, R24/R28 | Direct setter translation and captured-cell check are supplied | Component result paths and unrelated capture destinations: R39–R40 |
| NE-S5b, R25 | Fixed: the called closure enters cold `run` | R34 is a different omission in weakening |
| NE-S5c, R26 | Explicit frame slots repair the shown two-handle constructor | Ordinary marked inputs still disappear, R34; constructor identity remains unfinished, R41 |
| NE-S1c, R29 | Fixed: fresh mutable output no longer makes an unrelated environment invariant | This review does not reopen the choice |
| NE-S11b, R30 | Guide note added for accepted L9 | L9 remains an intentional cost, not a new hole |
| NE-S7b, R33 | Fixed: optimizer reads full result/store summary | No new optimizer exploit or measured payoff established |
| Prior controls R27/R31/R32 | Retention rules remain useful; fresh escape stays intentional | R43–R45 exercise repaired paths |

NE-S3, NE-S8, NE-S9, and the closing part of NE-S4 remain parked.
Their old disposal claims are not active obligations. NE-S10's category
calculation and the older accepted limits are not silently reopened.

## Cases

### R34. Weakening Drops An Ordinary Buffer

Original shape, approximating a Rust future factory:

```rust
async fn inspect(bytes: &[u8]) { let _ = bytes.len(); }
fn make(bytes: &[u8]) -> impl Future<Output = ()> + '_ { inspect(bytes) }
```

```text
data BufferHeader:
    length: usize

fn inspect!(bytes: ^BufferHeader) -> void: # hypothetical syntax
    _ := bytes.length

fn direct(bytes: ^BufferHeader) -> ^{} mut Suspend[void]: # hypothetical syntax
    inspect(bytes)

fn lowered(bytes: ^BufferHeader) -> ^{} mut Suspend[void]: # hypothetical syntax
    let make: fn(b: ^BufferHeader) -> ^{} mut Suspend[void] = inspect # hypothetical syntax
    make(bytes)
```

**Generated behavior.** Both paths create a cold frame that holds the
same ordinary heap object. The example keeps only its length to make
the carrying test unambiguous. Neither path drives or discards the frame
before returning it.

**Breaks: direct rule conflict.** `ne.param.any-type` gives `bytes` a
root. The direct cold-call rule puts that argument in `run`, so
`direct` must reject. `BufferHeader` stores only a scalar; it is neither
a root type nor a holder and has no type parameters. Its marked
parameter is therefore excluded from `ne.sig.input`. The scalar-list
case follows the same distinction in `ne.param.ordinary-result`.

But `ne.suspend.weaken` constructs `run` from tracked inputs. Its rule
therefore produces empty `run` and `out`, matching the single empty prefix on `make`. That permits
`lowered` to claim no borrowed input while returning its frame.
An extra conversion check could reject this; it would contradict the
stated weakening translation rather than validate it. The rule also
glosses tracked inputs as "its `^` inputs"; reading that phrase as
*all* marked inputs repairs the case, but conflicts with the defined
term and needs to be stated explicitly.

The problem is not that ordinary buffers need the NonEscapable category.
They already have provenance by NE-S2. Retention inputs and elision
inputs are different sets. The same distinction applies to any explicitly
marked ordinary parameter omitted from elision.

### R35. Generic Identity Flattens Independent Reaches

Original shape:

```rust
fn identity<T>(value: T) -> T { value }
```

```text
fn same[T](value: ^T) -> ^{value} T: # hypothetical syntax
    value

fn use_same(xs: ^mut List[Conn], c: ^{xs.T} Conn) -> void: # hypothetical syntax
    let back: mut List[Conn] = same(xs)
    back.push(c)
```

**Generated behavior.** `same` returns the exact list object; the caller
keeps both aliases. No new container or element is created.

**Friction under the literal bound rule.** In `use_same`, `xs` has an
own root and an independent `xs.T` root. `^{value}` denotes their
union after substitution. The written prefix bounds every reach of
the result, including its invariant element component.

Replacing `xs.T` by `{xs, xs.T}` is forbidden widening. A sound
implementation must reject this instantiation, or reject the generic
contract for shapes it cannot preserve. Specializing from `same`'s body
would violate modular signature checking.

This is evidence about the reach of L5, not a newly alleged escape.
It affects identity on mutable collections, not only functions bundling
unrelated resources. Returning the input's reach *shape* would work,
but the record currently defines substitution as a set union.

### R36. The Declared-Slot Workaround Still Flattens Its Field

Original shape:

```rust
struct Bundle<'s, 'a, 'b> {
    list: &'s mut Vec<&'a Conn>,
    conn: &'b Conn,
}
```

```text
data Bundle[^l, ^k]: # hypothetical syntax
    list: ^{l} mut List[Conn] # hypothetical syntax
    conn: ^{k} Conn # hypothetical syntax
impl NonEscapable for Bundle

fn bundle_apart(dst: ^mut List[Conn], c: ^Conn) -> Bundle[^{dst}, ^{c}]: # hypothetical syntax
    Bundle { list: dst, conn: c }
```

**Generated behavior.** A new holder aliases the original list and
handle. This is the design's own round-6 R23 fallback, with its claimed
successful outcome under test.

**Breaks: claimed workaround conflicts with the bound rules.** Slot
`l` is instantiated by `^{dst}`, which denotes the whole list. The
field prefix `^{l}` covers its own reach and `T` component together.
Thus `dst.T` must widen to `{dst, dst.T}`, although mutable components
are invariant.

Giving `l` only `dst.T` would instead omit the list object's own root.
The holder needs three distinctions: list identity/locality, list element
reach, and the unrelated handle. The shown two-slot holder only
separates the last from a flattened list.

This does not disprove independent component tracking. It disproves the
claim that this particular prefix-annotated field preserves it. A wrapper
with an unannotated generic field might preserve the local shape; its
exported constructor/result contract still needs an account.

### R37. A Safe List Lookup Becomes A Flattening Boundary

Original shape, using readonly wrappers around an interior mutable value:

```rust
struct Pair<'a, 'b> { cell: &'a RefCell<Vec<Conn>>, conn: &'b Conn }
// A borrowed vector lookup preserves each lifetime in Pair's type.
```

```text
data WritablePair[^a, ^b]: # hypothetical syntax
    first: ^{a} mut List[Conn] # hypothetical syntax
    second: ^{b} Conn # hypothetical syntax
impl NonEscapable for WritablePair

fn read_element(xs: ^List[WritablePair]) -> usize: # hypothetical syntax
    match xs.get(0):
        .Some(p) => p.first.len()
        .None => 0

fn direct_element(xs: ^List[WritablePair]) -> usize: # hypothetical syntax
    for p in xs:
        _ := p.first.len()
    0
```

**Generated behavior.** `get` returns an optional reference to an
existing element. The loop reads the same stored elements; neither
operation widens or writes the list at run time.

**Breaks: extraction and signature rules disagree.** `ne.data.wrapper`
and `ne.data.no-join` preserve `xs.T.a` and `xs.T.b` on extraction.
However, the published `get` result is `^{self.T} T?`. It has one
bound covering every nested slot, including invariant `a`.

Under whole-subvalue substitution, that bound joins the independent
sources and must reject. If a component atom denotes only its own
node's set instead, it fails to name the nested slots at all. Neither
reading justifies the promised component-preserving `get`.

The distinction matters even for this read-only use. The returned value
still contains mutable access, which another use could exercise. An
intrinsic indexing or pattern operation may preserve the shape directly;
that does not supply the missing ordinary method contract.

### R38. Different List Elements Still Need A Join Check

Original shape:

```rust
// Two cells can coexist with independent internal lifetimes.
// Treating them as one Cell<T> type requires compatible invariant T.
```

```text
fn collect_cells(a: ^mut List[Conn], b: ^mut List[Conn]) -> usize: # hypothetical syntax
    let rows: List[mut List[Conn]] = [a, b]
    rows.len()
```

**Generated behavior.** A fresh outer list holds aliases to both
independent lists. Only the outer length is observed.

**Friction.** The outer component must share a reach, and its nested
element component must summarize both `a.T` and `b.T`. The inner
lists expose mutable storage, so those distinct fixed reaches cannot
widen to their union. A conservative implementation rejects.

This is the sound consequence of the chosen homogeneous component model.
It is not a request to give every list element a separate lifetime.
However, "aggregation never widens a mutable component" needs qualification:
multiple values entering the *same* component can demand exactly that
widening, even though different named components are never joined.

### R39. Binding A Getter Removes The Name Of Its Component

Original shape:

```go
get := repository.Get // saves the receiver; invokes Get later
```

```text
fn first_direct(xs: ^List[Conn]) -> ^{xs.T} Conn?: # hypothetical syntax
    xs.get(0)

fn first_bound(xs: ^List[Conn]) -> ^{xs.T} Conn?: # hypothetical syntax
    get := xs::get
    get(0)
```

**Generated behavior.** The bound reference captures the same list and
calls the same method. Both functions return its optional first element.

**Breaks: missing result translation.** The method's result names
`self.T`. Round 6 states how `self.T` *stores* become whole-environment
stores, but gives no corresponding translation for result paths.
The receiver parameter no longer exists in the callable type.

Replacing `self.T` by `get` yields the union of the captured list's
own and element reaches, so `first_bound` rejects while `first_direct`
works for this simple `Conn` element. Leaving `self.T` is unbound;
writing `get.T` does not name a declared component of a function type.

Conservative whole-environment results are possible, but should be an
explicit loss like whole-environment stores. Retaining an internal
captured-component map is another possibility, requiring an exported
account when the callable crosses a helper boundary.

### R40. Reading Metadata Turns It Into A Store Destination

Original shape, approximating a Go callback with two captures:

```go
put := func(c Conn) { _ = metadata.Len(); destination.Push(c) }
```

```text
fn apply_store(put: ^fn(c: ^{put} Conn) -> void, c: ^{put} Conn) -> void: # hypothetical syntax
    put(c)

fn store_with_metadata(dst: ^mut List[Conn], metadata: ^List[usize], c: ^{dst} Conn) -> void: # hypothetical syntax
    apply_store(
        (fn(x):
            _ := metadata.len()
            dst.push(x)
        ),
        c,
    )
```

**Generated behavior.** The closure reads metadata and writes only to
`dst`. This is legal permission-wise; metadata is read-only.

**Friction by the explicit proposed rule.** `ne.fntype.env-store`
adds `c` to *every* captured object and binding. The caller therefore
needs permission to add it to `metadata` too. Its fixed parameter
reach has no such permission, so the call rejects.

The mark `^{dst}` is deliberately broader than `^{dst.T}` here, to
avoid confusing this rejection with the acknowledged whole-receiver loss.
Even so, the unrelated capture causes rejection. If metadata is a static
provider value captured in a local, its object cannot accept a borrowed
root at all.

This is conservative, not an undeclared write exploit. The workaround
is a data method with separate parameters/destinations, or moving the
metadata read outside the closure when its timing permits. Reading
metadata earlier can change observable behavior when it is mutable.

### R41. Function Constructor Identity Still Needs Retention Binders

Original comparison shape:

```rust
// A callable accepting only &'static [u8] cannot satisfy a bound
// requiring it to accept any short-lived slice.
```

```text
use std.function.Fn
let BUFFERS: mut List[List[u8]] = []

fn retain(bytes: List[u8]) -> void:
    BUFFERS.push(bytes)

fn through_constructor(bytes: ^List[u8]) -> void: # hypothetical syntax
    let global: Fn[(List[u8],), void, $()] = retain
    let local: fn(b: ^List[u8]) -> void = global # hypothetical syntax
    local(bytes)
```

**Generated behavior.** If the second conversion were accepted, the
global-retaining function would receive a borrowed ordinary buffer. It
would retain it in `BUFFERS`, contradicting the local callback contract.

**Breaks: incomplete representation, not proven accepted conversion.**
The second assignment must reject. The design's input-mode rules give
the reason. But the still-exact `Fn` sugar has only its input tuple,
output type, and row; those do not represent modes, store destinations,
environment binders, or component-dependent results.

Defaulting the constructor spelling to global inputs could reject this
example soundly. It still leaves no stated constructor equivalent for
the local function type. Parameter *names* alone cannot encode the
new retention relations in the existing three-argument constructor.

This is the representation part of review 2's NE-S5c, not a new
discovery credited twice. Round 6 supplies frame slot arguments, but
saying the translation also holds for constructor forms does not define
those forms' retention identity or substitution.

### R42. A Lazy Iterator Produces Through Its Closure Field

Original shape:

```rust
let iter = std::iter::from_fn(move || Some(conn));
// next calls the closure; an element need not already be stored.
```

```text
data Generated[T]:
    step: fn() -> T?

impl[T] Generated[T]:
    fn next(^mut self) -> ^{self.T} T?: # hypothetical syntax
        (self.step)()
```

**Generated behavior.** This is the actual `Iterator[T]` implementation
shape under a different name. It stores a closure and calls it once
per `next`, with no eagerly stored `T` payload.

**Breaks: the proposed element contract does not follow from the field.**
The function value is stored as `step`; its environment contributes to
the holder's own reach. Its elided output depends on that environment.
Calling it therefore yields a value bounded by that own reach, whereas
the proposed result names the independent `self.T` root.

No rule equates them. A generic parameter's occurrence as a callback's
future result is not an already stored value of type `T`.
Thus a modular body check rejects the proposed `next` contract.
The round-6 std estimate includes three iterator payload reads without
accounting for this distinction.

Returning `^{self}` conservatively could work, with whole-object
dependence. A representation with an explicitly related output slot
could also work. Treating callbacks as automatically populating an
output component would need a new rule, including callbacks whose
output depends on their call inputs or providers.

### R43. Direct Frame Outputs Survive A Local Tuple

Original shape:

```rust
// A future may retain scratch data without returning a borrow of it.
// Combining results need not combine their retained environments.
```

```text
use std.task.all

fn select!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn: # hypothetical syntax
    _ := scratch.sent()
    keep

fn count!(scratch: ^Conn) -> usize: # hypothetical syntax
    scratch.sent()

fn together!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn: # hypothetical syntax
    values := all!(select(keep, scratch), count(scratch))
    values._0
```

**Generated behavior.** The first frame retains both arguments but its
output reaches only `keep`. The second produces a scalar. `all!`
drives both and returns their output tuple.

**Works.** `ne.all.typing` puts each child's `out` in a separate
tuple element. Local projection preserves the first output, and
`scratch` does not enter it. This does not cross a helper signature
which flattens that tuple's components.

### R44. Ordinary And Tracked Provider Keys Now Differ Deliberately

Original shape:

```go
// context.WithValue may carry a callback closing over request data.
```

```text
trait Reader:
    fn read(^self) -> usize # hypothetical syntax
trait ScopedReader < NonEscapable:
    fn read(^self) -> usize # hypothetical syntax

data CallbackReader:
    callback: fn() -> usize
impl Reader for CallbackReader:
    fn read(^self) -> usize: self.callback() # hypothetical syntax
impl NonEscapable for CallbackReader
impl ScopedReader for CallbackReader:
    fn read(^self) -> usize: self.callback() # hypothetical syntax

fn ordinary(p: ^dyn Reader) -> void: # hypothetical syntax
    $.with(Reader=p): # hypothetical syntax
        pass

fn current() -> dyn ScopedReader $ ScopedReader:
    $.use(ScopedReader)

fn tracked(p: ^dyn ScopedReader) -> ^{p} dyn ScopedReader: # hypothetical syntax
    $.with(ScopedReader=p):
        current()
```

**Generated behavior.** Ordinary binding rejects because its provider
argument has a parameter root. The tracked key instead exports a key
root, substituted with `p` at its call site.

**Works.** The returned scoped provider reaches `p`. A global store
would reject; a local observation can proceed. For an ordinary key,
a fresh callback provider fitting static remains allowed.

The two traits are deliberately different API promises. A concrete
provider opting into the scoped root trait implements NonEscapable;
erasure to the ordinary trait does not erase its provenance.

### R45. Scoped Callback Checking Still Rejects A Borrowed Result

Original shape:

```rust
// A scoped API can use a higher-ranked callback to prohibit returning
// a borrow of its temporary argument.
```

```text
fn with_conn[T](body: ^fn(c: ^Conn) -> ^{} T) -> ^{} T: # hypothetical syntax
    c := open()
    body(c)

fn observe() -> usize:
    with_conn(fn(c): c.sent())

fn borrow_out() -> Conn:
    with_conn(fn(c): c)
```

**Generated behavior.** The helper creates a handle and invokes its
callback. `observe` returns a scalar; `borrow_out` attempts to return
the callback parameter.

**Works, including the rejection.** `observe` has no retained parameter
root. `borrow_out` fails while checking the callback against `^{}`.
The helper's fresh handle fitting static does not make the callback's
abstract local parameter static during that check.

This proves a retention boundary only. Closing and runtime use after
closing remain outside this review's guarantee.

## Comparison With Other Languages

These comparisons identify semantic obligations, not proposals to adopt
another language's ownership system. The original-shape snippets are
approximations, not executable translations of hd's whole runtime.

| Boundary | Primary-source comparison | Implication for hd |
| --- | --- | --- |
| Mutable aggregation, R35–R38 | Rust makes interior-mutable lifetime positions invariant; its type arguments retain separate lifetime identities. [Rustonomicon](https://doc.rust-lang.org/nomicon/subtyping.html) | Equalities must survive construction, calls, fields, and same-component aggregation |
| Generic identity, R35 | Rust's `identity<T>` returns the same `T`; it moves its argument. [Identity documentation](https://doc.rust-lang.org/std/convert/fn.identity.html) | hd's shared aliases make silently flattening invariant reaches especially dangerous; rejecting the conversion is sound but costly |
| Stored elements, R37–R38 | Rust's vector stores a single `T` type and `push` accepts that `T`. [Vec documentation](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.push) | Independent components do not eliminate compatibility checks among elements sharing a component |
| Bound receiver, R39 | Go evaluates and saves a method value's receiver. [Method values](https://go.dev/ref/spec#Method_values) | Saving the receiver must also translate its dependent output contract in hd |
| Partial application, R39 | Python's `partial` binds supplied arguments. [functools documentation](https://docs.python.org/3/library/functools.html#functools.partial) | Capturing an argument changes where dependencies are named, even if runtime behavior is unchanged |
| Frame versus output, R34/R43 | Rust's `Future` has a separate associated `Output` and a polling operation. [Future documentation](https://doc.rust-lang.org/std/future/trait.Future.html) | Retention and output are distinct; constructor weakening must preserve both |
| Generated iterator, R42 | Rust's `from_fn` uses its captured closure as `next`. [from_fn documentation](https://doc.rust-lang.org/std/iter/fn.from_fn.html) | Item production cannot be modeled solely as extraction of an already stored payload |
| Context services, R44 | Go's `context` carries request-scoped values through API boundaries. [context documentation](https://pkg.go.dev/context#WithValue) | hd deliberately adds a static-provider restriction unless the key opts into tracking |

## Problems, Ranked

Severity describes the consequence of the permissive interpretation.
Sound rejection is always available at the uncertain boundaries, but
can contradict an advertised example or remove useful expressiveness.

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | NE-S5d | Weakening uses elision inputs as frame-retention inputs | Critical contract conflict | R34 |
| 2 | NE-S1d | Reach components collapse again at written bounds | High: conflicting claimed support and invariant conversion obligation | R35–R37 |
| 3 | NE-S4c | Bound component-dependent results lack a translation | High: modular callable boundary incomplete | R39 |
| 4 | NE-S5c, remaining part | `Fn`/`SuspendFn` retention identity is unspecified | High: global/local conversion must remain distinguishable | R41 |
| 5 | NE-S12 | A generated output is not a stored generic payload | High: proposed std signature fails its body | R42 |
| 6 | NE-S4d | Whole-environment stores include unrelated captures | Moderate expressiveness cost | R40 |
| 7 | NE-S1e | Same-component aggregation still needs invariant checks | Moderate accepted-model cost, missing qualification | R38 |

### NE-S5d. Retention Inputs Are Not Elision Inputs

**Effect.** R34 is a new omission in NE-S5c's translation. Direct
construction and constructor weakening disagree on the same ordinary
marked buffer. A lowered frame can falsely advertise empty retention.

**Candidates, for the owner.**

| Question | Cost | Effect |
| --- | --- | --- |
| Should weakening include every marked parameter, including non-carrying ordinary types, alongside the environment and tracked providers? | One language semantic correction | Matches direct cold construction; leaves result-elision policy unchanged |
| Should all cold-constructor translations share the direct frame-retention computation rather than enumerate elision inputs separately? | One language semantic definition/refactoring | Removes this duplicated formula; ordinary static arguments still contribute no scoped constraint |

No ordinary library addition can repair compiler-provided callable weakening.
Both candidates belong to the language tier and add no syntax or intrinsic.

### NE-S1d. An Independent Reach Tree Needs A Boundary Representation

**Effect.** R35 broadens the practical cost of the new L5. R36
contradicts its advertised workaround. R37 shows a conflict between
component-preserving extraction and the explicitly proposed method signature.

This is new evidence about the *round-6 replacement*, not a request
to restore the rejected one-reach aggregation model. The case needs
either preserved shape or soundly documented rejection.

**Candidates, for the owner.**

| Question | Cost | Effect |
| --- | --- | --- |
| Should existing prefix bounds remain flat sets, with these helpers/fields rejected and the unsupported fallback removed? | One language semantic qualification and documentation | Least new machinery; makes the larger L5 cost explicit and does not restore joining |
| Should a component/parameter reference preserve a reach shape when the result has the corresponding type, with explicit rules for unions and unequal shapes? | Language semantic representation and substitution rules | Could preserve identity and generic reads; changes the current set-only meaning and must be checked independently |
| Should written field/result contracts gain per-component relations? | New syntax plus language semantic rules | Most expressive and most expensive; applies only if existing path/slot notation cannot express the relation |

A library wrapper is cheapest only if its declared fields and exported
factory can preserve the required distinctions. R36 shows why merely
adding an outer slot is not already such a solution. No candidate is
selected, and the current single-bound agent detail is not assumed accepted.

### NE-S4c. Bound Results Need Receiver-Path Substitution

**Effect.** R39 compares two operations with identical runtime behavior.
Only binding the receiver loses the component-specific result guarantee.
NE-S4b translated stores; it did not finish the result translation.

**Candidates, for the owner.**

| Question | Cost | Effect |
| --- | --- | --- |
| Should bound references conservatively replace every receiver-dependent result with the whole environment and document the loss? | One language semantic translation | Sound for covariant results; invariant results still require equality or rejection |
| Should inferred callable types retain a receiver-component substitution map, with an explicit account at exported boundaries? | Language semantic representation and translation | Preserves precise bound getters locally; requires a finite interface representation |

The named helper `first_direct` is an ordinary library workaround for
this example. It does not make `first_bound`'s contract expressible.
The core question remains in the language tier.

### NE-S5c. Function Constructors Still Need The Full Contract

**Effect.** R41 keeps the unresolved representation part of review 2's
NE-S5c visible. Frame slots alone do not encode input modes and binders.
Constructor equivalence must not turn a retaining callback into a local one.

**Candidates, for the owner.**

| Question | Cost | Effect |
| --- | --- | --- |
| Should a normalized callable contract, including modes and bound parameter indices, become part of constructor identity with a stated constructor spelling? | Language semantic representation; syntax if the current constructor cannot encode it | Preserves explicit sugar equivalence once fully defined |
| Should constructor forms temporarily denote only global/nondependent contracts, and cease being exact sugar for richer function types? | One language semantic restriction | Rejects R41 soundly, but explicitly narrows the existing equivalence claim |

Both are language-tier changes. A library type cannot distinguish
contracts the language currently identifies as the same callable type.
The second is a question about an unfinished representation, not a
chosen retreat from the owner's syntax decisions.

### NE-S12. Stored Producers Need An Output Dependency

**Effect.** R42 uses the existing std implementation rather than an
invented eager iterator. The proposed `self.T` result cannot be checked
from a closure field whose output depends on the holder's own reach.

**Candidates, for the owner.**

| Question | Cost | Effect |
| --- | --- | --- |
| Should generated iterators keep the conservative `^{self}` result, documenting that they differ from eager container reads? | Ordinary core-library signature change over existing language rules | Cheapest if it suffices; loses output precision and still faces L5 for mutable result shapes |
| Should an existing declared slot relate the stored producer's result to the iterator's item reach? | Ordinary library representation if existing slots suffice; otherwise a language semantic relation | Could preserve item precision; requires a valid field declaration and `from_fn` contract, not only a `next` annotation |

The iterator protocol and `next` are compiler-known language-tier items;
their signatures would need corresponding language-tier text. Lazy
adapters that remain ordinary hd belong to the stdlib tier. A new
compiler exception for iterators is not justified before those cheaper
representations are tried.

### NE-S4d. A Store Permission Covers Unrelated Captures

**Effect.** R40 is the direct consequence of the round-6 agent detail
that an environment store targets every capture. No soundness failure
is alleged. The cost grows when closures mix reads with writes.

**Candidates, for the owner.**

| Question | Cost | Effect |
| --- | --- | --- |
| Should this conservative rule remain, with named data methods or moving independent reads outside as documented workarounds? | Ordinary core-library structuring plus documentation | No new language machinery; moving reads is valid only when timing is preserved |
| Should environment store destinations retain a subset of captured components instead of the whole environment? | Language semantic representation; syntax only if exported subsets need new spelling | Preserves callbacks with unrelated captures; adds an effect relation to carry through helpers |

The whole-receiver loss already stated in R24 is not counted again.
R40 demonstrates the additional loss from unrelated read-only captures.

### NE-S1e. Sharing A Component Is Still Aggregation

**Effect.** R38 must reject when mutable element reaches differ. The
decision to separate named components does not remove this equality
requirement among values stored in the same component.

**Candidate, for the owner.** Should `ne.data.component-share` explicitly
require compatible invariant subcomponents, and should the unconditional
"aggregation never widens" claim be narrowed to separate components?
This is one language semantic clarification plus a documented restriction.
It introduces no per-element lifetimes, syntax, or compiler intrinsic.

## Evaluation By Dimension

The same eleven dimensions are retained. Migration is excluded.
No score is inferred from parser acceptance or from rewritten regression verdicts.

| Dimension | Change since review 2 | Current assessment |
| --- | --- | --- |
| Guarantee clarity | Better: provider restriction, environment stores, optimizer wording explicit | The retention promise is clear; retention inputs and elision inputs must remain distinct |
| Soundness | Old direct attacks now have stated rejection rules | Not established: R34 conflicts directly; other boundaries require rejection or fuller contracts |
| Precision | Independent components improve local tuples, optionals, and frame combinations | Written bounds can erase the improvement; L5 affects identity, fields, and ordinary getters |
| Practical expressiveness | Scoped callbacks and controlled setters remain useful | Some claimed library paths fail: slot wrapper, bound getter, generated iterator |
| Composability | Local structure is stronger | Main weakness: components do not yet cross all ordinary signature/callable boundaries |
| Modular contracts | Stores and two-slot constructors are more explicit | Reach-shape substitution, bound result paths, and constructor identity remain incomplete |
| Agent writability and human readability | Paths expose distinctions previously hidden in one reach | Agent must know own reach versus nested components and when a prefix flattens them; no writing trial performed |
| Conceptual cost | More capability, more machinery | Category, locality roots, nested component trees, declared slots, callable environments, and flat bounds now interact; cost has increased |
| Compiler feasibility and cost | More explicit representation requirements | Bitset solving alone does not define the shape/binder representation; no new complexity proof or benchmark |
| Optimization validity and payoff | R33 wording repaired correctly | No new optimization counterexample; provenance loss such as R34 would invalidate summary-based assumptions; payoff remains unmeasured |
| Specification readiness | Previous patches are concrete progress | Still not ready as a complete system: settle these core translations before expanding the feature surface |

The judgment is **better local rules, unfinished boundary algebra**.
Most issues no longer concern whether a bare callback may store an input.
They concern preserving the now-richer information through existing abstraction forms.

## Parse Log

Every one of the thirteen `text` blocks was sent to
`scripts/hd-conformance.sh parse FILE` in the isolated checkout.
All raw blocks reject proposed syntax. Nine marker-only projections
parse, and all thirteen structural projections parse.

Projection rules, applied only to code before each line's comment:

1. Remove lifetime slot declarations/applications from generic lists, preserving ordinary type arguments.
2. Remove `^{...}` prefixes and bare `^` markers; preserve line breaks.
3. For the structural projection only, erase parameter names inside function *types*, preserving executable closure parameters.

This is a mechanical syntax check. It does not replace slots with
a proposed implementation, infer a contract, resolve names, or type-check.
The parser accepts the operations after unsupported syntax is removed.

| Block | Case | Raw | Markers only | Structural |
| --- | --- | --- | --- | --- |
| 1 | Prelude | Reject | Parse | Parse |
| 2 | R34 | Reject | Reject named function-type parameters | Parse |
| 3 | R35 | Reject | Parse | Parse |
| 4 | R36 | Reject | Parse | Parse |
| 5 | R37 | Reject | Parse | Parse |
| 6 | R38 | Reject | Parse | Parse |
| 7 | R39 | Reject | Parse | Parse |
| 8 | R40 | Reject | Reject named function-type parameters | Parse |
| 9 | R41 | Reject | Reject named function-type parameters | Parse |
| 10 | R42 | Reject | Parse | Parse |
| 11 | R43 | Reject | Parse | Parse |
| 12 | R44 | Reject | Parse | Parse |
| 13 | R45 | Reject | Reject named function-type parameters | Parse |

The following are all diagnostics, verbatim except for replacing the
temporary source path by `FILE`. Each rejected invocation also ends
with `0/1 clean`; each accepted invocation prints `1/1 clean`.
Named function-type parameters are proposed by D8; the current
[function type rule](../spec/lang/07-functions.md#function-types-and-values) forbids them.
Lifetime prefixes and slots are also proposed rather than specified.
No rejection of currently specified syntax remains in these blocks.

```console
block 1 (Prelude), raw:
error: FILE:3:13: syntax-error: expected a parameter name, found '^'
block 2 (R34), raw:
error: FILE:4:20: syntax-error: expected a type, found '^'
error: FILE:7:18: syntax-error: expected a type, found '^'
error: FILE:10:19: syntax-error: expected a type, found '^'
markers only:
error: FILE:11:19: syntax-error: expected ')', found ':'
block 3 (R35), raw:
error: FILE:1:19: syntax-error: expected a type, found '^'
error: FILE:4:17: syntax-error: expected a type, found '^'
block 4 (R36), raw:
error: FILE:1:13: syntax-error: expected a generic parameter name, found '^'
error: FILE:6:22: syntax-error: expected a type, found '^'
block 5 (R37), raw:
error: FILE:1:19: syntax-error: expected a generic parameter name, found '^'
error: FILE:6:21: syntax-error: expected a type, found '^'
error: FILE:11:23: syntax-error: expected a type, found '^'
block 6 (R38), raw:
error: FILE:1:21: syntax-error: expected a type, found '^'
block 7 (R39), raw:
error: FILE:1:21: syntax-error: expected a type, found '^'
error: FILE:4:20: syntax-error: expected a type, found '^'
block 8 (R40), raw:
error: FILE:1:21: syntax-error: expected a type, found '^'
error: FILE:4:29: syntax-error: expected a type, found '^'
markers only:
error: FILE:1:25: syntax-error: expected ')', found ':'
block 9 (R41), raw:
error: FILE:7:31: syntax-error: expected a type, found '^'
error: FILE:9:20: syntax-error: expected ')', found ':'
markers only:
error: FILE:9:20: syntax-error: expected ')', found ':'
block 10 (R42), raw:
error: FILE:5:13: syntax-error: expected a parameter name, found '^'
block 11 (R43), raw:
error: FILE:3:18: syntax-error: expected a type, found '^'
error: FILE:7:20: syntax-error: expected a type, found '^'
error: FILE:10:20: syntax-error: expected a type, found '^'
block 12 (R44), raw:
error: FILE:2:13: syntax-error: expected a parameter name, found '^'
error: FILE:4:13: syntax-error: expected a parameter name, found '^'
error: FILE:9:13: syntax-error: expected a parameter name, found '^'
error: FILE:12:13: syntax-error: expected a parameter name, found '^'
error: FILE:14:16: syntax-error: expected a type, found '^'
error: FILE:21:15: syntax-error: expected a type, found '^'
block 13 (R45), raw:
error: FILE:1:23: syntax-error: expected a type, found '^'
markers only:
error: FILE:1:27: syntax-error: expected ')', found ':'
```


## Repository Validation

`git fetch origin` refreshed the base. The isolated checkout at
`44c14e595d4c751cf2be2a3b1a258dcb58eb7904` ran `git rebase origin/main`,
which reported `HEAD is up to date`. The reviewed design is identical
there; intervening commits change other areas.

Only this report and its README link were copied over that base. Existing
parser and playground Wasm artifacts supplied prerequisites; the website
build compiled its isolated native compiler prerequisite automatically.

- All thirteen raw report blocks were checked and rejected proposed syntax as expected.
- All thirteen structural projections parse; nine need only lifetime removal.
- `bash spec/check.sh` passed: links/anchors, 5971 rule IDs, 1396 diagnostic rows, imports, and example inventory.
- The check reports eighty existing dead-citation warnings and zero failing citations.
- `pnpm run website:build` passed: 79 pages, 265 grammar rules, 266 definitions, and 693 of 752 references linked.
- Whitespace checks passed. No decision, design, spec, compiler, or fixture was edited. No commit or push was made.

The first documentation check found one incorrect anchor in this report;
it was corrected to `function-types-and-values` before the passing run.
No semantic lifetime-checker or runtime validation is claimed.
