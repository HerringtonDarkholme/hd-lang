# Decorators: Survey And Design Options

Status: design exploration, 2026-09-28; nothing here is decided or in the
specification.

On 2026-09-28 the owner asked for one general decorator design. It puts
decorators on most declarations and members, and each decorator declares a
target that checks both kind and signature. Decorators are read-only, and
compiler special cases are kept few. This record surveys how other languages do it, gives five
options, and ends with questions. It reviews
[Annotations](../spec/14-annotations.md), in particular
[Prefix Decorators](../spec/14-annotations.md#prefix-decorators),
[Member Metadata](../spec/14-annotations.md#member-metadata),
[Common Shape Representation](../spec/14-annotations.md#common-shape-representation),
[Opting In](../spec/14-annotations.md#opting-in) and
[Facts](../spec/14-annotations.md#facts), and the
[Annotations grammar](../spec/02-grammar.md#annotations). It also reviews
[Typed Derivation M1-M25](TYPED_DERIVATION.md#owner-decisions),
[Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions) (`@error`),
[Literal Suffixes L11](LITERAL_SUFFIXES.md#owner-decisions) (`@suffix`, on
hold pending this record), and
[FN_TYPE questions 9 and 10](FN_TYPE.md#9-how-do-tool-adapters-get-per-declaration-data).

## Owner Decisions

Decided 2026-09-28, after the ranked options. The owner went with a
simpler model than option 1: decorators stay plain values, and only the
kind of target is checked.

1. **D1: a decorator is a value attached to its target.** Any compile-time
   value can decorate any item or member. Items are functions, data types,
   enums, traits, impls and newtypes. Members are fields, variants,
   parameters and methods. Locals and expressions never take decorators.
   `annot.metadata.any-value` extends to every target, and
   `annot.decorator.function` (which rejects decorators before functions)
   is retired.
2. **D2: `@annotate` checks only the kind, and it is an ordinary
   decorator.** `std.annotation` declares:

   ```text
   pub enum Target: Fn, Data, Enum, Field, Variant, Param, Trait, Impl, Method
   pub data Annotate:
       pub kinds: List[Target]
   pub fn annotate(kinds: List[Target]) -> Annotate
   ```

   Writing `@annotate([.Fn])` on a fact type limits where values of that
   type may appear. A value placed on another kind of target is an error
   at the decorator. The compiler recognizes `std.annotation.Annotate` by
   its qualified name. No new syntax is needed: `annotate` is no longer
   reserved (M26), and the kinds are enum values. Bootstrapping: std writes
   `@annotate([.Data, .Enum])` on the `Annotate` type itself.
3. **D3: signatures are checked by the reader, not the compiler.**
   Whatever reads a fact validates it. `@suffix` on `fn s(x: string)` is
   accepted at the declaration and fails as `type-mismatch` at the first
   `5s`. A template rejects a `max_len` on an `i32` field through
   `member[F]`. The runner rejects a test marker on the wrong signature.
   Options 1, 4 and 5 (target types, signature patterns, a binder) are not
   adopted.
4. **D4: `@annotate` is optional.** A fact type without an `@annotate`
   line may go on any target, so literal facts such as `@"note"` keep
   working.

The preview the owner approved showed `@annotate([.Data])` above
`pub fn annotate`. The intended meaning is the bootstrap line in D2, on
the `Annotate` type.

Questions 1-4, 6 and 14 below are settled or moot under D1-D4.
Questions 5 and 7-13 remain.

## Contents

- [Owner Decisions](#owner-decisions)
- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Owner Direction](#owner-direction)
- [Use Cases](#use-cases)
- [Survey](#survey)
- [What Read-Only Means](#what-read-only-means)
- [Who Reads A Decorator](#who-reads-a-decorator)
- [Minimum Compiler Knowledge](#minimum-compiler-knowledge)
- [Option 1: A Decorator Trait With Target Types](#option-1-a-decorator-trait-with-target-types)
- [Option 2: A Check Function Over Shapes](#option-2-a-check-function-over-shapes)
- [Option 3: A Closed Standard Set](#option-3-a-closed-standard-set)
- [Option 4: A Root Meta-Decorator, `@annotate`](#option-4-a-root-meta-decorator-annotate)
- [Option 5: A Decorator Declaration With Target Patterns](#option-5-a-decorator-declaration-with-target-patterns)
- [Pitfalls](#pitfalls)
- [Comparison](#comparison)
- [Ranking By Design Cost](#ranking-by-design-cost)
- [Re-Evaluation After M26](#re-evaluation-after-m26)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Problem

What is a decorator in hd, where may it go, and how does it say where it
may go? The answer must let the standard library's `@derive`, `@error`,
`@suffix` and a test-runner marker be as ordinary as possible. It must do so
without full compile-time reflection and without a growing list of
intrinsics.

## What hd Has Today

| Topic | Today | Rule |
| --- | --- | --- |
| Type-level decorator | `@value` before a data type or enum attaches a type-level fact | [`annot.fact.type-level-decorator`](../spec/14-annotations.md#r-annot.fact.type-level-decorator) |
| Member decorator | `@value` before a field, a variant, or a payload parameter attaches member metadata | [Prefix Decorators](../spec/14-annotations.md#prefix-decorators), [`annot.fact.member-metadata`](../spec/14-annotations.md#r-annot.fact.member-metadata) |
| Parameter decorator | Only on value parameters of module-level named functions | [`grammar.fn.decorator-targets`](../spec/02-grammar.md#r-grammar.fn.decorator-targets) |
| Function decorator | Parses, then rejected | [`annot.decorator.function`](../spec/14-annotations.md#r-annot.decorator.function), `decorator-not-annotator` |
| Newtype decorator | Only `@derive` | [`grammar.annot.newtype-derive`](../spec/02-grammar.md#r-grammar.annot.newtype-derive) |
| Trait, impl, method, `use` | Not in the grammar: `syntax-error` or `decorator-not-top-level` | [Annotations grammar](../spec/02-grammar.md#annotations) |
| Module | No module declaration exists | [`module.path.no-declaration`](../spec/10-modules.md#r-module.path.no-declaration) |
| What a decorator value is | Any value; no marker trait | [`annot.metadata.any-value`](../spec/14-annotations.md#r-annot.metadata.any-value) |
| When it is evaluated | Once, at compile time, requirement-free | [`annot.fact.eval`](../spec/14-annotations.md#r-annot.fact.eval) |
| Target checking | None: "the language does not check that a metadata value suits its member's type" | [Member Metadata](../spec/14-annotations.md#member-metadata), the open fact check hook |
| Duplicates | Two values of one concrete type on one target: `duplicate-fact` | [Typed Derivation M25](TYPED_DERIVATION.md#still-open-after-the-prototype-pass) |
| `@derive(...)` | Special grammar: its arguments are trait names | [`grammar.annot.derive-traits`](../spec/02-grammar.md#r-grammar.annot.derive-traits) |
| `@error` | A compiler intrinsic, decided, not yet in the spec; `transparent`, `from` and `source` are markers, not names | [Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions) |
| `@suffix` | L11: the one intrinsic decorator allowed before a function; on hold | [Literal Suffixes L11](LITERAL_SUFFIXES.md#owner-decisions) |
| Tests | `it("name"):` calls in a `tests:` block; the `@test` label was dropped | [Testing T11](TESTING.md#owner-decisions) |
| Readers | Templates read facts through `T::facts()` and `info.facts`; runtime code reads member metadata through `shape[T]()` and `shape_of(f)` | [Facts](../spec/14-annotations.md#facts), [`ShapeMetadata`](../spec/14-annotations.md#common-shape-representation) |

The forms that exist today, in one block:

```text
use std.structure.Structure

@derive(Show)
@style(prefix="user_")
data User:
    @rename("mail")
    email: string

fn get_user(@description("User identifier") id: i64) -> User:
    pass

@tool("search")  # error: decorator-not-annotator
fn search(query: string) -> string:
    query
```

## Owner Direction

These points from 2026-09-28 bind every option below.

| # | Direction |
| --- | --- |
| D1 | Decorators go on module-level items (functions, data types, enums, traits, impls, newtypes, modules) and on members (fields, variants, parameters, trait methods). Never on locals or expressions. |
| D2 | A decorator declares what it attaches to, checking the kind and the signature, as TypeScript does. A suffix decorator on a function that takes no number is an error. |
| D3 | Read-only: a decorator never mutates or replaces its target. |
| D4 | The definition is open. The tentative form, `impl Decorator[FnTarget[fn(i64) -> Any]] for Suffix`, is to be reconsidered against real alternatives. |
| D5 | No full compile-time reflection, and few compiler special cases. Find the least compiler knowledge that makes `@derive`, `@error`, a suffix marker and test markers ordinary std decorators. |

## Use Cases

Every option is shown on the same six cases.

| # | Case | Needs |
| --- | --- | --- |
| U1 | `@suffix` on `fn ms(count: i64) -> Duration` | a signature check (one number, no providers, not suspending) and a compiler effect (`250ms` calls `ms`) |
| U2 | `@derive(Eq, Show)` on a data type | arguments that are trait names; generated impls |
| U3 | `@error` on an enum, `@error("...")` on a variant | a message template over payload members; generated impls |
| U4 | A test-runner marker, shown as `@bench` on `fn(b: mut Bencher) -> T` with `T < Termination` | a signature check with a bound; discovery by the runner |
| U5 | A library fact, `@rename("mail")` or `@max_len(80)` on a field | a kind check, and for `max_len` a member-type check |
| U6 | A function fact, such as `@route("/users")`, read by a tool or by user code | a place to store it, and a reader that is not reflection |

U4 uses a benchmark marker because Testing T11 replaced `@test` with
`tests:` blocks. Rust's `#[test]` has the same shape: a monomorphic free
function with no arguments whose result implements `Termination`.

## Survey

| Language | How a decorator is defined | Where it may go, and how that is checked | Replaces the target? | Compiler-known ones | Source |
| --- | --- | --- | --- | --- | --- |
| Java | `@interface` declaration with typed elements | `@Target(ElementType...)`: kinds only (type, method, field, parameter, package, module, local variable, type use) | No; processors only add files | `@Override`, `@FunctionalInterface`, `@SafeVarargs` in `java.lang`, checked by name | JLS 9.6, `@Target`, `Processor` |
| C# | A class deriving from `System.Attribute` | `[AttributeUsage(AttributeTargets...)]`: kinds only, plus `AllowMultiple`; default is all targets | No; source generators only add code | `[Obsolete]`, `[Conditional]`, `[CallerMemberName]` | C# attributes, `AttributeUsage`, source generators |
| Kotlin | `annotation class` | `@Target(AnnotationTarget...)`: kinds only; use-site targets such as `@file:` and `@get:` | No; KSP only generates new files | `@Deprecated`, `@JvmStatic` by qualified name | Kotlin annotations, KSP |
| TypeScript 5 | A plain function `(target, context) => replacement` | Classes and class members only, not parameters or free functions; the target is checked against the function's parameter type | Yes, it may return a replacement | none | TS 5.0 release notes, TC39 proposal |
| Python | Any callable | Anywhere a `def` or `class` is; no check | Yes, the result rebinds the name | `@staticmethod`, `@dataclass` are ordinary library code | PEP 318, PEP 3129, PEP 612 |
| Rust | Built-in attributes, plus procedural macros | Built-ins have compiler target rules (`#[test]` only on monomorphic, argument-free functions returning `Termination`); macros check by hand | Attribute macros may replace the item; derives only add | Built-in attributes; lang items (`#[lang = "add"]`) mark std items the compiler needs | Rust Reference: attributes, testing attributes, procedural macros; unstable book: lang items |
| Swift | `macro` declarations with roles (`@attached(peer)`, `member`, `accessor`, `memberAttribute`, `conformance`); property wrappers are types | The role fixes the kind; the macro implementation emits diagnostics for signatures | Expansions only add code; property wrappers change storage | `@Test` (Swift Testing) is a library macro | SE-0389, SE-0258 |
| Zig | No attributes | Not applicable: `comptime` code reads `@typeInfo` and generates code | Not applicable | `test` blocks, `export`, `inline` are keywords | Zig language reference |
| Scala | A class extending `scala.annotation.Annotation` (`StaticAnnotation` for compile time) | Meta-annotations (`@field`, `@getter`) direct placement; otherwise no target check | No; Scala 3 macro annotations (experimental) may | `@tailrec` makes the compiler check the method; `@main` | Scala tour, Scala 3 reference |
| Dart | A `const` variable or a `const` constructor call | `@Target(TargetKind...)` from `package:meta`, checked by the analyzer, not the compiler | No; the macros project was stopped in January 2025 | `@override` is a `const` in `dart:core` | Dart metadata, `meta` `Target`, Dart blog |
| Go | No decorators; struct tags are strings read by `reflect` | Tags go only on struct fields; directives such as `//go:noinline` are comments | No | `//go:` compiler directives by name | Go `reflect.StructTag`, `cmd/compile` |

Takeaways:

1. **Declared kinds are standard; declared signatures are not.** Java, C#,
   Kotlin and Dart all list target kinds. Only TypeScript checks a
   signature, and it does so by ordinary typing of the decorator function's
   parameter. Rust checks signatures only for its built-in attributes.
2. **Read-only is the proven compile-time model.** Java processors, KSP, C#
   source generators and Swift macros may add code but never edit the
   target. Replacement (Python, TypeScript, Rust attribute macros) is where
   full metaprogramming lives. Dart stopped its macros over analysis,
   incremental compilation and hot reload costs.
3. **Compilers recognize a few std annotations by name.** `java.lang.Override`,
   C# `ObsoleteAttribute`, Kotlin `JvmStatic` and Dart `override` are
   ordinary declarations the compiler knows. Rust's lang items are the same
   idea, but internal and unstable.
4. **Module annotations need a special place.** Java uses a
   `package-info.java` file, Kotlin `@file:`, C# `[assembly:]` and Rust
   `#![...]`.

## What Read-Only Means

"Read-only" must still allow `@derive` and `@error`, which generate impls.
The proven line is Swift's and KSP's: a decorator may **add** declarations
beside its target, but never **change** the target.

1. A decorator never changes its target's name, type, body, visibility or
   members.
2. A decorator never wraps or replaces a function, as Python and
   TypeScript do.
3. A compiler-known decorator may add new items (an impl) or make the
   target visible to one compiler feature (literal suffix lookup).
4. So the order of decorators never changes meaning. Source order is kept
   only for readers.

## Who Reads A Decorator

| Reader | What it reads | How, today or proposed | Reflection? |
| --- | --- | --- | --- |
| The compiler | Only compiler-known decorators (`@derive`, `@error`, `@suffix`) | By declaration identity, at compile time | No |
| Typed-derivation templates | Facts of the target type and its members | `T::facts()` and `h.info.facts.find[F]()`, today | No: one type, statically chosen |
| Tools and the program database | Every decorator value on every declaration, from the package interface | The values are compile-time constants, so an interface can carry them as data | No: tools read data, not code |
| User code at run time | A decorator on one named declaration | `shape[T]()` for types, `shape_of(f)` for functions | No enumeration; see below |

User code can read function facts without reflection by one small
extension: function-level decorator values become the `FnShape`'s metadata.
`FnShape` already implements the sealed `ShapeMetadata`, so `metadata[M]()`
needs no new API. `shape_of` already takes only a direct name, so user code
can never enumerate "every function marked `@route`". Discovery stays with
tools, as the test runner already registers `it` cases.

```text
@route("/users")
fn list_users() -> string:
    "[]"

fn routes() -> List[Route]:
    match shape_of(list_users).metadata[Route]():
        .Some(r) => [r]
        .None => []
```

## Minimum Compiler Knowledge

Of the four std decorators, only some need the compiler at all:

| Decorator | Effect | Needs the compiler? | Why |
| --- | --- | --- | --- |
| `@derive(...)` | Adds impls | Yes | Its arguments are trait names, not values, and it runs templates |
| `@error`, `@error("...")`, `@from`, `@source` | Adds `Display`, `Error` and `From` impls | Yes | The message is a template over payload members, not a compile-time value; `transparent` is a marker |
| `@suffix` | `250ms` calls `ms(250)` | Yes, one lookup | Literal resolution must know which functions are suffixes |
| A test-runner marker | The runner finds marked functions | No | The runner reads the program database; only the target check is needed |

So the minimum is: the general decorator mechanism, one **lang
decorator** for suffixes, and the two existing intrinsics. The mechanism
itself has one root the compiler knows, plus the std target types. The
root is a trait in options 1 and 2, a keyword in option 5, and a root
decorator in option 4.

A lang decorator is an ordinary std declaration that the compiler
recognizes. Two ways to recognize it:

| Way | Example | Precedent | Cost |
| --- | --- | --- | --- |
| By qualified name | The compiler knows `std.ops.Suffix` | Java `java.lang.Override`, Kotlin `kotlin.jvm.JvmStatic`, Dart `override` | A std rename is a compiler change |
| By a lang marker | `@lang("suffix")` on the std declaration | Rust `#[lang = "..."]` | `@lang` is itself an intrinsic, and must be closed to user packages |

`@derive` and `@error` cannot become ordinary values without changing
their spelling. `@derive(Eq)` passes a trait, and hd has no trait values.
`@error("not found: $path")` interpolates `path`, which is not in scope at
compile time. Making `@error` a name would also reverse Error Conversion
gap 2, which made `transparent`, `from` and `source` unshadowable markers.
Each option below keeps both as intrinsics; question 9 asks whether to
revisit that.

```text
use std.time.{Duration, ms}

delay := 250ms
```

Under every option except option 3, the compiler resolves `ms` as L10
decided. It then checks one fact: `ms` carries a `std.ops.Suffix` value.
The signature rules of L11 come from the decorator's own target
declaration, not from compiler rules.

## Option 1: A Decorator Trait With Target Types

The owner's tentative model, refined. A decorator is a value whose type
implements `std.decorator.Decorator[Target]` once per target it accepts.
`Target` is one of a small family of std target types. The compiler builds
the target type of each decorated declaration and asks ordinary trait
resolution whether the value's type implements `Decorator` for it.

```text
pub trait Decorator[Target]

pub data OnFn[F]: pass
pub data OnData[T]: pass
pub data OnField[F]: pass
pub data OnTrait: pass
```

The full family would be `OnFn[F]`, `OnData[T]`, `OnEnum[T]`,
`OnNewtype[T]`, `OnField[F]`, `OnParam[P]`, and the kind-only `OnVariant`,
`OnTrait`, `OnImpl` and `OnMethod`. `F` is the declaration's function type,
`T` the declared type, and `P` a parameter's type.

**U1, the suffix.** "Any output" is a type parameter of the impl, not
`Any`. Trait resolution then unifies `fn(i64) -> O` with
`fn(i64) -> Duration`, with no variance question. The sugar `fn(i64) -> O`
has the empty row and does not suspend. So L11's "no providers, never
suspends, no type parameters" follow from the pattern, with no extra rules.

```text
use std.decorator.{Decorator, OnFn}

pub data Suffix: pass

pub fn suffix() -> Suffix:
    Suffix {}

impl[O] Decorator[OnFn[fn(i64) -> O]] for Suffix
impl[O] Decorator[OnFn[fn(f64) -> O]] for Suffix
```

A suffix over `i32` or `u8` needs one more impl line each. A numeric
trait, the idea logged with L11, would reduce that to one bounded line.

**U4, the test-runner marker.** A bound in the impl states "the result
implements `Termination`", as Rust's `#[test]` rule does.

```text
use std.decorator.{Decorator, OnFn}
use std.process.Termination

pub data BenchMarker: pass

pub fn bench() -> BenchMarker:
    BenchMarker {}

impl[T < Termination] Decorator[OnFn[fn(mut Bencher) -> T]] for BenchMarker
```

**U5, library facts.** `rename` accepts any field, variant or parameter.
`max_len` accepts only string fields, which covers the member-type part of
the open fact check hook.

```text
use std.decorator.{Decorator, OnField, OnVariant, OnParam}

pub data Rename:
    pub name: string

pub fn rename(name: string) -> Rename:
    Rename { name: name }

impl[F] Decorator[OnField[F]] for Rename
impl Decorator[OnVariant] for Rename
impl[P] Decorator[OnParam[P]] for Rename

pub data MaxLen:
    pub value: i64

impl Decorator[OnField[string]] for MaxLen
```

**All uses.** `@derive` and `@error` are unchanged intrinsics.
`decorator-target` is a proposed diagnostic name.

```text
use std.ops.suffix
use std.testing.{bench, Bencher}
use std.time.Duration

@suffix()
pub fn ms(count: i64) -> Duration:
    Duration::milliseconds(count)

@suffix()  # error: decorator-target
pub fn px(label: string) -> Px:
    Px(0)

@bench()
fn parse_large(b: mut Bencher) -> void:
    pass

@derive(Eq, Show)
data Point:
    x: i64

@error
enum LoadError:
    @error("cannot read $path")
    Read(path: string)
```

`@suffix()` is written with parentheses because a bare `@suffix` would name
the function value, not a `Suffix`; question 10 covers bare markers.

| Rules | Change |
| --- | --- |
| Added | `std.decorator` (one marker trait, about ten target types); one check per decorator: `Decorator[target]` must hold; the lang decorator `std.ops.Suffix` |
| Removed | `decorator-not-annotator`; L11's parameter, type-parameter, provider and suspension rules (now std impls); the member-type part of the fact check hook |
| Soundness | Resolution is the existing trait solver; the orphan rule keeps a decorator's targets in its own package |
| Interaction | Facts and templates unchanged; member lines are checked against the member's target; package interfaces carry the impls as ordinary impls |

## Option 2: A Check Function Over Shapes

A decorator's type implements `std.decorator.Decorator` with one method,
`check`, that the compiler runs at compile time on the target's shape. The
target check is ordinary hd code, like a Java annotation processor or a
Scala 3 macro annotation that only reports errors.

```text
use std.decorator.{Decorator, Target}
use std.annotation.FnShape

pub data Suffix: pass

impl Decorator for Suffix:
    fn check(self, target: Target) -> Result[void, string]:
        match target:
            .Function(f) => one_number(f)
            _ => .Err("a suffix goes on a function")

fn one_number(f: FnShape) -> Result[void, string]:
    if f.params.len() == 1 && f.params[0].param_type.is_number() && !f.suspending:
        .Ok()
    else:
        .Err("a suffix takes one number and never suspends")
```

Uses look as in option 1. `Target` is an enum over `FnShape`, `DataShape`,
`EnumShape`, `FieldShape` and new shapes for traits, impls and methods.

| Rules | Change |
| --- | --- |
| Added | `std.decorator` with `Target`; compile-time evaluation of `check` on shapes; `TraitShape`, `ImplShape` and `MethodShape`; `TypeShape` comparison helpers |
| Removed | `decorator-not-annotator`; L11's signature rules; the whole fact check hook, including cross-member checks |
| Soundness | Checks are code: a `check` that panics or loops needs the compile-time evaluator's limits, still open as "plan constants" |
| Interaction | The most flexible; also the closest to reflection, because user code walks declaration structure at compile time |

The target is not visible in any signature, so an editor cannot offer
only the decorators that fit a position. Error text is the library's own
string.

## Option 3: A Closed Standard Set

The radical simplification. A decorator stays any value, as today, and may
go on any target D1 allows. The language checks targets only for a closed
list of std decorators, and each such check is a spec rule. User decorators
are unchecked data for their readers.

```text
use std.time.Duration

@suffix
pub fn ms(count: i64) -> Duration:
    Duration::milliseconds(count)

@derive(Eq, Show)
data Point:
    x: i64

@error
enum LoadError:
    @error("cannot read $path")
    Read(path: string)

@bench
fn parse_large(b: mut Bencher) -> void:
    pass

@route("/users")
fn list_users() -> string:
    "[]"

@route("/users")
data Oops: pass
```

`@route` on a data type is accepted: nothing says where `Route` belongs.
`@suffix` and `@bench` are intrinsics like `@derive` and `@error`, each with
spec rules for its targets and signature.

| Rules | Change |
| --- | --- |
| Added | One target rule per std intrinsic (`@suffix`, `@bench`, later `@deprecated`, ...); `decorator-not-annotator` removed |
| Removed | Nothing else |
| Soundness | Unchanged: user decorators are data only |
| Interaction | Facts and templates unchanged; `shape_of(f).metadata[M]()` works as above |

This is Rust's and Go's model. It drops D2 for user decorators, and it
grows the intrinsic list with every std marker, against D5.

## Option 4: A Root Meta-Decorator, `@annotate`

The owner's idea, added on 2026-09-28. One root decorator, `@annotate`,
marks a type as a decorator and lists its targets. It plays the part of
Java's `@Target` on an `@interface`, or C#'s `[AttributeUsage]`.
`@annotate` is the one root the compiler knows, and every other decorator
is defined through it.

```text
@annotate[O](OnFn[fn(i64) -> O], OnFn[fn(f64) -> O])  # hypothetical syntax
pub data Suffix: pass

@annotate[T < Termination](OnFn[fn(mut Bencher) -> T])  # hypothetical syntax
pub data BenchMarker: pass

@annotate[F, P](OnField[F], OnVariant, OnParam[P])  # hypothetical syntax
pub data Rename:
    pub name: string

@annotate(OnField[string])  # hypothetical syntax
pub data MaxLen:
    pub value: i64
```

Uses look exactly as in option 1. `@derive` and `@error` stay intrinsics.

**Checking kinds and signatures.** In the simplest reading, each listed
target means one impl of a sealed `Decorator` trait. So
`@annotate[O](OnFn[fn(i64) -> O])` means option 1's
`impl[O] Decorator[OnFn[fn(i64) -> O]] for Suffix`. Kind and signature
checks are then the same trait resolution as option 1, with the same
target types. They accept and reject the same programs, including generic
targets and bounds.

**Does it reduce intrinsics?** Not compared with option 1. Both need the
same compiler knowledge: the target types, the `Suffix` lang decorator,
and one root. Option 1's root is a lang trait with ordinary impl syntax.
Option 4's root is an intrinsic decorator with its own argument grammar,
like `@derive`. Option 4 does save an intrinsic compared with options 3
and 5: it replaces option 5's `decorator` keyword, and option 3's
per-marker intrinsics.

**Bootstrapping.** The root cannot describe itself in a useful way. Its
arguments are types, so `@annotate` is not a value, and no std declaration
of it can carry a checked `@annotate` line. Java has the same fixed point:
`@Target` is annotated with `@Target(ANNOTATION_TYPE)`, which only
documents what javac already knows. In hd the root's own rule, "only on a
data type or enum", would be a spec rule, as `@derive`'s rules are. A
self-description would be documentation:

```text
@annotate[T](OnData[T], OnEnum[T])  # hypothetical syntax
pub data Annotate: pass
```

**Compared with option 1.**

| Point | Option 1, trait opt-in | Option 4, `@annotate` |
| --- | --- | --- |
| Checking | Trait resolution | The same, through generated impls |
| Where the targets are | Any impl the orphan rule allows in the type's package | On the declaration, in one place |
| Binding "any output" | `impl[O]`, existing syntax | `@annotate[O](...)`, a new binder on a decorator line |
| Bounds (`T < Termination`) | Existing impl bounds | The same binder grammar |
| Arguments | Not applicable | Types, not values: a grammar exception like `@derive` |
| Spelling today | Parses | `annotate` is a reserved word, so `@annotate` is a `syntax-error` |
| Root | One lang trait | One intrinsic decorator |
| Reading targets | Tools read impls | Tools read one decorator line |
| Undeclared values | Need a separate rule (question 6) | A type without `@annotate` is plainly not a decorator |

Option 1 can gain option 4's locality with one rule: a `Decorator` impl
must be declared in the module of its decorator type. That keeps all
targets beside the type with no new syntax.

**A kind-only variant.** If `@annotate` took kind values instead of types,
its argument would be an ordinary value, and no binder would be needed.
This is exactly Java's and C#'s model. It drops the signature half of D2,
so the suffix check would be a spec rule again. With a non-keyword name,
it parses today:

```text
@decorator(on=[.Function, .Field])
pub data Route:
    path: string
```

**Pitfalls specific to option 4.**

1. `annotate` is reserved for `annotate Target:` blocks. The root needs a
   grammar exception, another name such as `@decorator`, or the removal of
   `annotate` blocks.
2. Its arguments are types, so it adds a second decorator with its own
   argument grammar beside `@derive`.
3. Free type names such as `O` need an explicit binder. Guessing that an
   unknown name is a parameter would infer a declaration's generics, which
   the owner rules out.
4. `Any` is still not a wildcard, as in option 1.
5. Several `@annotate` lines on one type would break the M25 duplicate
   rule, so all targets must share one line and one binder list.
6. The root's own targets are a spec rule, not a checked declaration.

## Option 5: A Decorator Declaration With Target Patterns

Java's `@interface`, Kotlin's `annotation class` and C#'s `AttributeUsage`,
with TypeScript-style signature patterns added. A `decorator` declaration
names the decorator, its fields, and a list of target patterns. The
compiler generates a data type and a constructor of that name, and matches
each use against the patterns.

```text
use std.process.Termination
use std.time.Duration

pub decorator suffix on fn(i64) -> _, fn(f64) -> _  # hypothetical syntax
pub decorator bench[T < Termination] on fn(mut Bencher) -> T  # hypothetical syntax
pub decorator rename(name: string) on field, variant, param  # hypothetical syntax
pub decorator max_len(value: i64) on field[string]  # hypothetical syntax

@suffix
pub fn ms(count: i64) -> Duration:
    Duration::milliseconds(count)

@bench
fn parse_large(b: mut Bencher) -> void:
    pass
```

`@derive` and `@error` stay intrinsics. A reader writes
`facts.find[rename]()`, since the declaration also declares the type.

| Rules | Change |
| --- | --- |
| Added | A `decorator` declaration; a pattern language (kinds, `_`, pattern variables with bounds, member types); a generated type and constructor per declaration; the lang decorator `suffix` |
| Removed | `decorator-not-annotator`; L11's signature rules; `annot.metadata.any-value`, since a decorator must now be declared |
| Soundness | Matching is a new relation beside assignability and trait resolution, and must be specified |
| Interaction | Two kinds of fact appear: declared decorators and ordinary values in member lines, unless member lines also require declared ones |

Bare markers such as `@suffix` fall out naturally, as in Java and Kotlin.
The price is a second small type language: `_`, `field[string]` and the
bounds repeat what function types and generics already express.

## Pitfalls

### Generic Targets

Does `fn same[T](x: T) -> T` match `fn(i64) -> O`?

```text
@suffix()
fn same[T](x: T) -> T:
    x
```

| Answer | Meaning | Precedent |
| --- | --- | --- |
| Rigid parameters | Build `OnFn[fn(T) -> T]` with `T` rigid, as when checking the function's own body; `i64` never unifies with `T`, so no match | How every generic body is checked |
| Some instantiation fits | Match when some `T` makes it fit, here `T = i64` | TypeScript's inference on assignment |
| Kind only | Generic functions accept only decorators that take any function | Rust's `#[test]` rejects generics outright |

With rigid parameters, `impl[F] Decorator[OnFn[F]]` still matches a
generic function, so `@deprecated` works on one. "Some instantiation fits"
would accept `same` as a suffix, and `5same` would then need inference.

### `Any` Is Not A Wildcard

The tentative `FnTarget[fn(i64) -> Any]` does not match
`fn(i64) -> Duration` under hd's rules. Function types are covariant in the
output, but `Duration -> Any` builds a trait value, which
[`types.variance.repr.excluded`](../spec/04-type-system.md#r-types.variance.repr.excluded)
rules out of variance. Option 1 therefore writes "any output" as an impl
type parameter (`impl[O] ... fn(i64) -> O`), which unifies. Option 5
needs its own `_`.

### Targets Without A Type

`std.structure` and the shapes cover data types, enums and functions only.
Traits, impls, methods, variants and modules have no type to put in a
target.

| Target | Suggested first form | Later refinement |
| --- | --- | --- |
| Trait | Kind only (`OnTrait`) | The trait as a dynamic trait type, where it is one |
| Impl | Kind only (`OnImpl`) | Trait and target type, `OnImpl[Tr, T]` |
| Trait or impl method | Kind only (`OnMethod`) | The method's function type with a rigid `Self` |
| Variant | Kind only (`OnVariant`) | The payload as a tuple type |
| Newtype | `OnNewtype[T]` | None needed |
| Module | Waits | Needs a place, as Java's `package-info.java` or Kotlin's `@file:` |

None of these parse today, which is the grammar change D1 implies:

```text
@deprecated("use Render")  # hypothetical syntax
trait Show:
    @deprecated("use render")  # hypothetical syntax
    fn show(self) -> string
```

A module has no declaration
([`module.path.no-declaration`](../spec/10-modules.md#r-module.path.no-declaration)),
so a module decorator needs a new file-level line. No use case U1-U6 needs
one.

### Ordering And Duplicates

Read-only decorators do not compose, so their order has no meaning. It is
kept only so readers see source order. The M25 rule already forbids two
values of one concrete type on one target:

```text
@style(prefix="a")
@style(prefix="b")  # error: duplicate-fact
data Pair:
    left: i64
```

Java's `@Repeatable` and C#'s `AllowMultiple` exist for repeated
annotations. In hd a decorator that needs several entries can take a list.

### Typed Derivation Facts

Today every fact is an unchecked value
([`annot.metadata.any-value`](../spec/14-annotations.md#r-annot.metadata.any-value)).
D2 asks for declared targets. Either every fact type opts in, at one impl
line each in option 1. Or undeclared values keep today's targets, and only
new targets need a declaration. A derivation block's member lines
attach facts too, so they need the same check:

```text
use std.structure.Structure

data Pair:
    left: i64

impl Show for Pair by Structure:
    left += [max_len(3)]
```

Under option 1 with opt-in everywhere, `max_len(3)` on an `i64` member is
rejected, because `MaxLen` implements `Decorator` only for
`OnField[string]`. Cross-member checks, such as "exactly one member is the
id", stay the open fact check hook in every option except option 2.

### Bare Markers

`@error` and `@derive` are written bare because they are intrinsics. An
ordinary marker in options 1 and 2 is a value, so a bare `@suffix` would
name something else. Three answers exist. Write `@suffix()`, as the spec's `@flatten()`
already does. Let a bare fieldless data type name mean its one value. Or
let a bare function name with no required parameters mean a call.
Dart uses a fourth, a named `const`, which hd does not have.

### Evolution

| Change | Option 1 | Option 5 | Option 2 | Option 4 |
| --- | --- | --- | --- | --- |
| Add a target | Add an impl; compatible | Add a pattern; compatible | Accept more in `check`; compatible, but not visible | Extend the `@annotate` line; compatible |
| Remove a target | Remove an impl; breaking, visible in the interface | Breaking, visible | Breaking, invisible | Breaking, visible |
| Refine a kind-only target | New target type; breaking for std | New pattern form | No change | New target type; breaking for std |
| Third party adds a target | Blocked by the orphan rule | Blocked: one declaration | Blocked | Blocked: one declaration |
| A new lang decorator | A std declaration plus one spec rule | Same | Same | Same |

Unknown decorator names stay errors in every option, because a decorator
is resolved like any expression. Rust instead ignores unknown names in its
`diagnostic` namespace so that old compilers accept new code.

## Comparison

| | 1. Trait with target types | 2. Check function | 3. Closed set | 4. Root `@annotate` | 5. Declaration with patterns |
| --- | --- | --- | --- | --- | --- |
| U1 suffix | Two std impl lines plus one lang decorator | A `check` over `FnShape` plus one lang decorator | Intrinsic with spec rules | One `@annotate` line plus one lang decorator | One declaration plus one lang decorator |
| U2 `@derive` | Intrinsic | Intrinsic | Intrinsic | Intrinsic | Intrinsic |
| U3 `@error` | Intrinsic | Intrinsic | Intrinsic | Intrinsic | Intrinsic |
| U4 runner marker | Ordinary; one impl with a `Termination` bound | Ordinary; code | Intrinsic | Ordinary; a bound in the binder | Ordinary; pattern with a bound |
| U5 library facts | Kind and member type checked | Anything checked, including cross-member | Unchecked | Kind and member type checked | Kind and member type checked |
| U6 function facts | Same | Same | `shape_of(f).metadata[M]()` | Same | Same |
| New concepts | One marker trait, about ten target types | Compile-time `check`, three new shapes | None | One root decorator, a binder on its line, the same target types | A declaration form and a pattern language |
| Rules removed | That, L11's signature rules, part of the fact check hook | That, L11's rules, the whole fact check hook | `decorator-not-annotator` | Same as option 5 | That, L11's rules, `annot.metadata.any-value` |
| D2 (declared targets) | Yes | Yes, but not declared, only checked | Std only | Yes | Yes |
| D5 (few intrinsics) | Two intrinsics, one lang trait, one lang decorator | Two intrinsics, one lang trait, one lang decorator | Grows with each std marker | Three intrinsics (`@annotate` added) and one lang decorator | Two intrinsics, one keyword, one lang decorator |
| Reflection surface | None | Compile-time shape walking | None | None | None |
| Soundness | Existing trait solver | Depends on the evaluator's limits | Trivial | Existing trait solver, through generated impls | A new matching relation |
| Agent-writability | One impl per target; errors name the impl | Free-form code; errors are strings | Easy, but misuse is silent | One line; errors name the target | One line; errors name the pattern |
| Human readability | Good once the target types are known | Must read code | Good | Good, all targets at the declaration | Best at the declaration |
| Tooling: which decorators fit here? | Yes, from impls | No | Std only | Yes, from the root line | Yes, from patterns |
| Implementation cost | Low: target construction plus resolution | High: evaluator on shapes | Lowest | Low, plus a grammar exception for type arguments and the binder | Medium: parser, patterns, generated types |
| Evolution | Visible in interfaces | Invisible | Every std marker is a compiler change | Visible in declarations | Visible in declarations |

## Ranking By Design Cost

Added 2026-09-28. The owner set a general design cost order, now in
[AGENTS.md](../AGENTS.md#design-cost-order). From least favored to most
favored: (1) new syntax, (2) a semantic rule exception, (3) a compiler
intrinsic, (4) a core library addition. The table lists each option's
changes by kind. Every option shares two changes: decorator positions
before traits, impls and methods need a grammar change, and `@derive` and
`@error` stay intrinsics.

| Order | Option | 1. Syntax | 2. Rule exceptions | 3. Intrinsics | 4. Library | Costliest kind |
| --- | --- | --- | --- | --- | --- | --- |
| 1st | 1. Trait with target types | none | none, if question 14's placement rule is dropped | one lang decorator (`std.ops.Suffix`), and the compiler builds target types | the `Decorator` trait and about ten target types | 3 |
| 2nd | 2. Check function | none | none | a compile-time evaluator over shapes, and one lang decorator | three new shape types and `check` functions | 3, but it opens the compile-time reflection the owner ruled out |
| 3rd | 3. Closed standard set | none | one target rule in the spec for each std marker | one intrinsic for each std marker | none | 2, and it grows with each marker |
| 4th | 4. Root `@annotate` | a type binder `@annotate[O](...)`, and `annotate` is reserved | type arguments in a decorator line, like `@derive` | the `@annotate` root, and one lang decorator | the target types | 1 |
| 5th | 5. Declaration with patterns | a `decorator` keyword and a pattern language | a new matching relation | one lang decorator | none | 1 |

Effects on the recommendation:

- Option 1 stays first. It is the only option whose costliest change is a
  single intrinsic, and it adds no syntax and no rule exception.
- Question 14's recommended placement rule (target impls must sit in the
  decorator type's module) is a rule exception. Under the cost order it
  should be dropped: the ordinary impl ownership rules already decide
  where a `Decorator` impl may live.
- Option 4 drops from next best to fourth, because it needs new syntax.
  Option 2 ranks second by cost, but it conflicts with the owner's "no
  full compile-time reflection" direction. So in practice the next best
  is option 3, and only if user decorators need no checks.
- Recognizing `std.ops.Suffix` by qualified name (question 7) stays
  preferred over an `@lang` marker. A marker would add both an intrinsic
  and a rule exception (closed to user packages).

## Re-Evaluation After M26

Added 2026-09-28, after Typed Derivation M26 and the design cost order.

**What M26 changes.** `annotate` is no longer reserved, so option 4's
name clash is gone. Nothing else about the options changes. M26's
trait-less block `impl User by Structure:` writes facts through member
lines, and those facts need the same target check as `@` facts. Under
every option, a member line is checked as if its value were written as `@`
on that member.

**What option 4 still needs.** The decorator grammar is
`decorator_line = "@", ( derive_decorator | closed_expression )`. So
`@annotate(OnField[string])` needs its own production, just as `@derive`
has `derive_decorator`: `OnField[string]` and `fn(i64) -> O` are types,
not expressions. That production also has to accept full types (function
types included), not only the qualified names `derive_decorator` takes,
plus a binder such as `[O]`. So option 4 is a new syntax production of the
same kind as `derive_decorator`, plus one intrinsic root. Its costliest
change is still syntax, level 1. The `@name(...)` form looks the same as
today, but its argument grammar is new.

A variant that avoids the binder: `@annotate` accepts only concrete
targets, and a decorator with a generic output uses option 1's impl form.
Then option 4 is a partial sugar over option 1, which means two ways to
write the same thing.

**Order after M26 (unchanged):**

| Order | Option | Costliest change |
| --- | --- | --- |
| 1st | 1. Trait with target types | intrinsic (target types built by the compiler, `std.ops.Suffix`) |
| 2nd | 2. Check function | intrinsic, but it opens the compile-time reflection the owner ruled out |
| 3rd | 3. Closed standard set | rule exception per std marker |
| 4th | 4. Root `@annotate` | syntax: a `derive_decorator`-like production that takes types and a binder |
| 5th | 5. Declaration with patterns | syntax: a keyword and a pattern language |

**Recommendation (unchanged): option 1.** It needs no syntax and no rule
exception. The owner's readability goal, having every target next to the
type, is met by convention: write the `Decorator` impls directly under the
decorator type, as the examples here do. That convention is style, not a
rule, so it adds no rule exception.

## Recommendation

> **Update 2026-09-28.** Typed Derivation M26 removes the `annotate
> Target:` block, so `annotate` is no longer reserved and a root
> `@annotate` has no clash with a keyword.


**Recommendation: option 1, a decorator trait with target types,** with
these refinements:

1. "Any" in a target is an impl type parameter, not `Any`, so matching is
   plain trait resolution and needs no new relation.
2. Generic functions are checked with rigid type parameters.
3. Traits, impls, methods and variants start with kind-only targets;
   module decorators wait.
4. `std.ops.Suffix` is the one lang decorator, recognized by qualified
   name, with no `@lang` marker.
5. `@derive` and `@error` stay intrinsics, because their arguments are not
   values.
6. Function-level decorators become `FnShape` metadata, so user code reads
   them through `shape_of(f).metadata[M]()`, without enumeration.
7. A test-runner marker needs no compiler knowledge beyond the target
   check.

Why:

- It reuses traits, function types and generics, which hd already has, and
  follows TypeScript's proven idea of checking a target by its type.
- Its L11 rules become std code, and its intrinsic list stays at two.
- Targets are visible in package interfaces and to editors.

It gives up value-level and cross-member checks, which stay with the fact
check hook, and Java-style bare markers, which question 10 asks about.

Before the design cost order (see [Ranking By Design Cost](#ranking-by-design-cost)), the next best was option 4. It checks exactly what option 1 checks and
keeps every target beside the type. But it adds an intrinsic with type
arguments, a binder syntax, and a clash with the reserved word `annotate`.
Option 1 can get the same locality from one placement rule (question 14).
Option 5 comes after both, because its pattern language repeats function
types.

## Questions For The Owner

### 1. Which mechanism defines a decorator?

Effect: decides whether targets are trait impls, a new declaration, code,
or a closed std list.

- **A.** Option 1, `Decorator[Target]` impls with std target types.
- **B.** Option 2, a compile-time `check` over shapes.
- **C.** Option 3, a closed std set; user decorators unchecked.
- **D.** Option 4, a root `@annotate` line on the decorator type, meaning
  the same impls as A.
- **E.** Option 5, a `decorator` declaration with target patterns.

**Recommendation: A.** D checks the same things but needs a
`derive_decorator`-like production that takes types and a binder;
question 14 gives A D's locality.

```text
use std.decorator.{Decorator, OnFn}

pub data Suffix: pass

impl[O] Decorator[OnFn[fn(i64) -> O]] for Suffix
```

### 2. How is "any type" written in a target?

Effect: `fn(i64) -> Any` does not match `fn(i64) -> Duration` under
today's variance rules.

- **A.** An impl type parameter: `impl[O] ... fn(i64) -> O`.
- **B.** `Any`, with a new "matches" relation that ignores representation.

**Recommendation: A.** It needs no new relation.

```text
use std.decorator.{Decorator, OnFn}

impl[O] Decorator[OnFn[fn(f64) -> O]] for Suffix
```

### 3. Does a generic function match a signature target?

Effect: decides whether `fn same[T](x: T) -> T` can carry `@suffix`.

- **A.** Checked with rigid parameters: it matches only targets that fit
  every `T`, such as `impl[F] Decorator[OnFn[F]]`.
- **B.** It matches when some instantiation fits.
- **C.** Generic functions take only kind-only decorators.

**Recommendation: A.** It is how generic bodies are already checked.

```text
@suffix()
fn same[T](x: T) -> T:
    x
```

### 4. What targets do traits, impls, methods and variants get?

Effect: they have no type in `std.structure`, so they need their own
target types.

- **A.** Kind-only targets (`OnTrait`, `OnImpl`, `OnMethod`, `OnVariant`)
  first, refined later.
- **B.** Typed targets from the start, such as `OnImpl[Tr, T]` and
  `OnMethod[F]` with a rigid `Self`.

**Recommendation: A.** No use case needs more yet.

```text
use std.decorator.{Decorator, OnTrait}

impl Decorator[OnTrait] for Deprecated
```

### 5. Do modules take decorators now?

Effect: hd has no module declaration, so a module decorator needs a new
file-level line.

- **A.** Wait until a use case needs one.
- **B.** Add a file-level form now, like Kotlin's `@file:`.

**Recommendation: A.** No use case U1-U6 needs it.

```text
use std.time.Duration
```

### 6. Must every fact type opt in?

Effect: today any value is a fact on data types, enums, fields, variants
and parameters, unchecked.

- **A.** Every decorator value's type must implement `Decorator` for its
  target, including today's facts and member lines.
- **B.** Undeclared values keep today's targets, unchecked; new targets
  need the opt-in.

**Recommendation: A.** One rule for every target, and misplaced facts are
caught. It reverses `annot.metadata.any-value`, with D2 as the reason.

```text
use std.decorator.{Decorator, OnField}

impl[F] Decorator[OnField[F]] for Rename
```

### 7. How does the compiler recognize a lang decorator?

Effect: decides how `@suffix` gets its compiler meaning.

- **A.** By qualified name, such as `std.ops.Suffix`.
- **B.** By a `@lang("suffix")` marker on the std declaration.

**Recommendation: A.** Java, Kotlin and Dart do this, and it adds no
intrinsic.

```text
use std.time.{Duration, ms}

delay := 250ms
```

### 8. Does `@suffix` become an ordinary std decorator?

Effect: L11's signature rules move from spec prose to std impls, and the
compiler keeps one check at the literal.

- **A.** Yes: a std `Suffix` value with target impls, recognized as a lang
  decorator.
- **B.** No: it stays the L11 intrinsic.

**Recommendation: A.** It removes four spec rules and one intrinsic.

```text
use std.ops.suffix
use std.time.Duration

@suffix()
pub fn ms(count: i64) -> Duration:
    Duration::milliseconds(count)
```

### 9. Do `@derive` and `@error` stay intrinsics?

Effect: their arguments are trait names and message templates, which are
not values.

- **A.** Keep both as intrinsics, as decided.
- **B.** Respell them so their arguments are values, then make them lang
  decorators.

**Recommendation: A.** B would reopen Error Conversion gap 2 and change
`@derive`'s spelling, with no rule removed.

```text
@error
enum LoadError:
    @error("cannot read $path")
    Read(path: string)
```

### 10. How is a marker with no arguments written?

Effect: under option 1 a bare `@suffix` would name the function `suffix`,
not a `Suffix` value.

- **A.** Write `@suffix()`, as the spec already writes `@flatten()`.
- **B.** A bare name of a fieldless data type means its one value.
- **C.** A bare name of a function with no required parameters means a
  call.

**Recommendation: A.** It needs no new rule; B or C can come later.

```text
use std.testing.{bench, Bencher}

@bench()
fn parse_large(b: mut Bencher) -> void:
    pass
```

### 11. Do inherent methods take decorators?

Effect: D1 lists trait methods; without inherent methods, a method in
`impl Point:` cannot be marked `@deprecated`.

- **A.** Yes, with the same `OnMethod` target as trait methods.
- **B.** Trait methods only.

**Recommendation: A.** One method target for both.

```text
data Point:
    x: i64
```

### 12. Can user code read function-level decorators?

Effect: decides whether U6 works without a tool.

- **A.** Yes, as `FnShape` metadata through `shape_of(f).metadata[M]()`,
  one named function at a time.
- **B.** No; only tools read them.

**Recommendation: A.** It reuses `ShapeMetadata` and allows no
enumeration.

```text
fn route_of() -> Route?:
    shape_of(list_users).metadata[Route]()
```

### 13. May a decorator repeat on one target?

Effect: M25 forbids two values of one concrete type on one target.

- **A.** Keep it; a decorator that needs several entries takes a list.
- **B.** Allow a decorator type to opt into repetition, like Java's
  `@Repeatable`.

**Recommendation: A.** No use case needs repetition.

```text
@style(prefix="a")
data Pair:
    left: i64
```

### 14. Where are a decorator's targets written?

Effect: decides whether a reader finds every target of `Suffix` beside
its declaration. This is the main gain of option 4.

- **A.** Option 1 impls, which must be in the decorator type's module.
- **B.** Option 1 impls anywhere the orphan rule allows.
- **C.** One root line on the type, as option 4 proposes. It then needs a
  name other than the reserved `annotate`, or a grammar exception, plus a
  binder for `O`.

**Recommendation: A.** It gives option 4's locality with one placement
rule and no new syntax.

```text
use std.decorator.{Decorator, OnFn}

pub data Suffix: pass

impl[O] Decorator[OnFn[fn(i64) -> O]] for Suffix
```

## Sources

- Java Language Specification SE 21, 9.6 Annotation Interfaces and 9.6.4 Predefined Annotation Interfaces: <https://docs.oracle.com/javase/specs/jls/se21/html/jls-9.html#jls-9.6>
- Java `@Target` and `ElementType` (`@Target` itself carries `@Target(ANNOTATION_TYPE)`): <https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/annotation/Target.html>
- Java annotation processing, `Processor` and `Filer` (new files only): <https://docs.oracle.com/en/java/javase/21/docs/api/java.compiler/javax/annotation/processing/Processor.html>
- Java package annotations in `package-info.java`, JLS 7.4.1: <https://docs.oracle.com/javase/specs/jls/se21/html/jls-7.html#jls-7.4.1>
- C# attributes: <https://learn.microsoft.com/en-us/dotnet/csharp/advanced-topics/reflection-and-attributes/>
- C# `AttributeUsage` and compiler-interpreted attributes: <https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/attributes/general>
- C# source generators (additive only): <https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/source-generators-overview>
- Kotlin annotations, `@Target`, use-site targets and `@file:`: <https://kotlinlang.org/docs/annotations.html>
- Kotlin Symbol Processing (generates new files, cannot modify existing code): <https://kotlinlang.org/docs/ksp-overview.html>
- TypeScript 5.0 decorators (typed; no parameter decorators): <https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-0.html#decorators>
- TypeScript 5.2 decorator metadata: <https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-2.html#decorator-metadata>
- TC39 decorators proposal: <https://github.com/tc39/proposal-decorators>
- Python PEP 318, function decorators: <https://peps.python.org/pep-0318/>; PEP 3129, class decorators: <https://peps.python.org/pep-3129/>; PEP 612, `ParamSpec`: <https://peps.python.org/pep-0612/>
- Rust Reference, attributes and tool attributes: <https://doc.rust-lang.org/reference/attributes.html>
- Rust Reference, testing attributes (`#[test]` signature rule): <https://doc.rust-lang.org/reference/attributes/testing.html>
- Rust Reference, procedural macros and derive helper attributes: <https://doc.rust-lang.org/reference/procedural-macros.html>
- Rust Reference, the `diagnostic` tool attribute namespace: <https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-diagnostic-tool-attribute-namespace>
- Rust unstable book, lang items: <https://doc.rust-lang.org/unstable-book/language-features/lang-items.html>
- Swift SE-0389, attached macros (roles; expansions only add): <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0389-attached-macros.md>
- Swift SE-0258, property wrappers: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0258-property-wrappers.md>
- Swift Testing `@Test` macro: <https://developer.apple.com/documentation/testing/test(_:_:)>
- Zig language reference, `comptime`, `@typeInfo` and `test`: <https://ziglang.org/documentation/master/>
- Scala tour, annotations: <https://docs.scala-lang.org/tour/annotations.html>
- Scala 3 reference, macro annotations (experimental): <https://docs.scala-lang.org/scala3/reference/metaprogramming/macro-annotations.html>
- Dart metadata: <https://dart.dev/language/metadata>
- Dart `package:meta` `Target` and `TargetKind`: <https://pub.dev/documentation/meta/latest/meta_meta/Target-class.html>
- Dart blog, "An update on Dart macros & data serialization" (macros stopped, 2025-01-29): <https://dart.dev/blog/an-update-on-dart-macros-data-serialization>
- Go `reflect.StructTag`: <https://pkg.go.dev/reflect#StructTag>; compiler directives: <https://pkg.go.dev/cmd/compile#hdr-Compiler_Directives>

## Parse Log

Every `text` block was parsed with the reference parser on 2026-09-28.
Parsing checks syntax only; no block is claimed to type-check. Names such
as `Bencher`, `Route`, `Px`, `std.decorator` and `std.ops.suffix` are
placeholders for this record. `# error:` comments state the proposed or
existing diagnostic, not a parser result.

| Block | Section | Result |
| --- | --- | --- |
| 1 | What hd Has Today | parses |
| 2 | Who Reads A Decorator | parses |
| 3 | Minimum Compiler Knowledge | parses |
| 4 | Option 3 | parses |
| 5 | Option 1, `std.decorator` | parses |
| 6 | Option 1, U1 | parses |
| 7 | Option 1, U4 | parses |
| 8 | Option 1, U5 | parses |
| 9 | Option 1, all uses | parses |
| 10 | Option 5 | `syntax-error` at line 4, the first line marked `# hypothetical syntax` |
| 11 | Option 2 | parses |
| 12 | Option 4, targets | `syntax-error` at line 1, marked `# hypothetical syntax`: `annotate` was a reserved word. Parses since Typed Derivation M26 was applied (2026-09-28) |
| 13 | Option 4, bootstrapping | `syntax-error` at line 1, marked `# hypothetical syntax`. Parses since Typed Derivation M26 was applied (2026-09-28) |
| 14 | Option 4, kind-only variant | parses |
| 15 | Generic Targets | parses |
| 16 | Targets Without A Type | `syntax-error` at line 2 and `decorator-not-top-level` at line 3, both on lines marked `# hypothetical syntax` |
| 17 | Ordering And Duplicates | parses |
| 18 | Typed Derivation Facts | parses |
| 19-32 | Questions 1-14 | parse |
