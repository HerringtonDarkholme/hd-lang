# Type System

Status: language specification draft.

This chapter defines types, assignability, access permission, generics, and
variance.

1. r[types.static] hd-lang is statically typed: every expression has a compile-time type.
2. r[types.no-execute] A program with a type error must not execute.
3. r[types.local-inference] Local inference removes redundant annotations; public and aggregate boundaries remain explicit.

See also: [Type Inference Boundaries](#type-inference-boundaries).

## Type Forms

r[types.forms.set] The type forms are:

| Form | Spelling |
| --- | --- |
| Primitive types | |
| Nominal data types and enums | |
| Tuples | |
| Lists and maps | `List[T]`, `Map[K, V]` |
| Optional types | `T?` |
| Function types | `fn(...) -> T`, sugar for `Fn[(...), T, $()]` |
| Suspending function types | `fn!(...) -> T`, sugar for `SuspendFn[(...), T, $()]` |
| Requirement-bearing function types | ending in `$ Row` |
| Generic instantiations | |
| Associated type projections | such as `T::Item` |
| Variadic type packs and pack expansions | |
| Trait value types | |
| Transparent aliases and nominal newtypes | |
| Mutable-access types | `mut T` |

1. r[types.suspend] `Suspend[T]` is the dynamic one-shot computation protocol.
2. r[types.gadt-equalities] GADT refinements are arm-local type equalities rather than additional runtime type forms.

See also: [Requirements and Suspension](11-requirements-and-suspension.md),
[Variadic Generics](12-variadic-generics.md),
[Generalized Algebraic Data Types](13-gadts.md).

### The `never` Type

1. r[types.never] `never` is the uninhabited bottom type.
2. r[types.never.assignable] `never` is assignable to every type, and no ordinary value is assignable to it.
3. r[types.never.abrupt] An expression that completes abruptly has type `never` on that control-flow path.
4. r[types.never.abrupt-forms] These expressions complete abruptly: an unconditional `return`, `break`, or `continue`, propagation that exits the current body, and a call to `panic`.
5. r[types.never.expressions] `return`, `break`, `continue`, a call to `panic`, and any other call whose result type is `never` are expressions of type `never`. These five forms are the complete list.
6. r[types.never.fits] Each of them therefore fits any expected type, as in a match arm `.None => continue` or a branch `else: break`.

```text
fn total(entries: List[i32?]) -> i32:
    let sum = 0
    for entry in entries:
        amount := match entry:
            .Some(value) => value
            .None => continue
        sum = sum + amount
    sum
```

> **Note.** This explains why every reachable branch of a value-producing
> `if` or `match` may end in one of these forms. It changes no program's
> validity.

## Primitive Types

r[types.prim.set] The primitive types are:

| Category | Types |
| --- | --- |
| Boolean | `bool` |
| Signed integers | `i8`, `i16`, `i32`, `i64` |
| Unsigned integers | `u8`, `u16`, `u32`, `u64` |
| Floating point | `f32`, `f64` |
| Text | `char`, `string` |

1. r[types.prim.no-aliases] There are no core aliases named `int`, `uint`, or `float`.
2. r[types.prim.decimal] `decimal` may be a standard-library type but is not primitive.
3. r[types.prim.no-byte] hd-lang has no primitive `byte` or `bytes` type and no byte literal syntax.
4. r[types.prim.by-value] Primitive values are passed and returned by value.
5. r[types.prim.no-mut] Primitive types do not accept the `mut` modifier.
6. r[types.prim.no-mut.error] A primitive type written with `mut`, as in `let n: mut i32 = 0`, is an error. Error: `mut-on-primitive`.

```text
fn invalid(scale: mut f64) -> void:  # error: mut-on-primitive
    let n: mut i32 = 0  # error: mut-on-primitive
```

> **Why.** Primitive types do not expose mutable reference state.

### `void` And The Empty Tuple

1. r[types.void] `void` is the return type of a function that produces no useful value.
2. r[types.unit] The empty tuple value is `()`.
3. r[types.void.role] `void` describes the absence of a useful function or statement result.
4. r[types.unit.role] `()` is a tuple value and tuple type.
5. r[types.void.distinct] `void` and `()` are distinct types, and there is no implicit conversion between them.

> **Note.** Both carry no information, but they serve different source-level
> roles.

### Strings

A `string` is an immutable sequence of bytes that is always valid UTF-8. Its
length and its index count bytes:

```text
fn initial(name: string) -> u8:
    name[0]

fn size(name: string) -> i32:
    name.len()  # 6 for "héllo"
```

1. r[types.string.utf8-bytes] A `string` is a sequence of bytes whose contents cannot be mutated.
2. r[types.string.valid-utf8] Every `string` value is valid UTF-8.
3. r[types.string.literal-utf8] The value of a string literal is the UTF-8 encoding of its Unicode scalar values.
4. r[types.string.concat-bytes] Concatenation joins the bytes of its operands, so its result is valid UTF-8.
5. r[types.string.from-bytes] A conversion from arbitrary bytes to `string` must check that they are valid UTF-8, and must fail when they are not.
6. r[types.string.boundary] A **scalar boundary** of a string is a byte offset from `0` to its length that does not fall inside the encoding of a scalar value.
7. r[types.string.split-scalar] An operation given a byte offset that is not a scalar boundary is a checked runtime panic.
8. r[types.string.no-normalization] Source text and runtime operations do not perform Unicode normalization.
9. r[types.string.compare-bytes] Equality and ordering compare bytes in sequence.
10. r[types.string.encodings] Canonically equivalent but differently encoded scalar sequences are distinct.
11. r[types.string.len-bytes] `string.len()` is the number of bytes, and takes constant time.
12. r[types.string.graphemes] Libraries may provide grapheme traversal.
13. r[types.string.host-bytes] At every Wasm host boundary, a string crosses as its UTF-8 bytes with no conversion, including embedded U+0000 scalar values.

> **Note.** For valid UTF-8, the order of bytes is the order of scalar
> values, so comparing bytes sorts strings by scalar value.

> **Why.** This is Go's layout with the validity guarantee of Rust and
> Swift. Length and indexing take constant time, and a host string crosses
> without conversion. Because `len` and `s[i]` count bytes, a string is not
> `Iterable`: a loop over characters is written out, so no code silently
> treats a string as characters.

See also: [String Indexing](05-expressions.md#string-indexing),
[Ordering](05-expressions.md#ordering),
[Iteration Protocols](06-control-flow.md#iteration-protocols),
[String Methods](10-modules.md#string-methods).

## Literal Types

An integer literal with no expected type has type `i32`:

```text
x := 1  # i32
```

### Integer Literals

1. r[types.literal.int-default] An integer literal in any supported radix with no expected type has type `i32` in every value range.
2. r[types.literal.int-no-widen] An integer literal does not automatically choose a wider type.
3. r[types.literal.int-range] When an integer literal has an expected integer type, the compiler checks the literal against that type's range. A literal outside it is an error. Error: `integer-literal-range`.

```text
let small: i8 = 1    # valid
let bad: u8 = 300    # error: integer-literal-range
```

4. r[types.literal.int-diagnostic] The diagnostic must identify the literal, the target range, and an appropriate wider type when one exists.
5. r[types.literal.unary-plus] An expected numeric type passes through unary `+` to a numeric literal, so `let positive: u8 = +1` checks the literal against the `u8` range.

### Negated Integer Literals

1. r[types.literal.negation] Under an expected signed integer type, unary `-` applied directly to an integer literal is range-checked as a unit. The check considers the negated mathematical value.
2. r[types.literal.negation.minimum] This makes `let minimum: i8 = -128` valid even though positive `128` does not fit in `i8`.
3. r[types.literal.negation.unsigned] A negated literal is invalid for an unsigned expected type.
4. r[types.literal.negation.below-minimum] Values below the signed minimum remain errors.

### Floating-Point Literals

1. r[types.literal.float-default] A floating-point literal with no expected type has type `f64` and must be representable as a finite `f64` value.
2. r[types.literal.float-expected] When an expected `f32` or `f64` type is available, the literal is converted directly to that type.
3. r[types.literal.float-finite] The converted literal must be representable as a finite value under that type's IEEE 754 rounding rules. Error: `float-literal-range`.

### Suffixed Literals

A suffixed literal has the type that its suffix function returns:

```text
use std.time.{Duration, ms}

fn delay() -> Duration:
    250ms  # ms(250): Duration
```

1. r[types.literal.suffixed] A suffixed literal has the result type of its suffix function.
2. r[types.literal.suffixed.in] The numeric literal is checked with the suffix function's parameter type as its expected type, by the rules above. So a suffix whose parameter is `i8` range-checks `300b`.
3. r[types.literal.suffixed.kind] A literal of the wrong kind for that parameter is an error, so `1.5s` is invalid when `s` takes an `i64`; write `1500ms`. Error: `type-mismatch`.
4. r[types.literal.suffixed.negation] Unary `-` applied directly to a suffixed literal negates the numeric literal before the call, so `-5s` means `s(-5)`.
5. r[types.literal.suffixed.negation.check] The negated literal is checked against the parameter type as a unit, as [Negated Integer Literals](#negated-integer-literals) are, so an `i8` parameter accepts `-128b`.

```text
use std.time.s

fn wait() -> void:
    half := 1.5s  # error: type-mismatch
    pass
```

See also: [Literal Suffixes](05-expressions.md#literal-suffixes).

### Prefixed Strings

A prefixed string has the type that its prefix function returns, and each
interpolated value is checked like an argument:

```text
use std.ops.{Template, str_prefix}

@str_prefix
fn ids(t: Template[i32]) -> string:
    "ids"

fn find(name: string, big: i64) -> string:
    first := ids"id = $name"  # error: type-mismatch
    ids"id = $big"  # error: implicit-narrowing
```

1. r[types.literal.prefixed] A prefixed string has the result type of its prefix function.
2. r[types.literal.prefixed.values] Each interpolated expression is checked with the `T` of the prefix function's `Template[T]` parameter as its expected type, as an argument is, and converts to `T` by the same rules.
3. r[types.literal.prefixed.value-errors] An interpolated expression that cannot convert to `T` is an error at that expression: `type-mismatch` for a `string` where `T` is `i32`, and `implicit-narrowing` for an `i64` there.
4. r[types.literal.prefixed.display] When `T` is the trait value type `Display`, every value whose type implements `Display` converts, as [Dynamic Trait Values](09-traits.md#dynamic-trait-values) specifies.

See also: [Prefixed Strings](05-expressions.md#prefixed-strings).

### Other Literals

1. r[types.literal.bool] `true` and `false` have type `bool`.
2. r[types.literal.string] A string literal has type `string`, and a character literal has type `char`.
3. r[types.literal.no-absent] There is no literal for an absent optional; it is the enum variant `.None`.

See also: [Optional Types](#optional-types).

## Nominal And Structural Types

Each `data` and `enum` declaration introduces a distinct nominal type:

```text
data UserId:
    value: string

data PostId:
    value: string
```

`UserId` and `PostId` are distinct even though their field shapes match.

1. r[types.nominal.distinct] Each `data` and `enum` declaration introduces a distinct nominal type.
2. r[types.nominal.shape] Matching fields or variants do not make two nominal types interchangeable.

### Tuple Types

1. r[types.tuple.structural] Tuple types are structural.
2. r[types.tuple.same] Two tuples have the same type when they have the same arity and pairwise-equal element types.
3. r[types.tuple.one-element] One-element tuples require a trailing comma.
4. r[types.tuple.empty] `()` is the empty tuple.

### Function Type Identity

1. r[types.fn.constructor] A function type is an application of the standard constructor `Fn` or `SuspendFn`, and the `fn(...) -> T` spelling is exact sugar for it.
2. r[types.fn.same] Two function types are the same type when they apply the same constructor to the same inputs, the same output, and the same normalized requirement row.
3. r[types.fn.declared-variance] Function types convert only by the declared variance of their constructor, as [Readonly Outer Views](#readonly-outer-views) states.

See also: [Function Type Constructors](07-functions.md#function-type-constructors),
[Variance](#variance).

## Transparent Aliases And Newtypes

A transparent alias introduces another name for the same type:

```text
type UserName = string
```

1. r[types.alias.same] A transparent alias introduces another name for the same type.
2. r[types.alias.identical] `UserName` and `string` are identical for assignability, method lookup, trait conformance, and runtime representation.
3. r[types.alias.row] A transparent alias may also name a requirement row, as [Row Aliases](11-requirements-and-suspension.md#row-aliases) defines.
4. r[types.alias.cycle] An alias that expands to itself, directly or through other aliases, is an error. Error: `alias-cycle`.
5. r[types.alias.cycle.reported] The error is reported once per cycle, on the declaration of the cycle that comes first in the source.

```text
type Left = List[Right]  # error: alias-cycle

type Right = Left?
```

> **Why.** A transparent alias is replaced by its right side, so a cycle
> never ends. A recursive type needs a `data` or `enum` declaration.

### Newtypes

A parenthesized type declaration creates a nominal single-field newtype:

```text
type Mile(i32)
```

1. r[types.newtype.decl] A parenthesized type declaration creates a nominal single-field newtype.
2. r[types.newtype.distinct] `Mile` is distinct from `i32`.
3. r[types.newtype.construct] Constructor syntax creates the newtype, and the base type constructor unwraps it:

```text
m := Mile(10)
n := i32(m)
```

4. r[types.newtype.no-implicit] No implicit conversion exists in either direction.
5. r[types.newtype.no-inherit] A newtype does not inherit trait implementations from its underlying type.
6. r[types.newtype.map-key] In particular, a newtype is not a valid map key until it explicitly implements or derives both `Eq` and `Hash`.
7. r[types.newtype.construct-ref] Constructing a newtype over an `AnyRef` base creates no new object: the newtype value wraps the base value.
8. r[types.newtype.construct-permission] So such a construction carries its base value's permission. `Order(d)` has type `mut Order` exactly when `d` has mutable access, and readonly `Order` otherwise.
9. r[types.newtype.construct-value] A newtype construction over an `AnyVal` base, such as `Mile(10)`, is a value, as its base is. It is not a fresh mutable object.
10. r[types.newtype.unwrap-permission] Unwrapping a newtype over an `AnyRef` base carries the newtype value's permission. `Draft(o)` has type `mut Draft` exactly when `o` has mutable access, and readonly `Draft` otherwise.

```text
data Draft:
    lines: List[string]

type Order(Draft)

fn keep(open: Order, closed: Order) -> void:
    pass

fn wrap(draft: mut Draft, seen: Draft) -> void:
    let open: mut Order = Order(draft)
    let closed: mut Order = Order(seen)  # error: mutable-upgrade
    keep(open, closed)

fn unwrap(order: mut Order, sent: Order) -> void:
    let editable: mut Draft = Draft(order)
    let frozen: mut Draft = Draft(sent)  # error: mutable-upgrade
    editable.lines = frozen.lines
```

See also: [Map Key Types](#map-key-types).

## Optional Types

The prelude declares the ordinary generic enum `Option`:

```text
enum Option[T]:
    Some(value: T)
    None
```

1. r[types.option.decl] The prelude declares the ordinary generic enum `Option`.
2. r[types.option.sugar] `T?` is exact sugar for `Option[T]`: the two spellings denote the same type everywhere a type may appear, including implementation targets.
3. r[types.option.distinct] `T` and `T?` are different types; a non-optional type never contains an absent value.
4. r[types.option.prelude-name] `Option` is the only prelude name this adds; `Some` and `None` are variants, not prelude names.
5. r[types.option.bare-variant] A bare `None` or `Some(value)` expression therefore resolves like any other identifier. It is an error unless a declaration in scope supplies that name. Error: `unknown-name`.

### Optional Values

1. r[types.option.none] The absent value is written `.None` where an optional type is expected, or `Option.None`.
2. r[types.option.some] A present value is written `.Some(value)` or `Option.Some(value)`.
3. r[types.option.construction] These spellings follow the ordinary enum construction rules.
4. r[types.option.none-context] In particular, `.None` without an expected optional type is an error. Error: `missing-contextual-enum-type`.
5. r[types.option.patterns] Optionals are matched with ordinary enum patterns, `.Some(pattern)`, `.None`, and their `Option.`-qualified forms.

```text
let value: i32? = None  # error: unknown-name
missing := .None        # error: missing-contextual-enum-type
```

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations),
[Match Expressions](06-control-flow.md#match-expressions).

### Implicit Wrapping And Nesting

1. r[types.option.wrap] A value of `T` is also implicitly accepted where `T?` is expected, constructing `.Some(value)`.
2. r[types.option.wrap.sites] The implicit wrap applies to assignments, arguments, and return values.
3. r[types.option.wrap.once] The source expression is evaluated once.
4. r[types.option.wrap.one-layer] The implicit wrap adds one layer only.
5. r[types.option.nest] Optionality may nest: `T??` is `Option[Option[T]]`. It preserves the distinction between an absent outer value and a present outer value containing an absent inner value.
6. r[types.option.nest.accept] A `T?` value is accepted where `T??` is expected, but a plain `T` is not.
7. r[types.option.nest.explicit] The inner layer needs an explicit `.Some(...)`, as in `let nested: i32?? = .Some(1)`.
8. r[types.option.nest.none] `.None` with an expected `T??` is the outer absent value, and `.Some(.None)` is a present outer value holding an absent inner value.

### Representation And Propagation

1. r[types.option.repr] The representation of `Option[T]` is an implementation detail; for example, an implementation may represent `.None` as a null reference.
2. r[types.option.propagate] Postfix `?` on an optional expression either produces its contained value or returns `.None` from the nearest function.
3. r[types.option.propagate.result] That function must itself return a compatible optional type.
4. r[types.option.propagate.one-layer] Postfix `?` removes and propagates one optional layer at a time.
5. r[types.option.propagate.permission] A present value keeps the optional's declared contained type `T`, including `mut U` when `T = mut U`; unwrapping does not weaken that generic argument.

### Optional Identity And Variance

1. r[types.option.ordinary] Optionals follow the ordinary enum rules in every other respect.
2. r[types.option.any] An optional value may be erased to `Any` like any other enum value.
3. r[types.option.identity] `is` compares optionals as it compares other enum values.
4. r[types.option.identity.none] `.None` is payload-free and has one canonical identity.
5. r[types.option.identity.some] Each construction of `.Some(value)`, including an implicit wrap, has its own identity.
6. r[types.option.variance] Optional is covariant in its contained type for a readonly outer value.
7. r[types.option.variance.result] `Result[T, E]` is likewise covariant in both `T` and `E` for a readonly outer value.
8. r[types.option.variance.mutable] As with every generic composite, a mutable outer view is invariant.

See also: [Unary And Binary Operators](05-expressions.md#unary-and-binary-operators),
[Variance](#variance).

## Result Types

Recoverable errors use the prelude enum `Result`.

1. r[types.result.decl] Recoverable errors use the prelude enum `enum Result[T, E]: Ok(value: T); Err(error: E)`.
2. r[types.result.prelude-name] `Result` is a prelude name; `Ok` and `Err` are its variants, not prelude names.
3. r[types.result.construction] Values follow the ordinary enum construction rules: `.Ok(value)` and `.Err(error)` where a `Result` type is expected, or `Result.Ok(value)` and `Result.Err(error)`.
4. r[types.result.ok-context] In particular, `.Ok(value)` without an expected `Result` type is an error. Error: `missing-contextual-enum-type`.
5. r[types.result.bare-variant] A bare `Ok(value)` or `Err(error)` expression resolves like any other identifier. It is an error unless a declaration in scope supplies that name. Error: `unknown-name`.
6. r[types.result.patterns] Results are matched with the patterns `.Ok(pattern)`, `.Err(pattern)`, and their `Result.`-qualified forms.

```text
fn parse() -> Result[i32, string]:
    Ok(1)  # error: unknown-name
```

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations),
[Match Expressions](06-control-flow.md#match-expressions).

### Void Results

1. r[types.result.void-ok] When `T` is `void`, the success constructor is written `.Ok()` or `Result.Ok()` and has type `Result[void, E]` under an expected result type.
2. r[types.result.no-ok-pass] `.Ok(pass)` is not the source spelling for this case.

### Result Propagation

1. r[types.result.propagate] Postfix `?` on `Result[T, E]` either produces the success value or immediately returns the error from the nearest function.
2. r[types.result.propagate.enclosing] The enclosing function must return a `Result[U, F]`.
3. r[types.result.propagate.one-step] The error reaches `F` in one step: by one rule of [Assignability And Coercion](#assignability-and-coercion), or otherwise by one call of `F`'s `From[E]` implementation.
4. r[types.result.propagate.not-both] The step is never both, and conversions are never chained.
5. r[types.result.err-value] Returning an `.Err` value without `?` does not itself alter control flow, and it never calls a conversion.
6. r[types.result.propagate.permission] The success value retains its declared generic type `T`, including `mut U` when `T = mut U`, regardless of whether the `Result` value itself is readonly.
7. r[types.result.no-effects] The core language does not use effect syntax for recoverable errors.

See also: [Propagation](05-expressions.md#propagation), which defines the rule
and its diagnostic, and [Conversion Trait](09-traits.md#conversion-trait).

## Numeric Conversions

Implicit integer conversion widens along these chains:

```text
i8 -> i16 -> i32 -> i64
u8 -> u16 -> u32 -> u64
```

1. r[types.num.widen] Implicit integer conversion is limited to widening conversions that preserve every value of the source type. An implicit narrowing conversion is an error. Error: `implicit-narrowing`.
2. r[types.num.chains] The signed and unsigned widening chains are the two chains above.
3. r[types.num.no-sign-change] There is no implicit conversion between signed and unsigned integers.
4. r[types.num.no-int-float] There is no implicit integer-to-floating or floating-to-integer conversion in the current core.
5. r[types.num.f32-f64] `f32` widens implicitly to `f64`; `f64` to `f32` requires an explicit cast.

### Binary Numeric Operators

1. r[types.num.binary.literal] For a binary numeric operator, an untyped literal first adopts the compatible type expected from the other operand.
2. r[types.num.binary.widen] Otherwise, operands within one integer signedness family widen to the wider operand type, and the result has that type.
3. r[types.num.binary.float] `f32 op f32` produces `f32`; when one operand is `f64`, an `f32` operand widens and the result is `f64`.
4. r[types.num.binary.no-mix] Signed and unsigned integers do not mix implicitly, and integers do not mix implicitly with floating-point values. A signed and an unsigned operand are an error. Error: `mixed-signedness`.
5. r[types.num.binary.cast] The user must cast one operand explicitly in those cases.

### Numeric Casts

Explicit numeric conversion uses constructor-style casts:

```text
let wide: i64 = 9000
narrow := i16(wide)
```

1. r[types.cast.syntax] Explicit numeric conversion uses constructor-style casts.
2. r[types.cast.wrap] An integer-to-integer cast wraps at run time, as Go conversions and Rust `as` do: the result keeps the low bits of the source value's two's-complement form, read in the target type. It never panics.
3. r[types.cast.wrap.example] So `u8(x)` with `x = 300` gives 44, and `i8(x)` with `x = 200` gives -56.
4. r[types.cast.literal-range] When a cast's argument is an integer literal, alone or under unary `-` or `+`, the literal is checked with the target type as its expected type, by [Integer Literals](#integer-literals). An out-of-range literal is an error, as Go reports a constant overflow. Error: `integer-literal-range`.
5. r[types.cast.fallible] Libraries may provide separate fallible conversion functions returning `Result`.

```text
fn narrow(wide: i64) -> u8:
    u8(wide)          # wraps: 300 gives 44
fn fixed() -> u8:
    u8(300)           # error: integer-literal-range
```

The core numeric cast rules are:

| Cast | Behavior |
| --- | --- |
| r[types.cast.int-int-wrap] Integer to integer | wraps to the target width, by `types.cast.wrap` |
| r[types.cast.int-float] Integer to floating point | rounds to the nearest representable IEEE 754 value using ties-to-even |
| r[types.cast.f32-f64] `f32` to `f64` | is exact |
| r[types.cast.f64-f32] `f64` to `f32` | uses IEEE 754 ties-to-even rounding |
| r[types.cast.float-int-saturate] Floating point to integer | truncates toward zero, then saturates, by `types.cast.saturate` |

1. r[types.cast.saturate] A floating-to-integer cast saturates, as Rust `as` does. A value below the target's minimum, negative infinity included, gives the minimum. A value above its maximum, positive infinity included, gives the maximum. NaN gives 0.
2. r[types.cast.saturate.example] So `i8(x)` with `x = 300.0` gives 127, `u8(x)` with `x = -1.5` gives 0, and `i32(x)` gives 0 when `x` is NaN.
3. r[types.cast.no-panic] No numeric cast panics.
4. r[types.cast.precision] An explicit numeric cast may lose precision according to these rules.
5. r[types.cast.exact] Libraries may expose exact or fallible conversions when loss must be rejected.
6. r[types.cast.nonnumeric] Constructor-style calls involving nonnumeric types are not numeric casts: they must resolve to a nominal newtype constructor, enum constructor, or ordinary function.

> **Why.** Every numeric cast is total, as in Rust since 1.45, so a cast
> never needs a panic path. Code that must reject an out-of-range value uses
> a fallible library conversion.

### Integer Arithmetic

1. r[types.arith.checked] Integer arithmetic is checked.
2. r[types.arith.failure] Overflow, invalid shifts, and division errors cause checked runtime failure unless an explicit wrapping or fallible library operation is used.
3. r[types.arith.division] Integer division truncates toward zero.
4. r[types.arith.remainder] Integer remainder has the sign of the dividend.
5. r[types.arith.shift-count] A shift count must be non-negative and smaller than the bit width of the shifted value.
6. r[types.arith.shift-right] Right shift of a signed integer is arithmetic and sign-extending.
7. r[types.arith.min-division] For every signed width, `MIN / -1` panics with `integer-overflow`.
8. r[types.arith.min-remainder] For every signed width, `MIN % -1` produces zero.

See also: [Runtime Panics](06-control-flow.md#runtime-panics).

### Numeric Display

1. r[types.display.int] `Display` formats integers in base ten.
2. r[types.display.float] `Display` formats floating values with the shortest round-trip decimal digits.
3. r[types.display.width] The digits round-trip at the value's own width, so an `f32` displays its `f32` digits even when formatted through generic code.
4. r[types.display.notation] Finite floats use fixed notation when the normalized decimal exponent is in `[-6, 21)` and lowercase scientific notation otherwise.
5. r[types.display.exponent] Scientific exponents always include `+` or `-` and no leading zeroes.
6. r[types.display.fixed] Fixed notation always contains a decimal point and at least one fractional digit, so `1.0` remains visibly floating.
7. r[types.display.special] Negative zero is `-0.0`; infinities are `inf` and `-inf`; every NaN is `NaN`.
8. r[types.display.nan-canonical] Before hashing, boundary serialization, or `Display`, every NaN is replaced with the one canonical quiet-NaN value for its width.
9. r[types.display.nan-compare] NaN comparison continues to follow IEEE 754.

## Assignability And Coercion

r[types.assign] An expression of type `S` is assignable to a location of type `T` when at least one of these rules applies:

1. r[types.assign.identical] `S` and `T` are identical after expanding transparent aliases.
2. r[types.assign.int-widen] `S` is an integer type with a value-preserving widening conversion to `T`.
3. r[types.assign.float-widen] `S` is `f32` and `T` is `f64`.
4. r[types.assign.weaken] `S` is `mut T` and the target requests the readonly view `T`.
5. r[types.assign.variance] A declared generic variance conversion permits the readonly outer type to change its type arguments.
6. r[types.assign.trait-value] `S` explicitly implements trait `T`, or `T` is `Inspectable` and `S` is an inspectable type, allowing construction of a dynamic trait value.
7. r[types.assign.supertrait] `S` is a dynamic child-trait value whose trait has `T` as a direct or transitive supertrait.
8. r[types.assign.optional] A value of `T` is injected into `T?`. The injection adds one layer only, so a `T` is not injected into `T??`.
9. r[types.assign.shape] `S` is a specialized shape type returned by `shape[D]()` and `T` is its generic shape type, `DataShape` or `EnumShape`.
10. r[types.assign.row-subsumption] `S` and `T` are function types, `T`'s row entails every key of `S`'s row, and `S` with `T`'s row is assignable to `T`, as [Row Subsumption](11-requirements-and-suspension.md#row-subsumption) states.

See also: [Inspectable Types](09-traits.md#inspectable-types),
[Shape Intrinsics](14-annotations.md#shape-intrinsics).

### Assignment And Binding Types

1. r[types.assign.no-subtyping] No inheritance or structural record subtyping exists.
2. r[types.assign.binding-type] Assignment never changes the declared or inferred type of a binding.
3. r[types.assign.let] In particular, later assignment to a `let` binding must remain assignable to the type established at its declaration.

## Composite Values And Access Permission

Composite values are shared by reference, and `mut` grants mutable access to
one.

1. r[types.composite.kinds] Data types, enums with storage, tuples, lists, maps, trait values, and closures are composite values.
2. r[types.composite.shared] Composite parameters and results use shared references at the language level.
3. r[types.composite.aliases] hd-lang does not require exclusive ownership and may have multiple aliases to one composite value.

### Views

For a composite type `T`:

1. r[types.view.readonly] `T` is a readonly reference view. It permits observation but not mutation through that reference.
2. r[types.view.mutable] `mut T` is a mutable reference view. It permits operations that mutate the referenced value.
3. r[types.view.term] Throughout the specification, **readonly view** is the single term for `T` access to a composite value; it does not imply deep immutability.
4. r[types.view.non-reassignable] A binding is described separately as **non-reassignable** when its name cannot be rebound.

### Access Permission

1. r[types.mut.permission] `mut` expresses access permission, not ownership, uniqueness, or a deep freeze of the object.
2. r[types.mut.no-field-reassign] A readonly `T` reference cannot reassign its fields.
3. r[types.mut.mut-field] A direct data field declared `field: mut U` is read as `U` through readonly `T`.
4. r[types.mut.embedded] An embedded field follows its container's access.
5. r[types.mut.generic-field] A generic data field declared `field: P` retains its substituted type even when `P` is instantiated as `mut U`.
6. r[types.mut.other-forms] Other extraction forms state their own permission rules below.
7. r[types.mut.weaken] A `mut T` may be viewed as `T`.
8. r[types.mut.no-upgrade] A `T` must never be upgraded to `mut T`. An upgrade is an error. Error: `mutable-upgrade`.
9. r[types.mut.no-upgrade.inference] The error includes an upgrade that generic inference would produce. Given `keep[T](value: T) -> T`, binding `keep(readonly_value)` to a `mut T` declaration is rejected rather than inferring `T` as a mutable type.

```text
fn keep[T](value: T) -> T:
    value

fn invalid(user: User) -> void:
    let mutable: mut User = user     # error: mutable-upgrade
    let kept: mut User = keep(user)  # error: mutable-upgrade
```

### Readonly Views Are Shallow

1. r[types.readonly.blocks] A readonly view blocks reassignment of its fields and removes the `mut` of its direct `mut U` fields and embedded fields.
2. r[types.readonly.not-deep] A readonly view is not a deep authority boundary.
3. r[types.readonly.nested] A `mut U` supplied as an optional, tuple, collection, or other generic argument keeps its permission when that nested value is extracted.
4. r[types.readonly.deep-api] APIs that require a deep no-mutation guarantee must not expose mutable references through such nested field types.

```text
let mut user = User { name: "Ada" }
readonly := user
user.name = "Grace"
println(readonly.name)  # observes "Grace"
```

5. r[types.readonly.alias] The readonly alias prevents mutation through `readonly`; it does not freeze the underlying object against other mutable aliases.
6. r[types.readonly.no-guarantee] Readonly access does not guarantee any of the following:
   - that repeated reads return the same values;
   - that the object is a stable cache input;
   - that independently held mutable aliases cannot change its reachable state.
7. r[types.readonly.structural] A readonly view restricts structural access through that view.
8. r[types.readonly.results] A readonly view does not prohibit a called function from returning a separately held mutable reference according to its declared result type.

See also: [Mutable Paths](#mutable-paths).

### Bindings And Fresh Values

1. r[types.fresh.mutable] A fresh data or copy-update expression, stored enum construction, tuple expression, list expression, or map expression produces mutable access to its new outer object.
2. r[types.fresh.weaken] This permission may be weakened immediately by an expected readonly type.
3. r[types.fresh.not-recursive] Freshness does not recursively upgrade composite values stored in the new object.
4. r[types.fresh.element-permission] Each field or element keeps the permission of the supplied expression and declared edge.
5. r[types.fresh.element-no-weaken] An expected type weakens only the fresh expression it applies to, never the elements of a collection already built. So `[for p in parts => Word { text: p }].iter()` has type `mut Iterator[mut Word]`, and returning it as `mut Iterator[Word]` is an error. Error: `type-mismatch`.

#### Fresh Literals With Readonly Parts

1. r[types.fresh.readonly-field] A data literal with a direct `field: mut U` may produce readonly `T` when that field is supplied only `U`.
2. r[types.fresh.embedded-copy] Each embedded field receives a copy, which has readonly access when it is made from a readonly value whose type has mutable edges.
3. r[types.fresh.mut-literal] A literal produces `mut T` only when every direct mutable field is supplied mutable access and every embedded copy has mutable access.
4. r[types.fresh.spread] A value copied from a spread counts as supplied through the spread source's view.
5. r[types.fresh.expected-readonly] An expected readonly `T`, including a `:=` binding, permits the weaker field value.
6. r[types.fresh.expected-mut] An expected `mut T` rejects the weaker field value. Error: `mutable-upgrade`.
7. r[types.fresh.expected-mut.sites] The expected type is `mut T` wherever the literal is used as `mut T`:
   - an annotated `mut T` binding;
   - a `mut T` argument or result;
   - a store into a `mut T` field or element.
8. r[types.fresh.generic-field] A generic field declared `field: P` still requires its substituted type, including `mut U` when `P = mut U`.

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

#### Binding Forms

1. r[types.bind.short] `:=` always exposes a readonly composite view, even when its initializer creates a fresh value:

```text
user := User { name: "Ada" }  # User
```

2. r[types.bind.let-mut] A `let` annotation may state mutable access explicitly:

```text
let names: mut List[string] = []
```

3. r[types.bind.let-readonly] A `let` without `mut` and without an annotation infers the readonly view of its initializer's type, even when the initializer creates a fresh value.
4. r[types.bind.let-rebind] Such a binding may still be reassigned; only mutation through it is rejected.

```text
fn rename(other: User) -> void:
    let user = User { name: "Ada" }  # User
    user = other                     # valid: reassignment
```

```text
fn invalid() -> void:
    let user = User { name: "Ada" }
    user.name = "Grace"  # error: readonly-root
```

5. r[types.bind.let-mut-infer] `let mut name = value` infers mutable access: when `value` has type `mut T`, the binding has type `mut T`.
6. r[types.bind.let-mut-upgrade] `let mut` never upgrades access: an initializer with a readonly type is an error. Error: `mutable-upgrade`.
7. r[types.bind.let-mut-primitive] `let mut` on a name whose type is primitive, as in `let mut n = 0`, is an error, in place of `mutable-upgrade` or `let-mut-readonly-type`. Error: `mut-on-primitive`.
8. r[types.bind.let-mut-primitive.hint] The diagnostic says that a plain `let` is already reassignable.
9. r[types.bind.let-mut-optional] For `let mut`, an optional written `mut T?` counts as mutable access. So `let mut u = find()` is valid when `find` returns `mut User?`, and `u` has that type.
10. r[types.bind.let-mut-expected] The initializer of an unannotated `let mut` is used as `mut T`, so a fresh literal whose direct `mut` field or embedded copy is readonly is an error, as [`types.fresh.expected-mut`](#r-types.fresh.expected-mut) states. Error: `mutable-upgrade`.
11. r[types.bind.let-mut-copy] A mutable copy of a readonly value is therefore written as a fresh literal, such as `User { ...user }`, whose `mut` fields are supplied mutable values.

```text
fn find() -> mut User?:
    .Some(User { name: "Lin" })

fn edit(user: User) -> void:
    let mut draft = User { name: "Ada" }  # mut User
    draft.name = "Grace"
    let mut copy = User { ...user }       # a fresh copy
    copy.name = "Grace"
    let mut found = find()                # mut User?
```

```text
fn invalid(user: User) -> void:
    let mut alias = user  # error: mutable-upgrade
    let mut count = 0     # error: mut-on-primitive
    let mut n: i32 = 0    # error: mut-on-primitive
```

12. r[types.bind.let-mut-annotated] `let mut` with an annotation whose type is `mut T`, as in `let mut user: mut User = ...`, is valid; the `mut` after `let` is redundant.
13. r[types.bind.let-mut-annotated.warning] That redundant `mut` gets a warning. Warning: `redundant-let-mut`.
14. r[types.bind.let-mut-annotation] `let mut` with a readonly annotation, as in `let mut user: User = ...`, is an error, because the annotation and `let mut` disagree. Error: `let-mut-readonly-type`.
15. r[types.bind.let-mut-annotation.fix] The diagnostic suggests adding `mut` to the type or removing the `mut` after `let`.

```text
fn invalid() -> void:
    let mut names: List[string] = []  # error: let-mut-readonly-type
    let mut ids: mut List[i64] = []   # warning: redundant-let-mut
```

16. r[types.bind.let-mut-pattern] In a multi-name `let`, each name follows these rules for its own tuple element. In `let (mut log, db) = pair`, `log` has mutable access and `db` the readonly view.
17. r[types.bind.let-mut-pattern.annotated] With a tuple annotation, the element type of each name written `mut` must be a `mut` type. Error: `let-mut-readonly-type`.

```text
fn pair() -> (mut User, mut User):
    (User { name: "Ada" }, User { name: "Grace" })

fn edit() -> void:
    let (mut first, second) = pair()  # mut User, User
    first.name = "Lin"
    println(second.name)
```

> **Note.** No binding form is non-reassignable and mutable at once: a
> local that is mutated but never reassigned is written with `let mut`.

> **Why.** `let mut` removes the repeated type from
> `let mut user = User { ... }`, while the declaration still says
> which locals change. `mut` keeps one meaning, a permission in the type.

18. r[types.bind.call-result] Passing through a function also follows the declared result type rather than recovering freshness.
19. r[types.bind.call-result.argument] Consequently, a fresh literal may be passed directly to a `mut T` parameter. A call declared to return `T` cannot be passed to one, even when its implementation constructs a fresh value.

### Mutable Paths

This section defines the access type of each expression in an access path,
and the mutations that access type permits.

1. r[types.path.access-type] Every expression in an access path has an access type, computed one step at a time from the expression it extends.
2. r[types.path.root] A binding, parameter, or `self` has its declared or inferred type.
3. r[types.path.call] A call has its callable's declared result type, whatever value the call was reached through.
4. r[types.path.field] A field read `e.field` depends on the kind of field, and on whether `e` has type `mut T` or readonly type `T`, as this table shows.

| Field kind | Declared | Through `mut T` | Through readonly `T` |
| --- | --- | --- | --- |
| r[types.path.field.mutable-edge] **mutable edge** | `field: mut U` | `mut U` | `U` |
| r[types.path.field.readonly-edge] **readonly edge** | `field: U` with a composite `U` | `U` | `U` |
| r[types.path.field.embedded] **embedded field** | `E` | `mut E` | `E` |
| r[types.path.field.generic] **generic field** | `field: P`, with a generic parameter `P` | the substituted type, unchanged | the substituted type, unchanged |

5. r[types.path.field.mutable-edge.removed] The `mut` written directly in a mutable edge's declaration is removed by a readonly container.
6. r[types.path.field.embedded.not-readonly-edge] An embedded field yields the container's access and is never a readonly edge.
7. r[types.path.field.generic.example] Reading `value: P` from readonly `Box[mut User]` therefore yields `mut User`.
8. r[types.path.field.substituted] Every type is read after substitution of the container's type arguments.
9. r[types.path.promoted] A promoted field or method is reached through its embedded fields step by step.
10. r[types.path.collection] Indexing, iteration, and lookup on a built-in collection yield its declared element or value type, whatever the collection's own permission.
11. r[types.path.collection.example] Indexing readonly `List[mut User]` yields `mut User`, and a successful lookup in a readonly `Map[K, mut User]` yields `mut User` after unwrapping.
12. r[types.path.contents] Optional and `Result` unwrapping, tuple element extraction, and generic enum payloads likewise yield their declared contents.
13. r[types.path.enum-payload] A non-generic enum payload declared `mut U` follows the field rule above.

See also: [Data Embedding](08-data-and-enums.md#data-embedding),
[Member Resolution](03-names-and-scopes.md#member-resolution).

#### Mutation Checks

1. r[types.path.mutation] A mutation needs mutable access on exactly one expression: the one it acts on.
2. r[types.path.mutation.forms] Each of these requires `e` to have type `mut T`:
   - reassigning `e.field`;
   - replacing an element with `e[i] = value`;
   - calling a container-mutating method on `e`;
   - calling a `mut self` method on `e`.
3. r[types.path.mutation.only] Nothing else in the path is checked: the access type of `e` already records every permission removed on the way to it.

```text
data Account:
    profile: mut Profile

let mut account = Account { profile: profile }
account.profile.display_name = "Ada"   # account.profile has type mut Profile
```

When `e` is readonly, the diagnostic names why:

4. r[types.path.readonly-edge] A mutation through an `e` that is a field read through a readonly edge, `field: U`, is an error. Error: `readonly-edge`.
5. r[types.path.readonly-root] A mutation through any other readonly `e` is an error. Error: `readonly-root`.
6. r[types.path.readonly-root.cases] The `readonly-root` cases are these:
   - a readonly binding, parameter, `self`, call result, element, or unwrapped value;
   - a mutable edge or embedded field read through a readonly value;
   - a generic field whose type argument is readonly.

```text
data Child:
    name: string

data Parent:
    child: Child

fn invalid(parent: mut Parent, child: Child) -> void:
    parent.child.name = "new"  # error: readonly-edge
    child.name = "new"         # error: readonly-root
```

#### Field Stores

1. r[types.path.reassign] A `mut T` value may reassign every field with a value assignable to the field's declared type, whether the field is `field: U` or `field: mut U`.
2. r[types.path.reassign.old-value] Replacing a field does not mutate the old referenced value.
3. r[types.path.store-mut-edge] Storing into a direct `field: mut U` of a mutable value requires `mut U`.
4. r[types.path.store-readonly] A readonly value may store `U` there and cannot later be upgraded to `mut T`.
5. r[types.path.store-embedded] Storing into an embedded field, written `e.E ...= value`, stores a copy, which must have mutable access. Otherwise the store is an error. Error: `mutable-upgrade`.

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

#### Containers And Elements

r[types.path.container-element] Container mutation and element mutation are independent:

| Type | Replace elements | Mutate referenced elements |
| --- | --- | --- |
| `List[User]` | no | no |
| `List[mut User]` | no | yes |
| `mut List[User]` | yes | no |
| `mut List[mut User]` | yes | yes |

1. r[types.path.generic-args] Generic type arguments are never weakened because their enclosing value is readonly.
2. r[types.path.loses-mut] Only a mutable edge or an embedded field loses `mut` through a readonly container.
3. r[types.path.patterns] Data patterns and copy-update use the same access types as field reads on the subject.

### Parameters And Results

A function that returns mutable access declares `-> mut T`:

```text
fn new_user() -> User: User { name: "Ada" }
fn new_mutable_user() -> mut User: User { name: "Ada" }
```

1. r[types.param.readonly] `value: T` accepts readonly composite access.
2. r[types.param.mut] `value: mut T` requires mutable access.
3. r[types.param.mut-self] `mut self` is shorthand for `self: mut Self`.
4. r[types.return.mut] A function that returns mutable access must declare `-> mut T`.
5. r[types.return.readonly] A declared result of `T` exposes only readonly access, even when the function creates a fresh object internally.

## Generics

This section defines generic parameters, bounds, arguments, and reification.

1. r[types.generic.complete] Generic type and function parameters denote complete types.
2. r[types.generic.mut-argument] A type parameter `T` may therefore be instantiated with either `User` or `mut User`.
3. r[types.generic.naming] By convention, generic parameters, including row parameters, use uppercase names such as `T`, `U`, `K`, `V`, and `R`.
4. r[types.generic.kind] The convention is style only: a parameter's kind comes from its declaration and use, never from the case of its name.

### Mutable Bounds

Mutable generic requirements use a bound:

```text
fn clear_value[T < mut Clear](value: T) -> void:
    value.clear()
```

1. r[types.generic.direct] An unconstrained generic declaration stores or passes `T` directly.
2. r[types.generic.no-mut-t] An unconstrained generic declaration must not write `mut T`.
3. r[types.generic.mut-bound] Mutable generic requirements use a bound.
4. r[types.generic.mut-any] `T < mut Any` accepts any mutable-root type.
5. r[types.generic.mut-trait] `T < mut Trait` additionally requires the underlying type to implement `Trait`.
6. r[types.generic.trait-bound] A plain `T < Trait` requires trait conformance without mutable-root authority.

> **Why.** Substitution with `T = mut User` would turn `mut T` into the
> meaningless form `mut mut User`.

### Generic Arguments

1. r[types.generic.infer] Generic arguments are inferred at call sites when unambiguous.
2. r[types.generic.explicit] Callers may supply the complete generic argument list explicitly.
3. r[types.generic.short-list] An explicit list in an expression may omit trailing slots. Each omitted slot is inferred as a `_` slot is, then defaulted.
4. r[types.generic.placeholder-slot] In a generic function's or method's explicit list, `_` may occupy any slot and requests inference for that argument.
5. r[types.generic.placeholder.not-type] `_` is not itself a type and is invalid in ordinary type applications.
6. r[types.generic.too-long] A type-argument list with more positional arguments than its declaration has generic parameters is an error, in an expression and in a written type alike. Error: `argument-count`.

```text
fn count(names: List[string, i32]) -> i32:  # error: argument-count
    0
```

### Type-Argument Defaults

A generic parameter may declare a **type-argument default**, written with
`=` after its bound. It fills the parameter when a use site leaves it
unsolved or a written type omits it:

```text
data AppError:
    message: string

type Outcome[T, E = AppError] = Result[T, E]

fn load(path: string) -> Outcome[string]:
    .Ok(path)

fn widen[T = i64](value: T) -> T:
    value

fn small() -> i32:
    narrow := widen(3)
    narrow
```

1. r[types.generic.default.form] A generic parameter of a function, method, data type, enum, trait, or `type` declaration may declare a default.
2. r[types.generic.default.order] After the first parameter with a default, every later parameter of the same list must have one. A later parameter without one is an error. Error: `default-order`.
3. r[types.generic.default.scope] A default may name earlier parameters of the same list, the parameters of an enclosing declaration, and `Self` where `Self` is in scope.
4. r[types.generic.default.later] A default that names its own parameter or a later one is an error. Error: `binding-not-yet-visible`.
5. r[types.generic.default.kind] A default has its parameter's kind: a type for a type parameter, a row for a row parameter. A default of the other kind is an error. Error: `generic-kind-mismatch`.
6. r[types.generic.default.bound] A default must satisfy its parameter's bounds for every instantiation of the earlier parameters. A default that does not is an error. Error: `unsatisfied-trait-bound`.
7. r[types.generic.default.checked-once] That check is made once, at the declaration.

How a use site treats a defaulted parameter:

| Written | The parameter is |
| --- | --- |
| no list, in an expression | inferred; the default when inference leaves it unsolved |
| `_` in its slot | inferred; the default when inference leaves it unsolved |
| its slot omitted from an explicit list in an expression | inferred; the default when inference leaves it unsolved |
| its slot omitted, or no list, in a written type | the default |

8. r[types.generic.default.after-inference] At a use site that infers, such as a call, a function value, or a data literal, the use site first solves its parameters as it would without defaults.
9. r[types.generic.default.fill] Then each parameter left unsolved that has a default takes it, in declaration order, with the earlier arguments substituted. The bounds are checked last.
10. r[types.generic.default.argument-wins] A default never replaces a solution, so an argument or expected type that solves the parameter wins: `widen(3)` above has `T = i32`.
11. r[types.generic.default.unsolved] A parameter left unsolved that has no default stays an error. Error: `unresolved-generic-placeholder`.
12. r[types.generic.default.written] A written type, such as an annotation, a signature, a field, a bound, or an implementation header, infers nothing. An omitted trailing slot there takes its default.
13. r[types.generic.default.bare] A name written in a type without a list, or with bindings only, omits every positional slot. So `impl Same for Money` is valid for `trait Same[Other = Self]`.
14. r[types.generic.default.written-missing] A written type that omits a slot without a default is an error. Error: `partial-generic-arguments`.

```text
fn swap[A = B, B = i32](value: B) -> A:  # error: binding-not-yet-visible
    panic("unreachable")

fn pick[F = i32, T](value: T) -> T:  # error: default-order
    value

fn size(counts: Map[string]) -> i32:  # error: partial-generic-arguments
    0
```

> **Why.** Defaults apply only after inference, as in C++ and TypeScript,
> so adding a default never changes a program that compiled without it: it
> only fills a parameter that used to be an error. Rust's inference
> fallback could not keep that property. A default is written at the
> declaration, so declarations stay fully written; only use sites apply it.

See also: [Type-Argument Default Syntax](02-grammar.md#type-argument-default-syntax),
[Explicit Type Arguments](07-functions.md#explicit-type-arguments),
[Method Generic Parameters](09-traits.md#method-generic-parameters).

### Generic Representation

1. r[types.generic.unobservable] The runtime representation of generic code is not observable.
2. r[types.generic.sharing] A program cannot distinguish an implementation that shares one body among instantiations from one that specializes each instantiation, except through these rules of this chapter:
   - an erased generic parameter has no runtime type identity;
   - `is` on a type parameter requires `T < AnyRef`;
   - variance conversions must be representation-preserving.
3. r[types.generic.specialized] Pack functions and calls with `reified` parameters are specialized.
4. r[types.generic.interfaces] Package interfaces therefore carry the bodies of generic and pack functions needed by downstream compilation.

See also: [Implementation Model](#implementation-model-non-normative), which
describes the reference strategy.

### Reified Parameters

1. r[types.reified.metadata] A parameter marked `reified` carries runtime type metadata. It may be used by operations such as `shape[T]()` or passed to another reified operation.
2. r[types.reified.erased] An erased parameter must not be used where runtime type identity is required.
3. r[types.reified.inspectable] Runtime type identity for `Inspectable` comes from a bound instead: the evidence for `T < Inspectable` carries the runtime identity of `T`.
4. r[types.reified.inspectable.uses] Erasing a value of a type parameter to `Inspectable`, `TypeId::of[T]()`, and the `downcast` target need that bound, and `reified` alone permits none of them.
5. r[types.reified.abi] Reification is part of the function's public type and ABI, but its descriptor is not a source-level value argument.
6. r[types.reified.specialize] A backend may specialize a reified call only when doing so preserves observable reflection behavior.

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity).

### Shape Descriptors

1. r[types.shape.consumes] The prelude intrinsic `shape[T]()` consumes the reification descriptor.
2. r[types.shape.result] For a data type or enum, `shape[T]()` returns the corresponding specialized shape type.
3. r[types.shape.other] For any other type, including a reified type parameter, `shape[T]()` returns `TypeShape`.
4. r[types.shape.erased] An erased generic parameter cannot be passed as its type argument.
5. r[types.shape.members] Field and variant shapes are selected from the specialized result, and `shape_of(f)` reflects a function declaration.

See also: [Shape Intrinsics](14-annotations.md#shape-intrinsics).

### Identity On Type Parameters

1. r[types.generic.identity] Identity comparison `is` on a type parameter is permitted only with the sealed `T < AnyRef` bound. Without it, the comparison is an error. Error: `identity-needs-reference-bound`.
2. r[types.generic.identity.primitive] An unconstrained type parameter may be primitive after substitution and therefore cannot be used with `is`.

### Type Packs

1. r[types.pack.declare] An identifier followed by `...` in a generic parameter list declares a type pack.
2. r[types.pack.compile-time] Packs have a compile-time length and ordered element types; they are not runtime collection values.

See also: [Variadic Generics](12-variadic-generics.md), which specifies
expansion and inference.

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

1. r[types.variance.markers] Generic type declarations mark covariance with `+T`, contravariance with `-T`, and invariance by leaving `T` unmarked.
2. r[types.variance.verified] The compiler verifies each declared parameter against its use on the type's readonly public surface.
3. r[types.variance.surface] That surface includes data fields, enum shared data and variant payloads, trait method signatures, and every inherent method available with the nominal type.
4. r[types.variance.trait-impl] A separate trait implementation does not alter the nominal type declaration's variance; its own instantiated signatures must still type-check.
5. r[types.variance.trait-params] A trait's generic parameters are invariant. A variance marker on one is an error. Error: `invalid-variance`.

### GADT Results

1. r[types.variance.gadt] Each declaration parameter whose argument position in an explicit GADT variant result is not exactly that parameter is invariant.
2. r[types.variance.gadt.example] For example, `IsMutUser -> Witness[mut User]` makes `T` invariant in `Witness[T]`, so declaring `Witness[+T]` is rejected.

> **Why.** This prevents a variance conversion from making an arm-local GADT
> equality upgrade a readonly value or reinterpret a value's runtime
> representation.

See also: [Generalized Algebraic Data Types](13-gadts.md).

### Polarity

r[types.polarity] Polarity is computed as follows:

1. r[types.polarity.positive] A returned value and an ordinary readonly field are positive positions.
2. r[types.polarity.negative] A function or method parameter is a negative position.
3. r[types.polarity.function] Entering a function parameter reverses polarity, while entering a function result preserves it.
4. r[types.polarity.generic] Applying a covariant generic argument preserves polarity, a contravariant argument reverses it, and an invariant argument makes the occurrence invariant.
5. r[types.polarity.mut] Occurrence beneath `mut` is invariant.
6. r[types.polarity.embedded] Occurrence in an embedded field's type is invariant.
7. r[types.polarity.both] A parameter used in both positive and negative positions must be invariant.

> **Why.** The referenced storage beneath `mut` can be both read and
> written. Access through an embedded field follows its container.

1. r[types.variance.covariant-check] A declared `+T` is rejected if any occurrence is negative or invariant. Error: `invalid-variance`.
2. r[types.variance.contravariant-check] A declared `-T` is rejected if any occurrence is positive or invariant. Error: `invalid-variance`.
3. r[types.variance.unmarked] An unmarked invariant parameter may occur in any position.

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

### Readonly Outer Views

1. r[types.variance.readonly-only] Variance conversion applies only to a readonly outer view.
2. r[types.variance.mut-invariant] Every `mut G[T]` view is invariant in all generic arguments.
3. r[types.variance.list] The built-in `List` declares a covariant element parameter for its readonly view.
4. r[types.variance.map] Readonly `Map[K, V]` is invariant in `K` and covariant in `V`.
5. r[types.variance.function] The function type constructors `Fn` and `SuspendFn` are contravariant in each input element, covariant in the output, and invariant in the requirement row.

> **Why.** The mutable view may replace stored values. Map keys are both
> accepted for lookup and exposed during traversal. Converting a function's
> row would change which providers the call passes, so the row stays
> invariant under variance. A function value itself may still widen its row
> by [row subsumption](11-requirements-and-suspension.md#row-subsumption),
> which may adapt the value.

### Representation-Preserving Variance

1. r[types.variance.repr.covariant] A variance conversion `G[S] -> G[T]` requires a representation-preserving `S -> T` conversion for each covariant argument.
2. r[types.variance.repr.contravariant] A variance conversion `G[S] -> G[T]` requires a representation-preserving `T -> S` conversion for each contravariant argument.
3. r[types.variance.repr.weakening] Permission weakening `mut U -> U` is representation-preserving.
4. r[types.variance.repr.excluded] These conversions are not representation-preserving:
   - numeric widening such as `i8 -> i64`;
   - construction of a trait value such as `i32 -> Display` or `User -> Display`;
   - child-dynamic-trait to supertrait widening;
   - optional injection `U -> U?`.
5. r[types.variance.no-insertion] Variance never inserts element wrappers, metadata rewrapping, boxing, copies, or per-access conversions. A variance conversion that would need one is an error. Error: `variance-representation-change`.

See also: [Representation-Preserving Conversions](#representation-preserving-conversions).

## Trait Values And `Any`

This section defines trait conformance, dynamic trait values, and the `Any`
trait family.

1. r[types.trait.explicit] Trait conformance is explicit.
2. r[types.trait.no-shape] Matching method shape alone does not make a type implement a trait.
3. r[types.trait.static] A generic bound such as `T < Display` uses static dispatch and preserves the concrete type.

### Trait Value Types

1. r[types.trait.value] Using a trait name as a value type creates a Go-style dynamic trait value.
2. r[types.trait.value.contents] A dynamic trait value contains a concrete value and dispatch metadata for that trait.
3. r[types.trait.value.no-dyn] Source syntax does not use a `dyn` marker.
4. r[types.trait.value.bound] A dynamic trait value type satisfies a generic bound on its own trait and on each of that trait's supertraits. For example, a `Display` value is a valid argument for `T < Display`.
5. r[types.trait.value.bindings] A trait value type may bind the trait's associated types, as in `Supplier[Item = i32]`, as [Bound Associated Types](09-traits.md#bound-associated-types) specifies.
6. r[types.trait.value.not-target] A dynamic trait value type is still not an implementation target.

See also: [Dynamic Trait Values](09-traits.md#dynamic-trait-values), which
gives the rule.

### Dynamic Safety

1. r[types.trait.safe] Only a dynamically safe trait may be used as a value type. Error: `trait-not-dynamically-safe`.
2. r[types.trait.safe.one-copy] Dynamic safety is defined by the one-copy rule, [`trait.dyn.safe.one-copy`](09-traits.md#r-trait.dyn.safe.one-copy); the rules below restate its consequences.
3. r[types.trait.safe.members-bound] A dynamically safe trait and every supertrait must have no associated functions, and `Self` may appear only as the receiver type.
4. r[types.trait.safe.assoc-bound] The trait value type must bind each associated type of the trait and its supertraits, as in `Supplier[Item = i32]`.
5. r[types.trait.safe.method-type-param] A method-level type parameter is permitted only when it is bounded by `AnyRef`; further bounds such as `T < AnyRef & Display` are allowed.
6. r[types.trait.safe.no-reified-or-pack] A method must not declare a `reified` parameter or a type or value pack. Row parameters and suspending methods are allowed.
7. r[types.trait.safe.one-body] Every argument for such a parameter is a reference, so one method body serves every instantiation, and the further bounds are supplied with each call.
8. r[types.trait.safe.convert-value] A caller converts a primitive or tuple value explicitly before passing it.
9. r[types.trait.safe.trait-generic] Trait declaration generic parameters are permitted.
10. r[types.trait.safe.static] Traits that fail these rules remain valid for static generic bounds and explicit implementations.

> **Why.** One concrete trait instantiation, such as `Repository[User]`, fixes
> the trait declaration's generic parameters before dispatch.

### Supertrait Widening

1. r[types.trait.child] A dynamic child-trait value exposes methods declared by the child and all of its transitive supertraits.
2. r[types.trait.child.widen] A dynamic child-trait value may be widened implicitly to a dynamic supertrait value.
3. r[types.trait.child.one-way] That conversion discards access to child-only methods and cannot be reversed by a conversion; only `Inspectable` values recover a concrete type.
4. r[types.trait.child.repr] This direct widening may rewrap dispatch metadata and is therefore not representation-preserving for a variance conversion.

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity).

### `Any`

1. r[types.any] `Any` is the built-in universal empty trait.
2. r[types.any.all] Every value type, including an optional type, satisfies `Any` automatically.
3. r[types.any.erase] As a value type, `Any` erases the concrete type.
4. r[types.any.mut] `mut Trait` and `mut Any` preserve mutable access to an erased composite root.
5. r[types.any.one-way] `Any` erasure is one-way.
6. r[types.any.inspectable] A value erased to the sealed trait `std.inspect.Inspectable` instead keeps a runtime record of its concrete type, which `downcast` compares exactly.

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity).

### `AnyVal` And `AnyRef`

1. r[types.sealed.decl] `AnyVal` and `AnyRef` are the two sealed subtraits of `Any`, declared as `trait AnyVal < Any` and `trait AnyRef < Any`.
2. r[types.sealed.markers] Both are empty marker traits that the compiler implements.
3. r[types.sealed.no-impl] An explicit implementation of either is an error. Error: `sealed-trait-implementation`.
4. r[types.sealed.exactly-one] Every value type implements exactly one of them:

| Trait | Types | Identity |
| --- | --- | --- |
| r[types.sealed.anyval-types] `AnyVal` | `bool`, `char`, the integer types, `f32`, `f64`, `string`, `void`, and tuples | These values have no identity. |
| r[types.sealed.anyref] `AnyRef` | data types, enums (including optionals and `Result`), `List`, `Map`, function types, dynamic trait value types, `Any`, suspensions, and runtime handles | These values have identity. |

5. r[types.sealed.newtype] A newtype has its base type's category: `type Mile(i32)` implements `AnyVal`, and `type Owner(User)` implements `AnyRef`.
6. r[types.sealed.never] `never` implements neither, because it has no values.
7. r[types.sealed.permission] Access permission does not change the category, so `mut User` implements `AnyRef`.
8. r[types.sealed.type-parameter] A type parameter implements `AnyVal` or `AnyRef` only through its bound.
9. r[types.sealed.bounds] `T < AnyVal` accepts only `AnyVal` types, and `T < AnyRef` accepts only `AnyRef` types.

```text
data Handle: pass

impl AnyRef for Handle  # error: sealed-trait-implementation
impl AnyVal for Handle  # error: sealed-trait-implementation
```

## Map Key Types

This section defines which types may be map keys, and how keys behave.

1. r[types.map-key.bound] `Map[K, V]` requires `K < Eq & Hash` and rejects a `mut T` key type. Any other key type is an error. Error: `invalid-map-key`.
2. r[types.map-key.hash] `Hash` is a standard-library trait in `std.hash`.
3. r[types.map-key.user] User-defined data and enum types can become keys by explicitly implementing or deriving both traits.
4. r[types.map-key.builtin-types] Standard-library implementations cover eligible built-in scalar types and their supported compositions: `bool`, integers, `char`, `string`, tuples of hashable elements, and optionals of hashable elements.
5. r[types.map-key.no-hash] Lists, maps, floating-point values, functions, suspensions, dynamic trait values, and `Any` do not have built-in `Hash`.
6. r[types.map-key.user-enums] User data and every user enum, including a payload-free one, require an explicit or derived implementation of both traits.
7. r[types.map-key.float-no-hash] Floating-point types implement `Eq` but not `Hash`, so they are not valid map keys.
8. r[types.map-key.no-map-hash] Consequently maps have no built-in hash and impose no order-independent map-hash obligation.

> **Why.** A NaN key is unequal even to itself, so no lookup could find it.

### Lookup And Order

1. r[types.map.eq-hash] Map lookup and duplicate-key replacement use `Eq` for key comparison and `Hash` for indexing.
2. r[types.map.no-law] The language does not check or impose a law connecting these two implementations.
3. r[types.map.inconsistent] If an implementation hashes values differently that `Eq` considers equal, lookup and duplicate-key behavior are not guaranteed.
4. r[types.map.order] Map iteration follows insertion order.
5. r[types.map.replace] Replacing the value for an existing key does not move that entry; removing and later reinserting a key places it at the end.
6. r[types.map.semantics] Hash values remain outside map value semantics, and map equality remains independent of insertion order.

### Mutated Keys

1. r[types.map.key-view] A readonly key view does not freeze the object.
2. r[types.map.no-reindex] If another mutable alias changes a stored key's equality or hash after insertion, the map does not automatically reindex it.
3. r[types.map.ghost] The entry can remain visible during iteration yet be unreachable by lookup or removal with the mutated key: a ghost entry.
4. r[types.map.no-repair] Such mutation does not trigger a compile-time error or an automatic repair.

## Least Common Type

r[types.lct.sites] Several constructs infer one type from several values when no expected type is available:

| Construct | Values | Defined in |
| --- | --- | --- |
| List literal | the elements | [Expressions](05-expressions.md#list-and-map-expressions) |
| Map literal | the keys, and the values | [Expressions](05-expressions.md#list-and-map-expressions) |
| Value-producing `if` | the branches | [Control Flow](06-control-flow.md#conditional-expressions) |
| Value-producing `match` | the arm results | [Control Flow](06-control-flow.md#match-expressions) |
| Closure whose result type is inferred | the final value and `return` operands | [Functions](07-functions.md#closures) |
| Non-public function whose result type is omitted | the final value and `return` operands | [Functions](07-functions.md#declarations) |

1. r[types.lct.uses] Each of these uses the least common type defined here.
2. r[types.lct.unique] The compiler computes a unique least common type of the values' types using only the implicit conversions in [Assignability And Coercion](#assignability-and-coercion).
3. r[types.lct.contributors] Numeric widening, permission weakening, and declared readonly variance may contribute.
4. r[types.lct.no-combine] Least-common-type inference never combines permission weakening with a variance step for the same candidate conversion.
5. r[types.lct.row-union-every-site] At every construct in the table, function values with different rows are first widened to the union of their rows ([Row Union In Literals](11-requirements-and-suspension.md#row-union-in-literals)).

### No Implicit Erasure

1. r[types.lct.no-any] The compiler never falls back to `Any` merely to make heterogeneous values type-check.
2. r[types.lct.no-trait-value] Unconstrained inference also does not introduce a dynamic trait-value conversion.
3. r[types.lct.no-supertrait-widening] It also never widens a dynamic trait value to a supertrait value, so values of two child traits of one supertrait have no common type.
4. r[types.lct.expected-trait] An expected type such as `List[Display]` or `Map[K, Display]` may request that conversion explicitly.
5. r[types.lct.contextual] A contextual variant, `.None` included, takes its type only from an expected type, never from the other values.
6. r[types.lct.contextual.error] `[1, .None]` or `if c: 1 else: .None` without an expected type is an error. Error: `missing-contextual-enum-type`.
7. r[types.lct.contextual.expected] `let values: List[i32?] = [1, .None]` supplies the type.

```text
fn pick(flag: bool) -> void:
    values := [1, .None]             # error: missing-contextual-enum-type
    value := if flag: 1 else: .None  # error: missing-contextual-enum-type
```

> **Why.** Unconstrained inference introduces no trait-value conversion because
> a concrete type may satisfy multiple unrelated traits.

### Inference Failure

1. r[types.lct.fail] If no unique least type exists, inference fails and the user must add an expected type.
2. r[types.lct.no-common] When the values have no common type, the failure is an error. Error: `no-common-type`.
3. r[types.lct.no-least] When they have common types but these rules admit no unique least one, the failure is an error. Error: `no-least-common-type`.

```text
fn combine(small: mut List[mut User], wide: List[User]) -> void:
    both := [small, wide]  # error: no-least-common-type

fn mix(shown: Shown, tagged: Tagged) -> void:
    items := [shown, tagged]  # error: no-common-type
```

In the second function, `Shown` and `Tagged` are traits that both extend
`Named`; `let items: List[Named] = [shown, tagged]` is valid.

## Type Inference Boundaries

r[types.infer.sites] The compiler infers the following:

- local binding types;
- closure parameter or result types when an expected function type supplies them;
- generic call arguments when the solution is unambiguous.

r[types.infer.explicit] The following declarations require explicit types:

1. r[types.infer.explicit.parameters] Named function parameters.
2. r[types.infer.explicit.results-declared] The results of public functions, public inherent methods, trait methods, and methods of trait implementations, and of at least one function in each recursive cycle.
3. r[types.infer.explicit.fields] Public and private data fields.
4. r[types.infer.explicit.enum] Enum payload fields and constructor data.
5. r[types.infer.explicit.trait-methods] Trait method parameters and results.
6. r[types.infer.explicit.fn-type] Named function type parameters and bounds where applicable.

r[types.infer.body-result-private] Any other named function, inherent method, or local `fn` may infer its result from its body, as [Parameter And Result Types](07-functions.md#parameter-and-result-types) states.

r[types.infer.named-fn] For a named function, inference covers only its result type and its requirement row. Its parameter types, generic parameters, and bounds are always written in its declaration.

### Ambiguous Inference

1. r[types.infer.no-overload] Inference must not select among overloaded functions.
2. r[types.infer.ambiguous] If inference has multiple valid solutions, compilation fails and the diagnostic must identify an annotation site that disambiguates the program.

> **Why.** hd-lang has no function overloading.

## Implementation Model (Non-Normative)

This section is non-normative. It describes the reference strategy that the
normative rules above are designed to allow, so that implementers and readers
can predict costs. An implementation may choose any other representation
that preserves the observable semantics.

### Value Categories

Runtime values fall into three categories:

| Category | Types | Identity |
| --- | --- | --- |
| Scalar values | `bool`, `char`, the integer types, `f32`, `f64` | none |
| Identity-free composites | `string`, tuples | none |
| Reference values | data values, stored enum values (including `Result` and optionals), lists, maps, closures, trait values, `Any`, suspensions, runtime handles | allocation identity, or one canonical identity for values that store no data |

The reference values are exactly the implementers of the sealed `AnyRef`
trait. The scalar values and identity-free composites are exactly the
implementers of the sealed `AnyVal` trait.

Values without identity are immutable, so storing one by copy or by
reference cannot be observed. A payload-free enum value and a fieldless data
value store no data and have one canonical identity each.

Converting a value without identity to a trait value or `Any` allocates a box
with its own identity, as
[Expressions](05-expressions.md#unary-and-binary-operators) specifies.

See also: [Trait Values And `Any`](#trait-values-and-any).

### Shapes and Generic Code

A **shape** is the machine representation a value occupies in generic code.
The reference strategy uses five shapes:

| Shape | Types |
| --- | --- |
| `i32` | `bool`, `char`, `i8`, `i16`, `i32`, `u8`, `u16`, `u32` |
| `i64` | `i64`, `u64` |
| `f32` | `f32` |
| `f64` | `f64` |
| reference | every other type, including strings, tuples, optionals, data, enums, collections, closures, and trait values |

A generic function is compiled in its defining package once for each shape,
at most five bodies. Packages ship their sources, and package interfaces
carry generic and pack function bodies, as
[`types.generic.interfaces`](#r-types.generic.interfaces) requires, so a
downstream package may also use a carried body to specialize an
instantiation. All reference-shaped instantiations share one body. Scalar-shaped instantiations
get a specialized body, so a generic function over `List[i32]` reads and
writes unboxed `i32` elements.

Trait bounds are passed as dictionaries of the selected operations;
associated types are represented through those dictionaries. A dictionary for
a statically known implementation is a constant, not a per-call allocation.

When the set of shapes reachable from one generic function is unbounded, the
implementation falls back to the reference shape with boxed scalars. An
example is polymorphic recursion such as `f[T]` calling `f[(T, T)]`. This is
unobservable, because values without identity cannot be distinguished by
storage.

A method called through a trait value has exactly one body at run time, as
the one-copy rule of [Dynamic Safety](09-traits.md#dynamic-safety) requires.
The dynamic-safety rule in [Trait Values And `Any`](#trait-values-and-any)
therefore limits method-level type parameters of dynamically safe traits to
reference types, which all share the reference shape, and excludes `reified`
parameters and packs. A row parameter passes its providers as one bundle, so
it keeps one body.

See also: [Name Resolution Across Packages](10-modules.md#name-resolution-across-packages).

### Composite Representation

- A data type is a record of its fields. Scalar fields are stored unboxed.
- Every enum uses the reference shape, with one representation. A
  payload-free variant, in any enum, is an `i31ref` holding its tag: it
  allocates nothing, and `ref.eq` on it is its canonical identity. A variant
  with a payload is a GC struct, one struct subtype per variant of the
  enum's base type. An enum-typed slot is an `eqref`, and `match` tests for
  `i31` first, then reads the struct's tag. There is no separate `i32` form.
- Shared constructor data is a per-variant constant, stored once in a table
  indexed by the tag and never in an enum value. A payload-free variant
  therefore stays an `i31ref` tag even when its enum declares shared data.
- A tuple is an immutable record typed by its element shapes. In locals,
  parameters, and results, it may be split into its elements.
- `T?` is an enum like any other: `.None` is a canonical constant, which
  the `i31ref` tag or a null reference may represent. `.Some(value)` is a
  tagged record holding the value, unboxed for a scalar `T`. Because each `.Some` construction has
  its own identity, a present value cannot be represented by the payload
  itself.
- A list is a growable array of its element shape. A map is expected to use
  hashing, with insertion order kept separately.
- A closure is a function reference plus an environment record. A closure
  without captures needs no environment. Because function identity is
  unspecified, an implementation may share one value for a named function or
  allocate one at each use.
- A dynamic trait value is the underlying reference plus a shared method
  table for the implementation. For `Inspectable` and the traits that
  extend it, the table also holds one interned descriptor of the recorded
  type. A `downcast` is then a descriptor comparison followed by a cast or an
  unboxing, and a `T < Inspectable` dictionary is that descriptor.

### Suspension Frames

A suspending function is compiled to a frame record, a state number, and a
poll function. The frame holds the arguments, construction-time providers,
the active child suspension, and the locals that are live across suspension
points. Frame contents are not observable, so an implementation may keep
only live values.

The frame representation as a whole is not observable either. When a
suspension frame is statically known, as for a direct `fn!` call, an
implementation may skip the uniform `Suspend[T]` wrapper and the boxing of
the frame's result.

### Requirement Rows

How providers for a requirement row are passed is not observable. An
implementation may pass them positionally, ordered by a canonical order of
the row's keys, instead of through a keyed lookup.

### Representation-Preserving Conversions

A conversion is representation-preserving when the value after conversion is
the same runtime value as before. It keeps the same identity and the same
stored content, with no wrapper, copy, box, or re-encoding.

Permission weakening `mut U -> U` preserves representation. Numeric widening,
conversion to a trait value or `Any`, supertrait widening of a dynamic value,
and optional injection do not, which is why [Variance](#variance) excludes
them.

## Unsupported Type-System Extensions

This section lists type-system features that hd-lang does not have.

1. r[types.unsupported.type-test] The only runtime type test is exact-type recovery from an `Inspectable` value.
2. r[types.unsupported.no-assertions] There are no trait-to-trait assertions, no tests on `Any` or on trait values whose trait does not extend `Inspectable`, and no type patterns in `match`.
3. r[types.unsupported.no-unions] hd-lang has no anonymous union types, including error unions such as `FsError | HttpError`.
4. r[types.unsupported.error-type] An error type is a nominal type or a dynamic trait value such as `std.error.Error`.
5. r[types.unsupported.panic-abi] The exact host representation of a checked runtime panic is an ABI concern; its language-level control-flow semantics are defined in [Control Flow](06-control-flow.md#runtime-panics).

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity),
[Error Trait](09-traits.md#error-trait).
