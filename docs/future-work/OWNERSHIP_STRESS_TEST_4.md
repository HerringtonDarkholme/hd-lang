# Ownership And Borrows: Adversarial Review 4

Status: design review, 2026-10-16; changes no decision, design record,
spec text, or compiler code. Nothing here is accepted behavior.
This reviews [Ownership And Borrows](OWNERSHIP_DESIGN.md) at commit
`334aec98d`, including the structural-derivation decision in `281c34d5c`,
against [review 3](OWNERSHIP_STRESS_TEST_3.md).

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

The owner now chose post-substitution mode normalization. A generic
position becomes owned when its instantiated type is `OwnOnly`; concrete
source still writes its ownership modes. The earlier complaint that no
such translation exists is therefore resolved.

`OwnOnly` by `Structure` is explicitly compiler-known. Its condition is
structural rather than a collection of ordinary conditional impls.
The intermediate decision to permit overlapping marker impls is not
the surface under test.

`defer` bodies require an empty requirement row. Fetching a plain provider
first and capturing that value is the chosen workaround. This review
does not ask for implicit provider capture or restore the rejected body.

| Current policy | Review treatment |
| --- | --- |
| `OwnOnly` and position modes coexist | Preserve both; test their composition |
| No partial moves | Preserve the chosen restriction |
| Drop keeps held borrows live | Treat previous destructor-liveness probes as resolved |
| Providers are plain only | Exclusive services remain parameters |
| Function-borrowed globals never move | Treat the earlier static-accessor attack as resolved |
| Exclusive captures are one-shot | Named producers remain the chosen route to repeated exclusive work |
| Plain aliases and plain captures remain unchecked | Do not claim universal alias freedom |

Dependencies span [types and generics](../../spec/lang/04-type-system.md),
[cleanup](../../spec/lang/06-control-flow.md#deferred-cleanup),
[method evaluation](../../spec/lang/07-functions.md#methods-and-receivers),
[data](../../spec/lang/08-data-and-enums.md),
[trait coherence](../../spec/lang/09-traits.md#overlap),
[typed derivation](../../spec/lang/14-annotations.md#typed-derivation),
and [requirements and suspension](../../spec/lang/11-requirements-and-suspension.md).
The stdlib-tier dependencies are [collections](../../spec/std/collections.md),
[iterators](../../spec/std/iter.md), and [tasks](../../spec/std/task.md).
These chapters supply the existing integration surface, not an ownership
implementation.

## Method

Eight cases cover three repaired boundaries, a basic ownership control,
receiver evaluation, structural classification, generic body checking,
and the cost of moving captures into cleanup guards. Foreign code is a
short approximation of a library shape.

| Verdict | Meaning |
| --- | --- |
| Works | The selected rules supply the intended operation or rejection |
| Friction | A conservative or practical answer exists, but its cost or checking contract needs to be stated |
| Breaks | A required safety contract is missing from the written rule |

Every hd block is parsed as written and after erasing proposed syntax.
Parsing checks grammar only. No ownership implementation, executable
exploit, benchmark, or writing trial is claimed.

The same eleven dimensions remain; migration is excluded. A formalization
detail is distinguished from a request for a new mechanism. The report
does not inflate every implementation question into a new safety hole.

## Summary

| Case | Reference shape | Verdict | Finding |
| --- | --- | --- | --- |
| X1 | Consuming optional extraction | Works | Post-substitution normalization resolves the generic outer result mode |
| X2 | Nested aggregate and two-payload result | Works | One structural derivation replaces the previous field/coherence gaps |
| X3 | Empty-row scope guard | Works | The selected rejection and captured-provider workaround are explicit |
| X4 | Moved resource and dropped collection | Works | Existing ownership/drop guarantees remain intact |
| X5 | Mutable receiver with nested argument evaluation | Breaks | Delayed activation needs receiver reservation or an equivalent safety rule |
| X6 | Structural ownership through borrowed and recursive fields | Friction | A mode-aware classifier and recursive policy need definition |
| X7 | Generic copying of a potentially owned wrapper | Friction | Body checking must follow normalized composite modes; rejection timing is unspecified |
| X8 | Cleanup closure capturing an owner | Friction | Registering the guard moves the owner; an explicit shared view is a workaround |

The result is **four works, three friction, and one break**. The break
is a receiver-evaluation contract gap, not a demonstrated compiler exploit.
The two classification/checking cases primarily demand a precise
algorithm and diagnostics; they do not establish that a new feature
is necessary.

This revision resolves the previous round's central objections. The
remaining review is closer to defining a small ownership checker than
choosing the overall resource model. The most consequential new probe
concerns the period before a receiver's mutable borrow becomes active.

## Cases

### X1. Optional Results Normalize After Substitution

Original shape: consuming extraction returns the first item and drops
the remainder.

```rust
fn take_first<T>(xs: Vec<T>) -> Option<T> {
    xs.into_iter().next()
}
```

```text
fn take_first[T < own Any](xs: own List[T]) -> Option[T]: # hypothetical syntax
    xs.into_iter().next()

fn take_connection(xs: own List[own Conn]) -> own Option[own Conn]: # hypothetical syntax
    take_first(xs)
```

**Generated behavior.** With `T = own Conn`, structural membership
makes `Option[T]` owned-only, and generic normalization supplies its
outer owner mode. The concrete signature writes that resulting mode
explicitly. With plain primitive elements, the optional result stays
plain.

**Verdict: works.** OWN-S10's original result-mode objection is resolved
at the design-policy level. The compiler still needs a canonical
normalized type and suitable diagnostics; that is no longer an absent
owner decision.

[Rust Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.pop)
provides the extraction shape. hd's chosen normalization now explains
its additional mode behavior.

### X2. Nested Fields And Either-Owned Outcomes Derive Structurally

Original shapes: a batch contains a vector, and either result payload
may hold a resource.

```rust
struct Batch<T> { values: Vec<T> }
enum Outcome<A, B> { Good(A), Bad(B) }
```

```text
data Batch[T]:
    values: List[T]

impl OwnOnly for Batch[T] by Structure              # hypothetical syntax

enum Outcome[A, B]:
    Good(value: A)
    Bad(error: B)

impl OwnOnly for Outcome[A, B] by Structure         # hypothetical syntax

fn accept_batch(batch: own Batch[own Conn]) -> void: # hypothetical syntax
    pass

fn accept_result(result: own Outcome[own Conn, string]) -> void: # hypothetical syntax
    pass
```

**Generated behavior.** The generic list field normalizes to an owned
list when its element is owned. That field makes the batch owned-only.
The outcome's one derivation checks both payload types, so no pair of
overlapping user impls is required.

**Verdict: works.** OWN-S10's nested-field objection and OWN-S11's
coherence objection are resolved. The outcome remains owned-only even
when its current variant carries only the plain error. That is a
conservative type-level property, not a path-sensitive ownership test.

This answer is an explicit compiler intrinsic. It should not be
described as an ordinary std derivation template or as unchanged
conditional-impl coherence.

### X3. Cleanup Requirements Are Explicitly Closed By The Caller

Original shape: a scope guard calls a captured logger.

```rust
let _guard = scopeguard::guard((), |_| logger.note());
```

```text
trait Audit:
    fn note(self) -> void

fn record() -> void $ Audit:
    $.use(Audit).note()

fn accepted() -> void $ Audit:
    audit := $.use(Audit)
    defer: audit.note()

fn rejected() -> void $ Audit:
    defer: record()                                # rejects: defer-needs-row
```

**Generated behavior.** `accepted` captures a plain provider value in
an empty-row callback. `rejected` retains an unsatisfied row in its
cleanup body and is rejected by the new explicit diagnostic.

**Verdict: works.** OWN-S12 is resolved through a chosen restriction.
The workaround is more explicit and can require unwrapping a helper
that previously used implicit providers, but it supplies a valid
ordinary callback environment.

The comparison is [scopeguard](https://docs.rs/scopeguard/latest/scopeguard/).
No automatic capture of an arbitrary requirement row is assumed.

### X4. Ownership Still Protects Movement And Collection Cleanup

Original shape: Rust transfers ownership and drops discarded elements.

```rust
consume(file);
connections.clear();
```

```text
fn consume(file: own File) -> void: pass            # hypothetical syntax

fn late_use() -> void:
    file := open("input")
    consume(file)
    _ := size(file)                                # rejects: use-after-move

fn clear_connections(xs: ^mut List[own Conn]) -> void: # hypothetical syntax
    xs.clear()
```

**Generated behavior.** `consume` drops its parameter. The caller's
later read rejects. Clearing the list drops removed owned elements,
and the caller retains its now-empty container.

**Verdict: works.** Structural ownership does not replace ordinary move
or drop accounting. Earlier cancellation-buffer, destructor-liveness,
plain-provider, and frozen-global decisions remain in force too.

### X5. Receiver Activation Cannot Be The First Protection

Original shape: Rust permits a shared read during receiver reservation,
but not a conflicting mutation that invalidates the selected receiver.

```rust
values.push(values.len()); // accepted two-phase borrow
// A conflicting mutable operation during reservation rejects.
```

```text
data Holder:
    file: own File                                # hypothetical syntax

impl OwnOnly for Holder                           # hypothetical syntax

impl File:
    fn inspect_after(^mut self, ignored: void) -> usize: # hypothetical syntax
        sys_size(self.fd)

fn reset(holder: ^mut Holder) -> void:             # hypothetical syntax
    holder.file = open("replacement")

fn bad(holder: own Holder) -> usize:               # hypothetical syntax
    holder.file.inspect_after(reset(holder))
```

**Generated behavior.** Existing method evaluation selects the receiver
before evaluating ordinary arguments. Read literally, the study starts
the receiver loan only after those arguments. `reset` then replaces
the owned field and drops its original file before the saved receiver
is used.

**Verdict: breaks; receiver reservation is unstated.** A sound checker
must reject the conflict. The selected receiver needs protection during
argument evaluation, even before full mutable activation. Merely checking
that `holder` still exists afterward misses the replaced child.

Re-reading `holder.file` after `reset` is not an equivalent repair:
it calls the method on a different object and violates receiver-first
evaluation. Starting a full mutable loan immediately is sound but loses
the study's desired `xs.push(xs.len())` behavior.

The same boundary appears in a whole-owner call such as
`file.inspect_after(drop(file))`. Move checking can catch that simpler
case if activation counts as a use; the replaced-field case explains
why the root alone is insufficient.

Rust's [two-phase borrow documentation](https://rustc-dev-guide.rust-lang.org/borrow-check/two-phase-borrows.html)
distinguishes reservation from activation. The study currently states
only the delayed activation idea. No accepted hd runtime exploit is
claimed: an implementation that already treats receiver selection as
reservation rejects this program correctly.

### X6. The Structural Predicate Needs Mode-Aware Base Cases

Reference shape: a shared reference can be copied even when its referent
cannot, whereas an exclusive reference cannot be duplicated.

```rust
struct Shared<'a> { file: &'a File }
struct Exclusive<'a> { file: &'a mut File }
```

```text
@derive(OwnOnly)
data Shared[^a]:                                   # hypothetical syntax
    file: ^{a} File                               # hypothetical syntax

@derive(OwnOnly)
data Exclusive[^a]:                                # hypothetical syntax
    file: ^{a} mut File                           # hypothetical syntax

@derive(OwnOnly)
data Token[T]:
    id: usize

@derive(OwnOnly)
data Chain[T]:
    value: T
    next: Option[Chain[T]]
```

**Generated behavior required by the chosen model:**

| Shape | Intended classification obligation |
| --- | --- |
| `Shared` over `File` | The borrowed field must not inherit the referent's ownership obligation; the view can remain duplicable |
| `Exclusive` over `File` | The exclusive field must make the holder owned-only, or derivation must reject and require the manual marker |
| `Token[own File]` | No field holds the file, so field-based derivation should not invent a resource obligation from the unused argument |
| `Chain[i32]` | A recursive ownership equation with no owning leaf needs the plain solution |
| `Chain[own File]` | Its owning leaf must propagate through recursive container structure |

**Verdict: friction; classifier formalization.** Testing the referent's
nominal marker through every borrow would overclassify shared views.
Ignoring borrow modes entirely misses exclusive fields. Recursion also
needs a terminating policy, naturally a least fixed point for these
positive examples.

This does not prove a flaw in the chosen structural mechanism. One
small mode-aware predicate may settle all these cases. It must be
written before the compiler can implement “some field's type is” as
anything more than an informal instruction.

[Rust Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html) documents
the distinction between shared and exclusive reference copying. hd need
not add that trait; its existing modes can provide the base cases.

### X7. A Normalized Composite Can Be Move-Only Inside A Generic Body

Original shape: copying a generic wrapper requires duplicability of the
wrapper, not merely a rule about direct uses of its element.

```rust
#[derive(Copy, Clone)]
struct Boxed<T> { value: T }
fn duplicate<T: Copy>(x: Boxed<T>) -> (Boxed<T>, Boxed<T>) { (x, x) }
```

```text
data Boxed[T]:
    value: T

impl OwnOnly for Boxed[T] by Structure              # hypothetical syntax

fn duplicate_box[T < own Any](x: Boxed[T]) -> (Boxed[T], Boxed[T]): # hypothetical syntax
    (x, x)

fn plain_duplicate[T](x: Boxed[T]) -> (Boxed[T], Boxed[T]):
    (x, x)
```

**Generated behavior.** With an owned element type, `x` itself becomes
an owner after normalization. The first tuple element moves it, and the
second use must reject. A generic body cannot escape that obligation
by never spelling a local of bare type `T`.

**Verdict: friction; the safe rejection is clear.** A conservative
definition-time checker rejects `duplicate_box` because one allowed
instantiation is owned. `plain_duplicate` supplies the plain-only API.
Alternatively, instance-specific checking can reject only owning calls,
but then diagnostic timing and exported generic guarantees differ.

The record chooses modes after substitution but does not yet state
which checking policy it uses for dependent composite modes. The existing
rule that ownership-enabled bodies never copy `T` should extend to every
potentially owned composite, not just values spelled `T`.

No accepted owner duplication is demonstrated. Both sound policies
reject the owning use; this is a modularity, diagnostic, and compiler
architecture question. It does not require reopening the generic bound
spelling or the new normalization decision.

### X8. A Defer Over An Owner Moves That Owner At Registration

Reference shape: a Rust guard can borrow the object used by its cleanup.

```rust
let _guard = scopeguard::guard(&file, |file| inspect(file));
inspect(&file);
```

```text
fn inspect(file: ^File) -> void:                   # hypothetical syntax
    _ := size(file)

fn moved_into_cleanup() -> void:
    file := open("input")
    defer: inspect(file)
    inspect(file)                                 # rejects: defer's closure captured the owner

fn borrowed_cleanup() -> void:
    file := open("input")
    let view: ^File = file                         # hypothetical syntax
    defer: inspect(view)
    inspect(file)
```

**Generated behavior.** The first guard's closure captures an owner,
so the capture moves it at registration. The second captures a shared
borrow instead. Its cleanup runs before the earlier file owner drops,
and the file remains available for shared reads.

**Verdict: friction; accepted capture-policy cost.** The shared-view
workaround uses existing rules and needs no new syntax. The design
should show this consequence beside the empty-row cleanup example,
since `defer` otherwise looks like a late use of the outer binding.

This is not the former linear reservation rule. The new closure owns
the value immediately; the caller chooses a borrow when it needs to
retain the original owner. No change to capture policy is proposed.

## Comparison With Other Languages

| Mechanism | Evidence | Relevance |
| --- | --- | --- |
| Receiver reservation and activation | [Rust two-phase borrows](https://rustc-dev-guide.rust-lang.org/borrow-check/two-phase-borrows.html) | Shared argument reads can be allowed while conflicting mutation is excluded |
| Mode-aware structural classification | [Rust Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html) | A reference's copying rule differs from the referent's resource policy |
| Closed cleanup environment | [scopeguard](https://docs.rs/scopeguard/latest/scopeguard/) | Capturing an explicit provider or reference supplies an ordinary cleanup callback |
| GC plus usage modes | [OxCaml modes](https://oxcaml.org/documentation/modes/intro/) and [uniqueness](https://oxcaml.org/documentation/uniqueness/intro/) | Valid precedent for separating ordinary GC values from checked usage modes, not a proof of hd's destructor system |

The new OxCaml comparison is useful. Its documentation distinguishes
locality, uniqueness, and closure-use modes, including explicit field
modalities. hd should retain that distinction when explaining plain
descendants and owned containers.

Two qualifications matter. OxCaml says local values *may* be stack
allocated, not that locality guarantees placement. Its uniqueness
documentation also records an implementation limitation involving
pattern matching and destructive operations. Therefore neither the
analogy nor the paper establishes optimizer correctness for hd.

The DOI landing page for the cited ICFP paper was unavailable during
this review. The comparison above uses the accessible official OxCaml
documentation and makes no claim of having checked that paper's proof.

## Problems, Ranked

The prior round's three primary findings are resolved. The remaining
items have different status: one dangerous omitted receiver contract,
two formalization/checking questions, and one explicit ergonomics cost.

| Rank | ID | Problem | Cases | Status |
| --- | --- | --- | --- | --- |
| 1 | OWN-S13 | Preserve the selected receiver before mutable activation | X5 | Required safety contract |
| 2 | OWN-S14 | Define mode-aware, recursive structural ownership | X6 | Classifier specification |
| 3 | OWN-S15 | Check moves for dependent composite modes in generic bodies | X7 | Modularity and diagnostics policy |
| 4 | OWN-S16 | Explain owner-moving cleanup captures | X8 | Documentation/ergonomics |

### OWN-S13. Reservation Must Protect The Receiver's Place

**Effect.** X5 can replace and drop a selected owned child before its
mutable method call starts. A delayed exclusive loan is not enough;
the earlier selected place needs a reservation or an equivalent
invalidation check.

| Candidate question | Cost and tier | Effect |
| --- | --- | --- |
| Should implicit mutable receivers reserve their place during argument evaluation, allowing shared reads but rejecting conflicting mutation and moves, then activate exclusively? | One general language semantic rule, language tier | Preserves the chosen nested-read convenience and receiver-first semantics |
| Should receiver borrowing begin immediately, with users computing read-only arguments in separate statements? | A language semantic restriction plus ordinary caller restructuring | Simpler checker, but new evidence would justify revisiting the previously chosen convenience |

The first candidate is the closest completion of the stated intent.
The second is a fallback exposed by this new invalidation case, not a
selected reversal. Neither needs a keyword or named compiler intrinsic.
The rule should also define reservation across suspending arguments.

### OWN-S14. Structural Ownership Is A Predicate Over Modes And Shapes

**Effect.** X6 separates a borrowed referent's marker from the containing
field's ownership obligation. It also tests unused arguments and recursive
structures. These are inputs to one classifier, not reasons to add
separate ad hoc container exceptions.

**Candidate question.** Should the classifier explicitly distinguish
shared borrows, exclusive borrows, explicit owners, plain nominal types,
and recursive structural types, with least-fixed-point solving for
positive recursive definitions? This is a language semantic definition
of the chosen intrinsic, not new syntax or another intrinsic.

If derived exclusive-borrow holders are initially unsupported, they
can require the existing manual `OwnOnly` marker. That is a documented
restriction; silently treating the holder as duplicable is not an answer.
Optimization-specific uniqueness of plain descendants remains separate.

### OWN-S15. State The Generic Body-Checking Contract

**Effect.** X7 is safe only if potentially owned composites receive
move checking too. The observable question is whether the generic
definition rejects or a particular owning instantiation rejects.

| Candidate question | Cost and tier | Effect |
| --- | --- | --- |
| Should generic bodies prove correctness for all allowed modes at definition time, conservatively treating dependent owned-capable composites as move-only? | A language semantic body-checking rule | Clear modular guarantee; the plain-only helper is the ordinary-library workaround |
| Should dependent bodies be checked per instantiation, with exported metadata describing that obligation? | A language semantic instantiation/checking rule | More instance-specific acceptance; later diagnostics and additional checking/caching costs |

This question does not ask the owner to choose normalization again.
It specifies where the already-chosen normalized modes constrain body
operations. Trait method calls, captures, and drops must use the same
policy, not only direct tuple construction.

### OWN-S16. Show The Borrowed Cleanup Idiom

**Effect.** X8 rejects a later owner use because registration moved
the owner into the closure. Borrowing it explicitly preserves the
caller workflow and the chosen capture rules.

**Candidate question.** Should the guide pair the owned-capture example
with the explicit shared-view cleanup idiom, and should diagnostics
identify the capture that moved the owner? This is documentation and
diagnostics over existing language rules, with no new mechanism.

## Evaluation By Dimension

| Dimension | Change since review 3 | Current assessment |
| --- | --- | --- |
| Guarantee clarity | Generic mode normalization and structural membership now have explicit policies | Stronger; distinguish the classifier, move checking, actual resource closing, and optimizer alias facts |
| Soundness | Previous result-mode, nested-field, coherence, and cleanup-row objections have answers | No executable escape demonstrated. Receiver reservation remains a consequential omitted contract; structural/body rules need precise checking definitions |
| Precision | Generic wrappers can retain shared APIs across plain and owned arguments | Better; conservative generic body checking and no-partial-move policy still reject useful cases; closure cleanup captures can move earlier than readers expect |
| Practical expressiveness | Optional results, nested batches, two-payload outcomes, and explicit-provider guards now compose in principle | More convincing; reusable owned lazy producers and trait/container APIs remain the declared open work |
| Composability | A general normalization rule and structural classifier replace the previous local patches | Significant improvement. Evaluation order and mode-aware body operations are the next boundaries to nail down |
| Modular contracts | The generic exported result mode is now computable from type arguments | State whether body validity is universal or instance-specific; classifier base cases and canonical types must be consistent |
| Agent writability and human readability | Fewer contradictory contracts; structural derivation is easier than overlapping impls | Better, with new implicit generic modes and owner-moving captures to explain. No writing trial performed |
| Conceptual cost | Structural derivation removes conditional-impl coherence machinery | Better unification; still needs reservation/activation, structural mode classification, and a generic checking policy |
| Compiler feasibility and cost | Normalization and one intrinsic give concrete implementation tasks | Closer to a semantic prototype. Recursive classification, symbolic generic checking, cancellation/drop lowering, and diagnostics still need design; no benchmark |
| Optimization validity and payoff | OxCaml supplies a relevant precedent for GC plus modes | Analogy is useful, not a license to reorder reads across destructive operations. Plain sharing and destructor effects still constrain optimization; payoff unmeasured |
| Specification readiness | The central policies are increasingly settled | Closer to prototype and focused formalization; a complete spec still needs the receiver rule, classifier, body-checking contract, and declared iterator work |

The judgment is **substantially more coherent, with the remaining
work shifting toward precise checking rules and a semantic prototype**.
The previous critique of missing generic ownership policy should be
retired. It should not be recreated merely because the chosen policy
still needs an implementation algorithm.

## Earlier Findings

| Review-3 finding | Current status |
| --- | --- |
| W1 destruction and partial moves | Still resolved by the existing conservative rules |
| W2 exclusive borrowed holders | Still resolved by required owned-only classification |
| W3 exclusive providers | Still resolved by plain-only installation and explicit parameters |
| W4 static accessor/global movement | Still resolved conservatively |
| W5 optional generic result / OWN-S10 | Resolved by post-substitution normalization |
| W6 nested field mode / OWN-S10 | Resolved by the same rule across generic positions |
| W7 conditional coherence / OWN-S11 | Resolved by replacing conditional marker impls with one compiler-known structural derivation |
| W8 effectful defer / OWN-S12 | Resolved by explicit empty-row rejection and provider capture workaround |

Some prose still predates decisions: 3.4 says a data type never owns a
handle despite the restored owning Pool, and the annotation-count claim
has not been remeasured after the marker/derivation changes. These are
editorial issues, not additional runtime failures.

## Parse Log

All eight `text` blocks were passed to
`scripts/hd-conformance.sh parse FILE`. X3 parses as written. The other
seven reject proposed syntax at marked lines. All eight projections
parse. Nothing here was ownership-checked or executed.

Projection preserves ordinary operations, comments, and line breaks.
Before comments, it erases `own`, `^`, lifetime bounds and declarations,
then empty named lifetime applications. Hypothetical `OwnOnly` impl
lines become comments; `@derive(OwnOnly)` remains ordinary parseable
annotation syntax. This verifies surrounding grammar only.

| Block | Case | Raw | Projected |
| --- | --- | --- | --- |
| 1 | X1 | Rejects proposed syntax | Parses |
| 2 | X2 | Rejects proposed syntax | Parses |
| 3 | X3 | Parses | Parses |
| 4 | X4 | Rejects proposed syntax | Parses |
| 5 | X5 | Rejects proposed syntax | Parses |
| 6 | X6 | Rejects proposed syntax | Parses |
| 7 | X7 | Rejects proposed syntax | Parses |
| 8 | X8 | Rejects proposed syntax | Parses |

Raw diagnostics follow verbatim, replacing the temporary pathname with
`FILE`. X3 returned status 0; raw rejections returned status 101. Every
projected run returned status 0 and `1/1 clean`.

### Block 1

```console
error: FILE:1:23: syntax-error: expected ',' or ']', found 'Any'
error: FILE:4:28: syntax-error: expected ',' or ')', found 'List'
0/1 clean
```

### Block 2

```console
error: FILE:12:28: syntax-error: expected ',' or ')', found 'Batch'
error: FILE:15:30: syntax-error: expected ',' or ')', found 'Outcome'
0/1 clean
```

### Block 3

```console
1/1 clean
```

### Block 4

```console
error: FILE:1:22: syntax-error: expected ',' or ')', found 'File'
error: FILE:8:26: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 5

```console
error: FILE:2:15: syntax-error: expected a line ending, found 'File'
error: FILE:7:22: syntax-error: expected a parameter name, found '^'
error: FILE:10:18: syntax-error: expected a type, found '^'
error: FILE:13:20: syntax-error: expected ',' or ')', found 'Holder'
0/1 clean
```

### Block 6

```console
error: FILE:2:13: syntax-error: expected a generic parameter name, found '^'
error: FILE:3:11: syntax-error: expected a type, found '^'
error: FILE:6:16: syntax-error: expected a generic parameter name, found '^'
error: FILE:7:11: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 7

```console
error: FILE:6:26: syntax-error: expected ',' or ']', found 'Any'
0/1 clean
```

### Block 8

```console
error: FILE:1:18: syntax-error: expected a type, found '^'
error: FILE:11:15: syntax-error: expected a type, found '^'
0/1 clean
```

## Repository Validation

Origin was fetched. Checks ran in an isolated worktree at `334aec98d`,
rebased onto `origin/main`, with this report and its README entry copied
in. The ownership record stayed unchanged throughout the review.

| Check | Result |
| --- | --- |
| Raw parser runs | X3 parses; seven blocks reject unsupported proposed syntax |
| Projected parser runs | All eight parse |
| `bash spec/check.sh` | Passed; links and anchors, zero failing dead citations, 6,002 rule IDs, 1,564 diagnostic rows |
| `pnpm run website:build` | Passed; 79 pages, 265 grammar rules, 266 definitions |
| Whitespace checks | Passed for the README diff and new report |

The spec check reported nine warning citations and seven allowed as
history; none failed. Final validation prose changes no example.
No ownership type-checking or runtime safety validation is claimed.

Only this report and its README entry were written. The design record,
specification, compiler, and fixtures were left untouched. Nothing was
staged, committed, or pushed.
