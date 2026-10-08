# Type System

Status: language specification draft.

This chapter defines types, assignability, access permission, generics, and
variance.

1. r[types.static] hd-lang is statically typed: every expression has a compile-time type.
2. r[types.no-execute] A program with a type error must not execute.
3. r[types.local-inference] Local inference removes redundant annotations; public and aggregate boundaries remain explicit.

```hd
fn demo(x: i32) -> i32:
    x + 1   # every expression has a compile-time type
```

See also: [Type Inference Boundaries](#type-inference-boundaries).

## Type Forms

r[types.forms.set] The type forms are:

| Form | Spelling |
| --- | --- |
| Primitive types | |
| Nominal data types and enums | |
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
| Mutable-access types | `mut T` |

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
9. r[types.prim.no-mut.self-call] A call of such a method therefore needs no mutable access to its receiver.

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
| r[types.literal.local.form.element] Collections | the elements of one list, map, tuple, or data literal | `[small, 1]` | the typed elements |
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
13. r[types.literal.local.erased] A literal converted to `dyn Any` or to another trait value type has its class's default type. So `let x: dyn Any = 42` holds a `usize`.
14. r[types.literal.local.instantiation] Two or more instantiations of one generic trait may fit a call only because a literal argument could take several widths. Then the literal keeps its default type. With no instantiation for that type, the call is an error. Error: `type-mismatch`.

```text
use std.ops.Add

data Money:
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

> **Note.** `let mut i = 0` is still an error, by
> [`types.bind.let-mut-primitive`](#r-types.bind.let-mut-primitive): a
> number is primitive at every width.

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

### Suffixed Literals

A suffixed literal has the type that its suffix function returns:

```text
use std.ops.num_suffix

data Millis:
    count: i64

@num_suffix
fn ms(count: i64) -> Millis:
    Millis { count: count }

fn delay() -> Millis:
    250ms  # ms(250): Millis
```

1. r[types.literal.call-result] A suffixed literal or prefixed string has the result type of its literal function.
2. r[types.literal.suffixed.in] The numeric literal is checked with the suffix function's parameter type as its expected type, by the rules above. So a suffix whose parameter is `i8` range-checks `300b`.

```text
use std.ops.num_suffix

@num_suffix
fn s(count: i64) -> i64: count * 1000

fn wait() -> void:
    half := 1.5s  # error: type-mismatch
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

A tuple is an immutable value without identity, so it implements `AnyVal`,
as [`types.sealed.anyval-values`](#r-types.sealed.anyval-values) states. No
element of it is a place, as
[`expr.place.tuple-element`](05-expressions.md#r-expr.place.tuple-element)
states.

1. r[types.tuple.structural] Tuple types are structural.
2. r[types.tuple.same] Two tuples have the same type when they have the same arity and pairwise-equal element types.
3. r[types.tuple.one-element] One-element tuples require a trailing comma.
4. r[types.tuple.empty] `()` is the empty tuple.
5. r[types.tuple.no-mut] A tuple type has no `mut` form. A tuple type written with `mut`, as in `mut (User, i32)`, is an error. Error: `mut-on-tuple`.
6. r[types.tuple.element-permission] Each element type keeps its own permission, so `(mut User, i32)` is valid, and its first element has mutable access.

```text
fn rename(pair: (mut User, i32)) -> void:
    pair._0.name = "Grace"  # valid: the element has mutable access
```

```text
fn invalid(pair: mut (User, i32)) -> void:  # error: mut-on-tuple
    let p: mut (i32, i32) = (1, 2)            # error: mut-on-tuple
```

> **Why.** A tuple has no identity and no assignable element, so a mutable
> view of one would permit nothing. Its elements carry their own
> permissions.

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

```hd
fn demo(flag: bool) -> i32?:
    if flag: +1 else: .None
```

### Representation And Propagation

1. r[types.option.repr] The representation of `Option[T]` is an implementation detail; for example, an implementation may represent `.None` as a null reference.
2. r[types.option.propagate] Postfix `?` on an optional expression either produces its contained value or returns `.None` from the nearest function.
3. r[types.option.propagate.result] That function must itself return a compatible optional type.
4. r[types.option.propagate.one-layer] Postfix `?` removes and propagates one optional layer at a time.
5. r[types.option.propagate.permission] A present value keeps the optional's declared contained type `T`, including `mut U` when `T = mut U`; unwrapping does not weaken that generic argument.

```hd
fn first(items: List[i32]) -> i32?:
    if items.len() == 0: .None
    else: .Some(items[0])

fn demo(items: List[i32]) -> i32?:
    first(items)?   # the value, or .None returns at once
```

### Optional Identity And Variance

1. r[types.option.ordinary] Optionals follow the ordinary enum rules in every other respect.
2. r[types.option.any] An optional value may be erased to `dyn Any` like any other enum value.
3. r[types.option.value] An optional is an enum, so it is a value without identity and implements `AnyVal`, whatever its payload type.
4. r[types.option.identity-error] `is` with an optional operand is an error, as for every enum. Error: `identity-requires-references`.
5. r[types.option.invariant] `Option` declares its parameter unmarked, so an optional is invariant in its contained type, as every unmarked parameter is.
6. r[types.option.invariant.result] `Result[T, E]` likewise declares both parameters unmarked, so it is invariant in both `T` and `E`.
7. r[types.option.variance.mutable] As with every generic composite, a mutable outer view is invariant.

```text
data User:
    name: string

fn view(user: (mut User)?) -> User?:
    user  # error: type-mismatch

fn settle(result: Result[mut User, string]) -> Result[User, string]:
    result  # error: type-mismatch

data Maybe[+T]:
    value: T?  # error: invalid-variance
```

> **Why.** The standard methods on optionals and results take `T` as a
> parameter, as `unwrap_or(self, fallback: T)` does. That is a negative
> position on the readonly public surface, so a covariant `T` would fail
> its own polarity check.

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
6. r[types.result.propagate.permission] The success value retains its declared generic type `T`, including `mut U` when `T = mut U`, regardless of whether the `Result` value itself is readonly.
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
2. r[types.display.float] `Display` formats floating values with the shortest round-trip decimal digits.
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
2. r[types.assign.weaken] `S` is `mut T` and the target requests the readonly view `T`.
3. r[types.assign.variance] A declared generic variance conversion permits the readonly outer type to change its type arguments.
4. r[types.assign.trait-value] `S` explicitly implements trait `T`, or `T` is `Inspectable` and `S` is an inspectable type, allowing construction of a dynamic trait value.
5. r[types.assign.supertrait] `S` is a dynamic child-trait value whose trait has `T` as a direct or transitive supertrait.
6. r[types.assign.optional] A value of `T` is injected into `T?`. The injection adds one layer only, so a `T` is not injected into `T??`.
7. r[types.assign.row-subsumption] `S` and `T` are function types, and `T`'s row entails every key of `S`'s row. Then `S` with `T`'s row is assignable to `T`, as [Row Subsumption](11-requirements-and-suspension.md#row-subsumption) states.
8. r[types.assign.never] `S` is `never`, as [`types.never.assignable`](#r-types.never.assignable) states.
9. r[types.assign.trait-value.mut] `S` is `mut U`, `T` is `mut dyn Trait`, and `U` meets `types.assign.trait-value` for `Trait`. This builds a mutable dynamic trait value, as in `let edit: mut dyn Display = mutable_user`.

```text
data User:
    name: string

impl Display for User:
    fn to_string(self) -> string:
        self.name

fn edit(mutable_user: mut User) -> void:
    let edit: mut dyn Display = mutable_user
    let shown: dyn Display = edit
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

## Composite Values And Access Permission

Composite values are shared by reference, and `mut` grants mutable access to
one.

1. r[types.composite.kinds] Data types, enums with storage, tuples, lists, maps, trait values, and closures are composite values.
2. r[types.composite.shared] Composite parameters and results use shared references at the language level.
3. r[types.composite.aliases] hd-lang does not require exclusive ownership and may have multiple aliases to one composite value.

```hd
fn demo(items: mut List[i32]) -> void:
    items.push(+1)   # shared by reference: the caller sees the push
```

### Views

For a composite type `T`:

1. r[types.view.readonly] `T` is a readonly reference view. It permits observation but not mutation through that reference.
2. r[types.view.mutable] `mut T` is a mutable reference view. It permits operations that mutate the referenced value.
3. r[types.view.term] Throughout the specification, **readonly view** is the single term for `T` access to a composite value; it does not imply deep immutability.
4. r[types.view.non-reassignable] A binding is described separately as **non-reassignable** when its name cannot be rebound.

```hd
fn demo(items: List[i32]) -> usize:
    items.len()   # a readonly view: observation only
```

### Access Permission

1. r[types.mut.permission] `mut` expresses access permission, not ownership, uniqueness, or a deep freeze of the object.
2. r[types.mut.no-field-reassign] A readonly `T` reference cannot reassign its fields.
3. r[types.mut.mut-field] A direct data field declared `field: mut U` is read as `U` through readonly `T`.
4. r[types.mut.generic-field] A generic data field declared `field: P` retains its substituted type even when `P` is instantiated as `mut U`.
5. r[types.mut.other-forms] Other extraction forms state their own permission rules below.
6. r[types.mut.weaken] A `mut T` may be viewed as `T`.
7. r[types.mut.no-upgrade] A `T` must never be upgraded to `mut T`. An upgrade is an error. Error: `mutable-upgrade`.
8. r[types.mut.no-upgrade.inference] The error includes an upgrade that generic inference would produce. Given `keep[T](value: T) -> T`, binding `keep(readonly_value)` to a `mut T` declaration is rejected rather than inferring `T` as a mutable type.
9. r[types.mut.argument] A readonly argument cannot satisfy a mutable parameter. Error: `readonly-argument-to-mutable-parameter`.
10. r[types.mut.argument.infer] Before reporting this error for a generic mutable parameter, inference solves its type variables from the argument's readonly shape.

> **Why.** Solving the generic variables keeps the diagnostic concrete without
> granting mutable access to the readonly argument.

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

1. r[types.fresh.mutable-outer] A fresh data or copy-update expression, stored enum construction, list expression, or map expression produces mutable access to its new outer object.
2. r[types.fresh.tuple] A tuple expression produces a tuple, which has no `mut` form, as [`types.tuple.no-mut`](#r-types.tuple.no-mut) states. Its elements keep their permissions.
3. r[types.fresh.weaken] This permission may be weakened immediately by an expected readonly type.
4. r[types.fresh.weaken.meet] It is also weakened where the fresh value meets a readonly value of its type. An `==` or `!=` operand pair compares at the readonly view, and a generic inference join solves at it.
5. r[types.fresh.not-recursive] Freshness does not recursively upgrade composite values stored in the new object.
6. r[types.fresh.element-permission] Each field or element keeps the permission of the supplied expression and declared edge.
7. r[types.fresh.element-no-weaken] An expected type weakens only the fresh expression it applies to, never the elements of a collection already built. So `[for p in parts => Word { text: p }].iter()` has type `mut Iterator[mut Word]`, and returning it as `mut Iterator[Word]` is an error. Error: `type-mismatch`.

```hd
fn demo() -> List[i32]:
    [+1, +2]   # fresh: mutable access to the new outer object
```

#### Fresh Literals With Readonly Parts

1. r[types.fresh.readonly-field] A data literal with a direct `field: mut U` may produce readonly `T` when that field is supplied only `U`.
2. r[types.fresh.mut-literal] A literal produces `mut T` only when every direct mutable field is supplied mutable access and every embedded copy has mutable access.
3. r[types.fresh.spread] A value copied from a spread counts as supplied through the spread source's view.
4. r[types.fresh.expected-readonly] An expected readonly `T`, including a `:=` binding, permits the weaker field value.
5. r[types.fresh.expected-mut] An expected `mut T` rejects the weaker field value or a readonly part copy. Error: `mutable-upgrade`.
6. r[types.fresh.expected-mut.sites] The expected type is `mut T` wherever the literal is used as `mut T`:
   - an annotated `mut T` binding;
   - a `mut T` argument or result;
   - a store into a `mut T` field or element.
7. r[types.fresh.generic-field] A generic field declared `field: P` still requires its substituted type, including `mut U` when `P = mut U`.

```hd
data Box:
    content: mut List[i32]

fn demo(items: List[i32]) -> Box:
    Box { content: items }   # readonly items: the Box is readonly
```

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
9. r[types.bind.let-mut-tuple] `let mut` on a name whose type is a tuple, as in `let mut pair = (1, 2)`, is an error, in place of `mutable-upgrade` or `let-mut-readonly-type`. Error: `mut-on-tuple`.
10. r[types.bind.let-mut-tuple.hint] The diagnostic says that a plain `let` is already reassignable, and that an element's permission comes from its own type.
11. r[types.bind.let-mut-optional] For `let mut`, an optional written `mut T?` counts as mutable access. So `let mut u = find()` is valid when `find` returns `mut User?`, and `u` has that type.
12. r[types.bind.let-mut-expected] The initializer of an unannotated `let mut` is used as `mut T`. So a fresh literal whose direct `mut` field or embedded copy is readonly is an error, as [`types.fresh.expected-mut`](#r-types.fresh.expected-mut) states. Error: `mutable-upgrade`.
13. r[types.bind.let-mut-copy] A mutable copy of a readonly value is therefore written as a fresh literal, such as `User { ...user }`, whose `mut` fields are supplied mutable values.

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
    let mut pair = (1, 2) # error: mut-on-tuple
```

14. r[types.bind.let-mut-annotated] `let mut` with an annotation whose type is `mut T`, as in `let mut user: mut User = ...`, is valid; the `mut` after `let` is redundant.
15. r[types.bind.let-mut-annotated.warning] That redundant `mut` gets a warning. Warning: `redundant-let-mut`.
16. r[types.bind.let-mut-annotated.fix] The warning's fix-it removes the `mut` before the name and keeps the annotation.
17. r[types.bind.let-mut-annotation] `let mut` with a readonly annotation, as in `let mut user: User = ...`, is an error, because the annotation and `let mut` disagree. Error: `let-mut-readonly-type`.
18. r[types.bind.let-mut-annotation.fix] The diagnostic suggests adding `mut` to the type or removing the `mut` after `let`.

```text
fn invalid() -> void:
    let mut names: List[string] = []  # error: let-mut-readonly-type
    let mut ids: mut List[i64] = []   # warning: redundant-let-mut
```

19. r[types.bind.let-pattern-mut] In a `let` pattern, each name follows these rules for the value it binds, as a `match` arm would bind it. In `let (mut log, db) = pair`, `log` has mutable access and `db` the readonly view.
20. r[types.bind.let-pattern-mut.data] The same holds in a data or variant pattern. In `let User { mut tags, name } = user`, with `user: mut User` and a field `tags: mut List[string]`, `tags` has type `mut List[string]`.
21. r[types.bind.let-mut-pattern.annotated] With a tuple annotation, the element type of each name written `mut` must be a `mut` type. Error: `let-mut-readonly-type`.
22. r[types.bind.let-mut-pattern.redundant] That `mut` before the name is redundant and gets the same warning, with the same fix-it. Warning: `redundant-let-mut`.

```text
fn pair() -> (mut User, mut User):
    (User { name: "Ada" }, User { name: "Grace" })

fn edit() -> void:
    let (mut first, second) = pair()  # mut User, User
    first.name = "Lin"
    println(second.name)
```

```text
data Profile:
    name: string
    tags: mut List[string]

fn tag(profile: mut Profile, readonly: Profile) -> void:
    let Profile { mut tags, name } = profile  # mut List[string], string
    tags.push(name)
    let Profile { tags: mut others } = readonly  # error: mutable-upgrade
```

```text
fn pair() -> (mut User, mut User):
    (User { name: "Ada" }, User { name: "Grace" })

fn edit() -> void:
    let (mut first, second): (mut User, User) = pair()  # warning: redundant-let-mut
    first.name = "Lin"
    println(second.name)
```

> **Why.** The fix-it keeps the annotation because the annotation may be
> what solves a generic right-hand side, as in
> `let (a, b): (User, mut User) = make_pair()` for
> `fn make_pair[A, B]() -> (A, B)`. A `mut` before a name never supplies a
> type.

> **Note.** No binding form is non-reassignable and mutable at once: a
> local that is mutated but never reassigned is written with `let mut`.

> **Why.** `let mut` removes the repeated type from
> `let mut user = User { ... }`, while the declaration still says
> which locals change. `mut` keeps one meaning, a permission in the type.

23. r[types.bind.call-result] Passing through a function also follows the declared result type rather than recovering freshness.
24. r[types.bind.call-result.argument] Consequently, a fresh literal may be passed directly to a `mut T` parameter. A call declared to return `T` cannot be passed to one, even when its implementation constructs a fresh value.
25. r[types.bind.never] A binding without a type annotation whose initializer has type `never` is an error. Error: `uninhabited-binding`.

```text
fn first_even(values: List[i32]) -> i32:
    found := panic("no even value")  # error: uninhabited-binding
    found
```

> **Why.** No value of type `never` exists, so the binding could never be
> read. The initializer alone, without `found :=`, says the same thing.

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
6. r[types.path.field.generic.example] Reading `value: P` from readonly `Box[mut User]` therefore yields `mut User`.
7. r[types.path.field.substituted] Every type is read after substitution of the container's type arguments.
8. r[types.path.collection] Indexing, iteration, and lookup on a built-in collection yield its declared element or value type, whatever the collection's own permission.
9. r[types.path.collection.example] Indexing readonly `List[mut User]` yields `mut User`, and a successful lookup in a readonly `Map[K, mut User]` yields `mut User` after unwrapping.
10. r[types.path.contents] Optional and `Result` unwrapping, tuple element extraction, and generic enum payloads likewise yield their declared contents.
11. r[types.path.enum-payload] A non-generic enum payload declared `mut U` follows the field rule above.

```hd
data User:
    name: string

fn rename(user: mut User, name: string) -> void:
    user.name = name
```

See also: [Data Embedding](08-data-and-enums.md#data-embedding),
[Member Resolution](03-names-and-scopes.md#member-resolution).

#### Mutation Checks

An expression has **mutable access** when one of these holds:

| Rule | The expression | Example |
| --- | --- | --- |
| r[types.path.access.mut-type] Mutable type | has a type `mut U` | `account.profile` below |
| r[types.path.access.mut-bound] Mutable bound | has a type parameter's type, and the parameter is bounded by `mut Trait` or `mut Any` | `value` in `clear_value[T < mut Clear](value: T)` |
| r[types.path.access.mut-self] Mutable receiver | is `self` inside a `mut self` method | `self` in `fn clear(mut self)` |

1. r[types.path.mutation] A mutation needs mutable access on exactly one expression: the one it acts on.
2. r[types.path.mutation.needs-access] Each of these requires mutable access to `e`:
   - reassigning `e.field`;
   - replacing an element with `e[i] = value`;
   - calling a `mut self` method on `e`, including a container-mutating method such as `push`, and a method promoted from an embedded field.
3. r[types.path.mutation.only] Nothing else in the path is checked: the access type of `e` already records every permission removed on the way to it.
4. r[types.path.mutation.receiver] A `mut self` call on an `e` without mutable access is an error, whatever the path to `e`, including a promoted call. Error: `mutable-receiver-required`.

```text
data Account:
    profile: mut Profile

let mut account = Account { profile: profile }
account.profile.display_name = "Ada"   # account.profile has type mut Profile
```

When the `e` of a field store or an element replacement lacks mutable
access, the diagnostic names why:

5. r[types.path.store.readonly-edge] A store through an `e` that is a field read through a readonly edge, `field: U`, is an error. Error: `readonly-edge`.
6. r[types.path.store.readonly-root] A store through any other `e` without mutable access is an error. Error: `readonly-root`.
7. r[types.path.readonly-root.cases] The `readonly-root` cases are these:
   - a readonly binding, parameter, `self`, call result, element, or unwrapped value;
   - a mutable edge or embedded field read through a readonly value;
   - a generic field whose type argument is readonly.

```text
data Child:
    name: string

impl Child:
    fn rename(mut self, name: string) -> void:
        self.name = name

data Parent:
    child: Child

fn invalid(parent: mut Parent, child: Child) -> void:
    parent.child.name = "new"      # error: readonly-edge
    child.name = "new"             # error: readonly-root
    parent.child.rename("new")     # error: mutable-receiver-required
```

#### Field Stores

1. r[types.path.reassign] A `mut T` value may reassign every field with a value assignable to the field's declared type, whether the field is `field: U` or `field: mut U`.
2. r[types.path.reassign.old-value] Replacing a field does not mutate the old referenced value.
3. r[types.path.store-mut-edge] Storing into a direct `field: mut U` of a mutable value requires `mut U`.
4. r[types.path.store-readonly] A readonly value may store `U` there and cannot later be upgraded to `mut T`.
5. r[types.path.store-embedded] Storing into an embedded field, written `e.E ...= value`, stores the copy-update `T { ...value }` of the field's type `T`, which must have mutable access. Otherwise the store is an error. Error: `mutable-upgrade`.

```hd
data Box:
    content: mut List[i32]

fn demo(box: mut Box) -> void:
    box.content = [+1]   # storing mutable access into a mut edge
```

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

```hd
fn demo() -> void:
    let table: mut Map[string, i32] = {}
    table["a"] = +1   # replace elements: yes
```

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

This section defines generic parameters, bounds, arguments, and their runtime representation.

1. r[types.generic.complete] Generic type and function parameters denote complete types.
2. r[types.generic.mut-argument] A type parameter `T` may therefore be instantiated with either `User` or `mut User`.
3. r[types.generic.naming] By convention, generic parameters, including row parameters, use uppercase names such as `T`, `U`, `K`, `V`, and `R`.
4. r[types.generic.kind] The convention is style only: a parameter's kind comes from its declaration and use, never from the case of its name.

```hd
fn first[T](items: List[T]) -> T?:
    if items.len() == 0: .None
    else: .Some(items[0])
```

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

### Bound Order

A bound may name a later parameter of its list, and two bounds may name
each other:

```text
trait Source[T]:
    fn take(self) -> T

trait Sink[T]:
    fn give(self, value: T) -> void

data Tank:
    level: i32

data Pipe:
    width: i32

data Hose:
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
```

### Inference From Several Arguments

When a call solves one type parameter from several arguments, the
arguments' types may differ only in `mut`:

1. r[types.generic.infer.join] When generic call inference solves one type parameter from several arguments, the only conversion between their types is permission weakening. `mut X` and `X` meet at `X`.
2. r[types.generic.infer.join.outer-permission] Sources that disagree only in outer permission weaken to the readonly view, so a `mut T` and a `T` solve the parameter as `T`. This holds when one argument is a fresh value, as in `same(Date { year: 2026 }, d)` with `d: Date`.
3. r[types.generic.infer.join.no-trait-value] A trait-value conversion never applies, as [`types.lct.no-trait-value`](#r-types.lct.no-trait-value) states for the least common type. So `cmp(user, label)` with a `User` and a `Display` argument is an error. Error: `no-common-type`.
4. r[types.generic.infer.join.no-supertrait-widening] A supertrait widening never applies either, so two arguments of two child traits of one supertrait are an error. Error: `no-common-type`.
5. r[types.generic.infer.join.other-conflict] Any other conflict between the arguments' types is an error. Examples are `choose(1, true)`, `max(small, large)` with an `i32` and an `i64`, a `List[mut User]` and a `List[User]`, and a `T` and a `T?`. Error: `type-mismatch`. The caller writes a cast, as in `max(i64(small), large)`.
6. r[types.generic.infer.join.literal] An integer literal argument is not a conversion: it takes the type solved from the other arguments as its expected type, in any position. So `pick(1, large)` with an `i64` `large` solves `T = i64`.
7. r[types.generic.infer.join.explicit] An explicit type argument, as in `cmp::[dyn Display](user, label)`, is an expected type for each argument, which then converts by [Assignability And Coercion](#assignability-and-coercion), as [`types.lct.expected-trait`](#r-types.lct.expected-trait) allows.
8. r[types.generic.infer.join.not-lct] This join is narrower than the [least common type](#least-common-type), and is not one of that section's constructs.

```text
fn max[T < Ord](left: T, right: T) -> T:
    match left.cmp(right):
        .Less => right
        _ => left

fn cmp[T < Display](left: T, right: T) -> bool:
    "$left" == "$right"

fn widest(small: i32, large: i64) -> i64:
    max(small, large)  # error: type-mismatch

fn same(user: User, label: dyn Display) -> bool:
    cmp(user, label)  # error: no-common-type
```

```text
data User:
    name: string

fn pick[T](left: T, right: T) -> T:
    left

fn first(owned: mut User, shared: User) -> User:
    pick(owned, shared)  # T is User

fn either(large: i64) -> i64:
    pick(1, large)  # T is i64; the literal 1 is an i64

fn widest(small: i32, large: i64) -> i64:
    max(i64(small), large)

fn same(user: User, label: dyn Display) -> bool:
    cmp::[dyn Display](user, label)
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

data Label:
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

1. r[types.generic.infer.bound] A **bound-only parameter** of a call is a type parameter that no parameter type names but a bound of another type parameter does. One case is `U` in `read[U, S < Source[U]](source: S)`.
2. r[types.generic.infer.bound.after] Call inference solves a bound-only parameter after the other parameters.
3. r[types.generic.infer.bound.one] Once the bounded parameter is known, if its type implements the bound's trait for exactly one instantiation, that instantiation solves the bound-only parameter.
4. r[types.generic.infer.bound.none] If the type implements the bound's trait for no instantiation, the call is an error. Error: `unsatisfied-trait-bound`.
5. r[types.generic.infer.bound.ambiguous] If the type implements the bound's trait for several instantiations, this step leaves the bound-only parameter unsolved.
6. r[types.generic.infer.bound.default] A bound-only parameter left unsolved takes its type-argument default, as [`types.generic.default.fill`](#r-types.generic.default.fill) gives, and the bound is then checked.
7. r[types.generic.infer.bound.no-default] A bound-only parameter left unsolved without a default is an error, and an explicit type argument resolves it.
8. r[types.generic.infer.bound.no-default.ambiguous] When a bound of the parameter allows several instantiations, the error is `ambiguous-type`, by [`types.infer.ambiguous.code`](#r-types.infer.ambiguous.code). Error: `ambiguous-type`.
9. r[types.generic.infer.bound.no-default.disagree] When each bound allows exactly one instantiation and they differ, no solution exists. Error: `cannot-infer-type`.
10. r[types.generic.infer.bound.several-bounds] When several bounds name one bound-only parameter, this step solves it only once every parameter those bounds constrain is known.
11. r[types.generic.infer.bound.agree] It then solves the parameter only if each of those bounds allows exactly one instantiation and they all allow the same one.
12. r[types.generic.infer.bound.disagree] Otherwise, when bounds allow different instantiations or one allows several, the parameter is left unsolved, as for one bound.
13. r[types.generic.infer.bound.fixed-point] The step repeats until it solves no further parameter, so a chain of such bounds is solved in dependency order.
14. r[types.generic.infer.bound.precedence] An explicit type argument and an expected type take precedence: this step never solves a parameter that either of them solves.
15. r[types.generic.infer.bound.placeholder] An explicit list may mix written arguments with `_` slots, as in `read::[string, _](both)`; each `_` slot is inferred by the steps above.
16. r[types.generic.infer.bound.call-site] This step is call-site inference only. A declaration's generic parameters and signature are never inferred, as [`types.infer.explicit`](#r-types.infer.explicit) requires.

```text
data Both:
    count: i32

impl Source[i32] for Both:
    fn take(self) -> i32:
        self.count

impl Source[string] for Both:
    fn take(self) -> string:
        "both"

data Plain:
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

A default fills a bound-only parameter that several instantiations leave
open. The bound of `S` names the later `U`, so only `U` needs a default:

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
data Note:
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

8. r[types.generic.default.after-inference] At a use site that infers, the use site first solves its parameters as it would without defaults. That holds for a call, a function value, and a data literal.
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
6. r[types.generic.polymorphic-recursion] So polymorphic recursion, in which a generic function calls itself with ever larger type arguments, is always an error under any limit.
7. r[types.limit.type-size] An implementation may limit the size of one type, counted in the types it is built from. The limit is implementation-defined.
8. r[types.limit.type-size.error] A type over that limit, whether written or inferred, is an error. Error: `type-too-large`.

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
> compile generic code per concrete type with no fallback path. The fix
> is a trait value or a non-generic helper.

See also: [Implementation Model](#implementation-model-non-normative), which
describes the reference strategy.

### Identity On Type Parameters

1. r[types.generic.identity] Identity comparison `is` on a type parameter is permitted only with the sealed `T < AnyRef` bound. Without it, the comparison is an error. Error: `identity-requires-references`.
2. r[types.generic.identity.primitive] An unconstrained type parameter may be a value type after substitution, such as a primitive or an enum, and therefore cannot be used with `is`.

```hd
fn same[T < AnyRef](a: T, b: T) -> bool:
    a is b
```

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
2. r[types.variance.verified] The compiler verifies each declared parameter against its use on the type's readonly surface.
3. r[types.variance.surface] That surface includes data fields, enum shared data and variant payloads, and trait method signatures. It also includes the signature of every inherent method of the nominal type that has a `self` receiver, private methods included.
4. r[types.variance.trait-impl] A separate trait implementation does not alter the nominal type declaration's variance; its own instantiated signatures must still type-check.
5. r[types.variance.trait-params] A trait's generic parameters are invariant. A variance marker on one is an error. Error: `invalid-variance`.

```text
data Box[+T]:
    value: T

impl[T] Box[T]:
    fn replace(self, next: T) -> void:  # error: invalid-variance
        pass
```

> **Why.** Privacy in hd is module-wide. So any code in the declaring
> module can convert a value by variance and then call a private method on
> it. So a private method's signature counts toward variance as a public
> one's does.

A `mut self` method is on that surface too:

1. r[types.variance.surface.mut-self] An inherent method with a `mut self` receiver counts toward declared variance, as a method with a `self` receiver does.

```text
data Box[+T]:
    value: T

impl[U] Box[U]:
    pub fn set(mut self, value: U) -> void:  # error: invalid-variance
        self.value = value
```

An associated function with no receiver is not on that surface:

1. r[types.variance.surface.no-receiver] An inherent associated function with no `self` receiver does not count toward declared variance. Only instance methods do, whether `self` or `mut self`.

```text
data Box[+T]:
    value: T

impl[U] Box[U]:
    pub fn new(value: U) -> Box[U]:
        Box { value: value }
```

### Polarity

r[types.polarity] Polarity is computed as follows:

1. r[types.polarity.positive] A returned value and an ordinary readonly field are positive positions.
2. r[types.polarity.negative] A function or method parameter is a negative position.
3. r[types.polarity.function] Entering a function parameter reverses polarity, while entering a function result preserves it.
4. r[types.polarity.generic] Applying a covariant generic argument preserves polarity, a contravariant argument reverses it, and an invariant argument makes the occurrence invariant.
5. r[types.polarity.mut] Occurrence beneath `mut` is invariant, and an embedded field's type counts as beneath `mut`.
6. r[types.polarity.both] A parameter used in both positive and negative positions must be invariant.

> **Why.** The referenced storage beneath `mut` can be both read and
> written. Access through an embedded field follows its container.

1. r[types.variance.covariant-check] A declared `+T` is rejected if any occurrence is negative or invariant. Error: `invalid-variance`.
2. r[types.variance.contravariant-check] A declared `-T` is rejected if any occurrence is positive or invariant. Error: `invalid-variance`.
3. r[types.variance.unmarked] An unmarked invariant parameter may occur in any position.

```hd
data Box[+T]:
    value: T
```

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

### Variance On A Non-Bare Target

An inherent implementation whose target is not the bare declared
parameters checks its methods by the variance each `impl` parameter has
in the target:

```text
data Box[+T]:
    value: T

data Consumer[-T]:
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

```hd
data User:
    name: string

data Box[+T]:
    value: T

fn demo(box: Box[mut User]) -> Box[User]:
    box   # a readonly outer view converts covariantly
```

### Representation-Preserving Variance

1. r[types.variance.repr.covariant] A variance conversion `G[S] -> G[T]` requires a representation-preserving `S -> T` conversion for each covariant argument.
2. r[types.variance.repr.contravariant] A variance conversion `G[S] -> G[T]` requires a representation-preserving `T -> S` conversion for each contravariant argument.
3. r[types.variance.repr.weakening] Permission weakening `mut U -> U` is representation-preserving.
4. r[types.variance.repr.excluded] These conversions are not representation-preserving:
   - construction of a trait value such as `i32 -> dyn Display` or `User -> dyn Display`;
   - child-dynamic-trait to supertrait widening;
   - optional injection `U -> U?`.
5. r[types.variance.no-insertion] Variance never inserts element wrappers, metadata rewrapping, boxing, copies, or per-access conversions. A variance conversion that would need one is an error. Error: `variance-representation-change`.

```hd
fn demo(items: mut List[i32]) -> List[i32]:
    items   # weakening mut U to U keeps the representation
```

See also: [Representation-Preserving Conversions](#representation-preserving-conversions).

## Trait Values And `Any`

This section defines trait conformance, dynamic trait values, written
`dyn Trait`, and the `Any` trait family.

1. r[types.trait.explicit] Trait conformance is explicit.
2. r[types.trait.no-shape] Matching method shape alone does not make a type implement a trait.
3. r[types.trait.static] A generic bound such as `T < Display` uses static dispatch and preserves the concrete type.

```hd
data User:
    name: string

fn demo(user: User) -> dyn Any:
    user   # every value satisfies Any
```

### Trait Value Types

1. r[types.trait.value] A trait value type is written `dyn` and a trait, as in `dyn Display`. It creates a Go-style dynamic trait value.
2. r[types.trait.value.contents] A dynamic trait value contains a concrete value and dispatch metadata for that trait.
3. r[types.trait.value.dyn-required] A bare trait name in a type position is an error, and its fix-it inserts `dyn`. Error: `trait-used-as-type`.
4. r[types.trait.value.bound] A dynamic trait value type satisfies a generic bound on its own trait and on each of that trait's supertraits. For example, a `dyn Display` value is a valid argument for `T < Display`.
5. r[types.trait.value.bound.available] It satisfies such a bound only when every member of the bound's trait is available on the `dyn` value, as [`trait.dyn.bound.available`](09-traits.md#r-trait.dyn.bound.available) states.
6. r[types.trait.value.bindings] A trait value type may bind the trait's associated types, as in `dyn Supplier[Item = i32]`, as [Bound Associated Types](09-traits.md#bound-associated-types) specifies.
7. r[types.trait.value.not-target] A dynamic trait value type is still not an implementation target.
8. r[types.trait.value.mut] `mut dyn Trait` is the mutable view of a trait value, as `mut T` is of any type.

```hd
trait Greet:
    fn greet(self) -> string

fn demo(g: dyn Greet) -> string:
    g.greet()
```

```text
trait Greet:
    fn greet(self) -> string

fn demo(g: Greet) -> string:  # error: trait-used-as-type
    g.greet()
```

> **Why.** `dyn` marks where a value is boxed and its calls are dispatched
> at run time. A bound such as `T < Greet` stays the monomorphized form.

See also: [Dynamic Trait Values](09-traits.md#dynamic-trait-values), which
gives the rule.

### Dynamic Safety

1. r[types.trait.dyn.any-trait] Every trait may be used as a `dyn` type. There is no per-trait safety check.
2. r[types.trait.dyn.member] A member that cannot work through a `dyn` value is unavailable on it, and calling it there is an error at the call. [Dynamic Safety](09-traits.md#dynamic-safety) lists those members. Error: `dyn-member-unavailable`.
3. r[types.trait.safe.assoc-bound] The trait value type must bind each associated type of the trait and its supertraits, as in `dyn Supplier[Item = i32]`.
4. r[types.trait.safe.method-type-param] A method with a method-level type parameter is available on a `dyn` value whatever the parameter's bounds, as in `T < Display` or an unbounded `T`.
5. r[types.trait.safe.method-type-arg] A call through a trait value may pass any type argument for such a parameter, including an `AnyVal` type such as `i32` or an enum.
6. r[types.trait.safe.method-type-arg.any-use] The method's signature and body may use the parameter in any way, including inside a container or a function type.
7. r[types.trait.safe.method-type-arg.box] An implementation may box a value-typed argument at that call. The box has no identity, so the boxing is not observable.
8. r[types.trait.safe.trait-generic] Trait declaration generic parameters are permitted.

```text
trait Versioned:
    fn version(self) -> i32
    fn newer_than(self, other: Self) -> bool

fn audit(record: dyn Versioned) -> bool:
    if record.version() > 0:
        return true
    record.newer_than(record)  # error: dyn-member-unavailable
```

> **Why.** One concrete trait instantiation, such as `dyn Repository[User]`, fixes
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

data Pup:
    name: string

impl Animal for Pup:
    fn name(self) -> string: self.name

impl Dog for Pup:
    fn bark(self) -> string: "woof"

fn name_of(animal: dyn Animal) -> string:
    animal.name()

fn demo(pup: Pup) -> string:
    name_of(pup)   # a Pup widens to Animal
```

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity).

### `Any`

1. r[types.any] `Any` is the built-in universal empty trait.
2. r[types.any.all] Every value type, including an optional type, satisfies `Any` automatically.
3. r[types.any.never] `never` satisfies `Any` too. A `void` value needs no rule of its own, because it is the tuple `()`.
4. r[types.any.erase] As a value type, `dyn Any` erases the concrete type.
5. r[types.any.mut] `mut dyn Trait` and `mut dyn Any` preserve mutable access to an erased composite root.
6. r[types.any.one-way] `Any` erasure is one-way.
7. r[types.any.inspectable] A value erased to the sealed trait `std.inspect.Inspectable` instead keeps a runtime record of its concrete type, which `downcast` compares exactly.

```hd
fn demo(value: dyn Any) -> string:
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
| r[types.sealed.anyval-values] `AnyVal` | `bool`, `char`, the integer types, `f32`, `f64`, `string`, tuples (`void` included), enums (optionals and `Result` included), and function types | These values have no identity. |
| r[types.sealed.anyref-values] `AnyRef` | data types, `List`, `Map`, dynamic trait value types, `Any`, suspensions, and runtime handles | These values have identity. |

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

1. r[types.map-key.declared-bound] `Map` is declared as `Map[K < Eq & Hash, V]`. A key type that does not implement both traits fails that bound, as any unmet bound does. Error: `unsatisfied-trait-bound`.
2. r[types.map-key.no-mut] A `mut T` key type is an error, even when `T` implements both traits. Error: `invalid-map-key`.
3. r[types.map-key.hash] `Hash` is a standard-library trait in `std.hash`.
4. r[types.map-key.user] User-defined data and enum types can become keys by explicitly implementing or deriving both traits.
5. r[types.map-key.builtin-hash] Standard-library implementations of `Hash` cover `bool`, integers, `char`, `string`, and tuples, optionals, results, and lists of hashable elements. So `List[T]` implements `Hash` when `T < Hash`, and `Result[T, E]` when `T < Hash` and `E < Hash`.
6. r[types.map-key.unhashable] Maps, floating-point values, functions, suspensions, dynamic trait values, and `Any` do not have built-in `Hash`.
7. r[types.map-key.user-enums] User data and every user enum, including a payload-free one, require an explicit or derived implementation of both traits.
8. r[types.map-key.float-no-hash] Floating-point types implement `Eq` but not `Hash`, so they are not valid map keys.
9. r[types.map-key.no-map-hash] Consequently maps have no built-in hash and impose no order-independent map-hash obligation.

```text
data UserId:
    value: string

@derive(Eq, Hash)
data Session:
    token: string

fn setup() -> void:
    let users: Map[UserId, string] = {}  # error: unsatisfied-trait-bound
    let open: Map[mut Session, i32] = {}  # error: invalid-map-key
    pass
```

> **Why.** A NaN key is unequal even to itself, so no lookup could find it.

> **Note.** A list key can still change through a `mut` alias, as a data
> key with a derived `Hash` already can. Its entry then becomes a
> [stale entry](#r-types.map.stale).

> **Why.** The key traits are an ordinary bound, so a missing trait reads
> as any other unmet bound. A `mut` key stays an error: code holding it
> could change the key and leave a [stale entry](#r-types.map.stale).

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
    let keys: mut List[string] = []
    for entry in m:
        keys.push(entry._0)
    keys   # ["b", "a"] or ["a", "b"]: the map's order, the same in every run
```

> **Why.** A deterministic order keeps output and tests reproducible.
> Leaving it unspecified lets the map use a faster layout and hash, as
> Go and Rust do. Code that needs an order sorts the keys.

### Mutated Keys

1. r[types.map.key-view] A readonly key view does not freeze the object.
2. r[types.map.no-reindex] If another mutable alias changes a stored key's equality or hash after insertion, the map does not automatically reindex it.
3. r[types.map.stale] The entry can remain visible during iteration yet be unreachable by lookup or removal with the mutated key: a stale entry.
4. r[types.map.no-repair] Such mutation does not trigger a compile-time error or an automatic repair.

```hd
use std.hash.Hash

@derive(Eq, Hash)
data Tag:
    id: i32

fn demo() -> void:
    let table: mut Map[Tag, string] = {}
    key := Tag { id: +1 }
    table[key] = "one"   # keyed by id through Eq and Hash
```

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
2. r[types.lct.unique] The compiler computes a unique least common type of the values' types using only the conversions that `types.lct.conversions` lists.
3. r[types.lct.conversions] Permission weakening, declared readonly variance, and optional injection may contribute. No other conversion does: no numeric conversion, so `[small, large]` with an `i16` and an `i64` has no common type. Error: `no-common-type`.
4. r[types.lct.optional] Optional injection adds one layer, as [`types.assign.optional`](#r-types.assign.optional) states. So an `i32` and an `i32?` join to `i32?`, in a list literal, a value-producing `if` or `match`, and an inferred result type alike.
5. r[types.lct.optional.one-layer] A `T` and a `T??` have no common type, because injection never adds two layers. Error: `no-common-type`.
6. r[types.lct.no-combine] Least-common-type inference never combines permission weakening with a variance step for the same candidate conversion.
7. r[types.lct.row-union-every-site] At every construct in the table, function values with different rows are first widened to the union of their rows ([Row Union In Literals](11-requirements-and-suspension.md#row-union-in-literals)).
8. r[types.lct.never.dropped] The least common type first drops every value of type `never`, then joins the rest. So `if ok: 1 else: return .None` has type `i32`.
9. r[types.lct.never.all] When every value has type `never`, the least common type is `never`.

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

1. r[types.lct.no-any] The compiler never falls back to `dyn Any` merely to make heterogeneous values type-check.
2. r[types.lct.no-trait-value] Unconstrained inference also does not introduce a dynamic trait-value conversion.
3. r[types.lct.no-supertrait-widening] It also never widens a dynamic trait value to a supertrait value, so values of two child traits of one supertrait have no common type.
4. r[types.lct.expected-trait] An expected type such as `List[dyn Display]` or `Map[K, dyn Display]` may request that conversion explicitly.
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
`Named`; `let items: List[dyn Named] = [shown, tagged]` is valid.

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

data User: pass
data Post: pass

data Job[A, B]:
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
| Identity-free composites | `string`, tuples, enum values (including `Result` and optionals), function values | none |
| Reference values | data values, lists, maps, trait values, `Any`, suspensions, runtime handles | allocation identity, or one canonical identity for a fieldless data value |

The reference values are exactly the implementers of the sealed `AnyRef`
trait. The scalar values and identity-free composites are exactly the
implementers of the sealed `AnyVal` trait.

Values without identity are immutable at their top level, so storing one by
copy or by reference cannot be observed. An implementation may copy, share,
or deduplicate such a value freely. A fieldless data value stores no data and
has one canonical identity.

Converting a value without identity to a trait value or `Any` may box it.
The box has no identity of its own, as
[Expressions](05-expressions.md#unary-and-binary-operators) specifies.

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
| [`types.generic.polymorphic-recursion`](#r-types.generic.polymorphic-recursion) | the number of instantiations, which is always finite |
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

- A data type is a record of its fields. A field whose type has a value
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

Permission weakening `mut U -> U` preserves representation. Conversion to a
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
