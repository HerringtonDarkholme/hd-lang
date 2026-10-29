# The Path To Returned Local References

**Status:** study, 2026-10-29; domain: references and lifetimes; nothing
here is accepted behavior. It reviews
[Local References](../../spec/lang/13-values-and-references.md#local-references),
[Element References](../../spec/lang/13-values-and-references.md#element-references),
the parked [tier-2 study](OBJECT_MODEL_TIER2.md), Muse's unreviewed PR #999
(`OBJECT_MODEL_TIER2_LIFETIMES.md`), and the tier-2 requirements in #978
and #995. It changes no spec, compiler or std code.

## Context

hd has two kinds of reference. A counted `*T` refers to a whole object that
`box` made, and may be stored, returned and captured. A local `&T` or
`&mut T` refers to any place, a local, a field or the referent of another
reference, and never leaves the function that holds it. Element
references, `&xs[i]`, are tighter still: under decision E (#494) they live
for one call.

Writes are not exclusive (`ref.write.no-exclusivity`): a `&mut` may alias
any other reference. Values copy, lists and maps are copy-on-write, and
`mut self` on a collection is an owned copy.

The owner's goal, 2026-10-29: ship something like Kotlin's local lifetimes,
Swift 6.4's lifetime dependence, or Rust's lifetimes, broken into stages.
The first stage must let a function return a local reference borrowed from
`self` or an argument. The owner then relaxed the constraints: many stages
are fine, and exclusivity can come later. Later stages may break earlier
code. An early stage may leave a hole if the hole panics rather than
corrupts memory. Stage 1 should be the smallest build. No stopgaps: no
"take `*T`" fix-its and no special case for one intrinsic.

## The Problem

A getter cannot return what it reads, so every reference-shaped API needs a
counted `*T` or a copy:

```text
struct User:
    name: string

impl User:
    fn name(&self) -> &string:  # error today: local-reference-escapes
        &self.name

fn label(e: &dyn Inspectable) -> string:
    match e.downcast_ref::[User]():  # error today: downcast takes *self
        .Some(u) => u.name
        .None => "?"
```

Today's diagnostic on `name` offers a fix-it that rewrites the result to
`*string`. That fix-it cannot work: `ref.box.object` lets no counted
reference refer to a field. It is the stopgap the owner rejected.

The same wall blocks `downcast_ref` on `&dyn` (#996), builder methods
returning `&mut Self`, `List.first`, `List.get` and `Map.get`.

## The End State In Brief

Three models were weighed as end states. Appendix A has the prior art.

| | Kotlin local lifetimes | Swift 6.4 lifetime dependence | Rust lifetimes |
| --- | --- | --- | --- |
| What a lifetime bounds | how long a value may live | how long a value may live, plus exclusive access to its source | how long a borrow lives, plus aliasing XOR mutation |
| Memory safety from | the GC (in hd: counts and pins) | static plus dynamic exclusivity | the borrow checker, with `RefCell` flags at run time |
| Ordinary code | no marks | no marks; inference rules | elision, with explicit `'a` in data types |
| Fit with hd today | keeps aliasable `&mut` | needs exclusivity, which hd has only for length (E) | needs exclusivity everywhere, plus flags at every `*mut` |

**Recommendation.** Swift 6.4's lifetime dependence is the end state:

- A result, and later a stored value, depends on named sources.
- Sources default by Rust's elision rules.
- Source sets are written in types, as Kotlin's `_{this}` and the tier-2
  study's `&{a} T` are.
- Exclusivity comes last and narrow. It generalizes decision E: a live
  borrow freezes the shape of its path (length, enum variant, a reference
  along it), not every write to it.

Against Kotlin: counting alone keeps every heap-crossing reference and
element reference counted, and enum payloads can never become places. It
also misses the owner's accepted rule that a guard's mutable capture is
exclusive (#978 item 2). Against Rust: hd's counted `*mut` aliases defeat a
purely static checker, so Rust's model would still need run-time flags. It
would also turn today's aliasing `&mut` code into errors.

Stage 1 does not depend on this choice. It holds under all three models.

## The Stages

Each stage is sound on its own, using run-time counting where its static
rules stop. Each heads toward the end state. Hypothetical syntax is marked.

| Stage | Adds | Unlocks | Static or run time |
| --- | --- | --- | --- |
| 1. Dependent results | a result may hold local references; its source is elided; origins are checked in the body | getters, `&mut Self` builders, `downcast_ref` on `&dyn`, `(&T)?` results | static escape check; memory safety by counts (cells) |
| 2. Frame-slot results | backend only: source liveness covers the result's uses; inlining of reference getters | the counts and cells stage 1 adds go away | no language change |
| 3. Named sources | `-> &{a, b} T` (hypothetical); dependent function types; per-position precision | `choose(a, b)`, `get_or(&self, fallback)`, closures that return references | static |
| 4. Element results | reverses E item 4 (owner question): element references flow into results and bindings | `List.first`/`get`/`get_mut`, `Map.get`, `r := &mut xs[i]` | pins panic on a length change (exists today) |
| 5. Generic locality | local-holding type arguments for generic functions; `T < global` at persistent boundaries (hypothetical) | `Option` methods on `(&T)?`, `debug(&x)`, `id(&x)` (#987) | static |
| 6. Stored references | lifetime parameters on structs and enums; scoped closures; guards; `Span` | the #978 items: phantom markers, guards, read-only captures, library `defer`/`ensure`, live spans | static, counts at heap crossings |
| 7. Exclusivity | a live borrow freezes its path's shape; static for unaliased roots, borrow flags through `*mut` (panic `borrow-conflict`) | enum payload places (`as_mut`), exclusive guard captures, uncounted element and heap references | static plus flags |
| 8. Uncounted scoped references | the backend drops the counts that stage 7 makes unnecessary | the perf payoff | none |

**What stage 7 rejects.** These earlier programs become errors where the
checker sees the source, and panics through a counted alias:

- a length change of a list or map while a stage-4 result into it is live
  (it panicked `collection-pinned` before);
- replacing a `*mut` field that a live result's path crosses (it was valid
  before, with a stale result);
- under a full Swift law (question 4), also any plain write to the source
  while a `&mut` result is live.

### Stage 2: frame-slot results

Stage 1 is correct but pays a counted cell and a count per call; see Cost.
Stage 2 has three parts:

- The ownership passes count every use of a dependent result as a use of
  its source root.
- The frame-slot rule (§23.38) follows the result through those uses, and
  the borrow pass lends the result where the root's count spans it.
- The inliner learns to copy a callee whose reference parameter is a
  dependent result's source. Today it refuses every callee that takes a
  reference (`hd_opt/src/inline.rs`, `takes_reference`).

### Stage 3: named sources

```text
fn choose(a: &User, b: &User, first: bool) -> &{a, b} User:  # hypothetical syntax
    if first: a else: b

impl[T] Pair[T]:
    fn left_or(&self, fallback: &T) -> &{self, fallback} T:  # hypothetical syntax
        &self.left
```

The set names parameters, as `Entry[{self}, K, V]` does in the #494 log.
Stage 6 reuses the same set in struct fields. A function type takes the
same elision as a declaration, so `fn(&User) -> &string` depends on its
one parameter.

### Stage 4: element results

```text
impl[T] List[T]:
    fn first_mut(&mut self) -> &mut T?:
        if self.len() == 0: .None else: .Some(&mut self[0])

fn bump_first(xs: &mut List[i64]) -> void:
    match xs.first_mut():
        .Some(slot) =>
            *slot += 1
            xs.push(0)  # stage 4: panic collection-pinned; stage 7: conflicting-access
        .None => pass
```

The run time already supports kept element references: each copy holds a
count of the buffer, which is the pin, until its last use (DESIGN.md
§23.20). So stage 4 is checker work, plus the owner's answer to question 2.

## Stage 1 In Full

### Rules

1. **Result types.** A function's or method's result type may hold a local
   reference. That covers `&T` and `&mut T`, tuples of them, and an
   instance of a generic struct or enum whose type argument holds one, such
   as `&T?` or `Result[&T, E]`. A field, element, top-level or capture
   position still may not.
2. **Source positions.** Each `&` or `&mut` in a parameter's type is one
   reference position: in `&self`, in `p: &T`, in `opt: &T?`, and in each
   element of `pair: (&A, &B)`.
3. **Elision.** A result that holds a local reference depends on `self`
   when the receiver is `&self` or `&mut self`. Otherwise it depends on the
   parameter of the one reference position. With no position, it is an
   error: `reference-result-without-source`. With two or more and no
   reference receiver, it is an error: `ambiguous-reference-source`.
4. **The body.** Every value the body returns may depend only on the
   source. A value that depends on a local, on a value parameter's own
   storage, or on another parameter is `local-reference-escapes`. The
   message names the root, and there is no fix-it.
5. **Origins.** The checker follows each local-holding value's origins
   through the body, as "Provenance" below states.
6. **The call.** A call's result depends on what its source argument
   depends on. The result is a local-holding value of the caller. Every
   rule of Local References applies to it, and the caller may return it
   only by rule 4.
7. **Trait methods.** An impl's method has the same source as the trait's,
   since both are elided from the same signature shape. A call through
   `&dyn`, a bound or an impl takes the source from the trait signature.
8. **Element references are unchanged.** An element reference, a `for p in
   &xs` binding included, may not be the source argument of a call whose
   result holds a local reference. Error:
   `element-reference-outlives-call` (decision E).
9. **Generic calls.** A written or inferred type argument of a generic
   function or method never holds a local reference. This includes one
   inferred from a declared `&T` value, which today slips through (#987).
   Error: `local-reference-escapes`. A generic type may still hold one:
   rule 1 builds `&T?` with `.Some`, `match` reads it, and `?` and the
   implicit wrap work on it.
10. **Stores.** A local-holding value stored through a reference must land
    in a place of this body. A store into a place the caller owns, through
    `out: &mut &T?`, is `local-reference-escapes` until stage 3 can name
    its source.
11. **Downcast.** `dyn Inspectable` and `dyn Error` gain
    `downcast_ref[T](&self) -> &T?` and `downcast_mut[T](&mut self) -> &mut T?`,
    with the existing `*self` forms kept. They are ordinary declarations
    elided by rule 3; only their bodies are intrinsic.

```text
struct User:
    name: string
    tags: List[string]

impl User:
    fn name(&self) -> &string:           # depends on self
        &self.name

    fn tags_mut(&mut self) -> &mut List[string]:
        &mut self.tags

    fn named(&mut self, name: string) -> &mut User:
        self.name = name
        self

fn longest(users: &(User, User)) -> &User:  # one position: depends on users
    if users._0.name.len() >= users._1.name.len(): &users._0 else: &users._1

fn bad_local() -> &string:  # error: reference-result-without-source
    u := User { name: "a", tags: [] }
    &u.name

fn bad_pick(a: &User, b: &User) -> &User:  # error: ambiguous-reference-source
    a

fn bad_value(u: User, other: &User) -> &string:
    &u.name  # error: local-reference-escapes: depends on `u`, a value parameter
```

### Provenance

An **origin** is a storage root that a local reference may point into:

- `p*`, what parameter `p`'s value refers to, which the caller owns;
- a local of this body, a hidden local of `&e` on a non-place included;
- a parameter's own storage, which the call owns.

Each local-holding value has an origin set. The checker solves the sets
once per body, as Rust's NLL does for regions: the sets are per binding and
location-insensitive. A later stage may make them flow-sensitive and
accept more programs.

| Expression | Origins |
| --- | --- |
| `p` for a parameter `p` whose type holds a local reference | `{p*}` |
| `&x`, `&x.f.g`, for a local or parameter `x` | `{x}`, its own storage |
| `&r.f`, `&*r`, `r.f` auto-addressed, for a reference value `r` | origins of `r` (a reborrow) |
| `&h.p.x` where `h: &H` and `p: *mut P` | origins of `h`; the `*T` crossing changes nothing in stage 1 |
| `&e` on a non-place, `&self` on a temporary | `{hidden local}` |
| a read of binding `b` | `O(b)`: the union of every value stored into `b` anywhere in the body |
| `(a, b)`, `.Some(a)`, a struct or enum literal | the union of its parts |
| `t._0`, a pattern binding of a scrutinee `s` | origins of `t`, of `s` |
| `if`/`match` as a value | the union of the arms |
| `f(..., a, ...)` with source position at `a` | origins of `a` |
| `x?`, an implicit `.Some` | origins of `x` |

The sets are the least solution of these subset constraints. They
terminate because the roots of one body are finite.

```text
fn pick(people: &List[User], fallback: &User, i: usize) -> &User:
    let mut best = fallback        # O(best) gets {fallback*}
    for p in &people:              # p is an element reference
        if p.name.len() > best.name.len():
            best = p               # error: element-reference-outlives-call (E)
    best

fn first_named(u: &User, v: &User) -> &string:  # error: ambiguous-reference-source
    &u.name

fn walk(u: &User) -> &string:
    let mut r = &u.name            # O(r) gets {u*}
    let spare = User { name: "x", tags: [] }
    if r.len() == 0:
        r = &spare.name            # O(r) also gets {spare}
    r                              # error: local-reference-escapes: may depend on `spare`

fn optional(u: &User) -> &string?:
    o := if u.name.len() > 0: .Some(&u.name) else: .None   # O(o) = {u*}
    match o:
        .Some(n) => .Some(n)       # O(n) = O(o)
        .None => .None
```

Loops need nothing special: a binding's set covers every iteration. A
reassignment adds origins and never removes them. That is the one place
where per-binding sets reject a correct program. Polonius-style
flow-sensitive sets would remove that.

### The caller side

The caller may do anything with the source while a result is live: read
it, write it, reassign it, pass `&mut` of it, push to a list in it, or
suspend across a `!` call. Stage 1 needs no exclusivity:

```text
fn demo(u: &mut User) -> void $ Console:
    n := u.name()
    u.name = "changed"   # valid: n refers to the field, and sees the write
    u.tags.push("x")     # valid: no result points into a buffer in stage 1
    t := u.tags_mut()
    t.push("y")          # valid: writes u.tags in place
    println(*n)          # prints "changed"
```

This is sound with no new check:

- A stage-1 result points at a place, never at a buffer, since rule 8
  keeps element references out. A write to the source replaces the value
  in that place, and the place stays where it is.
- Enum payloads are not places (`ref.match.copy`), so no write changes a
  variant under a reference.
- What could free the place holds a count. A source local of a dependent
  call lives in a counted cell, and a result through a `*mut` field holds a
  count of that object. A write that replaces the `*mut` field leaves the
  result on the old object, which stays alive.

That last case is a stale reference, as a `*T` copy is today. Stage 7
rejects it.

### Soundness stress cases

| Case | Stage 1 |
| --- | --- |
| aliasing through `*mut T` | Valid. The result holds a count of the object its path crosses. After `h.p = box(..)` it reads the old object. Stale but safe; stage 7 rejects it |
| copy-on-write buffers | No result points into a buffer (rule 8). `&self.tags` points at the list's header place, and a write through it makes the buffer unique as any write does |
| `push` reallocating | The same: only stage 4 results point into a buffer, and the pin makes `push` panic |
| a reference into a frame local | A local passed as a source argument lives in a counted cell, not a frame slot. Returning it is a rule-4 error. In stage 1, `slot_locals` refuses such locals |
| generics | Rule 9: no local-holding type argument for a generic call. `fn get[T](w: &Wrapper[T]) -> &T` is valid at a plain `T` |
| `&dyn` | `downcast_ref` points where the trait value's data does, and has that reference's origins. A vtable call's result is owned, as every call result is |
| closures | Capturing a result is today's capture error. A function type whose result holds a local reference stays an error until stage 3 |
| suspension | A suspending body keeps its locals in its frame object, so a result across `!` is counted. A bang function may have a dependent result |
| recursion | `fn last(n: &Node) -> &Node` over `next: *Node?` is an error: the payload copy `m` is a local root. A `*Node` result is the natural type there. Stage 7 may make lent counted references origin-free |
| a reference to a temporary | `make().name()` binds a hidden local that lives until the result's last use. hd frees at last use, not at scope end, so Rust's E0716 does not arise |

**Holes.** None found. Stage 1 also closes one that exists today: a
declared `&T` passed to a generic `T` is accepted (probe below). A generic
body may box it, and the caller's frame slot does not count, so the box
could outlive the slot. Rule 9 rejects the call.

### Cost

| Component | Lines, bottom-up |
| --- | --- |
| checker: result types, elision and the two signature errors | 80 |
| checker: rule 1's "holds" through generic type instances; rule 9 at generic calls | 60 |
| checker: origin sets (roots, constraints, solve, expressions, stores) | 250 |
| checker: return check, call results and messages | 100 |
| checker: trait signatures and the downcast branch | 40 |
| backend: `slot_locals` refuses a `Ref` passed to a call whose result holds a local reference | 15 |
| backend: count fixtures that results are owned and never `LENT` | 30 |
| std: `downcast_ref`/`downcast_mut` on `&dyn` in `inspect.hd` and `error.hd` | 12 |
| spec: a Dependent Results section in ch. 13 (about 25 rules) plus changes in ch. 4, 7 and 9 | 130 |
| conformance fixtures, about 25 cases | 300 |

That is about 590 Rust and hd lines, 130 spec lines and 300 fixture lines.
Stop at 1.5 times that. Run time:

- A dependent call's result takes one count, a retain through the page
  table in the callee and a release at the caller's last use. The tier-2
  study measured a count pair at 2.1 ns on V8. The interior lookup
  measured no slower than a base pointer.
- A source local of a dependent call moves from a frame slot to a cell:
  one allocation per execution of its declaration.
- A getter stays a call, since the inliner refuses reference callees.
- Stage 2 removes all three. Wasm size and compile time change by noise:
  one pass over bodies whose types hold local references.

### Diagnostics

- `local-reference-escapes` (existing): rule 4 names the root, as in
  "`spare` is a local of `walk`; the result depends only on `u`". It has no
  fix-it.
- `reference-result-without-source` (new): "a reference result needs a
  reference parameter to borrow from".
- `ambiguous-reference-source` (new): "the result may borrow from `a` or
  `b`; naming the source comes in a later stage".
- `element-reference-outlives-call` (existing), for rule 8.

## Owner Questions

1. **The end state.** Swift 6.4-style lifetime dependence, Kotlin-style
   outlives-only lifetimes, or Rust lifetimes. **Recommendation:** Swift
   6.4-style dependence, with Rust's elision and narrow exclusivity at
   stage 7.
2. **Decision E, item 4.** May element references flow into results and
   bindings (stage 4), enforced by today's pins first and by a static check
   at stage 7? The #494 log says E's length rule "will cover the later tier
   2 entry API". **Recommendation:** yes, at stage 4.
3. **Elision.** Rust's three rules, counting reference positions, or C#'s
   rule that a result may come from any reference argument.
   **Recommendation:** Rust's. C#'s rule never errors, but it ties every
   result to every argument.
4. **Exclusivity, stage 7.** Freeze a borrowed path's shape only (length,
   variant, references along it), or follow Swift's full law, where a live
   `&mut` result bars every other access. **Recommendation:** shape only.
   It generalizes E, and keeps `swap(&mut xs[i], &mut xs[j])` and
   `n := u.name(); u.age += 1` valid.
5. **The `*` fix-it.** `ref.local.escape.fix` still offers "write `*`" on
   every escape row. **Recommendation:** drop it on all rows with stage 1,
   since it cannot work for a field or an element.
6. **Rule 9 closes a slip.** `show(p)` with `p: &T` passed to a generic
   `T` becomes an error. **Recommendation:** yes. It is a soundness fix, and
   stage 5 lifts it properly.
7. **Named-source spelling, stage 3.** A set in the result type, `&{a, b}
   T`, or a Swift-style attribute, `@lifetime(a, b)`. **Recommendation:**
   the set in the type, since stage 6 needs it in struct fields.
8. **The generic default, stage 5.** Every `T` may hold a local reference,
   with `T < global` at persistent boundaries (Rust's model, as in #999).
   Or a type parameter opts in, as Swift's `~Escapable` does.
   **Recommendation:** Rust's. Only a few std headers need `T < global`:
   `box`, provider installs, detached tasks and `dyn` erasure.

## Appendix A: Prior Art

### Swift 6.4

```swift
struct Vec3 {
  var x, y, z: Double
  @_lifetime(&self)
  mutating func at(index: Int) -> MutableRef<Double> { ... }
}
@_lifetime(&array)
func element(of array: inout [Int], at: Int) -> MutableRef<Int>?
```

Swift has the pieces below. hd takes dependence on a named source and
Swift's inference shape. It collapses `borrow` and `copy`: an hd reference
is a value, and a value parameter's own storage is never a source.

- SE-0446 makes values `~Escapable`.
- `@lifetime(borrow x)` gives exclusive read access to `x`. `@lifetime(&x)`
  gives exclusive write access. `@lifetime(copy x)` copies `x`'s own
  dependencies. A method with no parameters defaults to `borrow self`.
- The target of a dependency must be `~Escapable`.
- SE-0519 adds `Ref` and `MutableRef`, implemented for Swift 6.4. While a
  `MutableRef` lives, its target "cannot be used directly at all".
- `@lifetime` itself is still the experimental `@_lifetime`. Its pitch #3
  was posted 2026-02-25.

Sources:
[SE-0446](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md),
[SE-0447 Span](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0447-span-access-shared-contiguous-storage.md),
[SE-0519](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0519-ref-mutableref-types.md),
[lifetime annotation guide](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/LifetimeAnnotation.md),
[pitch #3](https://forums.swift.org/t/pitch-3-compile-time-lifetime-dependency-annotations/84968),
[SE-0176 exclusivity](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md).

### Kotlin: local lifetimes

```kotlin
fun <E, R> local Iterator<E>.fold(init: R, local folder: (R, E) -> R): R
fun iterator(): Iterator<E>_{this}
```

"Kotlin's local" is design note KEEP 0007, "Local Lifetimes", by Ross Tate.
It is research, not a proposal or a release. `local` marks a parameter that
does not escape the call. A result's type names what it depends on,
`_{this&transform}`. Global is the default, and memory safety stays the
GC's.

hd's `&T` is already local by default, and `*T` is global. Stage 3's set
in the result type is Kotlin's `_{...}`. Sources:
[KEEP note 0007](https://github.com/Kotlin/KEEP/blob/main/notes/0007-local-lifetimes.md),
[discussion #485](https://github.com/Kotlin/KEEP/discussions/485).

### Rust: elision and NLL

```rust
fn name(&self) -> &str { &self.name }          // tied to self
fn first(x: &[u8]) -> &u8 { &x[0] }            // the one input lifetime
fn pick<'a>(a: &'a U, b: &'a U) -> &'a U { a }  // two inputs: must be written
```

hd takes the three elision rules as they are:

- each input reference gets its own lifetime;
- one input lifetime goes to every output;
- with several, `&self` wins;
- otherwise the signature is an error.

Adding a second reference parameter never silently changes the source.
Without `&self` it is an error, and with `&self` the source stays `self`.
#999's first draft claimed otherwise, and that claim is wrong.

NLL ends a borrow at its last use, with location-insensitive subset
constraints. hd's per-binding origin sets are the same. Sources:
[elision](https://doc.rust-lang.org/reference/lifetime-elision.html),
[RFC 2094](https://rust-lang.github.io/rfcs/2094-nll.html).

### Mojo: origins

```mojo
def __getitem__(ref self, index: Int) -> ref[self.names[0]] String: ...
def pick_one(cond: Bool, ref a: String, ref b: String) -> ref[a, b] String: ...
```

A `ref` result must name its origin. Mojo keeps each source alive while the
result is live, and enforces exclusivity only between a call's arguments.
hd takes the origin-set view of provenance. It does not take the mandatory
annotation: elision covers the common case. Source:
[Mojo lifetimes and origins](https://mojolang.org/docs/manual/values/lifetimes).

### Hylo: projections

```hylo
subscript min(_ x: yielded Int, _ y: yielded Int): Int {
  inout { if y > x { yield &x } else { yield &y } }
}
```

Hylo has no first-class references. A subscript yields a projection to its
caller for an access, under the Law of Exclusivity. That matches hd's
call-scoped `&T` today. It cannot express a result bound across
statements, which is the point of stage 1. Sources:
[Hylo subscripts](https://hylo-lang.org/docs/user/language-tour/subscripts/),
[the Val object model](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2022/p2676r0.pdf).

### C#: ref returns

```csharp
public ref int GetReferenceToMax() {
    ref int max = ref numbers[0];
    for (int i = 1; i < numbers.Length; i++)
        if (numbers[i] > max) max = ref numbers[i];
    return ref max;
}
```

C# is the closest shipped analogue of stage 1. Ref returns have no
exclusivity, the GC keeps the heap safe, and "ref-safe contexts" keep a
reference from outliving a stack variable. A call's result is as narrow as
its narrowest ref argument, and `scoped` narrows a parameter. That is the
alternative of question 3. Source:
[C# reference variables](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/statements/declarations#reference-variables).

## Appendix B: Today In hd, Verified

Each fact below was checked at cc63fb077. The two spec errors are first.

- **The spec is wrong that local references are counted.**
  `ref.type.kind-runtime` says a local reference is counted at run time.
  The Local References Why says the escape check "changes nothing at run
  time". Both are stale. Since #702 (§23.38, `emit/closure.rs`
  `slot_locals`), a local whose every `Ref` is local and is one call's
  argument lives in a frame slot. Retains through a slot's address do
  nothing, since slot pages are static. The borrow pass's `LENT` loans and
  `&T` parameters take no count either (§23.25, §23.31). The escape check
  is what makes these sound.
- **Otherwise a local reference is counted.** `Ref` emits its address with
  "a count of its own" (`emit/refs.rs` `place_ref`). The count is a retain
  through the page table at any interior address (§23.5, §23.20).
- **An address-taken local** lives in a counted cell, one per execution of
  its declaration (`cell_locals`), unless it qualifies for a frame slot.
- **Lends that assume no escape:**
  - frame slots (#702);
  - `LENT` field references of lent parameters;
  - `&T` parameter loans;
  - span loans (rule 3 refuses a local that has a `Ref`, so they are
    unaffected).
- **The inliner refuses any callee that takes a reference**
  (`hd_opt/src/inline.rs`, `takes_reference`). So `&self` getters are
  never copied.
- **Kept element references are already handled at run time**: each copy
  holds a buffer count, which is the pin (§23.20).
- **Probes** on the dev build (`target/debug/hd check`):

  | Probe | Today |
  | --- | --- |
  | `fn pick(p: &Point) -> &Point` | `local-reference-escapes`, with a fix-it writing `*` |
  | `fn first(..) -> &User?` | `local-reference-escapes` for the type argument, with the same fix-it |
  | `let mut r = &a`, then `r = &x` for an inner-block `x`, then `r.x` | accepted; it runs on cells |
  | `fn pass_on(p: &Point): show(p)` with `show[T](v: T)` | accepted (#987); `show(&q)` is rejected |
  | `debug` of a `*dyn Debug` boxed from a `*Point` | prints `Point { x: 131096, y: 0 }`, a separate backend bug |

  The last bug kept the #987 slip from being shown dangling. File it
  separately.
- **#999 reviewed.**
  - Its slice 0 freezes the source and makes `&mut` results exclusive, with
    borrow flags through `*T`. Stage 1 needs neither, because a stage-1
    result never points into a buffer.
  - Its generic tunnelling (stage 5 here) is right in shape but not
    needed first.
  - Its own estimate for slice 0 is 550 to 800 lines. That includes
    freezing, which this stage 1 drops.

## Appendix C: Parse Log

Every `text` block was parsed with `scripts/hd-conformance.sh parse` at
cc63fb077. Parsing checks syntax only, not types. Five of the six blocks
parse. Block 2, stage 3's, fails at lines 1 and 5 with "expected a type,
found '{'", the two lines marked as hypothetical syntax.
Error comments in the examples are what the proposal reports, not what
today's checker reports.
