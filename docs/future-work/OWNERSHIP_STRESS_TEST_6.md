# Ownership And Borrows: Adversarial Review 6

Status: design review, 2026-10-16; changes no decision, design record,
spec text, or compiler code. Nothing here is accepted behavior.
This reviews [Ownership And Borrows](OWNERSHIP_DESIGN.md) at `5d9e3f7d6`,
including decisions `5f6412d66` and `5130db6cd`, against
[review 5](OWNERSHIP_STRESS_TEST_5.md).

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

The revision resolves all three review-5 findings. Universal generic
checking now distinguishes potentially owned values from shared references
and plain factories. Guarded patterns inspect through shared access and
commit moves only after success. Replaceable owners use built-in Option.take;
general replace remains absent by decision.

The owner also confirmed module-local OwnOnly implementations and the
classification of an `own U` argument as owning. The following policies
remain fixed: no partial moves, destructor-aware borrow liveness,
one-shot exclusive captures, plain providers, and immovable function-borrowed
global owners. Migration is excluded.

The language integration surface includes [types](../../spec/lang/04-type-system.md),
[global initialization dependencies](../../spec/lang/03-names-and-scopes.md#initialization-order),
[guards and cleanup](../../spec/lang/06-control-flow.md),
[function evaluation and captures](../../spec/lang/07-functions.md),
[data](../../spec/lang/08-data-and-enums.md),
[traits](../../spec/lang/09-traits.md),
[module initialization](../../spec/lang/10-modules.md#module-initialization),
[suspension](../../spec/lang/11-requirements-and-suspension.md), and
[derivation](../../spec/lang/14-annotations.md#typed-derivation).
The stdlib surface includes [option](../../spec/std/option.md),
[collections](../../spec/std/collections.md),
[iteration](../../spec/std/iter.md), and [tasks](../../spec/std/task.md).

These chapters do not implement ownership. Their general rules still
matter: global dependency analysis already follows implicit calls.
The new destructor machinery must integrate with that existing analysis.

## Method

Ten cases approximate Rust standard-library and scopeguard workflows.
The cases include earlier failures, nested mode boundaries, cancellation,
global shutdown, and an always-owned generic wrapper. Foreign snippets
are explanatory approximations, not copied library implementations.

| Verdict | Meaning |
| --- | --- |
| Works | The chosen rules determine the intended operation or rejection |
| Friction | A conservative integration or existing workaround is available, with a visible cost or contract to pin down |
| Breaks | No consistent safe answer follows without changing a chosen policy |

Counts are by case. An integration obligation is not counted as an
accepted-program exploit. Every hd block is parsed raw and after erasing
proposed syntax. Parsing does not establish ownership semantics, runtime
safety, performance, or agent-writing accuracy.

## Summary

| Case | Reference shape | Verdict | Result |
| --- | --- | --- | --- |
| Z1 | Shared-reference and factory duplication | Works | The revised mode predicate resolves OWN-S17 |
| Z2 | Guarded consuming Option match | Works | False guards preserve the subject; consuming guards reject |
| Z3 | Option.take inside a Drop-owning wrapper | Works | Extraction leaves a whole, empty field; no partial move is needed |
| Z4 | Suspended match guard | Works | Cancellation precedes move commitment and drops the original owner |
| Z5 | Cleanup that consults a later global resource | Friction | Implicit destructor dependencies must participate in global analysis |
| Z6 | Reciprocal global cleanup dependencies | Friction | No sequential drop order satisfies both; conservative dependency rejection is needed |
| Z7 | Taking a resource before a suspending replacement | Friction | Cancellation can leave the caller's wrapper empty, although ownership remains safe |
| Z8 | Containers of references and wrappers of factories | Works | The same mode cutoffs must compose through structural classification |
| Z9 | Owned argument before a suspending sibling argument | Works | Partially evaluated calls need ordinary cancellation cleanup for their owners |
| Z10 | Always-owned generic with a custom destructor | Friction | A handwritten marker alone does not admit owned arguments under current 4.1.12 |

The result is **six works, four friction, and no new break**.
All previous findings have answers. The most consequential remaining
integration is global destruction: `static` borrows cannot remain usable
after their referents' destructors run. Existing implicit-call analysis
offers a conservative route without inventing another lifetime mechanism.

The model is increasingly ready for a checker prototype. The remaining
findings concern integration and practical costs, rather than requiring
a new ownership model. The global cases below are conditional counterexamples
to naive lowering, not claims that the complete specification must accept them.

## Cases

### Z1. Generic Copying Preserves Known Modes

Reference shape: shared references and plain factories can be duplicated
without duplicating an owned referent or result.

```rust
fn refs<T>(x: &T) -> (&T, &T) { (x, x) }
fn factories<T>(f: fn() -> T) -> (fn() -> T, fn() -> T) { (f, f) }
```

```text
fn refs[T < own Any](x: ^T) -> (^T, ^T):                  # hypothetical syntax
    (x, x)

fn factories[T < own Any](f: fn() -> T) -> (fn() -> T, fn() -> T): # hypothetical syntax
    (f, f)
```

**Generated behavior.** Both definitions check once for all admitted
T modes. Each shared-reference copy retains the input lifetime. Each
factory copy aliases the plain callable; calls may produce distinct
owners. An exclusively capturing `own fn` still cannot satisfy f's contract.

**Verdict: works.** Review 5's blanket-quantifier objection is resolved.
The potentially owning Boxed[T] duplication from that review still rejects.
[Rust Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html) supplies
the corresponding reference and function-pointer distinction.

### Z2. Guard Failure Preserves The Owner

Reference shape: inspect an optional file before choosing which consumer
receives it.

```rust
match input {
    Some(file) if ready(&file) => archive(file),
    Some(file) => retry(file),
    None => (),
}
```

```text
fn route(input: own Option[own File]) -> void:            # hypothetical syntax
    match input:
        .Some(file) if ready(file) => archive(file)
        .Some(file) => retry(file)
        .None => pass

fn invalid(input: own Option[own File]) -> void:          # hypothetical syntax
    match input:
        .Some(file) if consume_and_test(file) => pass
        .Some(file) => retry(file)
        .None => pass
```

Assume ready takes a shared borrow. archive, retry, and consume_and_test
consume owners; the last returns bool.

**Generated behavior.** ready receives shared access during inspection.
A false guard leaves input intact. Success commits the consuming pattern
and transfers the file into archive. invalid rejects the consuming guard
with the chosen move-in-guard diagnostic.

**Verdict: works.** The ownership-commit policy is now explicit. Primitive
payload reads must retain ordinary copying; the shared guard access is
protection against consuming a candidate, not a new arithmetic operand
type. The existing primitive-copy rule supplies that reading.

The [Rust guard rules](https://doc.rust-lang.org/reference/expressions/match-expr.html#match-guards)
give the same delayed-move reference shape.

### Z3. Option.take Does Not Tear Apart A Drop-owning Value

Reference shape: a wrapper retains its destructor while detaching an
optional resource.

```rust
struct Session { file: Option<File> }
impl Session {
    fn detach(&mut self) -> Option<File> { self.file.take() }
}
```

```text
data Session:
    file: own Option[own File]                           # hypothetical syntax

impl OwnOnly for Session                                # hypothetical syntax
impl DropOwn for Session:                               # hypothetical syntax
    fn drop(^mut self) -> void: note_session_end()        # hypothetical syntax

impl Session:
    fn detach(^mut self) -> own Option[own File]:         # hypothetical syntax
        self.file.take()
```

**Generated behavior.** take exclusively borrows the field, moves its
optional payload into the returned option, and stores None. Session
remains whole. Its later destructor runs normally; automatic field drop
sees an empty option. The returned owner closes at its eventual owner's drop.

**Verdict: works.** This is the selected container-removal operation,
not partial destructuring of Session. The built-in's semantics must leave
None, including when the input was already empty, as
[Rust Option.take](https://doc.rust-lang.org/std/option/enum.Option.html#method.take) does.
Its compiler-known status places that operation in the language tier.

### Z4. Suspension In A Guard Does Not Commit A Move Early

Reference shape: an asynchronous guard awaits a predicate over a borrow.

```rust
match input {
    Some(file) if ready_async(&file).await => archive(file),
    Some(file) => retry(file),
    None => (),
}
```

```text
fn route_async!(input: own Option[own File]) -> void:     # hypothetical syntax
    match input:
        .Some(file) if ready_async!(file) => archive(file)
        .Some(file) => retry(file)
        .None => pass
```

Assume ready_async borrows File and may suspend. Neither archive nor
retry needs a requirement row.

**Generated behavior.** The parent frame retains the owned input while
the predicate frame holds a shared loan. Cancellation ends the predicate
and its loan first, then drops input. A false result proceeds to retry;
a true result commits the move into archive.

**Verdict: works.** The guard rule and existing structured cancellation
compose without an extra ownership mode. The implementation needs a
distinct precommit state; it must not set the payload's move flag merely
because a pattern matched.

### Z5. An Implicit Destructor Can Read A Global During Shutdown

Reference shape: a Rust destructor may consult a static service, but
Rust does not automatically drop static items at program termination.

```rust
struct AuditOnDrop;
impl Drop for AuditOnDrop {
    fn drop(&mut self) { inspect(current_log()); }
}
// current_log() returns a reference to a static service.
```

```text
data AuditOnDrop: pass
impl OwnOnly for AuditOnDrop                             # hypothetical syntax

let AUDIT: own AuditOnDrop = AuditOnDrop {}               # hypothetical syntax
LOG := open("log")

fn current_log() -> ^{static} File: LOG                   # hypothetical syntax

impl DropOwn for AuditOnDrop:                            # hypothetical syntax
    fn drop(^mut self) -> void: inspect(current_log())    # hypothetical syntax
```

Assume open returns an owned File with a closing destructor. inspect
takes a shared borrow, touches the live handle, and has an empty row.
The impl appears after LOG, so its method can see the accessor and binding.

**Naive generated behavior.** Applying reverse binding order to these
global owners closes LOG before dropping AUDIT. AUDIT then obtains a
supposedly static borrow of the destroyed resource. LOG was never moved,
so the immovable-global rule alone does not prevent this sequence.

**Verdict: friction; required global-analysis integration.** The existing
initialization rules already follow implicit calls. If constructing or
retaining an owned global includes its future drop glue in that dependency
analysis, AUDIT's initialization must reject because LOG is not ready.
Putting LOG first then permits reverse-order cleanup to work in this example.

This review does not assert that the complete specification accepts
the unsafe ordering. It asks the ownership implementation to make the
implicit dependency explicit and preserve it through shutdown. A destructor
can create a global loan when it runs; its type need not hold that loan.

The [Rust static-item rule](https://doc.rust-lang.org/reference/items/static-items.html)
explicitly avoids automatic static destruction. hd's selected global drop
policy therefore needs additional integration; Rust's static-lifetime
argument cannot be transferred unchanged.

### Z6. Reciprocal Global Cleanup Defeats Any Simple Drop Order

Reference shape: two cleanup callbacks each need the other's service
to remain alive.

```rust
// Conceptual shutdown dependencies:
// drop(left)  -> inspect(right), then close(left)
// drop(right) -> inspect(left),  then close(right)
```

```text
data Left:
    fd: i32
data Right:
    fd: i32

impl OwnOnly for Left                                    # hypothetical syntax
impl OwnOnly for Right                                   # hypothetical syntax

let LEFT: own Left = Left { fd: sys_open("left") }        # hypothetical syntax
let RIGHT: own Right = Right { fd: sys_open("right") }    # hypothetical syntax

fn left() -> ^{static} Left: LEFT                        # hypothetical syntax
fn right() -> ^{static} Right: RIGHT                     # hypothetical syntax

impl DropOwn for Left:                                  # hypothetical syntax
    fn drop(^mut self) -> void:                          # hypothetical syntax
        inspect_right(right())
        sys_close(self.fd)

impl DropOwn for Right:                                 # hypothetical syntax
    fn drop(^mut self) -> void:                          # hypothetical syntax
        inspect_left(left())
        sys_close(self.fd)
```

Assume inspect_left and inspect_right borrow their named types, touch
the handle, and require no row. Both accessors make their global owners
immovable, as required. Neither destructor holds a declared borrowed field.

**Naive generated behavior.** Whichever destructor runs second touches
an already closed peer. Merely reversing the shutdown order cannot fix
the cycle. Ordinary field-based drop checking has no stored lifetime
through which to see this dependency.

**Verdict: friction; reject the dependency cycle.** Including implicit
destructor reads in the existing global dependency graph can reject this
program conservatively. The rejection must also cover indirect calls and
drop glue for nested owned fields. No new lifetime syntax is needed.

The existing initialization rule already recognizes dependencies through
trait methods and other implicit calls. This is a concrete regression
obligation for applying that rule to DropOwn, not a request to allow cycles.

### Z7. Take Before Suspension Makes The Empty State Observable

Reference shape: taking an option before an asynchronous replacement
leaves None if the operation is cancelled.

```rust
let old = session.file.take();
let next = rebuild(old).await?;
session.file = Some(next);
```

```text
fn rebuild!(old: own Option[own File]) -> Result[own File, string]: # hypothetical syntax
    wait!()
    .Ok(open("new"))

fn refresh!(session: ^mut Session) -> Result[void, string]: # hypothetical syntax
    old := session.file.take()
    next := rebuild!(old)?
    session.file = .Some(next)
    .Ok(())

fn prepare_first!(session: ^mut Session) -> Result[void, string]: # hypothetical syntax
    next := prepare_new!()?
    old := session.file.take()
    session.file = .Some(next)
    drop(old)
    .Ok(())
```

Assume Session has the owned option field from Z3. prepare_new returns
an owned File on success and does not need the old file. The caller
retains ownership of Session while lending it to refresh.

**Generated behavior.** refresh moves old into rebuild's frame. On
cancellation, that frame drops old. The caller gets exclusive access
to Session back with its file field still None. An error propagated by
`?` likewise skips refilling the field.

prepare_first preserves the old slot until preparation succeeds. The
take/refill sequence then contains no bang call or propagation point.
It drops the returned old option explicitly after installing the new owner.

**Verdict: friction; accepted representation cost.** Ownership remains
safe, but Option does not promise an always-populated application invariant.
Destructors and later methods must tolerate None whenever this workflow
can expose it. A destructor that blindly expects Some can panic and poison
the instance under the already-chosen panic policy.

This is not evidence for restoring general replace. Reordering existing
operations solves workflows where preparation can occur first. Work that
must consume the old resource before suspension necessarily needs an
explicit empty/transition state or an application-level recovery strategy.

### Z8. The Mode Cutoffs Must Compose Through Containers

Reference shape: a wrapper around a plain factory does not own the
factory's future results; shared-reference collections do not own referents.

```rust
struct FactoryBox<T> { make: fn() -> T }
fn refs<T>(xs: Vec<&T>) -> Vec<&T> { xs.clone() }
```

```text
@derive(OwnOnly)
data FactoryBox[T]:
    make: fn() -> T

fn factory_boxes[T < own Any](box: FactoryBox[T]) -> (FactoryBox[T], FactoryBox[T]): # hypothetical syntax
    (box, box)

fn view_lists[T < own Any](xs: List[^T]) -> (List[^T], List[^T]): # hypothetical syntax
    (xs, xs)
```

**Generated behavior.** The factory field is plain for every T, so
structural classification must not follow its result signature into an
imaginary stored owner. Each list element is a shared borrow, so the
list instance stays plain. Copies alias these GC objects and preserve
the borrows' lifetimes.

**Verdict: works under the general mode predicate.** This extends Z1
beyond direct values. The classifier must inspect stored value modes,
not search arbitrary nested type text for `own`. Function result types
do not describe captured environment ownership.

[Rust Vec.clone](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.clone)
builds a distinct vector here; hd's plain-list copy aliases the list by decision. This behavioral difference is permitted;
it does not duplicate an owned referent or erase a lifetime.

### Z9. Partially Evaluated Calls Still Own Their Earlier Arguments

Reference shape: an owned first argument exists while another argument
awaits a result.

```rust
consume_pair(open_file(), open_later().await).await;
```

```text
fn consume_pair!(first: own File, second: own File) -> void: # hypothetical syntax
    wait!()

fn build!() -> void:
    consume_pair!(open("first"), open_later!("second"))
```

Assume open returns an owned File. open_later may suspend before returning
another owner. All helpers have empty requirement rows.

**Generated behavior.** Evaluate left to right. While open_later is
pending, the parent owns the first argument temporary. Cancellation must
drop it even though consume_pair's frame does not yet exist. After both
arguments transfer into that frame, its cleanup owns both obligations.

**Verdict: works, with a lowering obligation.** The ordinary ownership
and cancellation policy supplies the answer. Temporary cleanup needs
the same exit handling as named bindings; it cannot depend on reaching
the textual end of the statement. This is a regression test for drop
flags and frame construction, not a proposed new rule.

### Z10. A Handwritten Always-owned Generic Has An Admission Problem

Reference shape: a generic wrapper always performs custom cleanup,
whether its payload is a primitive or an owned file.

```rust
struct Tracked<T> { value: T }
impl<T> Drop for Tracked<T> {
    fn drop(&mut self) { note_drop(); }
}
```

```text
data Tracked[T]:
    value: T

impl[T < own Any] OwnOnly for Tracked[T]                  # hypothetical syntax
impl[T < own Any] DropOwn for Tracked[T]:                 # hypothetical syntax
    fn drop(^mut self) -> void: note_drop()               # hypothetical syntax

fn receive(value: own Tracked[own File]) -> void: pass    # hypothetical syntax

data OwnershipTag: pass
impl OwnOnly for OwnershipTag                            # hypothetical syntax

@derive(OwnOnly)
data TaggedTracked[T]:
    value: T
    tag: own OwnershipTag                                # hypothetical syntax

impl[T < own Any] DropOwn for TaggedTracked[T]:           # hypothetical syntax
    fn drop(^mut self) -> void: note_drop()               # hypothetical syntax

fn wrap_file(value: own File) -> own TaggedTracked[own File]: # hypothetical syntax
    TaggedTracked { value: value, tag: OwnershipTag {} }
```

Assume note_drop has an empty row and observes that cleanup ran.
No generic constructor over arbitrary aliased plain payloads is claimed;
the freshness restriction still applies to such constructors independently.

**Generated behavior.** Tracked declares that every instance is owned-only,
which is necessary for its unconditional custom destructor. However,
4.1.12 admits owned generic arguments only through structural derivation.
The handwritten marker alone therefore does not admit Tracked[own File].

Replacing that marker with structural derivation alone does not preserve
the intended contract: Tracked[i32] would become plain and could not have
the unconditional DropOwn implementation. Adding both marker implementations
is not a proposed coherence workaround.

TaggedTracked provides an existing-rule workaround. Its owned tag makes
every structural instance owned-only, including primitive payloads, and
the derivation admits owned arguments. The file constructor moves its
payload and constructs the tag fresh.

**Verdict: friction.** A legal representation exists, but a bookkeeping
field is required to express an already explicit always-owned guarantee.
This is narrower than the settled conditional-classification question;
it asks whether a stronger handwritten guarantee should also admit owned
type arguments. No overlapping conditional impls are needed.

## Comparison With Other Languages

| Mechanism | Primary source | Implication |
| --- | --- | --- |
| Shared references and plain factories | [Rust Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html) | The revised generic rule now preserves the important mode distinction |
| Delayed moves in guards | [Rust Reference](https://doc.rust-lang.org/reference/expressions/match-expr.html#match-guards) | The new guard decision has an established checking model |
| Optional extraction | [Rust Option.take](https://doc.rust-lang.org/std/option/enum.Option.html#method.take) | Removal leaves None; cancellation cannot imply automatic restoration |
| Static shutdown | [Rust static items](https://doc.rust-lang.org/reference/items/static-items.html) | Rust statics have no automatic program-end drop; hd's policy needs separate dependency reasoning |
| Destructor liveness | [Rust drop checking](https://doc.rust-lang.org/nomicon/dropck.html) | Data needed during cleanup must remain valid through cleanup; ambient global reads also need accounting in hd |

Rust's static policy is a comparison, not a request to reverse hd's
chosen automatic global destruction. The affordable path here is to
reuse hd's existing implicit-call dependency analysis conservatively.

## Problems, Ranked

No new inconsistency is forced by this revision. These findings have
different status: one important analysis integration, one generic declaration
restriction, and one application-level cancellation cost.

| Rank | ID | Finding | Cases | Status |
| --- | --- | --- | --- | --- |
| 1 | OWN-S20 | Implicit destructors must participate in global lifetime/dependency analysis | Z5, Z6 | Required integration; naive lowering would use destroyed resources |
| 2 | OWN-S21 | Manual always-owned generics do not admit owned arguments | Z10 | Expressiveness/representation friction |
| 3 | OWN-S22 | Optional extraction exposes an empty state across suspension | Z7 | Accepted policy cost; ordinary-code mitigation |

### OWN-S20. Account For Cleanup's Ambient Global Dependencies

**Effect.** Z5 reads a resource through a static accessor after a naive
reverse-order teardown destroys it. Z6 proves that changing the order
alone is insufficient. Neither value stores the problematic borrow:
the destructor creates it through an ordinary function call.

`names.init.references` already includes implicit calls. A conservative
ownership integration can treat future drop glue as a dependency of an
owner's initialization and retention. Z5 then requires LOG first; Z6
rejects a cycle. This is an application of an existing analysis, not
evidence that another lifetime annotation is necessary.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should the ownership integration explicitly include reachable destructor calls, owned-field drop glue, and callbacks in existing global dependency analysis, rejecting unsatisfiable shutdown dependencies? | One general language semantic integration with existing analysis; no syntax or intrinsic addition | Supplies conservative rejection and a regression contract for Z5/Z6 |
| If some drop dependencies cannot be summarized, should global owners with those cleanup paths reject conservatively? | One language semantic restriction; no new syntax or intrinsic | Keeps a modular safety guarantee while limiting acceptance |

An explicit early drop can order a particular acyclic example, but it
cannot justify accepting arbitrary unsafe automatic shutdown. The checker
must also handle cancellation during initialization. Empty requirement
rows do not imply absence of ambient global reads.

### OWN-S21. Decide Whether Structural Derivation Is The Only Admission Proof

**Effect.** Z10's handwritten marker promises that no plain alias can
contain its payload. Nevertheless, the owned type argument is rejected
because the admission rule names structural derivation specifically.
An owned sentinel field supplies a workaround without changing the language.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should always-owned generic wrappers use a small ordinary owned tag field so Structure supplies the admission proof? | One ordinary library helper type and one field; stdlib or application tier; no new language mechanism | Cheapest change category under the owner's cost order, with representation and teaching cost |
| Should owned arguments also be admitted when an unconditional handwritten OwnOnly impl already guarantees every instance is owned-only? | One language semantic generalization of admission; no syntax or intrinsic addition | Removes the sentinel without reintroducing conditional impl overlap |

The second does not replace structural derivation for conditional modes.
It recognizes a stronger existing guarantee. It also does not relax
the freshness rule for arbitrary plain payloads. Both costs should be
compared before adding a special guard or wrapper syntax.

### OWN-S22. Treat None As A Real Transition State

**Effect.** Z7 can end a cancelled borrowed operation with Session
still alive and empty. All files are dropped correctly; the application
may still have lost an always-open-session invariant. The option type
accurately exposes that possibility.

**Candidate question.** Should the guide show preparation before take,
with destructors and later methods explicitly handling None when taking
must precede suspension? This is documentation and ordinary application
or stdlib code, with zero syntax, semantic-exception, or intrinsic additions.

No rollback guard is proposed as an automatic cure: restoring into a
borrowed Session would itself need the appropriate exclusive loan, which
could obstruct the refill operation. The simple reorder in Z7 is enough
when preparation does not consume the old resource.

## Evaluation By Dimension

| Dimension | Change since review 5 | Assessment |
| --- | --- | --- |
| Guarantee clarity | Generic copying, guard commitment, and extraction policy now have explicit answers | Stronger; distinguish memory/resource ownership from an application's populated-slot invariant |
| Soundness | The previous copying conflict and guard omission are resolved | No new accepted-program exploit established; global destructor dependencies are a critical integration obligation |
| Precision | Shared references and factories remain copyable, including nested plain wrappers | Improved; conservative global dependency analysis may reject some safe programs |
| Practical expressiveness | Option.take supports extraction inside Drop-owning wrappers | Useful; always-owned generics require an admission workaround, and asynchronous refill may expose None |
| Composability | Guard moves compose with cancellation; nested generic modes have a consistent predicate | Better; global shutdown must compose with ordinary calls and implicit drop glue |
| Modular contracts | Definition-time checking remains universal and known modes remain stable | Improved; destructor global-read summaries or conservative restrictions are needed across indirect calls |
| Agent writability and human readability | Direct safe helper copies now match their concrete equivalents | Better; an ownership tag added solely for generic admission is a teaching cost; no writing trial performed |
| Conceptual cost | The revision reuses mode checking and Option rather than adding replace machinery | Favorable; prefer completing existing dependency analysis over new lifetime/context syntax |
| Compiler feasibility and cost | Guard precommit states, cancellation temporaries, and mode cutoffs have concrete tests | Ready for a narrow prototype; global drop dependencies and generic admission need deliberate integration |
| Optimization validity and payoff | No new measurement or optimizer proof | Existing plain aliases, cleanup effects, and slot transition states still constrain transformations |
| Specification readiness | Every review-5 finding is resolved | Closer to formalization; spell out the global-drop integration, retain the acceptance matrix, and finish declared section-6 work |

This review strengthens the prototype recommendation. A small checker
should now demonstrate Z1/Z2/Z3 acceptance, Z2's consuming-guard rejection,
and sound global dependency handling. Runtime probes should count drops
around Z4/Z7/Z9 cancellation rather than relying on successful parsing.

## Earlier Findings

| Review-5 finding | Current status |
| --- | --- |
| OWN-S17: every mention of T was move-only | Resolved by owner-capable mode checking, with explicit shared-borrow and plain-function cutoffs |
| OWN-S18: guarded consuming patterns | Resolved by shared guard inspection and move commitment after success |
| OWN-S19: replacement capability | Resolved by the decision against general replace and for built-in Option.take |

Earlier receiver reservations, structural base cases, recursive classification,
borrow-aware drop, provider restrictions, and immovable borrowed globals
remain settled. OWN-S20 does not reopen movement of global owners;
it tests the separate point where their automatic destruction occurs.

Section 6's iteration, receiver-trait, clone, generic-drop dispatch, and
higher-order API questions remain declared open work. They are not
counted as newly discovered failures. Prior editorial observations about
section 3.4 and annotation counts remain editorial.

## Parse Log

All ten `text` blocks were passed to `scripts/hd-conformance.sh parse`.
Each raw block rejects marked proposed syntax. Every projected block
parses. No ownership checking or runtime validation is implied.

Projection preserves ordinary operations, comments, and line breaks.
It erases ownership marks, borrow marks, lifetime applications, and
standalone proposed OwnOnly impl lines. Generic DropOwn impl bodies and
derive annotations remain, so their surrounding grammar is checked.

| Block | Case | Raw | Projected |
| --- | --- | --- | --- |
| 1 | Z1 | Rejects proposed syntax | Parses |
| 2 | Z2 | Rejects proposed syntax | Parses |
| 3 | Z3 | Rejects proposed syntax | Parses |
| 4 | Z4 | Rejects proposed syntax | Parses |
| 5 | Z5 | Rejects proposed syntax | Parses |
| 6 | Z6 | Rejects proposed syntax | Parses |
| 7 | Z7 | Rejects proposed syntax | Parses |
| 8 | Z8 | Rejects proposed syntax | Parses |
| 9 | Z9 | Rejects proposed syntax | Parses |
| 10 | Z10 | Rejects proposed syntax | Parses |

Raw diagnostics follow verbatim, replacing temporary paths with FILE.
All raw statuses are 101. All projected statuses are 0, with `1/1 clean`.

### Block 1

```console
error: FILE:1:17: syntax-error: expected ',' or ']', found 'Any'
error: FILE:4:22: syntax-error: expected ',' or ']', found 'Any'
0/1 clean
```

### Block 2

```console
error: FILE:1:21: syntax-error: expected ',' or ')', found 'Option'
error: FILE:7:23: syntax-error: expected ',' or ')', found 'Option'
0/1 clean
```

### Block 3

```console
error: FILE:2:15: syntax-error: expected a line ending, found 'Option'
error: FILE:6:13: syntax-error: expected a parameter name, found '^'
error: FILE:9:15: syntax-error: expected a parameter name, found '^'
0/1 clean
```

### Block 4

```console
error: FILE:1:28: syntax-error: expected ',' or ')', found 'Option'
0/1 clean
```

### Block 5

```console
error: FILE:4:16: syntax-error: expected '=', found 'AuditOnDrop'
error: FILE:7:21: syntax-error: expected a type, found '^'
error: FILE:10:13: syntax-error: expected a parameter name, found '^'
0/1 clean
```

### Block 6

```console
error: FILE:9:15: syntax-error: expected '=', found 'Left'
error: FILE:10:16: syntax-error: expected '=', found 'Right'
error: FILE:12:14: syntax-error: expected a type, found '^'
error: FILE:13:15: syntax-error: expected a type, found '^'
error: FILE:16:13: syntax-error: expected a parameter name, found '^'
error: FILE:21:13: syntax-error: expected a parameter name, found '^'
0/1 clean
```

### Block 7

```console
error: FILE:1:22: syntax-error: expected ',' or ')', found 'Option'
error: FILE:5:22: syntax-error: expected a type, found '^'
error: FILE:11:28: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 8

```console
error: FILE:5:26: syntax-error: expected ',' or ']', found 'Any'
error: FILE:8:23: syntax-error: expected ',' or ']', found 'Any'
0/1 clean
```

### Block 9

```console
error: FILE:1:29: syntax-error: expected ',' or ')', found 'File'
0/1 clean
```

### Block 10

```console
error: FILE:4:14: syntax-error: expected ',' or ']', found 'Any'
error: FILE:5:14: syntax-error: expected ',' or ']', found 'Any'
error: FILE:6:13: syntax-error: expected a parameter name, found '^'
error: FILE:8:23: syntax-error: expected ',' or ')', found 'Tracked'
error: FILE:16:14: syntax-error: expected a line ending, found 'OwnershipTag'
error: FILE:18:14: syntax-error: expected ',' or ']', found 'Any'
error: FILE:19:13: syntax-error: expected a parameter name, found '^'
error: FILE:21:25: syntax-error: expected ',' or ')', found 'File'
0/1 clean
```

## Repository Validation

Origin was fetched. Checks ran in an isolated worktree at `5d9e3f7d6`,
rebased onto `origin/main`, with this report and its README entry copied
in. Concurrent compiler and fixture edits in the main checkout were
excluded from validation and left untouched.

| Check | Result |
| --- | --- |
| Raw parser runs | Ten expected rejections of marked proposed syntax |
| Projected parser runs | All ten parse |
| `bash spec/check.sh` | Passed; links and anchors, zero failing dead citations, 6,002 rule IDs, 1,565 diagnostic rows |
| `pnpm run website:build` | Passed; 79 pages, 265 grammar rules, 266 definitions |
| Whitespace checks | Passed for the README diff and new report |

The spec check reported nine citation warnings and seven allowed as
history; none failed. Final validation prose changes no example.
No ownership type-checking or runtime safety validation is claimed.

Only this report and its README entry were written. The design record,
specification, compiler, and fixtures were left untouched. Nothing was
staged, committed, or pushed.
