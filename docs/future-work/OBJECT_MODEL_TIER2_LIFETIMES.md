# Object Model, Tier 2: Opt-In Lifetimes

**Status:** proposal, 2026-10-29; reviews [tier 2](OBJECT_MODEL_TIER2.md),
[element model E](https://github.com/HerringtonDarkholme/hd-lang/issues/494),
and [local references](../../spec/lang/13-values-and-references.md#local-references);
nothing here is accepted behavior.

## Proposal

Ship borrowed results as a separate first slice. It adds no syntax. A result
`&T`, `&mut T`, or optional reference borrows from `self` or the only reference
argument. The call roots that result at the source place until its last use.

```text
impl[T] List[T]:
    fn first(&self) -> (&T)?: panic("compiler-provided")

fn read_first(xs: &mut List[i64]) -> i64:
    match xs.first():
        .Some(found) =>
            xs.push(1)  # error: found keeps xs frozen until its last use
            *found
        .None => 0
```

After that slice, keep today's call-local `&T` and counted `*T`. Add `&{a} T`
only when a reference must be stored. The set stays inside every containing
type, including generic `T`. It also describes closure captures, scope guards,
phantom facts, and live spans. Code that never opts in writes no lifetime.

## Open Questions

1. **Element model E:** may slice 0 reverse #494's ban on returning element
   references? Yes admits `first`, `get`, and `get_mut` under last-use loans;
   no limits slice 0 to field and dynamic downcasts. **Recommendation:** yes,
   conditional on the source freeze and escape checks below.
2. **Elision:** recommend Rust's deterministic rules for slice 0. A result
   borrows from `self`, otherwise from the only reference argument. Two
   reference arguments without `self` are an error until explicit sets exist.
3. **Spelling:** recommend `[{a}]`, `&{a} T`, and `->{a}` only for stored
   references, multiple result sources, and scoped closure captures.
4. **Closure kinds:** recommend shared captures for scoped `fn`, plus `fn mut`
   and `fn once`; no-set tier-1 `fn` keeps mutable captured cells.
5. **Phantom facts:** recommend one compiler-known `std.marker.Phantom[T]`;
   include function-shaped variance-only forms in its first lifetime-set slice.
6. **`ensure`:** recommend a result-aware `AutoClosing` refinement, direct
   function-prologue placement, and execution after ordinary cleanup.
7. **Views:** recommend `Span` and `SpanMut`; keep `ListView` for snapshots and
   ship the read-only span first.
8. **Counted aliases:** recommend dynamic borrow flags only where static
   checking cannot see through a counted alias.
9. **`defer` token:** recommend parsing `defer(...)` as a call while retaining
   `defer:` as the suite statement; `(` versus `:` makes the forms unambiguous.

## Problems It Solves

| Requirement | Today | With this proposal |
| --- | --- | --- |
| Borrowed results | `downcast_ref` returns `*T`; collection and field references cannot escape the call | slice 0 returns a local reference rooted at `self` or one argument, with no new syntax |
| Phantom ownership | an unused type parameter has no drop meaning | `Phantom[T]`, `Phantom[&{a} T]`, and function forms state ownership, borrows, and variance |
| Scope guards | a guard closure cannot capture local `&T` / `&mut T` | the guard carries the closure's set and dies before every source |
| `require` | its condition may mutate captured cells | `fn() ->{} bool` is call-scoped and read-only |
| Library cleanup | `defer(action)` cannot capture locals; `ensure` cannot see a result | scoped guards implement `defer`; a result-aware guard receives the saved result |
| Live list views | `ListView[T]` owns a copy-on-write snapshot | `Span[{a}, T]` borrows live storage; `to_list()` still makes a snapshot |
| Generic `T` | `debug(&x)` fails, while passing a declared local ref onward can slip through | scopedness tunnels through `T`; persistent APIs write `T < global` |

## How It Works

- Slice 0 adopts Rust's elision shape. `&self` chooses `self` even when other
  reference parameters exist. Without `self`, exactly one reference parameter
  is required. Two are `ambiguous-borrow-source` until explicit sets ship.
- At a call, a borrowed result is a local reference rooted at its source place.
  A shared result freezes that place; a mutable result is exclusive. The loan
  ends at the result's last use, not at the end of the lexical scope.
- A local or value source needs no runtime state. A source reached through
  counted `*T` is retained and needs a shared/exclusive borrow flag while the
  result lives. Existing collection pins cover length-changing element access;
  field and dynamic downcasts need the same check generalized to their owner.
- `&T` / `&mut T` remain unnameable call lifetimes. `*T` / `*mut T` remain
  counted and storable. `&{a} T` / `&{a} mut T` are scoped and storable only
  inside types that carry `{a}`.
- When a call-local reference is inferred as generic `T`, the checker reifies
  its actual source as an anonymous scoped lifetime in that instantiation. The
  result is no longer an ordinary call-local `&T`, and never becomes `*T`.
- An explicitly lifetime-bearing result names every possible source. Elision
  handles only the deterministic slice-0 cases above.
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

These are bottom-up implementation budgets, not measurements. Each slice is
independently gated and stops if its budget is wrong.

| Slice | Estimated change | Checker and loan work | Runtime and footprint |
| --- | ---: | --- | --- |
| 0. Borrowed results | 550–800 Rust lines; 25–40 fixture cases | elision, source-place identity, last-use shared/exclusive loans | zero for local/value roots; counted roots retain and borrow-flag their owner |
| 1. Sets in interfaces | 500–700 Rust lines | parse, format, resolve, cache, and compare sets | erased before codegen; web-compiler gzip measured before landing |
| 2. Generic tunnelling and `Phantom` | 650–950 Rust lines | variance, `global`, hidden lifetimes in `T`, drop facts | zero-sized phantom; no runtime path |
| 3. General loans | 800–1,200 Rust lines | loans × control-flow points in opt-in bodies; worst case quadratic | no local-root runtime cost |
| 4. Scoped closures | 600–900 Rust lines | capture sets and `fn` / `fn mut` / `fn once` subsumption | existing closure records |
| 5. Guards and cleanup | 400–650 Rust/hd lines | auto-close registration and result availability | existing cleanup frames; no host ABI change |
| 6. `Span` and `SpanMut` | 500–800 Rust/hd lines | source loans and disjoint split proof | two words per span; checks only at construction/split |
| 7. Stored-set counted aliases | 350–600 Rust/runtime lines | generalize slice-0 flags to guards, closures, and spans | retain plus shared/exclusive flag only where used |

Ordinary tier-1 code adds no annotation and skips loan dataflow. Existing
`&T`, `*T`, `fn`, `ListView`, `defer:`, `using`, and `using!` keep their
meaning. Every slice measures compile time, optimized Wasm, and web-compiler
gzip against the 2 MB cap; this design has no defensible byte estimate before
a compiler prototype.

## Recommendation

Approve borrowed results as slice 0 if the owner accepts the narrow reversal of
element model E. Then adopt lifetime sets as tier 2's one opt-in mechanism.
Let scopedness tunnel through generic types, with `T < global` only at
persistent boundaries. Build closures, guards, cleanup, and spans on those
sets. Ordinary code keeps no lifetime annotations.

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
# Slice 0 uses existing syntax; only the return rule is proposed.
impl dyn Inspectable:
    fn downcast_ref[T < Inspectable](&self) -> (&T)?: panic("compiler-provided")

impl[T] List[T]:
    fn first(&self) -> (&T)?: panic("compiler-provided")
    fn get(&self, index: usize) -> (&T)?: panic("compiler-provided")
    fn get_mut(&mut self, index: usize) -> (&mut T)?: panic("compiler-provided")

impl[K, V] Map[K, V]:
    fn get(&self, key: &K) -> (&V)?: panic("compiler-provided")

struct User:
    name: string

impl User:
    fn name(&self) -> &string:
        &self.name
```

Every optional result is rooted at its receiver. `downcast_ref` can therefore
return `&T` from `&dyn`, rather than the counted `*T` workaround added by #988.
List and map results keep their collection frozen; `get_mut` holds exclusive
access. The field getter keeps `User` frozen through its result's last use.

```text
fn inspect(value: &dyn Inspectable, xs: &mut List[i64], users: &Map[string, User]) -> void:
    match value.downcast_ref::[User]():
        .Some(user) => audit(user.name)
        .None => pass
    match xs.first():
        .Some(first) => audit(*first)
        .None => pass
    match xs.get_mut(0):
        .Some(slot) => *slot = *slot + 1
        .None => pass
    key := "owner"
    match users.get(&key):
        .Some(user) => audit(user.name())
        .None => pass
```

These are real uses, not only signature forms. Their syntax exists today; the
borrowed-return semantics are proposed. The list and map methods are conditional
on open question 1 because element model E currently forbids these results.

```text
# Slice-0 elision rules.
fn one[T](value: &T) -> &T: value

struct Pair[T]:
    value: T

impl[T] Pair[T]:
    fn own_or(&self, fallback: &T) -> &T:
        &self.value  # result is tied to self, not fallback

fn choose[T](left: &T, right: &T, first: bool) -> &T:  # error: ambiguous-borrow-source
    if first: left else: right
```

This matches Rust's deterministic elision: one reference input supplies the
result, while a receiver supplies it even beside other inputs. Two reference
inputs without `self` are an error. Explicit lifetime sets later express the
`choose` relationship; slice 0 does not guess or silently change it.

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

| Option | Meets all six | Ordinary burden | Costliest change | Why not recommended |
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

### Borrowed-result rules

Slice 0 changes results, not type syntax. An elided local-reference result
borrows from `self`, or from the only local-reference parameter when there is
no receiver. A receiver wins beside other reference parameters. Otherwise the
signature is `ambiguous-borrow-source`.

The call records the source place in the result. A shared result freezes that
place; a mutable result exclusively borrows it. Both loans end at last use.
Returning, storing, capturing, or passing the result beyond its source remains
an escape error. Nested optional and tuple results carry the same source.

For a provably local/value source, the checker proves the loan and codegen adds
nothing. Through counted `*T`, the allocation remains retained until last use.
An unseen alias requires a shared/exclusive runtime borrow flag; mutation checks
it. Collection element results reuse #494's pin for length changes, while field
and dynamic downcasts need the owner-level form.

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

0. Borrowed results with existing syntax: elision, source-place results,
   local/value loans, counted-owner retention and flags, diagnostics, and the
   six worked API shapes. This slice depends only on open question 1.
1. Grammar, formatting, interface representation, and parse fixtures for sets.
2. Header checks, generic tunnelling, `global`, and `Phantom` facts.
3. General loan liveness, shared freezes, exclusive conflicts, and diagnostics.
4. Scoped closure kinds and read-only `require` captures.
5. Scoped `ScopeGuard`, `AutoClosing`, library `defer`, and the approved
   result-aware `ensure` protocol.
6. Read-only `Span`; then `SpanMut` and disjoint splitting if approved.
7. Generalize counted-storage retention/flags from borrowed results to stored
   sets, guards, closures, and spans; add host parity.

Each slice ships its spec rules and conformance fixtures. This design PR changes
no spec, compiler, or std implementation.

## Appendix F: Parse Log

All eleven hd `text` blocks were checked with `scripts/hd-conformance.sh parse`
on 2026-10-29. The proposal example, borrowed-result signatures and uses,
elision rules, generic tunnelling, and result protocol parse: six blocks. The
other five reject at their first marked, unimplemented lifetime,
closure-kind, phantom, or auto-closing form, as expected. Parsing validates
syntax only, not the proposal's semantics.
