# Ownership And Borrows: Adversarial Review 3

Status: design review, 2026-10-16; changes no decision, design record,
spec text, or compiler code. Nothing here is accepted behavior.
This reviews [Ownership And Borrows](OWNERSHIP_DESIGN.md) at commit
`7e85e38b01d90510d4232d4885be7cc61d684522`, against
[review 2](OWNERSHIP_STRESS_TEST_2.md).

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

The latest owner decisions supersede the intermediate bans on owned
fields. `OwnOnly` is now the chosen type category. This review does not
propose removing it or reversing plain-only providers.

| Current decision | Meaning under test |
| --- | --- |
| Every use of an `OwnOnly` type is explicitly owned or borrowed | A bare instance in a plain position rejects |
| Only `OwnOnly` types may contain written `own` or `^mut` fields | Exclusive state cannot hide in a freely copied plain holder |
| `DropOwn` extends `OwnOnly` | Destruction remains tied to unique ownership |
| Generic data headers stay plain; a conditional marker impl admits owned arguments | `Box[i32]` can stay plain while `Box[own File]` is `OwnOnly` |
| An argument `own U` counts as `OwnOnly` | Optional ownership of an otherwise plain type also triggers conditional ownership |
| No partial moves; complete destructuring excludes `DropOwn` types | The previous destructor/initialization attack rejects |
| Destruction uses every borrow the value holds | Last-source-use reasoning cannot end a destructor's loan early |
| Providers accept plain values only | Exclusive or owning services are explicit parameters |
| Any function borrowing a top-level owner makes it immovable | Static accessors cannot be invalidated by later top-level moves |

The dependent language-tier sections are [types and generic substitution](../../spec/lang/04-type-system.md),
[cleanup](../../spec/lang/06-control-flow.md#deferred-cleanup),
[captures](../../spec/lang/07-functions.md#captures),
[data](../../spec/lang/08-data-and-enums.md),
[trait overlap](../../spec/lang/09-traits.md#overlap),
[inherent member names](../../spec/lang/09-traits.md#inherent-member-names),
and [requirements and suspension](../../spec/lang/11-requirements-and-suspension.md).
The stdlib-tier dependencies are [collections](../../spec/std/collections.md),
[iterators](../../spec/std/iter.md), and [tasks](../../spec/std/task.md).

The current chapters do not implement ownership. They supply the
existing substitution, coherence, closure-row, and library contracts
with which the new proposal must compose. `File`, `Conn`, and resource
operations use the study's intended contracts.

W5 assumes `Option` receives the single-argument conditional marker
impl proposed for std containers. Omitting that impl rejects its owned
argument earlier and does not supply the missing generic API.

## Method

Eight cases include four regression controls and four additional
integration probes. Foreign code shows short library shapes, not copied
implementation bodies. Every hd block is checked raw and after erasing
unsupported proposed syntax.

| Verdict | Meaning |
| --- | --- |
| Works | Current decisions supply the intended result or rejection |
| Friction | A sound restriction has a practical workaround |
| Breaks | A written contract conflicts with another rule or lacks a required translation |

These are design judgments, not compiler exploits. Parsing does not
validate ownership, lifetimes, effect rows, or runtime destruction.
No new accepted use-after-close is demonstrated in this round.

The same eleven dimensions remain. Migration is excluded. No new
annotation count, writing trial, solver proof, or performance benchmark
is claimed. Counts across reports are not comparative safety scores.

## Summary

| Case | Reference shape | Verdict | Finding |
| --- | --- | --- | --- |
| W1 | Rust destructor-bearing holder | Works | Partial moves and early referent destruction reject |
| W2 | Wrapper around an exclusive reference | Works | `OwnOnly` prevents the previous generic/capture bypass |
| W3 | Mutable context service | Works | Borrowed-provider installation rejects; explicit borrowing works |
| W4 | Static resource accessor | Works | The borrowed global is immovable |
| W5 | `Vec.pop` / optional generic result | Breaks | Conditional marker membership needs a result-mode substitution rule |
| W6 | Batch containing a generic list | Breaks | Ownership of the outer type does not spell ownership of a nested field |
| W7 | Two-payload result type | Breaks | Conditional marker impls collide with existing coherence |
| W8 | Scope guard around an effectful cleanup | Breaks | Ordinary closure lowering leaves a requirement row that `drop` cannot supply |

The result is **four works and four breaks**. W5–W8 are incomplete
contracts or expressiveness failures. They are not four new safety
exploits, and the old attacks are not counted again under new names.

The core is more defensible than in review 2. The remaining weakness
has shifted toward generic type formation and library lowering.
`OwnOnly` supplies a clear category, but category membership alone does
not define every mode appearing in an instantiated signature or field.

## Cases

### W1. Partial Moves And Destructor-Only Uses Reject

Rust reference shapes: moving a field from a destructor-bearing value
rejects, and an inspector's destructor keeps its referent alive.

```rust
struct Watch<'a>(&'a File);
impl Drop for Watch<'_> {
    fn drop(&mut self) { inspect(self.0); }
}
```

```text
data Watch[^a]:                                    # hypothetical syntax
    file: ^{a} File                               # hypothetical syntax

impl[^a] OwnOnly for Watch[^{a}]                   # hypothetical syntax

impl[^a] DropOwn for Watch[^{a}]:                   # hypothetical syntax
    fn drop(^mut self) -> void:                    # hypothetical syntax
        _ := size(self.file)

data Owner:
    file: own File                                # hypothetical syntax

impl OwnOnly for Owner                            # hypothetical syntax

fn no_partial(p: own Owner) -> void:               # hypothetical syntax
    file := p.file                                # rejects: partial move

fn no_early_drop() -> void:
    file := open("input")
    let view: ^File = file                         # hypothetical syntax
    let own watch = Watch { file: view }           # hypothetical syntax
    drop(file)                                    # rejects: watch still uses the borrow at drop
```

**Generated behavior.** There is no partially initialized owner for a
destructor to inspect. The implicit use at `watch`'s drop keeps the file
borrow live until then, so the explicit early drop rejects.

**Verdict: works.** OWN-S5's two previous probes now have explicit rules.
An implementation must apply drop-aware liveness transitively to owned
fields and generated storage, rather than checking root bindings only.
The rule applies to values, so that is its natural interpretation.

Complete destruction order and concrete generic drop dispatch still
need specification, but those implementation obligations do not reopen
the original attack. The comparison is
[Rust E0509](https://doc.rust-lang.org/error_codes/E0509.html) and
[drop checking](https://doc.rust-lang.org/nomicon/dropck.html).

### W2. A Mutable-Borrow Holder Is No Longer Plain

Original shape: a wrapper around a mutable reference cannot duplicate
the reference's exclusive access.

```rust
struct Cursor<'a> { source: &'a mut Document }
fn duplicate<T: Copy>(x: T) -> (T, T) { (x, x) }
```

```text
data Cursor[^a]:                                   # hypothetical syntax
    source: ^{a} mut Document                     # hypothetical syntax

impl[^a] OwnOnly for Cursor[^{a}]                  # hypothetical syntax

fn duplicate[T](x: T) -> (T, T): (x, x)

fn no_duplicate(d: ^mut Document) -> void:         # hypothetical syntax
    let own cursor = Cursor { source: d }         # hypothetical syntax
    _ := duplicate(cursor)                        # rejects: ordinary T cannot take this owner
```

**Generated behavior.** The marker is required for the field declaration.
Its instance must be owned or borrowed, so ordinary `[T]` cannot receive
the owner. Capturing that owner moves it into a one-shot closure.

**Verdict: works.** The previous OWN-S6 gap is closed for the explicit
nominal holder. The same check must occur after aliases and nested type
applications are resolved. W5–W7 test that generic formation boundary,
not a repetition of the direct duplication attack.

### W3. Exclusive Services Are Parameters

Original shape: two pending operations cannot share an exclusive service
borrow; sequential calls can reborrow it.

```rust
async fn emit(sink: &mut Sink) { sink.write().await; }
```

```text
trait Sink:
    fn write!(^mut self) -> void                   # hypothetical syntax

fn emit!(sink: ^mut dyn Sink) -> void:             # hypothetical syntax
    sink.write!()

fn sequential!(sink: ^mut dyn Sink) -> void:       # hypothetical syntax
    emit!(sink)
    emit!(sink)

fn wrong(sink: ^mut dyn Sink) -> void:             # hypothetical syntax
    $.with(Sink=sink):                            # rejects: provider-not-plain
        pass
```

**Generated behavior.** Installation rejects before sibling provider
capture can duplicate the exclusive loan. Sequential parameter calls
use the ordinary mutable reborrow rule.

**Verdict: works.** OWN-S2's provider issue is resolved by reducing the
provider surface, not by secretly serializing all services. Plain
providers intentionally retain existing shared semantics. Borrowed or
owned state hidden inside a nominal provider must still fail the
appropriate plain-position or lifetime checks.

This decision trades implicit service plumbing for explicit arguments.
That is a practical expressiveness cost, not an unresolved safety hole.

### W4. A Static Accessor Makes Its Owner Immovable

Rust reference shape: a static accessor relies on a stable owner.

```rust
static VALUE: u32 = 7;
fn current() -> &'static u32 { &VALUE }
```

```text
GLOBAL := open("global")

fn current() -> ^{static} File:                    # hypothetical syntax
    let view: ^File = GLOBAL                       # hypothetical syntax
    view

drop(GLOBAL)                                      # rejects: current borrows GLOBAL
```

**Generated behavior.** The compiler marks `GLOBAL` immovable because
the function borrows it. It need not reconstruct the specific owner
from every later `static` result. The resource drops at instance end.

**Verdict: works.** OWN-S7 is resolved conservatively. An unused accessor
can freeze the global too, and an interactive session must reject a
new accessor after that owner has already moved. Those follow from
the chosen check, rather than proving a new escape.

The specification should clarify whether generated closure bodies count
as functions for this rule. Ordinary function access is enough to
reject the earlier probe.

### W5. Conditional Ownership Needs A Generic Result Mode

Original shape: a generic consuming lookup returns an optional item.

```rust
fn take_first<T>(xs: Vec<T>) -> Option<T> {
    xs.into_iter().next()
}
```

The study now uses this hd signature:

```text
fn take_first[T < own Any](xs: own List[T]) -> Option[T]: # hypothetical syntax
    xs.into_iter().next()

fn concrete(xs: own List[own Conn]) -> own Option[own Conn]: # hypothetical syntax
    xs.pop()
```

**Generated behavior.** Substituting `T = own Conn` makes the generic
result `Option[own Conn]`. Under the proposed container marker rule,
that instance is `OwnOnly`. Rule 4.1.10 therefore requires an owning or
borrowed outer mode, yet the generic result is written plain.

**Verdict: breaks; missing substitution rule.** Under the literal
plain-position rule, the ownership-enabled generic signature cannot
serve this instantiation. If substitution instead promotes its result
to `own Option[own Conn]`, that is a coherent candidate, but the
promotion and its boundary behavior need an explicit rule.

This is not the previous extra-`own T` typo. That spelling was correctly
removed. The remaining question concerns a constructor around `T`,
whose required outer mode changes with its argument.

Writing `own Option[T]` everywhere would choose an owned wrapper even
for plain arguments, changing the plain API. For a general `Box[T]`
constructor, that workaround also faces fresh-plain-field uniqueness:
an arbitrary pre-aliased plain `T` is not known fresh.

[Rust Vec.pop](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.pop)
and [Option](https://doc.rust-lang.org/std/option/enum.Option.html) establish
the library shape. hd's extra outer mode creates the additional question.

### W6. Marking The Outer Batch Does Not Mark Its List Field

Original shape: a generic aggregate contains a collection of its item.

```rust
struct Batch<T> { values: Vec<T> }
```

```text
data Batch[T]:
    values: List[T]

impl[T < OwnOnly] OwnOnly for Batch[T]              # hypothetical syntax

fn consume_batch(batch: own Batch[own Conn]) -> void: # hypothetical syntax
    pass
```

**Generated behavior.** The conditional marker makes the outer batch
owned-only, so its parameter has the right outer mode. Its field still
substitutes to bare `List[own Conn]`, itself an `OwnOnly` instance in a
plain field position.

**Verdict: breaks; nested mode formation is missing.** Promoting the
outer batch does not by itself promote that field. A uniform
substitution rule could cover both W5 and W6; a return-only exception
would not.

Writing `values: own List[T]` fixes that field's mode for every
instantiation, but then every batch needs `OwnOnly`, including
`Batch[i32]`. That is a valid different representation, not the claimed
plain-versus-owned conditional behavior.

The same issue appears in `List[Option[T]]`, enum payloads, callback
results, and associated type applications. Existing generic substitution
preserves complete types; it does not insert new outer modifiers into
arbitrary surrounding constructors.

### W7. Either Owned Payload Meets Head-Only Coherence

Original shape: a result can contain an owned success value, an owned
error value, or both types can be owned.

```rust
enum Outcome<A, B> { Good(A), Bad(B) }
```

```text
enum Outcome[A, B]:
    Good(value: A)
    Bad(error: B)

impl[A < OwnOnly, B] OwnOnly for Outcome[A, B]       # hypothetical syntax
impl[A, B < OwnOnly] OwnOnly for Outcome[A, B]       # hypothetical syntax
```

**Generated behavior.** The two natural conditional impls have identical
target shapes after renaming parameters. Existing overlap checks use
heads only and ignore bounds. They reject this pair.

**Verdict: breaks; coherence integration is required.** Omitting either
condition leaves one owned-payload family unsupported. If plain and
owned parameter domains are intended to partition the cases, the
coherence algorithm must recognize that distinction. Coverage of the
both-owned case must be defined too.

A single conjunctive condition requiring both payloads to be owned
misses the mixed cases. A universal marker instead makes even the
all-plain instance owned-only. The single-argument `Box[T]` example
does not establish a scalable rule for these constructors.

There is a related limit on a possible W5 workaround. Defining plain
and owning variants of the same inherent method in impls distinguished
only by bounds currently gives `duplicate-inherent-member`.
Neither conditional membership nor method availability automatically
changes [Overlap](../../spec/lang/09-traits.md#overlap) or
[Inherent Member Names](../../spec/lang/09-traits.md#inherent-member-names).

This is a concrete language integration issue, not a demand for general
specialization. [Rust E0119](https://doc.rust-lang.org/error_codes/E0119.html)
provides a comparison for conflicting implementations; the operative
rules here are hd's own, stricter head-only checks.

### W8. An Effectful Defer Is Not An Ordinary Empty-Row Callback

Original shape: a scope guard calls a captured logger on exit.

```rust
let _guard = scopeguard::guard((), |_| logger.note());
```

```text
trait Audit:
    fn note(self) -> void

fn record() -> void $ Audit:
    $.use(Audit).note()

fn work() -> void $ Audit:
    defer: record()
```

**Generated behavior.** The advertised direct lowering constructs a
guard from a closure whose body calls `record`. Under existing capture
rules, that closure requires `$ Audit`; it does not capture its provider.
The guard's `DropOwn.drop` has no requirement row supplying `Audit`.

**Verdict: breaks; the lowering needs a contract.** The source defer is
synchronous, returns `void`, and obeys existing defer restrictions.
It is not an asynchronous-close problem. Merely spelling its body as
an ordinary closure leaves the effect requirement unsatisfied.

A concrete workaround captures the provider value before building an
empty-row callback, which calls its method directly. The compiler can
potentially perform that translation for the sugar. The study must
state it, rather than claiming the direct closure expansion already
provides it.

A general library wrapper cannot simply add a stored row parameter:
data types do not declare row parameters, and `$.Context[$ R]` forbids
row variables. Callback type erasure alone does not supply the missing
providers at destruction time either.

The relevant rules are [Captures](../../spec/lang/07-functions.md#captures),
[Row Parameters](../../spec/lang/11-requirements-and-suspension.md#row-parameters),
and [Deferred Cleanup](../../spec/lang/06-control-flow.md#deferred-cleanup).
[scopeguard](https://docs.rs/scopeguard/latest/scopeguard/) illustrates
the ordinary-library API once its callback environment is closed.

## Comparison With Other Languages

| Boundary | Source | What it establishes |
| --- | --- | --- |
| Whole destructor-bearing values | [Rust E0509](https://doc.rust-lang.org/error_codes/E0509.html) | The new no-partial-move rule gives a conservative answer |
| Implicit use on destruction | [Rust drop checking](https://doc.rust-lang.org/nomicon/dropck.html) | The new lifetime rule addresses the earlier destructor-use probe |
| Generic owning and duplicable values | [Swift noncopyable generics](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md) | Admissible arguments and a generic body's permitted operations must agree; this does not define hd's nested mode insertion |
| Optional extraction | [Rust Vec.pop](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.pop) | The result carries the element type; hd must additionally define the wrapper's outer mode |
| Conditional implementations | [Rust E0119](https://doc.rust-lang.org/error_codes/E0119.html) | Overlapping impls need a coherence policy, even for marker traits |
| Cleanup callback | [scopeguard](https://docs.rs/scopeguard/latest/scopeguard/) | A guard can invoke a closed callback; hd's implicit requirements need closing during lowering |

No source establishes soundness for hd's hybrid mode system. These are
reference shapes and specific obligations, not proofs by analogy.

## Problems, Ranked

This round ranks broad contract formation above the smaller lowering
gap. No new accepted resource escape is alleged. Existing owner choices
remain fixed; candidate mechanisms are questions, not decisions.

| Rank | ID | Problem | Cases | Severity |
| --- | --- | --- | --- | --- |
| 1 | OWN-S10 | Conditional ownership needs modes throughout substituted types | W5, W6 | High generic expressiveness and modular-contract gap |
| 2 | OWN-S11 | Conditional membership must coexist with coherence | W7 | High integration gap for multi-parameter containers |
| 3 | OWN-S12 | Guard lowering must close cleanup requirements | W8 | Moderate integration gap for an already chosen feature |

### OWN-S10. Membership And A Mode Are Different Information

**Effect.** W5 changes the required mode of a generic result. W6
changes a nested field's required mode. Knowing that an instantiated
constructor implements `OwnOnly` does not say how its unmarked uses
are represented or rejected after substitution.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should separate ordinary library carriers/APIs use an explicitly owned representation for owning cases? | Core-library additions where sufficient; stdlib tier | Cheapest local workaround, but different type/function names and fresh-field obligations remain; not a complete answer for the proposed shared `Option` API |
| Should substitution uniformly supply required ownership modes inside generic type applications, while concrete source uses remain explicit? | One general language semantic rule, including fields, payloads, results, and identity | Preserves more shared APIs; introduces a real implicit generic rule requiring canonicalization and diagnostics |
| Should signatures express mode-dependent constructor results explicitly? | New syntax or a new compiler-known type operation, depending on representation | Potentially clearer exported contracts, but more expensive than a sufficient existing-rule answer |

The second candidate needs to distinguish a well-formed generic
expression from a handwritten concrete plain use that must reject.
It must also preserve stable type identity and avoid turning an
existing plain argument into an owner. Adding a prefix at call sites
alone cannot fix W6's declaration.

An always-owned wrapper is not automatically a universal constructor:
arbitrary plain composite inputs can already have aliases, while fresh
owned fields require uniqueness. That constraint must remain visible.

### OWN-S11. Conditional Marker Coverage Needs A Coherence Rule

**Effect.** W7 cannot state the natural two-sided condition through
ordinary overlapping impls. The inherited coherence rules also limit
splitting methods by ownership bounds to work around W5.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should a separate always-owned multi-payload carrier cover these cases initially? | An ordinary core-library type and APIs, stdlib tier | Cheapest restricted surface; does not preserve one conditionally plain `Result` family |
| Should compiler-recognized structural ownership propagation cover containers with multiple payload types? | Compiler intrinsic/type-classification addition, language tier | Avoids overlapping user impls; must respect owner control over which types admit owned arguments |
| Should `OwnOnly` conditional implementations combine their applicability conditions under an explicit coherence rule? | A language semantic exception or general coherence extension | Can preserve the proposed marker form; cannot be inferred from current head-only matching |

Costs follow the repository order: ordinary library additions first,
then intrinsics, semantic exceptions, and new syntax last. General
specialization is not justified before these narrower mechanisms are
evaluated. An ordinary library workaround should not be advertised as
providing the unchanged conditional API.

### OWN-S12. Close The Callback Environment Before Drop

**Effect.** W8 has a synchronous callback with an unsatisfied implicit
requirement when moved into an ordinary destructor. Scope and cleanup
order alone do not provide that requirement.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should library guard callers explicitly capture concrete plain provider values and pass an empty-row callback? | Ordinary core-library/caller structuring, stdlib tier | Works for direct guard use; makes implicit cleanup dependencies explicit |
| Should `defer` sugar perform that provider capture and environment-closing transformation? | A language semantic lowering rule over the chosen syntax | Preserves existing effectful defer bodies while keeping guard destruction ordinary |

The lowering must preserve the provider selected by the lexical scope
and cleanup timing, including cancellation. It should not require a
general new effect system. This is needed for the chosen basic `defer`
surface; unrelated advanced guard features can wait.

## Evaluation By Dimension

| Dimension | Change since review 2 | Current assessment |
| --- | --- | --- |
| Guarantee clarity | Better: `OwnOnly` makes the category explicit; providers have a clear plain-only boundary | Explain both type category and position mode. The system is no longer accurately described as purely position-based |
| Soundness | The earlier partial-move, destructor-use, holder-copy, provider, and static-global probes have rejection rules | No new demonstrated accepted escape in this round; generic formation and lowering remain incomplete, so soundness is not established |
| Precision | Several fixes deliberately choose conservative rejection | No partial moves; function borrowing freezes a global even if unused; exclusive providers require parameters. These are chosen costs, not stale holes |
| Practical expressiveness | Owning pools and exclusive holders now have explicit declarations | Generic optional results, nested containers, and multi-payload carriers need working contracts. Reusable lazy producers remain openly unfinished |
| Composability | Concrete ownership and borrowing cases are much stronger | Weakest seam is now substitution and conditional marker coverage, rather than individual cleanup paths |
| Modular contracts | The marker states nominal ownership policy directly | Need a canonical mode for every instantiated result/field and a coherence policy; effectful guard environments need explicit lowering |
| Agent writability and human readability | Concrete declarations are clearer; previous contradictory instructions mostly fixed | Agents must distinguish `OwnOnly`, outer `own`, ownership-enabled function bounds, and conditional impls. No writing trial performed |
| Conceptual cost | More explicit category, fewer ambiguous structural assumptions | Category plus mode plus conditional conformance is additional machinery. Its cost depends heavily on whether one substitution rule suffices |
| Compiler feasibility and cost | Conservative restrictions simplify move and loan analysis | Conditional well-formedness, mode normalization, and trait matching now need definition. No solver benchmark or proof of complexity |
| Optimization validity and payoff | Previous explicit exclusive-state bypasses now reject | Still no universal alias guarantee for plain descendants or captures; no new measured payoff |
| Specification readiness | Old safety repairs are concrete and testable | Closer, but generic type formation and coherence precede a complete spec pass. Guard lowering and declared iterator work remain |

My assessment is **a defensible concrete core with unfinished generic
composition**. This is a narrower criticism than the previous review.
The old safety probes should become regression requirements rather
than being repeatedly reported as open findings.

`OwnOnly` and `DropOwn` are compiler-recognized language-tier items under
the repository's tier test, despite their std trait spelling. Ordinary
guard, pool, and adapter implementations belong to the stdlib tier when
they need no additional compiler knowledge.

## Earlier Findings

| Review-2 finding | Current status |
| --- | --- |
| U1 hidden owned results and discarded elements | Destruction remains the chosen answer; no new attack found here |
| U2 original owner duplication and plain holder | Marker/position restrictions preserve rejection |
| U3 mixed ownership and post-await transfer | Still expressible using owned fields in an `OwnOnly` type; the prose saying a data type never owns a handle is stale beside the restored Pool example |
| U4 partial moves / OWN-S5 | Fixed by the blanket no-partial-move rule and the ban on destructuring `DropOwn` types |
| U5 destructor-only uses / OWN-S5 | Fixed by counting drop as a use of held borrows |
| U6 globals / OWN-S7 | Fixed by immovable function-borrowed owners |
| U7 move-only borrowed holders / OWN-S6 | Fixed for nominal holders by required `OwnOnly`; W5–W7 are formation/coherence follow-ups |
| U8 providers / OWN-S2 | Fixed by plain-only providers and explicit service parameters |
| U9 reusable owning callbacks / OWN-S9 | Still a declared open producer/iterator question, not rediscovered here |
| U10 stale rules / OWN-S8 | Main contradictions corrected: replacement drops, generic result uses `T`, and top-level movement is qualified |

The annotation-count claim is not revalidated. It predates the latest
marker declarations and conditional impls, and should not stand in for
a measured writing/readability evaluation.

## Parse Log

All eight `text` blocks were passed to
`scripts/hd-conformance.sh parse FILE`. W7 and W8 parse as written.
W1–W6 reject unsupported proposed syntax. All eight projections parse.

The projection preserves line breaks and executable statements. Before
each line's comment, it removes `own`, `^`, lifetime bounds and parameter
lists, then empty named lifetime applications. Standalone hypothetical
`OwnOnly` impl declarations become comments. This projection checks the
surrounding grammar only; it deliberately erases the disputed semantics.

| Block | Case | Raw | Projected |
| --- | --- | --- | --- |
| 1 | W1 | Rejects proposed syntax | Parses |
| 2 | W2 | Rejects proposed syntax | Parses |
| 3 | W3 | Rejects proposed syntax | Parses |
| 4 | W4 | Rejects proposed syntax | Parses |
| 5 | W5 | Rejects proposed syntax | Parses |
| 6 | W6 | Rejects proposed syntax | Parses |
| 7 | W7 | Parses | Parses |
| 8 | W8 | Parses | Parses |

Raw diagnostics follow verbatim, replacing only the temporary pathname
with `FILE`. Raw acceptance returned status 0; rejections returned 101.
Every projected run returned status 0 and `1/1 clean`.

### Block 1

```console
error: FILE:1:12: syntax-error: expected a generic parameter name, found '^'
error: FILE:2:11: syntax-error: expected a type, found '^'
error: FILE:4:6: syntax-error: expected a generic parameter name, found '^'
error: FILE:6:6: syntax-error: expected a generic parameter name, found '^'
error: FILE:7:13: syntax-error: expected a parameter name, found '^'
error: FILE:11:15: syntax-error: expected a line ending, found 'File'
error: FILE:15:22: syntax-error: expected ',' or ')', found 'Owner'
error: FILE:20:15: syntax-error: expected a type, found '^'
error: FILE:21:13: syntax-error: expected ':' or '=' after the pattern, found 'watch'
0/1 clean
```

### Block 2

```console
error: FILE:1:13: syntax-error: expected a generic parameter name, found '^'
error: FILE:2:13: syntax-error: expected a type, found '^'
error: FILE:4:6: syntax-error: expected a generic parameter name, found '^'
error: FILE:8:20: syntax-error: expected a type, found '^'
error: FILE:9:13: syntax-error: expected ':' or '=' after the pattern, found 'cursor'
0/1 clean
```

### Block 3

```console
error: FILE:2:15: syntax-error: expected a parameter name, found '^'
error: FILE:4:16: syntax-error: expected a type, found '^'
error: FILE:7:22: syntax-error: expected a type, found '^'
error: FILE:11:16: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 4

```console
error: FILE:3:17: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 5

```console
error: FILE:1:23: syntax-error: expected ',' or ']', found 'Any'
error: FILE:4:21: syntax-error: expected ',' or ')', found 'List'
0/1 clean
```

### Block 6

```console
error: FILE:6:29: syntax-error: expected ',' or ')', found 'Batch'
0/1 clean
```

### Block 7

```console
1/1 clean
```

### Block 8

```console
1/1 clean
```

## Repository Validation

Origin was fetched. An isolated worktree at `7e85e38b0` was rebased onto
`origin/main`; only this report and its README entry were copied in for
the documentation checks. The pinned ownership record remained unchanged.

| Check | Result |
| --- | --- |
| Raw parser runs | W7 and W8 parse; W1–W6 reject unsupported proposed syntax |
| Projected parser runs | All eight parse |
| `bash spec/check.sh` | Passed; links and anchors, zero failing dead citations, 5,963 rule IDs, 1,564 diagnostic rows |
| `pnpm run website:build` | Passed; 79 pages, 265 grammar rules, 266 definitions |
| Whitespace checks | Passed for the README diff and new report |

The spec check reported eight existing dead-citation warnings and six
citations allowed as history; none failed. Final validation prose changes
no example. No ownership type-check or runtime implementation is claimed.

Only this report and its README entry were written. The design record,
specification, fixtures, and compiler were not edited. Nothing was
staged, committed, or pushed.
