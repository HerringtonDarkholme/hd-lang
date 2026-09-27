# Nominal Function Types: Survey And Design Options

Status: design exploration with owner decisions 1-10 (2026-09-27); nothing
is applied to the specification yet. Remaining questions are at the end.

The owner's sketch is to make function types an ordinary generic type
constructor, `Fn[Is..., O, Rs...]`, so that the rules for nominal types
(implementation targets, ownership, overlap, variance, runtime identity)
apply to functions as they already apply to `Option`, `List`, and tuples.
This document works out that idea, its alternatives, and the related
question of how tool adapters obtain per-declaration data about a function.

Spelling follows the current naming decisions: `List[T]` and `Map[K, V]`,
uppercase generic parameters (including row parameters), `AnyRef` and
`AnyVal`, and contextual `.Ok`, `.Err`, `.Some`, and `.None`. Every code block
was parsed with the [reference parser](../spec/reference-parser/index.ts) on
2026-09-27 unless its first line says **Hypothetical syntax**. A block that
parses still needs the rules proposed here to type-check.

## Owner Decisions

Decided 2026-09-27:

1. **Q1: `Fn[(Is...), O, R]`.** The inputs are one tuple-kinded argument,
   the output one type, and the requirement row one row-kinded argument.
   One constructor covers every arity; one impl over `Fn[(Is...), O, R]`
   covers all functions.
2. **Q2: no mutation capability on function types.** A closure may mutate
   its captures freely (as in Swift, Kotlin, and Go); `mut fn`, the
   `mutable-capture-requires-mut-fn` rule, and the plain closure's
   readonly view of its captures are removed, and `mut` on a function type
   is an error (a function value has no fields to take a permission on).
   Rust's `FnMut` exists for unique borrows, which hd does not have. The
   standard constructors are therefore `Fn` and `SuspendFn` only, and
   question 6 disappears.
3. **Q3: the sugar is exact.** `fn(A) -> O $ R` and `Fn[(A,), O, R]` are the
   same type, either spelling is valid anywhere, and diagnostics print the
   sugar, as for `T?` and `Option[T]`.
4. **Q4: varargs stay in function types, through a marker element** of the
   input tuple, for example `Fn[(string, Rest[i32]), i32, $()]` for
   `fn(string, i32...) -> i32`; `Rest[T]` is valid only as the final element
   of a function type's input tuple.

5. **Q5: declared variance.** `Fn` and `SuspendFn` are contravariant in
   each input element, covariant in the output, and invariant in the row;
   04's special function-variance paragraph is deleted.
6. **Q7: function types are implementation targets** under the ordinary
   ownership and overlap rules; `function-impl-target` is removed. Row
   positions in impl heads are a row parameter or a concrete row.
7. **Q8: function values stay out of `Inspectable`,** as values and as type
   arguments; the exclusion list names `Fn` and `SuspendFn`.
8. **Q12: `Fn`, `SuspendFn`, and `Rest[T]` live in `std.function`.** The
   sugar needs no import; the spelled forms are imported where written.

Requirement rows (raised the same day) are comma lists with no `+` or `-`,
so a row type argument is written `$(Db, Cache)`; applied to spec 02 and
11 on 2026-09-27 (commits e176f5d to 9541c58). Question 6 disappeared with decision 2.

9. **Q11: function values are `AnyRef` with unspecified identity, and
   cannot be compared** (revised 2026-09-27, following Scala, where
   functions are `AnyRef` `FunctionN` objects and each eta-expansion or
   lambda evaluation may or may not share an object). The identity of every
   function value (named function, generic instantiation, one-payload
   variant constructor, closure) is unspecified; the compiler may share or
   allocate. A direct `is` on an expression whose static type is a function
   type is an error (as is `==`, since function types have no `Eq`);
   generic code over `T < AnyRef` may still compare, with an unspecified
   result. Code that needs to remove a callback keeps a handle returned at
   registration, or scopes the registration (see the example in question
   11). This replaces the earlier "canonical for non-generic functions"
   answer the same day.
10. **Q9 and Q10 (per-declaration data for tools, item types) are parked
    with [typed derivation](TYPED_DERIVATION.md),** which the owner
    deferred; tools register functions by hand for now.

## Contents

1. [Problem](#problem)
2. [What hd Has Today](#what-hd-has-today)
3. [Survey](#survey)
4. [Why Not Two Packs](#why-not-two-packs)
5. [Design 1: A Family Of Standard Constructors](#design-1-a-family-of-standard-constructors)
6. [Design 2: One Constructor With A Mode Argument](#design-2-one-constructor-with-a-mode-argument)
7. [Design 3: Callable Traits](#design-3-callable-traits)
8. [Variance](#variance)
9. [What Becomes Possible: Implementations Over Functions](#what-becomes-possible-implementations-over-functions)
10. [Runtime Identity And `Inspectable`](#runtime-identity-and-inspectable)
11. [Closures, Captured State, And Identity](#closures-captured-state-and-identity)
12. [Per-Declaration Data For Tools](#per-declaration-data-for-tools)
13. [Wasm GC Representation](#wasm-gc-representation)
14. [Migration](#migration)
15. [Recommendation](#recommendation)
16. [Questions For The Owner](#questions-for-the-owner)
17. [Parse Log](#parse-log)

## Problem

Function types are a separate structural type form. Chapter 04 lists them
beside tuples and generic instantiations, and three rules single them out:

1. **No implementations.** `impl Marker for fn(i32) -> i32` is a
   `function-impl-target` error
   ([Implementation Targets](../spec/09-traits.md#implementation-targets),
   the second half of TQ-27). A library cannot say "every function whose
   parameters decode from JSON is a tool handler", and `std` cannot give
   function values `Display` or a debug form, even though tuples, `Option`,
   and `List` accept implementations.
2. **Special-cased variance.** Function types are invariant for standalone
   conversions, but the polarity rules in
   [Variance](../spec/04-type-system.md#variance) treat parameters as negative
   and results as positive, and a function value read through a converted
   container is "viewed at the converted field type". That split exists only
   because function types are not declarations with declared variance.
3. **No per-declaration data at the value.** A tool adapter needs the
   parameter names, documentation, defaults, and metadata of `get_user`, and
   a typed callable. A function value carries only the second. The Design G
   decisions in [Typed Derivation](TYPED_DERIVATION.md#owner-decisions) left
   "function targets (tools)" open for this reason: `@derive` needs a type to
   implement a trait for, and a function declaration has none of its own.

The first two problems are about the shape of the function type itself. The
third needs something more than a function type, and is treated separately in
[Per-Declaration Data For Tools](#per-declaration-data-for-tools).

## What hd Has Today

- **Four function-type forms.** `fn(...) -> T`, `mut fn(...) -> T` (the
  closure mutates captures; calling it needs mutable access), `fn!(...) -> T`
  (suspending), and `mut fn!(...) -> T`, each with an optional `$ Row`
  ([Function Types And Values](../spec/07-functions.md#function-types-and-values)).
  A final vararg element, as in `fn(string, i32...) -> i32`, is part of the
  type. Parameter names and defaults are not.
- **Suspension lowering.** `fn!(A) -> T $ R` converts one way to
  `fn(A) -> mut Suspend[T] $ R`. The first form promises a cold construction
  and allows `callee!(...)`; the second may run code before it returns a
  suspension ([Suspending Functions](../spec/11-requirements-and-suspension.md#suspending-functions)).
- **Rows are sets.** A requirement row is an unordered, normalized set,
  written as a comma list; a row parameter listed beside keys, as in
  `$(R, Logger)`, contributes its keys. A row parameter is inferred from
  its use after `$`, and a row-kinded argument is written as a parenthesized
  row such as `$(Log, Clock)` or `$()` ([Types](../spec/02-grammar.md#types)).
- **GQ1.** The clause before a header's `:` belongs to the declaration, so a
  function-typed result with its own row is parenthesized:
  `fn make() -> (fn() -> i32 $ Log) $ Console:`.
- **Packs.** Type packs are valid on function, method, variant, and
  generic-implementation parameters, not on type declarations. Pack
  arguments are always inferred; "explicit pack arguments ... are not
  language constructs" ([Calls And Inference](../spec/12-variadic-generics.md#calls-and-inference)).
  A tuple type may expand a pack, as in `(Ts...)`.
- **Tuples as precedent.** Tuples are structural, yet they are
  implementation targets through "one built-in constructor per arity". `T?`
  is exact sugar for the prelude enum `Option[T]` (O1 to O3). Both show how a
  structural spelling can sit on a constructor the standard library owns.
- **Values.** A named function, a closure, or a single-payload variant
  constructor is a function value. Function values are `AnyRef`, have no
  `Eq`, and each closure evaluation has its own identity. Function
  types, closures, and function values are not inspectable, "whatever their
  row" ([Inspectable Types](../spec/09-traits.md#inspectable-types)).
- **Method values** stay deferred (P6).
- **Derivation.** Design G generates a sealed `Structure` per data type and
  enum, and `@derive` forwards to a structural function named by
  `@derivable`. Decision 6 makes "an obligation on a function type" an error
  naming the field. Decision 8 adds `FnMetadata` container metadata.
  Decisions 9 and 10 leave function targets open.

## Survey

| Language | Function type | Arity | Variance | Implementations or extensions on function types | Per-declaration type or data |
| --- | --- | --- | --- | --- | --- |
| Rust | `fn(A) -> B` pointer type; closures are unique anonymous types implementing `Fn`/`FnMut`/`FnOnce` | `Fn<Args>` takes a **tuple** of arguments; `Fn(A, B) -> C` is sugar for `Fn<(A, B), Output = C>` | Only lifetimes vary | Blanket impls over `F: Fn(..)`; impls on `fn` pointers were long written per arity by macro | Each `fn` item has a unique zero-sized type that coerces to a `fn` pointer; it cannot be named, so attribute macros that need an impl per function generate a unit struct |
| Scala | `A => B` is `Function1[A, B]`, a trait | `Function0` to `Function22` (Scala 3 removes the limit with an erased `FunctionXXL`) | `Function1[-T, +R]` | Any class may extend `Function1`; implicits and extension methods apply | Eta-expansion yields a plain `FunctionN`; no declaration data |
| Kotlin | `(A) -> B` is `Function1<A, B>` on the JVM | `Function0` to `Function22`, then a vararg `FunctionN` | `Function1<in P1, out R>` | Extension functions on function types | `::f` has type `KFunction1<A, B>`, a subtype of `(A) -> B` that exposes `name` and `parameters` through reflection |
| Kotlin `suspend` | `suspend (A) -> B`, a distinct type | Lowered to `Function2<A, Continuation<B>, Any?>` | as above | Few | as above |
| Swift | Structural `(A) async throws -> B`; closure parameters are non-escaping unless marked `@escaping` | Parameter packs: `(repeat each T) -> U` | Parameters contravariant, results covariant (subtyping) | "Non-nominal type cannot be extended"; no conformances | None; macros generate peer declarations |
| C# | Delegates are **nominal**; two delegate types with one signature are distinct | `Func<...>` and `Action<...>` up to 16 parameters | `Func<in T, out TResult>` | Extension methods on delegate types | `Delegate.Method` gives a `MethodInfo` with parameter names and attributes; ASP.NET minimal APIs build handlers from it |
| TypeScript | Structural `(a: A) => B`; parameter names are documentation | Rest parameters may be **tuple types**: `(...args: [A, B]) => R` | Contravariant under `strictFunctionTypes` (bivariant for methods) | Conditional types destructure: `Parameters<F>`, `ReturnType<F>` | None at run time; decorators on methods only |

### Takeaways

1. **Group the inputs.** Rust's `Fn<Args>` and TypeScript's tuple rest
   parameters both treat the parameter list as one tuple type. That is the
   only design here that gives one constructor for every arity without
   packs on the type declaration. Scala, Kotlin, and C# chose one
   constructor per arity and then paid for it with arity limits, a
   vararg fallback, or macro-generated impls.
2. **Declaration-site variance is ordinary.** Scala, Kotlin, and C# declare
   contravariant inputs and a covariant output on their function
   constructors. hd's polarity rules already treat functions that way.
3. **Suspension is a separate type.** Kotlin keeps `suspend` function types
   distinct and lowers them to an extra continuation parameter, as hd lowers
   `fn!` to a `Suspend[T]` result.
4. **Declaration data travels in two ways.** In Rust a trait is implemented
   per function: each `fn` item has its own type, and attribute macros
   generate a nameable unit struct when they need an impl. Kotlin and C#
   attach reflective declaration data to the function value. hd has no
   general reflection, so a per-function type or an explicit view is the
   realistic choice.
5. **Structural function types block extension.** Swift's "non-nominal type
   cannot be extended" is TQ-27's current rule. Swift's work on tuple
   conformances built on parameter packs moves toward what the owner's sketch
   proposes for functions.

## Why Not Two Packs

`Fn[Is..., O, Rs...]` has three problems:

1. **Positional ambiguity.** In `Fn[A, B, C]`, the split between `Is...`,
   `O`, and `Rs...` is not determined: `Is = (A)`, `O = B`, `Rs = (C)`, or
   `Is = (A, B)`, `O = C`, `Rs = ()`, among others. Chapter 12 already
   refuses to guess a partition between positional packs.
2. **Rows are not packs.** A pack is ordered and may repeat an element. A row
   is an unordered set with normalization.
   `Fn[..., Log, Clock]` and `Fn[..., Clock, Log]` must be the same type, and
   an extension such as `$(R, Logger)` has no pack meaning. The row belongs in one row-kinded
   parameter, which the language already has.
3. **Packs are not allowed on type declarations,** and pack arguments are
   never written explicitly. `Fn[i32, string, bool]` with a pack parameter
   would be the first explicit pack argument.

Grouping the inputs in a tuple removes all three: `Fn[(A, B), O, R]` has
exactly three ordinary arguments. The first is a tuple type, which may expand
a pack in generic code as `(Is...)`. The second is the result. The third is a
row.

## Design 1: A Family Of Standard Constructors

The standard library declares four opaque constructors, one per existing
function-type form, each with the same three parameters. They live in the
prelude or in a `std.function` module ([question 12](#12-where-do-the-constructors-live));
the sugar needs no import either way.

| Sugar | Constructor form |
| --- | --- |
| `fn(A, B) -> O $ R` | `Fn[(A, B), O, R]` |
| `mut fn(A, B) -> O $ R` | `mut MutFn[(A, B), O, R]` |
| `fn!(A, B) -> O $ R` | `SuspendFn[(A, B), O, R]` |
| `mut fn!(A, B) -> O $ R` | `mut MutSuspendFn[(A, B), O, R]` |

A missing row is `$()`. The sugar is exact, as `T?` is for `Option[T]`: both
spellings name the same type, and diagnostics print the sugar. The types
have no fields and no construction syntax of their own; values come only from
function names, closures, single-payload variant constructors, and
instantiated generic functions.

```text
let plain: Fn[(i32, string), bool, $()] = check
let rowed: Fn[(UserId,), User, $(Database, Cache)] = load
let counter: mut MutFn[(), i32, $()] = next
let loader: SuspendFn[(UserId,), Result[User, DbError], Database] = load_user
```

**The input argument is tuple-kinded.** It is a tuple type, such as `()`,
`(A,)`, `(A, B)`, `(Is...)`, or `(A, Is...)`, or a generic parameter. A
parameter used there becomes tuple-kinded, just as a parameter used after `$`
becomes a row parameter, and it can be instantiated only with a tuple type.
Code that holds a function through such a parameter can store, pass, and
convert it; a call needs the elements, so calling code writes the inputs as
a pack, `(Is...)`. `Fn[i32, i32, $()]` is a `generic-kind-mismatch` error,
like a row argument for a type parameter. A one-parameter function whose
parameter is a pair is `Fn[((A, B),), O, R]`, distinct from the
two-parameter `Fn[(A, B), O, R]`, so no tuple is ever flattened into
parameters.

**Mutation.** `mut fn` becomes the ordinary `mut` access modifier on its own
constructor. Calling a `MutFn` needs mutable access, exactly as a `mut self`
method does, so a readonly `MutFn[...]` view is a valid type that can be
stored and passed but not called. This settles a question the current text
leaves implicit: whether rule 4 (`mut T -> T`) may turn `mut fn() -> i32`
into a callable `fn() -> i32`. Under Design 1 it cannot, because `Fn` and
`MutFn` are different constructors.

**Suspension.** `SuspendFn[I, O, R]` keeps the cold-construction promise and
the `callee!(...)` form. The existing one-way lowering becomes a conversion
between constructors:

```text
let load: fn!(UserId) -> User $ Db = fetch_user
let lowered: fn(UserId) -> mut Suspend[User] $ Db = load
```

That is `SuspendFn[(UserId,), User, Db]` to
`Fn[(UserId,), mut Suspend[User], Db]`, and the reverse stays a
`type-mismatch`.

**Varargs.** A vararg element does not fit a tuple: `(string, i32...)` is not
a tuple type, because `i32` is not a pack. [Question 4](#4-do-varargs-stay-in-function-types)
proposes that a vararg function used as a value has a final `List[T]`
parameter, as in Kotlin and Scala, so the vararg convention stays a property
of calls by name, like defaults and named arguments.

**Results with rows need no parentheses.** The spelled form carries its row
inside brackets, so GQ1's parenthesized case has a second spelling:

```text
fn make() -> Fn[(), i32, Log] $ Console:
    _ := $.use(Console)
    fn() -> i32 $ Log:
        _ := $.use(Log)
        2
```

**Generic code over every arity.** Because the inputs are one tuple, a pack
pattern covers every arity with one declaration:

```text
fn call_all[Is..., O, R](fs: List[Fn[(Is...), O, R]], args: (Is...)) -> List[O] $ R:
    let results: mut List[O] = []
    for f in fs:
        results.append(f(args...))
    results
```

Cost: four new standard names and four implementations when a trait wants
to cover every kind of function. Most traits want one or two (a tool adapter
wants `SuspendFn` and `Fn`).

## Design 2: One Constructor With A Mode Argument

One constructor `Fn[M, I, O, R]` whose first argument is one of four sealed
marker types, such as `Plain`, `Mutating`, `Suspending`, and
`MutatingSuspending`:

```text
let f: Fn[Plain, (i32,), i32, $()] = inc
```

It gives one implementation for every mode, as in
`impl[M, Is..., O, R] Describe for Fn[M, (Is...), O, R]`. But the call rules
depend on the mode: whether mutable access is needed, whether the call is a
cold construction, whether `callee!(...)` is allowed. A value of
`Fn[M, ...]` with a generic `M` therefore cannot be called at all, so the
one-implementation benefit covers only implementations that never call the
function (debug printing, marker traits). Implementations that call, such as
tool adapters, still split by mode. Every spelled type also grows a fourth
argument, and none of the surveyed languages encodes the mode as a type
argument.

## Design 3: Callable Traits

The Rust model: sealed traits `Call[I, O, R]`, `CallMut[I, O, R]`, and
`CallSuspend[I, O, R]`, implemented by one anonymous type per closure
expression and per named function. `fn(A) -> O` becomes the dynamic trait
value type of `Call[(A,), O, $()]`.

This does not reach the goal. An implementation over "every function" is then
`impl[F < Call[...]] Tr for F`, a blanket implementation over a bare
parameter, which hd rejects (`bare-parameter-impl-target`). An
implementation for the trait value type is `trait-value-impl-target`. Unique
closure types also buy less in hd than in Rust: generic code is compiled once
per shape, and every closure has the reference shape, so a unique type gives
no specialization. What remains useful from this model is the per-declaration
item type, which [option B](#option-b-per-declaration-item-types) below
borrows for named functions only.

## Variance

Under Design 1 the family is declared with ordinary declaration-site
variance: contravariant in each input element, covariant in the output, and
invariant in the row.

- **Inputs and output.** This is the polarity chapter 04 already uses to
  check declarations such as `data Producer[+T]: produce: fn() -> T`. The
  only representation-preserving component conversion is permission
  weakening, so every new standalone conversion only changes permissions:
  `mut U -> U` in the result and `U -> mut U` in a parameter, possibly nested
  inside other covariant arguments. They are reachable today through a
  converted container. Declaring the variance
  removes the special paragraph in
  [Nominal And Structural Types](../spec/04-type-system.md#nominal-and-structural-types).
  The input argument is tuple-kinded, so its variance is stated per element
  by the declaration; tuples in general need no new variance rule.
- **Row.** Row subsumption, using a function that needs `Log` where one that
  may need `$(Log, Clock)` is expected, is sound but not
  representation-preserving: the implementation model passes providers
  positionally in canonical key order, so the callee's provider list would
  differ. The row stays invariant, as today.
- **`mut MutFn`.** Chapter 04 makes every `mut G[T]` view invariant, because
  the mutable view could replace stored values. A mutating closure is always
  used through `mut MutFn[...]`, so under that rule mutating closures get no
  variance at all. The reason does not apply to an opaque constructor with no
  replaceable contents, so an exemption for the function family is sound.
  [Question 5](#5-declare-variance-on-the-function-constructors) asks whether
  to make it.

```text
fn widen(f: fn(User) -> mut Post) -> fn(mut User) -> Post:
    f
```

Accepted with declared variance; a `type-mismatch` today.

A related conversion is not variance: a plain closure where a mutating one is
expected. Rust has `Fn: FnMut`. In Design 1 it would be
`Fn[I, O, R] -> mut MutFn[I, O, R]` (and `SuspendFn` to `MutSuspendFn`).
Both have the same runtime layout, and a plain closure mutates nothing, so
granting `mut` access is harmless. It is still a new implicit rule, so it is
[question 6](#6-may-a-plain-function-be-used-where-a-mutating-one-is-expected).

```text
fn repeat[R](times: i32, body: mut fn() -> void $ R) -> void $ R:
    let i = 0
    while i < times:
        body()
        i = i + 1

fn main() -> void $ Console:
    repeat(3, fn() -> void $ Console: println("tick"))
```

The closure is plain, so today it does not match `mut fn`; under question 6
it is accepted.

## What Becomes Possible: Implementations Over Functions

With `Fn` as a standard-library constructor, a function type is an ordinary
implementation target and `function-impl-target` is removed (revisiting
TQ-27).

**Ownership.** The standard library owns the four constructors, as it owns
`Option`, `List`, and the tuple constructors. By
[Implementation Ownership](../spec/09-traits.md#implementation-ownership), an
implementation for a function type is therefore allowed only in the package
of the trait or of one of its trait arguments. Inherent implementations on
the family stay standard-library only, so `std` could add methods such as
`then` or `compose` and nobody else could.

```text
trait Describe:
    fn describe(self) -> string

impl[Is..., O, R] Describe for Fn[(Is...), O, R]:
    fn describe(self) -> string:
        "function"

impl Describe for Fn[(i32,), i32, $()]:
    fn describe(self) -> string:
        "i32 to i32"
```

**Overlap.** Full-head unification ([Overlap](../spec/09-traits.md#overlap))
applies unchanged. The two implementations above overlap: `(Is...)` unifies
with `(i32,)`, and `R` unifies with `$()`, so the pair is an
`overlapping-impl` error. `Fn[(i32,), i32, $()]` and `Fn[(string,), i32, $()]`
do not overlap. Two points need rules:

- a pack pattern `(Is...)` unifies with any tuple and `(A, Is...)` with any
  tuple of at least one element; implementation heads with packs already
  exist, so this only needs to be written down;
- a row position in an implementation head is either a row parameter or a
  concrete row. Patterns such as `$(R, Log)` in a head are
  rejected, so row unification stays "a parameter unifies with anything, two
  concrete rows unify when they are equal sets".

**Tool handlers without derivation.** A library can accept any suspending
function whose parameters decode:

```text
impl[Ps... < Decode + Schema, O < Encode + Schema, R] IntoTool[R] for SuspendFn[(Ps...), O, R]:
    fn into_tool(self, shape: FnShape) -> Tool[R]:
        tool(self, shape)
```

The implementation covers calling, not naming: the function type still has
no parameter names, documentation, or defaults. That is why it takes a
`FnShape`; see [Per-Declaration Data For Tools](#per-declaration-data-for-tools).

**Derivation.** Decision 6's special case, "an obligation on a function type
is an error naming the field", becomes the ordinary missing-implementation
error, and `std` could supply, for example, a debug form for every function
type so that `@derive` of a debug trait on a data type with a callback field
succeeds. `Fn` still gets no `Structure`: it is opaque, not a data
declaration.

**Dynamic safety** is unaffected. A trait implemented for a function type is
dynamically safe or not by its own members. A function value converted to
such a trait value keeps its identity, as any `AnyRef` value does.

**Trait delegation** is unaffected: `by` names an embedded data field, and a
function type cannot be embedded.

## Runtime Identity And `Inspectable`

Function values stay excluded from `Inspectable`, as values and as type
arguments. The reason recorded in [Runtime Type Identity](INSPECTABLE.md)
still holds: a function type's identity would include its row, and rows have
no runtime descriptor. Nominal constructors change one detail. Rule 2 of
[Inspectable Types](../spec/09-traits.md#inspectable-types) makes a
module-level declaration applied to inspectable arguments inspectable, and
the four constructors are module-level declarations in `std`. The exclusion
must therefore name them explicitly, as it names `Suspend[T]`:

```text
fn erase(f: fn() -> i32) -> Inspectable:
    f
```

stays a `type-mismatch`, and `Box[fn() -> i32]` stays non-inspectable. A data
type with a function-typed field stays inspectable (INSPECTABLE question 5).
`TypeId` names are unaffected because no function type has one.

## Closures, Captured State, And Identity

Nothing about closures changes. A value of `Fn[I, O, R]` is still a function
reference plus an environment record. The environment is not part of the
type: the constructors are opaque, so captured locals and captured providers
are invisible to implementations, to `Structure`, and to `Inspectable`.

- **Rows are residual.** A closure that captured a `Logger` provider from an
  enclosing `$.with` has a row without `Logger`. The type argument `R` is the
  row the caller must still satisfy.
- **Captured storage.** `let` bindings shared by a closure and its scope stay
  shared. Only a `MutFn` or `MutSuspendFn` may assign them, as today.
- **Identity.** Each closure evaluation has one identity, and `is` compares
  it. The identity of a named function value is not specified today.
  [Question 11](#11-what-is-the-identity-of-a-named-function-value) proposes
  one canonical identity per declaration (and per instantiation of a generic
  function), which costs nothing in the Wasm lowering below:

```text
fn same() -> bool:
    get_user is get_user
```

- **Serializable closures** stay deferred
  ([Open Issues](OPEN_ISSUES.md#serializable-closures-and-incremental-computation)).
  A nominal function type does not give a closure a serializable
  environment.

## Per-Declaration Data For Tools

A tool adapter for

```text
pub fn get_user!(id: UserId, include_deleted: bool = false) -> Result[User, NotFound] $ Users:
    pass
```

needs (1) a typed callable, `SuspendFn[(UserId, bool), Result[User, NotFound], Users]`;
(2) the parameter names, documentation, and metadata, which `FnShape`
already carries; and (3) the defaults, which neither carries as values. A
function type gives (1) only. The options differ in how (2) and (3) reach the
adapter.

### Option A: Function types plus a shape at registration

**A1, a pair.** The adapter is the generic implementation above and takes
`shape_of(f)` beside the value:

```text
fn register() -> void:
    registry.add(mcp.tool(get_user, shape_of(get_user)))
```

```text
pub fn tool[Ps... < Decode + Schema, O < Encode + Schema, Rq](f: fn!(Ps...) -> O $ Rq, shape: FnShape) -> Tool[Rq]:
    pass
```

Nothing new beyond Design 1. But the name is written twice and nothing ties
the two arguments together: `mcp.tool(get_user, shape_of(delete_user))`
type-checks. The adapter can compare parameter counts and `TypeShape`s at
run time, not statically. Defaults are unusable, because `FnShape` records
only `has_default`.

**A2, a typed view.** A `fn_view(f)` intrinsic, as in Typed Derivation
question 8, returns one value holding the callable, the `FnShape`, and typed
default thunks (defaults are requirement-free by
[Default Values](../spec/07-functions.md#default-values), so a thunk can run
anywhere). With nominal types it can be typed by the function type:
`FnView[SuspendFn[(Ps...), O, Rq]]`.

```text
fn register() -> void:
    registry.add(mcp.tool(fn_view(get_user)))
```

This is Kotlin's `KFunction` and C#'s `Delegate.Method`, made explicit. It
is one intrinsic and one type. It does not make `@derive` meaningful on a
function: the adapter is an ordinary generic function, and a decorator such
as `@mcp.tool` could only attach metadata.

### Option B: Per-declaration item types

Each non-generic module-level function declaration also declares a unique
zero-size **item type**, printed `fn get_user`. The item type converts
implicitly to the declaration's function type. The compiler implements a
sealed `std.derive.FnStructure[F]` for it, the function analog of Design G's
`Structure`, where `F` is the function type. A value-free `describe` visits
each parameter with its static type and `ParamShape`. An `arguments` method
builds the argument tuple from a source, evaluating a declared default when
the source supplies no value. A derivable trait then maps its methods to
structural functions over `FnStructure`, and `@derive(mcp.Tool)` works on a
function as it does on a data type:

```text
@derive(mcp.Tool)
pub fn get_user!(id: UserId) -> Result[User, NotFound] $ Users:
    pass
```

```text
fn tool_structure[T < FnStructure[SuspendFn[I, O, Rq]], I, O < Encode + Schema, Rq]() -> ToolSpec[Rq]:
    pass
```

As in Design G, the structural function uses no packs. `I` is a
tuple-kinded parameter the function never spreads. The per-parameter bounds
(`Decode + Schema`) sit on the library's describer and argument-source
methods, as `param[P < Decode + Schema]`. Decision 6 then reports a
parameter that fails them at the derive site, naming the parameter.

```text
impl[Rq] Registry[Rq]:
    pub fn add[T < Tool[Rq]](mut self, item: T) -> void:
        self.tools.append(T::tool())
```

Registration is `registry.add(get_user)`: the argument's item type is
inferred for `T`, and `T::tool()` is a static call through the bound (TQ-9).
The zero-size value is never read. A hand-written implementation for one
function, the escape hatch decision 6 describes, needs a way to name the item
type in a head:

```text
# Hypothetical syntax: item types in implementation and derive heads
impl mcp.Tool[Users] for fn get_user:
    fn tool() -> ToolSpec[Users]:
        with_strict_schema(tool_structure[fn get_user, (UserId,), Result[User, NotFound], Users]())

derive mcp.Tool for fn dep.users.get_user
```

**How visible should item types be?** Rust gives every function name its item
type and coerces on demand, which produces the familiar "expected fn item,
found a different fn item" errors. The narrower rule proposed in
[question 10](#10-where-are-item-types-visible) keeps the item type only
where a generic parameter is inferred from the argument (`registry.add(get_user)`
with `add[T < Tool[Rq]]`). Everywhere else the name has its function type: a
binding `f := get_user` has type `fn!(UserId) -> ...`, and `[add, sub]` is a
`List[fn(i32) -> i32]` without a least-common-type rule for item types. Item
types are printed as `fn get_user` in diagnostics and can be written only in
implementation and derive heads.

**Scope.** Item types exist for module-level, non-generic functions, the
same targets `shape_of` accepts. Generic functions would need a complete
explicit instantiation (the former Shape Intrinsic Coverage issue, now
superseded by [typed derivation](TYPED_DERIVATION.md)).
Methods stay with P6. Closures never have item types.

**Ownership.** The item type belongs to the function's package, so
`@derive(mcp.Tool)` beside the declaration is always allowed, and a standalone
derive for another package's function follows decision 9 (the trait's
package, or the root-application exception).

### Comparison

| | A1 pair | A2 `fn_view` | B item types |
| --- | --- | --- | --- |
| New language surface | none beyond Design 1 | one intrinsic, one view type | a type category, one conversion, `FnStructure`, head syntax |
| Name and value tied statically | no | yes | yes |
| Defaults usable | no | yes, through thunks | yes, generated `arguments` |
| `@derive` and decorators on functions | no | no, metadata only | yes, same rule as data |
| Per-function hand-written impl | no | no | yes |
| Registration | `mcp.tool(get_user, shape_of(get_user))` | `mcp.tool(fn_view(get_user))` | `registry.add(get_user)` |
| Canonical identity of named functions | separate decision | separate decision | follows from zero-size items |

Option B is the only one that makes decision 7's "`@Facet` is sugar for
`@derive(Facet)`" meaningful on a function. Option A2 is the cheaper fallback,
and neither needs to block Design 1.

## Wasm GC Representation

The prototype already lowers a closure to a struct of a typed function
reference and an environment, keyed by signature
(`$closureN { fn: (ref $sigN), env: anyref }` in
[src/emitter/emitter.ts](../src/emitter/emitter.ts)), and wraps a named
function in an adapter that takes and ignores `$env`. Design 1 keeps this:

- **One layout for the family.** `Fn`, `MutFn`, `SuspendFn`, and
  `MutSuspendFn` share the closure struct. Mutation is a static discipline.
  A `SuspendFn` adapter already returns the uniform suspension record, so the
  `SuspendFn` to `Fn[I, mut Suspend[O], R]` conversion is the identity at
  the Wasm level, and so is `Fn` to `mut MutFn`.
- **Signatures come from shapes.** The Wasm signature of `Fn[(A, B), O, R]`
  is the environment, the shapes of `A` and `B`, the providers of `R` in
  canonical key order, and the shape of `O`. `mut` is erased, so the variance
  conversions of [Variance](#variance) change no Wasm type.
- **Generic rows.** A stored `Fn[I, O, R]` with a generic `R`, as in
  `data Tool[Rq]`, already needs either specialization or a provider-record
  convention for the unknown row. Nominal types do not change that.
- **Implementations over the family.** An implementation with a pack head is
  specialized per instantiation, as pack functions are, and its dictionary
  for a statically known head is a constant.
- **Item types.** A zero-size item has no Wasm value. Converting it to its
  function type reads one immutable global closure per declaration (a null
  environment and the adapter), so it allocates nothing and gives the
  canonical identity of question 11.

## Migration

With the recommended answers, a valid program stays valid with the same
meaning, with two exceptions. A vararg function used as a value changes type
under question 4. A declaration named `Fn`, `MutFn`, `SuspendFn`, or
`MutSuspendFn` becomes a `prelude-name-shadow` error if the constructors go
in the prelude (question 12). Questions 5, 6, and 7 only accept more
programs. The `fn(...)` sugar stays the primary spelling. The spelled forms
are for implementation heads, pack patterns, and row-bearing results.

Specification text that would change if the owner accepts:

- **02 Grammar.** No production changes. Add that each function-type form is
  sugar for its constructor, and state the meaning of the grouped
  `mut (fn() -> i32)`, which parses today and would be `mut Fn[...]`: mutable
  access to a plain function, which calls like the readonly form.
- **04 Type System.** Type Forms: function types become sugar for the family,
  like `T?`. Nominal And Structural Types: replace the function-type
  paragraph. Variance: the declared variance of the family, if question 5 is
  accepted. The `AnyRef` list is unchanged.
- **07 Functions.** Function Types And Values: the sugar table and the vararg
  rule of question 4. The suspending-lowering paragraph restated as a
  constructor conversion.
- **09 Traits.** Implementation Targets: delete `function-impl-target`.
  Implementation Ownership: `std` owns the family. Overlap: pack and row
  unification in heads. Inspectable Types: name the family in the
  exclusions.
- **11 Requirements And Suspension.** The `fn!` equivalence in terms of
  `SuspendFn` and `Fn`. Row-kinded parameters on data declarations, which
  `data Tool[Rq]` in Typed Derivation already assumes, stated explicitly.
- **README.** A revision note superseding the second sentence of TQ-27
  (partial), and `function-impl-target` removed from the error list.
- **Conformance.** `typing/invalid/function-impl-target.hd` implements a
  local trait for a function type, which becomes valid; it would be replaced
  by fixtures for overlap between function heads, an orphan implementation
  on `Fn`, `generic-kind-mismatch` for a non-tuple input, and the variance
  and `MutFn` conversions if accepted.
- **Design records.** Typed Derivation decision 6's function-type case
  becomes an ordinary missing implementation; INSPECTABLE's table row for
  function types is unchanged in meaning.
- **Prototype.** The checker can keep its structural function `ValueType` and
  map the spelled forms onto it; the emitter changes only for item types and
  canonical identity.

## Recommendation

1. **Design 1**: `Fn[(Is...), O, R]`, `MutFn`, `SuspendFn`, and
   `MutSuspendFn` as opaque constructors in `std.function`, with a tuple-kinded
   input argument and a row-kinded third argument; the existing syntax is
   exact sugar and needs no import.
2. Declare contravariant inputs, a covariant output, and an invariant row,
   deleting the special variance paragraph; exempt the family from `mut`-view
   invariance.
3. Remove `function-impl-target`. Ordinary ownership and overlap rules apply;
   row positions in heads are a parameter or a concrete row.
4. Keep function values out of `Inspectable`, naming the family explicitly.
5. For tools, **option B** (item types, visible only to inference and heads),
   staged after Design 1; option A2 (`fn_view`) if item types are rejected.
6. Function values drop the vararg convention (a final `List[T]`), and
   named function values get one canonical identity.

## Questions For The Owner

### 1. How are the inputs, output, and row arranged?

- **A.** `Fn[(Is...), O, R]`: the inputs are one tuple-kinded argument, the
  row is row-kinded.
- **B.** One constructor per arity, `Fn0[O, R]`, `Fn1[A, O, R]`, and so on,
  like tuples, Kotlin, and Scala.
- **C.** Output first with one trailing pack, `Fn[O, R, Is...]`, which needs
  packs on type declarations and explicit pack arguments.
- **D.** The sketch `Fn[Is..., O, Rs...]`, which is ambiguous and treats a
  row as an ordered pack.

**Recommendation: A.** One constructor for every arity, one implementation
covers them through `(Is...)`, and it matches Rust's `Fn<Args>`.

```text
impl[Is..., O, R] Describe for Fn[(Is...), O, R]:
    fn describe(self) -> string:
        "function"
```

### 2. How are mutation and suspension expressed?

- **A.** Four constructors: `Fn`, `MutFn`, `SuspendFn`, `MutSuspendFn`.
- **B.** One constructor with a mode argument, `Fn[M, I, O, R]`.
- **C.** Fold suspension into the result: `fn!` becomes
  `Fn[I, mut Suspend[O], R]`, and `f!(x)` works on any function returning a
  suspension. This drops the cold-construction promise.

**Recommendation: A.** Call rules differ by mode, so B's single
implementation cannot call the function, and C loses a guarantee chapter 11
states. Naming is open; `FnMut` and `FnSuspend` group better alphabetically,
while `MutFn` reads like the sugar.

```text
let counter: mut MutFn[(), i32, $()] = next
let loader: SuspendFn[(UserId,), Result[User, DbError], Database] = load_user
```

### 3. Is the sugar exact?

- **A.** Yes: `fn(A) -> O $ R` and `Fn[(A,), O, R]` are the same type, both
  may be written anywhere, and diagnostics print the sugar, as for `T?`.
- **B.** The spelled form is allowed only in implementation heads and generic
  patterns.

**Recommendation: A.** It matches the `Option` decision, and B would make
`Fn[(), i32, Log]` unusable as a result type, where it avoids GQ1's
parentheses.

```text
fn make() -> Fn[(), i32, Log] $ Console:
    _ := $.use(Console)
    fn() -> i32 $ Log:
        _ := $.use(Log)
        2
```

### 4. Do varargs stay in function types?

- **A.** No. A vararg function used as a value has a final `List[T]`
  parameter; calls by name keep the vararg form, like defaults.
- **B.** Yes, through a marker element in the input tuple, such as
  `Fn[(string, Rest[i32]), i32, $()]`.
- **C.** Yes, through a fifth dimension (more constructors or a flag).

**Recommendation: A.** Kotlin and Scala do the same, a tuple cannot hold a
vararg element, and function values already drop names and defaults.

```text
let f: fn(List[i32]) -> i32 = sum
```

Today `sum` has type `fn(i32...) -> i32`.

### 5. Declare variance on the function constructors?

- **A.** Contravariant in each input element, covariant in the output,
  invariant in the row; the family is exempt from `mut`-view invariance.
- **B.** The same without the exemption, so `mut MutFn[...]` is invariant.
- **C.** Keep function types invariant; the constructors are a re-spelling.

**Recommendation: A.** The polarity rules already assume it, the new
conversions are permission weakening only, and a special paragraph goes.

```text
fn widen(f: fn(User) -> mut Post) -> fn(mut User) -> Post:
    f
```

### 6. May a plain function be used where a mutating one is expected?

- **A.** Yes: `Fn[I, O, R]` converts to `mut MutFn[I, O, R]`, and
  `SuspendFn` to `mut MutSuspendFn`, like Rust's `Fn: FnMut`.
- **B.** No: the kinds must match, and callers write a `mut fn` literal.

**Recommendation: A.** It preserves representation, grants nothing a plain
closure could misuse, and lets a plain closure reach a `mut fn` parameter.

```text
fn twice[R](body: mut fn() -> void $ R) -> void $ R:
    body()
    body()

fn main() -> void $ Console:
    twice(fn() -> void $ Console: println("tick"))
```

### 7. Are function types implementation targets?

- **A.** Yes, under the ordinary ownership and overlap rules; row positions
  in heads are a row parameter or a concrete row; inherent methods on the
  family are `std` only.
- **B.** Yes, but only with fully generic heads such as
  `Fn[(Is...), O, R]`, never a concrete signature.
- **C.** No; keep `function-impl-target`.

**Recommendation: A.** TQ-28 already lets heads differ by argument, and B
would forbid a useful `Fn[(Request,), Response, R]` handler impl.

```text
impl[Ps... < Decode + Schema, O < Encode + Schema, R] IntoTool[R] for SuspendFn[(Ps...), O, R]:
    fn into_tool(self, shape: FnShape) -> Tool[R]:
        tool(self, shape)
```

### 8. Do function values stay out of `Inspectable`?

- **A.** Yes, as values and as type arguments, with the four constructors
  named in the exclusion list.
- **B.** Make function types with a concrete row inspectable, giving rows a
  descriptor built from their keys' identities.

**Recommendation: A.** Nothing needs to recover a function type, and B would
let code branch on which providers a callback requires.

```text
fn erase(f: fn() -> i32) -> Inspectable:
    f
```

This stays a `type-mismatch`.

### 9. How do tool adapters get per-declaration data?

- **A1.** Function types plus `shape_of(f)` passed beside the value.
- **A2.** Function types plus a `fn_view(f)` intrinsic returning a typed view
  with the callable, the shape, and default thunks.
- **B.** Per-declaration item types with a compiler-generated
  `FnStructure`, so `@derive(mcp.Tool)` works on functions.

**Recommendation: B**, staged after Design 1, because it gives functions the
same derive and decorator rule as data (decisions 1, 7, and 8). If item types
are too much surface, **A2**; A1 lets the value and its shape disagree.

```text
@derive(mcp.Tool)
pub fn get_user!(id: UserId) -> Result[User, NotFound] $ Users:
    pass
```

### 10. Where are item types visible?

- **A.** Only where a generic parameter is inferred from the argument, and in
  implementation and derive heads written `fn name`; everywhere else a
  function name has its function type.
- **B.** Everywhere, as in Rust: every function name has its item type and
  converts to its function type on demand, including in least common types.

**Recommendation: A.** Bindings and list literals keep today's types, and the
Rust errors about distinct item types cannot arise.

```text
fn tools() -> mcp.Registry[Users]:
    registry := mcp.Registry::new()
    registry.add(get_user)    # T is the item type `fn get_user`
    handler := get_user       # the function type fn!(UserId) -> ...
    registry
```

### 11. What is the identity of a named function value?

- **A.** One canonical identity per declaration, and per instantiation of a
  generic function, so `get_user is get_user` is `true`.
- **B.** Leave it unspecified.

**Recommendation: A.** It costs one immutable global per declaration, and
item types would imply it anyway.

```text
fn same() -> bool:
    get_user is get_user
```

### 12. Where do the constructors live?

- **A.** In a `std.function` module (`fn` is reserved, so not `std.fn`).
  The sugar needs no import; the spelled forms are imported where they are
  written.
- **B.** In the prelude, like `Option`.

**Recommendation: A.** The spelled forms appear mostly in implementation
heads and library signatures. B would make every existing declaration named
`Fn` a `prelude-name-shadow` error.

```text
use std.function.{Fn, SuspendFn}

impl[Is..., O, R] Describe for Fn[(Is...), O, R]:
    fn describe(self) -> string:
        "function"
```

## Parse Log

Every block without the **Hypothetical syntax** first line was parsed with
the reference parser on 2026-09-27; statements were parsed as module items
or inside the function shown. The hypothetical block is rejected because
`fn name` is not a type and the standalone `derive ... for` form of decision 9
is not in the grammar yet. `@derivable(...)` on a trait, used in Typed
Derivation, is also outside the current grammar (decorators precede only
data, enum, and function declarations) and is not used in a block here.
