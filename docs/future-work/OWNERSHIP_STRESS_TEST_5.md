# Ownership And Borrows: Adversarial Review 5

Status: design review, 2026-10-16; changes no decision, design record,
spec text, or compiler code. Nothing here is accepted behavior.
This reviews [Ownership And Borrows](OWNERSHIP_DESIGN.md) at `b3711481e`,
against [review 4](OWNERSHIP_STRESS_TEST_4.md).

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

The revision answers every review-4 finding. Mutable receivers reserve
their place before arguments run. Structural classification now specifies
shared borrows, exclusive fields, unused arguments, and recursive types.
Generic bodies are checked once, for every admitted mode.

The generic rule additionally says **every value whose type mentions
`T` is move-only**. This review tests that quantifier against shared
borrows and plain function values. It does not reopen definition-time
checking or the ownership-enabled bound.

The new defer example explicitly borrows before registering cleanup.
Capturing an owner still moves it immediately. Plain aliases remain
unchecked; exclusive captures remain one-shot; partial moves remain
forbidden. Migration is excluded.

Dependencies include [type forms and generics](../../spec/lang/04-type-system.md),
[expressions](../../spec/lang/05-expressions.md),
[match guards](../../spec/lang/06-control-flow.md#arms-and-guards),
[functions and captures](../../spec/lang/07-functions.md),
[data and enums](../../spec/lang/08-data-and-enums.md),
[traits](../../spec/lang/09-traits.md),
[suspension](../../spec/lang/11-requirements-and-suspension.md), and
[derivation](../../spec/lang/14-annotations.md#typed-derivation).
The stdlib dependencies are [collections](../../spec/std/collections.md),
[option](../../spec/std/option.md), [iteration](../../spec/std/iter.md),
and [tasks](../../spec/std/task.md).

These are integration surfaces, not an implementation of ownership.
In particular, today's specification says matching transfers no ownership;
the proposed consuming patterns necessarily change that policy.

## Method

Nine cases combine regression controls with new composition attacks.
Foreign snippets approximate standard Rust APIs or the scopeguard crate;
they are small explanatory shapes, not copied implementations.
Undefined hd helpers have the contracts stated beside each case.

| Verdict | Meaning |
| --- | --- |
| Works | Existing chosen rules determine the intended operation or rejection |
| Friction | A safe restriction or workaround exists, with an unresolved contract or practical cost |
| Breaks | Two written guarantees require incompatible answers |

Counts are by case, not by function or rejected line. No annotation
count, performance result, writing trial, ownership type-check, or runtime
exploit is claimed. Every hd block is parsed raw and after erasing
proposed syntax; a projected parse checks surrounding grammar only.

## Summary

| Case | Reference shape | Verdict | Finding |
| --- | --- | --- | --- |
| Y1 | Vec nested receiver arguments | Works | Reservation protects the selected place and allows completed shared reads |
| Y2 | Mutable method receiving an alias of itself | Works | Activation must apply the ordinary live-borrow conflict rules |
| Y3 | Shared view, phantom parameter, recursive wrapper | Works | The new classifier base cases give consistent answers |
| Y4 | Generic wrapper duplication | Works | Definition-time rejection closes the previous checking-policy question |
| Y5 | Duplicating a shared reference to noncopyable data | Breaks | Shared-borrow copying conflicts with the blanket generic rule |
| Y6 | Duplicating a factory that produces owners | Friction | A result mentioning T does not make the factory's environment owned |
| Y7 | Borrowed scope guard | Works | Explicit view preserves ownership, but keeps the shared loan until cleanup |
| Y8 | Consuming enum pattern with a fallible guard | Friction | Move commitment on guard failure needs a contract |
| Y9 | mem::replace through an exclusive borrow | Friction | Rejection is safe; a general replacement API needs compiler support or a different representation |

The result is **five works, three friction, and one contract break**.
No accepted double ownership or dangling reference is demonstrated.
The strongest new issue is overconservative generic wording, not a
reason to abandon universal checking. The other two questions concern
pattern lowering and the boundary of ordinary library implementation.

The design continues to improve. The receiver fix is substantive, and
the structural classifier is now much more than an informal slogan.
The remaining work should concentrate on a small semantic prototype,
with explicit accept/reject cases for these compositions.

## Cases

### Y1. Reservation Protects The Selected Place

Reference shape: Vec permits a nested shared read, but a receiver
cannot be invalidated while its arguments are being evaluated.

```rust
v.push(v.len());
holder.file.inspect_after(reset(&mut holder)); // must reject
```

```text
fn nested_read(xs: ^mut List[usize]) -> void:             # hypothetical syntax
    xs.push(xs.len())

fn invalidate(holder: own Holder) -> void:                # hypothetical syntax
    holder.file.inspect_after(reset(holder))
```

Assume Holder owns its file; reset requires `^mut Holder` and replaces
that file. inspect_after has an exclusive receiver and accepts reset's
plain result. These are the same contracts as review 4's X5.

**Generated behavior.** Reserve xs, read its length, end that read,
activate the exclusive receiver, then push. For the second call, reserve
holder.file before evaluating reset. Taking an exclusive borrow of its
containing Holder conflicts with that reservation.

**Verdict: works.** The old invalidation objection is resolved. This
control should be retained when lowering arguments that suspend: the
reservation lasts through argument evaluation, including suspension.
Cancellation must discard pending argument state before dropping owners
it borrows, under the existing frame/drop rules.

### Y2. A Read That Survives Into The Call Still Conflicts

Reference shape: the returned reference remains live as a method argument.

```rust
fn same(x: &File) -> &File { x }
file.compare(same(&file)); // compare takes &mut self and &File: rejects
```

```text
fn same(file: ^File) -> ^File: file                       # hypothetical syntax

impl File:
    fn compare(^mut self, other: ^File) -> bool:          # hypothetical syntax
        sys_compare(self.fd, other.fd)

fn overlapping(file: own File) -> bool:                  # hypothetical syntax
    file.compare(same(file))
```

**Generated behavior.** Reservation permits the shared argument borrow.
That borrow survives until compare returns, so exclusive activation
conflicts with it. Unlike len's primitive result in Y1, the argument
still carries the receiver's lifetime.

**Verdict: works, with required rejection.** Rules 4.3.3 and 4.3.4
already forbid the overlap; this is not another missing mechanism.
The checker must apply those rules at activation, rather than treating
all reads during reservation as automatically harmless.

[Rust's checker description](https://rustc-dev-guide.rust-lang.org/borrow-check/two-phase-borrows.html)
explicitly performs a conflict check at activation. hd's equivalent
belongs in the same receiver rule or its lowering specification.

### Y3. Structural Ownership Stops At A Shared Borrow

Reference shapes: borrowed views and phantom parameters need not inherit
the referent's resource obligations; recursive ownership follows stored data.

```rust
struct View<'a, T> { value: &'a T }
struct Token<T> { id: usize, marker: std::marker::PhantomData<T> }
struct Chain<T> { value: T, next: Option<Box<Chain<T>>> }
```

```text
@derive(OwnOnly)
data View[T, ^a]:                                        # hypothetical syntax
    value: ^{a} T                                       # hypothetical syntax

@derive(OwnOnly)
data Token[T]:
    id: usize

@derive(OwnOnly)
data Chain[T]:
    value: T
    next: Option[Chain[T]]
```

**Generated behavior.** A View over an owned element remains a shared
view with a checked lifetime. Token's unused argument creates no owning
field. Chain with plain elements stays plain; an owning leaf taints
the owning recursive instance. hd's GC representation removes Rust's
need for an explicit Box in this shape.

**Verdict: works.** The least fixed point selects the plain solution
without an owning leaf. The explicit shared-borrow cutoff prevents
spurious taint. Exclusive fields still require the chosen manual marker;
this review does not restore automatic derivation for them.

### Y4. Potentially Owned Composites Reject Duplication At Definition

Reference shape: Rust requires a copying bound before duplicating a wrapper.

```rust
struct Boxed<T> { value: T }
fn duplicate<T>(x: Boxed<T>) -> (Boxed<T>, Boxed<T>) {
    (x, x) // rejects
}
```

```text
@derive(OwnOnly)
data Boxed[T]:
    value: T

fn duplicate[T < own Any](x: Boxed[T]) -> (Boxed[T], Boxed[T]): # hypothetical syntax
    (x, x)

fn plain_duplicate[T](x: Boxed[T]) -> (Boxed[T], Boxed[T]):
    (x, x)
```

**Generated behavior.** Check duplicate once over all admitted modes.
Some allowed Boxed instances own their field, so x cannot be duplicated.
The plain-only helper admits duplicable instances and aliases them as
ordinary hd objects.

**Verdict: works.** The rejection occurs at the definition. There is
no longer an unresolved choice between universal and per-instantiation
body validity. Generic drop dispatch remains the separately declared
implementation question in section 6 of the design.

### Y5. The Same Blanket Rule Also Rejects Shared-Borrow Copies

Reference shape: a shared reference copies even when its referent does not.

```rust
fn twice<T>(x: &T) -> (&T, &T) { (x, x) }
```

```text
fn twice[T < own Any](x: ^T) -> (^T, ^T):                 # hypothetical syntax
    (x, x)

fn concrete_twice(x: ^File) -> (^File, ^File):            # hypothetical syntax
    (x, x)
```

**Generated behavior required by borrowing.** Both functions copy one
shared loan, retaining its lifetime in both tuple elements. With an
owned argument type, `^T` normalizes to `^U`; it never becomes an owner.
No referent or destructor obligation is duplicated.

**Generated behavior required by literal 4.6.2.** The generic x's type
mentions T, so x is move-only. The tuple's second use rejects, even
though 4.3.6 says a shared borrow copies.

**Verdict: breaks the written contract.** A checker must choose which
rule wins. Choosing rejection is safe, but makes generic shared borrows
behave differently from concrete ones. This is a precision and modular
contract defect, not a demonstrated memory-safety exploit.

The plain-only `[T]` workaround loses the owned instantiations that the
helper exists to support. This new evidence challenges the blanket
quantifier; it does not challenge universal definition-time checking.
[Rust Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html) explicitly
distinguishes shared-reference copying from referent copyability.

### Y6. A Factory's Return Type Does Not Describe Its Captures

Reference shape: copying a plain function pointer does not copy its result.

```rust
fn duplicate_factory<T>(make: fn() -> T) -> (fn() -> T, fn() -> T) {
    (make, make)
}
```

```text
fn duplicate_factory[T < own Any](make: fn() -> T) -> (fn() -> T, fn() -> T): # hypothetical syntax
    (make, make)

fn plain_factory() -> own File:                          # hypothetical syntax
    open("fresh")
```

**Generated behavior.** The input explicitly has a plain function
type. It can be a named function like plain_factory, producing a fresh
owner on each call. Aliasing this function value duplicates no file.
An exclusively capturing one-shot closure would instead have `own fn`
type and would not satisfy this parameter contract.

**Verdict: friction.** The new blanket rule rejects duplicating make
solely because its result mentions T. That is conservative but conflates
output ownership with environment ownership. The same concern applies
to storing or forwarding a reusable producer to multiple consumers.

Move-only storage does not itself prove calls consume the callback;
the design should keep copying and call cardinality separate. This
probe does not reopen the chosen one-shot rule for exclusive captures.
[Rust's function-pointer Copy rule](https://doc.rust-lang.org/std/marker/trait.Copy.html)
provides the comparison; hd need not add a Copy trait.

### Y7. Borrowed Defer Preserves The Owner And Extends The Loan

Reference shape: scopeguard can hold a shared reference until cleanup.

```rust
let guard = scopeguard::guard(&file, |file| report_size(file));
inspect(&file);              // allowed
rewrite(&mut file);          // rejects while guard will still use its borrow
```

```text
fn shared_cleanup() -> void:
    file := open("input")
    let view: ^File = file                               # hypothetical syntax
    defer: report_size(view)
    inspect(file)

fn conflicting_cleanup() -> void:
    file := open("input")
    let view: ^File = file                               # hypothetical syntax
    defer: report_size(view)
    rewrite(file)
```

Assume report_size and inspect take shared borrows with empty rows;
rewrite requires `^mut File`. No suspension or failure is needed.

**Generated behavior.** Both guards capture the shared view. Dropping
the guard uses that loan, so its lifetime extends to cleanup. The first
function permits inspect. The second rejects rewrite; the cleanup loan
remains live even if view has no later explicit use.

**Verdict: works.** The workaround fixes owner capture, not every
borrow conflict. The design's append example is valid if append uses a
shared receiver, as external I/O can. It would reject if append required
an exclusive receiver; the example should make that API assumption clear.

[scopeguard](https://docs.rs/scopeguard/latest/scopeguard/) illustrates the
borrowed guard shape. hd's empty-row restriction remains a chosen limit.

### Y8. A Failed Match Guard Must Not Dispose Of Its Candidate

Reference shape: Rust inspects a payload during the guard, then moves
it only into the selected arm.

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

fn consuming_guard(input: own Option[own File]) -> void:  # hypothetical syntax
    match input:
        .Some(file) if consume_and_test(file) => pass
        .Some(file) => retry(file)
        .None => pass
```

Assume ready takes `^File`; archive and retry consume `own File`.
consume_and_test also consumes a file and can return false. Option
owns its payload structurally and has no custom DropOwn implementation.

**Generated behavior required.** The first function should inspect
without consuming during the guard. If false, the next arm still owns
the candidate. If true, the selected body receives ownership. The second
function must reject consuming file before the guard commits.

**Verdict: friction; missing pattern-commit detail.** Rule 4.2.4 says
a complete pattern consumes its owned subject. Existing guard rules
evaluate bindings before the boolean and permit fallthrough. Naively
moving and dropping the first candidate loses the resource on failure.

Ordinary move-flow checking can safely reject both functions, so this
is not proof that the design accepts an unsafe program. It is an
unspecified acceptance and lowering boundary in a common enum workflow.
The question also affects nested owned patterns and guard cancellation.

The [Rust Reference](https://doc.rust-lang.org/reference/expressions/match-expr.html#match-guards)
specifies shared access during guards, followed by moves only after
success. Its policy is a concrete completion of the chosen Rust model.

### Y9. Atomic Replacement Is More Than An Ordinary Assignment

Reference shape: mem::replace returns the previous owner without dropping
it and leaves the destination initialized.

```rust
fn replace<T>(dst: &mut T, next: T) -> T {
    std::mem::replace(dst, next)
}
```

```text
fn naive_replace[T < own Any](dst: ^mut T, next: T) -> T: # hypothetical syntax
    old := dst
    dst = next
    old

fn replace_slot[T < own Any](slot: ^mut List[T], next: T) -> Option[T]: # hypothetical syntax
    old := slot.pop()
    slot.push(next)
    old
```

**Generated behavior.** The naive function cannot move T out through
dst. Reading dst instead moves its exclusive borrow or yields another
borrow, neither a returned owner. Assigning dst also targets the
non-reassignable parameter binding; it is not a specified dereference
store. Normal assignment over an owner would drop the previous value.

replace_slot uses the already-permitted container removal operation.
Assume a slot is represented by a one-element owned List and pop/push
have the proposed ownership-aware signatures. The old optional result
normalizes after substitution. The destination remains available after
the function returns, at the cost of a container representation.

**Verdict: friction; an explicit expressiveness limit.** A general
Rust-style replace is not established as an ordinary hd library function.
No arbitrary uninitialized field, partial move, or unsafe escape is
needed to retain the conservative policy. Built-in List.pop already
supplies a working removal route.

The [Rust replace contract](https://doc.rust-lang.org/std/mem/fn.replace.html)
explains why assigning and returning an old borrow is insufficient.
If hd later wants the general operation, its implementation boundary
must be stated rather than hidden behind a method named take.

## Comparison With Other Languages

| Mechanism | Primary source | Result for hd |
| --- | --- | --- |
| Receiver reservation and activation | [Rust compiler guide](https://rustc-dev-guide.rust-lang.org/borrow-check/two-phase-borrows.html) | Current rules support both nested reads and rejection of retained conflicting loans |
| Copying references and function pointers | [Rust Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html) | Ownership of a referent or future result differs from ownership of the current value |
| Guarded consuming patterns | [Rust Reference](https://doc.rust-lang.org/reference/expressions/match-expr.html#match-guards) | Shared guard access with delayed move is an established policy |
| Replacing a borrowed destination | [Rust replace](https://doc.rust-lang.org/std/mem/fn.replace.html) | Returning an old owner needs a removal/replacement operation, beyond ordinary assignment |
| Borrowed cleanup guard | [scopeguard](https://docs.rs/scopeguard/latest/scopeguard/) | Explicit capture preserves the owner but retains a lifetime obligation |

These comparisons supply test shapes, not a claim that copying Rust's
surface transfers its soundness proof to hd. Existing GC aliases still
restrict hd's exclusivity and optimization claims as previously documented.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | OWN-S17 | Syntactic mention of T overclassifies known duplicable modes | Written-contract conflict and precision loss | Y5, Y6 |
| 2 | OWN-S18 | Consuming patterns lack an explicit guard-commit policy | Acceptance and lowering gap; conservative rejection is safe | Y8 |
| 3 | OWN-S19 | General replacement has no ordinary-library derivation | Expressiveness and implementation boundary; optional API | Y9 |

### OWN-S17. Universal Checking Needs A Mode Predicate, Not A Text Search

**Effect.** Y4 correctly rejects because its wrapper can actually own
T. Y5's shared reference cannot own T, and Y6's plain factory does not
contain its future result. The new universal policy remains appropriate;
its blanket quantifier loses these distinctions.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should universal checking conservatively move values whose mode can be owned, while preserving known shared-borrow and plain-function modes? | One general language semantic clarification; no new syntax or intrinsic | Retains Y4's rejection and admits the safe Y5/Y6 copies |
| Should ownership-enabled generics intentionally restrict even shared borrows and plain factories, with explicit precedence over the ordinary rules? | One generic semantic exception, language tier | Safe but less precise; concrete and generic helpers differ |

The first uses existing mode distinctions and the structural classifier.
It is cheaper conceptually than adding a Copy-like trait or specialized
syntax. Neither candidate changes definition-time checking or restores
per-instantiation body validity. No ordinary library wrapper can repair
inconsistent rules governing duplication inside that wrapper.

### OWN-S18. Specify The Point Where A Pattern Takes Ownership

**Effect.** Y8 needs an owner available on false-guard fallthrough and
available by value in the selected arm. Eagerly consuming into a
temporary arm binding makes those requirements conflict unless moves
are delayed or the entire construct is rejected.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should owned guard bindings provide shared access until success, then commit their moves when entering the selected body? | One pattern semantic rule, language tier; no new syntax or intrinsic | Admits route and rejects consuming_guard; fits the chosen Rust model |
| Should consuming patterns with guards initially reject, with an outer borrowed inspection before consuming the subject? | One semantic restriction, language tier; ordinary caller restructuring | Simpler initial lowering with a visible expressiveness cost |

This is unrelated to permitting partial moves. A complete enum payload
still consumes as a whole after selection. Guard-only loans and pending
temporaries need the existing cancellation/drop order when a guard suspends.

### OWN-S19. Keep The Removal Privilege Explicit

**Effect.** Y9 shows why an arbitrary library method cannot simply
remove an owner from an exclusive borrow. The existing built-in container
operations can provide that privilege; plain assignment cannot.

| Candidate question | Cost and tier | Consequence |
| --- | --- | --- |
| Should the initial design retain built-in removal only and document the one-element container workaround for replaceable slots? | Ordinary stdlib helper/representation using existing intrinsics; no new language mechanism | Cheapest; accepts the representation cost and leaves a general replace API absent |
| If a general replace API is wanted, should one compiler-known operation exchange a borrowed destination with an owner without an intermediate uninitialized state? | One added compiler intrinsic, language tier; wrapper API in stdlib | Preserves no-partial-move and no-move-through-borrow rules at caller level |

This is lower priority than OWN-S17 and OWN-S18. The record need not
add general replace to be coherent. The question prevents a future
stdlib plan from assuming a capability ordinary hd does not yet have.
No new keyword or generic partial-move permission is proposed.

## Evaluation By Dimension

| Dimension | Change since review 4 | Assessment |
| --- | --- | --- |
| Guarantee clarity | Reservation, classifier, generic diagnostic timing, and defer capture are explicit | Stronger; generic copying now needs consistent precedence across modes |
| Soundness | Receiver invalidation and classifier omissions have answers | No new accepted ownership escape demonstrated; guard commitment and activation deserve focused lowering checks |
| Precision | Structural cutoff preserves shared views and phantom parameters | Blanket generic move-only wording overrejects shared borrows and plain factories |
| Practical expressiveness | Borrowed cleanup and owned generic wrappers have clear workflows | Guarded enums need a completion; generic replacement has a container workaround; declared iterator work remains open |
| Composability | Earlier local fixes now share general reservation and classification rules | Better; generic copying must use the same mode distinctions; pattern selection must compose with moves |
| Modular contracts | Definitions are checked once for all admitted modes | A substantial improvement; future instantiations should not change validity; known duplicable modes need stable contracts |
| Agent writability and human readability | Capture diagnostics and the borrowed-defer idiom are documented | Better; blanket T wording invites incorrect rejection of safe generic helpers; no writing trial measured |
| Conceptual cost | Four prior questions now have direct answers | Remaining clarification can reuse mode classification and delayed pattern commitment; no new syntax is justified by these cases |
| Compiler feasibility and cost | Reservation and least-fixed-point classification specify concrete tasks | Ready for a narrow checker prototype; mode abstraction, guards, drop glue, and suspension still need implementation evidence |
| Optimization validity and payoff | No new optimization evidence | Ownership of a root does not erase permitted plain aliases; destructor and guard effects constrain reordering; payoff remains unmeasured |
| Specification readiness | Review-4 policies are resolved | Suitable for focused formalization and prototyping; reconcile generic copying and guarded consumption before claiming a complete core; section 6 remains open |

The strongest next evidence would be a small checked matrix covering
Y1/Y2, Y4/Y5/Y6, and Y8, including cancellation and drop counts.
The same type-mode abstraction should decide duplication, closure capture,
and destruction. Merely parsing the proposal cannot validate those claims.

## Earlier Findings

| Review-4 finding | Current status |
| --- | --- |
| OWN-S13: receiver reservation | Resolved by explicit reservation and exclusive activation; Y1/Y2 provide regression obligations |
| OWN-S14: structural base cases and recursion | Resolved by shared-borrow cutoff, manual exclusive holders, unused-argument rule, and least fixed point |
| OWN-S15: generic body-checking policy | Resolved by universal definition-time checking; OWN-S17 concerns its overbroad quantifier, not diagnostic timing |
| OWN-S16: cleanup captures | Resolved by the new explanation, defer-specific move diagnostic, and explicit shared view |

Earlier decisions about partial moves, borrow-aware drop, providers,
globals, normalization, and conditional marker coherence remain resolved.
The limits in section 6 are not newly discovered failures. Previous
editorial notes about section 3.4 and annotation counts also remain
editorial; they do not add runtime findings to this report.

## Parse Log

All nine `text` blocks were passed to `scripts/hd-conformance.sh parse`.
All raw blocks reject proposed syntax at marked lines. All nine
projections parse. These results establish no ownership semantics.

Projection preserves operations, comments, and line breaks. It erases
ownership and borrow marks, lifetime declarations and applications, and
proposed marker impls. The mixed type/lifetime header in Y3 becomes an
ordinary type-parameter header. Derive annotations remain in place.

| Block | Case | Raw | Projected |
| --- | --- | --- | --- |
| 1 | Y1 | Rejects proposed syntax | Parses |
| 2 | Y2 | Rejects proposed syntax | Parses |
| 3 | Y3 | Rejects proposed syntax | Parses |
| 4 | Y4 | Rejects proposed syntax | Parses |
| 5 | Y5 | Rejects proposed syntax | Parses |
| 6 | Y6 | Rejects proposed syntax | Parses |
| 7 | Y7 | Rejects proposed syntax | Parses |
| 8 | Y8 | Rejects proposed syntax | Parses |
| 9 | Y9 | Rejects proposed syntax | Parses |

Raw diagnostics follow verbatim, with temporary paths replaced by FILE.
Every raw status is 101; every projected status is 0, with `1/1 clean`.
No parser rejection of specified surrounding syntax remains.

### Block 1

```console
error: FILE:1:20: syntax-error: expected a type, found '^'
error: FILE:4:27: syntax-error: expected ',' or ')', found 'Holder'
0/1 clean
```

### Block 2

```console
error: FILE:1:15: syntax-error: expected a type, found '^'
error: FILE:4:16: syntax-error: expected a parameter name, found '^'
error: FILE:7:26: syntax-error: expected ',' or ')', found 'File'
0/1 clean
```

### Block 3

```console
error: FILE:2:14: syntax-error: expected a generic parameter name, found '^'
error: FILE:3:12: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 4

```console
error: FILE:5:22: syntax-error: expected ',' or ']', found 'Any'
0/1 clean
```

### Block 5

```console
error: FILE:1:18: syntax-error: expected ',' or ']', found 'Any'
error: FILE:4:22: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 6

```console
error: FILE:1:30: syntax-error: expected ',' or ']', found 'Any'
error: FILE:4:27: syntax-error: expected ':' or a line ending after the function signature, found 'File'
0/1 clean
```

### Block 7

```console
error: FILE:3:15: syntax-error: expected a type, found '^'
error: FILE:9:15: syntax-error: expected a type, found '^'
0/1 clean
```

### Block 8

```console
error: FILE:1:21: syntax-error: expected ',' or ')', found 'Option'
error: FILE:7:31: syntax-error: expected ',' or ')', found 'Option'
0/1 clean
```

### Block 9

```console
error: FILE:1:26: syntax-error: expected ',' or ']', found 'Any'
error: FILE:6:25: syntax-error: expected ',' or ']', found 'Any'
0/1 clean
```

## Repository Validation

Origin was fetched. Validation ran in an isolated worktree at `b3711481e`,
rebased onto `origin/main`, with this report and its README entry copied
in. Concurrent networking implementation edits in the main checkout were
excluded from these checks and left untouched.

| Check | Result |
| --- | --- |
| Raw parser runs | Nine expected rejections of marked proposed syntax |
| Projected parser runs | All nine parse |
| `bash spec/check.sh` | Passed; links and anchors, zero failing dead citations, 6,002 rule IDs, 1,564 diagnostic rows |
| `pnpm run website:build` | Passed; 79 pages, 265 grammar rules, 266 definitions |
| Whitespace checks | Passed for the README diff and new report |

The spec check reported nine citation warnings and seven allowed as
history; none failed. Final validation prose changes no example.
No ownership type-checking or runtime safety validation is claimed.

Only this report and its README entry were written. The design record,
specification, compiler, and fixtures were left untouched. Nothing was
staged, committed, or pushed.
