# Type System

Status: language specification draft.

hd-lang is statically typed. Every expression has a compile-time type, and a
program with a type error must not execute. Local inference removes redundant
annotations; public and aggregate boundaries remain explicit.

## Type Forms

The type forms are:

- primitive types;
- nominal data types and enums;
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

`never` is the uninhabited bottom type. It is assignable to every type and no
ordinary value is assignable to it. Expressions that complete abruptly—an
unconditional `return`, `break`, `continue`, propagation that exits the current
body, and a call to `panic`—have type `never` on that control-flow path.

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

### Strings

A `string` is a sequence of Unicode scalar values whose contents cannot be
mutated. Source text and
runtime operations do not perform Unicode normalization. Equality and ordering
compare scalar values in sequence; canonically equivalent but differently
encoded scalar sequences are distinct. `string.len()` counts scalar values.
The core type has no integer-indexing or slicing operation with an O(1)
guarantee; libraries may provide explicit scalar, byte, or grapheme traversal.
At every Wasm host boundary, strings are encoded as UTF-8, including embedded
U+0000 scalar values.

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

A floating-point literal with no expected type has type `f64` and must be
representable as a finite `f64` value. When an expected `f32` or `f64` type is
available, the literal is converted directly to that type and must be
representable as a finite value under its IEEE 754 rounding rules.

`true` and `false` have type `bool`. A string literal has type `string`, and a
character literal has type `char`.

`nil` has no standalone concrete type. It is valid only where an optional type
`T?` is expected.

## Nominal And Structural Types

Each `data` and `enum` declaration introduces a distinct nominal type.
Matching fields or variants do not make two nominal types interchangeable:

```text
data UserId:
    value: string

data PostId:
    value: string
```

`UserId` and `PostId` are distinct even though their field shapes match.

Tuple types are structural. Two tuples have the same type when they have the
same arity and pairwise-equal element types. One-element tuples require a
trailing comma; `()` is the empty tuple.

Function types are structural when their parameter types, result type,
mutability, suspension marker, and normalized requirement row match. Function
types are invariant in every parameter and the result; there are no standalone
implicit function-type variance conversions. A function value read through a
representation-preserving variance conversion of its containing value is
viewed at the converted field type. The only component change this can
introduce is `mut U -> U` in a positive position or `U -> mut U` in a negative
position. This is field access through the converted container, not a general
conversion between function values.

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

A newtype does not inherit trait implementations from its underlying type. In
particular, it is not a valid map key until it explicitly implements or derives
both `Eq` and `Hash`.

## Optional Types

`T?` is an optional type containing either a `T` or `nil`. `T` and `T?` are
different types; a non-optional type never contains `nil`.
A value of `T` is implicitly accepted where `T?` is expected, constructing a
present optional. This applies to assignments, arguments, and return values;
the source expression is evaluated once. `nil` constructs the absent case.
There is no built-in `Some(value)` constructor for `T?`.

Optionality may nest: `T??` is `(T?)?`, preserving the distinction between an
absent outer value and a present outer value containing an absent inner value.
`nil` with an expected nested optional type constructs absence at the outermost
level. Postfix `?` removes and propagates one optional layer at a time.

Optional syntax is equivalent in role to `Option[T]`, but its representation is
an implementation detail. Postfix `?` on an optional expression either
produces its contained value or returns `nil` from the nearest function. That
function must itself return a compatible optional type. A present value keeps
the optional's declared contained type `T`, including `mut U` when
`T = mut U`; unwrapping does not weaken that generic argument.

`Any` does not include `nil`. An optional value may be erased to `Any?`, not to
`Any`.

Optional is covariant in its contained type for a readonly outer value.
`Result[T, E]` is likewise covariant in both `T` and `E` for a readonly outer
value. As with every generic composite, a mutable outer view is invariant.

## Result Types

Recoverable errors use the ordinary generic type `Result[T, E]`. Values are
constructed with `Ok(value)` and `Err(error)`.

When `T` is `void`, the success constructor is written `Ok()` and has type
`Result[void, E]` under an expected result type. `Ok(pass)` is not the source
spelling for this case.

Postfix `?` on `Result[T, E]` either produces the success value or immediately
returns the error from the nearest function. The enclosing function must return
a `Result[U, F]` whose error type accepts the propagated error. Returning an
`Err` value without `?` does not itself alter control flow. The success value
retains its declared generic type `T`, including `mut U` when `T = mut U`,
regardless of whether the `Result` value itself is readonly.

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
type. `f32 op f32` produces `f32`; when one operand is `f64`, an `f32` operand
widens and the result is `f64`. Signed and unsigned integers do
not mix implicitly, and integers do not mix implicitly with floating-point
values. The user must cast one operand explicitly in those cases.

Explicit numeric conversion uses constructor-style casts:

```text
let wide: i64 = 9000
narrow := i16(wide)
```

A narrowing integer cast must range-check at runtime when the compiler cannot
prove it safe. An out-of-range cast causes a checked runtime failure; the exact
panic reporting ABI is deferred to runtime design. Libraries may
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
Right shift of a signed integer is arithmetic and sign-extending. For every
signed width, `MIN / -1` panics with `integer-overflow`, while `MIN % -1`
produces zero.

`Display` formats integers in base ten and floating values with the shortest
round-trip decimal digits. Finite floats use fixed notation when the normalized
decimal exponent is in `[-6, 21)` and lowercase scientific notation otherwise;
scientific exponents always include `+` or `-` and no leading zeroes. Fixed
notation always contains a decimal point and at least one fractional digit, so
`1.0` remains visibly floating. Negative zero is `-0.0`; infinities are `inf`
and `-inf`; every NaN is `NaN`. Before hashing, boundary serialization, or
`Display`, every NaN is replaced with the one canonical quiet-NaN value for its
width; NaN comparison continues to follow IEEE 754.

## Assignability And Coercion

An expression of type `S` is assignable to a location of type `T` when at least
one of these rules applies:

1. `S` and `T` are identical after expanding transparent aliases.
2. `S` is an integer type with a value-preserving widening conversion to `T`.
3. `S` is `f32` and `T` is `f64`.
4. `S` is `mut T` and the target requests the readonly view `T`.
5. A declared generic variance conversion permits the readonly outer type to
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

Data types, enums with storage, tuples, lists, maps, trait values, and closures are
composite values. Composite parameters and results use shared references at the
language level. hd-lang does not require exclusive ownership and may have
multiple aliases to one composite value.

For a composite type `T`:

- `T` is a readonly reference view. It permits observation but not mutation
  through that reference.
- `mut T` is a mutable reference view. It permits operations that mutate the
  referenced value.

Throughout the specification, **readonly view** is the single term for `T`
access to a composite value; it does not imply deep immutability. A binding is
described separately as **non-reassignable** when its name cannot be rebound.

`mut` expresses access permission, not ownership, uniqueness, or a deep freeze
of the object. A readonly `T` reference cannot reassign its fields. A direct
data field declared `field: mut U` is read as `U` through readonly `T`, while a
generic data field declared `field: P` retains its substituted type even when
`P` is instantiated as `mut U`. Other extraction forms state their own
permission rules below. A `mut T` may be viewed as `T`; a `T` must never be
upgraded to `mut T`.

A readonly root blocks reassignment of its fields and mutation through a direct
`mut U` field. It is not a deep authority boundary: a `mut U` nested inside an
optional, tuple, collection, or other generic argument keeps its permission when
that nested value is extracted. APIs that require a deep no-mutation guarantee
must not expose mutable references through such nested field types.

```text
let user: mut User = User { name: "Ada" }
readonly := user
user.name = "Grace"
println(readonly.name)  # observes "Grace"
```

The readonly alias prevents mutation through `readonly`; it does not freeze the
underlying object against other mutable aliases. Readonly access does not
guarantee that repeated reads return the same values, that the object is a
stable cache input, or that independently held mutable aliases cannot change
its reachable state. A readonly view restricts structural access through that
view; it does not prohibit a called function from returning a separately held
mutable reference according to its declared result type.

### Bindings And Fresh Values

A fresh data or copy-update expression, stored enum construction, tuple
expression, list expression, or map expression produces mutable access to its
new outer object. This permission may be weakened immediately by an expected
readonly type. Freshness does not recursively upgrade composite values stored in
the new object; each field or element keeps the permission of the supplied
expression and declared edge.

A data literal with a direct `field: mut U` may produce readonly `T` when that
field is supplied only `U`. It produces `mut T` only when every direct mutable
field is supplied mutable access. An expected readonly `T`, including a `:=`
binding, permits the weaker field value; an expected `mut T` rejects it. An
unannotated `let` infers readonly `T` when any such direct field is supplied
only `U`. A generic field declared `field: P` still requires its substituted
type, including `mut U` when `P = mut U`.

`:=` always exposes a readonly composite view, even when its initializer creates a
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
never upgrades readonly access. Passing through a function also follows the
declared result type rather than recovering freshness. Consequently, a fresh
literal may be passed directly to a `mut T` parameter, but a call declared to
return `T` cannot, even when its implementation constructs a fresh value.

### Mutable Paths

Mutation through a composite access path requires:

1. a mutable root expression; and
2. mutable permission on every stored composite edge traversed before the
   location being mutated.

```text
data Account:
    profile: mut Profile

let account: mut Account = Account { profile: profile }
account.profile.display_name = "Ada"
```

Given a `mut T` root, every field may be reassigned with a value assignable to
its declared type, whether the field is `field: U` or `field: mut U`. Replacing
a field does not mutate the old referenced value. An ordinary field `field: U`
is a readonly edge: reading it yields only `U`, so its child cannot be mutated
through that path. A direct field `field: mut U` preserves mutable access when
read through a mutable root. That access permits assigning the child's fields
and calling its `mut self` methods. A mutable root must store `mut U` in that
field. A readonly root may store `U`; it cannot later be upgraded to `mut T`.

Given a readonly `T` root, no field may be reassigned. A direct data field
`field: mut U` read through it yields only `U`; neither the child's fields nor
its `mut self` methods are available through that path. A field declared with
a generic parameter instead yields that parameter's substituted type: reading
`value: P` from readonly `Box[mut User]` yields `mut User`. A list's element
type likewise remains its generic argument: indexing `list[mut User]` yields
`mut User`, even when the list itself is readonly. Mutating that user does not
replace the list slot. A map lookup likewise returns `V?` for the declared
`map[K, V]`, so a successful lookup from `map[K, mut User]` yields `mut User`
after unwrapping; a readonly map root still cannot replace entries.

Container mutation and element mutation are independent:

| Type | Replace elements | Mutate referenced elements |
| --- | --- | --- |
| `list[User]` | no | no |
| `list[mut User]` | no | yes |
| `mut list[User]` | yes | no |
| `mut list[mut User]` | yes | yes |

Generic type arguments are not weakened solely because their enclosing value
is readonly. Data patterns and copy-update use the effective field types of
the subject's view; built-in collection indexing and iteration preserve their
declared generic element or value types; optional and `Result` unwrapping
preserve their declared generic contents. Tuple elements likewise retain their
declared element types on extraction. Enum payload extraction preserves a
generic parameter's substituted type while direct non-generic mutable payloads
follow the subject's viewpoint. A call result is checked against
the callable's declared return type, not automatically weakened merely
because the callable was reached through a readonly value.

### Parameters And Results

`value: T` accepts readonly composite access. `value: mut T` requires mutable
access. `mut self` is shorthand for `self: mut Self`.

A function that returns mutable access must declare `-> mut T`. A declared
result of `T` exposes only readonly access, even when the function creates a fresh
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
supply the complete generic argument list explicitly. Partial explicit lists
are not permitted. In a named generic-function reference, `_` may occupy a
slot in the complete list and requests inference for that argument; it is not
itself a type and is invalid in ordinary type applications.

Function generic parameters are erased with dictionary passing and a uniform
`anyref` runtime representation. Primitive values are boxed in generic
positions. Trait bounds pass dictionaries containing the selected operations;
associated types are represented through those dictionaries. Pack functions
and calls with `reified` parameters are specialized, while ordinary erased
calls are not required to be specialized. Package interfaces therefore carry
the bodies of generic and pack functions needed by downstream compilation.

A parameter marked `reified` carries runtime type metadata and may be used by operations
such as `shape(T)` or passed to another reified operation. An erased parameter
must not be used where runtime type identity is required.

Identity comparison `is` on a type parameter is permitted only with the sealed
`T: Reference` bound. An unconstrained type parameter may be primitive after
substitution and therefore cannot be used with `is`.

Reification is part of the function's public type and ABI, but its descriptor
is not a source-level value argument. A backend may specialize a reified call
only when doing so preserves observable reflection behavior.

`shape(Target)` consumes this descriptor. For a concrete data type, enum,
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
data Producer[+T]:
    produce: fn() -> T

data Consumer[-T]:
    consume: fn(T) -> void

data Cell[T]:
    value: T
```

The compiler verifies each declared parameter against its use on the type's
readonly public surface. That surface includes data fields, enum shared data
and variant payloads, trait method signatures, and every inherent method
available with the nominal type. A separate trait implementation does not alter
the nominal type declaration's variance; its own instantiated signatures must
still type-check.

Each declaration parameter whose argument position in an explicit GADT variant
result is not exactly that parameter is invariant. For example,
`IsMutUser -> Witness[mut User]` makes `T` invariant in `Witness[T]`; declaring
that `Witness[+T]` is rejected. This prevents a variance conversion from making
an arm-local GADT equality upgrade a readonly value or reinterpret a value's
runtime representation.

Polarity is computed as follows:

- a returned value and an ordinary readonly field are positive positions;
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

Variance conversion applies only to a readonly outer view. Every `mut G[T]`
view is invariant in all generic arguments because the mutable view may replace
stored values. The built-in `list` declares a covariant element parameter for
its readonly view. Readonly `map[K, V]` is invariant in `K`, because keys are
both accepted for lookup and exposed during traversal, and covariant in `V`.

A variance conversion `G[S] -> G[T]` requires a representation-preserving
`S -> T` conversion for each covariant argument and a
representation-preserving `T -> S` conversion for each contravariant argument.
Permission weakening `mut U -> U` is representation-preserving. Numeric
widening such as `i8 -> i64`, construction of a trait value such as
`i32 -> Display` or `User -> Display`, child-dynamic-trait to supertrait
widening, and optional injection `U -> U?` are not. Variance never inserts
element wrappers, metadata rewrapping, boxing, copies, or per-access
conversions.

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
cannot be reversed without an unsupported downcast. This direct widening may
rewrap dispatch metadata and is therefore not representation-preserving for a
variance conversion.

`Any` is the built-in universal empty trait. Every non-optional value type
satisfies it automatically. As a value type, `Any` erases the concrete type.
`mut Trait` and `mut Any` preserve mutable access to an erased composite root.

`Any` is not a top type containing `nil`; use `Any?` when absence is permitted.

## Map Key Types

`map[K, V]` requires `K: Eq + Hash` and rejects a `mut T` key type.
`Hash` is a standard-library trait in `std.hash`; user-defined data and enum
types can become keys by explicitly implementing or deriving both traits. Standard-library
implementations cover eligible built-in scalar types and their supported
compositions: `bool`, integers, `char`, `string`, payload-free enums, tuples of
hashable elements, and optionals of hashable elements. Lists, maps,
floating-point values, functions, suspensions, dynamic trait values, and `Any`
do not have built-in `Hash`; user data and stored enums require an explicit or
derived implementation. Floating-point types do not implement `Eq` because of
NaN. Consequently maps have no built-in hash and impose no order-independent
map-hash obligation.

Map lookup and duplicate-key replacement use `Eq` for key comparison and
`Hash` for indexing. The language does not check or impose a law connecting
these two implementations. If an implementation hashes values differently
that `Eq` considers equal, lookup and duplicate-key behavior are not
guaranteed. Map iteration follows insertion order. Replacing the value for an
existing key does not move that entry; removing and later reinserting a key
places it at the end. Hash values remain outside map value semantics, and map
equality remains independent of insertion order.

A readonly key view does not freeze the object. If another mutable alias
changes a stored key's equality or hash after insertion, the map does not
automatically reindex it. The entry can remain visible during iteration yet
be unreachable by lookup or removal with the mutated key: a ghost entry.
Such mutation does not trigger a compile-time error or an automatic repair.

## Type Inference Boundaries

The compiler infers local binding types, closure parameter or result types when
an expected function type supplies them, and generic call arguments when the
solution is unambiguous.

The following declarations require explicit types:

- named function parameters and results;
- public and private data fields;
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
