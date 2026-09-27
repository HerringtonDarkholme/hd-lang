# Runtime Type Identity: `Inspectable`, `RuntimeType`, And `downcast`

Status: design draft for
[Runtime Type Identity And `reified`](OPEN_ISSUES.md#runtime-type-identity-and-reified)
([Roadmap area 2](ROADMAP.md#2-type-checking-rules)). Nothing here is
accepted. It changes no specification text; accepted parts move into
chapters 04, 09, and 14 and into [Standard Library Design](STDLIB.md).

This document turns the decided direction into a concrete proposal: the
trait declaration, the runtime type object, which types are inspectable,
how a value is erased, what `downcast` does, the error-chain helpers, the
representation, and the questions still open for the owner.

Code sketches use `List[T]`, `Map[K, V]`, uppercase generic parameters,
`std.error.Error`, and `std.convert.From`, following the owner's naming
decisions. Every block was parsed with the
[reference parser](../spec/reference-parser/index.ts) on 2026-09-26 unless it
is marked **hypothetical syntax** (the parser rejects it). Short examples
that are statements rather than items were parsed inside a function body. A
block that
parses still needs the rules proposed here to type-check; none of it
type-checks under today's specification, because `Inspectable` does not
exist yet.


## Owner Decisions

Decided 2026-09-26:

1. **Question 11: a trait that extends `Inspectable` may not be a
   requirement key** (`inspectable-requirement`), so a provider view can
   never be downcast to recover more authority.
2. **Question 3: the downcast target spells `mut`** (`e.downcast[mut C]()`),
   checked statically against the receiver's access (`mutable-upgrade` on a
   readonly receiver). The runtime does not track mutability: a type's
   runtime identity ignores `mut` at every level, so `List[mut User]` and
   `List[User]` have the same identity. hd's permission system is a static
   discipline, not a runtime one.
3. **Questions 1 and 2: `Inspectable` and the runtime type object live in
   `std.inspect`, and the type object is named `TypeId`** (`runtime_type`
   becomes the method returning a `TypeId`; rename consistently).
4. **Question 6: erasing a generic value requires `T < Inspectable`;**
   `reified T` alone does not allow erasure. Direction item 11 in Open
   Issues changes accordingly.
5. **Question 4: trait value types and `Any` may appear as type arguments of
   an inspectable type, matched exactly.** An erased `List[FsError]` does not
   downcast to `List[Error]` (different exact type and representation).
6. **Question 5: data types with function-typed fields, and newtypes over
   function types, are inspectable.**
7. **Question 7: `downcast` on a concrete receiver is rejected**
   (`downcast-receiver`).
8. **Question 8: an impossible target is a compile error when written
   concretely and `.None` when it only arises through a generic parameter.**
9. **Question 9: `TypeId::of[T]()` exists.**
10. **Question 12: no type-pattern sugar in `match` for now.**
11. **Question 13: `std.error` ships `root_cause`.**

## Contents

- [Goals](#goals)
- [Decided Constraints](#decided-constraints)
- [Survey](#survey)
- [Proposed Design](#proposed-design)
  - [The trait](#the-trait)
  - [The runtime type object](#the-runtime-type-object)
  - [Which types are inspectable](#which-types-are-inspectable)
  - [Erasure](#erasure)
  - [`downcast`](#downcast)
  - [`Any` and `Inspectable`](#any-and-inspectable)
  - [Error-chain helpers](#error-chain-helpers)
  - [Registries and typed extension maps](#registries-and-typed-extension-maps)
  - [Representation and cost](#representation-and-cost)
  - [Replay and boundaries](#replay-and-boundaries)
  - [Pattern-matching sugar (options only)](#pattern-matching-sugar-options-only)
  - [Diagnostics](#diagnostics)
- [What Stays Out](#what-stays-out)
- [Reference-Parser Finding](#reference-parser-finding)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)

## Goals

1. **Recover a concrete type from an erased value** where the program asked
   for it: error-chain inspection, plugin registries, typed extension maps,
   and runtime codecs ([Typed Derivation Design C](TYPED_DERIVATION.md#design-c-runtime-shape-driven-codecs)).
2. **Keep it visible.** A function can branch on a type only if its
   signature says so, through `Inspectable` or a trait that extends it.
   Everything else stays parametric, and a reviewer can check that from
   signatures alone.
3. **Keep it sound.** No value can claim another type's identity, no
   downcast upgrades `mut`, and no downcast recovers authority that a trait
   view hid.
4. **Keep it cheap.** One descriptor per erased type, a pointer compare per
   test, and nothing for code that never erases to `Inspectable`.
5. **Keep it small.** Equality and a printable name. No reflection API, no
   conformance queries, no calls through a type object.

## Decided Constraints

These come from the
[Runtime Type Identity direction](OPEN_ISSUES.md#runtime-type-identity-and-reified)
(items 1 to 12), the type-audit owner decisions in
[audit/types/QUESTIONS.md](../audit/types/QUESTIONS.md), and the
[Error Conversion owner decisions](ERROR_CONVERSION.md#owner-decisions).
The proposal must satisfy all of them.

| # | Constraint | Source |
| --- | --- | --- |
| C1 | A type test's target needs a runtime descriptor, which comes from `reified`. | Direction 1 |
| C2 | `Inspectable` has a method returning the value's runtime type object. The compiler supplies every implementation; user code cannot write or override one. | Direction 2 |
| C3 | Opt-in is at the use site: erase to `Inspectable` (or a trait extending it) to keep a value recoverable. Erasing to `Any` stays one-way. | Direction 3 |
| C4 | Generic arguments are part of identity: `Box[User]` and `Box[Post]` differ. | Direction 4 |
| C5 | `value.downcast[T]() -> T?` with `T` reified. It is a compiler-provided method, available only on `Inspectable` values and on parameters bounded by `Inspectable`. It is not a trait method, so dynamic safety is unchanged. | Direction 5, TQ-22 |
| C6 | An unbounded erased type parameter never supports a type test. | Direction 6 |
| C7 | No trait-to-trait assertions. | Direction 7 |
| C8 | A downcast preserves permission and never upgrades readonly to `mut`. Its target must be nameable at the call site. | Direction 8 |
| C9 | Closures, function values, suspension frames, local declarations, and `NonEscapable` values are not inspectable. `NonEscapable` itself is parked; this document keeps only the exclusion. | Direction 9, TQ-24 to TQ-26 |
| C10 | The runtime type object supports equality and a printable name only. It never answers whether a type implements a trait. | Direction 10 |
| C11 | Erasing needs the full type at the erasure site. For `x: T`, the descriptor comes from a `T < Inspectable` dictionary or from `reified T`. Per-object storage of generic arguments is an implementation choice. | Direction 11 |
| C12 | The standard error trait extends `Inspectable`. It is `std.error.Error`, a dynamic trait value; `std` ships `.context`, `Context`, `chain`, and a `find[T]` helper built on `downcast`. | Direction 12, Error Conversion decision 3 |
| C13 | `T::f()` is served by the bound's dictionary, never through a runtime type object. | TQ-9 |
| C14 | `TypeShape` gains `Mut`, `Trait`, `Any`, `Suspend`, and `Newtype` cases. | TQ-23 |
| C15 | Assignability is single-step; least-common-type inference never constructs trait values or widens to a supertrait. | TQ-14, TQ-15 |
| C16 | No impl may target a trait value type or `Any`. | TQ-22, 09 Implementation Targets |
| C17 | A dynamic trait value satisfies bounds on its own trait and its supertraits. | Error Conversion decision 4 |
| C18 | Dynamic safety stays literal: only `Reference`-bounded method generics. | TQ-10 |
| C19 | An erased `Error` never crosses a registered boundary. | Error Conversion decision 6 |

## Survey

| Language | How identity is obtained | Who can provide it | Test syntax | Generic arguments | Interface assertions | Parametricity |
| --- | --- | --- | --- | --- | --- | --- |
| Rust | `Any::type_id() -> TypeId`, blanket impl for every `T: 'static` | Compiler, through the blanket impl | `downcast_ref::<T>()`, inherent on `dyn Any` | Part of `TypeId` | None; trait upcasting only since 1.86 | Kept unless `T: Any` is in the bound |
| Rust `dyn Error` | `downcast_ref` on `dyn Error + 'static`; `source()` chain | Compiler; `Error::type_id` was made unoverridable after a soundness bug | `downcast_ref::<T>()` | Part of `TypeId` | None | Kept |
| Go | Every interface value carries its dynamic type | Runtime | `x.(T)`, `switch x.(type)`, `errors.As` walks `Unwrap` | Part of the type | Yes: `x.(io.Writer)` checks the method set | Lost: `any(x).(int)` works on any `T` |
| Swift | Existentials carry type metadata; `type(of:)` | Runtime | `as?`, `as!`, `is`, `catch let e as E` | Part of the type | Yes: `as? Codable` | Lost: `x as? Int` works inside a generic body |
| Kotlin | JVM class of the object | Runtime | `is`, `as?`, smart casts | Erased (`is List<*>` only), except `reified` in inline functions | Yes (`is Interface`) | Lost for classes; `reified` makes `T` testable |
| Java | JVM class | Runtime | `instanceof T t`, switch patterns | Erased; `instanceof List<String>` is illegal | Yes | Lost |
| C# | Reified generics | Runtime | `is T t`, switch type patterns, `typeof(T)` | Part of the type | Yes | Lost |
| Haskell | `Typeable a` gives `TypeRep a` | Compiler only; hand-written instances are rejected since GHC 7.8 | `cast :: (Typeable a, Typeable b) => a -> Maybe b`; `Data.Dynamic` | Part of `TypeRep` | No | Kept: the `Typeable` constraint is in the signature |
| Zig | `@TypeOf` and `@typeName` at compile time only | Compiler | Compile-time `switch` on types; runtime recovery uses tagged unions or hand-rolled type ids | Compile time only | No | Not applicable |

Takeaways for hd:

1. **Haskell's `Typeable` is the closest model.** Compiler-derived, never
   hand-written, visible as a constraint, and `cast` is descriptor equality.
   hd's `T < Inspectable` plays the role of the `Typeable` constraint, and
   dynamic `Inspectable` plays the role of `Dynamic`.
2. **Rust shows why the provider must be sealed.** When `Error::type_id` was
   an overridable trait method, safe code could return another type's
   `TypeId` and transmute through `downcast_ref` (CVE-2019-12083, fixed in
   Rust 1.34.2). C2 closes this in hd from the start.
3. **Go, Swift, Kotlin, Java, and C# all allow interface assertions.** This
   is the capability C7 rules out: it recovers authority that a narrower
   view hid. hd's provider views make this sharper than in those languages.
4. **Go and Swift lose parametricity.** Any generic body can test its
   argument's type. C6 keeps hd on the Haskell and Rust side.
5. **Kotlin's `reified` matches C1.** A type test needs a runtime
   descriptor for its target, and only a reified parameter carries one.
6. **Go's `errors.As` and anyhow's `downcast_ref` walk a cause chain.** This
   is the `find[T]` helper in C12.
7. **Rust returns the original on failure** (`Box<dyn Any>::downcast` gives
   `Result<Box<T>, Box<dyn Any>>`). hd does not need that: the receiver is a
   shared reference that the caller still holds, so `T?` loses nothing.

## Proposed Design

### The trait

`Inspectable` is a sealed trait with one non-generic method:

```text
pub trait Inspectable:
    fn runtime_type(self) -> RuntimeType
```

- **Sealed.** Like `Reference`, `ShapeMetadata`, and `Suspend`, user code
  cannot implement it. An explicit `impl Inspectable for User` is
  `sealed-trait-implementation`. The compiler supplies the implementation
  for every inspectable type ([Which types are inspectable](#which-types-are-inspectable)).
- **Not overridable.** A trait that extends `Inspectable` cannot redeclare
  `runtime_type` or give it a default body; that is
  `sealed-trait-implementation` on the redeclaration. A type's own inherent
  method named `runtime_type` is allowed and wins ordinary method lookup on
  the concrete type, but `downcast` never calls a method: it reads the
  compiler's descriptor directly. So no source-level method can change what
  `downcast` sees.
- **Dynamically safe.** One non-generic method, no associated types or
  functions, `Self` only as the receiver. `Inspectable` is usable as a value
  type, and so is any trait that extends it and is itself dynamically safe.
- **`downcast` is not declared here.** It is a compiler-provided method
  (C5), so the dynamic-safety rule (C18) needs no exception.
- **Location** is an owner question ([question 1](#1-where-do-inspectable-and-runtimetype-live)).
  The recommendation is a `std` module outside the prelude, so that writing
  `Inspectable` needs an import, which matches the "deliberate choice" of C3.
  `downcast` needs no import.

Why a method and not only an intrinsic: `x.runtime_type()` is useful for
logs and registries, and a trait method is the ordinary way a dynamic value
exposes behavior. The descriptor behind it is the same one `downcast` reads.

### The runtime type object

`RuntimeType` is an opaque, compiler-constructed value:

```text
pub data RuntimeType:
    key: TypeKey

impl RuntimeType:
    pub fn of[reified T < Inspectable]() -> RuntimeType:
        runtime_type_of[T]()

impl Display for RuntimeType:
    fn to_string(self) -> string:
        self.key.name
```

Parses. The field and the helper `runtime_type_of` stand for compiler
internals; the public surface is only what the next list names.

**Public surface.**

- `PartialEq` and `Eq`: two runtime types are equal exactly when they denote
  the same type, as defined below.
- `Hash`, so `Map[RuntimeType, V]` works for registries. The hash is a
  deterministic function of the type within one program build
  ([question 10](#10-printable-name-and-hash-stability)).
- `Display`: the printable name.
- `RuntimeType::of[T]()` with `reified T < Inspectable`: the runtime type of
  a statically named type. It exists for comparisons and registry keys
  ([question 9](#9-does-runtimetypeoft-exist)).

Nothing else. There is no `args()`, no `shape()`, no `implements(...)`, no
`name()` returning a structured path, and no way to construct a value from a
`RuntimeType` or call a function through it (C10, C13).

**Identity.** Two runtime types are equal when the types are identical after
expanding transparent aliases, under these rules:

| Case | Same runtime type? |
| --- | --- |
| `UserName` (alias of `string`) and `string` | Yes |
| `Mile` (newtype over `i32`) and `i32` | No |
| `Box[User]` and `Box[Post]` | No (C4) |
| `Box[User]` and `Box[mut User]` | No: inner permission is part of the type, as in TQ-30 |
| `mut User` and `User` | Yes: outer permission is a property of the view, not of the type |
| `i32` and `i64` | No: no numeric widening |
| `User?` and `User` | No |
| `List[Error]` and `List[FsError]` | No: variance is not consulted |
| Two declarations named `User` in different modules | No: identity is the declaration, not the name |
| The same declaration from two versions of one package, if packages ever allow that | No |

**Printable name.** The canonical source spelling of the type, with every
nominal declaration written by its fully qualified path:

- primitives and `string` as written: `i32`, `string`;
- built-in constructors unqualified: `List[acme.model.User]`,
  `Map[string, i32]`;
- `Option[T]` printed as `T?`, tuples as `(A, B)`;
- inner permission shown: `List[mut acme.model.User]`;
- trait value types by their qualified trait name: `List[std.error.Error]`.

The name is for humans, logs, and diagnostics. It is deterministic within a
build but is not a stable identifier across compiler versions or renames,
and nothing parses it back into a type.

**Relation to `TypeShape` and `shape[T]()`.** They answer different
questions and stay separate values:

| | `RuntimeType` | `TypeShape` / `shape[T]()` |
| --- | --- | --- |
| Answers | Is this the same type? | What is this type made of? |
| Obtained from | a value (`x.runtime_type()`) or `RuntimeType::of[T]()` | a static, reified type only |
| Surface | equality, hash, name | structure, declaration identities, metadata |
| Conversion to the other | none | none |

Both come from the same compiler descriptor, so an implementation can share
one object and attach the shape lazily. As a non-normative invariant,
`RuntimeType::of[A]() == RuntimeType::of[B]()` holds exactly when `shape[A]()`
and `shape[B]()` describe the same type once an outer `TypeShape.Mut` is
dropped. The TQ-23 cases fit that picture: `Trait(decl, args)` and `Any`
describe a trait value type used as a generic argument, `Mut(inner)` an inner
permission, and `Newtype(decl, base)` gives a newtype its own identity,
which is exactly what the identity table needs. `Suspend(result)` describes
a type that is never inspectable.

There is deliberately no `RuntimeType.shape()`. Reflection over structure
stays tied to a statically named, reified type, so a value's runtime type
never opens its fields to code that could not name the type.

### Which types are inspectable

A type is **inspectable** when its runtime identity can be described by a
closed descriptor. The compiler supplies `Inspectable` for exactly these
types:

1. primitives (`bool`, `char`, the integer types, `f32`, `f64`) and `string`;
2. module-level data, enum, and newtype declarations, public or private,
   applied to inspectable type arguments (see item 5 for which arguments
   count). `Option[T]` and `Result[T, E]` are ordinary enums, so `T?` is
   inspectable when `T` is, and `Result[T, E]` when both are;
3. `List[T]` and `Map[K, V]` with inspectable arguments;
4. tuples of inspectable elements;
5. as a **type argument only**, also `void`, a trait value type, and `Any`,
   so `List[std.error.Error]`, `Result[void, E]`, and
   `Map[string, Any]` are inspectable
   ([question 4](#4-trait-value-types-and-any-as-type-arguments)).

A dynamic value of a trait that extends `Inspectable` (such as `Error`, or a
user `Plugin < Inspectable`) satisfies `Inspectable` by C17, and its
`runtime_type()` is its **concrete** type, never the trait. Widening such a
value to `Inspectable` is ordinary supertrait widening (assignability rule 7).

Not inspectable, as a value or as an argument:

| Type | Why |
| --- | --- |
| Function types, closures, and function values, whatever their row | C9. A function type's identity would include its requirement row, and rows have no runtime descriptor. |
| `Suspend[T]` and every suspension frame | C9 |
| Local data, enum, newtype, and trait declarations | C9. They cannot be named outside their body, and in a generic body they may depend on erased parameters. |
| `NonEscapable` types, when that category lands | C9; parked, exclusion only |
| A dynamic trait value whose trait does not extend `Inspectable`, as a value | C3 and C7. `Display` and provider views such as `FsRead` never reveal their concrete type. |
| `Any` as a value | C3. `Any` stays one-way. |
| `never` | It has no values. |
| A type applied to a row argument, if data types ever take row parameters | Rows have no runtime descriptor. |
| A type with a non-inspectable argument, such as `Box[fn() -> i32]` or `List[Suspend[i32]]` | Its identity would contain the excluded type (C4). |

A nominal type with a function-typed **field**, such as
`data Handler: run: fn() -> void $ Clock`, is inspectable: its identity is
the declaration, not its fields, and calling `handler.run()` after recovery
is checked against the field's declared row like any other call
([question 5](#5-nominal-types-that-hold-functions)). The same holds for a
newtype over a function type.

Private types are inspectable. A private error in a chain must still report
its runtime type and printable name; what C8 prevents is naming it as a
downcast target outside its module.

### Erasure

There is no new syntax and no `as` operator. A value is erased by an
expected type, exactly as for other trait values:

```text
data User:
    name: string

data Box[T]:
    value: T

type IntBox = Box[i32]

fn keep(value: Inspectable) -> Inspectable:
    value

fn erase_generic[T < Inspectable](value: T) -> Inspectable:
    value

fn demo() -> void $ Console:
    user := User { name: "Ada" }
    let erased: Inspectable = user
    match erased.downcast[User]():
        .Some(found) => println(found.name)
        .None => println("not a user")
    let boxed: Inspectable = Box { value: 1 }
    println(boxed.runtime_type().to_string())
    same := boxed.runtime_type() == RuntimeType::of[IntBox]()
    missing := boxed.downcast[Box[i64]]()
```

Parses. `boxed` prints `acme.demo.Box[i32]`, `same` is `true`, and `missing`
is `.None`.

Rules:

1. **Assignability rule 6 gains one clause.** Today it constructs a trait
   value when `S` "explicitly implements" the trait. `Inspectable` is never
   explicitly implemented, so the rule reads "explicitly implements, or `S`
   is inspectable and the trait is `Inspectable`". A trait extending
   `Inspectable` still needs its own explicit impl (`impl Error for
   FsError`); the `Inspectable` part comes for free.
2. **The recorded runtime type is the static type at the erasure site,**
   outer `mut` removed. It is not the allocation's original type. A
   `mut List[mut User]` viewed as readonly `List[User]` through variance and
   then erased records `List[User]`, so `downcast[List[mut User]]` returns
   `.None`. This is what makes C11's "implementation choice" unobservable:
   per-object argument storage, if an implementation keeps it, is never
   consulted by `downcast`.
3. **Erasing a generic `x: T` requires `T < Inspectable`.** The bound both
   proves `T` is inspectable and carries the descriptor in its dictionary.
   `reified T` alone does not suffice, because it does not prove that `T`
   is not a closure; with the bound present, `reified` adds nothing for
   erasure. An unbounded `fn f[T](x: T)` cannot erase `x` to `Inspectable`
   (C6). This refines C11's "or from `reified T`" wording, which the owner
   may want to keep as an alternative
   ([question 6](#6-does-reified-t-alone-allow-erasure)).
4. **Erasing a value built from `T`**, such as `Box[T]` inside
   `fn wrap[T < Inspectable](x: T) -> Inspectable`, is allowed: the
   compiler builds the descriptor for `Box[T]` from `Box`'s constructor and
   `T`'s dictionary.
5. **Single-step assignability (C15) applies.** `let i: Inspectable =
   mutable_user` is weakening plus construction, two steps, and is rejected;
   bind a readonly view first or erase to `mut Inspectable`. Likewise a
   `User` does not convert to `Inspectable?` in one step. A generic body
   erasing `x: T` is one step from the body's point of view even when `T`
   is instantiated as `mut User`.
6. **`mut Inspectable`** keeps mutable access to an erased composite root,
   as `mut Trait` and `mut Any` do today. Erasing to it requires a `mut`
   source.
7. **Erasing a dynamic `Inspectable` to `Inspectable`** (for example
   `x: T` with `T = Inspectable` under C17) does not wrap twice; the value
   keeps its concrete runtime type.
8. **Least-common-type inference never erases** (C15): `[user, post]` needs
   an expected `List[Inspectable]`.

### `downcast`

**Form.** `receiver.downcast[T]()` returns `T?`. It is a compiler-provided
method (TQ-22), written with ordinary method-call syntax, with no value
arguments and exactly one explicit type argument.

**Receivers.** It is available only on:

- a dynamic `Inspectable` or `mut Inspectable` value;
- a dynamic value of a trait that extends `Inspectable`, such as
  `std.error.Error` or a user `Plugin < Inspectable`;
- a parameter or binding of type `U` where `U < Inspectable` or
  `U < mut Inspectable`, including `U` instantiated with a dynamic trait
  value type under C17.

On any other receiver it is `downcast-receiver`. That includes a concrete,
statically known receiver such as `user.downcast[User]()`, whose answer the
compiler already knows ([question 7](#7-downcast-on-a-concrete-receiver)),
and a value of `Any` or of a trait that does not extend `Inspectable` (C3,
C7). A method named `downcast` that a user declares is found first by
ordinary lookup on types that have it; the compiler-provided method applies
only to the receivers above.

**Target.** `T` must carry a descriptor (C1): a concrete type written at the
call site, or a `reified` type parameter.

- A concrete target must be nameable at the call site under ordinary
  visibility (C8). A private type of another module cannot be written, so it
  cannot be recovered there. A generic helper such as `find[reified T]`
  recovers only the types its callers can name.
- A concrete target that can never match is rejected as `downcast-target`:
  a function type, `Suspend[T]`, `Any`, a trait value type such as
  `downcast[Display]()` or `downcast[Error]()`, or any other
  non-inspectable type. A trait value target would be the trait assertion
  C7 rules out, and `Error` as a target never matches because the recorded
  runtime type is always concrete.
- A reified parameter instantiated with such a type produces `.None` at run
  time; the generic body is checked once, not per instantiation
  ([question 8](#8-mut-and-trait-value-targets-in-generic-code)).

**Matching.** The result is `.Some` exactly when the receiver's recorded
runtime type equals the target's runtime type ([identity table](#the-runtime-type-object)).
No variance, numeric widening, optional unwrapping, newtype unwrapping, or
supertrait search takes place. An erased `User?` downcasts to `User?`
(giving a `User??`), never to `User`.

**Result value.** For a reference value, the recovered value is the same
reference: `found is original` holds. For a value without identity, the box
created at erasure is unwrapped, and the result is the scalar, string, or
tuple.

**Permission** ([question 3](#3-how-does-downcast-express-mut)). The
target spells the result's permission:

```text
data Counter:
    count: mut i32

fn bump(value: mut Inspectable) -> void:
    match value.downcast[mut Counter]():
        .Some(counter) => counter.count = counter.count + 1
        .None => pass

fn look(value: Inspectable) -> i32:
    match value.downcast[Counter]():
        .Some(counter) => counter.count
        .None => 0
```

Parses.

- On a `mut` receiver, `downcast[mut Counter]` gives `(mut Counter)?` and
  `downcast[Counter]` gives a readonly view.
- On a readonly receiver, `downcast[mut Counter]` is `mutable-upgrade`.
- Inner permission is part of identity: `downcast[List[mut User]]` matches
  only a value erased as `List[mut User]`. On a readonly receiver the result
  is a readonly outer list with `mut` elements. That is the same access the
  caller had before erasure, because a readonly view is not deep
  ([chapter 04](../spec/04-type-system.md#composite-values-and-access-permission)).
  No permission is gained.

**No trait-to-trait assertions.** `downcast` compares one descriptor with
another. Nothing asks whether the value implements a trait, and
`RuntimeType` has no query for it (C7, C10). A `FsRead` provider view whose
concrete type also implements `FsWrite` stays a `FsRead` view: `FsRead` does
not extend `Inspectable`, so `downcast` is not even available on it. A
requirement trait that did extend `Inspectable` would let
`$.use(FsRead).downcast[LocalFs]()` recover write authority;
[question 11](#11-may-a-requirement-trait-extend-inspectable) proposes to
forbid that.

**Parametricity.** A function can branch on a value's type only when one of
its parameter types is `Inspectable`, a trait extending it, or a parameter
bounded by one of those. A reviewer can see type-dependent behavior in the
signature, as with Haskell's `Typeable` constraint.

### `Any` and `Inspectable`

- `Any` stays the universal, parametric erasure. Every value type satisfies
  it, erasure costs no descriptor, and it can never be undone.
- `Inspectable` is the recoverable erasure. Fewer types satisfy it (closures
  and frames do not), and erasure records a descriptor.
- An `Inspectable` value may be erased further to `Any`, like any value.
  Nothing converts back: `Any` is not inspectable, so
  `let back: Inspectable = any_value` is `unsatisfied-trait-bound` (or
  `not-inspectable`, see [Diagnostics](#diagnostics)).
- `Inspectable` does not declare `Any` as a supertrait; it does not need to.
- `T < Any` and `T < mut Any` keep their meaning. Code that only stores or
  passes values should keep using `Any` or an unbounded `T`, so its
  signature promises not to inspect.

### Error-chain helpers

`std.error` ships the decided surface (C12). With `Inspectable` in place:

```text
pub trait Error < Display + Inspectable:
    fn cause(self) -> Error?:
        .None

pub fn chain(error: Error) -> List[Error]:
    let found: mut List[Error] = [error]
    let current: Error? = error.cause()
    while true:
        match current:
            .Some(next) =>
                found.append(next)
                current = next.cause()
            .None => break
    found

pub fn find[reified T](error: Error) -> T?:
    for part in chain(error):
        match part.downcast[T]():
            .Some(found) => return .Some(found)
            .None => pass
    .None

pub fn root_cause(error: Error) -> Error:
    let current: Error = error
    while true:
        match current.cause():
            .Some(next) => current = next
            .None => break
    current

fn report(error: Error) -> string:
    match find[FsError](error):
        .Some(FsError.NotFound(path)) => "missing ${path}"
        _ => root_cause(error).to_string()
```

Parses.

- `find[T]` is Go's `errors.As` and anyhow's chain-walking `downcast_ref`.
  It needs `reified T` for the target (C1) and no bound: a
  non-inspectable `T` simply never matches.
- `root_cause` is proposed alongside; it is a three-line helper that most
  error libraries grow.
- Go's `errors.Is` (compare against a sentinel value) is `find[T]` followed
  by `==`, so it needs no separate helper.
- A later iterator-returning `chain` changes nothing here.

Application code, with the decided module names:

```text
use std.error.{Error, chain, find}
use std.fs.{FsError, FsRead}
use std.http.{Http, HttpError}

pub fn report!() -> Result[string, Error] $ FsRead + Http:
    files, http := $.use(FsRead, Http)
    url := files.read_text!(Path::parse("endpoint.txt")).context("reading endpoint")?
    response := http.send!(Request::get(url))?
    Ok("status ${response.status}")

pub fn main!() -> Result[void, Error] $ FsRead + Http + Console:
    match report!():
        Ok(line) => println(line)
        Err(error) =>
            match find[FsError](error):
                .Some(FsError.NotFound(path)) => println("missing ${path}")
                _ => println(error.to_string())
            return Err(error)
    Ok()
```

Parses. Each `?` constructs a dynamic `Error` from a domain error (rule 6),
or converts through `std.convert.From` when the target is a domain enum
(Error Conversion decision 2). Each domain error that implements `Error`
becomes inspectable through the supertrait without extra code.

### Registries and typed extension maps

```text
pub data TypeMap:
    entries: mut Map[RuntimeType, Inspectable]

impl TypeMap:
    pub fn insert[T < Inspectable](mut self, value: T) -> void:
        let erased: Inspectable = value
        self.entries[erased.runtime_type()] = erased

    pub fn get[reified T < Inspectable](self) -> T?:
        self.entries.get(RuntimeType::of[T]())?.downcast[T]()

pub trait Plugin < Inspectable:
    fn name(self) -> string

pub fn find_plugin[reified P < Plugin](plugins: List[Plugin]) -> P?:
    for plugin in plugins:
        match plugin.downcast[P]():
            .Some(found) => return .Some(found)
            .None => pass
    .None
```

Parses. `insert` needs no `reified`: the bound's dictionary supplies the
descriptor. `get` needs `reified T` twice over, for `RuntimeType::of[T]()`
and for the downcast target. Assigning through `self.entries[...]` stands
for the future `Map` insertion method.

### Representation and cost

Non-normative; one reference strategy that fits
[chapter 04's Implementation Model](../spec/04-type-system.md#implementation-model-non-normative).

- **Descriptor.** One record per closed type: a declaration identity, the
  argument descriptors, and a lazily built printable name. Declaration
  identities are globally unique per program (package identity plus
  declaration path), the same `DeclarationId` shapes use.
- **Interning.** Descriptors are hash-consed in one table per program
  instance, so `RuntimeType` equality and `downcast` are a pointer compare
  (Wasm GC `ref.eq`). Descriptors for concrete types are constants emitted
  by the package that declares the outermost constructor or first needs the
  instantiation, and deduplicated at link time or on first use.
- **Generic instantiations in generic code.** Erasing `Box[T]` under
  `T < Inspectable` looks up `(Box, [T's descriptor])` in the interning
  table: one hash lookup per erasure of a generic-dependent type. Concrete
  instantiations cost nothing at run time. Implementations may cache the
  lookup per call site.
- **Dictionary.** The `Inspectable` dictionary for `T` is one word, the
  descriptor. A dictionary for a trait extending `Inspectable` carries the
  descriptor at a fixed slot.
- **Dynamic value.** A dynamic `Inspectable` value is the existing pair of
  reference and method table; the table's descriptor slot is the recorded
  runtime type. For `Error` and other subtraits, the descriptor sits at the
  same slot, so `downcast` on an `Error` value is a load and a compare.
  Supertrait widening already rewraps the table, which now also copies the
  descriptor slot.
- **Scalars and identity-free values** are boxed on erasure, as for `Any`
  today, and unboxed by a successful `downcast`.
- **`downcast` lowering.** Load the descriptor, compare with the target's
  descriptor, then (in Wasm GC) `ref.cast` the payload to the erased struct
  type of the target or unbox it. The descriptor check comes first because
  Wasm types do not carry hd generic arguments.
- **No cost when unused.** Descriptors are emitted only for types that reach
  an erasure to an inspectable trait, a `RuntimeType::of`, or a downcast
  target. Programs that never mention `Inspectable` pay nothing.
- **Reified parameters.** The descriptor a reified call passes (C1) and the
  runtime type descriptor can be one object; `shape[T]()` then hangs off it
  lazily.

### Replay and boundaries

- **Deterministic.** `runtime_type()`, `RuntimeType` equality, hashing, the
  printable name, and `downcast` are pure functions of the program build and
  the value. They touch no host provider, so replayed code may use them
  freely and gets the same answers on every replay of the same build.
- **Not boundary-safe.** A dynamic `Inspectable` value is a dynamic trait
  value and already fails the boundary-safe list in
  [chapter 10](../spec/10-modules.md). `RuntimeType` is also not
  boundary-safe: decoding a type from wire data would need a global type
  registry and would let a remote caller pick which type the receiver
  materializes (the Java deserialization problem). An erased `Error` never
  crosses a boundary (C19).
- **What crosses instead.** Code at a boundary converts to a boundary-safe
  value, for example the `ErrorReport` sketch in
  [Error Conversion](ERROR_CONVERSION.md). It may include
  `error.runtime_type().to_string()` as a diagnostic string, with the
  understanding that the name changes when the type is renamed.
- **Durable histories** never store `Inspectable` values or `RuntimeType`
  values. So a program upgrade between recording and replay cannot find a
  stale type identity in a history.
- **Annotations.** A runtime type cannot fetch an annotation;
  `Facet::annotation(T)` keeps requiring a static target, consistent with
  C13.

### Pattern-matching sugar (options only)

Nothing is proposed as decided. The options, from least to most language
change:

**Option P1: no sugar.** `match x.downcast[T]()` with `.Some(v)` and `.None`,
as in every example above. Works today, and each test is an explicit call.

**Option P2: nominal patterns on an inspectable subject imply a downcast.**
When the subject is `Inspectable` or a trait extending it, a qualified
variant pattern or a data pattern tests the runtime type first:

```text
fn describe(error: Error) -> string:
    match error:
        FsError.NotFound(path) => "missing ${path}"
        HttpError.Status(code) if code >= 500 => "server ${code}"
        _ => error.to_string()
```

Parses; the typing is new. The arm matches when the subject's runtime type
is the pattern's enum or data type and the payload pattern matches. Such a
match always needs a catch-all, and the `.Variant` shorthand is not
available because the subject fixes no enum type. Generic instantiations
cannot be written in pattern position, so `Box[i32] { value }` has no
spelling; a non-generic target is the common case for errors. The cost is
that a downcast happens without the word `downcast` appearing, which weakens
goal 2 slightly.

**Option P3: a typed binding pattern.**

```text
fn describe(error: Error) -> string:
    match error:
        e: FsError => "fs ${e}"
        _ => error.to_string()
```

**Hypothetical syntax**: the parser rejects `e: FsError =>` today
(`missing-let`). It names generic targets naturally (`b: Box[i32]`) but adds a
pattern form used nowhere else.

**Option P4: an `if let` form**, for example
`if let .Some(fs) = error.downcast[FsError]():`. **Hypothetical syntax**:
the parser rejects it (`syntax-error`). This is general optional sugar, not
a type test, and belongs to its own discussion.

Recommendation: P1 now; revisit P2 once real error-handling code shows the
`find` plus `match` form is too verbose
([question 12](#12-pattern-matching-sugar)).

### Diagnostics

Proposed codes; existing codes are reused where they fit.

| Situation | Code |
| --- | --- |
| `impl Inspectable for X`, or a subtrait redeclaring `runtime_type` | `sealed-trait-implementation` (existing) |
| Erasing a non-inspectable type to `Inspectable` or a subtrait (closure, `Any`, `Display` value, local type, frame) | `not-inspectable` (new) |
| Erasing `x: T` without `T < Inspectable` | `unsatisfied-trait-bound` (existing) |
| `downcast` on a receiver that is not an inspectable trait value or an `Inspectable`-bounded parameter | `downcast-receiver` (new) |
| Concrete `downcast` target that can never match: trait value type, `Any`, function type, frame, other non-inspectable type | `downcast-target` (new) |
| `downcast` target that is an erased, non-reified type parameter | `identity-needs-reified` (new; today's text says "an erased parameter must not be used where runtime type identity is required") |
| `downcast[mut T]` on a readonly receiver | `mutable-upgrade` (existing) |
| Target not nameable at the call site | existing name-resolution and `private-member` rules |
| A trait extending `Inspectable` used as a requirement key (if question 11 is accepted) | `inspectable-requirement` (new) |

## What Stays Out

- Trait-to-trait assertions and conformance queries (C7, C10).
- Calling associated functions through a runtime type (C13).
- Structural decomposition of a `RuntimeType` (arguments, fields, name
  parts). Use `shape[T]()` with a static, reified type.
- Downcasting `Any` or trait values whose trait does not extend
  `Inspectable`.
- Type tests on unbounded type parameters (C6).
- Any blanket or user-written `Inspectable` implementation.
- Boundary encoding of `RuntimeType` or of erased values.
- `NonEscapable` design (parked); only its exclusion is recorded.

## Reference-Parser Finding

While parsing the examples, `RuntimeType::of[Box[i32]]()` was rejected with
`deferred-method-value`. Chapter 02 allows method-level type arguments after
`::name` ([Grammar](../spec/02-grammar.md)), and `RuntimeType::of[User]()`
is accepted. The contextual check in `spec/reference-parser/contextual.ts`
matches `::name[...]` with a bracket pattern that does not allow nested
brackets, so a nested generic argument looks like an uncalled member
reference. The example above uses a transparent alias (`IntBox`) instead.
This is a reference-parser false positive, not a design issue.

## Questions For The Owner

### 1. Where do `Inspectable` and `RuntimeType` live?

- **A.** Prelude names (`std.core`), next to `Any` and `Reference`.
- **B.** A `std` module outside the prelude, for example
  `std.inspect.{Inspectable, RuntimeType}`, the way `Suspend` lives in
  `std.task`. `downcast` needs no import either way.
- **C.** Inside `std.error`, since errors are the main user.

**Recommendation: B.** An import makes opting in visible (C3) and keeps two
common words out of the prelude. `std.error` imports it for its supertrait,
and C is too narrow for plugin registries and type maps.

```text
use std.inspect.{Inspectable, RuntimeType}
```

### 2. What are the runtime type object and its method called?

- **A.** `RuntimeType` and `runtime_type()`.
- **B.** `TypeId` and `type_id()`, as in Rust.
- **C.** `Type` and `type_of()`.

**Recommendation: A.** `TypeId` suggests an integer and hides the printable
name. `Type` collides with common user names. `type` is a keyword, so
`self.type()` is not an option.

```text
println(error.runtime_type().to_string())
```

### 3. How does `downcast` express `mut`?

- **A.** The target spells it: `downcast[mut Counter]()` on a `mut`
  receiver; readonly receivers reject `mut` targets with `mutable-upgrade`.
- **B.** The result follows the receiver: `downcast[Counter]()` on
  `mut Inspectable` returns `(mut Counter)?`.

**Recommendation: A.** The result type is exactly `T?`, which keeps the
signature honest, and a readonly view from a `mut` receiver needs no extra
step. B makes the result type depend on the receiver, unlike every other
method.

```text
counter := value.downcast[mut Counter]()   # value: mut Inspectable
```

### 4. Trait value types and `Any` as type arguments

Is `List[Error]` (or `Map[string, Any]`) inspectable?

- **A.** Yes. The trait value type is part of the recorded identity, so
  `downcast[List[Error]]` matches only a value erased as `List[Error]`. It
  never inspects the elements.
- **B.** No. Only types built from concrete types are inspectable.

**Recommendation: A.** It is exact matching of what was erased, recovers no
hidden authority, and `Result[T, Error]` is common enough that B would
exclude most application results.

```text
let erased: Inspectable = errors          # errors: List[Error]
back := erased.downcast[List[Error]]()    # .Some(errors)
```

### 5. Nominal types that hold functions

Is `data Handler: run: fn() -> void $ Clock` inspectable (and a newtype over
a function type)?

- **A.** Yes: identity is the declaration, and calls after recovery are
  checked against the declared field type.
- **B.** No: exclude any type that transitively contains a function or
  frame.

**Recommendation: A.** The C9 reason applies to function **types** as
identities, whose rows have no descriptor, not to declarations that
happen to store a callable. B would also make inspectability change when a
private field changes.

```text
match event.downcast[Handler]():
    .Some(handler) => handler.run()    # requires Clock, as the field declares
    .None => pass
```

### 6. Does `reified T` alone allow erasure?

Direction 11 says the descriptor may come "from a `T: Inspectable`
dictionary or from `reified T`".

- **A.** Erasure always needs `T < Inspectable`; `reified` is for targets
  and `RuntimeType::of`.
- **B.** `reified T` alone also allows erasure, with a check at each
  instantiation that `T` is inspectable.

**Recommendation: A.** Without the bound the body cannot know `T` is not a
closure, and B would move a type error from the generic declaration to its
instantiations.

```text
fn wrap[T < Inspectable](value: T) -> Inspectable:
    value
```

### 7. `downcast` on a concrete receiver

- **A.** Reject `user.downcast[User]()` with `downcast-receiver`; the answer
  is known statically.
- **B.** Allow it and fold it to a constant.

**Recommendation: A.** TQ-22 names `Inspectable` values and bounded
parameters. A static downcast is always a mistake or dead code.

```text
user.downcast[User]()          # downcast-receiver
```

### 8. `mut` and trait value targets in generic code

In `find[reified T](error: Error) -> T?`, a caller may pass
`T = mut FsError` (readonly receiver) or `T = Display` (trait value type).
The body is checked once.

- **A.** The runtime check also compares outer permission: a `mut` target on
  a readonly receiver, or a trait value target, yields `.None`. Written
  concretely, both are compile errors.
- **B.** Reject such instantiations at the call site
  (`find[mut FsError](error)` is an error), as a post-instantiation check.
- **C.** Add a bound that excludes `mut` and trait value types from `T`.

**Recommendation: A.** It is sound, needs no new bound, and keeps generic
bodies checked once. B puts a body-dependent error at the call site; C adds
bound vocabulary for one operation.

```text
missing := find[mut FsError](error)    # always .None under A
```

### 9. Does `RuntimeType::of[T]()` exist?

- **A.** Yes, with `reified T < Inspectable`, for registry keys and
  comparisons.
- **B.** No; a runtime type comes only from a value.

**Recommendation: A.** Without it a typed map cannot look up by type before
it has a value (see `TypeMap.get`). It calls nothing on `T`, so C13 is
unaffected.

```text
key := RuntimeType::of[User]()
```

### 10. Printable name and hash stability

- **A.** The name is the qualified canonical spelling and the hash derives
  from the descriptor. Both are deterministic within one build and carry no
  cross-version promise.
- **B.** Promise stable names and hashes across builds, so they can be
  persisted.
- **C.** Leave the name unspecified (implementation text).

**Recommendation: A.** Determinism is enough for replay. Stability across
builds would make renames breaking changes and invite persisting type
identities, which the boundary rules reject. C makes test output
non-portable.

```text
println(RuntimeType::of[User]().to_string())   # acme.model.User
```

### 11. May a requirement trait extend `Inspectable`?

A provider view such as `FsRead` attenuates a concrete provider that may
also implement `FsWrite`. If a requirement trait extended `Inspectable`,
`$.use(FsRead).downcast[LocalFs]()` would recover the concrete provider and
its write authority, the same leak C7 rules out.

- **A.** A trait that extends `Inspectable` may not be a requirement key
  (`inspectable-requirement`); `std` effect traits never extend it.
- **B.** Allow it; it is the trait author's choice.
- **C.** Only document that `std` effect traits do not extend it.

**Recommendation: A.** Authority attenuation is a language guarantee, not a
library convention, and the rule is checkable at the row.

```text
trait Storage < Inspectable:     # fine as a value trait
    fn get(self, key: string) -> string?

fn load() -> void $ Storage:     # inspectable-requirement under A
    pass
```

### 12. Pattern-matching sugar

- **P1.** None; `match x.downcast[T]()`.
- **P2.** Qualified variant and data patterns on an inspectable subject
  imply a downcast (parses today).
- **P3.** Typed binding pattern `e: FsError =>` (new syntax).

**Recommendation: P1 now, P2 later** if error-handling code proves verbose.
P2 needs no new syntax but hides the downcast; P3 is new pattern syntax for
one feature.

```text
match error:
    FsError.NotFound(path) => "missing ${path}"    # P2
    _ => error.to_string()
```

### 13. Does `std.error` ship `root_cause`?

- **A.** Yes, with `chain` and `find`.
- **B.** No; `chain(error)` and taking the last element suffices.

**Recommendation: A.** It is small, common in error libraries, and reads
better than indexing a list.

```text
println(root_cause(error).to_string())
```

## Sources

- Rust: `std::any` (`Any`, `TypeId`, `type_name`, `downcast_ref`),
  `std::error::Error` (`source`, `downcast_ref`),
  CVE-2019-12083 (overridable `Error::type_id`), trait upcasting stabilized
  in Rust 1.86.
- Go: the specification's type assertions and type switches; `errors.Is`,
  `errors.As`, and `Unwrap` (multi-error `Unwrap() []error` since Go 1.20).
- Swift: type casting (`is`, `as?`, `as!`), existential `any P`, typed
  throws (SE-0413).
- Kotlin: `is`, smart casts, and `reified` type parameters of inline
  functions.
- Java: `instanceof` pattern matching (JEP 394) and switch patterns
  (JEP 441); generic type erasure.
- C#: reified generics, `is` type patterns, `typeof(T)`.
- Haskell: `Data.Typeable` (`cast`, `TypeRep`), `Data.Dynamic`, GHC's
  rejection of hand-written `Typeable` instances (GHC 7.8), type-indexed
  `TypeRep` (GHC 8.2).
- Zig: `@TypeOf`, `@typeName`, `anytype`.
