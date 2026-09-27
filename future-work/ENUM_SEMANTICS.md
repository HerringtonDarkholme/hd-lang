# Enum Semantics: Value Category And Identity

Status: decided 2026-09-27 (see Owner Decisions); not yet applied. Research from 2026-09-26 below; nothing here is normative;
the [specification](../spec/README.md) is unchanged.

The specification has two sealed auto traits under `Any`
([Prelude](../spec/10-modules.md#prelude)):

- `AnyVal`: scalars, `string`, and tuples. These values have no identity and
  are immutable.
- `AnyRef`: data values, `List`, `Map`, closures, trait values, and the other
  values with identity.

`AnyRef` replaced the earlier sealed marker `Reference`, so `is` and
identity generics are `AnyRef`-bounded. The
[value-category table](../spec/04-type-system.md#value-categories) puts every
enum on the reference side, `Option` and `Result` included. A payload-free
variant has one canonical identity.

This document asks where enums belong. It covers what the specification says
today, how other languages decide, four options with their consequences, a
recommendation, and questions for the owner.

This document was written before the rename landed; its examples use the
current names. No example needs new syntax. Every `text` block below parses with
the [reference parser](../spec/reference-parser/index.ts), checked
2026-09-26. Parsing is not type checking: each example states what today's
rules and each option would do with it.


**Status: deferred by the owner (2026-09-26).** The enum value category
(question 1) and whether shared constructor data becomes read-only
(question 2) are postponed; until then the specification keeps enums on
`AnyRef` as it is today.

## Owner Decisions

Decided 2026-09-27:

1. **Question 1: keep option (a).** Every enum, `Option` and `Result`
   included, stays `AnyRef`; each construction of a variant with payloads
   has its own identity, and a payload-free variant has one canonical
   identity. `downcast`, `find`, and `AnyRef`-bounded dynamic methods keep
   working for enums. Implementation note (no rule change; decided
   2026-09-27): every enum uses the reference shape, with one
   representation. A payload-free variant, in any enum, is an `i31ref`
   holding its tag (no allocation, and `ref.eq` on it is the canonical
   identity); a variant with a payload is a GC struct, one struct subtype
   per variant of the enum's base type; an enum-typed slot is `eqref`, and
   `match` tests for `i31` first, then reads the struct's tag. No `i32`
   form: one representation keeps implementations simple, at the cost of an
   `i31.get` per tag read and pointer-sized storage. `.None` may be the
   `i31ref` tag or a null reference.
2. **Question 2: shared constructor data is read-only.** An enum's own
   slots never change once built; this is shallow, since a payload may be a
   mutable reference.

4. **Shared constructor data becomes per-variant constants** (decided
   2026-09-27, revising decision 2's storage): the declaration syntax
   stays (`enum HttpStatus(code: i32, phrase: string):` with
   `NotFound -> HttpStatus(404, phrase="Not Found")`), but each variant's
   `->` values are evaluated once at compile time (requirement-free, the
   annotation evaluator), stored once per variant in a table indexed by
   tag, and never stored in enum values; `s.code` reads the table. The `->`
   expression cannot use the payload. Payload-free variants therefore stay
   plain `i31ref` tags even with shared data; typed derivation reads shared
   data from the variant (`v.info`), never as members, and `build` never
   fills it; `@error`'s review R12 rule about common fields becomes moot.
   Per-value common data (a different span on each value) belongs in each
   variant's payload or a wrapper data type. Like Java and Kotlin enum
   constructor arguments.
3. **Question 7: payload-free enums get no automatic `Eq` or `Hash`.**
   `PayloadLess.A is PayloadLess.A` is always `true` (canonical identity),
   and `==` needs `@derive(Eq)` (and `Hash` for map keys), as for every
   other enum; adding a payload never silently removes a conformance.

The remaining questions (3-6, 8) assumed option (b) and are closed by
decision 1.

## Contents

- [Summary](#summary)
- [What The Specification Says Today](#what-the-specification-says-today)
- [Question 1: Are Enum Values Immutable?](#question-1-are-enum-values-immutable)
- [Question 2: Is Identity Meaningful For Enums?](#question-2-is-identity-meaningful-for-enums)
- [Survey](#survey)
- [The Options](#the-options)
- [Question 3: Consequences](#question-3-consequences)
- [Question 4: Recommendation](#question-4-recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Findings Outside The Main Question](#findings-outside-the-main-question)
- [Sources](#sources)

## Summary

An enum's variant and payloads are fixed once it is built. The one exception
is a named field of shared constructor data, which a `mut` view may reassign.
Nothing else in the language can change an enum value. So `is` is the only
way to tell two equal constructions apart. It answers a question no other
operation can raise, and it costs something. Each `.Some(value)`, including
each implicit wrap of a `T` into `T?`, must allocate a fresh record. `.Some`
can never be the payload itself, and equal constructions can never be shared.

The recommendation is option (b): every enum, `Option` and `Result`
included, is `AnyVal`. Shared constructor data becomes read-only. An enum is
then a nominal, tagged version of a tuple. Its category does not depend on
its variants or payload types, so a library can add a variant or a payload
without breaking callers.

The main cost is the dynamic-safety rule (G2). A dynamically safe trait
method can take a generic argument only when it is `AnyRef`-bounded, so
enums cannot pass through such methods. `downcast` is one of them, and error
enums are the most common downcast target. The fix proposed here: the static
`downcast_val` recovers enums too, and `std.error.find` becomes a static
function.

## What The Specification Says Today

**Construction and identity**
([Unary And Binary Operators](../spec/05-expressions.md#unary-and-binary-operators)).

- A payload-free variant without shared data is canonical. Every
  `Color.Red` is the same value, and building one allocates nothing.
- A variant with a payload has allocation identity per construction:
  `!(Shape.Circle(1) is Shape.Circle(1))`.
- A variant that carries shared constructor data allocates on every
  construction, even with no payload of its own. `Color.Red is Color.Red`
  holds, but `HttpStatus.Ok is HttpStatus.Ok` does not.
- Optionals follow the same rules (owner decision A3, recorded in the
  [Revision Notes](../spec/README.md)). `.None` is canonical. Each
  `.Some(value)` has its own identity, distinct from its payload's. That
  includes the implicit wrap of a `T` where `T?` is expected. The
  [implementation model](../spec/04-type-system.md#composite-representation)
  states the consequence: "a present value cannot be represented by the
  payload itself."
- Converting to `Any` or a trait value keeps the identity of a heap
  composite and of a canonical variant. A primitive or tuple instead gets a
  fresh box per conversion.

**Mutation.**

- There is no syntax to assign a payload field or to change a variant.
  Payload fields are reached only through patterns
  ([Shared Enum Constructor Data](../spec/08-data-and-enums.md#shared-enum-constructor-data)),
  and pattern bindings are new names, not places
  ([Match Expressions](../spec/06-control-flow.md#match-expressions)).
- Named shared constructor data is a field on the enum value, and "assignment
  requires a mutable enum root and the required mutable edges"
  ([Shared Enum Constructor Data](../spec/08-data-and-enums.md#shared-enum-constructor-data)).
  A stored enum construction produces mutable access to its new outer object
  ([Bindings And Fresh Values](../spec/04-type-system.md#bindings-and-fresh-values)).
  So the following is legal today:

```text
enum HttpStatus(code: i32, phrase: string, retryable: bool = false):
    Ok -> HttpStatus(200, phrase="OK")
    Busy -> HttpStatus(503, phrase="Busy", retryable=true)

fn calm(status: mut HttpStatus) -> void:
    status.retryable = false
```

  This one sentence in chapter 08 is the only mutation of enum storage in the
  language. No conformance fixture exercises it. The prototype does not
  implement it: `let s: mut HttpStatus = HttpStatus.Busy` is rejected with
  `mutable-upgrade`, because the prototype never gives an enum construction
  `mut` access.
- `mut E` has one other effect. A non-generic payload declared `mut U`
  follows the data-field rule: through `mut E` it binds `mut U`, and through
  a readonly `E` it binds `U`
  ([Mutable Paths](../spec/04-type-system.md#mutable-paths)). A generic
  payload binds its substituted type either way, as a tuple element does.

**Equality and hashing.** User enums have no implicit `Eq`, `Hash`, or
ordering. They derive them
([Comparison Traits](../spec/09-traits.md#comparison-traits)). The standard
library implements equality for `Option` and `Result`. `==` never falls back
to identity.

**Generics and dynamic safety.**

- `is` on a type parameter requires `T < AnyRef`
  ([Generics](../spec/04-type-system.md#generics)).
- A dynamically safe trait may have a method-level generic parameter only
  when it is bounded by `AnyRef` (G2,
  [Dynamic Trait Values](../spec/09-traits.md#dynamic-trait-values)). All
  arguments then share one reference-shaped body.

**Runtime type identity.** [INSPECTABLE.md](INSPECTABLE.md#owner-decisions)
decision 15 makes `downcast` and `downcast_mut` default methods of
`Inspectable`, bounded by `T < AnyRef + Inspectable`. Values without
identity use the static free function
`std.inspect.downcast_val[T < Inspectable](value: Inspectable) -> T?`.

**Representation**
([Composite Representation](../spec/04-type-system.md#composite-representation),
[Representation And Garbage Collection](../spec/08-data-and-enums.md#representation-and-garbage-collection)).
An enum is a tagged representation, and payload-free variants are canonical
constants. Programs must not depend on tags or addresses. In the
[shape table](../spec/04-type-system.md#shapes-and-generic-code), enums and
optionals use the reference shape. GADT refinements are static; the runtime
value carries only its ordinary tag and payload
([Runtime Representation](../spec/13-gadts.md#runtime-representation)).

**The prototype** (`src/emitter`):

- A user enum is one flat struct: an `i32` tag plus the union of every
  variant's fields. Each field is declared `(mut ...)`. Unused fields hold
  defaults.
- A payload-free variant of an enum without shared data is an immutable
  global built once. Every other construction runs `struct.new`.
- `Option` and `Result` share a runtime struct `$hd.variant` holding a tag
  and an `anyref` payload, with scalars boxed. `.None` is allocated at each
  construction, including in the map runtime's `get`. `is` on optionals is
  emulated: two values count as identical when the references are equal or
  both tags are `None`.
- That emulation does not reach generic code. Two `.None` values passed to
  `fn same[T < AnyRef](left: T, right: T) -> bool: left is right`
  compare as distinct, which contradicts chapter 05. Keeping `.None`
  canonical takes deliberate work under option (a).

## Question 1: Are Enum Values Immutable?

Yes for the variant and its payloads. No for named shared constructor data.

| Part of an enum value | Reassignable today? | What `mut E` grants |
| --- | --- | --- |
| Variant tag | no; there is no syntax | nothing |
| Payload field | no; reached only through patterns | nothing directly |
| Object behind a non-generic `mut U` payload | the object, yes; the payload slot, no | the pattern binds `mut U` instead of `U` |
| Object behind a generic payload `P = mut U` | the object, yes | nothing extra; `mut U` either way |
| Named shared constructor data | **yes**, through a `mut E` root | field assignment |
| Unnamed shared data (`._0`) | no assignment form is specified | unclear |

So `mut` applies to enum payloads only as a viewpoint on the objects they
point to, the way a data field `field: mut U` works. The payload slot itself
never changes. Enum storage is "immutable except shared data". A value
reached through a payload can still change through another alias: a
`Filled(user: mut User)` payload keeps pointing at the same `User` whose
fields may change. That matches tuples, which the owner already classes as
`AnyVal` even when they hold references.

## Question 2: Is Identity Meaningful For Enums?

Identity is observable only when some operation can tell two values apart
without comparing their contents. For data values, mutation does that:
change one alias and the other alias sees it. For immutable enum storage,
nothing like that exists. The only observers are:

1. `is`, directly or through a `T < AnyRef` generic;
2. `is` on the `Any` or trait value made from an enum, which keeps the
   enum's identity;
3. assignment to shared constructor data, seen through another alias.

There is no identity hash, identity map, or weak reference in the language
or the drafted standard library, so identity cannot key a side table. For
payload-free variants, canonical identity coincides with value equality:
`Color.Red is Color.Red` gives the same answer under either model. For
payload variants, `is` separates two equal constructions and nothing else.

What that identity costs:

- **Allocation.** Every `.Some`, `.Ok`, `.Err`, and payload construction
  allocates a fresh record, including each implicit `T` to `T?` wrap and
  each successful `Map.get`.
- **No niche.** `User?` cannot be a nullable reference to the `User`.
  Rust, Swift, and Kotlin all use that representation.
- **No sharing.** The compiler cannot hoist `Expr.Lit(0)` out of a loop,
  intern constant constructions, or merge two `.Some(user)` built from the
  same `user`.
- **No unboxing.** A `Result[i32, E]` returned from a function cannot travel
  as a `(tag, value)` pair in Wasm locals or multi-value results. The
  specification already allows that for tuples.
- **Mutable fields.** Every enum field must be a mutable Wasm field, just so
  that shared data can be assigned.
- **An odd asymmetry.** `HttpStatus.Ok is HttpStatus.Ok` is false while
  `Color.Red is Color.Red` is true.

The four options follow from how much of this identity the language keeps.

## Survey

| Language | Payload-free ("C-like") | With payloads | Identity operator on enums | Notes |
| --- | --- | --- | --- | --- |
| Rust | value; `Copy` opt-in; castable to an integer | value, inline; recursion needs `Box` | none; addresses belong to places, not values | Niche optimization: `Option<&T>` and `Option<Box<T>>` are one pointer wide. |
| Swift | value | value; `indirect` boxes recursive cases and keeps value semantics | `===` only on `AnyObject` (classes); enums are never `AnyObject` | `Optional` of a class is one pointer, with null for `none`. Swift's `Any` over `AnyObject` is the closest match to the `AnyVal`/`AnyRef` split. |
| Kotlin | `enum class` entries are singleton objects | sealed classes, references | `===` works on references; a compile error on `value class` | Nullable `T?` is not a wrapper: `T??` collapses to `T?`. `kotlin.Result` is a value class. |
| Java | enum constants are singletons | records under sealed interfaces, references | `==` is identity | JEP 401 (Valhalla) removes identity from value classes; `Optional` and other "value-based" classes migrate to them. `==` on a value object compares state. |
| Scala 3 | `enum` cases are singleton objects | case classes, references (`AnyRef`) | `eq` on `AnyRef` | Scala coined `AnyVal`/`AnyRef`; `Option` is an `AnyRef`, and `Some` allocates. |
| OCaml | immediate integers | boxed blocks with a tag | `==` exists, but on immutable values its result is implementation-dependent; it only guarantees `e1 == e2` implies `compare e1 e2 = 0` | OCaml keeps the operator but declines to specify identity for immutable data. |
| Haskell | shared static closures | boxed; pointer tagging stores the constructor | none, except unsafe primitives | Sharing is invisible by design. |
| F# | union cases, reference type by default | reference by default; `[<Struct>]` gives value unions without recursion | `LanguagePrimitives.PhysicalEquality` | `option`'s `None` is `null`; `voption` is the struct form. Unions get structural equality automatically. |
| C# | integer-backed `enum` | no shipped discriminated unions; a union proposal is in design | reference types only | Sum types are emulated with records and inheritance, as references. |
| Go | `iota` integer constants | no sum types; interfaces emulate them | interface comparison compares dynamic type and value | |
| Zig, Odin | integer tags | tagged unions are plain values | none | |
| MoonBit | payload-free enums lower to integers | boxed | physical equality is a library function, not an operator | Targets Wasm GC, like hd. |

Two lessons stand out:

- Languages with immutable sums and no mandatory boxing (Rust, Swift, Zig,
  Haskell) give enums no identity. They then share, unbox, and niche-encode
  freely.
- Languages whose sums are references (Java, Scala, Kotlin sealed classes)
  give identity to values that never needed it. Java is now retrofitting
  `Optional` into an identity-free value class. OCaml avoids the question by
  leaving `==` unspecified on immutable data. Kotlin avoids it by making
  `===` a compile error on value classes. hd prefers a compile error to an
  unspecified result.

## The Options

**(a) All enums are `AnyRef` (today).** Payload-free variants keep one
canonical identity. Every other construction has its own.

**(b) All enums are `AnyVal`.** `is` on an enum type is rejected, like `is`
on a tuple. Equality comes only from `Eq`. The compiler may share,
copy, intern, unbox, or niche-encode enum values. Shared constructor data
becomes read-only, so enum storage is fully immutable. `mut E` survives only
as the payload viewpoint from question 1.

**(c) Split by payload.** An enum whose variants all lack payloads and
shared data is `AnyVal`. Every other enum is `AnyRef`. `Option` and `Result`
have payloads, so they stay `AnyRef`.

**(d) Split by payload category.** An enum is `AnyVal` when every payload
and shared-data type is `AnyVal`, and `AnyRef` otherwise. `Option[i32]` is
`AnyVal`, `Option[User]` is `AnyRef`, and `Option[T]` depends on `T`.

A fifth variant came up and is set aside. **(b') `AnyVal` with a
substitutability `is`**, following Valhalla and Julia's egal: `a is b` holds
when both have the same variant and pairwise `is`-equal payloads. That
would keep A3's "`is` works on optionals" without forcing allocation. But
`is` would stop being a constant-time check, recursive enums would make it
walk whole trees, and hd already rejects `is` on tuples and strings instead
of defining it by contents. [Question 3](#3-if-enums-are-anyval-what-happens-to-is)
keeps it open for the owner.

## Question 3: Consequences

| Concern | (a) all `AnyRef` | (b) all `AnyVal` | (c) payload-free `AnyVal` | (d) `AnyVal` if payloads are |
| --- | --- | --- | --- | --- |
| `is` on enums | allowed; A3 as written | rejected; A3's identity clause withdrawn | rejected on C-like enums only | depends on payload types |
| `is` on `T?` in generic code | needs `T? < AnyRef`, always true | never allowed | always allowed | unknown for `Option[T]` without a bound on `T` |
| `Option[T]` representation | fresh record per `.Some` | niche possible: `.Some(x)` may be `x` | fresh record per `.Some` | niche only where it does not help |
| G2 method generics (`[T < AnyRef]`) | accept every enum | reject every enum | reject C-like enums | accept some instantiations |
| `downcast[FsError]` (method) | works | rejected by the bound; use `downcast_val` | works unless `FsError` is C-like | depends on payloads |
| `downcast[Option[User]]` | works | use `downcast_val` | works | works; `Option[i32]` needs `downcast_val` |
| Erasure to `Any` | keeps identity | fresh box per conversion, as for tuples | box for C-like enums only | mixed |
| Map keys | derive `Eq` + `Hash` | same; with shared data read-only, an enum key over `AnyVal` payloads can never become a ghost entry | same as (a) | same as (a) |
| Allocation | every payload construction | none required; allocate only when useful | every payload construction | only for `AnyVal` payload sets |
| Wasm GC | mutable fields, global singletons | immutable fields, constant globals, i31ref, multi-value in locals | as (a), plus integers for C-like enums | mixed per instantiation |
| Evolution | stable | stable | adding the first payload or shared field flips the category | changing any payload type flips it, per instantiation |
| Enum holding `mut List` | viewpoint rule | viewpoint rule; copies are shallow | same | same |
| Recursive enums | heap records | heap records; no `indirect` needed | same | same |
| GADTs | no effect | no effect; payload-free witnesses are `AnyVal` | no effect | refined results may land in different categories |
| Least common type | no effect | no effect | no effect | no effect |
| Shared-data assignment | allowed | removed | allowed on payload enums only | depends |

### Generics and `Option[T]` uniformity

A generic function is compiled once per shape, and every reference-shaped
instantiation shares one body
([Shapes and Generic Code](../spec/04-type-system.md#shapes-and-generic-code)).
So `Option[T]` needs one representation for every reference-shaped `T`.

Under (a), that representation is a tagged record per `.Some`. Under (b), a
niche is possible even in the shared body. `.None` is `null`, and
`.Some(x)` is `x` itself unless `x` is `null` or a private `Some` box, in
which case it is wrapped in that box. Matching tests `null`, then the box
type, and otherwise treats the value as a present payload. `T??` still works:
`.Some(.None)` is a box holding `null`, distinct from `.None`. Kotlin's
collapse of `T??` is avoided. The construction test is one `ref.is_null`
plus one `ref.test`.

```wat
;; Illustration only; one possible lowering under option (b).
(type $some.box (struct (field anyref)))
;; .Some(x) in a reference-shaped generic body:
;;   if x is null or x is a $some.box -> (struct.new $some.box x)
;;   else                             -> x
;; match: null -> .None; $some.box -> .Some(field 0); anything else -> .Some(itself)
```

Under (d), the category of `Option[T]` depends on `T`. A generic body over
`T?` could not know whether `is` applies, and the shared body could not use
the niche for `AnyRef` instantiations. (d) gains little and makes generic
code harder to reason about.

### Dynamic safety (G2) and `downcast`

This is the real cost of (b) and, in part, of (c). G2 limits method-level
generics of dynamically safe traits to `AnyRef`-bounded parameters. The
bound does two jobs:

1. It lets the one body use `is` on `T`.
2. It guarantees that `T`, and every nested use like `List[T]`, has the
   reference shape.

Under (b), enums fail the bound, so they cannot be arguments of such
methods. `downcast` is the most visible case: decision 15 makes it a default
method bounded by `AnyRef`, and error enums (`FsError`, `ToolError`,
`SyncError` throughout the specification) are the most common targets.

```text
enum FsError:
    NotFound(path: string)
    Denied(path: string)

fn missing_path(error: std.error.Error) -> string?:
    match std.inspect.downcast_val[FsError](error):
        .Some(FsError.NotFound(path)) => path
        _ => .None
```

This parses, and under (b) it is the spelling that works. `downcast_val`'s
signature is `[T < Inspectable]` with no `AnyVal` bound. It is already a
general static downcast and needs no change to accept enums. What changes is
ergonomics:

- `error.downcast[FsError]()` is unavailable for enums.
- An `Error` default method `find[T < AnyRef + Inspectable]` cannot find an
  enum error. `std.error.find[T](error)` would have to be a static function,
  which is the form of the superseded decision 13.

Three ways to handle this; [question 4](#4-how-are-enums-recovered-from-erased-values)
asks the owner:

- (i) `downcast_val` recovers enums, gets a category-neutral name, and
  `find` becomes static.
- (ii) Bound G2 by a representation property instead: a third sealed marker
  for every non-scalar type, covering `AnyRef` plus `string`, tuples, and
  enums. The implementation must then keep enums reference-shaped in
  generic code, and a body wanting `is` adds `+ AnyRef`.
- (iii) Accept the gap.

Other dynamically safe traits with `AnyRef`-bounded method generics lose
enum arguments the same way. The specification already asks callers to
"convert a primitive, tuple, or optional value explicitly"
([Trait Values And `Any`](../spec/04-type-system.md#trait-values-and-any)).
Under (b), that sentence would say "enum" instead of "optional".

### `is`, identity, and A3

A3 did two things:

1. It made optionals follow the ordinary enum rules. They erase to `Any` and
   are not special.
2. Because enums were references, it gave `.Some` its own identity.

(b) keeps the first and withdraws the second. `Option` stays an ordinary
enum; all enums change category together. The conformance fixtures
`optional-identity`, `payload-free-variant-canonical`,
`shared-data-variant-identity`, and `variant-identity-through-any` would
become `identity-requires-references` rejections or move to `==` on derived
equality.

Erasing to `Any` also changes. Under (b), `let a: Any = Color.Red` allocates
a box as a primitive does, so two erasures of `Color.Red` are no longer
`is`-equal. [Question 6](#6-erasing-an-anyval-enum-to-any) asks whether
that is acceptable.

### Performance and Wasm GC representation

Under (b), the backend may use:

- **Immutable struct fields.** The prototype declares every enum field
  mutable only for shared-data assignment. Immutable fields allow constant
  globals and let an optimizer skip proving a field is never written.
- **Per-variant subtypes** instead of one flat union struct. Each variant
  stores only its own payload. This is independent of category, but
  immutable fields make it simpler.
- **i31ref or `i32`** for enums whose variants are all payload-free. The
  canonical identity of (a) already allows this, because `ref.eq` on i31ref
  compares values. So this gain alone does not justify (c).
- **Niche optionals**: `.Some(x)` as `x`, as in
  [Generics and `Option[T]` uniformity](#generics-and-optiont-uniformity).
  This removes an allocation from every implicit wrap and every `Map.get`
  hit.
- **Multi-value returns**: `Result[T, E]` and `T?` in locals and results as
  `(tag, payload)` pairs, as tuples already may be. The `?` fast path then
  allocates nothing.
- **Sharing**: literal constructions like `Shape.Circle(1)` or
  `HttpStatus.Ok` become static constants when their arguments are
  constants and their defaults are pure. A default with a side effect, such
  as the `next()` fixture, must still run at each construction; only the
  value may be shared.

None of these is required. `AnyVal` permits them and does not demand inline
layout. That is why hd needs no `indirect` keyword: recursive enums stay
heap records, unlike Swift or Rust, where values are inline by default.

### Evolution

A public enum's category is part of its API. Callers may use `is` on it,
pass it to `AnyRef`-bounded methods, or downcast it with one function or the
other.

- Under (c), adding a first payload variant or shared field to a C-like
  enum flips it from `AnyVal` to `AnyRef`. Removing the last one flips it
  back. Either direction breaks callers.
- Under (d), changing a payload type from `string` to a data type flips it,
  per instantiation.

That is the resilience argument owner decision TQ-24 already accepted for
NonEscapable: a property that callers depend on should not change when a
payload changes. (a) and (b) never flip.

### Mutable payload access

An `AnyVal` enum may still point to mutable objects, as a tuple may. Under
(b), copying or sharing an enum is shallow and never duplicates the objects
its payloads point to. The payload viewpoint from question 1 is a static
rule on the view's type, so it needs no identity:

```text
data User:
    name: string

enum Slot:
    Filled(user: mut User)
    Empty

fn rename(slot: mut Slot) -> void:
    match slot:
        .Filled(user) => user.name = "Grace"
        .Empty => pass
```

This parses. Under (b), `mut Slot` still means "payloads declared `mut U`
bind as `mut U`". A readonly `Slot` must still not be upgraded to `mut Slot`,
even though a copy would be unobservable. Otherwise a readonly view could be
copied into one that exposes `mut User`.

### Recursive enums and GADTs

Recursive enums work under every option, because the backend chooses heap
records anyway. Under (b), subtrees may be shared freely, as in Haskell.

GADTs are unaffected. Refinement is static, and runtime values carry only
tag and payload. A payload-free witness such as a type-equality variant is
naturally `AnyVal`.

The variance rule that makes a parameter invariant when a GADT result
refines it keeps working. Its purpose is to stop a variance conversion from
reinterpreting a runtime representation. A niche-encoded `Option` is still
one reference in the shared body, so readonly covariance stays
representation-preserving.

### Least common type

The category plays no part in [Least Common Type](../spec/04-type-system.md#least-common-type).
It uses only implicit conversions and never falls back to `Any`. No option
changes it.

## Question 4: Recommendation

Adopt (b):

1. Every enum, `Option` and `Result` included, is `AnyVal`. `is` on an
   enum type is rejected, and equality comes only from `Eq`.
2. Enum storage is fully immutable. Named shared constructor data becomes
   read-only. `mut E` remains as the static payload viewpoint, and a
   readonly `E` is never upgraded.
3. The category depends on nothing else. Adding a variant, a payload, or
   shared data never changes it.
4. Implementations may share, intern, unbox, or niche-encode enum values.
   `User?` may be a nullable reference, and `.Some` need not allocate. The
   implementation model says so instead of forbidding it.
5. Enums are recovered from erased values with the static `downcast_val`,
   renamed to a category-neutral name. `std.error.find` is a static function.
   The `AnyRef`-bounded `downcast` methods stay as a convenience for
   references.

Rationale:

- Enum values are already immutable apart from one unused sentence about
  shared data. So identity adds nothing a program can use except `is`, and
  `is` rules out the representations that make `T?` and `Result` cheap.
- Tuples already set the precedent: an immutable structural value that holds
  references is `AnyVal`. An enum is a nominal, tagged tuple.
- The languages that treat sums as values (Rust, Swift, Haskell, Zig) rely
  on this freedom. The languages that made them references are removing
  identity (Java's `Optional`), forbidding it (Kotlin value classes), or
  leaving it unspecified (OCaml).
- (b) keeps A3's main point: `Option` follows the ordinary enum rules.
- (c) and (d) change category when a payload changes. (c) gains nothing
  canonical identity does not already give. (d) makes the category of
  `Option[T]` unknowable in generic code.

What (b) gives up:

- `is` on enums. No program in the specification, guide, or fixtures relies
  on it except the identity fixtures themselves.
- Assignable shared data. No fixture uses it, and the prototype never
  implemented it.
- Enum arguments to `AnyRef`-bounded dynamic methods, `downcast` included.
  Point 5 covers the common case.

If the owner wants a smaller change, the next best choice is (a) with shared
data made read-only. That removes the `HttpStatus.Ok` asymmetry and
per-construction allocation for shared-data variants, and keeps A3 whole.

## Questions For The Owner

### 1. Which value category do enums have?

Options: (a) all `AnyRef`; (b) all `AnyVal`; (c) payload-free `AnyVal`, the
rest `AnyRef`; (d) `AnyVal` when every payload type is `AnyVal`.

**Recommend (b).** Enum storage is immutable, identity is observable only
through `is`, and the category never changes when a payload changes.

```text
data User:
    name: string

fn wrapped_twice(user: User) -> bool:
    let first: User? = user
    let second: User? = user
    first is second
```

Parses. Today it evaluates to `false`. Under (b) it is
`identity-requires-references`; `first == second` needs `User < Eq`.

### 2. Is shared constructor data read-only?

Options: (A) read-only: shared fields are readable through any view and
never assignable; (B) assignable through a `mut E` root, as today.

**Recommend A.** It holds under every option in question 1. It is required
for (b). It removes the only mutation of enum storage, which no fixture
exercises and the prototype never implemented. It also makes
`HttpStatus.Ok` shareable. Effective Java gives the same advice for Java
enums: their fields should be final.

```text
enum HttpStatus(code: i32, phrase: string, retryable: bool = false):
    Ok -> HttpStatus(200, phrase="OK")
    Busy -> HttpStatus(503, phrase="Busy", retryable=true)

fn calm(status: mut HttpStatus) -> void:
    status.retryable = false
```

Parses. Today it is valid. Under A, the assignment is
`invalid-assignment-target`.

### 3. If enums are `AnyVal`, what happens to `is`?

Options: (A) rejected, as for tuples and strings; (B) substitutability:
same variant and pairwise `is`-equal payloads, following Valhalla and
Julia's egal; (C) allowed with an unspecified result, as in OCaml.

**Recommend A.** `is` stays a constant-time identity test, and `==` stays
the only structural comparison. C contradicts hd's preference for rejecting
over leaving results unspecified.

```text
enum Shape:
    Dot
    Circle(radius: i32)

fn same_circle() -> bool:
    Shape.Circle(1) is Shape.Circle(1)
```

Parses. Today it evaluates to `false`. Under A it is rejected; under B it is
`true`.

### 4. How are enums recovered from erased values?

Applies if enums are `AnyVal`. Options:

- (i) the static `downcast_val` recovers enums too, is renamed to a
  category-neutral name, and `std.error.find[T]` becomes a static function;
- (ii) G2 is bounded by a third sealed marker for every non-scalar type, so
  the `downcast` method and `Error.find` accept enums; the implementation
  keeps enums reference-shaped in generic code;
- (iii) keep decision 15 as written, so enums use `downcast_val` and cannot
  use `find`.

**Recommend (i).** It needs no new trait, and `downcast_val` already
accepts any `Inspectable` target. (ii) is the principled fix if G2 keeps
coming up for other traits.

```text
enum FsError:
    NotFound(path: string)

fn is_missing(error: std.error.Error) -> bool:
    match std.inspect.downcast_val[FsError](error):
        .Some(_) => true
        .None => false
```

Parses. Under (b) with (i), this is how an error enum is found. Under (a),
`error.downcast[FsError]()` works as well.

### 5. What does `mut E` mean for an `AnyVal` enum?

Options: (A) keep the payload viewpoint: a non-generic `mut U` payload binds
`mut U` through `mut E` and `U` through `E`, with no upgrade from `E` to
`mut E`; (B) drop the viewpoint, so payloads bind their declared type as
tuple elements do, and ban `mut E`; (C) ban `mut U` in non-generic payloads.

**Recommend A.** It keeps a readonly view's guarantee exactly where it is
today. It is static and needs no identity.

```text
data User:
    name: string

enum Slot:
    Filled(user: mut User)
    Empty

fn peek(slot: Slot) -> string:
    match slot:
        .Filled(user) => user.name
        .Empty => ""
```

Parses. Under A, `user` has type `User` here, and assigning `user.name` is a
`readonly-root` error. Under B, it would have type `mut User`.

### 6. Erasing an `AnyVal` enum to `Any`

Options: (A) a fresh box per conversion, the existing rule for values
without identity; (B) a canonical box per payload-free variant, so erased
`Color.Red` values stay `is`-equal.

**Recommend A.** One rule covers every `AnyVal`. B resembles Java's
`Integer` cache: identity that holds only for some values.

```text
enum Color:
    Red
    Green

fn erased_twice() -> bool:
    let first: Any = Color.Red
    let second: Any = Color.Red
    first is second
```

Parses. Today it is `true`. Under (b) with A it is `false`.

### 7. Do payload-free enums have built-in `Eq` and `Hash`?

This question stands on its own and applies under every option.
[Map Key Types](../spec/04-type-system.md#map-key-types) lists
"payload-free enums" among the types with standard `Hash`. Chapters 05 and 09
say no user enum gets `Eq` or `Hash` without derivation.

Options: (A) delete the phrase from chapter 04, so C-like enums derive like
every other enum; (B) give every all-payload-free enum built-in `Eq` and
`Hash`, and say so in chapters 05 and 09.

**Recommend A.** No automatic conformance is the rule everywhere else, and
`@derive(Eq, Hash)` is one line.

```text
enum Color:
    Red
    Green

fn palette() -> Map[Color, string]:
    {Color.Red: "red"}
```

Parses. Under A this needs `@derive(Eq, Hash)` on `Color`.
Under B it is valid as written.

### 8. Does a mutable view of an enum stay invariant?

Applies if shared data is read-only (question 2). Every `mut G[T]` view is
invariant, because it may replace stored values. A read-only enum stores
nothing replaceable, and the payload viewpoint does not involve `T`.

Options: (A) keep enums under the uniform rule; (B) let `mut E[T]` keep
`E`'s declared variance.

**Recommend A.** The gain is small, and one variance rule for all
composites is easier to teach.

```text
data User:
    name: string

fn widen(value: mut Option[mut User]) -> mut Option[User]:
    value
```

Parses. Under A, the conversion is rejected, as today. Under B, it is
accepted.

## Findings Outside The Main Question

These hold whatever the owner decides about categories:

- **A stale sentence in chapter 04.** The G2 paragraph in
  [Trait Values And `Any`](../spec/04-type-system.md#trait-values-and-any)
  says "a caller converts a primitive, tuple, or optional value explicitly".
  After A3, optionals implement `AnyRef` and need no conversion.
- **The `Hash` contradiction** in question 7.
- **Unnamed shared data.** Chapter 08 gives `._0` access to unnamed shared
  constructor data. It does not say whether `._0` can be assigned when named
  shared fields can.
- **`EnumShape`** ([Shape Intrinsics](../spec/14-annotations.md#shape-intrinsics))
  lists variants and their payload fields but not shared constructor data. A
  derivation that must see `HttpStatus.code` cannot reach it through the
  shape. Relevant to [Typed Derivation](TYPED_DERIVATION.md).
- **The prototype** gives no `mut` access to enum constructions. It
  allocates `.None` per construction and emulates canonical identity only
  in a direct `is`, so a `AnyRef`-bounded generic sees two `.None`
  values as distinct.

## Sources

- Swift: [The Swift Programming Language, Enumerations](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/enumerations/)
  (value semantics, `indirect`); [AnyObject](https://developer.apple.com/documentation/swift/anyobject).
- Rust: [The Rust Reference, type layout](https://doc.rust-lang.org/reference/type-layout.html);
  [`std::option` representation](https://doc.rust-lang.org/std/option/index.html#representation)
  (null-pointer niche).
- Kotlin: [Equality](https://kotlinlang.org/docs/equality.html);
  [KEEP value classes notes](https://github.com/Kotlin/KEEP/blob/master/notes/value-classes.md)
  (`===` unavailable on value classes).
- Java: [JEP 401: Value Classes and Objects](https://openjdk.org/jeps/401)
  (`==` on value objects compares state; migration of value-based classes
  such as `Optional`).
- Scala: [Unified types](https://docs.scala-lang.org/tour/unified-types.html)
  (`AnyVal`, `AnyRef`); [Scala 3 enums](https://docs.scala-lang.org/scala3/reference/enums/enums.html).
- OCaml: [Stdlib, `( == )`](https://ocaml.org/manual/5.1/api/Stdlib.html)
  ("On non-mutable types, the behavior of ( == ) is
  implementation-dependent"); [Memory representation of values](https://ocaml.org/docs/memory-representation).
- F#: [Discriminated unions](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/discriminated-unions)
  (`[<Struct>]` unions).
- MoonBit: [Language tour, enum](https://tour.moonbitlang.com/data-types/enum/index.html);
  [Value types](https://www.moonbitlang.com/blog/moonbit-value-type).
- WebAssembly GC: [GC proposal overview](https://github.com/WebAssembly/gc/blob/main/proposals/gc/Overview.md)
  (`i31ref`, immutable fields, subtyping).
