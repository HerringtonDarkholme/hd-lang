# Type System

Status: language specification draft.

This chapter defines types, assignability, value types and writable places,
generics, and variance.

1. r[types.static] hd-lang is statically typed: every expression has a compile-time type.
2. r[types.no-execute] A program with a type error must not execute.
3. r[types.local-inference] Local inference removes redundant annotations; public and aggregate boundaries remain explicit.

```hd
fn demo(x: i32) -> i32:
    x + 1   # every expression has a compile-time type
```

See also: [Type Inference Boundaries](#type-inference-boundaries).

## Type Forms

1. r[types.forms.with-opaque-results] The type forms are:

| Form | Spelling |
| --- | --- |
| Primitive types | |
| Nominal struct types and enums | |
| Tuples | `(A, B)`, or ending in a rest element, `(A, List[T]...)` |
| Lists and maps | `List[T]`, `Map[K, V]` |
| Optional types | `T?` |
| Function types | `fn(...) -> T`, sugar for `Fn[(...), T, $()]` |
| Suspending function types | `fn!(...) -> T`, sugar for `SuspendFn[(...), T, $()]` |
| Requirement-bearing function types | ending in `$ Row` |
| Generic instantiations | |
| Associated type projections | such as `T::Item` |
| Trait value types | |
| Transparent aliases and nominal newtypes | |
| Reference types | `*T`, `*mut T`, `&T`, `&mut T` |
| Opaque function result types | `impl Trait[Args]` |

1. r[types.suspend] `Suspend[T]` is the dynamic one-shot computation protocol.

```hd
fn demo() -> (List[i32], string?):
    ([+1, +2], .Some("a"))   # a tuple of a list and an optional
```

See also: [Requirements and Suspension](11-requirements-and-suspension.md).

### The `never` Type

1. r[types.never] `never` is the uninhabited bottom type.
2. r[types.never.assignable] `never` is assignable to every type, and no ordinary value is assignable to it.
3. r[types.never.abrupt] An expression that completes abruptly has type `never` on that control-flow path.
4. r[types.never.abrupt-forms] These expressions complete abruptly: an unconditional `return`, `break`, or `continue`, propagation that exits the current body, and a call to `panic`.
5. r[types.never.forms] Exactly six forms have type `never`: `return`, `break`, `continue`, a call to `panic`, and any other call whose result type is `never`. The sixth is an [infinite loop](06-control-flow.md#r-flow.while.infinite) that no `break` targets.
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
| Unsigned integers | `u8`, `u16`, `u32`, `u64`, `usize` |
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

7. r[types.prim.no-mut.self] A `mut self` receiver in an impl whose `Self` is primitive is valid: it is not a primitive type written with `mut`.

```text
trait Counter:
    fn reset(mut self) -> void

impl Counter for i32:
    fn reset(mut self) -> void: pass  # valid
```

> **Why.** An impl repeats its trait method's signature. A trait with a
> `mut self` method must therefore stay implementable for a primitive
> `Self`.

8. r[types.prim.no-mut.self-type] In such a method the `mut` is dropped, because a primitive has no `mut` form: `self` has the plain type `Self`, as in `i32`.
9. r[types.prim.no-mut.self-call] A call of such a method therefore needs no writable place as its receiver.

```text
trait Step:
    fn next(mut self) -> i32

impl Step for i32:
    fn next(mut self) -> i32: self + 1  # valid: self is i32

fn advance(start: i32) -> i32: start.next()  # valid
```

### The `usize` Type

`usize` is the type of sizes: lengths, indices, counts, and byte offsets:

```text
fn last(items: List[string]) -> string:
    items[items.len() - 1]  # in a debug or test build, panics with integer-overflow when items is empty

fn middle(items: List[i32]) -> usize:
    items.len() / 2
```

1. r[types.usize.primitive] `usize` is a primitive unsigned integer type. It is distinct from `u32` and from every other numeric type.
2. r[types.usize.width] `usize` is an unsigned integer with the target's pointer width. On the Wasm32 target it is 32 bits.
3. r[types.usize.behavior] Its range, overflow, wrapping, casts, and literal range are those of an unsigned integer of that width, so on Wasm32 they are those of `u32`.
4. r[types.usize.prelude] `usize` is a [prelude](10-modules.md#prelude) name, so every module may write it without a `use`.
5. r[types.usize.sizes] Every length, index, count, and byte offset that the language defines has type `usize`.
6. r[types.usize.convert] A `usize` and a `u32` convert only by a written cast, as `usize(limit)` or `u32(size)`, by [`types.num.no-implicit`](#r-types.num.no-implicit). A value of one where the other is expected is an error. Error: `type-mismatch`.

```text
fn take_count(count: u32) -> u32:
    count

fn total(items: List[i32], limit: u32) -> usize:
    limit + items.len()  # error: type-mismatch

fn counted(items: List[i32]) -> u32:
    take_count(items.len())  # error: type-mismatch

fn fixed(items: List[i32], limit: u32) -> usize:
    usize(limit) + items.len()
```

> **Note.** On Wasm32 a `usize` has the representation of a `u32`, a Wasm
> `i32`. That is a fact about representation: the two stay distinct types.

> **Note.** A size is never negative, so `len() - 1` on an empty
> collection is below the range of `usize`. In a debug or test build it
> panics with `integer-overflow` by [`types.arith.checked`](#r-types.arith.checked),
> as in Rust. A release build wraps by [`types.arith.release`](#r-types.arith.release).

> **Why.** A size cannot be negative, so an unsigned type rejects a
> negative index when the program is checked rather than when it runs. A
> size has its own type, as in Rust's `usize` and Go's `int`. So a size
> never mixes silently with a 32-bit value that means something else.

See also: [Numeric Conversions](#numeric-conversions), [Numeric Casts](#numeric-casts).

### `void` And The Empty Tuple

1. r[types.void] `void` is an alias for the empty tuple type `()`, so the two spellings name one type and are interchangeable.
2. r[types.unit] The empty tuple value is `()`.
3. r[types.void.role] A function that produces no useful value returns `()`, and its result type is usually spelled `-> void`.
4. r[types.unit.role] `()` is a tuple value and tuple type.

```text
fn log_start() -> void:
    pass

fn same() -> ():
    log_start()  # valid: void and () are one type
```

> **Note.** The type has one value, `()`. A `void` result, a statement
> result, and `pass` all produce it, as Swift's `typealias Void = ()` does.

### Strings

A `string` is an immutable sequence of bytes that is always valid UTF-8. Its
length and its index count bytes:

```text
fn initial(name: string) -> u8:
    name[0]

fn size(name: string) -> usize:
    name.len()  # 6 for "héllo"
```

1. r[types.string.utf8-bytes] A `string` is a sequence of bytes whose contents cannot be mutated.
2. r[types.string.valid-utf8] Every `string` value is valid UTF-8.
3. r[types.string.literal-utf8] The value of a string literal is the UTF-8 encoding of its Unicode scalar values.
4. r[types.string.concat-bytes] Concatenation joins the bytes of its operands, so its result is valid UTF-8.
5. r[types.string.from-bytes] A conversion from arbitrary bytes to `string` must check that they are valid UTF-8, and must fail when they are not.
6. r[types.string.boundary] A **scalar boundary** of a string is a byte offset from `0` to its length. It does not fall inside the encoding of a scalar value.
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
> `Iterable`. A loop over characters is written out, so no code silently
> treats a string as characters.

See also: [String Indexing](05-expressions.md#string-indexing),
[Ordering](05-expressions.md#ordering),
[Iteration Protocols](06-control-flow.md#iteration-protocols),
[String Methods](10-modules.md#string-methods).

## Literal Types

An unsuffixed numeric literal takes its type inside the expression that
holds it. It comes from an expected type, from the typed parts of that
expression, or else from its own form, as [Open Literal Width](#open-literal-width)
states:

```text
x := 1   # usize
y := -1  # i32
```

### Integer Literals

1. r[types.literal.int-local] An integer literal in any supported radix with no expected type takes its width from its expression, by [`types.literal.local.class.meet`](#r-types.literal.local.class.meet). Otherwise it takes its [default type](#r-types.literal.local.default). Both hold in every value range.
2. r[types.literal.int-no-widen] An integer literal does not automatically choose a wider type.
3. r[types.literal.int-range] When an integer literal has an expected integer type, the compiler checks the literal against that type's range. A literal outside it is an error. Error: `integer-literal-range`.

```text
let small: i8 = 1    # valid
let bad: u8 = 300    # error: integer-literal-range
let ratio: f64 = 1   # error: type-mismatch
```

4. r[types.literal.int-diagnostic] The diagnostic must identify the literal, the target range, and an appropriate wider type when one exists.
5. r[types.literal.int-not-float] An integer literal never takes a floating-point type. An integer literal whose expected type is `f32` or `f64` is an error; write `1.0`. Error: `type-mismatch`.
6. r[types.literal.unary-plus] An expected numeric type passes through unary `+` to a numeric literal, so `let positive: u8 = +1` checks the literal against the `u8` range.

### Negated Integer Literals

1. r[types.literal.negation] Under an expected signed integer type, unary `-` applied directly to an integer literal is range-checked as a unit. The check considers the negated mathematical value.
2. r[types.literal.negation.minimum] This makes `let minimum: i8 = -128` valid even though positive `128` does not fit in `i8`.
3. r[types.literal.negation.unsigned] A negated literal for an unsigned expected type is an error. Error: `type-mismatch`.
4. r[types.literal.negation.below-minimum] Values below the signed minimum remain errors.

```hd
fn demo() -> i8:
    -128   # valid: checked as a unit against i8
```

### Floating-Point Literals

1. r[types.literal.float-local] A floating-point literal with no expected type takes its width from its expression, by [`types.literal.local.class.meet`](#r-types.literal.local.class.meet), or else `f64`. It must be representable as a finite value of the type it takes.
2. r[types.literal.float-expected] When an expected `f32` or `f64` type is available, the literal is converted directly to that type.
3. r[types.literal.float-finite] The converted literal must be representable as a finite value under that type's IEEE 754 rounding rules. Error: `float-literal-range`.

```hd
fn demo() -> f64:
    1.5
```

### Open Literal Width

A literal's width is settled within the one expression that holds it. An
expected type decides it; otherwise the typed parts of the expression do;
otherwise the literal's own form does:

```text
fn biggest[T < Ord](left: T, right: T) -> T:
    match left.cmp(right):
        .Less => right
        _ => left

fn total(price: i64, items: List[string]) -> i64:
    let count = 0                       # usize: nothing else decides it
    let delta = -1                      # i32: a signed literal
    let mixed = [1, -2]                 # List[i32]: a signed member
    let last = biggest(0, items.len())  # 0 is a usize, as items.len() is
    price + 5                           # 5 is an i64, as price is
```

1. r[types.literal.local.expected] An unsuffixed numeric literal with an expected type takes that type, by [Integer Literals](#integer-literals) and [Floating-Point Literals](#floating-point-literals).
2. r[types.literal.local.signed] A **signed literal** is an integer literal written directly after unary `-` or `+`, as in `-1` or `+5`.
3. r[types.literal.local.class] Unsuffixed literals with no expected type that meet one another, directly or through a type not yet known, form one **literal class**.
4. r[types.literal.local.default] A literal class's **default type** is `i32` for integers when a member is a signed literal, and `usize` otherwise. For floating-point literals it is `f64`.
5. r[types.literal.local.class.meet] When a literal class meets a type in one of the forms below, every literal of the class takes that type.
6. r[types.literal.local.class.open] A literal class that meets no type by the end of its statement takes its default type.
7. r[types.literal.local.class.once] A literal class's type is decided once, when it meets a type or takes its default. A statement that fails to check is an error and is not checked again with other widths.
8. r[types.literal.local.statement] Nothing crosses a statement. A binding takes the type of its initializer, and no later statement changes it.
9. r[types.literal.local.block] A block of two or more statements inside an expression is not part of it. A branch, arm, or closure body that is one expression is part of it.

```text
fn take(n: i32) -> i32:
    n

fn run(items: List[string]) -> i32:
    let i = 0               # a usize: a later use does not change it
    if i < items.len():
        return 0
    take(i)                 # error: type-mismatch
```

Two literal classes in one statement take their widths apart:

```hd
fn pick[T](first: T, second: T) -> T:
    first

fn widths(big: i64, small: i32) -> (i64, i32):
    (pick(1, big), pick(2, small))   # 1 is an i64, and 2 is an i32
```

The expression forms in which literals meet typed parts:

| Rule | Form | Example | The width comes from |
| --- | --- | --- | --- |
| r[types.literal.local.form.operand] Operands | an operand of an arithmetic or comparison operator, on either side | `5 + price`, `identity(0) + price` | the other operand |
| r[types.literal.local.form.argument] Call arguments | the arguments of one call | `biggest(0, items.len())` | the parameter types, and the other arguments that solve a type parameter |
| r[types.literal.local.form.receiver] Receivers | the receiver of a method call | `[1, 2].iter().all(fn(n: i32) -> bool: n > 0)` | the typed values the call meets, such as a closure parameter type |
| r[types.literal.local.form.element] Collections | the elements of one list, map, tuple, or struct literal | `[small, 1]` | the typed elements |
| r[types.literal.local.form.arm] Arms | the branches of one `if` and the arms of one `match` | `if c: 1 else: count` | the typed branches |
| r[types.literal.local.form.closure-return] Closure returns | the return paths of a closure with no written result type | `return value` and a final `0` | the typed return paths |
| r[types.literal.local.form.loop-else] Loop values | the `else` value and the `break` values of one `while` or `for` | `break total` and `else: 0` | the typed values |
| r[types.literal.local.form.pipe] Pipes | the value of a pipe and its step | `3 \|> twice` | the step's parameter type |
| r[types.literal.local.form.fact] Typed facts | the arguments of a typed fact | `@range(0, 100, "percent")` on an `i32` field | the annotated field or parameter type |
| r[types.literal.local.form.index] Indexes and ranges | an index, a slice bound, or a range bound | `xs[0]`, `0..items.len()` | `usize`, or the other bound |
| r[types.literal.local.form.interpolation] Interpolations | an expression inside `${...}` | `"${price + 1}"` | the expression's own typed parts |
| r[types.literal.local.form.expected] Expected types | a default argument, a returned value, an assigned value, or a typed field | `fn pay(amount: i64 = 5)` | the expected type |

10. r[types.literal.local.form.closure-return.statements] The return paths of one closure, and the values of one loop, join although they are in different statements of its body. A value that leaves its own statement takes no part.
11. r[types.literal.local.form.function-return.statements] The return paths of a non-public function whose result type is omitted join the same way, across the statements of its body.
12. r[types.literal.local.form.join-open] A literal class that reaches such a join stays open until the join, and takes its default type there when the join meets no type.
13. r[types.literal.local.erased.reference-values] A literal addressed and converted to a trait value reference has its class's default type. So `let x: &dyn Any = &42` refers to a `usize`.
14. r[types.literal.local.instantiation] Two or more instantiations of one generic trait may fit a call only because a literal argument could take several widths. Then the literal keeps its default type. With no instantiation for that type, the call is an error. Error: `type-mismatch`.

```text
use std.ops.Add

struct Money:
    cents: i64

impl Add[i32] for Money:
    type Out = Money
    fn add(self, rhs: i32) -> Money:
        Money { cents: self.cents + i64(rhs) }

impl Add[i64] for Money:
    type Out = Money
    fn add(self, rhs: i64) -> Money:
        Money { cents: self.cents + rhs }

fn charge(price: Money) -> Money:
    let after = price.add(-5)    # valid: the signed literal is an i32
    price.add(5)                 # error: type-mismatch
```

15. r[types.literal.local.hint] When a binding whose type is a literal's default type meets another type, the diagnostic should point at that literal and suggest the fix there.

> **Note.** The suggested fix is a sign for `i32`, as `let total = +0`
> or `{"tea": +3}`, and an annotation for another width, as
> `let total: i64 = 0`.

> **Note.** With no annotation, `let balance = 100` followed by
> `balance = balance - 150` is a `usize` subtraction below zero, which
> panics with `integer-overflow` in a debug or test build. Write `+100` for a value that may go
> negative. The panic report says that the type is a literal's default
> type, by [`flow.panic.report.fallback`](06-control-flow.md#r-flow.panic.report.fallback).

> **Why.** An unannotated integer usually counts or indexes, so `usize`
> lets `let i = 0` meet `len()` with no annotation. A written sign is the
> program's own statement that the value may be negative. Settling each
> literal within its expression keeps every type visible where it is
> written, and an error never blames a distant line.

> **Note.** `let mut i = 0` gets the warning of
> [`types.bind.let-mut-primitive-lint`](#r-types.bind.let-mut-primitive-lint)
> unless `&mut i` is taken: a number is primitive at every width.

> **Note.** A type that only an implementation names is not one the
> literals meet. So `10.halve()`, with only `impl Halve for i64`, is an
> error: write `i64(10).halve()`, or give its binding an annotation.

See also: [Binary Numeric Operators](#binary-numeric-operators),
[Inference From Several Arguments](#inference-from-several-arguments),
[Binding Forms](#binding-forms), [Range Expressions](05-expressions.md#range-expressions),
[For Loops](06-control-flow.md#for-loops),
[Let Patterns](06-control-flow.md#let-patterns), and
[the REPL](../cli/command-line.md#repl).
[Ordering](05-expressions.md#ordering) covers comparisons that a `usize`
default makes always true.

### Duration Literals

A duration literal has the standard `std.time.Duration` type without importing its unit function.

```text
fn delay() -> void:
    _ := 500ms
    _ := 30s
```

1. r[types.literal.duration.fixed] A duration literal has type `std.time.Duration`, independent of its expected type and names in scope.
2. r[types.literal.duration.units] Units `ms`, `s`, `min`, and `h` multiply their integer counts by 1, 1000, 60000, and 3600000 milliseconds, respectively.
3. r[types.literal.duration.range] A duration literal must represent between zero and 9223372036854775807 milliseconds, inclusive. Error: `integer-literal-range`.
4. r[types.literal.duration.call] A duration literal evaluates as the corresponding qualified `std.time` unit function applied to its integer count.
5. r[types.literal.duration.negative] Unary `-` does not apply to a duration literal. Error: `type-mismatch`.

The functions and named constructors remain available; a duration literal is the preferred spelling for a literal duration.

### Suffixed Literals

A primitive suffix fixes a numeric literal's type. A user suffix has the
type that its suffix function returns:

```text
use std.ops.num_suffix

struct Millis:
    count: i64

@num_suffix
fn ticks(count: i64) -> Millis:
    Millis { count: count }

fn delay() -> Millis:
    250ticks  # ticks(250): Millis
```

1. r[types.literal.user-call-result] A user-suffixed literal or prefixed string has the result type of its literal function.
2. r[types.literal.suffixed.in] The numeric literal is checked with the suffix function's parameter type as its expected type, by the rules above. So a suffix whose parameter is `i8` range-checks `300b`.
3. r[types.literal.primitive.fixed] A primitive-suffixed literal has the suffix's primitive type, independent of its expected type or literal class.
4. r[types.literal.primitive.kind] An integer literal requires an integer primitive suffix, and a floating-point literal requires a floating-point primitive suffix. Error: `type-mismatch`.
5. r[types.literal.primitive.integer-range] A primitive-suffixed integer literal must fit its suffixed type's range. Error: `integer-literal-range`.
6. r[types.literal.primitive.float-range] A primitive-suffixed floating-point literal must be finite under its suffixed type's rounding rules. Error: `float-literal-range`.
7. r[types.literal.primitive.negation] Unary `-` directly on a primitive-suffixed signed integer literal checks the negated value, so `-128i8` is valid.

```text
use std.ops.num_suffix

@num_suffix
fn secs(count: i64) -> i64: count * 1000

fn wait() -> void:
    half := 1.5secs  # error: type-mismatch
    pass
```

> **Note.** Argument checking rejects a literal of the wrong kind, so
> `1.5s` is invalid when `s` takes an `i64`; write `1500ms`.

> **Note.** `-5s` is the ordinary negation `-(5s)`: unary `-` applies to
> the result of `s(5)`, so a non-primitive result type needs `Neg`. The
> minus is not part of the literal, so `-128b` checks `128` against an
> `i8` parameter and is out of range; write `b(-128)`.

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

1. r[types.literal.prefixed.values] Each interpolated expression is checked with the `T` of the prefix function's `Template[T]` parameter as its expected type, as an argument is. It converts to `T` by the same rules.

> **Note.** So the argument errors apply at each value: a `string` where
> `T` is `i32` is `type-mismatch`, and an `i64` there is
> `implicit-narrowing`. When `T` is the trait value type `Display`, every
> value whose type implements `Display` converts, as
> [Dynamic Trait Values](09-traits.md#dynamic-trait-values) specifies.

See also: [Prefixed Strings](05-expressions.md#prefixed-strings).

### Other Literals

1. r[types.literal.bool] `true` and `false` have type `bool`.
2. r[types.literal.string] A string literal has type `string`, and a character literal has type `char`.
3. r[types.literal.no-absent] There is no literal for an absent optional; it is the enum variant `.None`.

```hd
fn demo() -> (bool, string, char):
    (true, "a", 'b')
```

See also: [Optional Types](#optional-types).

## Nominal And Structural Types

Each `struct` and `enum` declaration introduces a distinct nominal type:

```text
struct UserId:
    value: string

struct PostId:
    value: string
```

`UserId` and `PostId` are distinct even though their field shapes match.

1. r[types.nominal.distinct] Each `struct` and `enum` declaration introduces a distinct nominal type.
2. r[types.nominal.shape] Matching fields or variants do not make two nominal types interchangeable.

### Tuple Types

A tuple is an immutable value without identity, so it implements `AnyVal`,
as [`types.sealed.anyval-members`](#r-types.sealed.anyval-members) states. No
element of it is a place, as
[`expr.place.tuple-element`](05-expressions.md#r-expr.place.tuple-element)
states.

1. r[types.tuple.structural] Tuple types are structural.
2. r[types.tuple.same] Two tuples have the same type when they have the same arity and pairwise-equal element types.
3. r[types.tuple.one-element] One-element tuples require a trailing comma.
4. r[types.tuple.empty] `()` is the empty tuple.

```hd
fn swap(pair: (i32, string)) -> (string, i32):
    (pair._1, pair._0)   # (i32, string) is the same type wherever it is written
```

See also: [`types.value.no-mut.written`](#r-types.value.no-mut.written),
which rejects a tuple type written with `mut`.

#### Rest Elements

A tuple type may end in a **rest element** `List[T]...`, which stands for
any number of trailing `T` values:

```text
fn tail(values: (string, List[i32]...)) -> usize:
    values._1.len()

fn main() -> usize:
    tail(("a", 1, 2, 3))
```

1. r[types.tuple.rest.form] A tuple type may end in one rest element, a type followed by `...`, as in `(i32, i32, List[i32]...)`. Its other elements are its **fixed elements**.
2. r[types.tuple.rest.list] A rest element whose type is not `List[T]`, as in `(i32, i32...)`, is an error. Error: `type-mismatch`.
3. r[types.tuple.rest.value] A value of such a tuple holds its fixed elements, then one `List[T]` that holds the trailing values in order. Selection reads that list like any other element, so `values._1` above is a `List[i32]`.
4. r[types.tuple.rest.same] Two tuple types are the same type only when both or neither end in a rest element, so `(i32, List[i32]...)` and `(i32, List[i32])` differ. With rest elements, the fixed elements and the rest elements must each be pairwise equal.
5. r[types.tuple.rest.assign] No conversion adds, removes, or changes a rest element: a tuple type with a rest element converts only to an identical one. Error: `type-mismatch`.

```text
fn plain(values: (i32, List[i32]...)) -> (i32, List[i32]):
    values  # error: type-mismatch
```

> **Why.** The rest element lets a function's inputs tuple say that it
> takes varargs, so `fn(i32, List[i32]...)` and `fn(i32, List[i32])` are
> different types. Only a `List` tail is allowed: there is no tuple
> concatenation in types.

See also: [Vararg Inputs](07-functions.md#vararg-inputs),
[Tuple Rest Elements](05-expressions.md#tuple-rest-elements).

### Function Type Identity

1. r[types.fn.constructor] A function type is an application of the standard constructor `Fn` or `SuspendFn`, and the `fn(...) -> T` spelling is exact sugar for it.
2. r[types.fn.same] Two function types are the same type when they apply the same constructor to the same inputs. The output and the normalized requirement row must also be the same.
3. r[types.fn.declared-variance] Function types convert only by the declared variance of their constructor, as [Readonly Outer Views](#readonly-outer-views) states.

```hd
fn apply(f: fn(i32) -> i32, x: i32) -> i32:
    f(x)
```

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
6. r[types.alias.target-unknown] A name on an alias's right side that resolves to nothing is an error at the alias declaration, whether or not anything uses the alias. Error: `unknown-type`.
7. r[types.alias.target-unknown.key] A key after `$` on a row alias's right side that resolves to no trait is likewise an error at the alias declaration. Error: `unknown-trait`.

```text
type Left = List[Right]  # error: alias-cycle

type Right = Left?
```

```text
type Owner = Account  # error: unknown-type
type AppRow = $ Ledger  # error: unknown-trait
```

> **Why.** A transparent alias is replaced by its right side, so a cycle
> never ends. A recursive type needs a `struct` or `enum` declaration.

See also: [Lifetime Aliases](13-values-and-references.md#lifetime-aliases).

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
8. r[types.newtype.construct-value] A newtype construction over an `AnyVal` base, such as `Mile(10)`, is a value, as its base is.

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

```hd
fn demo(flag: bool) -> i32?:
    if flag: +1 else: .None
```

### Representation And Propagation

1. r[types.option.repr] The representation of `Option[T]` is an implementation detail; for example, an implementation may represent `.None` as a null reference.
2. r[types.option.propagate] Postfix `?` on an optional expression either produces its contained value or returns `.None` from the nearest function.
3. r[types.option.propagate.result] That function must itself return a compatible optional type.
4. r[types.option.propagate.one-layer] Postfix `?` removes and propagates one optional layer at a time.
5. r[types.option.propagate.permission] A present value keeps the optional's declared contained type `T`, including `*mut U` when `T = *mut U`; unwrapping does not weaken that generic argument.

```hd
fn first(items: List[i32]) -> i32?:
    if items.len() == 0: .None
    else: .Some(items[0])

fn demo(items: List[i32]) -> i32?:
    first(items)?   # the value, or .None returns at once
```

### Optional Identity And Variance

1. r[types.option.ordinary] Optionals follow the ordinary enum rules in every other respect.
2. r[types.option.any.reference-values] A reference to an optional value may be erased to `&dyn Any` like a reference to any other enum value.
3. r[types.option.value] An optional is an enum, so it is a value without identity and implements `AnyVal`, whatever its payload type.
4. r[types.option.identity-error] `is` with an optional operand is an error, as for every enum. Error: `identity-requires-references`.
5. r[types.option.invariant] `Option` declares its parameter unmarked, so an optional is invariant in its contained type, as every unmarked parameter is.
6. r[types.option.invariant.result] `Result[T, E]` likewise declares both parameters unmarked, so it is invariant in both `T` and `E`.

```text
struct User:
    name: string

fn view(user: (*mut User)?) -> (*User)?:
    user  # error: type-mismatch

fn settle(result: Result[*mut User, string]) -> Result[*User, string]:
    result  # error: type-mismatch

struct Maybe[+T]:
    value: T?  # error: invalid-variance
```

> **Why.** The standard methods on optionals and results take `T` as a
> parameter, as `unwrap_or(self, fallback: T)` does. That is a negative
> position on the public surface, so a covariant `T` would fail its own
> polarity check.

See also: [Unary And Binary Operators](05-expressions.md#unary-and-binary-operators),
[Variance](#variance), and the stdlib chapters [Option](../std/option.md)
and [Result](../std/result.md).

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

1. r[types.result.unit-ok] When `T` is `void`, the success value is `()`, so the constructor is written `.Ok(())` or `Result.Ok(())`.
2. r[types.result.no-ok-omit] No shorthand omits that `()`, so `.Ok()` passes no argument for `value`. Error: `argument-count`.
3. r[types.result.no-ok-pass] `.Ok(pass)` is not the source spelling for this case.
4. r[types.result.unit-pattern] A match names that success with the [unit pattern](06-control-flow.md#unit-pattern), as in `.Ok(()) => ...`, so construction and matching read alike.

```text
fn save(ready: bool) -> Result[void, string]:
    if ready:
        return .Ok(())
    .Ok()  # error: argument-count
```

### Result Propagation

1. r[types.result.propagate] Postfix `?` on `Result[T, E]` either produces the success value or immediately returns the error from the nearest function.
2. r[types.result.propagate.enclosing] The enclosing function must return a `Result[U, F]`.
3. r[types.result.propagate.one-step] The error reaches `F` in one step: by one rule of [Assignability And Coercion](#assignability-and-coercion), or otherwise by one call of `F`'s `From[E]` implementation.
4. r[types.result.propagate.not-both] The step is never both, and conversions are never chained.
5. r[types.result.err-value] Returning an `.Err` value without `?` does not itself alter control flow, and it never calls a conversion.
6. r[types.result.propagate.permission] The success value retains its declared generic type `T`, including `*mut U` when `T = *mut U`.
7. r[types.result.no-effects] The core language does not use effect syntax for recoverable errors.

```hd
fn get(items: List[i32]) -> Result[i32, string]:
    if items.len() == 0: .Err("empty")
    else: .Ok(items[0])

fn demo(items: List[i32]) -> Result[i32, string]:
    .Ok(get(items)?)   # the value, or the error returns at once
```

See also: [Propagation](05-expressions.md#propagation), which defines the rule
and its diagnostic, and [Conversion Trait](09-traits.md#conversion-trait).

## Numeric Conversions

No numeric value changes type implicitly. Every change of width is a
written [cast](#numeric-casts), and a literal takes the type it needs:

```text
fn total(small: i16, large: i64) -> i64:
    let wide: i64 = small  # error: type-mismatch
    i64(small) + large + 1
```

The numeric types form three families, each ordered from narrower to wider:

```text
i8, i16, i32, i64
u8, u16, u32, usize, u64
f32, f64
```

1. r[types.num.families] The signed integers, the unsigned integers, and the floating-point types are the three numeric families above, each ordered by width.
2. r[types.num.no-implicit] No numeric type converts implicitly to another numeric type. Every change of numeric type is an explicit cast, as in `i64(small)` or `f64(ratio)`.
3. r[types.num.no-implicit.wider] So a value where a wider type of its family is expected is an error. Examples are an `i16` passed to an `i64` parameter and an `f32` assigned to an `f64`. Error: `type-mismatch`.
4. r[types.num.no-implicit.fix] That diagnostic should offer a fix-it that writes the conversion around the value, as in `i64(small)`.
5. r[types.num.narrowing] A value where a narrower type of its family is expected is an error. Error: `implicit-narrowing`.
6. r[types.num.usize-width] `usize` is the unsigned type of the target's width, by [`types.usize.width`](#r-types.usize.width). On Wasm32 it is wider than `u16`, narrower than `u64`, and as wide as `u32`.
7. r[types.num.same-width] A value where another type of its family with the same width is expected is an error, as a `u32` passed to a `usize` parameter. Error: `type-mismatch`.
8. r[types.num.no-sign-change] There is no implicit conversion between signed and unsigned integers.
9. r[types.num.no-int-float] There is no implicit integer-to-floating or floating-to-integer conversion in the current core.
10. r[types.num.literal-kind] A numeric literal is not a conversion: it takes an expected type of its own kind, as [Literal Types](#literal-types) states. So `let x: i64 = 300` is valid, and `let y: f64 = 1` is an error. Error: `type-mismatch`.

> **Why.** Go, Rust, and Swift have no implicit numeric conversion, and
> Kotlin's mixed operators are one overload per pair of types. With none, an
> operator is one trait method on one type, and every width change is
> visible where it happens.

### Binary Numeric Operators

1. r[types.num.binary.literal-join] In a binary arithmetic or comparison expression, an operand built only of unsuffixed literals takes the other operand's type, on either side, by [`types.literal.local.form.operand`](#r-types.literal.local.form.operand).
2. r[types.num.binary.literal-left] So an unsuffixed integer literal on the left takes the right operand's integer type. `0xFFFF_FFFF_FFFF_FFFF - count` with a `u64` `count` is valid, and `1 < count` compares two `u64` values.
3. r[types.num.binary.same-type] Otherwise both operands must have one type, and the result has that type, so `f32 op f32` produces `f32`. Two types of one family are an error, as `small + large` with an `i16` and an `i64`, or an `f32` and an `f64` operand. Error: `type-mismatch`.
4. r[types.num.binary.no-mix] Signed and unsigned integers do not mix implicitly, and integers do not mix implicitly with floating-point values. A signed and an unsigned operand are an error. Error: `mixed-signedness`.
5. r[types.num.binary.cast] The user must cast one operand explicitly in those cases.

```text
fn add(small: i16, large: i64, ratio: f32, scale: f64) -> f64:
    let count: i64 = small + large  # error: type-mismatch
    let mixed: f64 = ratio * scale  # error: type-mismatch
    let next = small + 1            # the literal is an i16
    let rest = 100 - small          # so is this one, on the left
    f64(ratio) * scale
```

### Numeric Casts

Explicit numeric conversion uses constructor-style casts:

```text
let wide: i64 = 9000
narrow := i16(wide)
```

1. r[types.cast.syntax] Explicit numeric conversion uses constructor-style casts.
2. r[types.cast.wrap] An integer-to-integer cast wraps at run time, as Go conversions and Rust `as` do. The result keeps the low bits of the source value's two's-complement form, read in the target type. It never panics.
3. r[types.cast.wrap.example] So `u8(x)` with `x = 300` gives 44, and `i8(x)` with `x = 200` gives -56.
4. r[types.cast.literal-range] A cast's argument may be an integer literal, alone or under unary `-` or `+`. Then the literal is checked with the target type as its expected type, by [Integer Literals](#integer-literals). An out-of-range literal is an error, as Go reports a constant overflow. Error: `integer-literal-range`.
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

A program is built under one of three **build profiles**: debug, release, or test. The [`hd` command](../cli/command-line.md#build-profiles) selects the profile.

1. r[types.arith.checked] In a debug or test build, an integer overflow or underflow of `+`, `-`, `*`, `**`, or unary `-` panics. Panic: `integer-overflow`.
2. r[types.arith.release] In a release build, those operations wrap to the type's width as two's complement, as in Rust.
3. r[types.arith.failure] In a debug or test build, an invalid shift panics. An explicit wrapping or fallible library operation is the way to ask for other behavior.
4. r[types.arith.always] Division and remainder by zero, `MIN / -1`, an out-of-bounds index, and the rules of an explicit conversion behave the same in every build. So do the `checked_*`, `wrapping_*`, and `saturating_*` library operations.
5. r[types.arith.division] Integer division truncates toward zero.
6. r[types.arith.remainder] Integer remainder has the sign of the dividend.
7. r[types.arith.shift-count] A shift count is invalid when it is negative or not smaller than the bit width of the shifted value.
8. r[types.arith.shift-count.debug] In a debug or test build, an invalid shift count panics. Panic: `invalid-shift`.
9. r[types.arith.shift-count.release] In a release build, an invalid shift count is masked to the bit width of the shifted value. So `x << n` shifts by `n % bits`, as in Rust.
10. r[types.arith.shift-right] Right shift of a signed integer is arithmetic and sign-extending.
11. r[types.arith.min-division] For every signed width, `MIN / -1` panics with `integer-overflow` in every build.
12. r[types.arith.min-remainder] For every signed width, `MIN % -1` produces zero.

```text
let result: i32 = 2147483647 + 1  # panics in a debug or test build, gives -2147483648 in a release build

fn overflow() -> i32:
    let minimum: i32 = -2147483648
    minimum / -1                  # panics in every build
```

> **Why.** This is Rust's model: overflow checks help while a program is
> written and tested, and cost speed once it ships. `hd test` always uses a
> checked build, so a property test finds an overflow even when the
> program ships as a release build.

See also: [Runtime Panics](06-control-flow.md#runtime-panics).

### Numeric Display

1. r[types.display.int] `Display` formats integers in base ten.
2. r[types.display.float] `Display` formats floating values with the shortest round-trip decimal digits. When two shortest strings are equally close to the value, it writes the one with an even last digit.
3. r[types.display.width] The digits round-trip at the value's own width, so an `f32` displays its `f32` digits even when formatted through generic code.
4. r[types.display.notation] Finite floats use fixed notation when the normalized decimal exponent is in `[-6, 21)` and lowercase scientific notation otherwise.
5. r[types.display.exponent] Scientific exponents always include `+` or `-` and no leading zeroes.
6. r[types.display.fixed] Fixed notation always contains a decimal point and at least one fractional digit, so `1.0` remains visibly floating.
7. r[types.display.special] Negative zero is `-0.0`; infinities are `inf` and `-inf`; every NaN is `NaN`.
8. r[types.display.nan-canonical] Before hashing, boundary serialization, or `Display`, every NaN is replaced with the one canonical quiet-NaN value for its width.
9. r[types.display.nan-compare] NaN comparison continues to follow IEEE 754.

```hd
fn demo() -> string:
    "${1.5}"   # "1.5": shortest round-trip digits
```

> **Note.** Boundary serialization writes a float as its bit pattern, after
> this replacement, by [`module.boundary.float-bits`](10-modules.md#r-module.boundary.float-bits).

## Assignability And Coercion

r[types.assign] An expression of type `S` is assignable to a location of type `T` when at least one of these rules applies:

1. r[types.assign.identical] `S` and `T` are identical after expanding transparent aliases.
2. r[types.assign.weaken] `S` is `mut Suspend[U]` and `T` is its readonly form `Suspend[U]`, or `S` is a writable reference and `T` the read-only one of its kind, as `*mut U` and `*U`, by [`ref.convert.weaken`](13-values-and-references.md#r-ref.convert.weaken).
3. r[types.assign.variance] A declared generic variance conversion permits the readonly outer type to change its type arguments.
6. r[types.assign.optional] A value of `T` is injected into `T?`. The injection adds one layer only, so a `T` is not injected into `T??`.
7. r[types.assign.row-subsumption] `S` and `T` are function types, and `T`'s row entails every key of `S`'s row. Then `S` with `T`'s row is assignable to `T`, as [Row Subsumption](11-requirements-and-suspension.md#row-subsumption) states.
8. r[types.assign.never] `S` is `never`, as [`types.never.assignable`](#r-types.never.assignable) states.
9. r[types.assign.trait-value.ref] `T` is a trait value reference, such as `&dyn Trait` or `*mut dyn Trait`, and `S` converts to it by [Trait Value References](09-traits.md#trait-value-references).

```text
trait Counter:
    fn bump(&mut self) -> void

struct Clicks:
    value: i32

impl Counter for Clicks:
    fn bump(&mut self) -> void:
        self.value = self.value + 1

fn edit() -> void:
    let mut clicks = Clicks { value: 0 }
    let edit: &mut dyn Counter = &mut clicks
    edit.bump()
```

No rule converts one numeric type to another, as
[Numeric Conversions](#numeric-conversions) states.

See also: [Inspectable Types](09-traits.md#inspectable-types).

### Assignment And Binding Types

1. r[types.assign.no-subtyping] No inheritance or structural record subtyping exists.
2. r[types.assign.binding-type] Assignment never changes the declared or inferred type of a binding.
3. r[types.assign.let-declared] In particular, later assignment to a `let` binding must remain assignable to the type established at its declaration. A literal initializer's width is fixed there, by [`types.literal.local.statement`](#r-types.literal.local.statement).

```hd
fn demo() -> i32:
    let x = +1
    x = +2   # still i32: assignment never changes the binding's type
    x
```

### Access Permission

A `*mut T` or `&mut T` reference and a `mut Suspend[T]` permit writes. A
`*T`, a `&T`, and a readonly `Suspend[T]` do not, and never gain that
permission.

1. r[types.mut.weaken] A `mut Suspend[T]` may be used as a `Suspend[T]`, a `*mut T` as a `*T`, and a `&mut T` as a `&T`.
2. r[types.mut.no-upgrade] A `Suspend[T]` must never be upgraded to `mut Suspend[T]`, nor a `*T` or `&T` to `*mut T` or `&mut T`. An upgrade is an error. Error: `mutable-upgrade`.
3. r[types.mut.no-upgrade.inference] The error includes an upgrade that generic inference would produce. Given `keep[T](value: T) -> T`, binding `keep(user)` with `user: *User` to a `*mut User` declaration is rejected rather than inferring `T` as `*mut User`.
4. r[types.mut.argument] A readonly `Suspend[T]` argument cannot satisfy a `mut Suspend[T]` parameter. Error: `readonly-argument-to-mutable-parameter`.
5. r[types.mut.argument.infer] Before reporting this error for a generic mutable parameter, inference solves its type variables from the argument's readonly shape.

> **Why.** Solving the generic variables keeps the diagnostic concrete without
> granting mutable access to the readonly argument.

```text
struct User:
    name: string

fn keep[T](value: T) -> T:
    value

fn invalid(user: *User) -> void:
    let mutable: *mut User = user     # error: mutable-upgrade
    let kept: *mut User = keep(user)  # error: mutable-upgrade
```

See also: [Conversions](13-values-and-references.md#conversions).

## Value Types

A **value type** has values without identity: binding a value copies it,
and nothing shares it. A struct type is a value type.

```hd
struct Point:
    x: i32
    y: i32

fn moved(p: Point) -> Point:
    let mut q = p  # q is a copy of p
    q.x += 1       # p is unchanged
    q
```

1. r[types.value.struct] A struct type is a value type. Each struct declaration introduces a distinct nominal type, as an enum declaration does.
2. r[types.value.copy] Binding, assigning, passing, and returning a struct value copies it. A later change to the copy leaves the source unchanged, and the reverse.
3. r[types.value.stored] A struct value stored in a struct field, a list element, a map value, a tuple element, or an enum payload is a copy, and reading it out yields a copy.
4. r[types.value.no-view] A struct type has no readonly or `mut` view, so a struct literal or copy-update has the plain struct type.
5. r[types.value.no-mut.written] A type written with `mut`, as in `mut Point`, `mut List[i32]`, `mut (i32, i32)`, or `mut T` for a type parameter `T`, is an error unless it is a `Suspend[T]`. Error: `mut-on-struct`.
6. r[types.value.field-type] A field read through a struct value has the field's declared type, whatever holds the struct.
7. r[types.value.anyval] A struct type implements `AnyVal`, so `is` with a struct operand is an error. Error: `identity-requires-references`.

```text
struct Point:
    x: i32
    y: i32

fn nudge(p: mut Point) -> void:  # error: mut-on-struct
    pass

fn reset[T](value: mut T) -> void:  # error: mut-on-struct
    pass

fn invalid() -> i32:
    let p: mut (i32, i32) = (1, 2)  # error: mut-on-struct
    p._0

fn same(a: Point, b: Point) -> bool:
    a is b  # error: identity-requires-references
```

> **Note.** A primitive type written with `mut` is the error of
> [`types.prim.no-mut.error`](#r-types.prim.no-mut.error) instead.

> **Why.** A struct value has no identity, so a `mut` view of one would
> name no shared object, and `is` would have nothing to compare.

### Writable Places

A field of a struct value changes in place only through a writable place.

1. r[types.value.writable-places] A place is **writable** when it is a `let mut` binding, local or top-level, a `mut` parameter, or `mut self`. A value reached through a `&mut` reference is writable too. So are a struct field of a writable place, an element of a list in a writable place, and a value of a map in a writable place.
2. r[types.value.let-mut] `let mut name = value` with a struct value is valid, with or without a struct annotation, and makes `name` writable.
3. r[types.value.field-write] Assigning a struct field, with `=` or a compound assignment, requires the struct to be in a writable place.
4. r[types.value.write-once] The assignment changes that place alone, since no other place shares its value.
5. r[types.value.reassign-binding] A plain `let` binding of a struct and a `mut` parameter may be reassigned as a whole. A `:=` binding and a parameter that is not `mut` never change.

A field write through a struct that is not in a writable place is an error,
by what holds the struct:

| Rule | The struct is held by | Error |
| --- | --- | --- |
| r[types.value.write.using-split] Short binding | a `:=`, `using`, or `using!` binding | `non-reassignable-binding` |
| r[types.value.write.plain-param] Parameter | a value parameter or `self` that is not `mut` | `non-reassignable-parameter-binding` |
| r[types.value.write.let] Plain `let` | a `let` binding without `mut` | `not-writable` |
| r[types.value.write.other-holder] Other value | a call result, an element of a list or a value of a map that is no writable place, an unwrapped value, or a pattern binding | `not-writable` |
| r[types.value.write.shared-ref] Read-only reference | a value reached through a `&T` reference | `not-writable` |

```text
struct Point:
    x: i32
    y: i32

fn shift(p: Point) -> void:
    p.x += 1  # error: non-reassignable-parameter-binding

fn demo() -> void:
    origin := Point { x: 0, y: 0 }
    origin.x = 1  # error: non-reassignable-binding
    let start = origin
    start.x = 2  # error: not-writable
    start = Point { x: 2, y: 0 }  # valid: reassigned as a whole
    let mut cur = start
    cur.x = 3  # valid: cur is writable

fn shifted(mut p: Point) -> Point:
    p.x += 1  # valid: a mut parameter is writable
    p
```

Every other write to a place that is not writable has one code:

6. r[types.path.store.not-writable] A write to a place that is not writable is an error. A write is an assignment or compound assignment, an element store, or a call of a `&mut self` method on the place. A field write takes the code that the table above gives. A call of a `mut self` method on a readonly `Suspend[T]` is the same error. Error: `not-writable`.

```text
struct Child:
    name: string

impl Child:
    fn rename(&mut self, name: string) -> void:
        self.name = name

struct Parent:
    child: *Child

fn invalid(parent: &mut Parent, child: &Child) -> void:
    parent.child.name = "new"   # error: not-writable
    child.name = "new"          # error: not-writable
    parent.child.rename("new")  # error: not-writable
```

> **Note.** A struct's `mut self` method takes its own copy of the
> receiver, by [`fn.method.mut-self-struct`](07-functions.md#r-fn.method.mut-self-struct),
> so a call of one needs no writable place.

See also: [Struct Declarations](08-data-and-enums.md#struct-declarations),
[Places](05-expressions.md#places),
[Writing Through A Reference](13-values-and-references.md#writing-through-a-reference).

### Binding Forms

A binding's form decides whether its name may be rebound and whether its
value may change in place.

1. r[types.view.non-reassignable] A binding is **non-reassignable** when its name cannot be rebound, as a `:=` binding and a parameter that is not `mut` cannot.
2. r[types.bind.let-rebind] A `let` binding without `mut` may still be reassigned as a whole; only a change in place through it is rejected.

```text
fn rename(other: User) -> void:
    let user = User { name: "Ada" }  # User
    user = other                     # valid: reassignment
```

```text
fn invalid() -> void:
    let user = User { name: "Ada" }
    user.name = "Grace"  # error: not-writable
```

3. r[types.bind.let-mut-primitive-local] `let mut` on a local whose type is primitive, as in `let mut n = 0`, is valid, since `&mut n` needs a writable place.
4. r[types.bind.let-mut-primitive-lint] When no `&mut` of such a local is taken, its redundant `mut` produces a lint finding with a removal fix-it. Warning: `redundant-let-mut`.
5. r[types.bind.let-mut-primitive-top] `let mut` on a top-level binding whose type is primitive is an error. Error: `mut-on-primitive`.
6. r[types.bind.let-mut-primitive.hint] The diagnostic says that a plain `let` is already reassignable.
7. r[types.bind.let-mut-tuple-valid] `let mut` on a name whose type is a tuple, as in `let mut pair = (1, 2)`, is valid.

```text
fn count_up() -> void:
    let mut count = 0      # warning: redundant-let-mut
    let mut n: i32 = 0     # warning: redundant-let-mut
    let mut pair = (1, 2)  # valid: a tuple
```

8. r[types.bind.let-pattern-mut] A name written `mut` in a `let` pattern is a writable binding, as in a `match` arm; a name without `mut` is not. In `let (mut log, db) = pair`, `log` is writable and `db` is not.
9. r[types.bind.let-pattern-mut.data-field] The same holds in a struct or variant pattern. In `let Profile { mut tags, name } = profile`, `tags` is a writable copy of the field.

```text
struct User:
    name: string

fn pair() -> (User, User):
    (User { name: "Ada" }, User { name: "Grace" })

fn edit() -> void:
    let (mut first, second) = pair()  # first is writable, second is not
    first.name = "Lin"
    second.name = "Kay"  # error: not-writable
```

```text
struct Profile:
    name: string
    tags: List[string]

fn tag(profile: Profile) -> string:
    let Profile { mut tags, name } = profile  # tags is a writable copy
    tags.push(name)
    name
```

> **Note.** No binding form is non-reassignable and mutable at once: a
> local that is changed in place but never reassigned is written with
> `let mut`.

> **Why.** `let mut` says at the declaration which locals change in place.

10. r[types.bind.never] A binding without a type annotation whose initializer has type `never` is an error. Error: `uninhabited-binding`.

```text
fn first_even(values: List[i32]) -> i32:
    found := panic("no even value")  # error: uninhabited-binding
    found
```

> **Why.** No value of type `never` exists, so the binding could never be
> read. The initializer alone, without `found :=`, says the same thing.

## Collection Values

`List[T]` and `Map[K, V]` are values with copy-on-write: a copy shares the
collection's elements until one side changes.

```text
fn tagged(names: List[string]) -> List[string]:
    let mut copy = names  # a copy of names
    copy.push("new")      # names is unchanged
    copy
```

1. r[types.coll.value] `List[T]` and `Map[K, V]` are value types. Binding, assigning, passing, and returning a list or a map copies it, and so does storing one in a field, an element, or a payload. A later change to one copy leaves every other copy unchanged.
2. r[types.coll.copy-on-write] A copy shares its source's elements until one of the two changes. That change first copies the changed side's elements, once.
3. r[types.coll.no-identity] A list or a map has no identity. `List` and `Map` implement `AnyVal`, so `is` with one is an error. Error: `identity-requires-references`.
4. r[types.coll.let-mut] `let mut name = value` with a list or map value is valid, with no annotation or a `List[T]` or `Map[K, V]` one, and makes `name` writable.
5. r[types.coll.writable] Changing a list or a map needs a writable place, as a struct field write does ([`types.value.writable-places`](#r-types.value.writable-places)). A change is a call of a changing method such as `push`, or an element store.
6. r[types.coll.writable.call] A `&mut self` method such as `push`, called on a list or a map that is no writable place, is an error. A `mut self` method needs no writable place, since it changes its own copy ([`fn.method.mut-self-struct`](07-functions.md#r-fn.method.mut-self-struct)). The fix-it makes the binding writable. Error: `not-writable`.
7. r[types.coll.writable.store] An element store into such a list or map, or a field write through its element, is an error with the same fix-it. Error: `not-writable`.
8. r[types.coll.element-place] An element of a list in a writable place is a writable place. So `xs[i] = v`, `xs[i].x = v`, and `xs[i].push(v)` change `xs` in place.
9. r[types.coll.nested] A change through a path, such as `m[k].push(x)`, `team.members.push(x)`, or `xs[i].name = n`, changes the inner value where it is stored. A `mut self` method called on such a path changes only its own copy ([`fn.method.mut-self-struct`](07-functions.md#r-fn.method.mut-self-struct)), so the inner value stays as it was.
10. r[types.coll.shared] A list or a map that two places share is written `*mut List[T]` or `*mut Map[K, V]`, or `&mut List[T]` or `&mut Map[K, V]` when lent to one call. A change through the reference changes the place it refers to.

```text
fn names() -> usize:
    found := ["ada"]
    found.push("grace")  # error: not-writable
    found.len()

fn first() -> i32:
    let scores = [+1, 2]
    scores[0] = 5  # error: not-writable
    scores[0]

fn same(left: List[i32], right: List[i32]) -> bool:
    left is right  # error: identity-requires-references
```

> **Why.** A list that every binding shares lets a callee change its
> caller's list with no mark at the call. A value changes only where its
> place is written, and sharing is written `*mut` or `&mut`. Writing through the path
> keeps building a map of lists linear: each push copies nothing.

### Map Values

A value of a map is a place, as an element of a list is.

1. r[types.coll.map-value] `m[k] = v` and `m[k] op= v` replace a map value. A changing method called on one, such as `m[k].push(x)`, changes it in place.
2. r[types.coll.map-value.place] A value of a map in a writable place is a writable place. So `m[k].x = v` and `m[k].x += 1` change the value in place, as `xs[i].x = v` changes a list element.
3. r[types.coll.map-value.missing] A field write or a changing method call through `m[k]`, when the map holds no equal key, is a checked runtime panic, as a read of `m[k]` is. Panic: `index-out-of-bounds`.
4. r[types.coll.map-value.element-reference] `&m[k]` and `&mut m[k]` are element references, which are local reference values as [Element References](13-values-and-references.md#element-references) states.

```text
struct Stock:
    count: i32

fn restock(shelves: Map[string, Stock]) -> Map[string, Stock]:
    let mut copy = shelves
    copy["apple"].count = 5  # changes the value in place
    copy

fn count(shelves: Map[string, Stock]) -> i32:
    found := &shelves["apple"]
    found.count
```

See also: [Snapshot Iteration](06-control-flow.md#snapshot-iteration),
[Writable Places](#writable-places).

## Generics

This section defines generic parameters, bounds, arguments, and their compile-time and runtime representation.

1. r[types.generic.type-parameter.complete] A type parameter of a generic type, function, method, trait, or implementation denotes a complete type.
2. r[types.generic.naming] By convention, generic parameters, including row parameters, use uppercase names such as `T`, `U`, `K`, `V`, and `R`.
3. r[types.generic.kind] The convention is style only: a parameter's kind comes from its declaration and use, never from the case of its name.

```hd
fn first[T](items: List[T]) -> T?:
    if items.len() == 0: .None
    else: .Some(items[0])
```

### Const Generic Parameters

1. r[types.generic.const.parameter] A const generic parameter denotes one compile-time scalar value of its declared type rather than a type.
2. r[types.generic.const.types] Its declared type is one of `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`, `bool`, `char`, or `string`.
3. r[types.generic.const.type-error] Any other declared type is an error. Error: `generic-kind-mismatch`.
4. r[types.generic.const.positions] Const generic parameters have the same declaration positions and scopes as type parameters.
5. r[types.generic.const.type-position] Inside a generic body, a const parameter may occur alone as a const argument in a type position, as `N` does in `Buffer[T, N]`.
6. r[types.generic.const.no-runtime-value] Declaring a const parameter does not declare a runtime value with that name.
7. r[types.generic.const.no-symbolic-arithmetic] Stage 1 has no symbolic const arithmetic, so a use such as `Buffer[T, N + 1]` is an error. Error: `nonconstant-generic-argument`.

```hd
struct Buffer[T, N < const usize]:
    value: T

fn keep[T, N < const usize](value: Buffer[T, N]) -> Buffer[T, N]:
    value
```

> **Why.** Scalar values have stable equality and need no compile-time heap.
> Restricting a parameter to a bare type argument avoids choosing an
> algebra for symbolic expressions in stage 1.

### Bound Order

A bound may name a later parameter of its list, and two bounds may name
each other:

```text
trait Source[T]:
    fn take(self) -> T

trait Sink[T]:
    fn give(self, value: T) -> void

struct Tank:
    level: i32

struct Pipe:
    width: i32

struct Hose:
    length: i32

impl Source[Pipe] for Tank:
    fn take(self) -> Pipe: Pipe { width: self.level }

impl Source[Hose] for Tank:
    fn take(self) -> Hose: Hose { length: self.level }

impl Sink[Tank] for Pipe:
    fn give(self, value: Tank) -> void: pass

impl Sink[Tank] for Hose:
    fn give(self, value: Tank) -> void: pass

fn wire[S < Source[U], U < Sink[S]](s: S, u: U) -> void:
    u.give(s)

fn connect(tank: Tank, pipe: Pipe, hose: Hose) -> void:
    wire(tank, pipe)
    wire(tank, hose)
```

1. r[types.generic.bound.any-order] In a generic declaration of any kind, a bound may name any parameter of the same list, earlier or later.
2. r[types.generic.bound.mutual] Bounds may name each other, as `S < Source[U]` and `U < Sink[S]` do above. Such mutual references are not a circularity error.
3. r[types.generic.bound.defaults-apart] Defaults keep their own order: they are trailing, as [`types.generic.default.order`](#r-types.generic.default.order) states, and see only earlier parameters, as [`types.generic.default.later`](#r-types.generic.default.later) states.

> **Why.** A bound is a trait constraint, and hd has no subtyping, so
> bounds need no order, as Rust where-clauses need none. An associated
> type allows one pairing per type. A generic trait parameter with mutual
> bounds allows many, as `Tank` pairs with both `Pipe` and `Hose`.

See also: [Function And Closure Scopes](03-names-and-scopes.md#function-and-closure-scopes).

### Generic Arguments

1. r[types.generic.infer] Generic arguments are inferred at call sites when unambiguous.
2. r[types.generic.explicit] Callers may supply the complete generic argument list explicitly.
3. r[types.generic.short-list] An explicit list in an expression may omit trailing slots. Each omitted slot is inferred as a `_` slot is, then defaulted.
4. r[types.generic.placeholder-slot] In a generic function's or method's explicit list, `_` may occupy any slot and requests inference for that argument.
5. r[types.generic.placeholder.not-type] `_` is not itself a type. Its use in an ordinary type application is an error. Error: `syntax-error`.
6. r[types.generic.too-long] A type-argument list with more positional arguments than its declaration has generic parameters is an error, in an expression and in a written type alike. Error: `argument-count`.

```text
fn count(names: List[string, i32]) -> i32:  # error: argument-count
    0

struct Fixed[T, N < const usize]:
    value: T

WIDTH := 2 + 2

fn canonical(value: Fixed[i32, WIDTH]) -> Fixed[i32, 4]:
    value

fn different(value: Fixed[i32, 4]) -> Fixed[i32, 5]:
    value  # error: type-mismatch

fn runtime_value(n: usize, value: Fixed[i32, 4]) -> void:
    let bad: Fixed[i32, n] = value  # error: nonconstant-generic-argument

fn symbolic[N < const usize](value: Fixed[i32, N + 1]) -> void:  # error: nonconstant-generic-argument
    pass
```

### Const Generic Arguments

1. r[types.generic.const.argument] A const generic argument is either a closed const expression or one bare const parameter allowed by [`types.generic.const.type-position`](#r-types.generic.const.type-position).
2. r[types.generic.const.closed] A **closed const expression** is a literal, a named top-level constant, or an allowed scalar operator whose operands are recursively closed const expressions.
3. r[types.generic.const.named] A top-level `:=` binding is a named top-level constant when its initializer is closed. Every referenced binding must recursively qualify too.
4. r[types.generic.const.operators] Allowed operators are built-in primitive unary and binary operators whose operands and result have allowed const types.
5. r[types.generic.const.operator-set] They are integer arithmetic, bitwise operations, shifts, comparisons, boolean logic, and string `+`.
6. r[types.generic.const.short-circuit] Const evaluation of `&&` and `||` short-circuits, so an unevaluated right operand causes no evaluation failure.
7. r[types.generic.const.no-cast-or-trait] Casts and user trait operators are not const operations in stage 1, even when all their inputs are closed.
8. r[types.generic.const.evaluate] The compiler evaluates a closed const argument while checking its use, using the parameter's declared type.
9. r[types.generic.const.evaluate-failure] Overflow, division by zero, or an invalid shift during that evaluation is an error on the argument expression. Error: `const-evaluation-failed`.
10. r[types.generic.const.evaluate-message] A `const-evaluation-failed` diagnostic names the failure as overflow, division by zero, or invalid shift.
11. r[types.generic.const.nonconstant] A runtime-dependent argument, a cast, a user trait operation, or a symbolic expression over a const parameter is an error on that argument. Error: `nonconstant-generic-argument`.
12. r[types.generic.const.nonconstant-message] A `nonconstant-generic-argument` diagnostic says whether the expression depends on runtime data, uses symbolic const arithmetic, or contains an operation unavailable to const evaluation.
13. r[types.generic.const.identity] The evaluated value and its declared type determine const identity. Thus `Fixed[i32, 2 + 2]` equals `Fixed[i32, 4]`, but not `Fixed[i32, 5]`.
14. r[types.generic.const.display] A const argument in a displayed type uses its evaluated canonical literal spelling: decimal integers, `true` or `false`, and escaped character or string literals.
15. r[types.generic.const.infer-argument-type] Call-site inference may solve a const parameter by matching an argument's type against a parameter type that contains the const parameter.
16. r[types.generic.const.no-runtime-inference] A runtime argument value never solves a const parameter, even when its value is a literal.
17. r[types.generic.const.infer-unsolved] A const parameter left unsolved by argument types is an error unless the caller supplies it explicitly. Error: `cannot-infer-type`.
18. r[types.generic.const.impl-exact] Trait and inherent implementation selection matches const arguments by their exact evaluated value and declared type.

For example, `Byte[127i8 + 1i8]`, `Byte[1i8 / 0i8]`, and
`Byte[1i8 << 8i8]` report `const-evaluation-failed` for overflow, division
by zero, and an invalid shift, respectively.

```hd
struct Fixed[N < const usize]:
    value: i32

WIDTH := 2 + 2

fn canonical(value: Fixed[WIDTH]) -> Fixed[4]:
    value
```

> **Why.** Evaluating closed expressions gives `2 + 2` one stable identity
> without admitting general compile-time execution. Argument-type-only
> inference makes a const value visible in the source type that proves it.

### Inference From Several Arguments

When a call solves one type parameter from several arguments, the
arguments' types may differ only in an outer `mut`:

1. r[types.generic.infer.join] When generic call inference solves one type parameter from several arguments, the only conversion between their types is permission weakening. A `mut Suspend[X]` and a `Suspend[X]` meet at `Suspend[X]`.
2. r[types.generic.infer.join.no-trait-value] A trait-value conversion never applies, as [`types.lct.no-trait-value`](#r-types.lct.no-trait-value) states for the least common type. So `cmp(user, label)` with a `User` and a `Display` argument is an error. Error: `no-common-type`.
3. r[types.generic.infer.join.no-supertrait-widening] A supertrait widening never applies either, so two arguments of two child traits of one supertrait are an error. Error: `no-common-type`.
4. r[types.generic.infer.join.other-conflict] Any other conflict between the arguments' types is an error. Examples are `choose(1, true)`, `max(small, large)` with an `i32` and an `i64`, a `*mut User` and a `*User`, and a `T` and a `T?`. Error: `type-mismatch`. The caller writes a cast, as in `max(i64(small), large)`.
5. r[types.generic.infer.join.literal] An integer literal argument is not a conversion: it takes the type solved from the other arguments as its expected type, in any position. So `pick(1, large)` with an `i64` `large` solves `T = i64`.
6. r[types.generic.infer.join.explicit.reference-values] An explicit type argument, as in `cmp::[*dyn Display](&user, label)`, is an expected type for each argument, which then converts by [Assignability And Coercion](#assignability-and-coercion), as [`types.lct.expected-trait.reference-values`](#r-types.lct.expected-trait.reference-values) allows.
7. r[types.generic.infer.join.not-lct] This join is narrower than the [least common type](#least-common-type), and is not one of that section's constructs.

```text
fn max[T < Ord](left: T, right: T) -> T:
    match left.cmp(right):
        .Less => right
        _ => left

fn cmp[T < Display](left: T, right: T) -> bool:
    "$left" == "$right"

fn widest(small: i32, large: i64) -> i64:
    max(small, large)  # error: type-mismatch

fn same(user: User, label: *dyn Display) -> bool:
    cmp(user, label)  # error: no-common-type
```

```text
struct User:
    name: string

fn pick[T](left: T, right: T) -> T:
    left

fn either(large: i64) -> i64:
    pick(1, large)  # T is i64; the literal 1 is an i64

fn widest(small: i32, large: i64) -> i64:
    max(i64(small), large)

fn same(user: User, label: *dyn Display) -> bool:
    cmp::[*dyn Display](box(user), label)
```

> **Why.** A call's arguments are not a list literal. A reader expects `T`
> to be the type written at the call, as Rust and Go do. So a mixed
> numeric call names its cast, and a trait value is asked for by name.

### Inference Through A Bound

A call solves a type parameter that only a bound names from the bounded
argument's implementation of that bound's trait:

```text
trait Source[T]:
    fn take(self) -> T

struct Label:
    text: string

impl Source[string] for Label:
    fn take(self) -> string:
        self.text

fn read[U, S < Source[U]](source: S) -> U:
    source.take()

fn name(label: Label) -> string:
    text := read(label)  # S is Label, then U is string
    text
```

1. r[types.generic.infer.bound] A type parameter of a call is **named only in bounds** when no parameter type names it but a bound of another type parameter does. One case is `U` in `read[U, S < Source[U]](source: S)`.
2. r[types.generic.infer.bound.after] Call inference solves a parameter named only in bounds after the other parameters.
3. r[types.generic.infer.bound.one] Once the bounded parameter is known, if its type implements the bound's trait for exactly one instantiation, that instantiation solves the parameter.
4. r[types.generic.infer.bound.none] If the type implements the bound's trait for no instantiation, the call is an error. Error: `unsatisfied-trait-bound`.
5. r[types.generic.infer.bound.ambiguous] If the type implements the bound's trait for several instantiations, this step leaves the parameter unsolved.
6. r[types.generic.infer.bound.default] A parameter named only in bounds and left unsolved takes its type-argument default, as [`types.generic.default.fill`](#r-types.generic.default.fill) gives, and the bound is then checked.
7. r[types.generic.infer.bound.no-default] A parameter named only in bounds, left unsolved without a default, is an error, and an explicit type argument resolves it.
8. r[types.generic.infer.bound.no-default.ambiguous] When a bound of the parameter allows several instantiations, the error is `ambiguous-type`, by [`types.infer.ambiguous.code`](#r-types.infer.ambiguous.code). Error: `ambiguous-type`.
9. r[types.generic.infer.bound.no-default.disagree] When each bound allows exactly one instantiation and they differ, no solution exists. Error: `cannot-infer-type`.
10. r[types.generic.infer.bound.several-bounds] When several bounds name one parameter named only in bounds, this step solves it only once every parameter those bounds constrain is known.
11. r[types.generic.infer.bound.agree] It then solves the parameter only if each of those bounds allows exactly one instantiation and they all allow the same one.
12. r[types.generic.infer.bound.disagree] Otherwise, when bounds allow different instantiations or one allows several, the parameter is left unsolved, as for one bound.
13. r[types.generic.infer.bound.fixed-point] The step repeats until it solves no further parameter, so a chain of such bounds is solved in dependency order.
14. r[types.generic.infer.bound.precedence] An explicit type argument and an expected type take precedence: this step never solves a parameter that either of them solves.
15. r[types.generic.infer.bound.placeholder] An explicit list may mix written arguments with `_` slots, as in `read::[string, _](both)`; each `_` slot is inferred by the steps above.
16. r[types.generic.infer.bound.call-site] This step is call-site inference only. A declaration's generic parameters and signature are never inferred, as [`types.infer.explicit`](#r-types.infer.explicit) requires.

```text
struct Both:
    count: i32

impl Source[i32] for Both:
    fn take(self) -> i32:
        self.count

impl Source[string] for Both:
    fn take(self) -> string:
        "both"

struct Plain:
    count: i32

fn several(both: Both) -> void:
    value := read(both)  # error: ambiguous-type

fn none(plain: Plain) -> void:
    value := read(plain)  # error: unsatisfied-trait-bound
```

A written argument or an expected type picks one of `Both`'s
instantiations, and a chain of bounds is solved outermost first:

```text
fn label(both: Both) -> string:
    text := read::[string, _](both)  # U is string; S is Both
    text

fn count(both: Both) -> i32:
    let total: i32 = read(both)  # U is i32 from the expected type
    total

fn first_item[T, Inner < Iterable[T], Outer < Iterable[Inner]](groups: Outer) -> T?:
    for group in groups:
        for item in group:
            return .Some(item)
    .None

fn first_name(teams: List[List[string]]) -> string?:
    found := first_item(teams)  # Outer, then Inner is List[string], then T is string
    found
```

A default fills a parameter named only in bounds that several
instantiations leave open. The bound of `S` names the later `U`, so only `U` needs a default:

```text
fn read_or_text[S < Source[U], U = string](source: S) -> U:
    source.take()

fn label_of(both: Both) -> string:
    text := read_or_text(both)  # S is Both; U is open, so U is string
    text

fn label_of_both(both: Both) -> string:
    text := read_or_text::[Both](both)  # the list binds S; U is string
    text
```

Two bounds that name one parameter must agree on it:

```text
struct Note:
    text: string

impl Source[string] for Note:
    fn take(self) -> string:
        self.text

fn joined[U, A < Source[U], B < Source[U]](first: A, second: B) -> List[U]:
    [first.take(), second.take()]

fn titles(label: Label, note: Note) -> List[string]:
    found := joined(label, note)  # both bounds allow only U = string
    found

fn mixed(label: Label, both: Both) -> void:
    items := joined(label, both)  # error: ambiguous-type
```

> **Note.** The std adapter `zip[U, I < Iterable[U]](self, other: I)`
> ([More Adapters](../std/iter.md#more-adapters)) relies on this step.
> With `names: List[string]`, `xs.iter().zip(names)` solves `I` from the
> argument, then `U = string`.

> **Why.** When the argument's type has one implementation, the trait
> argument is a fact of that type, as Rust's associated
> `IntoIterator::Item` is. Writing it at every call adds no information.
> Several implementations are a real choice, so the caller names it,
> unless the declaration names it with a default. Bounds that disagree
> are a choice too, and inference does not pick one.

See also: [Explicit Type Arguments](07-functions.md#explicit-type-arguments),
[Instantiations Of One Generic Trait](09-traits.md#instantiations-of-one-generic-trait).

### Type-Argument Defaults

A generic parameter may declare a **type-argument default**, written with
`=` after its bound. It fills the parameter when a use site leaves it
unsolved or a written type omits it:

```text
struct AppError:
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

1. r[types.generic.default.type-row] A type or row parameter of a function, method, struct, enum, trait, or `type` declaration may declare a default.
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

8. r[types.generic.default.after-inference] At a use site that infers, the use site first solves its parameters as it would without defaults. That holds for a call, a function value, and a struct literal.
9. r[types.generic.default.fill] Then each parameter left unsolved that has a default takes it, in declaration order, with the earlier arguments substituted. The bounds are checked last.
10. r[types.generic.default.argument-wins] A default never replaces a solution, so an argument or expected type that solves the parameter wins: `widen(3)` above has `T = i32`.
11. r[types.generic.default.unsolved] A parameter left unsolved that has no default stays an error. Error: `cannot-infer-type`.
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

> **Why.** Defaults apply only after inference, as in C++ and TypeScript.
> So adding a default never changes a program that compiled without it; it
> only fills a parameter that used to be an error. Rust's inference
> fallback could not keep that property. A default is written at the
> declaration, so declarations stay fully written; only use sites apply it.

See also: [Type-Argument Default Syntax](02-grammar.md#type-argument-default-syntax),
[Explicit Type Arguments](07-functions.md#explicit-type-arguments),
[Method Generic Parameters](09-traits.md#method-generic-parameters).

### Generic Representation

1. r[types.generic.unobservable] The runtime representation of generic code is not observable.
2. r[types.generic.sharing] A program cannot distinguish an implementation that shares one body among instantiations from one that specializes each instantiation, except through these rules of this chapter:
   - a type parameter has runtime type identity only through a `T < Inspectable` bound;
   - `is` on a type parameter requires `T < AnyRef`;
   - variance conversions must be representation-preserving.
3. r[types.generic.interfaces] Package interfaces therefore carry the bodies of generic functions needed by downstream compilation.
4. r[types.generic.instantiation-depth] An implementation may limit how deeply generic instantiations nest, where each instantiation is reached from the one that calls it. The limit is implementation-defined.
5. r[types.generic.instantiation-depth.error] A program whose instantiations exceed that limit is an error, reported on the call that would exceed it. Error: `instantiation-too-deep`.
6. r[types.generic.call-cycle.step] A call passes type parameter `P` of its body to parameter `Q` of its callee when its type argument for `Q` holds `P`.
7. r[types.generic.call-cycle] A **generic call cycle** is a chain of such steps that leads a type parameter back to itself.
8. r[types.generic.call-cycle.grows] A step grows when its type argument is larger than `P`, such as `List[P]`, `(P, P)`, or `P?`.
9. r[types.generic.polymorphic-recursion] A generic call cycle with a step that grows is an error, reported on that step's call. Neither the instantiation limit nor how bodies are shared changes this. Error: `instantiation-too-deep`.
10. r[types.generic.call-cycle.fixed-callee] Only a call whose callee is fixed where it is written makes a step. That is a call of a function, a local function, a method, or a trait method whose implementation the receiver's type selects. A call through a bound or a trait value makes none.
11. r[types.generic.call-cycle.impl-params] Each method of an implementation has its own copy of the implementation's type parameters.
12. r[types.generic.call-cycle.projection] A type argument does not hold a type parameter that occurs only inside a projection, such as `P.Item`.
13. r[types.limit.type-size] An implementation may limit the size of one type, counted in the types it is built from. The limit is implementation-defined.
14. r[types.limit.type-size.error] A type over that limit, whether written or inferred, is an error. Error: `type-too-large`.
15. r[types.generic.const.runtime] A const generic argument contributes to compile-time identity and specialization keys but does not add a runtime parameter by itself.
16. r[types.generic.const.interfaces] Package interfaces preserve const parameter declarations, evaluated const arguments, and generic bodies needed by downstream compilation.

```hd
fn first[T](items: List[T]) -> T?:
    if items.len() == 0: .None
    else: .Some(items[0])   # shared or specialized: unobservable either way
```

```text
fn nest[T](value: T, depth: i32) -> i32:
    if depth == 0: return depth
    nest((value, value), depth - 1)  # error: instantiation-too-deep
```

> **Why.** Each instantiation chain then ends, so an implementation may
> compile generic code per concrete type with no fallback path. The
> check reads only the calls, as Go's instantiation cycle check does, so
> whether a program compiles never depends on how bodies are laid out.
> The fix is a trait value or a non-generic helper.

> **Note.** A chain that grows through a call with no fixed callee, such
> as a call through a bound, still meets the instantiation limit.

See also: [Implementation Model](#implementation-model-non-normative), which
describes the reference strategy.

### Identity On Type Parameters

1. r[types.generic.identity] Identity comparison `is` on a type parameter is permitted only with the sealed `T < AnyRef` bound. Without it, the comparison is an error. Error: `identity-requires-references`.
2. r[types.generic.identity.primitive] An unconstrained type parameter may be a value type after substitution, such as a primitive or an enum, and therefore cannot be used with `is`.

```hd
fn same[T < AnyRef](a: T, b: T) -> bool:
    a is b
```

### Lifetime Parameters

A **lifetime parameter** `'name` is a generic parameter that names a lifetime of local references, not a type:

```text
struct User:
    name: string

fn pick[T, 'a](a: &'a T, b: &'a T) -> &'a T:
    a

fn first['a, T](x: &'a T) -> &'a T:  # warning: lifetime-order (hd lint)
    x

fn name(a: &'b User) -> &string:  # error: unknown-lifetime
    &a.name

fn named['static](a: &User) -> void:  # error: syntax-error
    pass

fn demo(u: &User) -> i32:
    n := +1
    *pick::[i32](&n, &n)  # T is i32; 'a takes no argument
```

1. r[types.lifetime.declare] The generic parameter list of a function, a method, an implementation, a struct, an enum, a trait, or a `type` declaration may declare lifetime parameters beside its type parameters.
2. r[types.lifetime.scope] A lifetime parameter is in scope wherever a type parameter of the same list is, so an implementation's lifetime parameters are in scope in its members.
3. r[types.lifetime.namespace] Lifetimes have names of their own: `'a` never collides with a type, a value, or a type parameter named `a`.
4. r[types.lifetime.unique] A lifetime parameter that reuses the name of a lifetime parameter in scope is an error. Error: `duplicate-binding`.
5. r[types.lifetime.unknown] A lifetime written in a declaration's header or fields must name a lifetime parameter in scope. Error: `unknown-lifetime`.
6. r[types.lifetime.free-order] Lifetime parameters and type parameters may appear in any order in a generic parameter list, and the order has no meaning.
7. r[types.lifetime.arguments.type-const] Explicit generic arguments, as in `pick::[i32, 4](x, y)`, fill type and const parameters, while a lifetime parameter takes no explicit argument.
8. r[types.lifetime.static] `'static` is a reserved lifetime name. A generic list that declares it is an error. Error: `syntax-error`. It is written as a type parameter's bound, as [Generic Locality](13-values-and-references.md#generic-locality) states.

9. r[types.lifetime.convention] By convention, type parameters come first, as in `fn f[T, 'a]`. A lifetime parameter written before a type parameter produces a lint finding. Warning: `lifetime-order`.

> **Why.** Explicit type arguments fill only the type parameters, so the
> order of a list changes no meaning. The convention follows Mojo's
> `[T, origin]` lists.

See also: [Lifetime Variables](13-values-and-references.md#lifetime-variables),
[Lifetime Implementation Targets](13-values-and-references.md#lifetime-implementation-targets),
[Lifetimes](01-lexical-structure.md#lifetimes).

## Variance

Generic type declarations mark covariance with `+T`, contravariance with `-T`,
and invariance by leaving `T` unmarked:

```text
struct Box[+T]:
    value: T

struct Consumer[-T]:
    consume: fn(T) -> void

struct Cell[T]:
    value: T
```

1. r[types.variance.markers] Generic type declarations mark covariance with `+T`, contravariance with `-T`, and invariance by leaving `T` unmarked.
2. r[types.variance.verified] The compiler verifies each declared parameter against its use on the type's surface.
3. r[types.variance.surface] That surface includes struct fields, enum shared data and variant payloads, and trait method signatures. It also includes the signature of every inherent method of the nominal type that has a `self` receiver, private methods included.
4. r[types.variance.trait-impl] A separate trait implementation does not alter the nominal type declaration's variance; its own instantiated signatures must still type-check.
5. r[types.variance.trait-params] A trait's generic parameters are invariant. A variance marker on one is an error. Error: `invalid-variance`.

```text
struct Box[+T]:
    value: T

impl[T] Box[T]:
    fn replace(self, next: T) -> void:  # error: invalid-variance
        pass
```

> **Why.** Privacy in hd is module-wide. So any code in the declaring
> module can convert a value by variance and then call a private method on
> it. So a private method's signature counts toward variance as a public
> one's does.

A `mut self`, `&self`, or `&mut self` method is on that surface too:

1. r[types.variance.surface.mut-self] An inherent method with a `mut self`, `&self`, or `&mut self` receiver counts toward declared variance, as a method with a `self` receiver does.

```text
struct Box[+T]:
    value: T

impl[U] Box[U]:
    pub fn set(&mut self, value: U) -> void:  # error: invalid-variance
        self.value = value
```

An associated function with no receiver is not on that surface:

1. r[types.variance.surface.no-receiver] An inherent associated function with no `self` receiver does not count toward declared variance. Only instance methods do, whatever their receiver.

```text
struct Box[+T]:
    value: T

impl[U] Box[U]:
    pub fn new(value: U) -> Box[U]:
        Box { value: value }
```

### Polarity

r[types.polarity] Polarity is computed as follows:

1. r[types.polarity.positive] A returned value and an ordinary field are positive positions.
2. r[types.polarity.negative] A function or method parameter is a negative position.
3. r[types.polarity.function] Entering a function parameter reverses polarity, while entering a function result preserves it.
4. r[types.polarity.generic] Applying a covariant generic argument preserves polarity, a contravariant argument reverses it, and an invariant argument makes the occurrence invariant.
5. r[types.polarity.mut] Occurrence beneath `&mut` is invariant, and an embedded field's type counts as beneath `&mut`.
6. r[types.polarity.both] A parameter used in both positive and negative positions must be invariant.

> **Why.** The storage beneath `&mut` can be both read and written.
> Access through an embedded field follows its container.

1. r[types.variance.covariant-check] A declared `+T` is rejected if any occurrence is negative or invariant. Error: `invalid-variance`.
2. r[types.variance.contravariant-check] A declared `-T` is rejected if any occurrence is positive or invariant. Error: `invalid-variance`.
3. r[types.variance.unmarked] An unmarked invariant parameter may occur in any position.

```hd
struct Box[+T]:
    value: T
```

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

### Variance On A Non-Bare Target

An inherent implementation whose target is not the bare declared
parameters checks its methods by the variance each `impl` parameter has
in the target:

```text
struct Box[+T]:
    value: T

struct Consumer[-T]:
    consume: fn(T) -> void

impl[U] Box[Consumer[U]]:
    pub fn feed(self, value: U) -> void:  # U is contravariant in the target
        pass
```

1. r[types.variance.target.derived] For an inherent `impl[U, ...] G[A1, ..., An]`, each `impl` parameter `U` has a **derived variance**: its variance in the target type, which the [Polarity](#polarity) rules compute.
2. r[types.variance.target.compose] A covariant parameter of `G` keeps `U`'s variance in its argument, a contravariant one reverses it, and an unmarked one makes it invariant.
3. r[types.variance.target.both] A `U` that the target places both covariantly and contravariantly, as in `impl[U] Pair[U, U]` for `Pair[+A, -B]`, is invariant.
4. r[types.variance.target.check] Each method signature of that implementation is checked as if `U` were declared with its derived variance: covariant `U` by `types.variance.covariant-check`, contravariant `U` by `types.variance.contravariant-check`. Error: `invalid-variance`.
5. r[types.variance.target.invariant] An invariant `U`, as in `impl[U] Box[U?]`, may occur in any position, as `types.variance.unmarked` allows.

```text
impl[U] Box[Consumer[U]]:
    pub fn make(self, factory: fn() -> U) -> U:  # error: invalid-variance
        factory()
```

> **Note.** For a bare target, as in `impl[U] Box[U]`, the derived
> variance is `T`'s declared marker, so the check is the one
> [Polarity](#polarity) states.

> **Why.** A variance conversion changes a target's arguments only as
> `G`'s markers allow. So `U` can change only as its place in the target
> lets it, and an invariant place fixes it. Kotlin and Scala compose
> variance the same way.

See also: [Polarity](#polarity), [`types.option.invariant`](#r-types.option.invariant).

### Readonly Outer Views

1. r[types.variance.readonly-only] Variance conversion applies only to a readonly outer type: one not written `*mut`, `&mut`, or `mut`, such as `G[T]`, `*G[T]`, or `&G[T]`.
2. r[types.variance.mut-invariant] Every writable reference, `*mut G[T]` or `&mut G[T]`, and every `mut Suspend[T]`, is invariant in all generic arguments.
3. r[types.variance.list] The built-in `List` declares a covariant element parameter.
4. r[types.variance.map] `Map[K, V]` is invariant in `K` and covariant in `V`.
5. r[types.variance.function] The function type constructors `Fn` and `SuspendFn` are contravariant in each input element, covariant in the output, and invariant in the requirement row.

> **Why.** A writable reference may replace stored values. Map keys are both
> accepted for lookup and exposed during traversal. Converting a function's
> row would change which providers the call passes, so the row stays
> invariant under variance. A function value itself may still widen its row
> by [row subsumption](11-requirements-and-suspension.md#row-subsumption),
> which may adapt the value.

```hd
struct User:
    name: string

struct Box[+T]:
    value: T

fn demo(boxed: *Box[*mut User]) -> *Box[*User]:
    boxed   # a readonly outer type converts covariantly
```

### Representation-Preserving Variance

1. r[types.variance.repr.covariant] A variance conversion `G[S] -> G[T]` requires a representation-preserving `S -> T` conversion for each covariant argument.
2. r[types.variance.repr.contravariant] A variance conversion `G[S] -> G[T]` requires a representation-preserving `T -> S` conversion for each contravariant argument.
3. r[types.variance.repr.weakening] Permission weakening, `*mut U -> *U`, `&mut U -> &U`, or `mut Suspend[U] -> Suspend[U]`, is representation-preserving.
4. r[types.variance.repr.excluded] These conversions are not representation-preserving:
   - construction of a trait value reference such as `&i32 -> &dyn Display` or `&User -> &dyn Display`;
   - child-dynamic-trait to supertrait widening;
   - optional injection `U -> U?`.
5. r[types.variance.no-insertion] Variance never inserts element wrappers, metadata rewrapping, boxing, copies, or per-access conversions. A variance conversion that would need one is an error. Error: `variance-representation-change`.

```hd
fn demo(items: *mut List[i32]) -> *List[i32]:
    items   # weakening *mut U to *U keeps the representation
```

See also: [Representation-Preserving Conversions](#representation-preserving-conversions).

## Trait Values And `Any`

This section defines trait conformance, dynamic trait values, written
`&dyn Trait`, and the `Any` trait family.

1. r[types.trait.explicit] Trait conformance is explicit.
2. r[types.trait.no-shape] Matching method shape alone does not make a type implement a trait.
3. r[types.trait.static] A generic bound such as `T < Display` uses static dispatch and preserves the concrete type.

```hd
struct User:
    name: string

fn demo(user: User) -> *dyn Any:
    box(user)   # every value satisfies Any
```

### Trait Value Types

1. r[types.trait.value.reference-values] A trait value type is `&dyn Tr` or `&mut dyn Tr`, or counted, `*dyn Tr` or `*mut dyn Tr`, referring to an object whose concrete type implements `Tr`.
2. r[types.trait.value.contents.reference-values] A trait value reference holds a reference to an object and dispatch metadata for its trait.
3. r[types.trait.value.dyn-required.reference-values] A bare trait name in a type position is an error whose fix-it writes `&dyn`. Error: `trait-used-as-type`.
4. r[types.trait.value.bound.reference-values] A dynamic trait value type satisfies a generic bound on its own trait and on each of that trait's supertraits. For example, a `*dyn Display` value is a valid argument for `T < Display`.
5. r[types.trait.value.bound.available.reference-values] It satisfies such a bound only when every member of the bound's trait is available on the `dyn` value, as [`trait.dyn.bound.reference`](09-traits.md#r-trait.dyn.bound.reference) states.
6. r[types.trait.value.bindings.reference-values] A trait value type may bind the trait's associated types, as in `&dyn Supplier[Item = i32]`, as [Bound Associated Types](09-traits.md#bound-associated-types) specifies.
7. r[types.trait.value.not-target] A dynamic trait value type is still not an implementation target.
8. r[types.trait.value.ref-written] A trait value reference, `&dyn Trait` or `&mut dyn Trait`, is a trait value that is a reference. A converted `&S` or `&mut S` is shared, and a value that is no reference does not convert, as [Trait Value References](09-traits.md#trait-value-references) states.

```hd
trait Greet:
    fn greet(self) -> string

fn demo(g: &dyn Greet) -> string:
    g.greet()
```

```text
trait Greet:
    fn greet(self) -> string

fn demo(g: Greet) -> string:  # error: trait-used-as-type
    g.greet()
```

> **Why.** `&dyn` makes the reference and runtime dispatch explicit. A bound such as `T < Greet` stays the monomorphized form.

See also: [Dynamic Trait Values](09-traits.md#dynamic-trait-values), which
gives the rule.

### Dynamic Safety

1. r[types.trait.dyn.any-trait] Every trait may be used as a `dyn` type. There is no per-trait safety check.
2. r[types.trait.dyn.member] A member that cannot work through a `dyn` value is unavailable on it, and calling it there is an error at the call. [Dynamic Safety](09-traits.md#dynamic-safety) lists those members. Error: `dyn-member-unavailable`.
3. r[types.trait.safe.assoc-bound.reference-values] The trait value type must bind each associated type of the trait and its supertraits, as in `&dyn Supplier[Item = i32]`.
4. r[types.trait.safe.method-type-param] A method with a method-level type parameter is available on a `dyn` value whatever the parameter's bounds, as in `T < Display` or an unbounded `T`.
5. r[types.trait.safe.method-type-arg] A call through a trait value may pass any type argument for such a parameter, including an `AnyVal` type such as `i32` or an enum.
6. r[types.trait.safe.method-type-arg.any-use] The method's signature and body may use the parameter in any way, including inside a container or a function type.
7. r[types.trait.safe.method-type-arg.box] An implementation may box a value-typed argument at that call. The box has no identity, so the boxing is not observable.
8. r[types.trait.safe.trait-generic] Trait declaration generic parameters are permitted.

```text
trait Versioned:
    fn version(self) -> i32
    fn newer_than(self, other: Self) -> bool

fn audit(record: &dyn Versioned) -> bool:
    if record.version() > 0:
        return true
    record.newer_than(record)  # error: dyn-member-unavailable
```

> **Why.** One concrete trait instantiation, such as `&dyn Repository[User]`, fixes
> the trait declaration's generic parameters before dispatch. A member is
> judged where it is called, so a trait with one unusual member still works
> as a `dyn` type.

### Supertrait Widening

1. r[types.trait.child] A dynamic child-trait value exposes methods declared by the child and all of its transitive supertraits.
2. r[types.trait.child.widen] A dynamic child-trait value may be widened implicitly to a dynamic supertrait value.
3. r[types.trait.child.one-way] That conversion discards access to child-only methods and cannot be reversed by a conversion; only `Inspectable` values recover a concrete type.
4. r[types.trait.child.repr] This direct widening may rewrap dispatch metadata and is therefore not representation-preserving for a variance conversion.

```hd
trait Animal:
    fn name(self) -> string

trait Dog < Animal:
    fn bark(self) -> string

struct Pup:
    name: string

impl Animal for Pup:
    fn name(self) -> string: self.name

impl Dog for Pup:
    fn bark(self) -> string: "woof"

fn name_of(animal: &dyn Animal) -> string:
    animal.name()

fn demo(pup: Pup) -> string:
    name_of(&pup)   # the reference converts to Animal
```

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity).

### `Any`

1. r[types.any] `Any` is the built-in universal empty trait.
2. r[types.any.all] Every value type, including an optional type, satisfies `Any` automatically.
3. r[types.any.never] `never` satisfies `Any` too. A `void` value needs no rule of its own, because it is the tuple `()`.
4. r[types.any.erase.reference-values] The reference type `&dyn Any` hides the concrete type of its referent.
5. r[types.any.one-way] `Any` erasure is one-way.
6. r[types.any.inspectable] A value erased to the sealed trait `std.inspect.Inspectable` instead keeps a runtime record of its concrete type, which `downcast` compares exactly.

```hd
fn demo(value: &dyn Any) -> string:
    "got one"   # Any erases the concrete type
```

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity).

### `AnyVal` And `AnyRef`

1. r[types.sealed.decl] `AnyVal` and `AnyRef` are the two sealed subtraits of `Any`, declared as `trait AnyVal < Any` and `trait AnyRef < Any`.
2. r[types.sealed.markers] Both are empty marker traits that the compiler implements.
3. r[types.sealed.no-impl] An explicit implementation of either is an error. Error: `sealed-trait-implementation`.
4. r[types.sealed.exactly-one] Every value type implements exactly one of them:

| Trait | Types | Identity |
| --- | --- | --- |
| r[types.sealed.anyval-members] `AnyVal` | `bool`, `char`, the integer types, `f32`, `f64`, `string`, tuples (`void` included), enums (optionals and `Result` included), structs, `List`, `Map`, and function types | These values have no identity. |
| r[types.sealed.anyref-members.counted] `AnyRef` | counted reference types `*T` and `*mut T`, trait value references `*dyn Tr` included; suspensions and runtime handles with identity | These values have identity. |

5. r[types.sealed.newtype] A newtype has its base type's category: `type Mile(i32)` implements `AnyVal`, and `type Owner(*User)` implements `AnyRef`.
6. r[types.sealed.never] `never` implements neither, because it has no values.
7. r[types.sealed.type-parameter] A type parameter implements `AnyVal` or `AnyRef` only through its bound.
8. r[types.sealed.bounds] `T < AnyVal` accepts only `AnyVal` types, and `T < AnyRef` accepts only `AnyRef` types.
9. r[types.sealed.local-reference] A local reference `&T` or `&mut T` has identity too, so it implements `AnyRef`, and `T < AnyRef` accepts one as a type argument ([Generic Calls](13-values-and-references.md#generic-calls)).

```text
struct Handle: pass

impl AnyRef for Handle  # error: sealed-trait-implementation
impl AnyVal for Handle  # error: sealed-trait-implementation
```

## Map Key Types

This section defines which types may be map keys, and how keys behave.

1. r[types.map-key.declared-bound] `Map` is declared as `Map[K < Eq & Hash, V]`. A key type that does not implement both traits fails that bound, as any unmet bound does. Error: `unsatisfied-trait-bound`.
2. r[types.map-key.hash] `Hash` is a standard-library trait in `std.hash`.
3. r[types.map-key.user] User-defined struct and enum types can become keys by explicitly implementing or deriving both traits.
4. r[types.map-key.builtin-hash] Standard-library implementations of `Hash` cover `bool`, integers, `char`, `string`, and tuples, optionals, results, and lists of hashable elements. So `List[T]` implements `Hash` when `T < Hash`, and `Result[T, E]` when `T < Hash` and `E < Hash`.
5. r[types.map-key.unhashable] Maps, floating-point values, functions, suspensions, dynamic trait values, and `Any` do not have built-in `Hash`.
6. r[types.map-key.user-enums] Every user struct and every user enum, including a payload-free one, require an explicit or derived implementation of both traits.
7. r[types.map-key.float-no-hash] Floating-point types implement `Eq` but not `Hash`, so they are not valid map keys.
8. r[types.map-key.no-map-hash] Consequently maps have no built-in hash and impose no order-independent map-hash obligation.
9. r[types.map-key.reference] A reference type implements `Eq` and `Hash` when the type it refers to does, so `*T` may be a key, compared and hashed by value. A map keyed by place uses [`Id[T]`](../std/cmp.md#identity-keys).

```text
struct UserId:
    value: string

fn setup() -> void:
    let users: Map[UserId, string] = {}  # error: unsatisfied-trait-bound
    pass
```

> **Why.** A NaN key is unequal even to itself, so no lookup could find it.

> **Why.** The key traits are an ordinary bound, so a missing trait reads
> as any other unmet bound.

### Lookup And Order

1. r[types.map.eq-hash] Map lookup and duplicate-key replacement use `Eq` for key comparison and `Hash` for indexing.
2. r[types.map.no-law] The language does not check or impose a law connecting these two implementations.
3. r[types.map.inconsistent] If an implementation hashes values differently that `Eq` considers equal, lookup and duplicate-key behavior are not guaranteed.
4. r[types.map.order.deterministic] Map iteration order is deterministic. A program that builds a map by the same operations on the same data iterates it in the same order in every run.
5. r[types.map.order.unspecified] Beyond that, the order is unspecified. It need not be insertion order.
6. r[types.map.replace.in-place] Replacing the value for an existing key does not change the map's iteration order.
7. r[types.map.semantics] Hash values remain outside map value semantics, and map equality remains independent of insertion order.

```hd
fn demo() -> List[string]:
    m := {"b": +1, "a": +2}
    let mut keys: List[string] = []
    for entry in m:
        keys.push(entry._0)
    keys   # ["b", "a"] or ["a", "b"]: the map's order, the same in every run
```

> **Why.** A deterministic order keeps output and tests reproducible.
> Leaving it unspecified lets the map use a faster layout and hash, as
> Go and Rust do. Code that needs an order sorts the keys.

### Mutated Keys

1. r[types.map.no-reindex] If a stored key's equality or hash changes after insertion, the map does not automatically reindex it.
2. r[types.map.stale] The entry can remain visible during iteration yet be unreachable by lookup or removal with the mutated key: a stale entry.
3. r[types.map.no-repair] Such mutation does not trigger a compile-time error or an automatic repair.
4. r[types.map.reference-key] A key that holds a `*mut` reference, at any depth, changes when the value the reference refers to is written through any path. Its entry then becomes a stale entry.

```hd
use std.hash.Hash

@derive(Eq, Hash)
struct Tag:
    id: i32

fn demo() -> void:
    let mut table: Map[Tag, string] = {}
    key := Tag { id: +1 }
    table[key] = "one"   # keyed by id through Eq and Hash
```

## Least Common Type

r[types.lct.sites] Several constructs infer one type from several values when no expected type is available, as this table lists. An expected type that is a type parameter the enclosing call has not solved yet counts as none: the site joins first, and the call then solves the parameter from the result.

| Construct | Values | Defined in |
| --- | --- | --- |
| List literal | the elements | [Expressions](05-expressions.md#list-and-map-expressions) |
| Map literal | the keys, and the values | [Expressions](05-expressions.md#list-and-map-expressions) |
| Value-producing `if` | the branches | [Control Flow](06-control-flow.md#conditional-expressions) |
| Value-producing `match` | the arm results | [Control Flow](06-control-flow.md#match-expressions) |
| Value-producing `while` or `for` | the `break` values and the `else` value | [Control Flow](06-control-flow.md#break-continue-and-loop-else) |
| Closure whose result type is inferred | the final value and `return` operands | [Functions](07-functions.md#closures) |
| Non-public function whose result type is omitted | the final value and `return` operands | [Functions](07-functions.md#declarations) |

1. r[types.lct.uses] Each of these uses the least common type defined here.
2. r[types.lct.unique] The compiler computes a unique least common type of the values' types using only the conversions that `types.lct.conversions` lists.
3. r[types.lct.conversions] Permission weakening, declared readonly variance, and optional injection may contribute. No other conversion does: no numeric conversion, so `[small, large]` with an `i16` and an `i64` has no common type. Error: `no-common-type`.
4. r[types.lct.optional] Optional injection adds one layer, as [`types.assign.optional`](#r-types.assign.optional) states. So an `i32` and an `i32?` join to `i32?`, in a list literal, a value-producing `if` or `match`, and an inferred result type alike.
5. r[types.lct.optional.one-layer] A `T` and a `T??` have no common type, because injection never adds two layers. Error: `no-common-type`.
6. r[types.lct.no-combine] Least-common-type inference never combines permission weakening with a variance step for the same candidate conversion.
7. r[types.lct.row-union-every-site] At every construct in the table, function values with different rows are first widened to the union of their rows ([Row Union In Literals](11-requirements-and-suspension.md#row-union-in-literals)).
8. r[types.lct.never.dropped] The least common type first drops every value of type `never`, then joins the rest. So `if ok: 1 else: return .None` has type `i32`.
9. r[types.lct.never.all] When every value has type `never`, the least common type is `never`.
10. r[types.lct.never.infer] A type parameter that only values of type `never` meet is inferred as `never`, as for `id(panic("x"))` with `fn id[T](x: T) -> T`.

```text
fn first(values: List[i32]) -> i32?:
    let ok = values.len() > 0
    value := if ok: values[0] else: return .None
    let found: i32? = value
    found

fn fail(message: string) -> i32:
    if message == "": panic("empty") else: panic(message)

fn fallback(count: i32, found: i32?, flag: bool) -> List[i32?]:
    value := if flag: count else: found
    values := [count, value]
    values

fn nested(count: i32, found: i32??, flag: bool) -> void:
    value := if flag: count else: found  # error: no-common-type
```

See also: a generic call that solves one type parameter from several
arguments uses a narrower join, permission weakening only
([`types.generic.infer.join`](#r-types.generic.infer.join)).

### No Implicit Erasure

1. r[types.lct.no-any.reference-values] The compiler never falls back to `&dyn Any` merely to make heterogeneous values type-check.
2. r[types.lct.no-trait-value] Unconstrained inference also does not introduce a dynamic trait-value conversion.
3. r[types.lct.no-supertrait-widening] It also never widens a dynamic trait value to a supertrait value, so values of two child traits of one supertrait have no common type.
4. r[types.lct.expected-trait.reference-values] An expected type such as `List[*dyn Display]` or `Map[K, *dyn Display]` may request that conversion explicitly.
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
fn mix(shown: *dyn Shown, tagged: *dyn Tagged) -> void:
    items := [shown, tagged]  # error: no-common-type
```

Here `Shown` and `Tagged` are traits that both extend `Named`;
`let items: List[*dyn Named] = [shown, tagged]` is valid.

## Type Inference Boundaries

r[types.infer.sites] The compiler infers the following:

- local binding types;
- closure parameter or result types when an expected function type supplies them;
- generic call arguments when the solution is unambiguous.

r[types.infer.explicit] The following declarations require explicit types:

1. r[types.infer.explicit.parameters] Named function parameters.
2. r[types.infer.explicit.results-declared] The results of public functions, public inherent methods, trait methods, and methods of trait implementations, and of at least one function in each recursive cycle.
3. r[types.infer.explicit.fields] Public and private struct fields.
4. r[types.infer.explicit.enum] Enum payload fields and constructor data.
5. r[types.infer.explicit.trait-methods] Trait method parameters and results.
6. r[types.infer.explicit.fn-type] Named function type parameters and bounds where applicable.

r[types.infer.body-result-private] Any other named function, inherent method, or local `fn` may infer its result from its body, as [Parameter And Result Types](07-functions.md#parameter-and-result-types) states.

```hd
fn demo() -> i32:
    x := +1   # inferred: no annotation needed for a local
    x
```

r[types.infer.named-fn] For a named function, inference covers only its result type and its requirement row. Its parameter types, generic parameters, and bounds are always written in its declaration.

### Ambiguous Inference

1. r[types.infer.no-overload] Inference must not select among overloaded functions.
2. r[types.infer.ambiguous] If inference has multiple valid solutions, compilation fails and the diagnostic must identify an annotation site that disambiguates the program.
3. r[types.infer.ambiguous.code] That error is distinct from `cannot-infer-type`, which a rule names when no solution exists. Error: `ambiguous-type`.

```text
trait Repo[T]:
    fn number(self) -> i32

struct User: pass
struct Post: pass

struct Job[A, B]:
    callback: fn() -> i32 $ Repo[A] + Repo[B]

fn read_both() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).number() + $.use(Repo[Post]).number()

fn schedule() -> void:
    _ := Job { callback: read_both }  # error: ambiguous-type
    let job: Job[User, Post] = Job { callback: read_both }  # the annotation picks one
```

The callback's row fits `Repo[A] + Repo[B]` with `A = User, B = Post`
and with the reverse, so the unannotated literal has two solutions.

> **Why.** hd-lang has no function overloading. A separate code tells the
> reader that inference found several answers, not none, so an annotation
> picks one.

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
| Identity-free composites | `string`, tuples, enum values (including `Result` and optionals), struct values, lists, maps, function values | none |
| Reference values | references, trait value references included, suspensions, runtime handles | the place a reference refers to; otherwise allocation identity |

The reference values are exactly the implementers of the sealed `AnyRef`
trait. The scalar values and identity-free composites are exactly the
implementers of the sealed `AnyVal` trait.

Values without identity are immutable at their top level, or change only
through their own place, so storing one by copy or by reference cannot be
observed. An implementation may copy, share, or deduplicate such a value
freely. A list or a map shares its elements with its copies and copies them
before its first change while they are shared.

Writing `&e` for a value that is no place creates a new object with its
own identity. Converting an existing reference to a trait value reference
shares its place, by [Values And References](13-values-and-references.md).

See also: [Trait Values And `Any`](#trait-values-and-any).

### Shapes and Generic Code

Generic code behaves as if each instantiation were written out for its
concrete type arguments. How an implementation compiles it is not
observable, and this specification prescribes no strategy. An
implementation may emit a body per concrete type, share bodies between
types, or pass bounds in any form.

A few rules limit what generic code can observe, so that each strategy
stays possible:

| Rule | What it keeps unobservable |
| --- | --- |
| [`types.generic.sharing`](#r-types.generic.sharing) | whether instantiations share a body |
| [`types.generic.identity`](#r-types.generic.identity) | identity on a type that might be a value type |
| [`types.generic.polymorphic-recursion`](#r-types.generic.polymorphic-recursion) | whether a growing call cycle could share one body |
| [Dynamic Safety](09-traits.md#dynamic-safety) | the body behind a trait value's method, which is one per implementation |

A method of a trait value has one body per implementation, so a
method-level type parameter there is the one erased path. A value-typed
argument for it is boxed at the call, which no program can observe, as
[Trait Values And `Any`](#trait-values-and-any) states. A row parameter
passes its providers as one bundle.

A value with a value layout may stay unboxed in locals, fields,
parameters, generic code, and containers. A program cannot tell a packed
`List[i32]` from a boxed one.

See also: [Name Resolution Across Packages](10-modules.md#name-resolution-across-packages).

### Composite Representation

- A struct is a record of its fields. A field whose type has a value
  layout is stored unboxed.
- An enum has a value layout that the implementation chooses. A small
  enum may be a tag and a few unboxed slots, and a larger one an immutable
  record. A payload of the enum's own type, directly or through other
  enums, is a reference, so the layout stays finite.
- Shared constructor data is a per-variant constant, stored once in a table
  indexed by the tag and never in an enum value.
- `T?` is an enum like any other. Because an optional has no identity, a
  present value may be represented by the payload itself, and `.None` by a
  null reference or a tag.
- A list is a growable array of its element shape, with value-layout
  elements unboxed. A map is expected to use
  hashing, with a deterministic iteration order.
- A closure is a function reference plus an environment record. A closure
  without captures needs no environment. Because a function value has no
  identity, an implementation may share one value for a named function or
  allocate one at each use.
- A dynamic trait value is the underlying reference plus a shared method
  table for the implementation. For `Inspectable` and the traits that
  extend it, the table also holds one interned descriptor of the recorded
  type. A `downcast` is then a descriptor comparison followed by a cast or an
  unboxing, and a `T < Inspectable` bound needs only that descriptor.

### Suspension Frames

A suspending function is compiled to a frame record, a state number, and a
poll function. The frame holds the arguments, construction-time providers,
the active child suspension, and the locals that are live across suspension
points. Frame contents are not observable, so an implementation may keep
only live values.

The frame representation as a whole is not observable either. When a
suspension frame is statically known, as for a direct `fn!` call, an
implementation may skip the uniform `Suspend[T]` wrapper. It may also skip
the boxing of the frame's result.

### Requirement Rows

How providers for a requirement row are passed is not observable. An
implementation may pass them positionally, ordered by a canonical order of
the row's keys, instead of through a keyed lookup.

### Representation-Preserving Conversions

A conversion is representation-preserving when the value after conversion is
the same runtime value as before. It keeps the same identity and the same
stored content, with no wrapper, copy, box, or re-encoding.

Permission weakening `&mut U -> &U` preserves representation. Conversion to a
trait value or `Any`, supertrait widening of a dynamic value, and optional
injection do not, which is why [Variance](#variance) excludes them.

## Unsupported Type-System Extensions

This section lists type-system features that hd-lang does not have.

1. r[types.unsupported.type-test] The only runtime type test is exact-type recovery from an `Inspectable` value.
2. r[types.unsupported.no-assertions] There are no trait-to-trait assertions, no tests on `Any` or on trait values whose trait does not extend `Inspectable`, and no type patterns in `match`.
3. r[types.unsupported.no-unions] hd-lang has no anonymous union types, including error unions such as `FsError | HttpError`.
4. r[types.unsupported.error-type] An error type is a nominal type or a dynamic trait value such as `std.error.Error`.
5. r[types.unsupported.panic-abi] The exact host representation of a checked runtime panic is an ABI concern; its language-level control-flow semantics are defined in [Control Flow](06-control-flow.md#runtime-panics).

```text
fn pick(flag: bool) -> i32 | string:   # error: syntax-error
    if flag: +1 else: "a"
```

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity),
[Error Trait](09-traits.md#error-trait).


## Opaque Result Types

An opaque result gives a function a written trait interface while keeping
its concrete result type in the defining body.

1. r[types.opaque.occurrence] Each `impl Trait[Args]` occurrence in a return annotation denotes an opaque type with the written trait bound.
2. r[types.opaque.identity-position] The declaring function, its generic arguments and the occurrence's position in the return annotation determine the opaque type's static identity.
3. r[types.opaque.distinct] Results from different function declarations are distinct types, even when their written bounds and hidden concrete types are equal.
4. r[types.opaque.distinct-position] Different opaque occurrences in one return annotation are distinct types, even when their bounds and hidden concrete types are equal.
5. r[types.opaque.head-only] Callers check against the declared bound, including its supertraits and associated-type bindings, without reading the defining body.
6. r[types.opaque.no-leakage] The hidden concrete type adds no trait implementation or member to the opaque type during caller checking.
7. r[types.opaque.representation] The hidden concrete type is a code-generation fact, not part of a caller's checking dependencies.
8. r[types.opaque.codegen] An opaque result does not introduce a trait value or dynamic trait dispatch.
9. r[types.opaque.recursive] A hidden result type that recursively contains itself is an error. Error: `recursive-value-type`.

```hd
fn answer() -> impl Display:
    +42

pub fn main() -> void $ Console:
    println("${answer()}")
```

```text
fn recursive() -> impl Display:  # error: recursive-value-type
    recursive()
```

> **Why.** The written bound keeps checking independent of the body. Code
> generation still sees the concrete representation and selects its trait
> implementations.

### Opaque Results That Borrow

An opaque result may hide a local reference that it borrows from its function's parameters, so a caller treats it as a [dependent result](13-values-and-references.md#dependent-results), whatever its hidden type is.

```hd
struct User:
    name: string

fn label(u: &User) -> impl Display:  # borrows from u
    &u.name

fn count() -> impl Display:  # borrows nothing
    +3

pub fn main() -> void $ Console:
    u := User { name: "ada" }
    println("${label(&u)} ${count()}")
```

1. r[types.opaque.borrows] An opaque type **borrows** when its function has a local reference parameter, a `&self` or `&mut self` receiver included.
2. r[types.opaque.borrows.position] An opaque type in a type argument of a list, a map, or a trait, its own bound's included, borrows nothing.
3. r[types.opaque.borrows.holds] An opaque type that borrows holds a local reference, by [`ref.local.holding`](13-values-and-references.md#r-ref.local.holding), in one reference position of its own.
4. r[types.opaque.borrows.source] That position takes the lifetime that [`ref.lifetime.elide`](13-values-and-references.md#r-ref.lifetime.elide) gives a result position. Under two or more lifetimes with no reference receiver, it depends on every parameter position.
5. r[types.opaque.borrows.body] Each reference position of a hidden value depends only on what its opaque type's position depends on, by [`ref.lifetime.return`](13-values-and-references.md#r-ref.lifetime.return). Under an opaque type that borrows nothing, it depends on nothing. Error: `local-reference-escapes`.
6. r[types.opaque.borrows.call] At a call, an opaque value that borrows depends on the arguments in its lifetime's positions, by [`ref.lifetime.call`](13-values-and-references.md#r-ref.lifetime.call). Every rule of [Local References](13-values-and-references.md#local-references) applies to it.
7. r[types.opaque.static] An opaque type never meets `'static`, since a caller does not see its hidden type ([Generic Locality](13-values-and-references.md#generic-locality)).

```text
struct User:
    name: string

fn label(u: &User) -> impl Display:
    &u.name

fn own(u: User) -> impl Display:
    &u.name  # error: local-reference-escapes

fn shown(first: bool) -> string:
    keep := User { name: "a" }
    let mut l = label(&keep)
    if first:
        other := User { name: "b" }
        l = label(&other)
    "${l}"  # error: local-reference-outlives-local

fn kept(u: &User) -> void:
    b := box(label(u))  # error: local-reference-escapes
```

> **Why.** This is Rust's rule that `impl Trait` captures the lifetimes in
> scope. A caller checks only the signature, so it must assume the hidden
> value borrows, even where it does not.

See also: [Opaque Results](07-functions.md#opaque-results).
