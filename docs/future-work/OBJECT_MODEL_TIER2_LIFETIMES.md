# Object Model, Tier 2: Opt-In Lifetimes

**Status:** proposal, 2026-10-29; domain: references beyond one call, scoped
closures, drop checking, cleanup guards, and borrowed views; not accepted.

## Proposal

Keep today's call-local `&T` and counted `*T`. Add `&{a} T` only when a
reference must cross a call boundary. The lifetime stays inside every
containing type, including generic `T`. The same lifetime set describes closure
captures, scope guards, phantom facts, and live spans.

```text
# Proposed lifetime and closure syntax is hypothetical.
fn inspect[{a}, T](items: &{a} List[T]) -> Span[{a}, T]:  # hypothetical syntax
    items.span(0, items.len())

fn record(log: &mut List[string]) -> void:
    using guard = on_exit(fn once() ->{log} void: log.push("closed"))  # hypothetical syntax
```

Code that never opts in writes no lifetime. A scoped shared borrow freezes its
source while live; a scoped mutable borrow is exclusive. Local sources are
checked statically. A borrow through counted storage retains and flags that
storage only when an unseen alias could invalidate it.

## Open Questions

1. **Spelling and elision:** recommend the studied `[{a}]`, `&{a} T`, and
   `->{a}` sets, with explicit sources on results and no Rust-style result
   elision. Repeating one set is cheaper than silently changing a relationship
   when another borrowed input is added.
2. **Closure kinds:** recommend shared captures for scoped `fn`, plus `fn mut`
   and `fn once`; no-set tier-1 `fn` keeps mutable captured cells.
3. **Phantom facts:** recommend one compiler-known `std.marker.Phantom[T]`;
   include function-shaped variance-only forms in the first slice.
4. **`ensure`:** recommend a result-aware `AutoClosing` refinement, direct
   function-prologue placement, and execution after ordinary cleanup.
5. **Views:** recommend `Span` and `SpanMut`; keep `ListView` for snapshots and
   ship the read-only span first.
6. **Counted aliases:** recommend dynamic borrow flags only where static
   checking cannot see through a counted alias.
7. **`defer` token:** recommend parsing `defer(...)` as a call while retaining
   `defer:` as the suite statement; `(` versus `:` makes the forms unambiguous.

## Problems It Solves

| Requirement | Today | With this proposal |
| --- | --- | --- |
| Phantom ownership | an unused type parameter has no drop meaning | `Phantom[T]`, `Phantom[&{a} T]`, and function forms state ownership, borrows, and variance |
| Scope guards | a guard closure cannot capture local `&T` / `&mut T` | the guard carries the closure's set and dies before every source |
| `require` | its condition may mutate captured cells | `fn() ->{} bool` is call-scoped and read-only |
| Library cleanup | `defer(action)` cannot capture locals; `ensure` cannot see a result | scoped guards implement `defer`; a result-aware guard receives the saved result |
| Live list views | `ListView[T]` owns a copy-on-write snapshot | `Span[{a}, T]` borrows live storage; `to_list()` still makes a snapshot |
| Generic `T` | `debug(&x)` fails, while passing a declared local ref onward can slip through | scopedness tunnels through `T`; persistent APIs write `T < global` |

## How It Works

- `&T` / `&mut T` remain unnameable call lifetimes. `*T` / `*mut T` remain
  counted and storable. `&{a} T` / `&{a} mut T` are scoped and storable only
  inside types that carry `{a}`.
- When a call-local reference is inferred as generic `T`, the checker reifies
  its actual source as an anonymous scoped lifetime in that instantiation. The
  result is no longer an ordinary call-local `&T`, and never becomes `*T`.
- A lifetime-bearing result always names its source. The checker never guesses
  between multiple inputs.
- Generic parameters accept scoped types by default. A top-level binding,
  provider, detached task, channel, erased value, or other persistent boundary
  requires `T < global`.
- A scoped closure's set names what it borrows. `{}` on a parameter means the
  call's lifetime, not an empty set. Plain scoped `fn` borrows shared; `fn mut`
  borrows exclusively; `fn once` may mutate or move captures.
- `Phantom[T]` is zero-sized but participates in drop checking and variance.
  Its constructor cannot manufacture an ownership or borrow fact.
- `ScopeGuard[{a}]`, `Defer[{a}]`, and `Ensure[{a}, R]` are noncopyable library
  values. Their compiler-known pieces are lifetime checking and auto-closing
  registration, not their names.
- `Span[{a}, T]` is pointer plus length. `SpanMut` is noncopyable; splitting
  consumes it and uses one checked primitive that constructs disjoint spans.
- Loan liveness is last-use based. Bodies with no lifetime-bearing values skip
  the loan dataflow.

## Cost

| Component | Cost |
| --- | --- |
| Source | no annotation for tier-1 code; explicit sets on stored/returned borrows |
| Checker | interface set/variance checks plus loans × control-flow points in opt-in bodies; worst case quadratic |
| Runtime | zero for local/value roots; retain plus borrow flag only through counted storage |
| Representation | two words for a span; ordinary closure/guard records; zero bytes for `Phantom` |
| New mechanisms | lifetime/closure syntax; exclusivity and result-cleanup rules; compiler-known phantom and span split |
| Compatibility | existing `&T`, `*T`, `fn`, `ListView`, `defer:`, `using`, and `using!` keep their meaning |

## Recommendation

Adopt lifetime sets as tier 2's one opt-in mechanism. Let scopedness tunnel
through generic types, with `T < global` only at persistent boundaries. Build
read-only scoped closures on the same sets, then express guards, `defer`,
`ensure`, and spans as library types. This meets all five requirements without
body inspection or annotations in ordinary code.

## Appendix A: Stress Test

### Phantom ownership and borrowing

```text
# Proposed Phantom and lifetime syntax is hypothetical.
struct NativeOwner[T] ~Copy:  # hypothetical syntax
    handle: usize
    owns: Phantom[T]

struct NativeView[{a}, T]:  # hypothetical syntax
    handle: usize
    borrows: Phantom[&{a} T]  # hypothetical syntax

struct NativeViewMut[{a}, T] ~Copy:  # hypothetical syntax
    handle: usize
    borrows: Phantom[&{a} mut T]  # hypothetical syntax
```

The owner is dropped as though it stored `T`. The views cannot outlive `{a}`;
the mutable view holds exclusivity. A raw-handle constructor that lacks the
real value/reference is unavailable to ordinary code. Otherwise the marker
would invent a fact instead of recording one.

### Guards over local references

```text
# Proposed capture-set syntax is hypothetical.
fn shared(log: &List[string]) -> void:
    action := fn once() ->{log} void: audit(log.len())  # hypothetical syntax
    using guard = on_exit(action)

fn exclusive(log: &mut List[string]) -> void:
    action := fn once() ->{log} void: log.push("closed")  # hypothetical syntax
    using guard = on_exit(action)
    log.push("work")  # error: guard holds the exclusive borrow
```

The shared case is storable until scope exit. The mutable anti-case is
rejected: code must access a protected value through the guard or use lexical
`defer:` for late access.

### A read-only `require`

```text
# Proposed call-lifetime capture set is hypothetical.
pub fn require(message: string = "requirement failed", condition: fn() ->{} bool) -> void  # hypothetical syntax

fn debit(account: &mut Account, amount: i64) -> void:
    require("enough balance"):
        account.balance >= amount
    account.balance = account.balance - amount
```

On a parameter, `{}` means any capture that lives through this call; it is not
an empty set. The condition may read the local reference. A read-only capture
weakens `&mut` to `&`, `*mut` to `*`, and mutable cells to shared access for
the closure body. Assigning through any capture-rooted path, or calling a
writable receiver through one, is rejected. Release removal therefore cannot
erase program mutation.

### Library `defer` and result-aware `ensure`

```text
# Proposed auto-closing, fn once, and capture-set syntax is hypothetical.
@auto_closing
pub fn defer[{a}, $R](action: fn once() ->{a} void $ R) -> Defer[{a}, $R]  # hypothetical syntax

@auto_closing
pub fn ensure[{a}, R, $C](check: fn once(&R) ->{a} void $ C) -> Ensure[{a}, R, $C]  # hypothetical syntax

fn clamp(value: i64, low: i64, high: i64) -> i64:
    ensure(fn once(result: &i64) ->{} void:  # hypothetical syntax
        require("result is in range"): low <= *result && *result <= high
    )
    defer(fn once() ->{} void: audit("clamped"))  # hypothetical syntax
    value.max(low).min(high)
```

The `ensure` call is in the function prologue. The compiler infers `R` from
the function result and registers it first. Normal completion saves the result,
runs later cleanup in LIFO order, then gives `ensure` a shared result reference.
Panic and cancellation have no normal result and skip result-aware entries.

### A live list view

```text
# Proposed lifetime-bearing receiver and Span syntax is hypothetical.
impl[T] List[T]:
    fn span[{a}](&{a} self, start: usize, end: usize) -> Span[{a}, T]  # hypothetical syntax
    fn span_mut[{a}](&{a} mut self, start: usize, end: usize) -> SpanMut[{a}, T]  # hypothetical syntax

struct SpanMut[{a}, T] ~Copy:  # hypothetical syntax
    data: Phantom[&{a} mut T]  # hypothetical syntax

impl[{a}, T] SpanMut[{a}, T]:  # hypothetical syntax
    fn split_at(self, index: usize) -> (SpanMut[{a}, T], SpanMut[{a}, T])  # hypothetical syntax

fn sum_middle(items: &List[i64]) -> i64:
    middle := items.span(1, items.len() - 1)
    let total = 0
    for item in middle:
        total += *item
    total
```

The span copies neither elements nor the list value. It cannot outlive the
list. A shared span freezes its storage while live; a mutable span is the sole
writer. `SpanMut` is noncopyable, and `split_at` consumes the original before
returning disjoint halves. Length changes conflict with either. `to_list()`
remains the owned snapshot escape hatch.

### Direct borrowed results and elision

```text
# Proposed lifetime-bearing result syntax is hypothetical.
impl[T] List[T]:
    fn get_ref[{a}](&{a} self, index: usize) -> (&{a} T)?  # hypothetical syntax

fn choose[{a, b}, T](left: &{a} T, right: &{b} T, first: bool) -> &{a, b} T:  # hypothetical syntax
    if first: left else: right
```

The optional result carries the receiver's lifetime directly. The two-source
result carries both possible sources. Unlike Rust, the proposal does not elide
the output lifetime when there is one input or `self`. Adding a second borrowed
input must not silently change an interface relationship. The trade-off is one
repeated set in the common getter signature.

### Generic tunnelling

```text
# Proposed global bound; comments show conceptual inferred types.
fn debug[T](value: T) -> void: pass
fn id[T](value: T) -> T: value
fn install[T < global](value: T) -> void: pass

fn demo(point: Point) -> void:
    debug(&point)       # valid: T = &{point} Point
    same := id(&point)  # same carries {point}
    install(&point)     # error: scoped T is not global
```

Returning or wrapping `T` stays sound because its lifetime remains inside the
resulting type. A body-based “allow only if this callee currently does not
keep it” rule is rejected: it breaks separate checking and is source-fragile.
At generic inference only, `&point` is contextually reified as
`&{point} Point`; outside that context, plain `&T` keeps its call-only meaning.

| Adversarial case | Result | Reason |
| --- | --- | --- |
| return a span past its list | rejected | result names a dead source |
| store a guard globally | rejected | storage requires `global` |
| copy a guard | rejected | guard and `fn once` are noncopyable |
| return `id(&local)` beyond `local` | rejected at the boundary | lifetime tunnels through `T` |
| phantom borrow without a source | constructor unavailable/rejected | marker cannot manufacture a loan |
| split mutable span into overlapping halves | impossible in safe code | split primitive proves disjoint ranges |
| `ensure` after early `return` | runs with saved result | function cleanup owns every normal exit |
| panic or cancellation | no `ensure` result check | no normal result exists |

## Appendix B: Prior Art

### Rust

```rust
struct View<'a, T> { ptr: *const T, marker: PhantomData<&'a T> }
struct Owner<T> { ptr: *mut T, marker: PhantomData<T> }

fn inspect<T: core::fmt::Debug>(x: T) { println!("{x:?}"); }
let n = 1;
inspect(&n); // T = &'local i32
```

Rust makes every reference lifetime-bearing and uses elision for deterministic
signatures. Hd takes explicit relationships, generic tunnelling, and phantom
forms, but keeps call-local `&T` annotation-free. A wrong phantom form can make
variance or drop checking unsound, so construction is trusted. Sources:
[elision](https://doc.rust-lang.org/reference/lifetime-elision.html),
[bounds](https://doc.rust-lang.org/reference/trait-bounds.html),
[variance](https://doc.rust-lang.org/reference/subtyping.html), and
[`PhantomData`](https://doc.rust-lang.org/reference/special-types-and-traits.html).

### Swift

```swift
func inspect<T: ~Escapable>(_ value: borrowing T) { }
func callNow(_ work: () -> Void) { work() }
func save(_ work: @escaping () -> Void) { stored = work }
let view: Span<Int> = values.span
```

Swift keeps ordinary types escapable and makes possibly-local generic values
opt in. Hd instead recommends scoped-by-default `T`, because capture tunnelling
is already the tier-2 premise. Swift is easier locally; hd avoids rewriting
most generic std signatures. Sources: [SE-0103](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0103-make-noescape-default.md),
[SE-0446](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md),
[SE-0447](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0447-span-access-shared-contiguous-storage.md), and
[SE-0519](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0519-ref-mutableref-types.md).

### Mojo

```mojo
def to_span[origin: Origin](ref[origin] xs: List[Byte]) -> Span[Byte, origin]:
    return Span(xs)

def either[oa: Origin, ob: Origin](ref[oa] a: T, ref[ob] b: T)
    -> ref[origin_of(a, b)] T:
    return a
```

Mojo mostly infers origins and can union several. Hd takes source relationships
inside types, but writes result sources and avoids wildcard origins: a safe
union can retain values too long. Source: [Mojo lifetimes, origins, and
references](https://docs.modular.com/mojo/manual/values/lifetimes/).

### Hylo / Val

```hylo
fun increment(_ x: inout Int) {
  &x += 1
}

&increment(&count)
```

Hylo confines exclusive access to an argument or projection. That is close to
today's tier-1 `&mut T` and keeps ordinary code simple. It cannot alone express
a guard or span stored across statements. Sources: [Hylo functions and
methods](https://hylo-lang.org/docs/user/language-tour/functions-and-methods/)
and [The Val Object Model](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2022/p2676r0.pdf).

### Vale

```vale
pure func printShipNames(xs &List<Ship>) {
  foreach ship in xs { println(ship.name); }
}
```

Vale's opt-in immutable region freezes pre-existing data for a pure call. Hd
takes progressive disclosure, but rejects region granularity: it freezes
unrelated fields and cannot model independently lived guards. Sources: [region
overview](https://verdagon.dev/blog/zero-cost-borrowing-regions-overview) and
[immutable region borrowing](https://verdagon.dev/blog/zero-cost-borrowing-regions-part-1-immutable-borrowing).

## Appendix C: Alternatives

| Option | Meets all five | Ordinary burden | Costliest change | Why not recommended |
| --- | --- | --- | --- | --- |
| **Lifetime sets** | yes | none until a borrow crosses a call | syntax | recommended despite the largest opt-in checker |
| Swift-style `~Escapable` | with a separate source relation | low, but generic APIs opt in | syntax | broad bounds plus another lifetime mechanism |
| Mojo origins everywhere | yes | mostly inferred | syntax | wildcard/union complexity and over-retention |
| Region borrowing | coarsely | low | semantic rule | freezes unrelated values and loses disjointness |
| Call-scoped projections only | no | none | core library | cannot store guards, borrowed results, or spans |

The radical simplification is to keep only call-scoped projections and
`defer:`. It adds no lifetime syntax, but directly fails three requirements.
It is the fallback if tier 2 is rejected, not an implementation of tier 2.

## Appendix D: Today In Hd

- [`&T` / `&mut T`](../../spec/lang/13-values-and-references.md#local-references)
  are local and cannot be fields, results, captures, or type arguments.
- [`*T` / `*mut T`](../../spec/lang/13-values-and-references.md#reference-types)
  are counted and storable.
- [Closures](../../spec/lang/07-functions.md#captures) may mutate captured cells
  and cannot capture local references.
- [Scope guards](SCOPE_GUARDS.md) are built, but local-reference captures and
  the accepted `AutoClosing` convention wait for tier 2.
- [`require`](DESIGN_BY_CONTRACT.md) is an ordinary condition closure; its
  accepted record explicitly defers read-only local captures and result-aware
  `ensure`.
- [`ListView[T]`](../../spec/std/collections.md#views) owns a copy-on-write
  snapshot. It is not a live borrow.

## Appendix E: Detailed Rules And Implementation

### Lifetime and generic rules

`[{a}]` declares a lifetime parameter. `&{a} T` carries it. A stored type must
carry every lifetime in its fields. Shared references are covariant; mutable
references are invariant in their referent. A result names every possible
source. `global` outlives every scope and is required at persistent boundaries.

A plain local `&T` passed to generic `T` acquires an anonymous call lifetime;
it never converts to counted `*T`. The lifetime tunnels through `T`, tuples,
collections, and results. This gives `debug(&x)` and `id(&x)` one consistent
rule without inspecting bodies.

### Phantom rules

| Form | Fact | Variance |
| --- | --- | --- |
| `Phantom[T]` | logically owns `T` for drop checking | as an owned field |
| `Phantom[&{a} T]` | shared borrow for `{a}` | covariant in `{a}` and `T` |
| `Phantom[&{a} mut T]` | exclusive borrow for `{a}` | invariant in `T` |
| `Phantom[fn(T) -> U]` | variance only; owns neither | argument contravariant, result covariant |

### Closure and cleanup rules

A scoped closure is checked against an expected capture set. `{}` on a closure
parameter denotes the call lifetime, not an empty set. Plain scoped `fn`
captures by shared borrow; `fn mut` adds exclusive mutation; `fn once` adds
moves and may also mutate. The capabilities subsume in that order: `fn`
converts to `fn mut`, which converts to `fn once`. Existing no-set `fn` remains
a global tier-1 closure and may share mutable cells.

`ScopeGuard[{a}]` carries its callback set and is noncopyable. `defer(action)`
returns a value implementing `AutoClosing` plus `Disposable[$C]`.
`ensure[R, $C](check)` returns one implementing `AutoClosing` plus the proposed
`ResultClosing[R, $C]`:

```text
# Proposed result-aware cleanup protocol is hypothetical.
pub trait ResultClosing[R, $C = $()] :  # hypothetical syntax
    fn close_with_result(&mut self, result: &R) -> void $ C  # hypothetical syntax
```

The accepted marker rule is generalized: an `AutoClosing` result must refine
exactly one of `Disposable[$C]`, `AsyncDisposable[$C]`, or
`ResultClosing[R, $C]`. The decorator verifies that pair and records the
matching cleanup entry. `ensure` is not compiler-known, and its callback row is
preserved as `$C`. A result-aware call is legal only in a function prologue,
registers on that function's cleanup frame, runs after later ordinary cleanup
on normal completion, and receives `&R`.

### Span rules

`Span` is a shared pointer/length borrow; `SpanMut` is exclusive and
noncopyable. Safe constructors derive both from a real list borrow. Static
loans protect locals. A counted list buffer stays retained and flagged until
its span dies. Mutable splitting consumes the original span, checks the range
once, and returns disjoint halves.

### Compiler and runtime checklist

- Grammar/formatting: binders, reference sets, arrow capture sets, `fn mut`,
  `fn once`, and `global`; formatter round trips each.
- Typing: name resolution, set inclusion, result coverage, variance, generic
  tunnelling, and persistent-boundary errors.
- Ownership: scoped loans participate in last-use liveness, closure capture,
  cleanup order, suspension frames, and inliner legality.
- Diagnostics: name the source, borrow creation, conflicting access, last use,
  and a `*T`/copy escape hatch; no error for unmarked tier-1 code.
- Pipeline: lifetime facts live in checked interfaces and TIR, then erase after
  ownership/codegen selects local versus counted handling.
- Complexity/cache: interface facts are deterministic and cached; only opt-in
  bodies run loan dataflow; interface changes invalidate callers.
- Runtime/host: local loans change no ABI; counted-storage flags need Node and
  wasmtime parity; cancellation destroys guards before their sources.
- Rollback: each slice is additive until std signatures adopt it; removing the
  syntax and unused library types restores tier 1.

### Implementation slices after approval

1. Grammar, formatting, interface representation, and parse fixtures.
2. Header checks, generic tunnelling, `global`, and `Phantom` facts.
3. Loan liveness, shared freezes, exclusive conflicts, and diagnostics.
4. Scoped closure kinds and read-only `require` captures.
5. Scoped `ScopeGuard`, `AutoClosing`, library `defer`, and the approved
   result-aware `ensure` protocol.
6. Read-only `Span`; then `SpanMut` and disjoint splitting if approved.
7. Counted-storage retention/flags and host parity.

Each slice ships its spec rules and conformance fixtures. This design PR changes
no spec, compiler, or std implementation.

## Appendix F: Parse Log

All nine hd `text` blocks were checked with `scripts/hd-conformance.sh parse`
on 2026-10-29. The generic-tunnelling and result-protocol blocks parse. The
other seven reject at their first marked, unimplemented lifetime,
closure-kind, phantom, or auto-closing form, as expected. Parsing validates
syntax only, not the proposal's semantics.
