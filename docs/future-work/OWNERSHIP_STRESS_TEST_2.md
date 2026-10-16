# Ownership And Borrows: Adversarial Review 2

Status: design review, 2026-10-16; changes no decision, design record,
spec text, or compiler code. Nothing here is accepted behavior.
This reviews [Ownership And Borrows](OWNERSHIP_DESIGN.md) at commit
`bdade3ba81a577a726666a2191cbee686b0cc589`, against the
[first ownership review](OWNERSHIP_STRESS_TEST.md).

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

The owner now chose destructors. The earlier no-destructor restriction,
including the stale shared-skills guidance, no longer applies to this
design. This review respects the new choice and does not reopen linear
versus affine ownership or call-site move marks.

| Chosen change | Consequence under test |
| --- | --- |
| Owned values are unique, not linear; scope exit drops them | Explicit closing and defer reservation are no longer necessary for ordinary resources |
| `DropOwn` types cannot be held plain | A resource's destructor cannot be bypassed merely by choosing a plain producer result |
| Owned holders propagate ownership | B7's original plain-box example rejects |
| Ordinary `[T]` excludes owned types; `[T < own Any]` accepts them and moves values | B8's direct owner-duplication example rejects |
| Borrowed fields are exempt from fresh uniqueness | An owner can hold a resource and borrow another object |
| Exclusive captures make closures one-shot | Two calls cannot duplicate one exclusive closure environment |
| Destruction covers cancelled frames, completed-result buffers, discarded elements, and owning cursors | The first review's missing-consumer attacks now have a common answer |
| Later top-level inputs may move top-level owners | REPL handles can close before the session ends |
| Plain aliases and plain captures remain unchecked | Former B5 is an explicit guarantee boundary, not an unchosen repair |

The dependent language-tier sections are [access and generics](../../spec/lang/04-type-system.md),
[iteration and cleanup](../../spec/lang/06-control-flow.md),
[functions and captures](../../spec/lang/07-functions.md),
[data and visibility](../../spec/lang/08-data-and-enums.md),
[traits and dynamic values](../../spec/lang/09-traits.md),
[built-in list methods](../../spec/lang/10-modules.md#built-in-methods),
and [providers and suspension](../../spec/lang/11-requirements-and-suspension.md).
The stdlib-tier dependencies are [collections](../../spec/std/collections.md),
[iterators](../../spec/std/iter.md), and [tasks](../../spec/std/task.md).
These chapters provide the existing integration surface, not an
implementation of the proposal.

`File`, `Conn`, `connect!`, and resource methods use the study's contracts.
`tick!` represents a suspending operation. No particular platform handle
representation or host leak is assumed.

## Method

Ten cases cover regression controls and interactions introduced by
destruction. The foreign examples approximate library shapes; they are
not verbatim library source. hd examples are parsed raw and with proposed
syntax erased, as recorded below.

| Verdict | Meaning |
| --- | --- |
| Works | The written rules provide the expected behavior or rejection |
| Friction | A sound restriction has a practical workaround or exposes a declared open library question |
| Breaks | Rules conflict or a required safety contract is missing |

The compiler does not implement this proposal. A missing contract is
not a demonstrated accepted exploit. Where an implicit interpretation
would reject a probe, this report states that interpretation instead
of asserting inevitable acceptance.

The same eleven dimensions remain. Migration, annotation counts,
writing trials, and performance measurements are excluded. Verdict
counts across the two reports are not comparable scores: the cases
changed with the model.

## Summary

| Case | Library shape | Verdict | Main result |
| --- | --- | --- | --- |
| U1 | Concurrent acquisition and vector clearing | Works | Destructors supply the missing consumer for buffers and discarded elements |
| U2 | Generic duplication and owning boxes | Works | Original B8 and B7 attacks reject |
| U3 | Borrowed document plus owned output; validated transfer | Works | Borrow-field exemption and ordinary drop remove both earlier obstacles |
| U4 | Moving a field out of a destructor-bearing object | Breaks | Custom destructor cannot receive the partially moved holder |
| U5 | Inspector whose destructor reads a borrowed resource | Breaks | Loan liveness must include implicit destructor use |
| U6 | A static accessor over a movable global owner | Breaks | `static` needs a rule preserving the borrowed owner across later top-level moves |
| U7 | Generic duplication of a mutable-borrow holder | Breaks | Ordinary generic admissibility must exclude structurally move-only values too |
| U8 | Mutable context provider used by sibling tasks | Breaks | Exclusive provider loans remain unspecified |
| U9 | Lazy map capturing an owner | Friction | One-shot captures require a named producer representation for repeated pulls |
| U10 | Reassignment and ownership-polymorphic results | Breaks | Stale rules and an example contradict the recorded decisions |

The result is **three works, one friction, and six breaks**. U10 is an
editorial consistency failure. U4–U8 identify incompatible or missing
contracts, not five proven compiler vulnerabilities.

This revision is a substantial improvement. Destruction replaces
several unrelated cleanup workarounds with one mechanism. The remaining
work concerns the interaction of moves, loans, and destruction, plus
the structural meaning of a duplicable generic argument.

## Cases

### U1. Buffered Owners And Discarded Elements Now Have Cleanup

Original shapes: concurrent resource acquisition and clearing a vector.

```rust
let (a, b) = tokio::join!(acquire("a"), acquire("b"));
connections.clear();
```

```text
use std.task.all

fn acquire!(host: string) -> own Conn:              # hypothetical syntax
    connect!(host)

fn both!() -> void:
    let (a, b) = all!(acquire("a"), acquire("b"))
    a.send!([1])
    b.send!([2])

fn discard(xs: ^mut List[own Conn]) -> void:        # hypothetical syntax
    xs.clear()
```

**Generated behavior.** `all!` owns each completed result until handing
it to the parent. Cancellation drops the buffer and any pending owned
children. `clear` drops removed owned elements rather than merely
truncating reference storage.

**Verdict: works**, at the chosen semantics. The earlier OWN-S1 failure
is repaired, not merely hidden by a new rejection. The implementation
still needs initialized-state flags and exactly-once cleanup, including
after an explicit `cancel` followed by dropping the frame.

No implicit `Result.Err` short-circuit is inferred: existing `all!`
waits for its children, and an error result is an ordinary returned value
unless another contract changes that. Parent propagation later drops
the other owned result too. Panic remains the study's explicit exception.

[Rust destruction](https://doc.rust-lang.org/reference/destructors.html)
and [Vec.clear](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.clear)
are the reference shapes. hd now proposes the corresponding cleanup
obligation explicitly.

### U2. The Original Generic And Plain-Holder Attacks Reject

Original shape: copying requires a duplicable type, and an owning
aggregate retains ownership of its field.

```rust
fn duplicate<T: Copy>(x: T) -> (T, T) { (x, x) }
struct Boxed { conn: Conn }
```

```text
fn duplicate[T](x: T) -> (T, T): (x, x)

fn reject_duplicate(c: own Conn) -> void:           # hypothetical syntax
    _ := duplicate(c)

data Boxed:
    conn: own Conn                                # hypothetical syntax

fn inferred_owner(c: own Conn) -> void:            # hypothetical syntax
    box := Boxed { conn: c }
```

**Generated behavior.** The ordinary generic cannot instantiate `T` with
`own Conn`. Constructing `box` moves `c`, makes the holder owned, and
dropping `box` drops the connection.

**Verdict: works.** Explicitly forcing a plain `Boxed` destination must
instead reject. There is no reason to keep calling the original B7/B8
attacks unresolved. U7 tests a different, structural edge of generic
admissibility.

### U3. Mixed Owned/Borrowed Objects And Post-Await Transfer

Original shape: Rust packages an owned resource with a borrowed input,
then transfers the package after validation.

```rust
struct Export<'a> { out: File, source: &'a Document }
async fn prepare(source: &Document) -> Export<'_> {
    let out = acquire().await;
    validate(&out).await;
    Export { out, source }
}
```

```text
data Export[^doc]:                                 # hypothetical syntax
    out: own File                                 # hypothetical syntax
    source: ^{doc} Document                        # hypothetical syntax

fn prepare!(d: ^Document) -> own Export[^{d}]:      # hypothetical syntax
    out := open("out")
    check!(out)
    Export { out: out, source: d }
```

**Generated behavior.** Cancellation during `check!` drops `out`.
Success moves it into the result; the borrowed document remains
bounded by `d`. No reserved binding prevents the transfer.

**Verdict: works.** The direct mixed-field restriction and the earlier
reservation problem are both resolved. Generic destruction must still
respect any borrowed data a custom destructor can inspect; U5 isolates
that additional obligation.

### U4. Partial Move Followed By A Custom Destructor

Original shape: Rust rejects moving a field from a type implementing
`Drop`, because its destructor may inspect that field.

```rust
struct Traced { file: File }
impl Drop for Traced {
    fn drop(&mut self) { inspect(&self.file); }
}
// let file = traced.file; // E0509
```

```text
data Traced:
    file: own File                                # hypothetical syntax

impl DropOwn for Traced:
    fn drop(^mut self) -> void:                    # hypothetical syntax
        _ := size(self.file)

fn split(p: own Traced) -> void:                   # hypothetical syntax
    file := p.file
    drop(file)
```

**Generated behavior.** Rule 4.2.4 allows the field move through an owned
path. At scope exit, 4.5.3 calls the holder's destructor before dropping
remaining fields. That destructor requires an initialized whole `self`,
but `p.file` has already moved and been closed.

**Verdict: breaks; conflicting rules.** Field drop flags prevent a
second field drop. They do not make a custom whole-object destructor
valid on an incomplete object. If implicit whole-object use rejects
`split`, that rejection needs a rule and a diagnostic.

This applies to destructuring too. Moving an entire owner into another
function remains valid; moving only its field is the issue.
Rust's [E0509 explanation](https://doc.rust-lang.org/error_codes/E0509.html)
documents exactly this distinction. This is not merely a choice of
generic drop dispatch strategy.

### U5. Last Explicit Use Is Earlier Than Destructor Use

Original shape: an inspector's destructor observes borrowed data.

```rust
struct Watch<'a>(&'a File);
impl Drop for Watch<'_> {
    fn drop(&mut self) { inspect(self.0); }
}
```

```text
data Watch[^a]:                                    # hypothetical syntax
    file: ^{a} File                               # hypothetical syntax

impl[^a] DropOwn for Watch[^{a}]:                   # hypothetical syntax
    fn drop(^mut self) -> void:                    # hypothetical syntax
        _ := size(self.file)

fn destroy_early() -> void:
    file := open("input")
    let view: ^File = file                         # hypothetical syntax
    let own watch = Watch { file: view }           # hypothetical syntax
    drop(file)
```

**Generated behavior.** The last explicit use of the borrowed value is
construction of `watch`. Its custom destructor runs later, at scope
exit, and reads that same file. Closing the file in between must reject.

**Verdict: breaks; required lifetime contract is unstated.** Rule 4.3.5
can work if “last use” includes implicit destructor execution and all
transitive owned drop glue that may inspect the borrow. Reading it as
last source use would admit use after close.

The generic form needs modular treatment: a destructor may inspect a
borrow through a callback or trait method, not only a visible field
read. Rust calls this [drop checking](https://doc.rust-lang.org/nomicon/dropck.html).
The study's generic-dispatch open point does not yet state this lifetime
rule. No unsafe escape hatch is needed for an initial conservative rule.

### U6. A Static Accessor Over A Movable Global

Original shape: a Rust static accessor relies on a stable static owner.

```rust
static VALUE: u32 = 7;
fn current() -> &'static u32 { &VALUE }
```

hd additionally permits later top-level statements to move global owners:

```text
GLOBAL := open("global")

fn current() -> ^{static} File:                    # hypothetical syntax
    let view: ^File = GLOBAL                       # hypothetical syntax
    view

fn read_after(file: ^File, ignored: void) -> usize: # hypothetical syntax
    size(file)

read_after(current(), drop(GLOBAL))
```

**Generated behavior.** The first argument produces a static borrow.
The second argument moves and closes its global owner. The callee then
uses the borrow. This example stores no borrow in a top-level binding,
so the ban on such bindings does not resolve it.

**Verdict: breaks; missing global-loan contract.** Sound checking must
reject either the static accessor or the intervening move. Merely
checking ordinary variable use in statement order sees no second
syntactic use of `GLOBAL` after `drop(GLOBAL)`.

The accessor's signature exports `static`, not a dependency naming
`GLOBAL`. A body-independent caller therefore needs a conservative
restriction or an exported/global loan relation. Direct local borrowing
of a global is easier because the root remains visible.

[Rust statics](https://doc.rust-lang.org/reference/items/static-items.html)
have static storage and do not run destructors at program end. hd's
chosen movable top-level ownership differs and needs its own rule.
The owner decision to permit top-level moves is preserved here.

### U7. An Unowned Holder Can Still Be Move-Only

Original shape: a Rust wrapper around a mutable reference cannot be
copied, even though it owns no resource.

```rust
struct Cursor<'a> { source: &'a mut Document }
fn duplicate<T: Copy>(x: T) -> (T, T) { (x, x) }
```

```text
data Cursor[^a]:                                   # hypothetical syntax
    source: ^{a} mut Document                     # hypothetical syntax

fn duplicate[T](x: T) -> (T, T): (x, x)

fn duplicate_cursor(d: ^mut Document) -> void:      # hypothetical syntax
    cursor := Cursor { source: d }
    let (a, b) = duplicate(cursor)
```

**Generated behavior.** The holder-moves decision makes `Cursor`
nonduplicable. Yet its outer type is not written `own`, and receiving a
borrow does not trigger the owned-field construction rule. Excluding
only outer `own` arguments from `[T]` would let the generic duplicate
the exclusive loan.

**Verdict: breaks; generic admissibility needs a structural definition.**
If “plain types” already means structurally duplicable types, this probe
rejects correctly. The study must say so, including nested wrappers,
tuples, collections, and lifetime-bearing dynamic values. That reading
is stronger than “a plain type parameter never takes an owned type.”

The closure rules have the same edge: capturing this nominal holder
cannot use unchecked shared-storage capture merely because its outer
spelling lacks `own`. Either it is classified as owned or its move-only
status must propagate independently through capture and storage.

Rust's [Copy documentation](https://doc.rust-lang.org/std/marker/trait.Copy.html)
explains the structural issue and why mutable references cannot copy.
This does not require adding a public `Copy` trait to hd.

### U8. Exclusive Mutable Providers Still Need A Contract

Original shape: an exclusive mutable service borrow stays exclusive
while the asynchronous operation using it remains pending.

```rust
async fn emit(sink: &mut Sink) { sink.write().await; }
```

```text
use std.task.all

trait Sink:
    fn write!(^mut self) -> void                   # hypothetical syntax

fn emit!() -> void $ Sink:
    $.use(Sink).write!()

fn both!(sink: ^mut dyn Sink) -> void:             # hypothetical syntax
    $.with(Sink=sink):
        _ := all!(emit(), emit())
```

**Generated behavior.** Both child frames capture the same provider.
If retrieval yields a shared borrow, it cannot call `^mut self`. If
both children retain mutable provider borrows while pending, exclusivity
is violated. Destructors do not change either result.

**Verdict: breaks; the previous OWN-S2 provider issue remains.** The
existing [Mutable Providers](../../spec/lang/11-requirements-and-suspension.md#mutable-providers)
section expressly shares mutable providers across sibling tasks. The
new study states only the key-bounded lifetime and borrowing scope.

An access distinction could reject overlapping exclusive uses while
preserving ordinary shared services. The exact installation, retrieval,
and frame-capture rules are missing. This does not imply that all
provider use must become exclusive or that `$ mut Sink` is valid syntax.

### U9. A Lazy Mapper Capturing An Owner Cannot Be Repeatedly Called

Original shape: Rust uses a reusable mapping closure that retains an
owned lookup resource and only borrows it during each call.

```rust
let table = load_table();
let mapped = ids.into_iter().map(move |id| table.lookup(id));
```

```text
fn lazy_lookup(ids: own List[usize], table: own Table) -> void: # hypothetical syntax
    mapped := ids.into_iter().map(fn(id: usize) -> string:
        table.lookup(id)
    )
    _ := mapped.next()
    _ := mapped.next()
```

**Generated behavior.** Capturing `table` makes the callback `own fn`,
even though its body never moves the table out. Calling it once consumes
it and eventually drops its capture. A mapper cannot call that same
callback once for each of several elements.

**Verdict: friction; an acknowledged higher-order design question.**
The owner already chose named data for repeated exclusive work. A named
mapper that owns `table` and exposes a `^mut self` step can support the
behavior. The current closure-backed `Iterator[T]` cannot adopt that
representation simply by adding ownership bounds to its type parameters.

The existing source is [Iterator.map](../../lib/std/iter.hd); its stored
step closure is reusable. The relevant std contract is
[Iterator Adapters](../../spec/std/iter.md#iterator-adapters).
[Rust Iterator.map](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map)
accepts `FnMut`, and [Rust closure rules](https://doc.rust-lang.org/reference/types/closure.html#call-traits-and-coercions)
distinguish moving a capture into the environment from moving it out
on invocation. hd deliberately chose a more conservative interface.

This does not reopen that decision. It asks for a demonstrated named
producer representation before claiming every method remains available.

### U10. The Record Still Contains Contradictory Instructions

Original shape: ordinary Rust assignment drops the previous owned value;
a generic result retains its complete instantiated type.

```rust
file = replacement;
fn identity<T>(x: T) -> T { x }
```

```text
fn replace_owned() -> void:
    let file = open("first")
    file = open("second")

fn take_first[T < own Any](xs: own List[T]) -> Option[own T]: # hypothetical syntax
    xs.into_iter().next()
```

**Generated behavior.** The first body is `own-overwritten` under 4.2.2,
but drops the first file and accepts under 4.5.2. The second signature
is the study's own 3.8 example, but 4.6.4 forbids writing `own T`.
Its bound also admits plain primitives and already-owned types, making
an unconditional extra `own` inappropriate.

**Verdict: breaks; editorial consistency.** The chosen reassignment rule
already supplies the answer. A result `Option[T]` would preserve the
element's complete mode, subject to the chosen owning-container rule.
No owner decision or new syntax is required to remove those conflicts.

| Additional stale text | Conflict |
| --- | --- |
| 4.6.12 unconditionally forbids moving a top-level owner | Decisions So Far and 3.11 allow later top-level statements to move it |
| Section 2 still calls B7/B8 open holes | The appendix correctly says their original attacks are closed |
| 4.5.7 describes a plain value whose type implements `DropOwn` | 4.1.10 rejects that position |
| “A result without `^` carries no lifetime” in 3.3 | A named type can carry declared lifetime arguments, as 3.4 demonstrates |

These weaken agent writability and spec readiness, but should not be
reported as independent runtime exploits. The motivating Rust behavior
is documented in [destruction](https://doc.rust-lang.org/reference/destructors.html);
[mem.replace](https://doc.rust-lang.org/std/mem/fn.replace.html) additionally
illustrates an initialized replacement protocol for extracting a field.

## Comparison With Other Languages

| Boundary | Evidence | Relevance to this revision |
| --- | --- | --- |
| Custom destructor after a field move | [Rust E0509](https://doc.rust-lang.org/error_codes/E0509.html) rejects it | Drop flags alone do not solve whole-object destructor access |
| Implicit use at destruction | [Rust drop checking](https://doc.rust-lang.org/nomicon/dropck.html) constrains borrowed data | A last-source-use rule is insufficient for destructor-bearing holders |
| Move-only borrowed holders | [Rust Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html) is structural | Generic and capture admissibility must follow move-only status, not just outer `own` spelling |
| Persistent owner inside a reusable callable | [Rust call traits](https://doc.rust-lang.org/reference/types/closure.html#call-traits-and-coercions) depend on invocation behavior | hd's one-shot rule is conservative and needs named producer counterparts |
| Global borrowed lifetime | [Rust statics](https://doc.rust-lang.org/reference/items/static-items.html) have stable static storage | hd permits more movement at top level and must preserve loan validity across it |

The argument is not that hd must copy every Rust restriction. These
sources identify specific safety obligations hidden by the phrase
“Rust's model.” Alternative rules must discharge the same obligations.

## Problems, Ranked

Priority follows potential use-after-close or duplicate exclusive access,
then inconsistent instructions and acknowledged library costs. All
candidate questions respect the owner's current decisions. Ordinary
library changes are preferred when they can actually provide the needed
behavior; language safety contracts cannot be replaced by documentation.

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | OWN-S5 | Destruction must see initialized values and live borrowed data | High safety contract | U4, U5 |
| 2 | OWN-S6 | Generic/capture admissibility must include move-only borrowed holders | High structural contract | U7 |
| 3 | OWN-S2 remaining | Mutable provider access across pending children | High unresolved integration | U8 |
| 4 | OWN-S7 | Static accessor versus a movable top-level owner | High global-lifetime contract | U6 |
| 5 | OWN-S8 | Contradictory rules and examples | Moderate editorial defect | U10 |
| 6 | OWN-S9 | Reusable owning producers need a named representation | Moderate, already declared open | U9 |

### OWN-S5. Destruction Requires More Than A Drop Flag

**Effect.** U4 moves a field that a later custom destructor reads. U5
destroys a referent before a later implicit read. The first concerns
initialization; the second concerns loan liveness. Both apply through
generic and dynamic wrappers.

| Candidate question | Cost and tier | Effect |
| --- | --- | --- |
| Should partial field moves and consuming destructuring of `DropOwn` types reject, while implicit drop keeps relevant borrows live through execution? | Two language semantic rules; language tier | Conservative baseline, no new syntax or intrinsic |
| Should field extraction instead require replacement that leaves the holder initialized, with the same destructor-loan rule? | Existing container-removal APIs where sufficient; otherwise a compiler intrinsic for atomic replacement, plus language drop-liveness and initialization rules | Useful API; cannot eliminate the underlying safety rules or be assumed implementable in ordinary hd today |

Drop flags and these checks are complementary. Advanced precision that
proves particular destructor bodies ignore particular fields can wait.
The initial contract must also cover owned fields whose own destructors
inspect a borrow.

### OWN-S6. “Plain” Must Not Accidentally Mean “Outer Type Lacks own”

**Effect.** U7 owns no resource, yet duplicates an exclusive borrow if
admissibility checks only the outer mode. The same nominal holder cannot
become an unchecked shared capture. The original direct B8 attack stays
closed.

| Candidate question | Cost and tier | Effect |
| --- | --- | --- |
| Should ordinary `[T]` and plain copying impls require structural duplicability, with the existing ownership-enabled bound admitting move-only holders? | A general language semantic classification, applied to generics, copying methods, and captures | No new spelling; preserves the owner's generic default |
| Should holders containing mutable borrows always become explicitly owned, propagating that classification through wrappers? | A language semantic ownership-propagation extension | Reuses the existing outer-mode check; still must preserve the contained borrow lifetime |

These options need no new public marker trait. The cheapest adequate
answer is a uniform existing-mode rule, rather than independent special
cases for each collection or callback.

### OWN-S2 Remaining. Provider Lifetime Does Not Define Provider Access

**Effect.** U8 is unchanged in substance from the previous review.
One-shot closures fix the explicit captured-callable case, but an
implicit requirement key is not that callable's owned capture.

| Candidate question | Cost and tier | Effect |
| --- | --- | --- |
| Should exclusive provider retrieval and frame capture use the existing mutable-loan rules, rejecting simultaneous retained access? | Language semantic provider/access rules | Reuses ownership; shared services remain shared |
| Should services requiring concurrent sharing expose shared-receiver operations backed by ordinary shared state, while exclusive providers remain nonoverlapping? | Ordinary stdlib/application representation plus the required language distinction | Cheapest user-facing adaptation where shared semantics are appropriate |

Runtime single-thread scheduling does not terminate a loan held by a
pending operation. A complete contract must specify installation,
retrieval, nested access, and cold-frame capture.

### OWN-S7. A Static Borrow Cannot Outlive Its Dropped Owner

**Effect.** U6 exploits the difference between a global's syntactic
binding lifetime and its owned value's lifetime. A return contract that
exports only `static` loses the particular owner dependency.

| Candidate question | Cost and tier | Effect |
| --- | --- | --- |
| Should an owned global exposed through a static borrow become immovable, while other top-level owners may still move? | One language semantic restriction and global exposure tracking | Preserves the chosen REPL behavior for unexposed owners; conservative |
| Should global borrowing export an owner-root relation using the existing named-lifetime mechanism? | Language semantic lifetime/contract extension; new syntax only if existing names cannot express it | More precise; must work across function boundaries and statement evaluation |

This question does not propose banning all top-level moves. It asks
which loan contract makes the chosen movement sound.

### OWN-S8. Align The Record With Decisions Already Made

**Effect.** U10 gives incompatible answers without requiring any
implementation. A fresh agent cannot consistently follow both sections.

**Candidate question.** Can the next editorial pass remove the stale
overwrite/top-level prohibitions, use the complete `T` in `take_first`,
and qualify the plain-value and lifetime statements? This is a design
record correction with no new language mechanism and no owner policy
choice. This review does not edit the record.

### OWN-S9. Demonstrate A Reusable Named Producer

**Effect.** U9 exposes the cost of the accepted one-shot closure rule,
not a reason to reverse it. Existing `Iterator[T]` stores a reusable
closure, so ownership bounds alone cannot adapt its implementation.

| Candidate question | Cost and tier | Effect |
| --- | --- | --- |
| Should ownership-aware adapters use named data with `^mut self` step methods and an explicit owned producer representation? | Ordinary stdlib representation, with language-tier changes only where the compiler-known iterator protocol requires them | Reuses the chosen interface for repeated exclusive work |
| Should the first ownership surface support eager named-data loops while the general lazy adapter protocol remains open? | Ordinary stdlib API and an explicit restriction | Smaller initial surface; documents the lazy-use limitation |

Iteration, trait receivers, and higher-order signatures are already
listed as open. Settle the ownership core before adding a new callable
kind to make these cases convenient.

## Evaluation By Dimension

| Dimension | Change since review 1 | Current assessment |
| --- | --- | --- |
| Guarantee clarity | Major improvement: deterministic destruction replaces mandatory explicit consumption | Separate deterministic drop, resource-specific closing, panic release, and plain aliasing; “no known hole” overstates the completed safety argument |
| Soundness | Original B7/B8 and missing consumers have chosen answers; explicit exclusive closures cannot be called twice | Destructor initialization/liveness, structurally move-only holders, global loans, and providers still require contracts |
| Precision | No reservation barrier; conditional moves use drop flags | Better direct transfer and mixed-field precision; drop-aware loans can last longer than ordinary last-use loans |
| Practical expressiveness | Cold owned frames, cancelled buffers, early iterator exit, and ordinary post-await handoff become practical | Named reusable producers and ownership-aware iteration/HOF contracts remain unfinished; async/fallible cleanup stays explicitly manual |
| Composability | One destructor rule repairs several previously unrelated storage boundaries | Stronger overall; ownership and loan metadata must survive partial moves, generic wrappers, implicit keys, and dynamic erasure |
| Modular contracts | Plain versus ownership-enabled generics is a useful explicit boundary | Needs structural admissibility, drop-liveness, provider access, and global-borrow summaries; drop dispatch is explicitly open |
| Agent writability and human readability | Fewer manual cleanup obligations and one owned mutation form | Improved, but contradictory rules currently produce incompatible programs; one-shot owner capture has a visible library cost; no writing trial performed |
| Conceptual cost | A single cleanup mechanism replaces reservation, linear exits, and special discarded-frame restrictions | Better unification despite adding destructors; drop checking and structural move-only classification remain real concepts to explain |
| Compiler feasibility and cost | Ordinary drop elaboration and flags give a mature implementation path | More concrete than before; requires partial-initialization and drop-aware loan checks, cancellation lowering, and dynamic/generic glue; no benchmark |
| Optimization validity and payoff | Owned/exclusive positions still provide useful potential facts | Plain collection elements and captures remain shared by choice. Infer no transitive or universal no-alias property; destructor side effects also constrain motion |
| Specification readiness | Core owner choices are much more settled | Closer, but not ready for a complete spec pass: safety interactions and contradictory rules precede the open iterator/library work |

The judgment is **substantial progress through unification, with the
remaining safety argument concentrated at destruction and abstraction
boundaries**. The earlier “incomplete linear core” assessment is obsolete:
the owner intentionally chose a different, destructor-based core.

`DropOwn` is described as a std trait, but its compiler-recognized
behavior and prohibition on plain instances belong to the language
tier under the repository's tier test. Ordinary guards and adapters
implemented over that behavior belong to the stdlib tier.

The chosen plain-element and plain-capture semantics narrow the alias
guarantee. The earlier `refill` graph concern should therefore be treated
as a guarantee-wording boundary, not an automatic claim of a new resource
escape: plain descendants are intentionally shareable.

## Earlier Findings

| First-review finding | Updated status |
| --- | --- |
| O1 basic close/defer workflow | Replaced by simpler destructor-based cleanup; explicit consuming `drop` rejects subsequent use |
| O2 independent lifetime-bearing identity | Remains plausible for duplicable shared-borrow holders; structural generic admissibility must be explicit |
| O3 / B8 direct generic owner duplication | Fixed by the owner decision; U7 is a separate mutable-borrow-holder follow-up |
| O4 / B7 plain holder | Fixed by ownership propagation and dropping owned fields |
| O5 / B5 plain closure bypass | Accepted guarantee boundary. Do not continue counting it as an unresolved soundness promise |
| O6 buffered owned result | Fixed by owned result-buffer destruction, tested in U1 |
| O7 abandoned owning iterator tail | Cleanup mechanism supplied; exact iteration protocol remains openly undecided |
| O8 clear/truncate discard | Owned elements are dropped; copying methods are plain-only. Broad method availability still needs the open signatures |
| O9 exclusive closure overlap | Fixed by one-shot exclusive captures; U9 measures the accepted expressiveness cost |
| O10 owned plus borrowed fields | Fixed by the borrowed-field exemption; plain graph sharing now needs careful guarantee wording |
| O11 validation then transfer | Fixed directly by implicit destruction; a cleanup slot is no longer necessary for the basic case |
| O12 mutable provider sharing | Remains unresolved; U8 and OWN-S2 retain that finding |

## Parse Log

Every one of the ten `text` blocks was checked with
`scripts/hd-conformance.sh parse FILE`. All ten raw blocks reject
unsupported proposed syntax at marked lines. All ten erased blocks
parse. No type-checking, lifetime check, or runtime execution is claimed.

Erasure operates only on code before a comment. It removes lifetime
parameter lists, `^{...}` bounds, bare `^`, and `own`, then removes
empty named lifetime applications. It preserves ordinary generic
arguments, executable operations, comments, and line breaks.

| Block | Case | Raw | Erased |
| --- | --- | --- | --- |
| 1 | U1 | Rejects proposed syntax | Parses |
| 2 | U2 | Rejects proposed syntax | Parses |
| 3 | U3 | Rejects proposed syntax | Parses |
| 4 | U4 | Rejects proposed syntax | Parses |
| 5 | U5 | Rejects proposed syntax | Parses |
| 6 | U6 | Rejects proposed syntax | Parses |
| 7 | U7 | Rejects proposed syntax | Parses |
| 8 | U8 | Rejects proposed syntax | Parses |
| 9 | U9 | Rejects proposed syntax | Parses |
| 10 | U10 | Rejects proposed syntax | Parses |

The raw diagnostics follow verbatim, with the temporary pathname
replaced by `FILE`. All raw runs returned status 101. Each erased run
returned status 0 and `1/1 clean`.

### Block 1

```console
error: FILE:3:34: syntax-error: expected ':' or a line ending after the function signature, found 'Conn'
error: FILE:11:16: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 2

```console
error: FILE:3:28: syntax-error: expected ',' or ')', found 'Conn'
error: FILE:7:15: syntax-error: expected a line ending, found 'Conn'
error: FILE:9:26: syntax-error: expected ',' or ')', found 'Conn'
0/1 clean
```

### Block 3

```console
error: FILE:1:13: syntax-error: expected a generic parameter name, found '^'
error: FILE:2:14: syntax-error: expected a line ending, found 'File'
error: FILE:3:13: syntax-error: expected a type, found '^'
error: FILE:5:16: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 4

```console
error: FILE:2:15: syntax-error: expected a line ending, found 'File'
error: FILE:5:13: syntax-error: expected a parameter name, found '^'
error: FILE:8:17: syntax-error: expected ',' or ')', found 'Traced'
0/1 clean
```

### Block 5

```console
error: FILE:1:12: syntax-error: expected a generic parameter name, found '^'
error: FILE:2:11: syntax-error: expected a type, found '^'
error: FILE:4:6: syntax-error: expected a generic parameter name, found '^'
error: FILE:5:13: syntax-error: expected a parameter name, found '^'
error: FILE:10:15: syntax-error: expected a type, found '^'
error: FILE:11:13: syntax-error: expected ':' or '=' after the pattern, found 'watch'
0/1 clean
```

### Block 6

```console
error: FILE:3:17: syntax-error: expected a type, found '^'
error: FILE:7:21: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 7

```console
error: FILE:1:13: syntax-error: expected a generic parameter name, found '^'
error: FILE:2:13: syntax-error: expected a type, found '^'
error: FILE:6:24: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 8

```console
error: FILE:4:15: syntax-error: expected a parameter name, found '^'
error: FILE:9:16: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 9

```console
error: FILE:1:25: syntax-error: expected ',' or ')', found 'List'
0/1 clean
```

### Block 10

```console
error: FILE:5:23: syntax-error: expected ',' or ']', found 'Any'
0/1 clean
```

## Repository Validation

Origin was fetched. Checks used an isolated worktree at `bdade3ba8`,
rebased onto `origin/main`, with only this report and its README entry
copied in. The shared checkout's concurrent compiler edits were not
included, modified, or rebased.

| Check | Result |
| --- | --- |
| Ten raw report blocks | All reject proposed syntax at marked lines |
| Ten erased report blocks | All parse |
| `bash spec/check.sh` | Passed; links and anchors, zero failing dead citations, 5,963 rule IDs, 1,564 diagnostic rows |
| `pnpm run website:build` | Passed; 79 pages, 265 grammar rules, 266 definitions |
| Whitespace checks | Passed for the README diff and new report |

The spec check reported eight existing dead-citation warnings and six
citations allowed as history. None failed. The report's final validation
prose and candidate-cost clarification do not change any parsed block.
No ownership implementation or runtime validation is claimed.

Only this report and its README entry were written by this review.
The ownership record, specification, compiler, and fixtures were left
untouched. Nothing was staged, committed, or pushed.
