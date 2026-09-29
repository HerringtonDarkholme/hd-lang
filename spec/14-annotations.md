# Annotations

Status: language specification draft.

## Terminology

- **Member metadata** is the ordered list of values attached to a data
  field, an enum variant, or a parameter.
- A **shape** is a compiler-provided runtime value that describes the
  structure of a declaration or type.
- A **coherence slot** is one `(trait, concrete target)` pair over the
  resolved package graph.

Annotations attach typed values to declarations and expose declaration
structure as shape values. They do not alter a declaration's name, type,
behavior, or visibility, and they do not discover or register runtime
objects automatically.

The design principles are:

1. attached values are ordinary typed values;
2. derived behavior is requested explicitly, with `@derive` or a derivation
   block;
3. runtime information is produced explicitly when requested;
4. the semantic foundation is ordinary traits, implementations, values, and
   compiler-provided shape values.

An ordinary decorator attaches a value to the item or member it precedes:
a type-level fact before a data type or enum, or member metadata before a
field, variant, or parameter. A trait-less derivation block attaches the
same values away from the declaration.

The compiler lowers attached values to shape metadata
construction. This is not runtime wrapper execution. `@derive(Trait, ...)` generates
ordinary trait implementations, either through a compiler intrinsic or
through a trait's template, as [Typed Derivation](#typed-derivation)
defines. An ordinary decorator must not silently change a declaration's
name, type, behavior, or visibility.

> **Note.** Information derived from a type is an ordinary trait with an
> associated function, derived through a template, as in
> `trait Validate: fn validator() -> Validator` read with
> `User::validator()`. The earlier facet protocol (`Annotation`, `Annotate`,
> the annotator traits, `AnnotationRef`, and `annotate Facet for Target`)
> was removed by
> [Typed Derivation decision 10](../future-work/TYPED_DERIVATION.md#owner-decisions).
> Building such a value once per type, with deferred `Ref[T]` references for
> recursive types, is the standard library's
> [derived-function cache](../future-work/STDLIB.md#derived-function-cache).

## Common Shape Representation

The compiler exposes declaration structure as ordinary runtime shape values
through two intrinsic functions in the prelude, `shape` and `shape_of`:

```text
shape[User]()                        # DataShape specialized for User
shape[User]().fields.email           # FieldShape
shape[JobStatus]()                   # EnumShape specialized for JobStatus
shape[JobStatus]().variants.Active   # VariantShape
shape_of(get_user)                   # FnShape
```

The common representation includes at least:

- `TypeShape` for a concrete type;
- `DataShape` and ordered `FieldShape` values;
- `EnumShape`, ordered `VariantShape` values, and payload `FieldShape` values;
- `FnShape` and ordered `ParamShape` values;
- names, positions, declared runtime type descriptors, documentation, and
  attached member metadata.

The shape types below are not parameterized by the reflected declaration: the
type is `DataShape`, not `DataShape[S]`. Only a direct `shape[T]()` request
adds the typed member access described in
[Shape Intrinsics](#shape-intrinsics).

Shapes expose structure for generic handling but do not permit mutation of the
source declaration. Every shape provides a stable declaration identity, source
name, qualified name, source position, documentation string, and declaration
kind. `FieldShape`, `VariantShape`, and `ParamShape` additionally provide their
zero-based declaration position and declared `TypeShape`. Fields, variants,
and parameters expose their [member metadata](#member-metadata), and a
function exposes the values that its [decorators](#prefix-decorators)
attach.

`DataShape`, `EnumShape`, and `FnShape` contain their ordered direct
members. `FnShape` additionally exposes its result type, whether it is
suspending, and its normalized unordered requirement row; each `ParamShape`
records whether a default is declared. Promoted embedded members are not
duplicated as direct fields.

The following declarations are the normative shape surface. `DeclarationId`
is an opaque, equality-comparable identity allocated by the compiler. A
`SourcePosition` identifies the beginning of the reflected declaration or
member. `position` on a member shape is its zero-based declaration ordinal;
source coordinates are kept separately in `source`.

```text
data SourcePosition:
    file: string
    line: i32
    column: i32

enum DeclarationKind:
    Data
    Enum
    Function
    Field
    Variant
    Parameter

enum PrimitiveKind:
    Bool
    Signed(bits: i32)
    Unsigned(bits: i32)
    Float(bits: i32)
    Char
    String
    Void
    Never

enum TypeShape:
    Primitive(kind: PrimitiveKind)
    Optional(inner: TypeShape)
    List(element: TypeShape)
    Map(key: TypeShape, value: TypeShape)
    Tuple(elements: List[TypeShape])
    Named(decl: DeclarationId, args: List[TypeShape])
    Newtype(decl: DeclarationId, base: TypeShape)
    Mut(inner: TypeShape)
    Trait(decl: DeclarationId, args: List[TypeShape])
    Any
    Suspend(result: TypeShape)
    Fn(
        params: List[TypeShape],
        result: TypeShape,
        suspending: bool,
        requirements: List[TypeShape],
    )

data FieldShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    position: i32
    doc: string?
    field_type: TypeShape

data DataShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    doc: string?
    field_list: List[FieldShape]

data VariantShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    position: i32
    doc: string?
    payload: List[FieldShape]

data EnumShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    doc: string?
    variant_list: List[VariantShape]

data ParamShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    position: i32
    doc: string?
    param_type: TypeShape
    has_default: bool

data FnShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    doc: string?
    params: List[ParamShape]
    result: TypeShape
    suspending: bool
    requirements: List[TypeShape]

trait ShapeMetadata:
    fn metadata[reified M](self) -> M?
```

Every concrete shape type implements sealed `ShapeMetadata`; user code cannot
add implementations. `metadata[M]()` performs the one narrow runtime type
lookup supported for heterogeneous member metadata and preserves the
attached value's declared permission. It does not add a general `Any`
downcast; recovering the concrete type of an erased value is the separate
facility in [Runtime Type Identity](09-traits.md#runtime-type-identity), and
a `TypeId` exposes no shape. `TypeShape.is_optional() -> bool` is also a compiler-provided readonly
method and is true exactly for `TypeShape.Optional`.

`TypeShape.Mut` encodes mutable access, `TypeShape.Trait` a dynamic trait
value type with its trait declaration and arguments, `TypeShape.Any` the
`Any` type, and `TypeShape.Suspend` a `Suspend[T]` type with its result.
`TypeShape.Newtype` carries the newtype's own declaration identity beside its
base type.

Shape values are readonly runtime values and may be passed, stored, and
inspected like other composite values.

### Shape Intrinsics

`shape` and `shape_of` are compiler intrinsics declared by `std.annotation`
and re-exported by the prelude. They are ordinary names, not reserved words:
they are called with ordinary call syntax, and the prelude shadowing rule
applies to them. Each use must be the callee of a direct call; using either
name as a function value is an `unknown-shape-target` error. Missing or extra
arguments follow the ordinary call rules.

`shape[T]()` takes exactly one explicit type argument and no value arguments.
`T` is a reified type argument under the rules of
[Generics](04-type-system.md#generics): inside a generic declaration, a type
parameter passed as `T` must itself be `reified`. The result depends on `T`:

- For a data type `D`, the result has the **specialized data shape type** of
  `D`.
- For an enum `E`, the result has the **specialized enum shape type** of `E`.
- For any other type, including a type parameter, a primitive, a collection, a
  tuple, an optional, and an applied generic nominal type such as
  `Box[i32]`, the result is a `TypeShape`.

A specialized shape type is generated by the compiler and cannot be named in
source. The specialized data shape type of `D` has every member of `DataShape`
with the same name and type, plus `fields`: a readonly record with exactly
one member for each direct field of `D`, named after that field and of type
`FieldShape`. `shape[D]().fields.email` is therefore statically checked, and
equals the element of `field_list` at the position of `email`. The
specialized enum shape type of `E` likewise adds `variants`, with one
`VariantShape` member per variant of `E`, equal to the corresponding element
of `variant_list`. Selecting a member the declaration does not have, such as
`shape[User]().fields.deleted`, is an `unknown-shape-target` error. The
`fields` and `variants` records are not collections: they support only member
selection, and iteration uses `field_list` or `variant_list`.

A specialized shape value is assignable to its generic shape type, so
`shape[User]()` can be passed where `DataShape` is expected; the conversion
does not change the value. No other specialized member survives that
conversion.

`shape_of(f)` takes exactly one argument, which must directly name a
module-level function declaration, optionally through a module qualifier or
a `use` import. It returns that function's `FnShape`. The argument is not
evaluated. A closure, a local binding, a parameter, a method, a field access,
a call, or any other function-valued expression is an `unknown-shape-target`
error, as is a name that resolves to a type or a non-function binding.

## Two `annotate` Forms

Values are attached in two forms: a prefix decorator on the declaration or
member, and a member line of a trait-less derivation block for shared
metadata written away from the declaration.

> **Note.** This heading keeps its earlier name so that links to it stay
> valid. The `annotate Target:` block and the reserved word `annotate` were
> removed by
> [Typed Derivation M26](../future-work/TYPED_DERIVATION.md#still-open-after-the-prototype-pass).

### Prefix Decorators

An `@value` line immediately before a module-level data or enum declaration
attaches a type-level fact, as [Facts](#facts) defines. It does not register
or derive anything.

A decorator may precede any item or member, and attaches its value to it.
Items are functions, data types, enums, traits, implementations, and
newtypes. Members are fields, variants, value parameters, and methods:

```text
data Tool:
    name: string

fn tool(name: string) -> Tool:
    Tool { name: name }

@tool("search")
fn search(query: string) -> string:
    query

@"storage"
trait Store:
    @"read"
    fn load(self, @"key" key: string) -> string

@"inherent"
impl Tool:
    @"label"
    fn label(self) -> string:
        self.name

@"id"
type ToolId(i64)
```

1. r[annot.decorator.targets] A decorator may precede an item: a module-level function, data type, enum, trait, implementation, or newtype declaration.
2. r[annot.decorator.members] A decorator may also precede a member: a data field, an enum variant, a value parameter, or a method of a trait or implementation.
3. r[annot.decorator.attach] A decorator before a function, trait, implementation, newtype, or method attaches its value to that declaration and changes nothing else about it.
4. r[annot.decorator.no-locals] A local declaration, a statement, and an expression take no decorator. A decorator there is an error. Error: `decorator-not-top-level`.
5. r[annot.decorator.no-module] A module takes no decorator, because it has no declaration.
6. r[annot.decorator.function-derive] A `@derive(...)` line before a module-level function declaration is an error, reported on the decorator. Error: `decorator-not-annotator`.
7. r[annot.decorator.derive-targets] A `@derive(...)` line before a trait, an implementation, or a method is the same error. Error: `decorator-not-annotator`.
8. r[annot.decorator.duplicate] Two values of one concrete type before one function, trait, implementation, newtype, or method are an error, reported on the later value. Error: `duplicate-fact`.
9. r[annot.decorator.fn-read] Code reads a value attached to a module-level function `f` through `shape_of(f).metadata[M]()`.
10. r[annot.decorator.no-listing] No operation lists the declarations that carry a value, so code cannot enumerate every function with a given decorator.

```text
@derive(Eq)  # error: decorator-not-annotator
fn same(value: i32) -> i32:
    value

@derive(Eq)  # error: decorator-not-annotator
trait Named:
    fn name(self) -> string

@"first"
@"second"  # error: duplicate-fact
fn twice() -> void:
    pass
```

A decorator that names a function with no parameters calls it, so a marker
is written without parentheses:

```text
data Hidden: pass

fn hidden() -> Hidden:
    Hidden {}

@hidden
fn internal_check() -> bool:
    true

marked := shape_of(internal_check).metadata[Hidden]()
```

11. r[annot.decorator.bare-call] In a decorator, a bare name that resolves to a function with no parameters is called, so `@hidden` means `@hidden()`.
12. r[annot.decorator.bare-call.only] The rule applies only to decorators. Elsewhere, the same name stays a function value.

> **Why.** A decorator is a plain value, as in Java, C#, Kotlin, and Dart,
> so one rule covers every place it may go. Whatever reads a value checks
> that it suits its target, so the compiler knows no signatures. Reading
> by one function's shape, never by listing, keeps discovery with tools.

An `@value` line immediately before a named or embedded data field or an enum
variant attaches its value to that member's metadata. For a field
`name: string`, `@max_len(80)` attaches `max_len(80)` to `name`. Member
metadata is also the member's or variant's declaration facts for
[typed derivation](#facts).

For an embedded `Timestamps` field in `Post`, `@flatten()` attaches
`flatten()` to the member `Timestamps`. A generic embedded `Box[T]` keeps
the member name `Box`. Metadata is attached only to the embedded field's own
`FieldShape`; it is not propagated to promoted fields or methods.

Multiple lines retain source order and obey the duplicate rule of
[Member Metadata](#member-metadata). A trait-less derivation block may then
extend or replace a member's decorator values. Decorators attach to
declarations or members, never to type expressions. Declaration decorators
are module-level syntax; local declarations cannot be decorated.

An `@value` prefix on a parameter of a module-level named function attaches
parameter metadata. The prefix may be inline or occupy its own line in a
multiline parameter clause:

```text
fn get_user(
    @description("User identifier")
    id: UserId,
) -> User:
    ...
```

This attaches `description("User identifier")` to the parameter `id`, and
it is visible through that parameter's `ParamShape`. Parameter decorators
are also accepted on the value parameters of methods, including trait
requirements. They are not accepted on closures, receiver parameters, or
local functions.

`@derive` uses the same prefix position but is not an ordinary decorator.
It stays a compiler intrinsic, as `@error` does
([Error Conversion decision 10](../future-work/ERROR_CONVERSION.md#owner-decisions)).
Its arguments are trait names rather than metadata values. The compiler
checks and generates each requested implementation.
[Opting In](#opting-in) defines which traits it accepts.

### Member Metadata

Member metadata is written on the member with `@value` lines, or away
from the declaration in a
[trait-less derivation block](#trait-less-derivation-blocks):

```text
use std.structure.Structure

data User:
    display_name: string

impl User by Structure:
    display_name = [min_len(1), max_len(80)]
```

A trait-less block writes metadata for a data type's fields or an enum's
variants. A function has no derivation block, so parameter metadata is
written only with `@value` on the parameter.

1. r[annot.metadata.places] A field's or variant's metadata is written with `@value` lines on it and with member lines of a trait-less derivation block for its type.
2. r[annot.metadata.params-at-only] Parameter metadata, including a payload parameter's, is written only with `@value` lines on the parameter.
3. r[annot.metadata.list-any] Member metadata and parameter metadata are contextually typed as `List[Any]`.
4. r[annot.metadata.any-value] Any compile-time value may be attached to any item or member; no marker trait is required. Only a fact type's [target kinds](#target-kinds) limit where it goes.
5. r[annot.metadata.eval] Each metadata expression is evaluated once, at compile time, as a [fact expression](#r-annot.fact.eval) is, and under the same [`block_on` ban](#r-annot.fact.no-block-on).
6. r[annot.metadata.duplicate] Two metadata values of one concrete type on one member, variant, or parameter are an error, reported on the later value. Error: `duplicate-fact`.

One member or parameter must not contain two metadata values with the same
concrete type.

Metadata objects are ordinary values. Constructors and helper functions are
equivalent ways to make them:

```text
data MaxLen:
    value: i32

fn max_len(value: i32) -> MaxLen: MaxLen { value: value }
```

The language does not check that a metadata value suits its member's type.
The code that reads the value checks it. A fact type's compile-time check
against its member is the fact check hook in
[Undecided Parts](#undecided-parts).

Reusable compositions are ordinary values or lists, not new language syntax:

```text
let email_metadata: List[Any] = [
    min_len(3),
    max_len(320),
    contains("@"),
]
```

Different concrete metadata types coexist in one `List[Any]`. A member
line may name such a list directly, as `email = email_metadata`, since its
right side is [any list-typed expression](#r-annot.line.right-typed).

### Target Kinds

A fact type may limit the kinds of target that its values attach to, with
the standard `@annotate` decorator:

```text
use std.annotation.annotate

@annotate(.Field)
data MaxLen:
    value: i32

fn max_len(value: i32) -> MaxLen:
    MaxLen { value: value }

data Profile:
    @max_len(80)
    name: string

@max_len(3)  # error: decorator-target-kind
fn greet() -> string:
    "hi"
```

`std.annotation` declares the kinds and the limiting fact type:

```text
pub enum Target:
    Fn
    Data
    Enum
    Newtype
    Field
    Variant
    Param
    Trait
    Impl
    Method

@annotate(.Data, .Enum)
pub data Annotate:
    pub kinds: List[Target]

pub fn annotate(kinds: Target...) -> Annotate:
    Annotate { kinds: kinds }
```

Each target has one kind:

| Rule | Target | Kind |
| --- | --- | --- |
| r[annot.target.kind.fn] Function | a module-level function declaration | `.Fn` |
| r[annot.target.kind.data] Data type | a data type declaration | `.Data` |
| r[annot.target.kind.enum] Enum | an enum declaration | `.Enum` |
| r[annot.target.kind.field] Field | a named or embedded data field, or a variant payload member | `.Field` |
| r[annot.target.kind.variant] Variant | an enum variant | `.Variant` |
| r[annot.target.kind.param] Parameter | a value parameter of a function or method | `.Param` |
| r[annot.target.kind.trait] Trait | a trait declaration | `.Trait` |
| r[annot.target.kind.impl] Implementation | an implementation, including a derivation block | `.Impl` |
| r[annot.target.kind.method] Method | a method or associated function of a trait or implementation | `.Method` |
| r[annot.target.kind.newtype-kind] Newtype | a newtype declaration | `.Newtype` |

1. r[annot.target.declarations] `std.annotation` declares `Target`, `Annotate`, and `annotate`. They are not prelude names, so code imports them, as in `use std.annotation.annotate`.
2. r[annot.target.limit] A data type or enum `F` whose type-level facts include an `Annotate` value limits values of type `F` to targets whose kind that value lists.
3. r[annot.target.limit.kind-error] A value of a limited type attached to a target of any other kind is an error, reported on the decorator or member line that attaches it. Error: `decorator-target-kind`.
4. r[annot.target.unlimited] A type without an `Annotate` fact is not limited: its values may be attached to any target, as `@"note"` may.
5. r[annot.target.recognized] The compiler recognizes `std.annotation.Annotate` by its qualified name. A type of another package named `Annotate` limits nothing.
6. r[annot.target.bootstrap] `Annotate` itself carries `@annotate(.Data, .Enum)`, so an `Annotate` value may be attached only to a data type or an enum.
7. r[annot.target.kind-only] The compiler checks only the kind. Whether a value suits its target's type or signature is checked by the code that reads the value.

> **Why.** Java, C#, Kotlin, and Dart check declared target kinds the
> same way. A signature check belongs to the reader, which knows what it
> needs, so the compiler knows one standard type rather than a pattern
> language for targets.

See also: [Prefix Decorators](#prefix-decorators),
[Literal Suffixes](05-expressions.md#literal-suffixes).

## Grammar

Metadata has no declaration form of its own. Decorator lines are
`decorator_line` in the [Annotations grammar](02-grammar.md#annotations). A
trait-less derivation block is an `impl_decl` with `by` and no `for`, and
its members are `derivation_line`s, as
[Traits And Implementations](02-grammar.md#traits-and-implementations)
defines:

```ebnf
impl_decl = "impl", [ generic_params ], type,
            [ "for", type ], [ "by", identifier ],
            ( NEWLINE
            | ":", NEWLINE, INDENT,
              impl_member, { impl_member }, DEDENT )
            ;
```

## Typed Derivation

**Typed derivation** implements a library's trait for a data type or enum
from the type's members. The library writes the implementation once, as a
template over the compiler-generated `Structure` trait. A type opts in with
`@derive`, and configures the result with facts.

```text
@derive(Encode, Keys)
@style(prefix="user_")
data User:
    id: i64
    @rename("mail")
    email: string
    nickname: string = ""
```

1. r[annot.derive.definition] Typed derivation implements a trait for a data type or enum from the type's members, through the trait's template.
2. r[annot.derive.supplied] The compiler supplies only `Structure`, the handle constants, and the generated `walk`, `describe`, and `build` bodies.
3. r[annot.derive.library] Templates, walkers, describers, sources, and facts are ordinary library code.

> **Why.** The compiler generates one statically typed traversal per type.
> Every format, comparison, or schema is library code over it, so the
> language needs no macro system and no per-library compiler support.

See also: [Derived Implementations](09-traits.md#derived-implementations),
[Typed Derivation design record](../future-work/TYPED_DERIVATION.md).

### Opting In

`@derive(...)` lists the traits a declaration derives.

1. r[annot.derive.opt-in] `@derive(...)` is the only form that creates a derived implementation from a declaration.
2. r[annot.derive.accepted] It accepts the intrinsic comparison traits of [Derived Implementations](09-traits.md#derived-implementations) and any trait that has a [template](#templates).
3. r[annot.derive.means] For a trait `X` with a template, `@derive(X)` before a data type or enum `T` means exactly the derivation block `impl X for T by Structure` with an empty body. A newtype derives through its base type instead, as [Derived Newtypes](09-traits.md#derived-newtypes) defines.
4. r[annot.derive.no-use] `@derive(X)` needs no `use` of `Structure`. The import is needed only where code writes `by Structure`, as [`annot.block.structure-use`](#r-annot.block.structure-use) states.
5. r[annot.derive.other] Any other trait in a `@derive` list is an error, reported on the `@derive` line. Error: `underivable-trait`.
6. r[annot.derive.error-trait] `Error` has neither a template nor an intrinsic derivation, so `@derive(Error)` is an error. Error: `underivable-trait`.
7. r[annot.derive.facts-only] Every other decorator only attaches information: a configuration decorator such as `@style(prefix="user_")` attaches a fact and creates no implementation.
8. r[annot.derive.overlap] Listing a trait in `@derive` and also writing a derivation block for it on the same type is an error, reported on the block. Error: `overlapping-impl`.

```text
use std.error.Error
use std.structure.Structure

@derive(Error)  # error: underivable-trait
enum LoadError:
    Missing

@derive(Show)
data Point:
    x: i64

impl Show for Point by Structure:  # error: overlapping-impl
    x = []
```

> **Note.** An error type uses the separate `@error` intrinsic, as
> [Error Conversion decision 10](../future-work/ERROR_CONVERSION.md#owner-decisions)
> records.

### The `std.structure` Module

The module `std.structure` declares the derivation protocol. The compiler
supplies every body written `pass`:

```text
pub trait Structure:
    fn facts() -> Facts
    fn walk[W < Walker[Self]](self, w: mut W) -> Result[void, W::Error]
    fn describe[D < Describer[Self]](d: mut D) -> Result[void, D::Error]
    fn build[S < Source[Self]](s: mut S) -> Result[mut Self, S::Error]

pub data Member:
    pub name: string
    pub position: i32
    pub facts: Facts
    pub doc: string?
    pub embedded: bool
    pub positional: bool

pub data VariantInfo:
    pub name: string
    pub index: i32
    pub facts: Facts
    pub doc: string?
    pub of_data: bool
    pub shared: List[(string, Any)]

pub data Field[-S, +F]:
    pub info: Member

impl[S, F] Field[S, F]:
    pub fn get(self, s: S) -> F: pass
    pub fn has_default(self) -> bool: pass
    pub fn default(self) -> F?: pass

pub data Variant[S]:
    pub info: VariantInfo

impl[S] Variant[S]:
    pub fn holds(self, s: S) -> bool: pass

pub data Members[S]:
    pub infos: List[Member]

impl[S] Members[S]:
    pub fn end(self) -> Key[S]: pass
    pub fn at(self, position: i32) -> Key[S]: pass
    pub fn find(self, matches: fn(Member) -> bool) -> Key[S]: pass

pub data Key[S]:
    pub info: Member
    pub is_end: bool

pub trait Walker[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, Self::Error]

pub trait Describer[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]

pub trait Source[S]:
    type Error
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], Self::Error]
    fn next(mut self, members: Members[S]) -> Result[Key[S], Self::Error]
    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, Self::Error]
    fn missing[F](mut self, h: Field[S, F]) -> Result[F, Self::Error]
```

1. r[annot.structure.module] The standard module `std.structure` declares `Structure`, `Facts`, `Member`, `VariantInfo`, `Field`, `Variant`, `Members`, `Key`, `Walker`, `Describer`, and `Source`.
2. r[annot.structure.bodies] The compiler supplies every body written `pass` in the declarations above, and the `Facts` type.
3. r[annot.structure.facts-type] `Facts` holds the facts attached to one type, member, or variant, in source order. `facts.find[F]()` returns the fact whose concrete type is `F`, or `.None`.
4. r[annot.structure.find-lookup] `find` performs the same narrow runtime type lookup as `metadata[M]` in [Common Shape Representation](#common-shape-representation), and nothing more.
5. r[annot.structure.members-api] `members.end()` is the end key, `members.at(position)` is the key of the member at that position, and `members.find(matches)` is the key of the first member that `matches` accepts. Each returns the end key when no member fits.
6. r[annot.structure.no-names] The names of `Members`, `Key`, and the handle methods are fixed by these declarations. Further helpers over them are standard-library design, outside this specification.

> **Note.** Standard walkers, describers, and sources, such as the ones a
> `std.json` would use, are library design. The
> [design record](../future-work/TYPED_DERIVATION.md#current-design-full-example-m1-m21)
> shows a complete json library, and
> [STDLIB](../future-work/STDLIB.md) tracks the standard modules.

### The Structure Trait

`Structure` is the compiler-generated view of one type's members.

1. r[annot.structure.sealed] `Structure` is a sealed trait: an `impl Structure for T` outside the standard library is an error. Error: `sealed-trait-implementation`.
2. r[annot.structure.generated] The compiler generates `Structure` for a target only while it instantiates a template for that target. No type has it otherwise.
3. r[annot.structure.per-derivation] Each derivation sees its own `Structure` for the target, which reflects that derivation's facts and omitted members.
4. r[annot.structure.named-positions] `Structure` may be named, as a bound or in a call such as `T::facts()`, only inside a template. Outside one, it may appear only in the `use` declaration that imports it and after `by` in a derivation block's header.
5. r[annot.structure.named-positions.error] Any other use of `Structure`, such as the bound in `fn fields[X < Structure]`, is an error. Error: `structure-outside-template`.
6. r[annot.structure.receivers] `walk` takes the value as a readonly `self`. `facts`, `describe`, and `build` are receiverless.
7. r[annot.structure.build-fresh] `build` returns `mut Self`, because a built value is fresh like a data literal. Callers weaken it by ordinary assignability.
8. r[annot.structure.private] `walk`, `describe`, and `build` include private members. Opting a type in is consent for the template's library to read every member.
9. r[annot.structure.pure] `walk`, `describe`, and `build`, and every method of `Walker`, `Describer`, and `Source`, have the empty requirement row and are not suspending.
10. r[annot.structure.pure.impl] An implementation method of `Walker`, `Describer`, or `Source` that declares a requirement or suspends is an error. Error: `trait-method-signature`.

```text
use std.structure.Structure

data Point:
    x: i64

impl Structure for Point  # error: sealed-trait-implementation

fn fields[X < Structure]() -> void:  # error: structure-outside-template
    pass
```

> **Why.** Traversal stays pure, as serde's is: input and output happen
> before `build` or after `walk`, so a derivation never needs a provider.

### Templates

A template is a trait's one derived implementation, written over
`Structure`:

```text
use std.structure.Structure

impl[T] Encode for T by Structure:
    fn encode(self) -> string:
        let w: mut Encoder = Encoder { style: style_of(T::facts()), out: "" }
        _ := Structure::walk(self, w)
        w.out
```

1. r[annot.template.form] `impl[T] Trait for T by Structure:` declares the **template** of `Trait`.
2. r[annot.template.not-impl] A template is not an implementation. It never applies by itself and occupies no coherence slot, so it never overlaps a hand-written implementation.
3. r[annot.template.module] A template must be declared in the module that declares its trait. One declared elsewhere is an error. Error: `misplaced-derivation`.
4. r[annot.template.unique] A trait has at most one template. A second one is an error. Error: `overlapping-impl`.
5. r[annot.template.structure] Inside a template, `T` implements `Structure`. Its bodies may call `Structure::walk(self, w)`, `T::describe(d)`, `T::build(s)`, and `T::facts()`.
6. r[annot.template.body] A template must have a body in which at least one method calls `walk`, `describe`, or `build`.
7. r[annot.template.body.error] A template without such a body, such as a bodiless marker template, is an error. Error: `marker-template`.
8. r[annot.template.instance] Each derivation instantiates the template once, as one ordinary implementation of the trait for the target, in the target's module.
9. r[annot.template.no-families] A template never derives an implementation family, a new type, or a builder: it implements the existing trait exactly once per derivation.
10. r[annot.template.checked] The instantiated implementation is type-checked at its opt-in, where the member types and the walker's bounds are both known.

```text
use std.structure.Structure

trait Tagged:
    fn tag(self) -> string:
        "tagged"

impl[T] Tagged for T by Structure  # error: marker-template
```

> **Why.** Only the trait's module may declare the template, so a trait has
> one derivation and a reader finds it beside the trait. A marker template
> would derive a trait without reading one member.

### Derivation Blocks

A derivation block applies one template to one type and may configure it:

```text
use std.structure.Structure

data Order:
    id: i64
    total_cents: i64
    cache: Cache = Cache {}

impl Encode for Order by Structure:
    Self += [style(prefix="order_")]
    total_cents = [rename("total")]
    cache = pass
```

1. r[annot.block.form] `impl Trait for X by Structure:` is a **derivation block**: it applies the template of `Trait` to `X`.
2. r[annot.block.template] `Trait` must have a template. A block for any other trait, including a comparison trait, is an error. Error: `underivable-trait`.
3. r[annot.block.module] A derivation block must be declared in the module that declares `X`. One declared elsewhere is an error. Error: `misplaced-derivation`.
4. r[annot.block.structure-use] In an implementation header, `Structure` after `by` must name `std.structure.Structure`, imported with `use std.structure.Structure` like any other name. Without that import, `Structure` is unknown. Error: `unknown-trait`.
5. r[annot.block.not-delegation] `by Structure` is never a delegation, as [`trait.by.structure`](09-traits.md#r-trait.by.structure) also states.
6. r[annot.block.methods] A block may write a method of the trait. The written method replaces the template's method, as a written method replaces a default.
7. r[annot.block.lines] A block may carry [member lines](#member-lines), which edit the `Structure` that this derivation sees.
8. r[annot.block.local] A derivation block's member lines are local to it. Another derivation for the same type sees only the declaration facts.
9. r[annot.block.header] A block may state generic parameters and bounds in its header, as in `impl[T < Hash] Encode for Bag[T] by Structure:`. The derived implementation then has exactly those bounds.
10. r[annot.block.gadt] The target of a derivation, by `@derive` or by a block, must not be a GADT enum. Such a derivation is an error, reported at the opt-in. Error: `gadt-derivation`.
11. r[annot.block.newtype] The target of a derivation block must not be a newtype. A newtype derives only through its base type, with `@derive`, as [Derived Newtypes](09-traits.md#derived-newtypes) defines.
12. r[annot.block.newtype.error] A derivation block whose target is a newtype is an error, reported on the block. Error: `misplaced-derivation`.

```text
use std.structure.Structure

type Meters(i64)

impl Show for Meters by Structure  # error: misplaced-derivation
```

A module that writes `by Structure` without importing `Structure` gets the
ordinary unknown-trait error:

```text
data Point:
    x: i64

impl Show for Point by Structure  # error: unknown-trait
```

> **Note.** A foreign type has no derivation exception. Derive on a local
> mirror type with the foreign type's public fields, and convert, as with
> serde's `remote`.

#### Member Lines

A **member line** in a derivation block adjusts facts, or leaves a member
out, for that block only. A trait-less derivation block uses the same lines
to write shared metadata, as
[Trait-Less Derivation Blocks](#trait-less-derivation-blocks) defines:

| Rule | Form | Meaning for this block |
| --- | --- | --- |
| r[annot.line.extend] Extend | `f += [facts]` | the declaration facts of `f`, followed by `facts` |
| r[annot.line.replace] Replace | `f = [facts]` | `facts` in place of the declaration facts of `f` |
| r[annot.line.omit] Omit | `f = pass` | `f` is left out of `walk`, `describe`, and `build` |
| r[annot.line.self] Type-level | `Self += [facts]`, `Self = [facts]` | the type-level facts, extended or replaced |

1. r[annot.line.name] The name on the left of a member line must be a direct member of the target, or `Self`. Any other name is an error. Error: `unknown-annotation-member`.
2. r[annot.line.enum] In an enum's block, a member line names a whole variant or `Self`. A line that names a payload member is an error. Error: `unknown-annotation-member`.
3. r[annot.line.right-typed] The right side must be an expression of a list type, or `pass` after a member name and `=`. A named list, as in `name = shared_list`, needs no spread.
4. r[annot.line.right.error] Any other line is an error. That includes `f += pass`, `Self = pass`, and `pass` for a whole variant, such as `Busy = pass`. Error: `invalid-member-line`.
5. r[annot.line.right.not-list] A right side whose type is not a list type, such as `name = 5`, is this error rather than a type mismatch. Error: `invalid-member-line`.
6. r[annot.line.typed] A member line's list is contextually typed as that member's metadata list, as in [Member Metadata](#member-metadata). A `Self` line's list is contextually typed as `List[Any]`.
7. r[annot.line.duplicate] After a line applies, one member, variant, or type must not hold two facts of the same concrete type. `+=` with a type already present is an error; `=` changes it instead. Error: `duplicate-fact`.
8. r[annot.line.unchanged] A member without a line keeps its declaration facts.
9. r[annot.line.placement-blocks] A member line anywhere other than a derivation block or a trait-less derivation block, including in a template or an ordinary implementation, is an error. Error: `misplaced-derivation`.

```text
use std.structure.Structure

data Pair:
    left: i64
    right: i64

impl Show for Pair by Structure:
    left += [rename("l")]
    left += [rename("first")]  # error: duplicate-fact
    right = 5                  # error: invalid-member-line

enum Reply:
    Sent(id: i64)
    Busy

impl Show for Reply by Structure:
    id = [rename("x")]  # error: unknown-annotation-member
    Busy = pass         # error: invalid-member-line
```

> **Why.** A whole variant cannot be omitted, because `build` could never
> produce it. A payload member has no line, because a line names what a
> derivation walks as a unit.

#### Omitted Members

1. r[annot.omit.skip] A member omitted with `= pass` is not walked or described, and `build` never offers it to the source.
2. r[annot.omit.default] `build` fills an omitted member by evaluating its declared default.
3. r[annot.omit.no-default] Omitting a member that declares no default is an error, reported on the member line. Error: `omitted-member-without-default`.
4. r[annot.omit.exempt] An omitted member's type need not satisfy any walker's, describer's, or source's bound.
5. r[annot.omit.only-code] `= pass` is the only member line that changes generated code. Every other line changes only facts.

```text
use std.structure.Structure

data Audit:
    at: i64

data Account:
    Audit
    id: i64
    token: Token

impl Show for Account by Structure:
    token = pass  # error: omitted-member-without-default
    Audit = pass  # error: omitted-member-without-default
```

An embedded part has no default syntax
([`data.default.embedded`](08-data-and-enums.md#r-data.default.embedded)),
so omitting one, as `Audit = pass` above, is always this error.

#### Line Drift

1. r[annot.line.drift] Derivation blocks for traits of one package on one type whose member lines differ get a warning, reported on the later block. Warning: `derivation-line-drift`.
2. r[annot.line.drift.derive] A block beside a `@derive` of another trait from the same package gets the same warning when the block has any member line. Warning: `derivation-line-drift`.
3. r[annot.line.drift.legal] Different member lines stay legal: configuration may differ per direction, such as encoding and decoding.

> **Why.** Most blocks of one library should agree, so a difference is
> worth a second look. Some differences are deliberate, as serde's
> `skip_serializing` shows.

#### Trait-Less Derivation Blocks

A **trait-less derivation block** writes shared metadata for one type, away
from its declaration. It names no trait and derives nothing:

```text
use std.structure.Structure

data User:
    @max_len(80)
    name: string
    email: string

impl User by Structure:
    name += [min_len(1)]
    email = [max_len(320)]
```

Every derivation of `User`, and its shape, then sees `max_len(80)` and
`min_len(1)` on `name`, and `max_len(320)` on `email`.

Each line names an existing direct member, as any member line does. An
embedded field is named by its final type name, and members promoted
through it are not direct members. The block cannot add, rename, remove, or
change the type of a member.

1. r[annot.traitless.form] `impl X by Structure:`, written without a trait, is a trait-less derivation block. It writes metadata of `X` and implements nothing.
2. r[annot.traitless.not-inherent] A trait-less block is not an inherent implementation: it declares no members of `X`.
3. r[annot.traitless.lines-only] Its body holds only member lines. A method or an associated type in it is an error, reported on that member. Error: `misplaced-derivation`.
4. r[annot.traitless.no-omit] It writes only metadata, so an omit line `f = pass` in it is an error. Error: `invalid-member-line`.
5. r[annot.traitless.after-decorators] Its lines apply to the values that a member's decorators attach: a `+=` line appends after them, and a `=` line replaces them.
6. r[annot.traitless.self] Its `Self` lines apply the same way to the type-level facts that decorators before `X` attach.
7. r[annot.traitless.declaration-facts] The result is the declaration facts of `X` and its members: every derivation of `X` sees it, and so do the field and variant shapes of `X`.
8. r[annot.traitless.then-blocks] A derivation block's member lines then edit those declaration facts, for that block only.
9. r[annot.traitless.module] A trait-less block must be declared in the module that declares `X`, as an inherent implementation must. One declared elsewhere is an error, reported on the block. Error: `misplaced-derivation`.
10. r[annot.traitless.local] A trait-less block in a local scope is an error, reported on the block: local declarations carry no metadata. Error: `misplaced-derivation`.
11. r[annot.traitless.target] `X` must be a data type or an enum. Any other target, including a newtype, is an error, reported on the block. Error: `misplaced-derivation`.
12. r[annot.traitless.by] The name after `by` in a header without a trait must be `Structure`, as [`trait.by.trait-less`](09-traits.md#r-trait.by.trait-less) states.
13. r[annot.traitless.generic] The header must declare only the own type parameters of `X`'s declaration, in order and without bounds, and apply `X` to them, as `impl[T] Box[T] by Structure:`.
14. r[annot.traitless.generic.rename] The header may rename the parameters, as `impl[U] Box[U] by Structure:` for `data Box[T]`. Only their count, their order, and the absence of bounds are checked.
15. r[annot.traitless.generic.error] Any other header, such as `impl Box[i32] by Structure:` or `impl[T < Hash] Box[T] by Structure:`, is an error, reported on the block. Error: `misplaced-derivation`.
16. r[annot.traitless.unique] A type has at most one trait-less block. A second one is an error, reported on the second block. Error: `overlapping-impl`.

```text
use std.structure.Structure

data Token:
    text: string = ""

data Account:
    id: i64
    token: Token = Token {}

type Meters(i64)

data Box[T]:
    item: T

impl Account by Structure:
    token = pass  # error: invalid-member-line
    fn label(self) -> string:  # error: misplaced-derivation
        "account"

impl Meters by Structure  # error: misplaced-derivation

impl Box[i32] by Structure  # error: misplaced-derivation

impl Account by Structure  # error: overlapping-impl
```

> **Why.** The block reuses the derivation block's header and member lines,
> so shared metadata needs no keyword of its own. An omit line is not
> metadata, because it changes generated code. One block per type keeps
> shared metadata in one place, so no line depends on the order of blocks.

See also: [Member Metadata](#member-metadata), [Facts](#facts).

### Facts

A **fact** is an ordinary value attached to a type, member, or variant for
derivations to read:

```text
fn key_for(style: Style, m: Member) -> string:
    match m.facts.find[Rename]():
        .Some(r) => r.name
        .None => style.prefix + m.name
```

1. r[annot.fact.descriptive] Facts are descriptive: a fact changes no generated call. Only a template's own code reads it.
2. r[annot.fact.type-level-decorator] A decorator before a data or enum declaration attaches a type-level fact, as `@style(prefix="user_")` does.
3. r[annot.fact.member-metadata] A member's or variant's declaration facts are its member metadata: the values that its decorators attach, in source order, as the type's trait-less derivation block edits them.
4. r[annot.fact.payload] A payload member's declaration facts are the values of the decorators before its payload parameter.
5. r[annot.fact.shared] Declaration facts are seen by every derivation of the type. A derivation block's member lines edit them for that block only.
6. r[annot.fact.eval] A fact expression is evaluated once, at compile time. It must be requirement-free, as defined for [default values](07-functions.md#default-values).
7. r[annot.fact.read] A template reads the type-level facts through `T::facts()`, and a member's or variant's facts through its handle's `info.facts`.
8. r[annot.fact.default] A template falls back to its own default when a fact is absent. An absent or foreign fact is never an error.
9. r[annot.fact.unused-non-std] A type-level fact whose type comes from a package other than `std`, where that package supplies no template that the type derives, gets a warning, reported on its decorator. Warning: `unused-derivation-fact`.
10. r[annot.fact.unused-std] A fact of a primitive or standard type, such as `@"internal"`, never gets this warning.
11. r[annot.fact.unused-self-line] A type-level fact that a trait-less block's `Self` line writes gets the same warning under the same conditions, reported on that line. Warning: `unused-derivation-fact`.
12. r[annot.fact.unused-self-line.per-trait] A type-level fact that a `Self` line of a derivation block for a trait writes gets the same warning, reported on that line, when the fact's package does not supply that trait. Warning: `unused-derivation-fact`.
13. r[annot.fact.duplicate-decorator] Two decorators before one declaration whose type-level facts have one concrete type are an error, reported on the later decorator. Error: `duplicate-fact`.

```text
use std.structure.Structure

@style(prefix="p_")  # warning: unused-derivation-fact
data Plain:
    id: i64

@"internal"
data Note:
    id: i64

data Quiet:
    id: i64

impl Quiet by Structure:
    Self += [style(prefix="q_")]  # warning: unused-derivation-fact

@style(prefix="a_")
@style(prefix="b_")  # error: duplicate-fact
data Twice:
    id: i64
```

14. r[annot.fact.no-block-on] A fact or metadata expression must not call `std.task.block_on`, directly or transitively through the statically known call graph, as for a default expression in [Driving A Stored Suspension](11-requirements-and-suspension.md#driving-a-stored-suspension).
15. r[annot.fact.no-block-on.unprovable] A call through a function value or a dynamic trait method that prevents the compiler from proving `block_on` unreachable is rejected in a fact or metadata expression.
16. r[annot.fact.no-block-on.error] Every violation is an error, reported on the fact or metadata expression. Error: `suspension-forbidden-context`.

```text
use std.task.block_on

data Style:
    prefix: string

fn ready!() -> string:
    "p_"

fn loaded_style() -> Style:
    let pending: mut Suspend[string] = ready()
    Style { prefix: block_on(pending) }

@loaded_style()  # error: suspension-forbidden-context
data User:
    id: i64
```

> **Why.** Configuration is data on the type, not a hook on the trait, so
> a derived trait stays dynamically safe and two libraries' facts never
> collide.

> **Note.** A fact expression may call a function in another file. An
> implementation evaluates facts after it checks function bodies, and the
> [package interface](10-modules.md#r-module.interface.fact-values) records
> each fact's value, so a changed body that yields the same value leaves the
> interface unchanged.

### Walk, Describe, And Build

The compiler generates three traversals for each derivation. A data type is
traversed as an enum with one variant.

#### Walk

1. r[annot.walk.match] Generated `walk` matches the value once and calls `w.variant(v)` for the value's variant.
2. r[annot.walk.members] It then calls `w.member(h, value)` for each member of that variant, in declaration order.
3. r[annot.walk.value] Each member value is read through the readonly `self`, so it has the member's read type.
4. r[annot.walk.error] The first `.Err` that `variant` or `member` returns ends the walk, and `walk` returns it. Otherwise `walk` returns `.Ok()`.
5. r[annot.walk.nested] A member is passed as one value. Its own members are reached only through its own type's implementations.

#### Describe

1. r[annot.describe.order] Generated `describe` calls `d.variant(v)` for every variant in declaration order, each followed by `d.member(h)` for that variant's members in declaration order.
2. r[annot.describe.no-value] `describe` holds no value.
3. r[annot.describe.error] The first `.Err` ends the traversal and is returned, as for `walk`.

#### Build

1. r[annot.build.variant] Generated `build` first calls `s.variant(choices)` once, with every variant in declaration order. A data type offers its one variant.
2. r[annot.build.next] It then calls `s.next(members)` with the chosen variant's members, repeatedly, until the source returns the end key.
3. r[annot.build.member] For each member key, it calls `s.member(h, previous)`, where `previous` is the value already read for that member, or `.None`.
4. r[annot.build.missing] After the end key, it calls `s.missing(h)` once for each member that the source never named.
5. r[annot.build.construct] It then constructs the chosen variant from one local per member, and returns it as `mut Self`.
6. r[annot.build.errors] The first `.Err` from a source method ends the build and is returned. Generated code never creates an error value of its own.
7. r[annot.build.foreign-key] A key that does not name one of the chosen variant's members is a checked runtime panic with category `structure-variant-mismatch`.
8. r[annot.build.shared] `build` never reads shared constructor data: the chosen variant's `->` expression fixes it.

### Members And Variants

1. r[annot.member.info] Each member has a `Member` value: its name, its zero-based position within its variant, its facts, its doc comment, and the `embedded` and `positional` flags.
2. r[annot.member.embedded] An embedded field is one member, named by its embedded type's final name, with `embedded` true.
3. r[annot.member.embedded.part] Its value is the part itself, not a copy, as [`data.part.alias`](08-data-and-enums.md#r-data.part.alias) states.
4. r[annot.member.no-flatten] The language never flattens an embedded part. Flattening is library policy, read from the `embedded` flag.
5. r[annot.member.positional] An unnamed payload parameter is a member named `_0`, `_1`, and so on, by position, with `positional` true.
6. r[annot.variant.info] Each variant has a `VariantInfo` value: its name, its zero-based index, its facts, its doc comment, `of_data`, and `shared`.
7. r[annot.variant.data] A data type's one variant has `of_data` true, and its name and doc comment are the type's.
8. r[annot.variant.data-facts] That variant's `facts` is empty: a template reads the type-level facts once, through `T::facts()`.
9. r[annot.variant.shared] `shared` holds the variant's shared constructor data as `(name, value)` pairs, built once at compile time. An unnamed shared parameter is named `_0`, `_1`, and so on.
10. r[annot.variant.shared.no-handle] Shared constructor data is never a member: it is never passed as a handle.

See also: [Data Embedding](08-data-and-enums.md#data-embedding),
[Shared Enum Constructor Data](08-data-and-enums.md#shared-enum-constructor-data).

### Handles

A **handle** is a compiler-generated constant that names one member or
variant of a derivation's target.

1. r[annot.handle.constants] Each member has one constant handle of type `Field[S, F]`, and each variant one of type `Variant[S]`.
2. r[annot.handle.read-type] `walk` and `describe` pass handles whose `F` is the member's read type: for `hits: mut Counter`, it is `Counter`.
3. r[annot.handle.declared-type] `build` passes handles whose `F` is the member's declared type: for `hits: mut Counter`, it is `mut Counter`.
4. r[annot.handle.variance] `Field` is contravariant in `S` and covariant in `F`, so a declared-type handle weakens to its read-type handle.
5. r[annot.handle.get] `h.get(x)` accepts any `S`. Its type is the type that the field access `x.member` would have, for a member declared `F`.
6. r[annot.handle.get.views] So `get` on a declared-type handle returns the readonly view for a readonly `x`, and `F` for a `mut S`. No member is upgraded.
7. r[annot.handle.get.variant] `h.get(x)` on a payload member, when `x` holds another variant, is a checked runtime panic with category `structure-variant-mismatch`.
8. r[annot.handle.holds] `v.holds(x)` is true exactly when `x` holds the variant `v`.
9. r[annot.handle.default] `h.has_default()` is true when the member declares a default. `h.default()` evaluates that default, or returns `.None` when there is none.
10. r[annot.handle.escape] Handles are ordinary values and may escape the traversal that passed them.

```text
data Counter:
    count: i64

@derive(Duplicate)
data Stats:
    hits: mut Counter
    label: string

data CopySource[S]:
    old: mut S

impl[S] Source[S] for CopySource[S]:  # variant, next, and member elided
    type Error = never

    fn missing[F](mut self, h: Field[S, F]) -> Result[F, never]:
        .Ok(h.get(self.old))
```

> **Note.** A walker that holds a second value, such as a diff, checks
> `v.holds(other)` in `variant` before any `h.get(other)`. The
> standard-library `Clone` trait, listed in
> [STDLIB](../future-work/STDLIB.md#clone), declares `clone(self)`, which
> reads the readonly views, and `clone_mut(mut self) -> mut Self`, whose
> source reads the declared types from a `mut` value, as `CopySource` does.

> **Note.** Handles are constants, and each generated call passes a
> constant dictionary, so a traversal allocates nothing per member.

### Walkers, Describers, And Sources

A walker, describer, or source is an ordinary type that implements
`Walker`, `Describer`, or `Source`:

```text
impl[S] Walker[S] for Encoder:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        if !v.info.of_data:
            self.out = self.out + self.style.tag + "=" + v.info.name + ";"
        .Ok()

    fn member[F < Encode](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + key_for(self.style, h.info) + "=" + value.encode() + ";"
        .Ok()
```

1. r[annot.walker.strengthen-member] An implementation of `Walker`, `Describer`, or `Source` may strengthen the bound on `member[F]`, as `F < Encode` above.
2. r[annot.walker.missing-fixed] A `Source` implementation must not strengthen the bound on `missing[F]`: it keeps the trait's unbounded `F`.
3. r[annot.walker.missing-fixed.error] A strengthened bound on `missing[F]` is an error, reported at the implementation method. Error: `trait-method-signature`.
4. r[annot.walker.not-sealed] `Walker`, `Describer`, and `Source` are not sealed. Any package may implement them.
5. r[annot.walker.obligation] Every member that a derivation walks, describes, or builds must satisfy the strengthened bounds of the walker, describer, or source that the template passes.
6. r[annot.walker.obligation.error] The obligation is checked at the opt-in. A member that fails it is an error, reported at the opt-in and naming the member. Error: `member-not-derivable`.
7. r[annot.walker.generic-member-call] `member` may be called through a generic walker, describer, or source type only by generated code. Such a call written in source is an error. Error: `generic-member-call`.
8. r[annot.walker.generic-missing] Code outside generated code may call `missing` through a generic source type.
9. r[annot.walker.concrete-call] A call on a concrete walker, describer, or source type applies that type's own bounds.

```text
@derive(Show)  # error: member-not-derivable
data Label:
    text: string

fn forward[S, W < Walker[S]](w: mut W, h: Field[S, i64], value: i64) -> void:
    _ := w.member(h, value)  # error: generic-member-call

fn fill[S, F, R < Source[S]](r: mut R, h: Field[S, F]) -> Result[F, R::Error]:
    r.missing(h)  # valid: `missing` may be called through a generic source

impl[S] Source[S] for Strict:  # variant, next, and member elided
    type Error = never

    fn missing[F < Display](mut self, h: Field[S, F]) -> Result[F, never]:  # error: trait-method-signature
        panic("missing ${h.info.name}")
```

> **Why.** A strengthened bound becomes one obligation per member, checked
> where both the member types and the walker are known. A generic call
> could bypass that check. The bound on `missing` stays fixed, so a generic
> `missing` call is always checked against the trait's own signature.

### Derived Bounds

1. r[annot.bound.params] A derived implementation for a generic type gets `T < Trait` for each type parameter `T` that appears in a walked, described, or built member.
2. r[annot.bound.omitted] An omitted member contributes no bound.
3. r[annot.bound.recursive] Recursion is checked coinductively: while checking the members of `Tree[T]`, its own derived implementation is assumed to hold.
4. r[annot.bound.more] When a member needs more than `T < Trait`, as a `Map[T, V]` member needs `T < Hash`, the error suggests a derivation block whose header states the bounds. Error: `member-not-derivable`.

See also: [`trait.derive.bounds`](09-traits.md#r-trait.derive.bounds),
[Derived Newtypes](09-traits.md#derived-newtypes).

### Limits

1. r[annot.limit.in-place] There is no in-place traversal: generated code never assigns a member of an existing value. A derived merge returns a new value.
2. r[annot.limit.interfaces] A package interface carries each template's body, the walker, describer, and source bodies it names, and the functions its facts call.
3. r[annot.limit.specialize] Each (target, walker) pair is one specialization of the generated traversal.

> **Note.** Wire stability is documented, not checked. Reordering or
> renaming members or variants changes derived formats, `VariantInfo.index`,
> and derived `Ord`. A library may require explicit tags as facts.

### Undecided Parts

These parts of typed derivation are open design questions, listed in
[Open Issues](../future-work/OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets).
An implementation must not guess them:

| Part | State |
| --- | --- |
| Fact check hook | The form of a fact type's compile-time check against its member, and whether it covers cross-member and type-level checks. |
| Non-escaping handles | Whether a future non-escaping trait design makes handles non-escaping. |
| Plan constants | A template may declare a constant computed once per derivation at compile time. Its syntax and the evaluator's limits are open, so no syntax for it exists. |
| Typed shared constants | Typed handles for shared constructor data. |
| `T -> U` mapping | Whether derivation between two types is in scope. |
| Name clashes | How `walk`, `describe`, and `build` interact with trait methods of the same name. `Structure::walk(self, w)` is always unambiguous. |
| Derived bound | Whether a derived bound names the trait or the walker's strengthened bound, where the two differ. |
| `default()` allocation | Whether `h.default()` may allocate for every member type. |
| Composing templates | How a walker forwards to another walker's `member`, which only generated code may call generically. |
| `Clone`'s module | Which standard module declares `Clone`. It is chosen with the standard library. |
| Derived-function cache | The API of the standard cache for derived associated functions. It is chosen with the standard library. |
| Function targets | Deriving for functions, as tool adapters need ([FN_TYPE questions 9 and 10](../future-work/FN_TYPE.md#9-how-do-tool-adapters-get-per-declaration-data)). A decorator before a function attaches a value, as [Prefix Decorators](#prefix-decorators) defines. |
