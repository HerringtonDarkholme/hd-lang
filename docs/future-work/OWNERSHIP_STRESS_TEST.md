# Ownership And Borrows: Adversarial Review

Status: design review, 2026-10-16; changes no decision, design record,
spec text, or compiler code. Nothing here is accepted behavior.
The subject is [Ownership And Borrows](OWNERSHIP_DESIGN.md), at commit
`fbcde2bf4b9cc9ddf3dcfc2c2222c4e7c9d13b1c`.

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Problems, Ranked](#problems-ranked)
- [Evaluation By Dimension](#evaluation-by-dimension)
- [Earlier Findings](#earlier-findings)
- [Parse Log](#parse-log)
- [Repository Validation](#repository-validation)

## Surface Being Tested

The owner chose position-based ownership, moves, checked borrows, and
lifetimes inside types. The collector and `defer` stay; destructors and
RAII do not enter this review's candidate answers.

The rest of the study remains a proposal. This review tests its current
linear variant, including the three explicitly acknowledged holes.
It does not treat the recommendations in Open Points as accepted fixes.

| Assumption | Study location |
| --- | --- |
| Plain aliases coexist with owned and borrowed positions | Sections 2, 4.1, 4.3 |
| Fresh owned composites require unique composite fields | Decisions So Far; 4.1.7 |
| Every local `^` is checked, but earlier plain aliases remain unchecked | 3.6; 4.3.9 |
| A mutable borrow moves; a holder containing one must move | 4.3.6; Open Point 5 |
| An owner must be consumed or reserved at each suspension boundary | 3.1; 4.5 |
| Owned cold-frame arguments are forbidden, but owned results are not forbidden | 3.9; 4.6.7 |
| A consuming closure is one-shot; other functions retain today's callable behavior | 3.7; 4.6.5–6 |
| Provider installation borrows, and retrieval returns a key-bounded borrow | 3.10; 4.6.8 |
| `List.at`, `into_iter`, and ownership-aware pool methods are proposed APIs | 3.3; 3.5; not existing std signatures |

Dependent language-tier sections are [access and generics](../../spec/lang/04-type-system.md),
[iteration and cleanup](../../spec/lang/06-control-flow.md),
[function types, method references, and captures](../../spec/lang/07-functions.md),
[data and visibility](../../spec/lang/08-data-and-enums.md),
[trait values](../../spec/lang/09-traits.md),
[built-in list methods](../../spec/lang/10-modules.md#built-in-methods),
and [providers, frames, and cancellation](../../spec/lang/11-requirements-and-suspension.md).
The dependent stdlib-tier sections are [collections](../../spec/std/collections.md),
[iterators](../../spec/std/iter.md), and [tasks](../../spec/std/task.md).
The study supersedes those rules only where it proposes a replacement;
existing runtime behavior supplies the remaining integration questions.

## Method

Twelve small cases approximate public library patterns. The foreign code
shows a shape, not a line-for-line port. Undeclared operations such as
`connect!`, `wait!`, and `use_conn!` stand for ordinary resource APIs.
`Conn`, `Document`, and cleanup methods have the study's contracts.

Each hd block is parsed as written and after a documented erasure of
proposed syntax. Parsing is not type-checking or an executable exploit.
The verdicts follow the written rules, including conservative rejection
when that is the sound interpretation.

| Verdict | Meaning |
| --- | --- |
| Works | The proposed rules give the intended operation or rejection |
| Friction | A sound rejection has a practical restructuring workaround |
| Breaks | A stated guarantee fails, a contract is missing, or the requested abstraction is inexpressible |

“Breaks” is not a count of proven compiler vulnerabilities. The compiler
does not implement this design. Each case distinguishes a known hole,
a new conservation problem, and an unresolved integration contract.
No annotation or performance score is inferred from these examples.

Migration is excluded, as requested. Agent writing trials, borrow-solver
benchmarks, and optimization benchmarks were not performed.

## Summary

| Case | Library shape | Verdict | Finding |
| --- | --- | --- | --- |
| O1 | Go file plus deferred close | Works | Straight-line ownership and cleanup compose |
| O2 | Rust borrowed struct plus generic identity | Works | Independent lifetimes need not flatten at a generic boundary |
| O3 | Rust/Swift generic forwarding | Breaks | Known B8: unrestricted generic copying duplicates ownership |
| O4 | Rust owning aggregate | Breaks | Known B7: a plain holder hides the consumption obligation |
| O5 | Rust closure over mutable local | Breaks | Known B5: captures bypass the local loan check |
| O6 | Tokio `join!` returning resources | Breaks | New: buffered completed results have no cancellation cleanup |
| O7 | Rust consuming iterator with early exit | Friction | Remaining elements need an explicit owner and cleanup |
| O8 | Rust `Vec.clear` and consuming filters | Breaks | Destructive collection operations need a linear contract |
| O9 | Rust mutable async callback | Breaks | Moving a closure does not define exclusive invocation |
| O10 | Rust object with owned and borrowed fields | Breaks | Freshness rule excludes ordinary mixed ownership graphs |
| O11 | Python transferable cleanup stack | Friction | Reservation blocks direct transfer; an owner slot may help |
| O12 | Rust mutable context service | Breaks | Provider sharing needs a mutable-loan contract |

There are **two works, two friction, and eight breaks**. Three breaks are
the study's own B5, B7, and B8, not new discoveries. The remaining five
include missing contracts and an expressiveness restriction, not five
demonstrated accepted exploits.

The direction is stronger than the old retention-only design for its
stated resource-safety goal. Its main new burden is **conserving ownership
through every hidden holder**, including compiler frames, iterators,
generic bodies, and callable environments. Rust's ownership vocabulary
does not supply the cleanup behavior that hd deliberately omits.

## Cases

### O1. Deferred Close And A Late Use

Original shape: Go's file handle with deferred close.

```go
f, err := os.Open(path)
if err != nil { return err }
defer f.Close()
_, err = f.Read(buf)
return err
```

hd, with fallibility omitted to isolate ownership:

```text
fn read_once!(host: string) -> string:
    c := connect!(host)
    defer: c.close()
    c.read!()

fn late_use!(host: string) -> usize:
    c := connect!(host)
    c.close()
    c.sent()
```

**Generated behavior.** The first frame reserves `c`, lends it during
`read!`, and consumes it on normal exit or cancellation. The second body
fails at `sent` because `close` moved `c`.

**Verdict: works.** This is a real improvement over NonEscapable:
retention bounds alone did not make closing consume the handle.
The guarantee assumes a trustworthy resource implementation: consuming
the representation must actually close its external resource.

The [Go file API](https://pkg.go.dev/os#File.Close) documents the underlying
close operation. hd adds a static consumption obligation around that shape.

### O2. Independent Lifetimes Through Identity

Original shape: a Rust aggregate whose two references remain independent.

```rust
struct Pair<'a, 'b> { left: &'a Doc, right: &'b Doc }
fn same<T>(value: T) -> T { value }
```

```text
data Views[^a, ^b]:                              # hypothetical syntax
    left: ^{a} Document                         # hypothetical syntax
    right: ^{b} Document                        # hypothetical syntax

fn same[T](value: T) -> T: value

fn forward(a: ^Document, b: ^Document) -> Views[^{a}, ^{b}]: # hypothetical syntax
    same(Views { left: a, right: b })
```

**Generated behavior.** `T` is the complete instantiated `Views` type.
Its existing lifetime arguments remain in that type across identity.
Neither an additional whole-value reach nor a flat prefix is needed.

**Verdict: works**, for this single-use body and these shared borrows.
This does not cure O3's separate problem with the range of generic bodies.
It shows that the old identity-flattening objection is not intrinsic to
the replacement representation.

Rust's [elision rules](https://doc.rust-lang.org/reference/lifetime-elision.html)
also distinguish input lifetimes before assigning result lifetimes.
The hd study still needs to define how paths select nested arguments.

### O3. Generic Duplication Of An Owner

Original shape: Rust requires a duplicable type for this helper.

```rust
fn duplicate<T: Copy>(value: T) -> (T, T) { (value, value) }
```

```text
fn duplicate[T](value: T) -> (T, T):
    (value, value)

fn close_twice(c: own Conn) -> void:             # hypothetical syntax
    let (a, b) = duplicate::[own Conn](c)         # hypothetical syntax
    a.close()
    b.close()
```

**Generated behavior.** A body checked as today's unrestricted generic
copies `value`. Substituting an owned type then produces two consumers
of the same handle. Explicit instantiation avoids any ambiguity about
“inference never makes `T` owned.”

**Verdict: breaks; acknowledged B8.** This is a core soundness issue,
not an ergonomic edge. The study already proposes three possible answers
in Open Point 9; none has been chosen.

[Swift's noncopyable generics proposal](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md)
is relevant because a generic body's permitted operations must agree
with its admissible type arguments. Merely rejecting the spelling
`own T` does not establish that agreement.

### O4. Moving Into A Plain Holder

Original shape: Rust makes the owning aggregate noncopyable too.

```rust
struct Boxed { conn: Conn }
fn put(conn: Conn) -> Boxed { Boxed { conn } }
```

```text
data Boxed:
    conn: own Conn                             # hypothetical syntax

fn put(conn: own Conn) -> Boxed:                # hypothetical syntax
    Boxed { conn: conn }

fn park(conn: own Conn) -> void:                # hypothetical syntax
    box := put(conn)
    _ := box
```

**Generated behavior.** Moving `conn` satisfies the parameter's local
obligation. The plain result may be aliased and discarded, while
`move-out-of-plain` prevents extracting its owner for closing.

**Verdict: breaks; acknowledged B7.** The rule must cover indirect parts,
generic arguments, enum payloads, and tuples, not just named fields.
`Option[own Conn]` and `Result[own Conn, E]` make this an everyday boundary.
A tuple retaining element modes is promising, but its obligation still
must survive storage in a larger plain aggregate.

This is a consumption leak, not a demonstrated double-close. Preventing
two aliases from extracting the field solves a different problem.

### O5. A Captured Binding Bypasses A Shared Loan

Original Rust shape; the live reference prevents the intervening mutation.

```rust
let mut prices = vec![5, 7];
let mut change = || prices.push(9);
// A live shared loan and change() cannot overlap.
```

```text
fn captured_alias() -> usize:
    let prices: mut List[i32] = [+5, +7]
    change := fn() -> void: prices.push(+9)
    let view: ^List[i32] = prices                # hypothetical syntax
    change()
    view.len()
```

**Generated behavior.** The closure retains today's shared captured
storage. Its invocation mutates `prices` without a checked direct use
of that binding at the call site.

**Verdict: breaks; acknowledged B5.** Making closures borrow captures
would reject this overlap at capture or invocation. No such repair is
assumed here.

There is also a deliberately weaker plain-alias boundary: an earlier
plain list alias can mutate the same allocation. That is explicitly
documented in 3.6 and is not counted as another new hole.
Rust's [capture rules](https://doc.rust-lang.org/reference/types/closure.html)
make capture access part of checking a closure.

### O6. Cancellation After One Child Has Returned An Owner

Original shape: concurrent resource acquisition with Tokio.

```rust
let (left, right) = tokio::join!(acquire("left"), acquire("right"));
```

```text
use std.task.all

fn acquire!(host: string) -> own Conn:          # hypothetical syntax
    connect!(host)

fn both!() -> void:
    let (a, b) = all!(acquire("left"), acquire("right"))
    defer: a.close()
    defer: b.close()
    a.send!([1])
    b.send!([2])
```

**Generated behavior.** The cold children take only plain strings, so
the owned-argument restriction does not reject them. Suppose the left
child completes and the right remains pending. The existing `all!`
contract stores the left result and never polls that child again.

Now cancel the parent. The left child has no unfinished frame or pending
`defer`: it returned its owner. The parent's two defers have not been
registered. Cancelling the right child cannot consume the stored left
result.

**Verdict: breaks; new cancellation conservation gap.** Source bindings
alone are insufficient: the owner exists in generated storage before
the caller can name it. Treating that storage as linear would instead
force rejection or require a cleanup contract absent from the study.

The problem also reaches `all_list!`, partial tuple evaluation across a
later bang call, and adapters that buffer an owned completion. Those are
additional exposure paths, not separately counted exploits.
An owned result is distinct from an owned cold-frame argument.

Tokio documents [concurrent inline futures](https://docs.rs/tokio/latest/tokio/macro.join.html).
Rust's [destructor semantics](https://doc.rust-lang.org/reference/destructors.html)
explain why copying that API shape into hd requires another answer.

### O7. Leaving An Owning Iterator Early

Original shape: Rust's consuming vector iterator.

```rust
for conn in connections.into_iter() {
    use_conn(&conn);
    break;
}
```

```text
fn take_one(xs: own List[own Conn]) -> void:     # hypothetical syntax
    for c in xs.into_iter():
        c.close()
        break
```

**Generated behavior.** The loop transfers `xs` into a hidden cursor.
Closing the yielded connection consumes only one element. The cursor
still owns the tail when `break` exits.

**Verdict: friction.** Rejecting this loop is sound. A manual pool or
owner slot can retain the tail and explicitly close it after the useful
work stops. A suspending body additionally needs that cleanup registered
before suspension.

The study's exhaustive `shutdown` loop needs a defined exhaustion
operation too. Runtime exhaustion must discharge the cursor's outer
obligation without silently discharging unvisited elements. Existing
`Iterator[T]` is a concrete closure-backed type, so “as in Rust” does not
already define a separate owning cursor or its loop lowering.

Rust's [IntoIter](https://doc.rust-lang.org/std/vec/struct.IntoIter.html)
moves elements and has a destructor. hd must specify exhaustion and
abrupt exits explicitly while retaining its no-destructor decision.

### O8. Clearing An Owned Collection

Original shape: `Vec.clear` removes its contents; Rust's ordinary
destructor rules cover owned elements.

```rust
fn clear_connections(xs: &mut Vec<Conn>) { xs.clear(); }
```

```text
fn clear_connections(xs: ^mut List[own Conn]) -> void: # hypothetical syntax
    xs.clear()
```

**Generated behavior.** If `clear` merely gains a `^mut self` receiver,
the current implementation calls `list_truncate(self, 0)`. The receiver
is borrowed, so the caller still owns an empty list. No local owner was
overwritten or dropped, yet every previous element lost its consumer.

**Verdict: breaks; missing destructive-operation contract.** A sound
implementation can reject the call. It cannot preserve today's clear
behavior for an arbitrary linear element without returning or consuming
each removed value.

This remains a requirement after B8 is fixed. An owned-enabled generic
body still needs rules for truncation, element replacement, `filter`
discard paths, and duplicate reads such as `get` and list spreads.
`push` and `pop` do not define that whole surface.

The existing source is [List.clear](../../lib/std/collections.hd) and the
language-tier contract is [Built-in Methods](../../spec/lang/10-modules.md#built-in-methods).
[Rust Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.clear)
and [destructor rules](https://doc.rust-lang.org/reference/destructors.html)
provide the comparison; they are not a proposed hd cleanup mechanism.

### O9. An Exclusive Capture Hidden In A Reusable Callable

Original Rust shape: an async callback lends mutable captured state to
the future it returns. Two outstanding invocations cannot both own that
exclusive access.

```rust
let mut send = async || socket.send(bytes).await;
send().await;
send().await;
```

```text
use std.task.all

fn sender(c: ^mut Conn) -> ^{c} fn!() -> void:   # hypothetical syntax
    fn!() -> void: use_conn!(c)

fn overlap!(c: ^mut Conn) -> void:              # hypothetical syntax
    send := sender(c)
    _ := all!(send(), send())
```

Here `use_conn!` requires `^mut Conn`; it is not a plain API escape.

**Generated behavior.** A closure environment holds the exclusive loan.
The first cold invocation must retain a reborrow while pending. Today's
plain callable interface does not require exclusive access to the
callable for a second invocation.

**Verdict: breaks; missing callable-access contract.** Making the holder
move, as Open Point 5 proposes, prevents copying its environment. It does
not alone say whether calling moves the holder, borrows it exclusively,
or allows two cold frames to refer to it.

If such a closure becomes one-shot, or each invocation lends an exclusive
callable borrow to its frame, this example rejects correctly. The study
does not state either rule. This issue survives fixing B5: the capture
here is explicitly borrowed and already exclusive.

Rust distinguishes [FnMut's mutable receiver](https://doc.rust-lang.org/std/ops/trait.FnMut.html)
and [async closures lending to their futures](https://doc.rust-lang.org/reference/types/closure.html#async-closure-traits).
A lifetime bound on the environment addresses duration, not invocation
exclusivity.

### O10. Owning A Resource While Borrowing A Document

Original shape: an ordinary Rust object with one owned field and one
reference. The reference is external, not a sibling-field borrow.

```rust
struct Job<'a> { conn: Conn, source: &'a Document }
fn job(conn: Conn, source: &Document) -> Job<'_> {
    Job { conn, source }
}
```

```text
data Job[^doc]:                                   # hypothetical syntax
    conn: own Conn                               # hypothetical syntax
    source: ^{doc} Document                      # hypothetical syntax

fn job(c: own Conn, d: ^Document) -> own Job[^{d}]: # hypothetical syntax
    Job { conn: c, source: d }

data Bag:
    values: mut List[i32]

fn refill(xs: mut List[i32]) -> own Bag:           # hypothetical syntax
    let bag: own mut Bag = Bag { values: [] }     # hypothetical syntax
    bag.values = xs
    bag
```

**Generated behavior.** Moving `c` is valid, but `d` is a borrowed
composite field, neither fresh nor moved from an owned place. Rule 4.1.7
therefore rejects fresh ownership of the aggregate. Making the result
plain leads back to B7.

**Verdict: breaks; expressiveness restriction.** Unique ownership of the
holder and its owned fields does not require unique ownership of an
explicitly borrowed referent. The present freshness wording conflates
these two properties.

If “every composite field” was intended to exclude declared borrowed
edges, this is a wording repair, and the case should work. That exception
is not currently written. It is new evidence about the chosen freshness
rule, not a request to move ownership onto type declarations.

The `refill` probe tests the complementary direction. Its fresh `Bag`
starts unique, then ordinary field assignment installs a pre-aliased
plain list. Rule 4.1.7 governs fresh construction, while the existing
field-store rule accepts any value matching the declared plain field.

Either that assignment needs the ownership invariant too, or `own`
guarantees only root uniqueness after construction. These are different
promises. This is an unresolved graph invariant, not a claim that the
existing documented plain-alias exclusion disappeared.

### O11. Validate, Then Transfer Ownership

Original shape: Python registers cleanup while validating a resource,
then transfers responsibility using an `ExitStack` operation.

```python
with ExitStack() as stack:
    conn = acquire()
    stack.callback(conn.close)
    validate(conn)
    stack.pop_all()
    return conn
```

```text
fn validated!(host: string) -> own Conn:          # hypothetical syntax
    c := connect!(host)
    defer: c.close()
    validate!(c)
    c
```

**Generated behavior.** Reservation protects cancellation during
`validate!`, then prevents the successful return from moving `c`.
Removing the defer fails at that bang call instead.

**Verdict: friction; acknowledged direct-transfer limit.** A library
holder may be enough for many cases: reserve an owned slot, validate a
borrow from it, then take its content through a `^mut` method. The
reserved slot itself remains until cleanup and is empty on success.

That candidate must demonstrate its body and empty-state consumption
under B7's eventual answer and the collection rules. It must not silently
cancel a registered defer or move a reserved binding. This is evidence
for trying an ordinary library representation before adding cleanup
transfer syntax.

Python documents [ExitStack.pop_all](https://docs.python.org/3/library/contextlib.html#contextlib.ExitStack.pop_all).
Its dynamic cleanup policy is the use case, not a proposed hd guarantee.

### O12. One Mutable Provider In Two Children

Original shape: Rust passes a mutable service borrow through a context.

```rust
async fn emit(sink: &mut Sink) { sink.write().await; }
// Two pending emit(&mut sink) calls cannot share that exclusive loan.
```

```text
use std.task.all

trait Sink:
    fn write!(^mut self, bytes: List[u8]) -> void  # hypothetical syntax

fn emit!() -> void $ Sink:
    $.use(Sink).write!([1])

fn duplicate_provider!(sink: ^mut dyn Sink) -> void: # hypothetical syntax
    $.with(Sink=sink):
        _ := all!(emit(), emit())
```

**Generated behavior.** Both cold frames capture the same key. The
existing mutable-provider rules explicitly permit siblings to share
one mutable provider. They derive provider access from the trait,
without a separate mutable row spelling.

The ownership study says installation borrows and retrieval returns a
borrow, but does not explain mutable provider installation, repeated
retrieval, or loans retained by child frames. A shared `^dyn Sink` cannot
call `^mut self`. Two retained exclusive loans cannot coexist either.

**Verdict: breaks; missing integration contract.** Rejecting the second
outstanding exclusive use is sound. Preserving the existing sharing
behavior instead needs an explicit dynamic-access or different API
contract. Cooperative single-thread scheduling does not end a loan
while its frame is suspended.

This is not a claim that all shared providers must serialize. Readonly
providers and explicitly shared services need a different access
contract from a service exposing `^mut self`.
The conflict is with [Mutable Providers](../../spec/lang/11-requirements-and-suspension.md#mutable-providers),
not an invented `$ mut Sink` spelling. Rust's
[RefCell](https://doc.rust-lang.org/std/cell/struct.RefCell.html#method.borrow_mut)
illustrates checked dynamic exclusivity; it does not make simultaneous
mutable loans valid.

## Comparison With Other Languages

| Mechanism | External evidence | Consequence for hd |
| --- | --- | --- |
| Reference duration | [Rust elision](https://doc.rust-lang.org/reference/lifetime-elision.html) assigns distinct input lifetimes and has a receiver rule | Useful starting point; independent declared lifetimes remove the old universal flat-bound problem |
| Cleanup of hidden owned storage | [Rust destructors](https://doc.rust-lang.org/reference/destructors.html) cover initialized fields and temporary scopes | Rust's handling of buffered results and abandoned iterators cannot be assumed without equivalent hd contracts |
| Generic ownership | [Swift noncopyable generics](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md) distinguishes generic copyability requirements | Admissible instantiations and operations inside bodies must agree |
| Callable access | [Rust call traits and async closures](https://doc.rust-lang.org/reference/types/closure.html#call-traits-and-coercions) distinguish consuming, mutable, and shared calls | `own fn` and an environment lifetime do not fully define exclusive reusable callbacks |
| Resource transfer after validation | [Python ExitStack](https://docs.python.org/3/library/contextlib.html#contextlib.ExitStack.pop_all) transfers pending cleanup callbacks | A useful pressure case for an hd owner-slot library |
| Leak guarantee | [Rust mem::forget](https://doc.rust-lang.org/std/mem/fn.forget.html) safely suppresses destructors | Rust is not a proof of mandatory close; hd's linear variant makes a different, stronger consumption promise |

The study should narrow “Rust catches all three” in its motivation.
Rust's normal resource APIs close through destructors, but safe Rust
permits leaking resources. hd's collector, explicit cleanup, plain
aliases, and linear consumption form a distinct combination.

## Problems, Ranked

Ranking prioritizes silent loss or duplication of a resource, then
missing exclusive-access contracts, then explicit expressiveness costs.
Known problems retain their B identifiers. New problems use OWN-S names
because the old NonEscapable mechanisms have been removed.

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | OWN-S1 | Ownership conservation through generated and destructive storage | Critical if accepted; otherwise a required restriction | O6, O8; O7 |
| 2 | B8 / B7 | Generic and aggregate operations must preserve linear obligations | Critical; acknowledged | O3, O4; affects O7–O8, O11 |
| 3 | OWN-S2 / B5 | Exclusive access must survive callable and provider boundaries | High; two missing contracts and one acknowledged hole | O5, O9, O12 |
| 4 | OWN-S3 | Fresh uniqueness must distinguish owned and borrowed edges | High expressiveness cost or wording defect | O10 |
| 5 | OWN-S4 | Cleanup reservation needs a practical transfer idiom | Moderate; acknowledged limit | O11 |

All candidates below are questions, not chosen changes. They preserve
the owner's position-based model. Costs follow AGENTS.md: ordinary
library additions first, then compiler intrinsics, semantic exceptions,
and new syntax last, when the alternatives can provide the same behavior.
An implementation of a general language rule is not counted as a new
compiler intrinsic merely because compiler code must implement it.

### OWN-S1. Where Does An Owner Go When Its Hidden Holder Stops?

**Effect.** O6 creates an owned result that the caller cannot reserve
yet. O8 destroys elements without creating a visible local obligation.
O7 leaves a hidden tail. One rule should account for owned contents in
all three forms of storage, including partially evaluated expressions.

**Candidates, for the owner:**

| Question | Cost and tier | Effect |
| --- | --- | --- |
| Should scoped acquisition write into a caller-owned reserved pool and return no owned child result? | One ordinary core-library protocol, stdlib tier | Cheapest workaround for acquisition; does not by itself make arbitrary `all!` results safe |
| Should `all!` reject owned-containing outputs, and should destructive list methods reject linear element types unless they return the removed owners? | At least two language semantic restrictions; list mutation and combinator contracts are language tier | Closes these paths conservatively without generic destruction |
| Should buffering and removal require an explicit consumer supplied by the caller? | A semantic consumer contract at intrinsic boundaries, plus library APIs; syntax only if existing function arguments cannot express it | Can support cancellation cleanup; the consumer must be available before the first owner enters storage |

An automatic call to `close` is not an ordinary generic solution.
Arbitrary owned types need not implement a closer, and cleanup may have
requirements or be forbidden from suspending. Async close remains a
recorded limit and is not reopened here.

### B8 / B7. What Carries The Obligation Across A Generic Or Aggregate?

**Effect.** O3 duplicates owners and O4 strands them. Both failures are
already acknowledged, but both are prerequisites for useful ownership.
They cannot remain appendices to an otherwise sound core.

**Candidates, for the owner:**

| Question | Cost and tier | Effect |
| --- | --- | --- |
| Should generic ownership require the proposed `own` bound, with owned fields making their holders owned as Open Points 8–9 suggest? | Two language semantic rules over proposed syntax | Makes many existing copyable generic bodies reject owned instantiations; aggregate closure must be recursive |
| Should every ownership-polymorphic body move `T`, with holders checked structurally for linear parts? | Two language semantic rules | Preserves broader instantiation but changes what an unconstrained body may do |
| Should each owned instantiation be checked, with aggregate obligations still propagated structurally? | A language semantic instantiation rule plus aggregate rule | More specialization and diagnostic cost; still needs exported compatibility rules |

These are the study's existing alternatives, not a proposal to move
ownership onto type declarations. None allows a generic truncation
primitive to erase an obligation unchecked.

### OWN-S2 / B5. Does A Callable Or Provider Lend Exclusive Access?

**Effect.** O5 is the acknowledged plain-capture bypass. O9 and O12
already carry explicit exclusive borrows, so capture freezing alone
does not solve them. The lifetime of an environment and permission to
invoke it are separate parts of a contract.

**Candidates, for the owner:**

| Question | Cost and tier | Effect |
| --- | --- | --- |
| Should reusable exclusive jobs use ordinary named data methods with `^mut self`, while unsupported exclusive closures are rejected? | Library representation plus one language semantic restriction; respective tiers | Cheapest representation for explicit jobs; does not alone settle implicit provider loans |
| Should an exclusive capture make a callable one-shot, and should provider loans retained by cold frames exclude a second borrower? | Two language semantic rules, language tier | Conservative; sequential reusable callbacks need rebuilding or a named wrapper |
| Should function contracts record exclusive invocation and lend that access to their returned frames, with the same accounting for provider keys? | Language semantic callable/row contracts; new syntax only if existing modifiers cannot express them | More expressive; requires representation, substitution, subtyping, and method-reference rules |

This does not reopen the old `mut fn` decision by default. A new callable
form would need evidence that the cheaper existing-method representation
is insufficient. Any chosen rule must survive bound methods and function
parameters, rather than being checked only at closure literals.

### OWN-S3. May A Unique Owner Contain An Explicit Borrow?

**Effect.** O10 cannot package an owner with a borrowed document under
the literal freshness rule. Such a graph is not self-referential.
The borrowed edge already advertises its aliasing and lifetime.
The second probe asks how the same invariant survives field replacement.

**Candidates, for the owner:**

| Question | Cost and tier | Effect |
| --- | --- | --- |
| Should the restriction stay, with the resource owner outside a separate borrowed work view? | Ordinary library representation, stdlib tier | Cheapest workaround; loses the movable combined object |
| Should uniqueness constrain owning/plain composite edges at construction and replacement, while declared borrowed edges keep their lifetime and access rules? | A language semantic clarification or exception plus a store-preservation rule | Expresses the combined object without new syntax; mutable borrowed fields still make their holders move |

If the latter was already intended, write it explicitly and reclassify
O10 as works. No relaxation for unmarked pre-aliased fields follows.

### OWN-S4. Can A Reserved Holder Transfer Its Contents?

**Effect.** O11 needs cancellation cleanup during validation and an
ownership handoff after it. The direct binding is deliberately reserved.
The study's pool methods suggest an indirect library answer.

**Candidates, for the owner:**

| Question | Cost and tier | Effect |
| --- | --- | --- |
| Should an ordinary owned slot support borrowed inspection, taking its content, and consuming its empty state? | One core-library type and methods, stdlib tier | Preferred by cost if a checked body proves it; depends on settled aggregate and removal rules |
| Should direct reservation remain nontransferable and this pattern be an explicit limitation? | Documentation of existing language rules | Smallest design surface; keep the practical limitation visible |

There is no evidence yet that defer-cancellation syntax is necessary.
First test the library slot under cancellation, error propagation,
partial moves, and an already-empty cleanup path.

## Evaluation By Dimension

These are the same eleven dimensions as the previous review. The
comparison is with the last reviewed NonEscapable model, not a claim
that the deleted record's later unreviewed edits had no effect.

| Dimension | Compared with the previous approach | Assessment of this draft |
| --- | --- | --- |
| Guarantee clarity | Stronger goal and more familiar explanation | Separate use-after-move, mandatory consumption, actual close, and exclusive access. Plain aliases and trusted resource constructors limit the promise |
| Soundness | Can address use-after-close and missed consumption, which retention alone could not | Not established: B5/B7/B8 are acknowledged; O6 adds hidden-result conservation; O9/O12 lack exclusive-access contracts |
| Precision | Independent declared lifetimes avoid the former flat-bound trap | Better ownership-specific precision, more intentional rejections: receiver-wide borrows, reservation, owning iteration, top-level ownership, and plain aliases |
| Practical expressiveness | Basic file, pool, and scoped-borrow patterns improve | Reusable exclusive callbacks, async handoff, owning iterators, mixed owned/borrowed objects, and owned concurrent results need attention |
| Composability | Generic identity can preserve complete lifetime-bearing types | Obligation propagation through aggregates, primitives, callbacks, providers, and generated frames is the new weakest seam |
| Modular contracts | Modes and named lifetimes are more direct than category plus reach machinery | Generic copyability, callable invocation access, provider loans, frame outputs, and destructive methods need complete exported contracts |
| Agent writability and human readability | Familiar owner/borrow vocabulary helps humans and agents | Context decides whether an unmarked use moves or lends; generic admissibility and cleanup timing remain hard. The 34-versus-35 count does not measure these costs; no writing trial performed |
| Conceptual cost | Removes NonEscapable categories, independent reach trees, and old run/output reach machinery | Adds modes, uniqueness, partial moves, linear exits, NLL, reservation, owning cursors, and callable access. The explanation is simpler; the full semantic system is not yet demonstrably smaller |
| Compiler feasibility and cost | Mature borrow-checking techniques provide a path | Requires place-sensitive dataflow, move state, loan regions, cancellation edges, and generic mode checking. GC removes reclamation work, not this static analysis; no benchmark or complexity proof |
| Optimization validity and payoff | Exclusive owned loans could support stronger optimization than retention summaries | No global Rust-style alias assumption is valid for plain values or earlier aliases. B5/B8 and hidden exclusive environments must be closed first; payoff is unmeasured |
| Specification readiness | Shorter, clearer study; supersession is explicit | Early design, not spec-ready. Fourteen open points include core safety policy, and several runtime/library boundaries remain unstated |

My assessment is **a better foundation for the expanded resource-safety
goal, with an incomplete linear core**. The old proposal's hardest
problems concerned preserving retention information across boundaries.
The replacement makes many of those boundaries ordinary types, but now
must conserve both owners and exclusive loans through them.

The largest unresolved choice is not lifetime spelling. It is which
operations may destroy, duplicate, or temporarily hide a value whose
consumption hd promises to check. Resolve that before optimizing the
annotation count or adding advanced borrow analysis.

## Earlier Findings

The previous report is preserved in git at commit `80624dd9e` as
`future-work/NONESCAPE_LIFETIME_STRESS_TEST_3.md`. The old design is
superseded, so its problem IDs do not impose repair obligations on this
different model.

| Earlier finding | Status under ownership |
| --- | --- |
| R34: cold constructor loses an ordinary marked input | Rule 4.6.7 now says every captured borrow bounds a frame. Exact lowering still needs a contract, but the old elision-input formula is gone |
| R35–R37: identity, wrappers, and getters flatten component reaches | O2 preserves complete lifetime-bearing types without the former flat reach prefix; owned-container access instead borrows the container |
| R38: invariant components cannot always join | Variance remains relevant; distinct mutable referents still cannot be arbitrarily merged. No lifetime solver proof is supplied |
| R39: bound getters lose receiver component names | The old component mechanism is gone. Bound references still need capture mode and result-lifetime substitution; O9 exposes the harder exclusive case |
| R40: a store destination covers unrelated captures | The specific whole-environment store rule is gone. Closure capture access and invocation loans remain unresolved |
| R41: `Fn[...]` cannot represent full contracts | The draft restricts lifetime marks to named `fn(...)` forms and proposes a diagnostic for constructor use. Exact sugar/equivalence and callable access still need reconciliation |
| R42: stored producer output differs from holder reach | This becomes a lifetime-bearing iterator/callback representation problem. Current concrete `Iterator[T]` is not automatically an owning iterator |
| R43–R45: frame result precision, providers, and scoped callbacks | Basic shared-borrow patterns remain plausible. O6 adds owned-result cancellation; O12 adds exclusive provider sharing |
| Closing was parked | Closing is now a central goal and receives meaningful move checks, subject to trusted resource implementations and completed linearity rules |

## Parse Log

All twelve `text` blocks were passed to
`scripts/hd-conformance.sh parse FILE`, as written and with proposed marks
erased. O1 parses as written. O2–O12 reject proposed syntax at marked
lines. All twelve erased blocks parse.

Erasure is mechanical and operates only before each line's comment:
remove lifetime parameter lists, `^{...}` bounds, bare `^`, and `own`;
then remove empty named lifetime applications. Preserve ordinary type
arguments, executable operations, comments, and line breaks.

This verifies surrounding grammar only. It does not establish types,
linearity, capture behavior, lifetime solving, or runtime cleanup.

| Block | Case | Raw result | Erased result |
| --- | --- | --- | --- |
| 1 | O1 | Parses | Parses |
| 2 | O2 | Rejects hypothetical syntax | Parses |
| 3 | O3 | Rejects hypothetical syntax | Parses |
| 4 | O4 | Rejects hypothetical syntax | Parses |
| 5 | O5 | Rejects hypothetical syntax | Parses |
| 6 | O6 | Rejects hypothetical syntax | Parses |
| 7 | O7 | Rejects hypothetical syntax | Parses |
| 8 | O8 | Rejects hypothetical syntax | Parses |
| 9 | O9 | Rejects hypothetical syntax | Parses |
| 10 | O10 | Rejects hypothetical syntax | Parses |
| 11 | O11 | Rejects hypothetical syntax | Parses |
| 12 | O12 | Rejects hypothetical syntax | Parses |

Raw parser output follows verbatim, with the temporary pathname replaced
by `FILE`. Every rejection is included, including secondary diagnostics.

### Block 1

```console
1/1 clean
```

### Block 2

```console
error: FILE:1:12: syntax-error: expected a generic parameter name, found '^'
error: FILE:7:15: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 3

```console
error: FILE:4:23: syntax-error: expected ',' or ')', found 'Conn'
error: FILE:5:34: syntax-error: expected ']', found 'Conn'
0/1 clean
```

### Block 4

```console
error: FILE:2:15: syntax-error: expected a line ending, found 'Conn'
error: FILE:4:18: syntax-error: expected ',' or ')', found 'Conn'
error: FILE:7:19: syntax-error: expected ',' or ')', found 'Conn'
0/1 clean
```

### Block 5

```console
error: FILE:4:15: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 6

```console
error: FILE:3:34: syntax-error: expected ':' or a line ending after the function signature, found 'Conn'
0/1 clean
```

### Block 7

```console
error: FILE:1:21: syntax-error: expected ',' or ')', found 'List'
0/1 clean
```

### Block 8

```console
error: FILE:1:26: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 9

```console
error: FILE:3:14: syntax-error: expected a type, found '^'
error: FILE:6:16: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 10

```console
error: FILE:1:10: syntax-error: expected a generic parameter name, found '^'
error: FILE:2:15: syntax-error: expected a line ending, found 'Conn'
error: FILE:3:13: syntax-error: expected a type, found '^'
error: FILE:5:15: syntax-error: expected ',' or ')', found 'Conn'
error: FILE:11:37: syntax-error: expected ':' or a line ending after the function signature, found 'Bag'
0/1 clean
```

### Block 11

```console
error: FILE:1:36: syntax-error: expected ':' or a line ending after the function signature, found 'Conn'
0/1 clean
```

### Block 12

```console
error: FILE:4:15: syntax-error: expected a parameter name, found '^'
error: FILE:9:30: syntax-error: expected a type, found '^'
0/1 clean
```

Every erased run returned exit status 0 and `1/1 clean`. Raw rejections
returned status 101; O1 returned status 0. The parser was the repository
`hd_syntax` parser, accessed through the required conformance adapter.

## Repository Validation

Origin was fetched, and the initial clean tracked checkout was rebased
onto `origin/main`. Other work subsequently changed the shared checkout.
Validation therefore used an isolated worktree at `5704772a8`, rebased
onto `origin/main`, with only this report and its README entry copied in.
The reviewed ownership record was unchanged from the pinned design.

| Check | Result |
| --- | --- |
| Raw report examples | O1 parses; O2–O12 reject unsupported proposed syntax |
| Erased report examples | All twelve parse |
| `bash spec/check.sh` | Passed; links and anchors, zero failing dead citations, 5,971 rule IDs, 1,563 diagnostic rows |
| `pnpm run website:build` | Passed; 79 pages, 265 grammar rules, 266 definitions |
| `git diff --check` | Passed |

The documentation checks report eight existing dead-citation warnings;
none is a failing citation. No ownership implementation or runtime safety
test is claimed. Only this report and its README entry were written by
this review; concurrent compiler and fixture edits were left untouched.
No design, spec, fixture, or compiler change was made, committed, or pushed
by this review.
