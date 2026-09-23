# Type System

Status: language specification draft.

hd-lang is statically typed. Every expression has a compile-time type, and a
program with a type error must not execute. Local inference removes redundant
annotations; public and aggregate boundaries remain explicit.

## Type Forms

The type forms are:

- primitive types;
- nominal structs and enums;
- tuples;
- `list[T]` and `map[K, V]`;
- optional types `T?`;
- function types `fn(...) -> T` and mutable function types
  `mut fn(...) -> T`;
- suspending function types `fn!(...) -> T`;
- requirement-bearing function types ending in `$ Row`;
- generic instantiations;
- associated type projections such as `T::Item`;
- variadic type packs and pack expansions;
- trait value types;
- transparent aliases and nominal newtypes;
- mutable-access types `mut T`.

`Suspend[T]` is the dynamic one-shot computation protocol. GADT refinements are
arm-local type equalities rather than additional runtime type forms.

## Primitive Types

The primitive types are:

| Category | Types |
| --- | --- |
| Boolean | `bool` |
| Signed integers | `i8`, `i16`, `i32`, `i64` |
| Unsigned integers | `u8`, `u16`, `u32`, `u64` |
| Floating point | `f32`, `f64` |
| Text | `char`, `string` |

There are no core aliases named `int`, `uint`, or `float`. `decimal` may be a
standard-library type but is not primitive. hd-lang has no primitive `byte` or
`bytes` type and no byte literal syntax.

Primitive values are passed and returned by value. Primitive types do not
accept the `mut` modifier because they do not expose mutable reference state.

`void` is the return type of a function that produces no useful value. The
empty tuple value is `()`. Both carry no information, but they serve different
source-level roles: `void` describes the absence of a useful function or
statement result, while `()` is a tuple value and tuple type. They are distinct
types and there is no implicit conversion between them.

## Literal Types

An integer literal in any supported radix with no expected type has type `i32`
in every value range. It does not automatically choose a wider type:

```text
x := 1  # i32
```

When an integer literal has an expected integer type, the compiler checks the
literal against that type's range:

```text
let small: i8 = 1    # valid
let bad: u8 = 300    # error
```

The diagnostic must identify the literal, the target range, and an appropriate
wider type when one exists.

An expected numeric type passes through unary `+` to a numeric literal, so
`let positive: u8 = +1` checks the literal against the `u8` range.

When unary `-` is applied directly to an integer literal under an expected
signed integer type, range checking considers the negated mathematical value as
a unit. This makes `let minimum: i8 = -128` valid even though positive `128`
does not fit in `i8`. A negated literal is invalid for an unsigned expected
type, and values below the signed minimum remain errors.

A floating-point literal with no expected type has type `f64`. When an expected
`f32` or `f64` type is available, the literal is converted directly to that
type and must be representable under its IEEE 754 rounding rules.

`true` and `false` have type `bool`. A string literal has type `string`, and a
character literal has type `char`.

`nil` has no standalone concrete type. It is valid only where an optional type
`T?` is expected.

## Nominal And Structural Types

Each `struct` and `enum` declaration introduces a distinct nominal type.
Matching fields or variants do not make two nominal types interchangeable:

```text
struct UserId:
    value: string

struct PostId:
    value: string
```

`UserId` and `PostId` are distinct even though their field shapes match.

Tuple types are structural. Two tuples have the same type when they have the
same arity and pairwise-equal element types. One-element tuples require a
trailing comma; `()` is the empty tuple.

Function types are structural when their parameter types, result type,
mutability, suspension marker, and normalized requirement row match. Function types
are invariant in every parameter and the result; there are no implicit
function-type variance conversions.

## Transparent Aliases And Newtypes

A transparent alias introduces another name for the same type:

```text
type UserName = string
```

`UserName` and `string` are identical for assignability, method lookup, trait
conformance, and runtime representation.

A parenthesized type declaration creates a nominal single-field newtype:

```text
type Mile(i32)
```

`Mile` is distinct from `i32`. Constructor syntax creates it, and the base type
constructor unwraps it:

```text
m := Mile(10)
n := i32(m)
```

No implicit conversion exists in either direction.

## Optional Types

`T?` is an optional type containing either a `T` or `nil`. `T` and `T?` are
different types; a non-optional type never contains `nil`.

Optionality may nest: `T??` is `(T?)?`, preserving the distinction between an
absent outer value and a present outer value containing an absent inner value.
`nil` with an expected nested optional type constructs absence at the outermost
level. Postfix `?` removes and propagates one optional layer at a time.

Optional syntax is equivalent in role to `Option[T]`, but its representation is
an implementation detail. Postfix `?` on an optional expression either
produces its contained value or returns `nil` from the nearest function. That
function must itself return a compatible optional type.

`Any` does not include `nil`. An optional value may be erased to `Any?`, not to
`Any`.

Optional is covariant in its contained type for a read-only outer value.
`Result[T, E]` is likewise covariant in both `T` and `E` for a read-only outer
value. As with every generic composite, a mutable outer view is invariant.

## Result Types

Recoverable errors use the ordinary generic type `Result[T, E]`. Values are
constructed with `Ok(value)` and `Err(error)`.

Postfix `?` on `Result[T, E]` either produces the success value or immediately
returns the error from the nearest function. The enclosing function must return
a `Result[U, F]` whose error type accepts the propagated error. Returning an
`Err` value without `?` does not itself alter control flow.

The core language does not use effect syntax for recoverable errors.

## Numeric Conversions

Implicit integer conversion is limited to widening conversions that preserve
every value of the source type. The signed and unsigned widening chains are:

```text
i8 -> i16 -> i32 -> i64
u8 -> u16 -> u32 -> u64
```

There is no implicit conversion between signed and unsigned integers, and no
implicit integer-to-floating or floating-to-integer conversion in the current
core. `f32` widens implicitly to `f64`; `f64` to `f32` requires an explicit
cast.

For a binary numeric operator, an untyped literal first adopts the compatible
type expected from the other operand. Otherwise, operands within one integer
signedness family widen to the wider operand type, and the result has that
type. `f32` and `f64` operands widen to `f64`. Signed and unsigned integers do
not mix implicitly, and integers do not mix implicitly with floating-point
values. The user must cast one operand explicitly in those cases.

Explicit numeric conversion uses constructor-style casts:

```text
let wide: i64 = 9000
narrow := i16(wide)
```

A narrowing integer cast must range-check at runtime when the compiler cannot
prove it safe. An out-of-range cast causes a checked runtime failure; the exact
panic reporting ABI belongs to the runtime specification. Libraries may
provide separate fallible conversion functions returning `Result`.

The core numeric cast rules are:

- integer to integer checks the target integer range;
- integer to floating point rounds to the nearest representable IEEE 754 value
  using ties-to-even;
- `f32` to `f64` is exact, while `f64` to `f32` uses IEEE 754 ties-to-even
  rounding;
- floating point to integer first truncates toward zero, then checks that the
  original value was finite and the truncated value is in the target range.

A failed integer range check or a non-finite floating-to-integer conversion
panics. An explicit numeric cast may lose precision according to these rules;
libraries may expose exact or fallible conversions when loss must be rejected.
Constructor-style calls involving nonnumeric types are not numeric casts: they
must resolve to a nominal newtype constructor, enum constructor, or ordinary
function.

Integer arithmetic is checked. Overflow, invalid shifts, invalid integer
exponents, and division errors cause checked runtime failure unless an explicit
wrapping or fallible library operation is used. Integer division truncates
toward zero, and integer remainder has the sign of the dividend. A shift count
must be non-negative and smaller than the bit width of the shifted value.

## Assignability And Coercion

An expression of type `S` is assignable to a location of type `T` when at least
one of these rules applies:

1. `S` and `T` are identical after expanding transparent aliases.
2. `S` is an integer type with a value-preserving widening conversion to `T`.
3. `S` is `f32` and `T` is `f64`.
4. `S` is `mut T` and the target requests the const view `T`.
5. A declared generic variance conversion permits the read-only outer type to
   change its type arguments.
6. `S` explicitly implements trait `T`, allowing construction of a dynamic
   trait value.
7. `S` is a dynamic child-trait value whose trait has `T` as a direct or
   transitive supertrait.
8. `nil` is used with an expected optional type `T?`.
9. A value of `T` is injected into `T?`.

No inheritance or structural record subtyping exists. Assignment never changes
the declared or inferred type of a binding. In particular, later assignment to
a `let` binding must remain assignable to the type established at its
declaration.

## Composite Values And Access Permission

Structs, enums with storage, tuples, lists, maps, trait values, and closures are
composite values. Composite parameters and results use shared references at the
language level. hd-lang does not require exclusive ownership and may have
multiple aliases to one composite value.

For a composite type `T`:

- `T` is a const reference view. It permits observation but not mutation
  through that reference.
- `mut T` is a mutable reference view. It permits operations that mutate the
  referenced value.

`mut` expresses access permission, not ownership, uniqueness, or deep
immutability. A `mut T` may be viewed as `T`; a `T` must never be upgraded to
`mut T`.

```text
let user: mut User = User { name: "Ada" }
readonly := user
user.name = "Grace"
println(readonly.name)  # observes "Grace"
```

The const alias prevents mutation through `readonly`; it does not freeze the
underlying object against other mutable aliases.

### Bindings And Fresh Values

A fresh struct or copy-update expression, stored enum construction, tuple
expression, list expression, or map expression produces mutable access to its
new outer object. This permission may be weakened immediately by an expected
const type. Freshness does not recursively upgrade composite values stored in
the new object; each field or element keeps the permission of the supplied
expression and declared edge.

`:=` always exposes a const composite view, even when its initializer creates a
fresh value:

```text
user := User { name: "Ada" }  # User
```

`let` may state mutable access explicitly:

```text
let user: mut User = User { name: "Ada" }
```

An unannotated `let` infers the initializer's access type. A fresh composite
construction may infer `mut T`; an existing `T` remains `T`, and inference
never upgrades const access. Passing through a function also follows the
declared result type rather than recovering freshness. Consequently, a fresh
literal may be passed directly to a `mut T` parameter, but a call declared to
return `T` cannot, even when its implementation constructs a fresh value.

### Mutable Paths

Mutation through a composite access path requires:

1. a mutable root expression; and
2. mutable permission on every stored composite edge traversed before the
   location being mutated.

```text
struct Account:
    profile: mut Profile

let account: mut Account = Account { profile: profile }
account.profile.display_name = "Ada"
```

An ordinary field `field: T` is a const edge. A mutable owner may replace that
field, but reading through it yields only `T`, so the referenced child cannot be
mutated through that path. A field `field: mut T` preserves mutable access when
read through a mutable root.

Reading any field or element through a const composite root applies viewpoint
weakening: mutable permission stored beyond that root is observed as const. For
example, indexing `list[mut User]` yields `User`, while indexing
`mut list[mut User]` yields `mut User`.

Container mutation and element mutation are independent:

| Type | Replace elements | Mutate referenced elements |
| --- | --- | --- |
| `list[User]` | no | no |
| `list[mut User]` | no | no, due to const root |
| `mut list[User]` | yes | no |
| `mut list[mut User]` | yes | yes |

The same rules apply recursively to maps, tuples, and user-defined structs.

### Parameters And Results

`value: T` accepts const composite access. `value: mut T` requires mutable
access. `mut self` is shorthand for `self: mut Self`.

A function that returns mutable access must declare `-> mut T`. A declared
result of `T` exposes only const access, even when the function creates a fresh
object internally:

```text
fn new_user() -> User: User { name: "Ada" }
fn new_mutable_user() -> mut User: User { name: "Ada" }
```

## Generics

Generic type and function parameters denote complete types. A type parameter
`T` may therefore be instantiated with either `User` or `mut User`.

An unconstrained generic declaration stores or passes `T` directly. It must not
write `mut T`, because substitution with `T = mut User` would create the
meaningless form `mut mut User`. Mutable generic requirements use a bound:

```text
fn clear_value[T: mut Clear](value: T) -> void:
    value.clear()
```

`T: mut Any` accepts any mutable-root type. `T: mut Trait` additionally
requires the underlying type to implement `Trait`. A plain `T: Trait` requires
trait conformance without mutable-root authority.

Generic arguments are inferred at call sites when unambiguous. Callers may
supply the complete generic argument list explicitly. Partial explicit generic
arguments and placeholder generic arguments are not language constructs.

Function generic parameters are erased at runtime by default. A parameter
marked `reified` carries runtime type metadata and may be used by operations
such as `shape(T)` or passed to another reified operation. An erased parameter
must not be used where runtime type identity is required.

Reification is part of the function's public type and ABI, but its descriptor
is not a source-level value argument. A backend may specialize a reified call
only when doing so preserves observable reflection behavior.

`shape(Target)` consumes this descriptor. For a concrete struct, enum,
function, field, variant, or parameter declaration it returns the corresponding
specialized shape type; for an otherwise generic concrete type it returns
`TypeShape`. An erased generic parameter cannot be used as a shape target.
Annotation lookup for a generic target has the same reification requirement.

An identifier followed by `...` in a generic parameter list declares a type
pack. Packs have a compile-time length and ordered element types; they are not
runtime collection values. Expansion and inference are specified in
[Variadic Generics](12-variadic-generics.md).

## Variance

Generic type declarations mark covariance with `+T`, contravariance with `-T`,
and invariance by leaving `T` unmarked:

```text
struct Producer[+T]:
    produce: fn() -> T

struct Consumer[-T]:
    consume: fn(T) -> void

struct Cell[T]:
    value: T
```

The compiler verifies each declared parameter against its use on the type's
read-only public surface. That surface includes struct fields, enum shared data
and variant payloads, trait method signatures, and every inherent method
available with the nominal type. A separate trait implementation does not alter
the nominal type declaration's variance; its own instantiated signatures must
still type-check.

Polarity is computed as follows:

- a returned value and an ordinary read-only field are positive positions;
- a function or method parameter is a negative position;
- entering a function parameter reverses polarity, while entering a function
  result preserves it;
- applying a covariant generic argument preserves polarity, a contravariant
  argument reverses it, and an invariant argument makes the occurrence
  invariant;
- occurrence beneath `mut` is invariant because the referenced storage can be
  both read and written;
- a parameter used in both positive and negative positions must be invariant.

A declared `+T` is rejected if any occurrence is negative or invariant. A
declared `-T` is rejected if any occurrence is positive or invariant. An
unmarked invariant parameter may occur in any position.

Variance conversion applies only to a read-only outer view. Every `mut G[T]`
view is invariant in all generic arguments because the mutable view may replace
stored values. The built-in `list` declares a covariant element parameter for
its read-only view. Read-only `map[K, V]` is invariant in `K`, because keys are
both accepted for lookup and exposed during traversal, and covariant in `V`.

Direct composition of permission weakening and variance, such as converting
`mut Cell[Cat]` directly to `Cell[Animal]`, remains in the design backlog and is
not a core conversion.

## Trait Values And `Any`

Trait conformance is explicit. Matching method shape alone does not make a type
implement a trait. A generic bound such as `T: Display` uses static dispatch and
preserves the concrete type.

Using a trait name as a value type creates a Go-style dynamic trait value. It
contains a concrete value and dispatch metadata for that trait. Source syntax
does not use a `dyn` marker.

Only a dynamically safe trait may be used as a value type. A dynamically safe
trait and every supertrait must have no associated types, associated functions,
or method-level generic parameters, and `Self` may appear only as the receiver type. Trait declaration generic
parameters are permitted because one concrete trait instantiation, such as
`Repository[User]`, fixes them before erasure. Traits that fail these rules
remain valid for static generic bounds and explicit implementations.

A dynamic child-trait value exposes methods declared by the child and all of
its transitive supertraits. It may be widened implicitly to a dynamic
supertrait value; that conversion discards access to child-only methods and
cannot be reversed without an unsupported downcast.

`Any` is the built-in universal empty trait. Every non-optional value type
satisfies it automatically. As a value type, `Any` erases the concrete type.
`mut Trait` and `mut Any` preserve mutable access to an erased composite root.

`Any` is not a top type containing `nil`; use `Any?` when absence is permitted.

## Map Key Types

`map[K, V]` requires `K` to satisfy the compiler-defined `MapKey` contract.
The following types satisfy it:

- `bool`, `char`, `string`, and every signed or unsigned integer type;
- transparent aliases of a `MapKey` type;
- nominal single-field newtypes whose underlying type is `MapKey`;
- tuples whose elements are all `MapKey`;
- optional and enum types whose contained payload types are all `MapKey`.

Floating-point types, structs, lists, maps, functions, dynamic trait values, and
every `mut T` type do not satisfy `MapKey`. This conservative closed set ensures
that a key's equality and hash cannot change through another mutable alias while
the key is stored. A future extension may expose an explicit stable equality/hash
trait after its aliasing contract is designed.

`MapKey` equality is value equality: tuples compare element by element; enums
compare variant then payload; newtypes compare their underlying value; and
optional values compare absence or their contained key. Implementations may
choose any hash algorithm that agrees with this equality and must not expose
the hash or iteration order as value semantics.

## Type Inference Boundaries

The compiler infers local binding types, closure parameter or result types when
an expected function type supplies them, and generic call arguments when the
solution is unambiguous.

The following declarations require explicit types:

- named function parameters and results;
- public and private struct fields;
- enum payload fields and constructor data;
- trait method parameters and results;
- named function type parameters and bounds where applicable.

Inference must not select among overloaded functions because hd-lang has no
function overloading. If inference has multiple valid solutions, compilation
fails and the diagnostic must identify an annotation site that disambiguates
the program.

## Unsupported Type-System Extensions

The language has no runtime type tests or downcasts involving trait values. The exact
host representation of a checked runtime panic is an ABI concern; its
language-level control-flow semantics are defined in
[Control Flow](06-control-flow.md#runtime-panics).
