# The Path To Returned Local References

**Status:** accepted 2026-10-29; domain: references and lifetimes. Stages 1
and 2 are being built (#1002, #1003, #996, #1006); the stage 3 syntax is
open until the study #1005. It reviews
[Local References](../../spec/lang/13-values-and-references.md#local-references),
[Element References](../../spec/lang/13-values-and-references.md#element-references),
the parked [tier-2 study](OBJECT_MODEL_TIER2.md), Muse's closed PR #999
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

**The end state** is Rust's lifetimes laid over hd's counted references:

- A result, and later a stored value, depends on a lifetime variable, as
  Rust's `&'a T` does. One form covers functions, struct and enum fields,
  function types, trait methods and stores into a caller's place.
- Lifetimes default by Rust's elision rules, so ordinary code writes none.
- Memory safety comes from counts until exclusivity lands at stage 7.
- A lifetime says how long, not which part of the source a result points
  into. hd accepts that imprecision, as Rust does. Stage 7's rule works on
  whole sources, and only the optional stage 9 adds paths.

Against Kotlin: counting alone keeps every heap-crossing reference and
element reference counted, and enum payloads can never become places. It
also misses the owner's accepted rule that a guard's mutable capture is
exclusive (#978 item 2). Against Rust's full borrow checker: hd's counted
`*mut` aliases defeat a purely static checker, so hd keeps counts and pins
as the run-time fallback.

### Why not path origins

Mojo can name a path in a result: `-> ref[self.names] String`. A path
origin says which part of the source a result points into, so a caller
could tell a harmless write from a dangerous one. But a path only works
where the result's source is a parameter with fields. Every other position
needs a variable again:

```text
fn get_name(user: &User) -> &[user.name] string   # hypothetical syntax: a path
struct Parser[o]:                                 # hypothetical syntax: a variable
    src: &[o] string                              # hypothetical syntax
fn push_ref[o](xs: &mut List[&[o] User], x: &[o] User) -> void   # hypothetical syntax: a variable again
trait Named:
    fn name(&self) -> &[self] string              # hypothetical syntax: a trait cannot name an impl's fields
```

So a path design has two forms, paths in function results and variables
everywhere else, plus Mojo 1.0's interior origins for collections. A
`pub` signature would also publish private field paths. Rust needs one
form:

```text
fn get_name(user: &User) -> &string: &user.name    # elided: depends on user
fn choose['a](a: &'a User, b: &'a User, first: bool) -> &'a User   # hypothetical syntax
struct Parser['a]:                                # hypothetical syntax
    src: &'a string                               # hypothetical syntax
fn push_ref['a](xs: &mut List[&'a User], x: &'a User) -> void   # hypothetical syntax
fn apply(f: fn(&User) -> &string) -> void: pass   # elided inside the function type
trait Named:
    fn name(&self) -> &string                     # elided: depends on self
```

| | Rust lifetime variables | Mojo path origins | Swift named sources |
| --- | --- | --- | --- |
| Precision at a call | the whole source | a field path in a result; whole value elsewhere; field-level exclusivity is not documented | the whole source; path dependence (`@lifetime(self.storage)`) exists only in a draft |
| Forms needed | one: `'a` in functions, types, stores and function types | paths in results, origin parameters in types and stores, interior origins | an attribute on functions, `~Escapable` plus `@_lifetime` on initializers in types, `target:` for stores |
| Annotation burden | none where elision applies | an origin on every `ref` result | none where inference applies |
| Checker cost | origin sets and NLL-style liveness, already planned for stage 1 | path-aware loans and invalidation | dependence checking plus exclusivity |
| Prior-art maturity | shipped since 2015, NLL since 2018 | 1.0 in August 2026; interior origins experimental | `@_lifetime` still experimental; `Ref` and `MutableRef` in Swift 6.4 |

**Recommendation:** Rust's lifetime variables. The trade-off is precision
at a call. At stage 7, a live result freezes every write to its whole
source, so `n := u.name(); u.age += 1` is an error.
Optional stage 9 (views) would release such writes if real code needs it.

Stage 1 writes no lifetime and elides to the whole parameter, so it is
unchanged.

## The Stages

Each stage is sound on its own, using run-time counting where its static
rules stop. Each heads toward the end state. Hypothetical syntax is marked.

| Stage | Adds | Unlocks | Static or run time |
| --- | --- | --- | --- |
| 1. Dependent results | a result may hold local references; its source is elided; origins are checked in the body | getters, `&mut Self` builders, `downcast_ref` on `&dyn`, `(&T)?` results | static escape check; memory safety by counts (cells) |
| 2. Frame-slot results | backend only: source liveness covers the result's uses; inlining of reference getters | the counts and cells stage 1 adds go away | no language change |
| 3. Lifetime variables | `fn f['a](x: &'a T) -> &'a T` (hypothetical); dependent function types; per-position precision | `choose(a, b)`, `get_or(&self, fallback)`, closures that return references | static |
| 4. Element results | reverses E item 4 (owner, 2026-10-29): element references flow into results and bindings | `List.first`/`get`/`get_mut`, `Map.get`, `r := &mut xs[i]` | pins panic on a length change (exists today) |
| 5. Generic locality | Rust's model: any `T` may hold a local reference; `T < global` (hypothetical) only at persistent boundaries such as `box`, provider installs, detached tasks and `dyn` erasure | `Option` methods on `(&T)?`, `debug(&x)`, `id(&x)` (#987) | static |
| 6. Stored references | lifetime parameters on structs and enums; stores into a caller's place; scoped closures; guards; `Span` | the #978 items: phantom markers, guards, read-only captures, library `defer`/`ensure`, live spans | static, counts at heap crossings |
| 7. Exclusivity | Swift's `borrow`/`&` and Rust's rule: a live shared result freezes writes to its whole source, a live `&mut` result every other access; static where the checker sees the source, a borrow flag through a `*mut` alias | enum payload places (`as_mut`), exclusive guard captures, compile errors in place of pin panics | static plus pins |
| 8. Uncounted scoped references | the backend drops counts that stage 7 proves unnecessary; borrow flags where it cannot prove them | the perf payoff | flags only through `*mut` |
| 9. Views, if needed | a parameter names the fields it borrows, `user: &{name} User` (hypothetical, Rust's view-type experiment) | `n := get_name(&u); u.tags.push(x)` | static |

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

### Stage 3: lifetime variables

```text
fn choose['a](a: &'a User, b: &'a User, first: bool) -> &'a User:  # hypothetical syntax
    if first: a else: b

impl[T] Pair[T]:
    fn left_or['a](&'a self, fallback: &'a T) -> &'a T:  # hypothetical syntax
        &self.left
```

A lifetime variable is a generic parameter, as in Rust. Two references
with one variable may both be the source. Stage 6 reuses the same variables
in struct and enum fields (`struct Parser['a]`), and a store into a
caller's place shares one (`xs: &mut List[&'a T], x: &'a T`). A function
type takes the same elision as a declaration, so `fn(&User) -> &string`
depends on its one parameter.

**A candidate syntax, not decided.** One source set may hold every kind of
source, as Mojo 1.0's `ref[...]` holds both expressions and origin
parameters:

```text
fn get_name(user: &User) -> &{user.name} string        # a field path   (hypothetical syntax)
fn first(&self) -> &{self.items[_]} T?                 # an element      (hypothetical syntax)
fn choose(a: &User, b: &User) -> &{a, b} User          # either source   (hypothetical syntax)
struct Parser[^src]:                                    # a lifetime variable on a type
    text: &{^src} string                                # (hypothetical syntax)
fn push[^a, T](xs: &mut List[&{^a} T], x: &{^a} T)     # a store into a caller's place
```

A path makes stage 7's freeze precise: while `n := get_name(&u)` lives,
only `u.name` is locked, so stage 9's views would not be needed. Open
points:
- the sigil (`^l`, `'l` or another);
- paths through private fields in a `pub` signature;
- traits, which can't name an impl's fields;
- precision past a `*mut` on the path.

Before stage 3 starts, a study (#1005) prices it: the implementation cost of
path-aware loans across calls, and a stress test on real code against
plain lifetime variables.

### Stage 4: element results

Stage 4 reverses item 4 of decision E (#494): an element reference may
flow into a result or a binding. E's length rule stays, as the pin.

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
§23.20). So stage 4 is checker work.

### Stage 6: stored references

Stage 6 lets a lifetime variable appear in a struct or enum, and in a
store into a caller's place. A value of `Parser['a]` holds a local
reference, so every Local References rule applies to it.

```text
struct Parser['a]:                         # hypothetical syntax
    src: &'a string                        # hypothetical syntax
    pos: usize

fn parser(src: &string) -> Parser:         # elided: Parser['src]
    Parser { src: src, pos: 0 }

fn remember['a](seen: &mut List[&'a User], u: &'a User) -> void:   # hypothetical syntax
    seen.push(u)
```

Guards, scoped closures and `Span` reuse the same variable: `Span['a, T]`
and a closure type `fn() -> void + 'a` (hypothetical).

### Stage 7: exclusivity

Stage 7 follows Swift's lifetime dependence and Rust: a lifetime names a
whole source, so a live result locks all of it.

- While a shared result (`&T`, Swift's `borrow`) is live, its source may be
  read but not written: an assignment, a `&mut self` call, or `&mut` of it
  passed to a call is `conflicting-access`.
- While a `&mut` result (Swift's `&`) is live, its source may not be used at
  all except through that result.
- The lock ends at the result's last use, not at the end of the block, as
  with Rust's non-lexical lifetimes.
- Where the checker sees the source (a local, a parameter, a path of
  fields from one), the check is static. Through a `*mut` alias it cannot
  see, a borrow sets a flag on the object and a conflicting write panics
  `borrow-conflict`, as Swift does for class properties and Rust's
  `RefCell` does.

```text
fn demo(u: &mut User) -> void $ Console:
    n := u.name()        # n borrows all of u
    println(u.age)       # valid: a read
    u.age += 1           # stage 7 error: u is borrowed by n
    println(*n)          # n's last use: the lock ends
    u.age += 1           # valid
    t := u.tags_mut()    # t borrows u exclusively
    println(u.age)       # stage 7 error: u is borrowed exclusively by t
    t.push("y")          # valid: through t
```

The imprecision is accepted: `u.age += 1` is rejected while `n` is live,
though `n` points only into `u.name`. The fixes are Rust's: end the use of
`n` first, copy the value out (`name := *u.name()`), or borrow the field
directly (`&u.name`), which the checker sees is only `name`. A finer rule
that keeps writes of buffer-free values valid is a relaxation, so it breaks
no code and may come later if real programs hit the lock often.

**What stage 7 rejects that earlier stages allowed:**

- in stage 1, any write to the source while a result is live, as
  `n := u.name(); u.age += 1`;
- in stage 4, a length change while an element result is live (it
  panicked `collection-pinned`);
- replacing a `*mut` that a live result's path crosses (it left a stale
  result).

### Stage 9: views, if needed

A parameter may name the fields its body reaches, `fn get_name(user:
&{name} User) -> &string` (hypothetical). The caller then freezes only
`u.name`, so `u.tags.push(x)` stays valid while the result lives. It is
Rust's own unfinished direction: a `view_types` experiment is gated in
2026. Views are taken only if stage 7's whole-source freeze proves too
strict in real code.

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
3. **Elision, as in Rust.** A result that holds a local reference depends on `self`
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
    age: i32
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
    u := User { name: "a", age: 0, tags: [] }
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
    let spare = User { name: "x", age: 0, tags: [] }
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
| recursion | `fn last(n: &Node) -> &Node` over `next: *Node?` is an error: the payload copy `m` is a local root. A `*Node` result is the natural type there, as `Rc` is in Rust |
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
  "`spare` is a local of `walk`; the result depends only on `u`".
- No reference or type diagnostic has a fix-it until the type system is
  settled. Stage 1 makes every `local-reference-escapes` row name only the
  escape position (`ref.local.escape.position-message`): the former "write
  `*`" fix-it cannot work for a field or an element, and a counted reference
  is a different design choice, not a repair.
- `reference-result-without-source` (new): "a reference result needs a
  reference parameter to borrow from".
- `ambiguous-reference-source` (new): "the result may borrow from `a` or
  `b`; naming the source comes in a later stage".
- `element-reference-outlives-call` (existing), for rule 8.

## Owner Questions

None open. The owner answered every question on 2026-10-29; the answers are folded into the text above.

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

Swift has the pieces below. hd takes Swift's inference shape, but a
lifetime variable instead of a named source. It collapses `borrow` and `copy`: an hd reference
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

hd's `&T` is already local by default, and `*T` is global. Kotlin's
`_{this}` names a source, as Swift's attribute does; hd takes Rust's
variables instead, for the reasons in "Why not path origins". Sources:
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
def __getitem__(ref self, index: Int) raises -> ref[self.names[0]] String: ...
def pick_one(cond: Bool, ref a: String, ref b: String) -> ref[a, b] String: ...
struct ParametricRef[is_mutable: Bool, //, origin: Origin[mut=is_mutable]]: ...
fn to_byte_span[origin: Origin](ref[origin] list: List[Byte]) -> Span[Byte, origin]: ...
```

Verified against Mojo's manual and the 1.0 release notes:

- A `ref` result must name its origin.
- An origin may be an arbitrary expression, shorthand for
  `origin_of(expression)`, which is never evaluated. So `origin_of(x.y)`
  and `ref[self.names[0]]` are allowed. The manual uses one to take a
  stored string's origin rather than its owner's.
- Structs take origin parameters (`Span[T, origin]`). A union, `ref[a, b]`,
  keeps both sources alive.
- The wildcards `MutUnsafeAnyOrigin` and `ImmUnsafeAnyOrigin` disable
  exclusivity checking and ASAP destruction. Since 1.0 a struct field can
  no longer hide one.
- Exclusivity is documented only between a call's arguments, by value. We
  found no statement that `origin_of(self.a)` and `origin_of(self.b)` are
  disjoint for that check.
- Mojo 1.0 (2026-08-11) adds experimental **interior origins**. `List`
  element references bind to the list's interior, so `list.append(4)`
  invalidates them, and a later use is "use of invalidated interior
  reference".
- Chris Lattner's May 2026 forum reply sketches "indirect origins", valid
  until the owner is mutated. There is no design doc yet.

hd takes the origin-set view of provenance in the body (stage 1). It
rejects path origins in signatures: they need variables beside them, as
"Why not path origins" shows. Mojo's interior origins are hd's stage-4 pin
made static, which stage 7's freeze also gives. Sources:
[Mojo lifetimes and origins](https://mojolang.org/docs/manual/values/lifetimes),
[ownership and argument exclusivity](https://mojolang.org/docs/manual/values/ownership),
[Mojo 1.0 release notes](https://mojolang.org/releases/v1.0.0/),
[group origin proposal and Lattner's reply](https://forum.modular.com/t/proposal-group-origin/3010).

### Paths elsewhere: Rust views, Swift, Scala

- **Rust view types.** Matsakis's proposal lets a method borrow only named
  fields: `self.{messages}` in the 2026 draft, not final. T-lang discussed
  a `view_types` experiment gate on 2026-04-29. It is not accepted, and it
  is stage 9 here. Sources: [view types (2021)](https://smallcultfollowing.com/babysteps/blog/2021/11/05/view-types/),
  [T-lang notes](https://hackmd.io/pIbUgMQuQcGaibJcinOcEw).
- **Swift.** Shipped dependence names parameters (`borrow self`, `&self`).
  A per-property form, `@lifetime(self.storage)`, appears only in Andrew
  Trick's draft note "Property lifetimes". Source:
  [property lifetimes draft](https://gist.github.com/atrick/9409356c89a5f67dd9f68f708f57262e).
- **Scala 3 capture checking.** Capture sets name variables, `this` and
  parameters, and reach capabilities `x*` name what a value's contents
  capture. We found no user-level field paths. Source:
  [What's in the Box (OOPSLA 2025)](https://www.arxiv.org/abs/2509.07609).

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
its narrowest ref argument, and `scoped` narrows a parameter. hd takes
Rust's elision instead, which ties a result to one source. Source:
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
f20e7ff17 plus this edit. Parsing checks syntax only, not types. Six of
the ten blocks parse. The other four fail only on lines marked
hypothetical syntax:

- the path-origin sketch, on `&[...]`;
- the Rust-form sketch, on `'a`;
- stage 3, on `'a`;
- stage 6, on `'a`.

Error comments in the examples are what the proposal reports, not what
today's checker reports.
