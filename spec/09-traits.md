# Traits

Status: language specification draft.

This chapter defines traits, their implementations, and how trait methods are
selected.

1. r[trait.kind.shared] Traits describe behavior shared by otherwise unrelated types.
2. r[trait.kind.explicit] Conformance is explicit.
3. r[trait.kind.no-structural] hd-lang does not use structural method matching or class inheritance.

## Trait Declarations

A trait declares required methods:

```text
trait Describe:
    fn describe(self) -> string
```

A method with a body is a default implementation:

```text
trait Named:
    fn name(self) -> string

    fn label(self) -> string:
        "name: " + self.name()
```

### Trait Members

1. r[trait.decl.required] A trait declares required methods.
2. r[trait.decl.default] A method with a body is a default implementation.
3. r[trait.decl.unique] Member names must be unique within a trait. A repeated associated type, method, or associated function name is an error. Error: `duplicate-trait-member`.
4. r[trait.decl.typed] Every parameter and result type is explicit.
5. r[trait.decl.method] A function whose first parameter is `self` or `mut self` is a method.
6. r[trait.decl.mut-receiver] A mutable receiver is a requirement callers must satisfy.
7. r[trait.decl.associated-fn] A function without a receiver is an associated function. It is called with qualified `Trait::function(...)` or `Type::function(...)` syntax.

```text
trait Named:
    fn name(self) -> string
    fn name(self) -> string  # error: duplicate-trait-member
```

### Default Method Bodies

1. r[trait.default.checked-once] A default method body is checked once, in the trait declaration, with `Self` bounded by the trait.
2. r[trait.default.visible] Through `self` and `Self`, the body sees only the members of the trait and of its transitive supertraits.
3. r[trait.default.no-self-members] The fields and inherent methods of an implementing type are not accessible there, even when every implementing type declares them.
4. r[trait.default.no-self-members.error] A call there of a method that neither the trait nor a supertrait declares is an error. Error: `unknown-method`.

```text
trait Greeter:
    fn name(self) -> string

    fn greet(self) -> string:
        "hi " + self.name()

    fn shout(self) -> string:
        self.loud_name()  # error: unknown-method
```

See also: [Supertraits](#supertraits).

### Generic Traits

Traits may be generic:

```text
trait Add[T]:
    fn add(self, other: T) -> T
```

1. r[trait.decl.generic] Traits may be generic.
2. r[trait.decl.generic.invariant] A trait's generic parameters are invariant, so `Source[mut User]` and `Source[User]` are unrelated trait instantiations.
3. r[trait.decl.generic.no-variance] A variance marker on a trait's generic parameter is an error. Error: `invalid-variance`.

```text
trait Source[+T]:  # error: invalid-variance
    fn source(self) -> T
```

See also: [Variance](04-type-system.md#variance).

### Marker Traits

A trait with no methods is a marker, written without a body:

```text
trait Serializable

impl Serializable for User
```

1. r[trait.marker.decl] A trait with no methods may be declared as a marker without a body.
2. r[trait.marker.impl] The marker's implementation likewise has no body.
3. r[trait.marker.all-defaults] A bodyless implementation is also permitted when every method of the trait has a default.
4. r[trait.marker.no-promotion] A method promoted from an embedded field never fills a trait method, so it never makes a body optional.

See also: [Embedding And Trait Satisfaction](#embedding-and-trait-satisfaction).

### Supertraits

A trait may require another trait using a supertrait bound:

```text
trait Formattable < Display:
    fn format(self) -> string
```

1. r[trait.super.bound] A trait may require another trait using a supertrait bound.
2. r[trait.super.satisfy] An implementation of `Formattable` must also satisfy `Display`.
3. r[trait.super.missing] An `impl Child for X` for which `X` has no implementation of a supertrait of `Child` is an error. Error: `missing-supertrait-implementation`.
4. r[trait.super.acyclic] The supertrait graph must be acyclic: a direct or indirect cycle is a compile-time error. Error: `supertrait-cycle`.
5. r[trait.super.cycle-report] An indirect cycle is reported once, on the member of the cycle that appears first. Members are ordered first by module identity, then by source position within the module.

```text
trait Parent

trait Child < Parent:
    fn value(self) -> i32

data Item: pass

impl Child for Item:  # error: missing-supertrait-implementation
    fn value(self) -> i32: 42

trait Loop < Loop:  # error: supertrait-cycle
    fn step(self) -> void
```

#### Supertrait Member Names

1. r[trait.super.no-redeclare] A child trait must not declare a member whose name is also the name of a member of any of its transitive supertraits. Error: `duplicate-trait-member`.
2. r[trait.super.no-redeclare.when] The check is made at the child trait's declaration, whether or not any type implements it.
3. r[trait.super.no-redeclare.kinds] The check applies to associated types, methods, and associated functions alike.
4. r[trait.super.no-redeclare.report] The `duplicate-trait-member` error is reported on the child's member.
5. r[trait.super.no-redeclare.example] For example, when `Greeter` declares `fn greet(self) -> string`, `trait Loud < Greeter` must not declare `greet`, with or without a body.
6. r[trait.super.no-default-override] A child trait therefore cannot provide a default body for a supertrait's method.
7. r[trait.super.defaults] A supertrait's defaults come only from the supertrait.

```text
trait Greeter:
    fn greet(self) -> string

trait Loud < Greeter:
    fn greet(self) -> string  # error: duplicate-trait-member
```

### Associated Types

Traits may declare associated types, and implementations bind them:

```text
trait Supplier:
    type Item
    fn get(mut self) -> Self::Item?

impl Supplier for NameSupplier:
    type Item = string

    fn get(mut self) -> string?:
        ...
```

1. r[trait.assoc.declare] Traits may declare associated types, and implementations bind them.
2. r[trait.assoc.self] `Self::Item` projects from the current implementation.
3. r[trait.assoc.projection] `T::Item` projects from a generic type whose bounds select exactly one associated type declaration.
4. r[trait.assoc.ambiguous] Ambiguous projections are compile-time errors.
5. r[trait.assoc.bound] A bound may also fix a projection to a type.

See also: [Associated Type Bindings](#associated-type-bindings).

## Trait Implementations

This section defines the standard comparison, conversion, error, and debug
traits, and the rules that every trait implementation follows.

### Comparison Traits

The standard library defines `Eq`, `PartialOrd`, `Ord`, and `Ordering` in
`std.cmp`.

| Rule | Trait | Requires |
| --- | --- | --- |
| r[trait.cmp.equality] Equality | `Eq` | `fn eq(self, other: Self) -> bool` |
| r[trait.cmp.partial-ordering] Partial ordering | `PartialOrd < Eq` | `fn partial_cmp(self, other: Self) -> Ordering?`, where `.None` means unordered |
| r[trait.cmp.total-ordering] Ordering | `Ord < PartialOrd` | `fn cmp(self, other: Self) -> Ordering` |

1. r[trait.cmp.std-module] The standard library defines `Eq`, `PartialOrd`, `Ord`, and `Ordering` in `std.cmp`.
2. r[trait.cmp.ordering] `Ordering` has `Less`, `Equal`, and `Greater` cases.
3. r[trait.cmp.one-eq] `Eq` is the only equality trait. There is no separate partial-equality trait.
4. r[trait.cmp.reflexive] An `Eq` implementation must be reflexive: `x.eq(x)` is `true` for every value `x`.
5. r[trait.cmp.ord-agree] An `Ord` implementation must agree with the type's `PartialOrd` implementation.
6. r[trait.cmp.laws] These semantic laws are obligations of the implementer; ordinary trait checking cannot prove them.
7. r[trait.cmp.float-eq] Floating-point types implement `Eq` with IEEE 754 semantics, so NaN is unequal even to itself. This is the one documented exception to the reflexive law.
8. r[trait.cmp.float-ord] Floating-point types implement `PartialOrd` but not `Ord`, because NaN is unordered.
9. r[trait.cmp.operators-comparison] `==`, `!=`, and the relational operators invoke the comparison traits. The other operators invoke the `std.ops` traits of [Operator Traits](05-expressions.md#operator-traits).

> **Why.** This follows Swift's `Equatable`: one equality trait serves `==`,
> map keys, and derivation, and floats keep IEEE 754 equality.

#### Hashing

The standard library also defines `Hash` and `Hasher` in `std.hash`:

```text
trait Hasher:
    fn write(mut self, bytes: List[u8]) -> void

trait Hash:
    fn hash(self, state: mut Hasher) -> void
```

1. r[trait.hash.module] The standard library defines `Hash` and `Hasher` in `std.hash`.
2. r[trait.hash.map-key] A map key must implement both `Eq` and `Hash`.
3. r[trait.hash.not-inferred] Neither trait is inferred for user-defined data or enums, including payload-free enums.
4. r[trait.hash.unverified] The compiler does not verify any relationship between their implementations.

> **Note.** A readonly key can still change through another mutable alias,
> potentially leaving its map entry unreachable.

#### Derived Implementations

1. r[trait.derive.no-automatic] There is no automatic conformance for user-defined data or enum types.
2. r[trait.derive.explicit-impl] An explicit implementation may choose domain-specific equality or ordering.
3. r[trait.derive.intrinsic-decl] `@derive(Eq, Hash)` is an explicit compiler intrinsic on a data, enum, or newtype declaration.
4. r[trait.derive.arguments] Its arguments name traits, not metadata values.
5. r[trait.derive.generate] The compiler generates ordinary implementations of the named traits from the declaration's shape.
6. r[trait.derive.check] The compiler checks trait requirements and coherence, and rejects traits for which it has no derivation rule.
7. r[trait.derive.bounds] For each derived trait, the generated implementation adds a `T < Trait` bound for every declaration type parameter `T` that occurs in a field compared, ordered, or hashed by that derivation.
8. r[trait.derive.bounds.example] Thus `@derive(Eq) data Box[T]` produces conformance only when `T < Eq`.
9. r[trait.derive.supertraits] The target must also satisfy each derived trait's supertraits, whether through an existing implementation or another derivation.
10. r[trait.derive.intrinsic-set] The intrinsic derivations are exactly `Eq`, `PartialOrd`, `Ord`, and `Hash`. Each covers every member, and no member line or derivation block configures it.
11. r[trait.derive.templated] `@derive` also accepts a trait that has a derivation template, as [Typed Derivation](14-annotations.md#typed-derivation) defines. Any other trait is an error. Error: `underivable-trait`.
12. r[trait.derive.field-missing-trait] A field that an intrinsic derivation compares, orders, or hashes must implement the derived trait. A field that does not is an error at the field, whose message names the trait and the field. Error: `derive-field-missing-trait`.
13. r[trait.derive.bound-unmet] A use of a derived implementation whose type argument does not meet a bound that `trait.derive.bounds` added is an error at the use, as `==` on two `Box[fn() -> void]` values is. Error: `missing-derived-bound`.

```text
data Opaque: pass

@derive(Eq, Hash)
data Key:
    value: Opaque  # error: derive-field-missing-trait

@derive(Eq)
data Box[T]:
    value: T

fn same(left: Box[fn() -> void], right: Box[fn() -> void]) -> bool:
    left == right  # error: missing-derived-bound
```

#### Law Partners

The comparison and hash traits whose laws relate them are **law partners**:

| Rule | Trait | Law partners |
| --- | --- | --- |
| r[trait.derive.partners.hash] Hash | `Hash` | `Eq` |
| r[trait.derive.partners.partial-ord] Partial ordering | `PartialOrd` | `Eq` |
| r[trait.derive.partners.ord] Ordering | `Ord` | `Eq` and `PartialOrd` |

1. r[trait.derive.partners.same-list] Deriving `Hash`, `PartialOrd`, or `Ord` requires each of its law partners to be derived in the same `@derive` list.
2. r[trait.derive.partners.no-mix] One type's law partners must be all derived or all hand-written: a derived implementation and a hand-written implementation of two law partners never coexist.
3. r[trait.derive.partners.error] A derivation that breaks either rule is an error, reported on its `@derive` line. Error: `mixed-derived-law`.

```text
@derive(Hash)  # error: mixed-derived-law
data Session:
    token: string

impl Eq for Session:
    fn eq(self, other: Session) -> bool: self.token == other.token
```

> **Why.** A hand-written `Eq` that ignores a field, beside a derived `Hash`
> that hashes it, would put equal keys in different buckets.

#### Derived Newtypes

1. r[trait.derive.newtype] `@derive` on a newtype declaration generates each named trait's implementation from the base type's implementation.
2. r[trait.derive.newtype.base] The generated method applies the base type's method to the wrapped values, as in `Mile(1) == Mile(1)` comparing the two `i32` values.
3. r[trait.derive.newtype.requires] The base type must implement each derived trait, as a derived field must.
4. r[trait.derive.newtype.requires.error] A base type that does not implement a derived trait is an error at the base type, whose message names the trait and the base type. Error: `derive-field-missing-trait`.
5. r[trait.derive.newtype.not-inherited] A newtype still inherits no implementation it does not derive or implement.
6. r[trait.derive.newtype.templated] A trait with a template also derives through the base type: the newtype gets no `Structure`, and the base type's implementation is rewrapped.
7. r[trait.derive.newtype.self-positions] Forwarding is allowed only where the trait's methods use `Self` as the receiver, as plain `Self`, or inside `Self?`, `Result[Self, E]`, or `List[Self]`.
8. r[trait.derive.newtype.self-error] Any other position, such as `Map[Self, V]`, `Map[string, Self]`, or a tuple holding `Self`, is an error at the `@derive` line that names the trait method. Error: `newtype-derivation-self`.

```text
@derive(Eq, Hash)
type Mile(i32)

fn same(left: Mile, right: Mile) -> bool:
    left == right
```

```text
trait Pairing:
    fn pair(self) -> (Self, i32)

@derive(Pairing)  # error: newtype-derivation-self
type Tag(string)

data Opaque: pass

@derive(Eq)
type Wrapped(Opaque)  # error: derive-field-missing-trait
```

> **Why.** A `Map[Self, V]` result depends on the key's own `Hash` and `Eq`,
> which a rewrapped base value would not use. The allowed positions rewrap
> each value one at a time.

See also: [Newtypes](04-type-system.md#newtypes).

#### Derived Equality

1. r[trait.derive.eq.compare-fields] Derived `Eq` compares every declared data field, including embedded fields, by its `Eq` implementation.
2. r[trait.derive.eq.no-exclusion] No field is implicitly excluded.
3. r[trait.derive.eq.enum] Derived enum equality first compares the variant, then every payload field of that variant, including common enum fields.
4. r[trait.derive.eq.variants] Different variants are unequal.
5. r[trait.derive.eq.eq] Derived `Eq` requires every compared field to satisfy `Eq`.
6. r[trait.derive.eq.cycles] Derived equality does not detect cycles or track previously compared objects: it recursively invokes each field's `Eq` implementation.
7. r[trait.derive.eq.stack] A comparison that repeatedly traverses a cycle may exhaust the execution stack.

#### Derived Ordering

1. r[trait.derive.ord.support] `@derive(PartialOrd, Ord)` also supports data and enums.
2. r[trait.derive.ord.data] Derived ordering is lexicographic in declared data-field order, including embedded fields.
3. r[trait.derive.ord.variants] For enums, distinct variants compare by variant declaration order.
4. r[trait.derive.ord.same-variant] Values of the same variant compare shared enum data in declaration order, followed by that variant's payload parameters in declaration order.
5. r[trait.derive.ord.argument-order] Constructor argument order does not affect comparison.
6. r[trait.derive.ord.partial] Derived `PartialOrd` requires every compared field to satisfy `PartialOrd`. It returns `.None` if a field comparison is unordered before a comparison result is determined.
7. r[trait.derive.ord.ord] Derived `Ord` requires every compared field to satisfy `Ord`.

#### Derived Hashing

1. r[trait.derive.hash.support] `@derive(Hash)` supports data and enums.
2. r[trait.derive.hash.data] It generates an ordinary `Hash` implementation that hashes every declared data field in declaration order, including embedded fields.
3. r[trait.derive.hash.enum] For an enum, it hashes the variant identity, then shared enum data in declaration order, then that variant's payload fields in declaration order.
4. r[trait.derive.hash.fields] Every hashed field must implement `Hash`; no field is implicitly excluded.
5. r[trait.derive.hash.cycles] Like derived equality, derived hashing does not detect cycles, so hashing a cyclic graph may exhaust the execution stack.
6. r[trait.derive.hash.seeded] Hash values computed from a hash seed that the runtime provides are stable within one code identity and runtime profile, and may change when either changes.

### Conversion Trait

The standard library declares the general conversion trait `From` in
`std.convert`:

```text
trait From[T]:
    fn from(value: T) -> Self
```

1. r[trait.from.module] The standard library declares the general conversion trait `From` in `std.convert`.
2. r[trait.from.meaning] An implementation `impl From[T] for X` converts a `T` into an `X`.
3. r[trait.from.import] `From` is not a prelude name; code that names it imports it, as in `use std.convert.From`.
4. r[trait.from.propagation] Postfix `?` uses the trait without an import. When an error is not assignable to the enclosing function's error type, `?` calls that error type's `From` implementation once.
5. r[trait.from.direct] Any code may also call a conversion directly as `X::from(value)`.

See also: [Propagation](05-expressions.md#propagation).

#### Pure Conversions

1. r[trait.from.pure] A conversion is pure.
2. r[trait.from.row] The trait method `from` has the empty requirement row and is not suspending. An implementation method must agree with it on both.
3. r[trait.from.signature] An implementation whose `from` declares a requirement clause, as in `fn from(value: FsError) -> SyncError $ Console`, is an error. So is one whose `from` is suspending, as in `fn from!(value: FsError) -> SyncError`. Error: `trait-method-signature`.
4. r[trait.from.panic] The body of `from` may still panic.

```text
impl From[FsError] for SyncError:
    fn from(value: FsError) -> SyncError $ Console:  # error: trait-method-signature
        println("converting")
        SyncError.Fs(value)
```

See also: [Requirement Rows](11-requirements-and-suspension.md#requirement-rows).

#### Conversion Implementations

1. r[trait.from.coherence] `From` implementations follow the ordinary rules for implementation targets, ownership, overlap, and uniqueness.
2. r[trait.from.owners] `impl From[FsError] for SyncError` may be declared by the package that owns `SyncError`, the package that owns `FsError`, or the standard library.
3. r[trait.from.no-overlap] Implementations for different source types never overlap, because their trait arguments differ. One error type may therefore implement both `From[FsError]` and `From[HttpError]`.
4. r[trait.from.reflexive] A reflexive `impl[T] From[T] for T` is an error. Error: `bare-parameter-impl-target`.
5. r[trait.from.trait-value] A trait value type is never a target, so `impl From[FsError] for Error` is an error. Error: `trait-value-impl-target`.
6. r[trait.from.instantiations] When `X` implements `From` at several instantiations, `X::from(value)` chooses among them by the rule for instantiations of one generic trait in [Method Resolution](#method-resolution).
7. r[trait.from.candidates] Each instantiation is a candidate, and the one whose parameter the argument fits is selected.
8. r[trait.from.no-fit] When no instantiation fits, the call is an error. Error: `type-mismatch`.

### Error Trait

The standard library declares the standard error trait `Error` in
`std.error`.

1. r[trait.error.module] The standard library declares the standard error trait `Error` in `std.error`.
2. r[trait.error.supertraits] `Error` is dynamically safe and has `Display` and the sealed `Inspectable` as supertraits, as in `trait Error < Display & Inspectable`.
3. r[trait.error.defaults] Every member `Error` declares has a default, so an implementation needs no body.
4. r[trait.error.complete] `impl Error for FsError` is complete when `FsError` implements `Display`, because the compiler supplies `Inspectable` for every inspectable type.
5. r[trait.error.not-inspectable] An `impl Error` whose target is not inspectable, such as a type declared in a block suite, is an error. Error: `missing-supertrait-implementation`.
6. r[trait.error.api] Those members, and error-chain helpers built on them, such as `cause`, `chain`, `find[T]`, and `root_cause`, are standard-library API.
7. r[trait.error.import] `Error` is not a prelude name; code imports it with `use std.error.Error`.
8. r[trait.error.no-inspect-import] Implementing `Error` needs no import of `std.inspect`.

```text
use std.error.Error

fn local() -> void:
    enum LocalError:
        Failed

    impl Display for LocalError:
        fn to_string(self) -> string: "failed"

    impl Error for LocalError  # error: missing-supertrait-implementation
    pass
```

See also: [Sealed Traits](#sealed-traits).

#### Erased Errors

1. r[trait.error.erased] The dynamic trait value `Error` is the erased application error.
2. r[trait.error.result] A `Result[T, Error]` holds any error that implements `Error`. `?` reaches it by assignability, constructing the dynamic value.
3. r[trait.error.entry-point] A dynamic trait value satisfies bounds on its own trait and its supertraits. `Error` therefore satisfies an entry point's `E < Display` requirement, so `pub fn main() -> Result[void, Error]` is a valid entry point.
4. r[trait.error.boundary] Like every dynamic trait value, an erased `Error` is not boundary-safe and never crosses a registered boundary.
5. r[trait.error.convert-first] Code converts an erased `Error` explicitly to a boundary-safe error type first.
6. r[trait.error.downcast] Because `Error` extends `Inspectable`, an erased `Error` inherits the `downcast` methods, so `error.downcast[FsError]()` recovers the concrete error.

```text
use std.error.Error

enum FsError:
    NotFound(path: string)

impl Display for FsError:
    fn to_string(self) -> string:
        match self:
            FsError.NotFound(path) => "not found: " + path

impl Error for FsError

fn read_config(path: string) -> Result[string, FsError]:
    .Err(FsError.NotFound(path))

fn load(path: string) -> Result[string, Error]:
    text := read_config(path)?
    .Ok(text.trim())
```

See also: [Propagation](05-expressions.md#propagation),
[Dynamic Trait Values](#dynamic-trait-values),
[Wasm Boundary](10-modules.md#wasm-boundary),
[Runtime Type Identity](#runtime-type-identity).

### Debug Trait

The standard library declares `Debug` in `std.format`, beside `Display`, to
show a value's structure:

```text
trait Debug:
    fn debug(self, out: mut DebugWriter) -> void
```

A type usually derives it:

```text
@derive(Eq, Debug)
data Point:
    x: i32
    y: i32

fn describe(point: Point) -> string:
    debug(point)
```

1. r[trait.debug.module] `std.format` declares `Debug` and `DebugWriter`. `Debug` is a prelude name.
2. r[trait.debug.method] `Debug` declares `fn debug(self, out: mut DebugWriter) -> void`, which writes the value's structure through `out`.
3. r[trait.debug.writer] `DebugWriter` is the standard structured writer. An implementation describes the value through its builder calls, such as one call per field, rather than raw text.
4. r[trait.debug.render] The prelude function `debug(value)` returns the text that `Debug` writes for `value`: stable, field by field, multi-line, and consistently indented.
5. r[trait.debug.std] `std` implements `Debug` for the primitives, collections, `T?`, `Result`, and tuples, each when its type arguments implement `Debug`.
6. r[trait.debug.derive] `@derive(Debug)` derives `Debug` through its [template](14-annotations.md#templates). The derived implementation walks the declaration's members and writes each one.
7. r[trait.debug.not-display] `Debug` is separate from `Display`, which stays user-facing text.
8. r[trait.debug.writer-import] `DebugWriter` is not a prelude name, so a hand-written implementation imports it, as in `use std.format.DebugWriter`.

#### Debug Builders

`DebugWriter` describes a value through builders, like Rust's `Formatter`:

```text
use std.format.DebugWriter

data Point:
    x: i32
    y: i32

impl Debug for Point:
    fn debug(self, out: mut DebugWriter) -> void:
        out.debug_struct("Point").field("x", self.x).field("y", self.y).finish()
```

| Rule | Call | Describes |
| --- | --- | --- |
| r[trait.debug.builder.struct] Struct | `out.debug_struct(name)`, then `.field(field_name, value)` per field, then `.finish()` | a named value with named fields |
| r[trait.debug.builder.tuple] Tuple | `out.debug_tuple(name)`, then `.field(value)` per field, then `.finish()` | a named value with positional fields |
| r[trait.debug.builder.list] List | `out.debug_list()`, then `.entry(value)` per item, then `.finish()` | a sequence |
| r[trait.debug.builder.map] Map | `out.debug_map()`, then `.entry(key, value)` per pair, then `.finish()` | key-value pairs |
| r[trait.debug.builder.write] Write | `out.write(text)` | custom text, for a value that no builder fits |

1. r[trait.debug.builder.types] `std.format` declares the builder types `DebugStruct`, `DebugTuple`, `DebugList`, and `DebugMap`, which the calls above return. None is a prelude name.
2. r[trait.debug.builder.values] Each `field` and `entry` value, and each `entry` key, must implement `Debug`. The builder writes it through its own `debug`.
3. r[trait.debug.builder.chain] `field` and `entry` return their builder, so calls chain, and `finish` ends the value.
4. r[trait.debug.layout] The writer chooses a compact or a pretty layout. An implementation's builder calls are the same for both.
5. r[trait.debug.derive-builders] `@derive(Debug)` generates builder calls, as a hand-written implementation writes them, so derived and hand-written text share one layout.
6. r[trait.debug.derive-builders.mapping] The derived calls follow Rust's `#[derive(Debug)]`, by the shape of each value, as the table below states.

| Rule | Value | Derived calls |
| --- | --- | --- |
| r[trait.debug.derive-builders.data] Data type | a value of a `data` type, with or without fields | `out.debug_struct(type_name)`, then `.field(field_name, value)` per field in declaration order, then `.finish()` |
| r[trait.debug.derive-builders.record] Record variant | a variant whose payload fields are all named | `out.debug_struct(variant_name)`, then `.field(field_name, value)` per payload field in order, then `.finish()` |
| r[trait.debug.derive-builders.tuple] Tuple variant | a variant whose payload fields are all positional | `out.debug_tuple(variant_name)`, then `.field(value)` per payload value in order, then `.finish()` |
| r[trait.debug.derive-builders.unit] Unit variant | a variant without a payload | `out.write(variant_name)` only |
| r[trait.debug.derive-builders.mixed] Mixed variant | a variant with both positional and named payload fields, such as `Mixed(i32, label: string)` | `out.debug_struct(variant_name)`, then `.field(field_name, value)` per payload field in order, where a positional field is named `_0`, `_1`, and so on by its position, then `.finish()`, printing `Mixed { _0: 1, label: "x" }` |

> **Why.** `assert_equal` and property tests show failing values through
> `Debug`, so any type a test compares can show itself without a
> user-facing `Display`. Builders keep the layout in the writer: plain
> writes would fix it in each implementation, which leaves no pretty mode
> or depth limit and lets derived and hand-written text drift apart.

> **Note.** The exact layout of `debug` text is standard-library API.
> Portable code and conformance fixtures do not depend on that text.

See also: [Standard Testing](10-modules.md#standard-testing),
[Typed Derivation](14-annotations.md#typed-derivation).

### Literal Suffix Trait

> **Note.** This heading keeps its name so that links to it stay valid.
> `std.ops.LiteralSuffix` and its newtype carriers were removed by
> Literal Suffixes L11 (2026-09-28):
> a suffix is a function marked `@num_suffix`, as
> [Literal Suffixes](05-expressions.md#literal-suffixes) defines.

### Implementation Declarations

An explicit implementation names the trait and target type:

```text
impl Display for User:
    fn to_string(self) -> string:
        self.email
```

1. r[trait.impl.explicit] An explicit implementation names the trait and target type.
2. r[trait.impl.required] The implementation must write every required method not supplied by a default. Omitting one is an error. Error: `missing-trait-method`.
3. r[trait.impl.fill] Only a method written in the implementation or a trait default fills a trait method.
4. r[trait.impl.fill.never] An inherent method of the target and a method promoted from an embedded field never fill a trait method.
5. r[trait.impl.override] The implementation may override a default with the exact instantiated signature.
6. r[trait.impl.signature] A mismatched method is an error. Error: `trait-method-signature`.
7. r[trait.impl.extra-methods] Additional methods do not become part of that trait implementation; place them in an inherent `impl` instead.
8. r[trait.impl.unique] At most one implementation of the same instantiated trait for the same target type may exist in a resolved program.

```text
trait Named:
    fn name(self) -> string

data User: pass
impl Named for User  # error: missing-trait-method
```

#### Method Generic Parameters

1. r[trait.impl.generics] The exact signature includes the method-level generic parameters.
2. r[trait.impl.generics.count] An implementation method declares as many generic parameters as the trait method, and they correspond by position; names may differ.
3. r[trait.impl.generics.markers] Each parameter keeps the trait method's `reified` and pack markers and the same bounds.
4. r[trait.impl.generics.bounds] The same bounds are the same traits, with `mut` and the same instantiated arguments and associated type bindings, written in the same order.
5. r[trait.impl.generics.fixed-bounds] An implementation method therefore cannot add, drop, reorder, weaken, or strengthen a bound. The one exception is the strengthened member bound of a walker, describer, or source, which [`annot.walker.strengthen-member`](14-annotations.md#r-annot.walker.strengthen-member) allows.
6. r[trait.impl.generics.error] Any mismatch is an error reported at the implementation method. Error: `trait-method-signature`.

```text
trait Show:
    fn show(self) -> string

trait Describe:
    fn describe[T](self, value: T) -> string

data Describer: pass

impl Describe for Describer:
    fn describe[T < Show](self, value: T) -> string: value.show()  # error: trait-method-signature
```

#### Local Implementations

1. r[trait.impl.local] An `impl` inside an executable block suite is a compile-time declaration.
2. r[trait.impl.local.trait] A local trait implementation must involve a local trait or a local nominal target type visible at its declaration point.
3. r[trait.impl.local.inherent] A local inherent implementation must target a local nominal type.
4. r[trait.impl.local.nonlocal] Implementations for a pair of nonlocal types belong at module scope.
5. r[trait.impl.local.checks] Local implementations obey the same target, ownership, overlap, and uniqueness checks as module-level implementations.
6. r[trait.impl.local.no-second] Lexical scope does not permit a second implementation for an existing pair.
7. r[trait.impl.local.no-capture] Local methods and local-trait default methods cannot capture enclosing runtime values.
8. r[trait.impl.local.lookup] Their methods are available for lookup from the local `impl` declaration point through its enclosing suite and child scopes, not before or outside that scope.

### Implementation Targets

The target of every implementation, trait or inherent, starts with a type
constructor.

| Rule | Type constructor | Examples |
| --- | --- | --- |
| r[trait.target.declaration] Declared | a data, enum, or newtype declaration | `User`, `Box` |
| r[trait.target.builtin] Built-in | a built-in type constructor | `i32`, `string`, `List`, `Map` |
| r[trait.target.tuple] Tuple | a tuple constructor, one per arity | `(A, B)`, the two-element tuple constructor applied to `A` and `B` |
| r[trait.target.function-type] Function | a function type constructor, `Fn` or `SuspendFn` | `fn(i32) -> i32`, which is `Fn[(i32,), i32, $()]`, and `SuspendFn[(Is...), O, R]` |

1. r[trait.target.constructor] The target of every implementation, trait or inherent, starts with a type constructor from the table above.
2. r[trait.target.tuple.valid] `impl Display for (i32, string)` is a valid target.
3. r[trait.target.tuple.arity] Tuples of different arity never share a constructor.
4. r[trait.target.option] An optional target is the prelude enum `Option` applied to its contained type. `impl Validate for string?` targets `Option[string]`, and by [Overlap](#overlap) it does not overlap an implementation for `i32?`.
5. r[trait.target.arguments] The constructor's arguments may be any types, including implementation parameters, as in `impl[T < Display] Printable for Box[T]`.
6. r[trait.target.bare-parameter] A target that is a bare type parameter, as in `impl[T] Describe for T`, is an error. Error: `bare-parameter-impl-target`.
7. r[trait.target.no-blanket] hd-lang has no blanket implementations over every type.
8. r[trait.target.function-type.valid] A function type is an ordinary target under the ownership and overlap rules below, so `impl Marker for fn(i32) -> i32` is valid in the package that declares `Marker`.
9. r[trait.target.row-argument] A row argument in an implementation head, such as a function type's row, is a row parameter or a concrete row.
10. r[trait.target.row-argument.extension] A row argument that lists a row parameter beside other keys, as in `Fn[(), i32, $ R + Log]`, is invalid in an implementation head.
11. r[trait.target.trait-value] A trait value type is never an implementation target either: `Display` used as a type names a dynamic trait value, not a type constructor.
12. r[trait.target.trait-value.error] `impl Marker for Display` and `impl Marker for Any` are errors. Error: `trait-value-impl-target`.
13. r[trait.target.trait-value.argument] A trait value type may still be a constructor's argument, as in `impl Marker for List[Display]`.
14. r[trait.target.no-mut] A target must not be written with an outer `mut`: `impl Marker for mut Counter` is an error. Error: `mutable-impl-target`.
15. r[trait.target.permission] Permission belongs to method receivers (`self` and `mut self`) and to bounds (`T < mut Trait`), not to implementations.
16. r[trait.target.both-views] One implementation for `X` serves both the readonly view `X` and the mutable view `mut X`. Lookup through either view considers the same implementations.
17. r[trait.target.mut-self] A `mut self` method still requires mutable access at each call.
18. r[trait.target.inner-mut] Only the outer `mut` of a target is banned. A `mut` inside the target's type arguments or the trait's arguments is part of the implementation's head.
19. r[trait.target.inner-mut.distinct] `impl Store[User] for Shelf` and `impl Store[mut User] for Shelf` therefore implement distinct trait instantiations and do not overlap.
20. r[trait.target.template] A derivation template, written `impl[T] Trait for T by Structure`, is not an implementation, so these target rules do not apply to it. Each derivation it produces is an ordinary implementation for a declared target, as [Templates](14-annotations.md#templates) defines.

```text
trait Describe:
    fn describe(self) -> string

trait Marker

data Counter:
    value: i32

impl[T] Describe for T:  # error: bare-parameter-impl-target
    fn describe(self) -> string:
        "value"

impl Marker for Display      # error: trait-value-impl-target
impl Marker for mut Counter  # error: mutable-impl-target
```

A function type is a target like any other:

```text
use std.function.Fn

trait Describe:
    fn describe(self) -> string

impl[Is..., O, R] Describe for Fn[(Is...), O, R]:
    fn describe(self) -> string:
        "function"
```

> **Note.** A later revision may add blanket implementations as a compatible
> extension.

### Implementation Ownership

An `impl Trait[Args] for Target` may be declared only in a package that owns
one of these declarations:

| Rule | Owned declaration |
| --- | --- |
| r[trait.own.trait] Trait | the trait |
| r[trait.own.target] Target | the target's outer type constructor |
| r[trait.own.argument] Trait argument | the outer type constructor of one of the trait arguments `Args` |

1. r[trait.own.rule] An `impl Trait[Args] for Target` may be declared only in a package that owns one of the declarations in the table above.
2. r[trait.own.orphan] Any other trait implementation is an error. Error: `orphan-impl`.
3. r[trait.own.no-orphan-exception] No package, including the root application package, has an orphan exception.
4. r[trait.own.argument.example] For example, the package that declares `Money` may write `impl Add[Money] for i32`, because it owns the trait argument `Money`.
5. r[trait.own.bare-parameter] The trait-argument case never applies to a target that is a bare type parameter.
6. r[trait.own.aliases] Transparent aliases do not create ownership; nominal newtypes do.
7. r[trait.own.std] The standard library owns primitives, built-in collection type constructors, tuple constructors, and the prelude enums `Option` and `Result`.
8. r[trait.own.optional] An implementation for `string?` therefore needs the package of the trait or of a trait argument. An example is `impl Validate for string?` in the package that owns `Validate`.
9. r[trait.own.std.function] The standard library also owns the function type constructors `Fn` and `SuspendFn`. An implementation for a function type therefore needs the package of the trait or of a trait argument.
10. r[trait.own.inherent] An inherent implementation may be declared only in the package that owns its target nominal type.
11. r[trait.own.inherent.target-kinds] An inherent implementation cannot target a trait value, tuple, transparent alias, or type owned by another package.
12. r[trait.own.inherent.std] The standard library, which owns them, may declare inherent implementations for primitives, built-in collection type constructors, and the prelude enums `Option` and `Result`.
13. r[trait.own.inherent.std.no-use] Their `pub` members are found by ordinary member lookup on the receiver's type, so calling one needs no `use`.
14. r[trait.own.inherent.std.no-tuple] Tuples have no inherent members, including from the standard library; they get only trait implementations.
15. r[trait.own.graph] The compiler must also reject a resolved dependency graph containing duplicate exact implementations. This includes the possible conflict where two owning packages each provide the same pair.

```text
impl Display for i32:  # error: orphan-impl
    fn to_string(self) -> string:
        "$self"

impl Display for fn() -> i32:  # error: orphan-impl
    fn to_string(self) -> string:
        "function"
```

> **Why.** These ownership rules prevent downstream packages from creating
> globally surprising conformance.

#### Implementation Modules

1. r[trait.own.module.inherent-target] An inherent implementation must be declared in the module that declares its target type, except as `trait.own.module.inherent.std` allows.
2. r[trait.own.module.inherent.std] An inherent implementation that `trait.own.inherent.std` allows may be declared in any module of the standard library.
3. r[trait.own.module.trait] A trait implementation must be declared in a module that declares the trait, the target's outer type constructor, or the outer type constructor of a trait argument that gives its package ownership.
4. r[trait.own.module.error] An implementation declared in any other module of the owning package is an error. Error: `nonlocal-impl`.
5. r[trait.own.module.generated] A derived implementation is generated in the module of its declaration, so it always satisfies these rules.

> **Why.** A reader of the type's or the trait's module sees every
> implementation that can answer a call, and no distant module of the package
> can add one.

### Overlap

Implementations may be generic and state their bounds inline in the generic
parameter list:

```text
impl[T < Display] Printable for Box[T]:
    fn print(self) -> string:
        self.value.to_string()
```

1. r[trait.overlap.generic] Implementations may be generic and state their bounds inline in the generic parameter list.
2. r[trait.overlap.constrained] All generic implementation parameters must be constrained by the implemented trait, target type, or a bound reachable from them.
3. r[trait.overlap.definition] Two implementations overlap when they implement the same trait and their full heads unify.
4. r[trait.overlap.unify] Heads unify when, after each implementation's parameters are renamed apart, one substitution makes both their trait arguments and their complete target types equal.
5. r[trait.overlap.heads-only] Overlap is decided from the implementation heads alone.
6. r[trait.overlap.no-bounds] Bounds, including associated type bindings, are never used to claim that two implementations are disjoint.
7. r[trait.overlap.error] Overlapping implementations are an error. Error: `overlapping-impl`.

| First implementation | Second implementation | Overlap |
| --- | --- | --- |
| `impl[T] Marker for List[T]` | `impl Marker for List[i32]` | yes |
| `impl[T] Marker for Box[T]` | `impl Marker for Box[i32]` | yes |
| `impl Marker for Box[i32]` | `impl Marker for Box[string]` | no |
| `impl Add[i32] for Money` | `impl Add[Money] for Money` | no |
| `impl[Is..., O, R] Marker for Fn[(Is...), O, R]` | `impl Marker for fn(i32) -> i32` | yes |
| `impl Marker for fn(i32) -> i32` | `impl Marker for fn(string) -> i32` | no |

```text
data Box[T]:
    value: T

data Plain:
    value: i32

trait Marker

impl[T < Display] Marker for Box[T]
impl Marker for Box[Plain]  # error: overlapping-impl
```

> **Note.** Because bounds are ignored, an implementation added later in a
> dependency cannot make two existing implementations overlap. A pack
> parameter is substituted by a sequence of types, so `(Is...)` unifies with
> every tuple type, and a row parameter unifies with every row.

## Inherent Implementations

An inherent implementation, written `impl T:` without a trait, declares
members attached directly to the nominal type `T`:

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]
```

### Inherent Members

1. r[trait.inherent.decl] An inherent implementation, written `impl T:` without a trait, declares members attached directly to the nominal type `T` rather than through a trait.
2. r[trait.inherent.members] Its members are the type's inherent members, which the table below defines.
3. r[trait.inherent.owner] Only the package that owns `T` may declare them.
4. r[trait.inherent.vs-trait] An inherent method differs from a trait method, which a trait declares and a trait implementation supplies for `T`.
5. r[trait.inherent.vs-promoted] An inherent method also differs from a promoted method, which belongs to the type of an embedded field and is reached through the outer type.

| Member | Receiver | Called |
| --- | --- | --- |
| **inherent method** | `self` or `mut self` as its first parameter | with dot syntax, as in `user.domain()` |
| **inherent associated function** | none | through the type, as in `User::guest()` |

See also: [Implementation Ownership](#implementation-ownership),
[Member Resolution](03-names-and-scopes.md#member-resolution).

### Method Visibility

1. r[trait.inherent.private] Inherent methods and inherent associated functions are module-private unless individually marked `pub`, including when their nominal type is public.
2. r[trait.vis.trait-methods] Methods in trait declarations and trait implementations follow the trait's visibility.
3. r[trait.vis.no-pub] `pub` is not written on an individual trait method or its implementation.
4. r[trait.vis.uniform] A trait has one visibility level for all its methods; it cannot mix public and private methods.

### Inherent Member Names

1. r[trait.inherent.unique-unifying] Two inherent members with one name are an error when their implementations' targets unify, including two members of one implementation. Error: `duplicate-inherent-member`.
2. r[trait.inherent.unique-unifying.disjoint] Inherent implementations of one type constructor whose targets cannot unify may repeat a member name. `impl Box[i32]` and `impl Box[string]` may each declare `show`, but `impl[T] Box[T]` and `impl Box[i32]` may not.
3. r[trait.inherent.unique-unifying.unify] Targets unify as implementation heads do in [Overlap](#overlap), after renaming each implementation's parameters apart.
4. r[trait.inherent.target-match] An inherent member is a member of a receiver's type only when its implementation's target matches that type, so `box.show()` on a `Box[string]` calls the member of `impl Box[string]`.
5. r[trait.inherent.field-names] An inherent member may share its name with a field of the type, named or embedded, because fields and methods are separate namespaces.
6. r[trait.inherent.no-overloading] hd-lang has no method or associated-function overloading.

```text
data User: pass

impl User:
    fn name(self) -> string: "first"
    fn name(self) -> string: "second"  # error: duplicate-inherent-member

data Box[T]:
    value: T

impl[T] Box[T]:
    fn show(self) -> string: "box"

impl Box[i32]:
    fn show(self) -> string: "int box"  # error: duplicate-inherent-member
```

See also: [Member Resolution](03-names-and-scopes.md#member-resolution).

### Inherent Associated Functions

Inherent associated functions are called through the nominal type:

```text
impl User:
    fn guest() -> User:
        User { name: "guest" }

guest := User::guest()
```

1. r[trait.inherent.assoc-call] Inherent associated functions are called through the nominal type.

## Method Resolution

This section defines which trait methods are usable at a use and how trait
candidates are reported.

1. r[trait.resolve.lookup] For a receiver of nominal type `S` or `mut S`, `value.method(args)` selects a method with the method lookup in [Member Resolution](03-names-and-scopes.md#member-resolution).
2. r[trait.resolve.lookup.kinds] That lookup covers inherent methods, trait methods, and promoted methods together.
3. r[trait.resolve.no-field] It never selects a field.

### Trait Availability

For a concrete receiver, a trait is available to dot-call lookup when its
name is one of these:

| Rule | The trait's name is |
| --- | --- |
| r[trait.avail.module] Module | declared in the current module, or introduced by a use declaration in it |
| r[trait.avail.scope] Scope | visible in the current lexical scope |
| r[trait.avail.prelude] Prelude | supplied by the prelude |

1. r[trait.avail.concrete] For a concrete receiver, a trait is available to dot-call lookup when its name is one of the cases in the table above.
2. r[trait.avail.generic] For a generic receiver, its declared bounds are also available.
3. r[trait.avail.dynamic] A dynamic trait value always exposes the methods of its own erased trait.
4. r[trait.avail.no-injection] An implementation in the dependency graph does not inject its trait's method names into every module that can name the target type.
5. r[trait.avail.not-candidate] A method of a trait that is not available is not a candidate at all, wherever its implementation is declared.
6. r[trait.avail.as-absent] Lookup proceeds as if the implementation were absent, so a promoted method of the same name may be selected.
7. r[trait.avail.suggest] A call that finds no method should suggest a use declaration for the trait.

### Inherent Methods Win

1. r[trait.resolve.inherent-first] A visible inherent method of the receiver's nominal type is always selected over trait methods, including a method of a trait that the same type implements.

> **Why.** Only the package that owns the type can declare an inherent
> method, so no other package can change which method such a call reaches.

### Ambiguous Methods

1. r[trait.resolve.ambiguous] Suppose more than one available trait that the receiver implements supplies a method with that name, and no inherent method of that name is usable. The call is then an error. Error: `ambiguous-method`.
2. r[trait.resolve.ambiguous.promoted] A call where an available trait method of the receiver's type meets a promoted method of the same name is also an error: neither silently wins over the other. Error: `ambiguous-method`.
3. r[trait.resolve.ambiguous.defaults] This holds whether each method is written in its implementation or comes from a default.
4. r[trait.resolve.no-ranking] The compiler does not select by conversion ranking or declaration order.
5. r[trait.resolve.qualified-fix] A trait-qualified call resolves the ambiguity.

```text
trait Left:
    fn label(self) -> i32
trait Right:
    fn label(self) -> i32
data User:
    value: i32
impl Left for User:
    fn label(self) -> i32: self.value
impl Right for User:
    fn label(self) -> i32: self.value
fn main() -> i32: User { value: 42 }.label()  # error: ambiguous-method
```

See also: [Member Resolution](03-names-and-scopes.md#member-resolution).

### Instantiations Of One Generic Trait

A receiver may implement one generic trait at several instantiations that
each supply the method, as with `impl Add[i32] for Money` and
`impl Add[Money] for Money`.

1. r[trait.resolve.instantiation] When the receiver implements one generic trait at several instantiations that each supply the method, the call chooses the instantiation.
2. r[trait.resolve.instantiation.candidate] Each instantiation is a candidate.
3. r[trait.resolve.fits] A candidate **fits** when the call's arguments check against its method's parameter types, with that instantiation's trait arguments substituted.
4. r[trait.resolve.fits.expected] When the call has an expected type, a candidate fits only if, in addition, the method's result type is assignable to it.
5. r[trait.resolve.one-fit] Exactly one fitting candidate is selected, so `price.add(5)` calls the `Add[i32]` method.
6. r[trait.resolve.literal-default] Suppose two or more candidates fit. If exactly one of them fits with every integer literal argument at `i32` and every floating-point literal argument at `f64`, the literals' default types, that candidate is selected.
7. r[trait.resolve.literal-default.example] With `impl Add[i32] for Money` and `impl Add[i64] for Money`, `price.add(5)` calls the `Add[i32]` method.
8. r[trait.resolve.many-fit] Otherwise two or more fitting candidates are an error, and a trait-qualified call such as `Add[i64]::add(price, 5)` resolves it. Error: `ambiguous-method`.
9. r[trait.resolve.no-fit] When no candidate fits, the call is an error whose message lists the available instantiations. Error: `type-mismatch`.
10. r[trait.resolve.one-trait-only] This choice applies only among instantiations of one trait. Methods of two different traits stay ambiguous whatever the argument types. Error: `ambiguous-method`.

```text
trait Pick[T]:
    fn pick(self) -> T

data Money:
    cents: i32

impl Pick[i32] for Money:
    fn pick(self) -> i32:
        self.cents

impl Pick[string] for Money:
    fn pick(self) -> string:
        "money"

fn invalid(price: Money) -> void:
    value := price.pick()  # error: ambiguous-method
```

### Trait-Qualified Calls

Select one trait explicitly with `Trait::method(receiver, arguments...)`:

```text
label := Display::to_string(value)
sum := Add[Money]::add(left, right)
```

1. r[trait.qualified.form] `Trait::method(receiver, arguments...)` selects one trait explicitly.
2. r[trait.qualified.receiver] The receiver is the first ordinary argument and must implement the named trait instantiation.
3. r[trait.qualified.arguments] Remaining arguments follow normal positional/named ordering.
4. r[trait.qualified.bypass] This form bypasses member lookup and selects exactly the named trait method.
5. r[trait.qualified.type-arguments] A generic trait method takes its explicit type arguments after the method name, as in `Identity::select[i32](picker, 42)`.
6. r[trait.qualified.trait-arguments] The trait's own type arguments stay before `::`.
7. r[trait.qualified.generic-rules] The type argument list follows the rules of [Generic Functions](07-functions.md#generic-functions).

### Associated Function Calls

An associated function is called through a type, a type parameter, or a
trait:

```text
trait Factory:
    fn create() -> Self

data User:
    name: string

impl Factory for User:
    fn create() -> User:
        User { name: "new" }

fn make[T < Factory]() -> T:
    T::create()

first := User::create()
second := make[User]()
```

1. r[trait.assoc-call.type] `Type::f(args)` first looks for an inherent associated function or method `f` of `Type`.
2. r[trait.assoc-call.type.traits] When `Type` has no inherent member `f`, the candidates are the members named `f` of the available traits that `Type` implements.
3. r[trait.assoc-call.type.ambiguous] Two or more trait candidates are an error, and a trait-qualified call resolves it. Error: `ambiguous-method`.
4. r[trait.assoc-call.type.none] No candidate at all is an error. Error: `unknown-method`.
5. r[trait.assoc-call.parameter] `T::f(args)`, where `T` is a type parameter, resolves `f` through the bounds of `T`, and the call uses the bound's evidence.
6. r[trait.assoc-call.parameter.one] Exactly one trait among the bounds of `T` and their supertraits must declare `f`. Two or more are an error. Error: `ambiguous-method`.
7. r[trait.assoc-call.parameter.static] `T::f()` is always resolved statically through a bound, never through a runtime type object.
8. r[trait.assoc-call.trait] `Trait::f(args)` for an associated function `f` infers `Self` like a generic argument of the call, from the arguments and the expected type.
9. r[trait.assoc-call.trait.undetermined] A `Trait::f(args)` call whose `Self` that inference does not determine is invalid; write `Type::f(args)` or `T::f(args)` instead.

See also: [Trait-Qualified Calls](#trait-qualified-calls),
[Trait Availability](#trait-availability).

## Generic Bounds And Static Dispatch

A generic bound requires explicit conformance and uses static dispatch:

```text
fn show[T < Display](value: T) -> string:
    value.to_string()
```

Bounds compose with `&`:

```text
fn audit[T < Display & Named](value: T) -> string:
    value.to_string() + " / " + value.name()
```

1. r[trait.bound.static] A generic bound requires explicit conformance and uses static dispatch.
2. r[trait.bound.compose-and] Bounds compose with `&`: the type must implement every trait joined by it.
3. r[trait.bound.repeat] A bound may list the same trait more than once, as in `T < Display & Display`. The repetition adds no requirement and is not diagnosed.
4. r[trait.bound.mut] `T < mut Trait` additionally requires `T` to be a mutable-root type.
5. r[trait.bound.mut-any] `T < mut Any` requires mutable-root access without a type-specific behavior requirement.
6. r[trait.bound.unsatisfied] A type argument, explicit or inferred, that does not implement a trait its parameter's bound requires is an error. Error: `unsatisfied-trait-bound`.
7. r[trait.bound.unsatisfied.cases] This includes a non-reference type for `T < AnyRef`, a reference type for `T < AnyVal`, and a readonly argument for `T < mut Trait`.
8. r[trait.bound.depth] A bound required directly by a use has depth 1. A bound of the implementation that proves a bound of depth `n` has depth `n + 1`.
9. r[trait.bound.depth.limit] A proof that needs a bound of depth greater than 64 is an error, whether or not a deeper proof would succeed. Error: `trait-resolution-depth`.
10. r[trait.bound.depth.fixed] The limit is fixed by this specification; no package, module, or compiler option changes it.
11. r[trait.bound.representation] The compiler may monomorphize static calls, share one body among instantiations, or use another representation.
12. r[trait.bound.representation.semantics] The chosen representation must preserve the observable semantics, including reflection behavior for reified parameters.

```text
trait Clear:
    fn clear(mut self) -> void

data Counter:
    value: i32

impl Clear for Counter:
    fn clear(mut self) -> void:
        self.value = 0

fn clear_value[T < mut Clear](value: T) -> void:
    value.clear()

fn reject_readonly() -> void:
    counter := Counter { value: 42 }
    clear_value(counter)  # error: unsatisfied-trait-bound
```

> **Why.** A fixed limit makes every implementation accept the same programs,
> and it ends bound solving that would otherwise never finish.

See also: the non-normative
[Implementation Model](04-type-system.md#implementation-model-non-normative).

### Associated Type Bindings

A trait in a generic parameter bound may bind associated types after its
positional type arguments:

```text
trait Supplier:
    type Item
    fn get(self) -> Self::Item

fn describe[T < Display, I < Supplier[Item = T]](source: I) -> string:
    source.get().to_string()

data Feed[I]:
    source: I

impl[T < Display, I < Supplier[Item = T]] Display for Feed[I]:
    fn to_string(self) -> string:
        describe(self.source)
```

1. r[trait.binding.form] A trait in a generic parameter bound may bind associated types after its positional type arguments.
2. r[trait.binding.meaning] `I < Supplier[Item = T]` means that `I` implements `Supplier` and that its associated type `Item` equals `T`.
3. r[trait.binding.equality] The binding is an equality constraint on the projection `I::Item`, not a new type.
4. r[trait.binding.inside] Inside the declaration, `I::Item` remains a valid projection and denotes the same type as `T`.
5. r[trait.binding.interchangeable] The two spellings are interchangeable in parameter, result, and body types.
6. r[trait.binding.use-site] At a use site, after substitution, the argument's implementation of the trait must bind the associated type to the bound type.
7. r[trait.binding.inference] The constraint takes part in generic argument inference, so `T` above is inferred from the `Supplier` implementation of the argument passed for `I`.
8. r[trait.binding.mismatch] An argument whose implementation binds a different type is an error. Error: `unsatisfied-trait-bound`.
9. r[trait.binding.reachable] A binding counts as a bound reachable from its parameter.
10. r[trait.binding.reachable.example] In the implementation above, `T` is therefore constrained through `I`. The implementation satisfies the rule that every generic implementation parameter be constrained.

#### Binding Names

1. r[trait.binding.scope] The bound type may name any parameter of the same generic parameter list.
2. r[trait.binding.own-trait] A binding name must be an associated type declared by the named trait itself. Naming anything else, including an associated type that only a supertrait declares, is an error. Error: `unknown-associated-type`.
3. r[trait.binding.supertrait] Bind a supertrait's associated type with a separate bound on that supertrait.
4. r[trait.binding.once] Each projection may be bound at most once in one generic parameter list.
5. r[trait.binding.once.error] A second binding of the same parameter's associated type, in the same bound or another bound, is an error even when both bindings name the same type. Error: `duplicate-associated-binding`.

```text
trait Supplier:
    type Item
    fn get(self) -> Self::Item

fn first[T, I < Supplier[Element = T]](source: I) -> T:  # error: unknown-associated-type
    source.get()

fn second[T, I < Supplier[Item = T, Item = T]](source: I) -> T:  # error: duplicate-associated-binding
    source.get()
```

#### Binding Positions

1. r[trait.binding.positions-supertrait] Bindings appear only in generic parameter bounds and in supertrait lists.
2. r[trait.binding.rejected-other] The trait of an `impl` header, a trait-qualified call, a type argument, and a dynamic trait value type do not accept them. The grammar reports an error there. Error: `syntax-error`.
3. r[trait.binding.ambiguous] A binding does not make an ambiguous projection unambiguous: when two bounds on `I` both declare `Item`, `I::Item` is still ambiguous even if one of them binds it.

#### Supertrait Bindings

A supertrait may bind an associated type of its trait, so every
implementation fixes it:

```text
use std.ops.Add

trait Summable < Add[Self, Out = Self]

fn double[T < Summable](value: T) -> T:
    value + value
```

1. r[trait.binding.super.form] A trait in a supertrait list may bind associated types after its positional arguments, as a bound does.
2. r[trait.binding.super.meaning] `trait C < S[A, Out = U]` requires that a type `X` implementing `C` implement `S[A]` with `Out` equal to `U`, where `A` and `U` read `X` for `Self`.
3. r[trait.binding.super.projection] For a type parameter `T < C`, and for `Self` inside `C`, the projection is known to equal `U`. So `value + value` above has type `T`.
4. r[trait.binding.super.mismatch] An `impl C for X` whose implementation of `S[A]` binds `Out` to another type is an error. Error: `missing-supertrait-implementation`.
5. r[trait.binding.super.names] The rules of [Binding Names](#binding-names) apply to a supertrait list as to a generic parameter list.

```text
use std.ops.Add

trait Summable < Add[Self, Out = Self]

data Money:
    cents: i64

impl Add[Money] for Money:
    type Out = i64
    fn add(self, rhs: Money) -> i64:
        self.cents + rhs.cents

impl Summable for Money  # error: missing-supertrait-implementation
```

## Dynamic Trait Values

Using a trait name directly as a value type creates a Go-style dynamic trait
value:

```text
fn print_display(value: Display) -> void $ Console:
    println(value.to_string())
```

1. r[trait.dyn.form] Using a trait name directly as a value type creates a Go-style dynamic trait value.
2. r[trait.dyn.contents] Such a value contains a concrete value plus dispatch metadata for the trait.
3. r[trait.dyn.no-dyn] There is no `dyn` marker.
4. r[trait.dyn.methods] Only methods declared by the trait are available through the erased value.

### Dynamic Safety

1. r[trait.dyn.safe] The trait of a dynamic trait value must be dynamically safe.
2. r[trait.dyn.safe.one-copy] A trait is dynamically safe when every method of it and of its supertraits compiles to one copy: one body shared by every implementation's callers, with no per-call specialization.
3. r[trait.dyn.safe.error] Using a trait that is not dynamically safe as a value type is an error. Error: `trait-not-dynamically-safe`.

The one-copy rule has these consequences:

| Rule | Form | Dynamically safe |
| --- | --- | --- |
| r[trait.dyn.safe.no-assoc] Associated items | an associated type or associated function in the trait or a supertrait | no |
| r[trait.dyn.safe.anyref-type-param] Method type parameters | a method-level type parameter bounded by `AnyRef`, with any further bounds | yes; any other method-level type parameter is not |
| r[trait.dyn.safe.self] `Self` | `Self` as a method receiver | yes; `Self` anywhere else is not |
| r[trait.dyn.safe.reified-or-pack] Specialized parameters | a `reified` parameter, or a type or value pack | no |
| r[trait.dyn.safe.suspending] Suspending methods | a suspending method, as the prelude `Console`'s `write_line!` is | yes |
| r[trait.dyn.safe.row-parameter] Row parameters | a method-level row parameter, which needs no `AnyRef` bound | yes |

4. r[trait.dyn.static-still] A trait that is not dynamically safe can still be implemented and used as a static generic bound.
5. r[trait.dyn.generic-trait] Generic parameters of the trait itself are allowed when the value type names one complete instantiation.

```text
trait Runner:
    fn run[R](self, job: fn() -> void $ R) -> void $ R

trait Logger:
    fn log[Ts... < AnyRef](self, values: Ts...) -> void

fn valid(runner: Runner) -> void:
    pass

fn invalid(logger: Logger) -> void:  # error: trait-not-dynamically-safe
    pass
```

> **Why.** A method called through a trait value has exactly one body at run
> time. An `AnyRef`-bounded parameter shares the reference shape, a
> suspending method's frame is a reference-shaped heap value, and a row
> parameter's providers arrive as one bundle, so each keeps one body. A
> `reified` parameter and a pack are specialized per call, and an associated
> function has no receiver to dispatch on.

See also: [Trait Values And `Any`](04-type-system.md#trait-values-and-any).

### Supertrait Widening

1. r[trait.dyn.supertrait-methods] A child-trait bound or dynamic value exposes the methods of its transitive supertraits.
2. r[trait.dyn.widen] A dynamic child-trait value widens implicitly to a supertrait value, losing access to child-only methods.
3. r[trait.dyn.no-narrow] No conversion reverses the widening.
4. r[trait.dyn.recover] Only a value of `Inspectable` or of a trait that extends it can recover its concrete type, through [Runtime Type Identity](#runtime-type-identity).

### Trait Values As Bounds

```text
trait Named < Display:
    fn name(self) -> string

fn show[T < Display](value: T) -> string:
    value.to_string()

fn tag[T < Named](value: T) -> string:
    value.name()

fn describe(named: Named, shown: Display) -> string:
    tag(named) + show(named) + show(shown)
```

1. r[trait.dyn.bound] A dynamic trait value type satisfies a generic bound on its own trait and on each direct or transitive supertrait of that trait.
2. r[trait.dyn.bound.instantiation] For a generic trait, the bound must name the same instantiation, so `Repository[User]` satisfies `T < Repository[User]`.
3. r[trait.dyn.bound.dispatch] A statically dispatched call through such a bound dispatches each method through the value's table.
4. r[trait.dyn.bound.mut] A readonly trait value never satisfies a `mut` bound; `mut Tr` satisfies `T < mut Tr`.
5. r[trait.dyn.bound.no-impl] The rule adds no implementation: the trait value type satisfies no other bound through it, and it still cannot be an implementation target.

> **Why.** Dynamic safety guarantees the trait has no associated function or
> associated type that a bound could need.

### Conversion To Trait Values

1. r[trait.dyn.convert] Converting a concrete value to a trait value requires an explicit implementation.
2. r[trait.dyn.convert.any-type] The concrete type can be composite or primitive.
3. r[trait.dyn.mut] Mutable dynamic access uses `mut Trait` and cannot be recovered from a readonly `Trait` value.
4. r[trait.dyn.type-test] A dynamic trait value supports a type test only when its trait is `Inspectable` or extends it. The test recovers an exact concrete type.
5. r[trait.dyn.no-trait-test] No test asks whether a value implements another trait.

See also: [Runtime Type Identity](#runtime-type-identity).

## `Any`

`Any` is the universal empty trait.

1. r[trait.any.universal] `Any` is the universal empty trait.
2. r[trait.any.all] Every value type, including an optional type, implements `Any` automatically.
3. r[trait.any.erases] As a value type, `Any` erases the concrete type and exposes no type-specific methods.
4. r[trait.any.optional] An optional value erases to `Any` like any other enum value.
5. r[trait.any.none] A bare `.None` still needs an expected optional type. `let value: Any = .None` is an error, while `let value: Any? = .None` is valid. Error: `missing-contextual-enum-type`.
6. r[trait.any.mut] `mut Any` preserves mutable access to an erased composite value.

```text
let invalid: Any = .None  # error: missing-contextual-enum-type
```

## Sealed Traits

A **sealed trait** is a standard trait whose implementations only the
compiler and the standard library supply.

| Rule | Trait | Implemented for |
| --- | --- | --- |
| r[trait.sealed.any] Any | `Any` | every value type ([`Any`](#any)) |
| r[trait.sealed.anyval-types] AnyVal | `AnyVal` | the primitive types, `string`, `void`, tuples, and newtypes over them ([Trait Values And `Any`](04-type-system.md#trait-values-and-any)) |
| r[trait.sealed.anyref] AnyRef | `AnyRef` | the reference values ([Trait Values And `Any`](04-type-system.md#trait-values-and-any)) |
| r[trait.sealed.suspend] Suspend | `Suspend[T]` | compiler-generated suspension frames and `std.task` types ([`Suspend[T]` Protocol](11-requirements-and-suspension.md#suspendt-protocol)) |
| r[trait.sealed.shape-metadata] ShapeMetadata | `ShapeMetadata` | the concrete shape types ([Common Shape Representation](14-annotations.md#common-shape-representation)) |
| r[trait.sealed.inspectable] Inspectable | `Inspectable` | the inspectable types ([Inspectable Types](#inspectable-types)) |
| r[trait.sealed.structure] Structure | `Structure` | each derivation's target, while its template is instantiated ([The Structure Trait](14-annotations.md#the-structure-trait)) |
| r[trait.sealed.num] Num | `Num` | every integer and floating-point type ([Numeric Traits](#numeric-traits)) |
| r[trait.sealed.integer] Integer | `Integer` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, and `u64` ([Numeric Traits](#numeric-traits)) |
| r[trait.sealed.float] Float | `Float` | `f32` and `f64` ([Numeric Traits](#numeric-traits)) |

1. r[trait.sealed.definition] A sealed trait is a standard trait whose implementations only the compiler and the standard library supply.
2. r[trait.sealed.list] The sealed traits are those in the table above.
3. r[trait.sealed.use-positions-structure] User code may name a sealed trait in a bound, as a supertrait, and, where the trait is dynamically safe, as a value type. `Structure` is the exception: it may be named only where [`annot.structure.named-positions`](14-annotations.md#r-annot.structure.named-positions) allows.
4. r[trait.sealed.no-user-impl] User code cannot implement a sealed trait.
5. r[trait.sealed.user-impl] An `impl` of a sealed trait outside the standard library is an error, reported on the `impl` line, whatever its target. Error: `sealed-trait-implementation`.

```text
use std.inspect.{Inspectable, TypeId}

data User:
    name: string

data Handle: pass

impl Inspectable for User:  # error: sealed-trait-implementation
    fn runtime_type(self) -> TypeId: TypeId::of[string]()

impl AnyRef for Handle  # error: sealed-trait-implementation
```

### Compiler-Supplied Implementations

1. r[trait.sealed.supplied] The compiler supplies the implementations listed for each sealed trait.
2. r[trait.sealed.behaves] A compiler-supplied implementation behaves like an explicit one: it satisfies bounds, constructs dynamic trait values, and counts for supertrait checks.
3. r[trait.sealed.no-replace] A compiler-supplied implementation cannot be replaced or overridden.
4. r[trait.sealed.member-names] A trait that has a sealed trait as a direct or transitive supertrait must not declare a member with the name of one of that sealed trait's members.
5. r[trait.sealed.member-names.impl] An implementation of such a trait must not write such a member either.
6. r[trait.sealed.member-names.error] Either is an error, reported on the member, in place of `duplicate-trait-member`. Error: `sealed-trait-implementation`.
7. r[trait.sealed.inherent-ok] An inherent method with the same name is allowed.
8. r[trait.sealed.inherent-lookup] Ordinary method lookup finds that inherent method on the concrete type, and it changes nothing that the compiler-supplied implementation reports.

### Extending A Sealed Trait

1. r[trait.sealed.extend] A trait that extends a sealed trait is declared and implemented normally.
2. r[trait.sealed.extend.members] Its implementation writes only the child's members; the sealed supertrait's implementation comes from the compiler.
3. r[trait.sealed.extend.missing] When the target is not a type the compiler supplies the sealed trait for, the implementation is an error, as for any missing supertrait. Error: `missing-supertrait-implementation`.

### Numeric Traits

`std.num` declares three sealed traits for the primitive number types, so
generic code can compute over any of them:

```text
use std.num.Num

fn sum[T < Num](items: List[T]) -> T:
    let total = T::zero()
    for item in items:
        total = total + item
    total

fn kilo[N < Num](n: N) -> N:
    n * N::from_i64(1000)
```

`Num` is declared in this shape:

```text
pub trait Num < AnyVal & PartialOrd & Display & Add[Self, Out = Self] & Sub[Self, Out = Self] & Mul[Self, Out = Self] & Div[Self, Out = Self] & Rem[Self, Out = Self]:
    fn zero() -> Self
    fn one() -> Self
    fn from_i64(n: i64) -> Self
```

| Rule | Trait | Supertraits |
| --- | --- | --- |
| r[trait.num.num-ordered] Num | `Num` | `AnyVal`, `PartialOrd` (and so `Eq`), `Display`, and `Add`, `Sub`, `Mul`, `Div`, and `Rem`, each as `[Self, Out = Self]` |
| r[trait.num.integer] Integer | `Integer` | `Num`, `Ord`, `BitAnd`, `BitOr`, and `BitXor`, each as `[Self, Out = Self]`, `Not[Out = Self]`, `Shl[u32, Out = Self]`, and `Shr[u32, Out = Self]` |
| r[trait.num.float] Float | `Float` | `Num`, `PartialOrd`, and `Neg[Out = Self]` |

1. r[trait.num.module] `std.num` declares `Num`, `Integer`, and `Float`, with the supertraits in the table above.
2. r[trait.num.sealed] The three traits are sealed. They stand for the built-in primitive number types only, and only the standard library implements them.
3. r[trait.num.not-newtypes] A newtype over a number, such as `type Meters(i64)`, and a library number type, such as a `BigInt`, are not `Num`. They implement the operator traits they need by hand.
4. r[trait.num.members] `Num` declares `zero`, `one`, and `from_i64`. `Integer` and `Float` may declare further library methods, such as `checked_add` and `is_nan`, which this specification does not list.
5. r[trait.num.zero-one] `T::zero()` and `T::one()` are the values 0 and 1 of `T`. A numeric literal never has a type parameter's type, so generic code builds constants from these functions.
6. r[trait.num.from-i64-checked] `T::from_i64(n)` is checked. For an integer type `T`, it returns `n` as a `T` when `T` can hold it, and otherwise panics with `integer-overflow`. A floating-point type takes the nearest value.
7. r[trait.num.from-i64-cast] A [numeric cast](04-type-system.md#numeric-casts) `T(n)` stays the way to wrap.
8. r[trait.num.division] `/` and `%` keep each type's own meaning under `Num`. Integer division truncates and panics on a zero divisor, and floating-point division follows IEEE 754.
9. r[trait.num.std-operators] The standard library's operator implementations behave as the built-in operators do.
10. r[trait.num.suffix] A suffix function may be generic over `N < Num`, as [`expr.suffix.fn-shape-param`](05-expressions.md#r-expr.suffix.fn-shape-param) allows.

```text
use std.num.Num

type Meters(i64)

fn sum[T < Num](items: List[T]) -> T:
    let total = T::zero()
    for item in items:
        total = total + item
    total

fn total(items: List[Meters]) -> Meters:
    sum(items)  # error: unsatisfied-trait-bound
```

> **Why.** Sealing keeps the families to types whose operators the compiler
> knows. `zero` and `one` replace polymorphic literals, the simpler model of
> Rust's `num-traits` rather than Haskell's `fromInteger`.

See also: [Operator Traits](05-expressions.md#operator-traits),
[Supertrait Bindings](#supertrait-bindings).

## Runtime Type Identity

The standard module `std.inspect` lets a program erase a value so that its
concrete type can be recovered later:

```text
trait Inspectable:
    fn runtime_type(self) -> TypeId
    fn downcast[T < AnyRef & Inspectable](self) -> T?
    fn downcast_mut[T < AnyRef & Inspectable](mut self) -> mut T?

impl TypeId:
    pub fn of[T < Inspectable]() -> TypeId

pub fn downcast_val[T < Inspectable](value: Inspectable) -> T?
```

1. r[trait.rtti.module] The standard module `std.inspect` lets a program erase a value so that its concrete type can be recovered later.
2. r[trait.rtti.import] Its names are not prelude names; code imports them, as in `use std.inspect.{Inspectable, TypeId, downcast_val}`.
3. r[trait.rtti.surface] The block above lists the public surface.
4. r[trait.rtti.defaults] `downcast` and `downcast_mut` are default methods whose bodies, like the other bodies, are standard-library code.

### `Inspectable` And `TypeId`

1. r[trait.inspect.sealed] `Inspectable` is a sealed trait.
2. r[trait.inspect.safe] `Inspectable` is dynamically safe: the method-level parameter of `downcast` and `downcast_mut` is bounded by `AnyRef`, which the dynamic-safety rule permits.
3. r[trait.inspect.supplied] The compiler supplies its implementation for every [inspectable type](#inspectable-types).
4. r[trait.inspect.supplied.members] The implementation provides `runtime_type` and keeps the two default methods.
5. r[trait.inspect.runtime-type] `value.runtime_type()` returns the `TypeId` of the value's recorded type.
6. r[trait.inspect.static] For a value whose static type is concrete, the recorded type is its static type.
7. r[trait.inspect.dynamic] For a dynamic value of `Inspectable`, or of a trait that has `Inspectable` as a supertrait, it is the concrete type recorded when the value was erased, never the trait.

See also: [Trait Values And `Any`](04-type-system.md#trait-values-and-any).

#### `TypeId` Values

1. r[trait.typeid.opaque] `TypeId` is an opaque data type. User code cannot construct one or read its fields.
2. r[trait.typeid.traits] `TypeId` implements `Eq`, `Hash`, and `Display`.
3. r[trait.typeid.of] `TypeId` has the associated function `TypeId::of[T]()`, which returns the `TypeId` of `T`.
4. r[trait.typeid.equality] Equality holds exactly when two `TypeId` values denote the same runtime identity, as defined below.
5. r[trait.typeid.no-ops] `TypeId` has no other operations. It exposes no type arguments, fields, or shape.
6. r[trait.typeid.no-trait-query] A `TypeId` cannot answer whether a type implements a trait, and nothing can be constructed or called through it.

#### Runtime Identity

1. r[trait.identity.definition] Two types have the same **runtime identity** when they are the same declaration applied to type arguments that have the same runtime identity.
2. r[trait.identity.normalize] Runtime identity is compared after transparent aliases are expanded and the outer `mut` is removed.
3. r[trait.identity.inner-mut] A `mut` inside a type argument stays, so the type arguments `mut U` and `U` differ.
4. r[trait.identity.declarations] For this rule the primitive types, `string`, `void`, `List`, `Map`, each tuple arity, and `Any` count as declarations.
5. r[trait.identity.trait-value] A trait value type is its trait declaration applied to its arguments.

The rules have these consequences:

| Rule | Types | Runtime identity |
| --- | --- | --- |
| r[trait.identity.alias] Alias | a transparent alias and its target | the same |
| r[trait.identity.newtype] Newtype | a newtype and its base type | different |
| r[trait.identity.arguments] Arguments | `Box[User]` and `Box[Post]` | different |
| r[trait.identity.outer-mut] Outer `mut` | `User` and `mut User` | the same: the outer permission belongs to the view, not the type, and is carried by the signatures of `downcast` and `downcast_mut` |
| r[trait.identity.inner-permission] Inner `mut` | `List[User]` and `List[mut User]`; `(User, i32)` and `(mut User, i32)` | different: an inner permission is part of the type, so an erased `List[User]` never downcasts to `List[mut User]`, whose elements would be mutable |
| r[trait.identity.no-conversion] No conversion | `i32` and `i64`; `User` and `User?`; `List[FsError]` and `List[Error]` | different: no numeric widening, optional injection, or variance applies |
| r[trait.identity.modules] Modules | two declarations named `User` in different modules | different |

See also: [Transparent Aliases And Newtypes](04-type-system.md#transparent-aliases-and-newtypes).

#### Stability And Printing

1. r[trait.typeid.build] A `TypeId` depends only on the program's code identity.
2. r[trait.typeid.stable] Its equality, hash, and printable name are therefore the same in every program instance, process, and run of one build.
3. r[trait.typeid.cross-build] They carry no promise across builds.
4. r[trait.typeid.boundary] A `TypeId` is not boundary-safe.
5. r[trait.typeid.name] The printable name, produced by `Display`, spells the type canonically, as the table below defines.
6. r[trait.typeid.name.no-parse] The name is for people and logs; nothing parses it back into a type.

| Rule | Type | Printable name |
| --- | --- | --- |
| r[trait.typeid.name.prelude] Prelude name | a prelude name | written as it is, as in `i32`, `string`, `List[string]`, or `Map[string, i32]` |
| r[trait.typeid.name.qualified] Other declaration | every other nominal declaration, including the trait of a trait value type | its absolute qualified name, as in `std.error.Error` |
| r[trait.typeid.name.option] Option | `Option[T]` | `T?` |
| r[trait.typeid.name.tuple] Tuple | a tuple | `(A, B)` |
| r[trait.typeid.name.separator] Separator | elements and type arguments | separated by `, ` |
| r[trait.typeid.name.inner-mut] Inner `mut` | a `mut` inside a type argument | written before that argument, as in `List[mut acme.model.User]` |
| r[trait.typeid.name.outer-mut] Outer `mut` | the outer `mut` | never appears |

> **Note.** A tag that must persist is an explicit value with its own
> versioning.

See also: [Runtime Boundary](11-requirements-and-suspension.md#runtime-boundary),
[Wasm Boundary](10-modules.md#wasm-boundary).

### Inspectable Types

The compiler supplies `Inspectable` for exactly the **inspectable types**:

| Rule | Inspectable type |
| --- | --- |
| r[trait.inspectable.primitive] Primitive | the primitive types and `string` |
| r[trait.inspectable.declared] Declared | a data, enum, or newtype declared at module level, public or private, applied to inspectable type arguments |
| r[trait.inspectable.collections] Collection | `List[T]` and `Map[K, V]` with inspectable type arguments, and tuples of inspectable elements |
| r[trait.inspectable.dynamic] Dynamic value | a dynamic value of `Inspectable` or of a trait with `Inspectable` as a supertrait, which satisfies the trait by [Dynamic Trait Values](#dynamic-trait-values) |
| r[trait.inspectable.parameter] Type parameter | a type parameter bounded by `Inspectable` or by a trait that has it as a supertrait |

1. r[trait.inspectable.exact] The compiler supplies `Inspectable` for exactly the types in the table above.
2. r[trait.inspectable.option] The declared case includes `Option` and `Result`, so `T?` is inspectable when `T` is.
3. r[trait.inspectable.fields] What the fields hold does not matter. A data type with a function-typed field, and a newtype over a function type, are inspectable, because identity is the declaration.
4. r[trait.inspectable.argument-only] As a type argument only, `void`, any trait value type, and `Any` also count as inspectable, and they match exactly.
5. r[trait.inspectable.argument-examples] `List[Display]`, `Result[void, FsError]`, and `Map[string, Any]` are inspectable.

#### Types That Are Not Inspectable

These types are not inspectable, as values or as type arguments:

| Rule | Not inspectable |
| --- | --- |
| r[trait.inspectable.not.function] Function | function types, spelled or as the constructors `Fn` and `SuspendFn` from `std.function`, closures, and function values |
| r[trait.inspectable.not.suspend] Suspension | `Suspend[T]` and suspension frames |
| r[trait.inspectable.not.local] Local declaration | data, enum, newtype, and trait declarations local to a block suite |
| r[trait.inspectable.not.never] Never | `never` |
| r[trait.inspectable.not.argument] Argument | a type applied to a non-inspectable argument, such as `Box[fn() -> i32]` |
| r[trait.inspectable.not.parameter] Unbounded parameter | a type parameter without an `Inspectable` bound, whether or not it is `reified` |

1. r[trait.inspectable.dynamic-values] A dynamic trait value whose trait does not have `Inspectable` as a supertrait, and `Any`, are not inspectable as values.
2. r[trait.inspectable.one-way] Erasing to them stays one-way.

### Erasure To `Inspectable`

A value is erased to `Inspectable` by an expected type, like any other
dynamic trait value.

1. r[trait.erase.expected] A value is erased to `Inspectable` by an expected type, like any other dynamic trait value: assignability rule 6 constructs an `Inspectable` value from an inspectable type.
2. r[trait.erase.no-cast] There is no cast operator.
3. r[trait.erase.child-impl] A trait that extends `Inspectable` still needs its own explicit implementation; only its `Inspectable` part is supplied.
4. r[trait.erase.recorded] The recorded type is the static type of the value at the erasure site, without its outer `mut`.
5. r[trait.erase.parameter] For a value of a type parameter `T < Inspectable`, the recorded type is the type `T` is instantiated with, which the bound supplies at run time.
6. r[trait.erase.built] A value built from `T`, such as a `Box[T]`, records `Box` applied to that type. `T` instantiated with `mut User` therefore records `Box[mut User]`.
7. r[trait.erase.weakened] A `mut List[mut User]` weakened to `List[User]` before erasure records `List[User]`.
8. r[trait.erase.bound-required] Erasing a value of a type parameter requires an `Inspectable` bound on it.
9. r[trait.erase.reified] `reified T` alone does not allow the erasure. A value of a type parameter without the bound is not assignable to `Inspectable`, which is an error. Error: `type-mismatch`.
10. r[trait.erase.widen] Widening a dynamic value of a trait that extends `Inspectable` to `Inspectable` is ordinary supertrait widening (assignability rule 7).
11. r[trait.erase.keeps] The widened value keeps its recorded concrete type. Nothing is wrapped twice, including when a type parameter is instantiated with such a trait value type.
12. r[trait.erase.mut] `mut Inspectable` keeps mutable access to an erased composite root.
13. r[trait.erase.mut.source] Erasing to `mut Inspectable` requires mutable access to the source. Erasing a readonly value to `mut Inspectable` is an error. Error: `mutable-upgrade`.

```text
use std.inspect.{Inspectable, TypeId}

data Box[T]:
    value: T

fn erase[T < Inspectable](value: T) -> Inspectable:
    Box { value: value }

fn is_int_box(value: Inspectable) -> bool:
    value.runtime_type() == TypeId::of[Box[i32]]()

fn read_box(value: Inspectable) -> i32:
    match value.downcast[Box[i32]]():
        .Some(found) => found.value
        .None => 0
```

`erase(1)` records `Box[i32]`, so `is_int_box` returns `true` for it and
`read_box` returns `1`. `erase("one")` records `Box[string]`, and both
functions take their other branch.

```text
use std.inspect.Inspectable

fn erase[reified T](value: T) -> Inspectable:
    value  # error: type-mismatch
```

See also: [Assignability And Coercion](04-type-system.md#assignability-and-coercion).

### Recovering A Concrete Type

1. r[trait.downcast.some] `value.downcast[T]()` returns `.Some` of the value exactly when the value's recorded type has the same runtime identity as `T`, and `.None` otherwise.
2. r[trait.downcast.mut] `value.downcast_mut[T]()` does the same through a mutable receiver and returns `mut T?`.
3. r[trait.downcast.val] `downcast_val[T](value)` does the same for any inspectable `T`, including the value types that `AnyRef` excludes, such as scalars, `string`, and tuples.
4. r[trait.downcast.val.readonly] The result of `downcast_val` is readonly.
5. r[trait.downcast.exact] Type arguments must match exactly: an erased `Box[i32]` is not a `Box[i64]`, and an erased `List[FsError]` is not a `List[Error]`.
6. r[trait.downcast.no-conversion] No variance, numeric widening, optional unwrapping, newtype unwrapping, or supertrait search takes place.
7. r[trait.downcast.optional] An erased `User?` therefore downcasts to `User?`, giving a `User??`, and never to `User`.
8. r[trait.downcast.same-reference] A recovered reference value is the same reference that was erased, so `is` holds between them.
9. r[trait.downcast.unboxed] A value without identity is unboxed.

> **Note.** `downcast_val` is bounded by `Inspectable` alone on purpose:
> generic code over a type of either category needs only one downcast.

#### Ordinary Rules For Recovery

1. r[trait.downcast.ordinary] `downcast` and `downcast_mut` are ordinary default methods, inherited by every trait that extends `Inspectable`, such as `std.error.Error`.
2. r[trait.downcast.val-ordinary] `downcast_val` is an ordinary generic function.
3. r[trait.downcast.no-special] No rule is specific to these three; the ordinary rules give the results below.
4. r[trait.downcast.evidence] The `Inspectable` evidence for `T`, passed with each call like the evidence for any bound, carries the runtime identity of `T`. No `reified` marker is needed.
5. r[trait.downcast.pass-on] A generic function passes a target on through its own bound, as in `fn get[T < AnyRef & Inspectable](value: Inspectable) -> T?`.
6. r[trait.downcast.readonly] `downcast` yields a readonly `T`.
7. r[trait.downcast.mut-receiver] `downcast_mut` has a `mut self` receiver, so calling it through a readonly view is an error. Error: `mutable-receiver-required`.
8. r[trait.downcast.target] The target `T` is written, or inferred from the expected type, at the call.
9. r[trait.downcast.visibility] The target must be nameable there under ordinary visibility, so a private type of another module cannot be recovered outside it.
10. r[trait.downcast.bound] A target that fails a bound is an error. Error: `unsatisfied-trait-bound`.
11. r[trait.downcast.bound.examples] `Any`, `Display`, and function types fail `Inspectable` everywhere. `i32`, `string`, and tuples fail `AnyRef`, so they are recovered with `downcast_val`.

> **Note.** A concrete receiver uses its own compiler-supplied
> implementation, so `user.downcast[User]()` with `user: User` is valid and
> always returns `.Some`. A trait value target such as
> `error.downcast[Error]()` satisfies the bounds and always returns `.None`,
> because a recorded type is never a trait value type. Tools may warn about
> both.

### Limits Of Runtime Identity

1. r[trait.limit.no-trait-tests] **No trait tests.** A test compares two runtime identities. Nothing asks whether a value implements a trait, and a dynamic value of one trait is never converted to an unrelated trait.
2. r[trait.limit.no-requirement-keys] **No inspectable requirement keys.** A trait that is `Inspectable` or has it as a supertrait is never a requirement key. A provider view therefore cannot be tested to recover a concrete provider.
3. r[trait.limit.signatures] **Visible in signatures.** Only a value of an inspectable type can be erased to `Inspectable`. A value of an unbounded type parameter, of `Any`, or of a trait value type whose trait does not extend `Inspectable` cannot.
4. r[trait.limit.signatures.branch] A function can therefore branch on a value's type only when a parameter type names `Inspectable`, a trait extending it, or a parameter bounded by one of them.
5. r[trait.limit.deterministic] **Deterministic.** `runtime_type`, the `TypeId` operations, and `downcast` are pure functions of the build and the value, and call no provider.

See also: [Requirement Rows](11-requirements-and-suspension.md#requirement-rows).

## Embedding And Trait Satisfaction

Embedding never grants trait conformance: the outer type must declare its own
implementation.

1. r[trait.embed.no-conformance] Embedding never grants trait conformance.
2. r[trait.embed.explicit] The outer type must declare an explicit `impl`, and that implementation must write every required method that has no default.
3. r[trait.embed.no-fill] A method promoted from an embedded field never fills a trait method, whether required or defaulted, and whatever its receiver.
4. r[trait.embed.forward] An implementation that reuses an embedded type's behavior forwards to it explicitly, for example `fn label(self) -> string: self.Base.label()`.
5. r[trait.embed.delegate] When the embedded type implements the trait, the implementation may instead delegate the whole trait with `impl Trait for C by E`.
6. r[trait.embed.forward-mut] A forwarding `mut self` method may call a `mut self` method of the embedded part, as in `fn reset(mut self) -> void: self.Base.reset()`. This works because `self.Base` has `mut` access through `mut self`.
7. r[trait.embed.bodyless] A bodyless implementation of a trait with a required method is therefore an error even when an embedded type has a matching method. Error: `missing-trait-method`.
8. r[trait.embed.ambiguous] Where the trait is available, a dot call with the forwarded name meets both the implementation's method and the promoted method, and is an error. Error: `ambiguous-method`.
9. r[trait.embed.call-forms] The forwarding implementation is called as `Trait::method(x)`, as in `Describe::describe(service)`. The embedded type's method is called through its path, as in `service.Logger.describe()`.

```text
trait Describe:
    fn describe(self) -> string

data Label:
    text: string

impl Label:
    pub fn describe(self) -> string: self.text

data Page:
    Label

impl Describe for Page  # error: missing-trait-method
```

See also: [Trait Delegation](#trait-delegation),
[Data Embedding](08-data-and-enums.md#data-embedding).

### Trait Methods Of Embedded Types

1. r[trait.embed.no-promotion] An embedded type's trait methods are not promoted either.
2. r[trait.embed.no-promotion.example] If `Label` implements `Display` and `Page` embeds `Label`, `page.to_string()` never calls `Label`'s implementation.
3. r[trait.embed.no-hiding] `Label`'s method does not hide a `to_string` promoted from a type that `Label` embeds.
4. r[trait.embed.unknown] Suppose `Page` has no `to_string` of its own, no promoted one, and no available trait method with that name. The call is then an error whose message should suggest `page.Label.to_string()`. Error: `unknown-method`.
5. r[trait.embed.no-bound] `Page` satisfies no `Display` bound unless `Page` itself implements `Display`.
6. r[trait.embed.not-subtype] Embedding is composition, not subtype inheritance.
7. r[trait.embed.not-assignable] An outer data type is not assignable to an embedded type merely because it promotes that type's methods.

```text
trait Describe:
    fn describe(self) -> string

data Label:
    text: string

impl Describe for Label:
    fn describe(self) -> string:
        self.text

data Page:
    Label

fn invalid(page: Page) -> string:
    page.describe()  # error: unknown-method
```

See also: [Member Resolution](03-names-and-scopes.md#member-resolution).

## Trait Delegation

A trait implementation may delegate the trait to an embedded field of its
target by naming the field after `by`:

```text
trait Describe:
    fn describe(self) -> string
    fn headline(self) -> string:
        "* " + self.describe()

data Logger:
    name: string

impl Describe for Logger:
    fn describe(self) -> string:
        self.name

data Service:
    Logger
    port: i32

impl Describe for Service by Logger

data Worker:
    Logger

impl Describe for Worker by Logger:
    fn headline(self) -> string:
        "worker " + self.Logger.headline()
```

1. r[trait.by.form] A trait implementation may delegate the trait to an embedded field of its target by naming the field after `by`.
2. r[trait.by.valid-part] In `impl Trait for C by E`, where `E` is not `Structure`, `C` must be a data type, and `E` must name one of its embedded fields.
3. r[trait.by.part-impl] The type of that field, with `C`'s type arguments substituted, must implement the same instantiation of `Trait`.
4. r[trait.by.invalid] Otherwise the implementation is an error, reported on the line of `by`. Error: `invalid-delegation`.
5. r[trait.by.direct] `E` names a direct embedded field; a deeper part is reached by delegating to the field that contains it.
6. r[trait.by.structure] `impl Trait for C by Structure` is never a delegation. It declares a derivation template or a derivation block, as [Typed Derivation](14-annotations.md#typed-derivation) defines.
7. r[trait.by.trait-less] A header without a trait never delegates: `impl C by Structure` declares a [trait-less derivation block](14-annotations.md#trait-less-derivation-blocks).
8. r[trait.by.trait-less.error] `impl C by E` without a trait, where `E` is not `Structure`, is an error, reported on the line of `by`. Error: `invalid-delegation`.

```text
trait Describe:
    fn describe(self) -> string

data Logger:
    name: string

impl Describe for Logger:
    fn describe(self) -> string:
        self.name

data Service:
    Logger
    port: i32

impl Describe for Service by port  # error: invalid-delegation
impl Service by Logger  # error: invalid-delegation
```

### Generated Methods

1. r[trait.by.generated] For every method of `Trait` with a receiver, including methods that have a default body, the implementation has a generated method unless its body writes one with that name.
2. r[trait.by.generated.signature] The generated method has the trait method's signature. It calls the part's implementation of the same method with the same arguments, as `Trait::method(self.E, arguments...)` would.
3. r[trait.by.generated.variadic] A variadic parameter is passed on as a spread.
4. r[trait.by.generated.mut] A `mut self` method forwards through `self.E`, which has `mut` access through `mut self`.
5. r[trait.by.written] A method written in the body replaces the generated one and follows the rules of [Implementation Declarations](#implementation-declarations), as does any other member of the body.
6. r[trait.by.part-receiver] The part's implementation runs with the part as its receiver. A default body there calls the part's methods, never the delegating implementation's; there is no overriding.

See also: [Data Embedding](08-data-and-enums.md#data-embedding),
[Member Resolution](03-names-and-scopes.md#member-resolution).

### Delegated Associated Items

1. r[trait.by.assoc-types] Associated types take the part's bindings: each associated type of the delegating implementation is bound to the type that the part's implementation binds.
2. r[trait.by.assoc-binding] An associated type binding in the body is an error. Error: `invalid-delegation`.
3. r[trait.by.assoc-fn] Associated functions have no receiver to forward through and are never generated.
4. r[trait.by.assoc-fn.written] The body writes each associated function unless the trait gives it a default. A missing one is an error. Error: `missing-trait-method`.

```text
trait Named:
    fn name(self) -> string
    fn kind() -> string

data Logger:
    label: string

impl Named for Logger:
    fn name(self) -> string:
        self.label

    fn kind() -> string:
        "logger"

data Service:
    Logger

impl Named for Service by Logger  # error: missing-trait-method
```

### Delegation Is An Ordinary Implementation

1. r[trait.by.ordinary] Otherwise a delegating implementation is an ordinary trait implementation.
2. r[trait.by.ordinary.rules] It obeys [Implementation Ownership](#implementation-ownership), [Overlap](#overlap), and trait visibility, satisfies bounds, and supports dynamic trait values.
3. r[trait.by.ordinary.candidates] Its methods are candidates in [Method Resolution](#method-resolution) where the trait is available.
4. r[trait.by.dot-call] A dot call such as `service.describe()` therefore has one candidate unless an embedded type also has an inherent method of that name. In that case the call is an error, as for any implementation. Error: `ambiguous-method`.

## Default-Method Conflicts

A type that implements two traits with same-named default methods may make a
dot call ambiguous.

1. r[trait.conflict.ambiguous] If a type implements two traits that provide default methods with the same name, ordinary method-call resolution may be ambiguous even though both trait implementations are individually valid.
2. r[trait.conflict.inherent] The type may define an inherent method to provide its ordinary dot-call behavior.
3. r[trait.conflict.qualified] The caller may instead use `Trait::method(value, ...)` to select one implementation.

## Unsupported Trait Extensions

1. r[trait.unsupported.list] The language has no specialization, negative implementations, implicit structural conformance, trait-to-trait assertions, or type tests other than exact-type recovery from `Inspectable` values.
2. r[trait.unsupported.abi] Dynamic trait-value representation is an ABI detail and must preserve the dispatch semantics in this chapter.

See also: [Runtime Type Identity](#runtime-type-identity).
