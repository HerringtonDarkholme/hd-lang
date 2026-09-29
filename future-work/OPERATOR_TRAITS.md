# Operator Traits: Survey And Design Options

Status: design exploration, 2026-09-28; nothing here is decided or in the
specification. Since this record was written, Literal Suffixes L11 removed
`std.ops.LiteralSuffix`: a suffix is a function marked `@num_suffix`, so
the comparisons below with `LiteralSuffix[In, Out]` are historical.

The owner decided that hd plans operator traits in `std.ops`, on Rust's
model, so library types such as `Duration` support `5s + 3s` and `-d`
([Literal Suffixes L5](LITERAL_SUFFIXES.md#owner-decisions),
[Open Issues: Operator Traits](OPEN_ISSUES.md#operator-traits)). This
record asks how. It reviews [Unary And Binary Operators](../spec/05-expressions.md#unary-and-binary-operators),
[Numeric Conversions](../spec/04-type-system.md#numeric-conversions),
[Newtypes](../spec/04-type-system.md#newtypes), and
[Traits](../spec/09-traits.md), in particular
[Implementation Ownership](../spec/09-traits.md#implementation-ownership) and
[Instantiations Of One Generic Trait](../spec/09-traits.md#instantiations-of-one-generic-trait).
It also reviews the newtype forwarding rule of
[Typed Derivation M21 R3-5](TYPED_DERIVATION.md).

## Owner Decisions

Decided 2026-09-29.

1. **OP1: Rust's shape.** `trait Add[Rhs]` has an associated `type Out`
   and `fn add(self, rhs: Rhs) -> Out`. One output per (type, right
   operand) pair follows from existing impl uniqueness.
2. **OP2: primitives implement the traits through std impls whose bodies
   are compiler intrinsics.** `impl Add[i32] for i32` lives in std, and
   its body is a toolchain-internal intrinsic (see Runtime And Library,
   "Prototype Host Function Declarations"). So a primitive `+` compiles
   straight to `i32.add` plus the overflow check. Generic code bounded by
   `T < Add[T]` accepts `i32`. Primitive-with-primitive operators may
   skip trait search entirely, which also avoids Swift-style type-check
   blowups.
3. **OP3: the 12 operators** `+ - * / %`, unary `-`, `& | ^ ~` and
   `<< >>`. No `**` trait.
4. **OP4: no literal-typing rule for a left-hand literal.** `3 * d` with
   `d: Duration` is not typed from the one matching impl. Write `d * 3`.
   A left literal works only when both operands are the same primitive
   type.

5. **OP5: compound assignment through `AddAssign`-style traits,** as in
   Rust: `total += x` calls `add_assign(mut self, rhs: Rhs)`. There is one
   trait per compound operator. Consequence to handle in the apply pass:
   hd `data` values are shared references, so in-place `+=` on a `data`
   value is visible through every alias, and the receiver needs `mut`.
   Primitives and other value types (`AnyVal`) aren't affected. Open
   details for the apply pass:
   - whether `a += b` falls back to `a = a + b` when no assign impl exists;
   - how std implements the assign traits for primitives.
6. **OP6: newtype operators are hand-written.** A newtype doesn't inherit
   its base's operators, and there is no operator derive.
7. **OP7: user indexing through `Index` and `IndexSet` traits,** so
   `grid[i]` and `grid[i] = v` work for user types. The trait signatures
   (key type, output, mutability) are left to the apply pass, with a
   recommendation.
8. **OP8: a supertrait list may bind an associated output,** as in
   `trait Integer < Add[Self, Out = Self] & Sub[Self, Out = Self]`. This
   retires `trait.binding.rejected` for supertraits.

9. **OP9 (2026-09-29): numeric trait families.**
   - `Num < AnyVal`, and `Num`, `Integer` and `Float` are **sealed**. They
     stand for the built-in primitive number types only, and only std
     implements them. Newtypes (such as `type Meters(i64)`) and library
     number types (such as a `BigInt`) are not `Num`. They implement the
     individual operator traits by hand (OP6).
   - `Num` has `+ - * / %`, `zero()` and `one()`. Generic code writes
     `T::zero()` and `T::one()`; there are no polymorphic literals. `/` and
     `%` keep each type's own meaning: integer division truncates and
     panics on zero, and float division gives infinity or NaN.
   - The two families are `Integer < Num` (i8 through u64: `Ord`, bitwise
     and shifts) and `Float < Num` (f32, f64: `PartialOrd`, `is_nan` and
     the like).
   - The exact member lists are left to the apply pass. They build on OP8's
     supertrait `Out` binding.
   - `Num` also has `fn from_i64(n: i64) -> Self`, which converts the way
     a cast does: integers wrap, and floats take the nearest value. It lets
     generic code build constants, as in
     `@num_suffix fn k[N < Num](n: N) -> N: n * N::from_i64(1000)`. A
     generic `@num_suffix` function over `N < Num` is valid (L20, L22): `N`
     comes from the literal or from the expected type.

## Contents

- [Owner Decisions](#owner-decisions)
- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Use Cases](#use-cases)
- [Survey](#survey)
- [Rules Shared By Options 1-3](#rules-shared-by-options-1-3)
- [Option 1: Operand Argument, Associated Output](#option-1-operand-argument-associated-output)
- [Option 2: Operand And Output As Trait Arguments](#option-2-operand-and-output-as-trait-arguments)
- [Option 3: Same-Type Operators](#option-3-same-type-operators)
- [Option 4: Sealed Operator Traits](#option-4-sealed-operator-traits)
- [Which Operators](#which-operators)
- [Primitive Types](#primitive-types)
- [Mixed Operands And Ownership](#mixed-operands-and-ownership)
- [Compound Assignment](#compound-assignment)
- [Newtypes](#newtypes)
- [Indexing](#indexing)
- [Overflow And Effects](#overflow-and-effects)
- [Comparison Operators](#comparison-operators)
- [Dynamic Safety And Representation](#dynamic-safety-and-representation)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Problem

Which operators may a library type implement, and through which trait
shape? The answer must say how mixed operands such as `d * 3` and `3 * d`
resolve, and what the output type of `t2 - t1` is. It must also say how
primitive operators relate to the traits, and what `+=` and newtypes do.

## What hd Has Today

| Topic | Today | Rule |
| --- | --- | --- |
| User operators | None except comparison: arithmetic and bitwise operators are built in for numeric types | [`expr.op.builtin`](../spec/05-expressions.md#r-expr.op.builtin), [`expr.op.traits`](../spec/05-expressions.md#r-expr.op.traits), [`expr.unsupported.overloading`](../spec/05-expressions.md#r-expr.unsupported.overloading) |
| `true + false` | `type-mismatch` | [`expr.arith.non-numeric`](../spec/05-expressions.md#r-expr.arith.non-numeric) |
| `string + string` | Concatenation, the one non-numeric arithmetic form | [`expr.arith.string`](../spec/05-expressions.md#r-expr.arith.string) |
| `==`, `<` | Call `Eq.eq` and `PartialOrd.partial_cmp`, same-type only | [`expr.eq.calls-eq`](../spec/05-expressions.md#r-expr.eq.calls-eq), [`expr.ord.partial-cmp`](../spec/05-expressions.md#r-expr.ord.partial-cmp), [`trait.cmp.operators`](../spec/09-traits.md#r-trait.cmp.operators) |
| `i16 + i64` | Widens within one signedness family; a literal adopts the other operand's type | [`types.num.binary.widen`](../spec/04-type-system.md#r-types.num.binary.widen), [`types.num.binary.literal`](../spec/04-type-system.md#r-types.num.binary.literal) |
| Overflow | Integer arithmetic is checked and panics | [`types.arith.checked`](../spec/04-type-system.md#r-types.arith.checked) |
| Compound assignment | None; `+=` is one token used only by derivation member lines | [`lex.op.plus-equals`](../spec/01-lexical-structure.md#r-lex.op.plus-equals) |
| Indexing | Built in for `List` and `Map` only | [List Indexing](../spec/05-expressions.md#list-indexing), [Map Indexing](../spec/05-expressions.md#map-indexing) |
| Generic traits | Allowed; the spec's own example is `trait Add[T]` | [`trait.decl.generic`](../spec/09-traits.md#r-trait.decl.generic) |
| Associated types | Allowed, with bound bindings `I < Supplier[Item = T]`; not in supertrait lists | [`trait.assoc.declare`](../spec/09-traits.md#r-trait.assoc.declare), [`trait.binding.rejected`](../spec/09-traits.md#r-trait.binding.rejected) |
| `impl Add[Money] for i32` | Allowed in the package that owns `Money` (trait-argument ownership) | [`trait.own.argument.example`](../spec/09-traits.md#r-trait.own.argument.example) |
| `impl Add[i32] for Money` and `impl Add[Money] for Money` | Do not overlap; a call picks the instantiation whose argument fits, with a literal-default tie-break | [Overlap](../spec/09-traits.md#overlap), [`trait.resolve.one-fit`](../spec/09-traits.md#r-trait.resolve.one-fit), [`trait.resolve.literal-default`](../spec/09-traits.md#r-trait.resolve.literal-default) |
| Newtypes | Inherit no implementation; `@derive` forwards only where `Self` is the receiver, plain, `Self?`, `Result[Self, E]`, or `List[Self]` | [`types.newtype.no-inherit`](../spec/04-type-system.md#r-types.newtype.no-inherit), [`trait.derive.newtype.self-positions`](../spec/09-traits.md#r-trait.derive.newtype.self-positions) |
| Dynamic safety | Associated types, and `Self` outside the receiver, make a trait unusable as a value type | [Dynamic Safety](../spec/09-traits.md#dynamic-safety) |
| `std.ops` | One planned member, `LiteralSuffix[In, Out]`, with the output as a trait argument | [Literal Suffixes L2](LITERAL_SUFFIXES.md#owner-decisions) |
| `Duration` | Drafted as `data Duration: nanos: i64` with `Timestamp.plus` and `since` methods | [STDLIB `std.time`](STDLIB.md#stdtime) |

Two facts shape every option. The spec already uses `Add[Money]` in its
ownership and overlap examples, so generic operand traits fit the existing
coherence rules. The existing instantiation choice already makes
`price.add(5)` pick `Add[i32]`, so mixed operands need little new
resolution machinery.

## Use Cases

Every option is shown on the same six cases.

| # | Case | Written today |
| --- | --- | --- |
| U1 | Same-type sum and negation on a library type | `a.plus(b)`, `a.negate()` |
| U2 | Scaling: `d * 3` and `3 * d` | `d.times(3)` |
| U3 | Output differs from the operands: `t2 - t1` is a `Duration` | `t2.since(t1)` |
| U4 | Generic numeric code: `sum` over any addable `T` | Impossible with `+`; methods of `std.num.Integer` |
| U5 | A newtype over a number: `Meters + Meters` | `Meters(f64(a) + f64(b))` |
| U6 | Accumulating in a loop | `total = total + item` |

## Survey

| Language | Overloadable operators | Declaration shape | Mixed operands, output | Compound assignment | Primitives | Source |
| --- | --- | --- | --- | --- | --- | --- |
| Rust | `+ - * / %`, unary `- !`, `& \| ^ << >>`, `[]`, compound forms; not `&&`, `\|\|`, `=` | Traits in `std::ops`: `Add<Rhs = Self>` with an associated `type Output` | Yes: `impl Mul<Duration> for u32`; output fixed by `(Self, Rhs)` | `AddAssign` with `&mut self`; for two primitives it is built in | Operators are built into the language for primitives; std impls exist for generic code | [std::ops](https://doc.rust-lang.org/std/ops/index.html), [Add](https://doc.rust-lang.org/std/ops/trait.Add.html), [operator expressions](https://doc.rust-lang.org/reference/expressions/operator-expr.html) |
| C# | Unary, arithmetic, bitwise, shifts, comparisons in pairs; not `&&`, `\|\|`, `[]` (indexers instead) | `public static T operator +(T a, T b)` in a type that is one operand | Yes; generic math uses `IAdditionOperators<TSelf, TOther, TResult>`, output as a type parameter | Before C# 14, `x += y` is `x = x + y`; C# 14 adds instance `operator +=` | `int` implements the generic math interfaces | [Operator overloading](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/operators/operator-overloading), [IAdditionOperators](https://learn.microsoft.com/en-us/dotnet/api/system.numerics.iadditionoperators-3) |
| Swift | Any operator, plus new custom operators with precedence groups | `static func +(lhs:rhs:)` inside the type; protocols `AdditiveArithmetic`, `Numeric` | Yes: `Duration * Int` | `static func +=(lhs: inout T, rhs: T)` | `+` traps on overflow; `&+` wraps | [Advanced Operators](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/advancedoperators/), [Duration](https://developer.apple.com/documentation/swift/duration) |
| Kotlin | A fixed set, each mapped to a named method | `operator fun plus(other: T)` member or extension | Yes: `operator fun Int.times(duration: Duration)` | `plusAssign` if present, else `a = a + b`; both available is an error | Built in | [Operator overloading](https://kotlinlang.org/docs/operator-overloading.html), [kotlin.time.times](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.time/times.html) |
| Scala | Any symbolic method name | Operators are methods: `def +(that: T): T` | Yes, left operand's method | `a += b` is `a = a + b` when no `+=` method exists | Methods on `Int` | [Tour: operators](https://docs.scala-lang.org/tour/operators.html), [Spec 6.12.4](https://scala-lang.org/files/archive/spec/2.13/06-expressions.html#assignment-operators) |
| Haskell | Any operator symbol, with fixity declarations | Type class `Num a`: `(+), (-), (*), negate, abs, signum, fromInteger` | No: both operands and the result are `a` | None | `Int` is an ordinary `Num` instance | [Prelude: Num](https://hackage.haskell.org/package/base/docs/Prelude.html#t:Num) |
| MoonBit | `+ - * / %`, unary `-`, `& \| ^ << >>`, `==`; `_[_]` by method alias | Traits `Add`, `Sub`, `Mul`, ... : `impl Add for T with add(self : T, other : T) -> T` | No: same type in and out | None | Built in | [MoonBit: operator overloading](https://docs.moonbitlang.com/en/latest/language/methods.html) |
| Go | None | n/a | `time.Duration` is a named `int64`, so built-in `5 * time.Second` works | Built in only | Built in | [Go FAQ: overloading](https://go.dev/doc/faq#overloading), [time.Duration](https://pkg.go.dev/time#Duration) |
| Zig | None | n/a | n/a | Built in only | Built in; "no hidden control flow" | [Zig overview](https://ziglang.org/learn/overview/#no-hidden-control-flow) |

### Takeaways

1. **Mainstream typed languages allow mixed operands.** Rust, C#, Swift,
   and Kotlin all scale a `Duration` by an integer, on both sides. Only
   Haskell and MoonBit restrict operators to one type.
2. **The output type is either fixed by the operands or a free argument.**
   Rust fixes it with an associated type, so `a + b` has one type. C#
   passes `TResult` as a type parameter, since it has no associated types.
3. **Compound assignment splits three ways.** Rust and C# 14 use separate
   in-place operators. Scala and pre-14 C# rewrite `a += b` to
   `a = a + b`. Kotlin allows both and reports an error when both apply.
4. **Primitives stay built in everywhere.** Even Rust, whose `i32`
   implements `Add`, defines primitive operators in the language, not
   through the trait.

Kotlin's named-method convention (`operator fun plus`) and Swift's
operator functions are not options here. The owner chose traits in
`std.ops` (L2, L5), and no new evidence argues against that.

## Rules Shared By Options 1-3

These rules hold whatever the trait shape. Option 4 uses them too, but only
standard library types reach them.

1. **Desugaring.** When an operand is not a primitive numeric type after
   literal typing, `a + b` means the trait-qualified call
   `std.ops.Add[...]::add(a, b)`. No `use std.ops.Add` is needed.
2. **Order.** The operands are evaluated left to right, then the method is
   called, as [`expr.order.left-to-right`](../spec/05-expressions.md#r-expr.order.left-to-right)
   already says.
3. **Instantiation choice.** When the left operand's type implements
   several instantiations, the existing rules
   [`trait.resolve.fits`](../spec/09-traits.md#r-trait.resolve.fits) to
   [`trait.resolve.no-fit`](../spec/09-traits.md#r-trait.resolve.no-fit)
   choose. So `d * 3` checks `3` against `i64` in `Mul[i64]`.
4. **Unary operators.** `-a` means `Neg::neg(a)` and `~a` means
   `BitNot::bit_not(a)`.
5. **Fixed grammar.** Precedence and associativity never change, and no
   operator symbols are added.
6. **Suffixed literals.** `-5s` keeps L4: the minus folds into the literal
   before `from_literal`, so `Neg` is not called.
7. **Never overloaded.** `&&`, `||`, prefix `!`, `is`, `=`, `:=`, `?`, and
   `**` keep their built-in meaning. `==` and the relational operators keep
   calling `Eq` and `PartialOrd`.
8. **No implementation.** An operand type with no implementation is still
   `type-mismatch`; the message names the missing trait.

## Option 1: Operand Argument, Associated Output

**Idea.** Rust's shape. The right operand is a trait argument, so one type
may implement `Mul[i64]` and `Mul[f64]`. The output is an associated type,
so the operands fix it.

```text
# std.ops
pub trait Add[Rhs]:
    type Out
    fn add(self, other: Rhs) -> Self::Out

pub trait Mul[Rhs]:
    type Out
    fn mul(self, other: Rhs) -> Self::Out

pub trait Neg:
    type Out
    fn neg(self) -> Self::Out
```

```text
# std.time
impl Add[Duration] for Duration:
    type Out = Duration
    fn add(self, other: Duration) -> Duration:
        Duration { nanos: self.nanos + other.nanos }

impl Neg for Duration:
    type Out = Duration
    fn neg(self) -> Duration:
        Duration { nanos: -self.nanos }

impl Mul[i64] for Duration:
    type Out = Duration
    fn mul(self, other: i64) -> Duration:
        Duration { nanos: self.nanos * other }

impl Mul[Duration] for i64:
    type Out = Duration
    fn mul(self, other: Duration) -> Duration:
        other * self

impl Sub[Timestamp] for Timestamp:
    type Out = Duration
    fn sub(self, other: Timestamp) -> Duration:
        Duration { nanos: self.unix_nanos - other.unix_nanos }
```

```text
use std.ops.Add
use std.time.{Duration, Timestamp}

fn budget(start: Timestamp, now: Timestamp) -> Duration:   # U1, U2, U3
    limit := Duration::seconds(5) + Duration::milliseconds(250)
    left := limit - (now - start)
    3 * -left

fn sum[T < Add[T, Out = T]](items: List[T], zero: T) -> T:  # U4, U6
    let total = zero
    for item in items:
        total = total + item
    total

type Meters(f64)

impl Add[Meters] for Meters:                                # U5
    type Out = Meters
    fn add(self, other: Meters) -> Meters:
        Meters(f64(self) + f64(other))
```

Rules added: the eight shared rules; twelve traits in `std.ops`; the
left-literal rule from [Mixed Operands](#mixed-operands-and-ownership).
Rules changed: `expr.op.traits`, `trait.cmp.operators`, and
`expr.unsupported.overloading` stop saying comparison is the only case.

Soundness conditions: `Out` is unique per `(Self, Rhs)` because two
`impl Add[Duration] for Duration` already break
[`trait.impl.unique`](../spec/09-traits.md#r-trait.impl.unique). So
`x := a + b` always has one type, and no later implementation can change
it.

| Aspect | Effect |
| --- | --- |
| Generic bound | `T < Add[T, Out = T]`, with the existing binding syntax |
| Supertrait alias | `trait Integer < Add[Self, Out = Self]` is rejected today by [`trait.binding.rejected`](../spec/09-traits.md#r-trait.binding.rejected); see question 8 |
| Name clash | Every operator trait declares `Out`, so `T::Out` is ambiguous under two bounds; the bindings avoid naming it |
| Dynamic safety | Not dynamically safe (associated type); nobody needs `Add` as a value |
| `LiteralSuffix[In, Out]` | Differs: there the output is a trait argument |
| Wasm GC | A static call; nothing new |

## Option 2: Operand And Output As Trait Arguments

**Idea.** C#'s generic math shape. Both the right operand and the output
are trait arguments, as in the decided `LiteralSuffix[In, Out]`. No
associated type is involved.

```text
# std.ops
pub trait Add[Rhs, Out]:
    fn add(self, other: Rhs) -> Out

pub trait Neg[Out]:
    fn neg(self) -> Out
```

```text
# std.time
impl Mul[i64, Duration] for Duration:
    fn mul(self, other: i64) -> Duration:
        Duration { nanos: self.nanos * other }

impl Sub[Timestamp, Duration] for Timestamp:
    fn sub(self, other: Timestamp) -> Duration:
        Duration { nanos: self.unix_nanos - other.unix_nanos }
```

```text
use std.ops.{Add, Sub}

fn sum[T < Add[T, T]](items: List[T], zero: T) -> T:
    let total = zero
    for item in items:
        total = total + item
    total

pub trait Additive < Add[Self, Self] & Sub[Self, Self]
```

Rules added: the shared rules and twelve traits, as in option 1. One more
choice is needed. Either (2a) accept that a type may implement
`Add[Duration, Duration]` and `Add[Duration, Timestamp]`, or (2b) add a
coherence rule allowing one `Out` per `(Self, Rhs)`.

Soundness conditions: under 2a, `x := a + b` with two fitting outputs is
`ambiguous-method` by the existing rules. With an expected type,
[`trait.resolve.fits.expected`](../spec/09-traits.md#r-trait.resolve.fits.expected)
picks the one whose output fits. That is sound, but adding a second output
later breaks existing call sites. Rule 2b removes that at the cost of one
new coherence rule.

| Aspect | Effect |
| --- | --- |
| Generic bound | `T < Add[T, T]` |
| Supertrait alias | `trait Additive < Add[Self, Self]` needs no binding, assuming `Self` is allowed as a supertrait argument (no rule says) |
| Dynamic safety | Dynamically safe: `Self` appears only as the receiver |
| `LiteralSuffix[In, Out]` | Same shape |
| Evolution | Under 2a, an added output can make existing `a + b` ambiguous |

## Option 3: Same-Type Operators

**Idea.** MoonBit's and Haskell's shape. Every operator takes and returns
`Self`. Traits have no arguments and no associated types.

```text
# std.ops
pub trait Add:
    fn add(self, other: Self) -> Self

pub trait Neg:
    fn neg(self) -> Self
```

```text
use std.ops.Add
use std.time.{Duration, Timestamp}

fn budget(start: Timestamp, now: Timestamp) -> Duration:
    limit := Duration::seconds(5) + Duration::milliseconds(250)
    left := limit - now.since(start)
    -left.times(3)

fn sum[T < Add](items: List[T], zero: T) -> T:
    let total = zero
    for item in items:
        total = total + item
    total

@derive(Add)
type Meters(f64)
```

Rules added: the shared rules except rule 3; about twelve traits. No
left-literal rule is needed, because a literal operand takes the other
operand's type or fails.

Soundness conditions: none beyond ordinary trait calls. `@derive(Add)` on a
newtype needs no new rule under
[`trait.derive.newtype.self-positions`](../spec/09-traits.md#r-trait.derive.newtype.self-positions),
since `Self` appears only as the receiver and as plain `Self`. The
intrinsic list or a std template must still admit `Add`.

| Aspect | Effect |
| --- | --- |
| U2, U3 | Not operators: `d.times(3)` and `t2.since(t1)` stay methods |
| Generic bound | `T < Add`; `trait Integer < Add & Sub & Mul` works today |
| Dynamic safety | Not dynamically safe (`Self` as a parameter) |
| Evolution | Moving to option 1 later changes every trait's arity: a breaking change |

## Option 4: Sealed Operator Traits

**Idea.** The radical simplification. The `std.ops` operator traits join
the [sealed traits](../spec/09-traits.md#sealed-traits): only the standard
library implements them. `Duration`, `Timestamp`, and `std.decimal` get
operators; user types keep methods.

```text
use std.ops.Add

data Money:
    cents: i64

impl Add[Money] for Money:  # error: sealed-trait-implementation
    type Out = Money
    fn add(self, other: Money) -> Money:
        Money { cents: self.cents + other.cents }
```

Rules added: the shared rules, applied only to std types, and one row per
trait in the sealed table. Std's traits may take option 1's shape
internally; users can still write bounds such as `T < Add[T, Out = T]`.

Soundness conditions: none new. Every overloaded operator is visible in one
package, which removes the "hidden call" concern for user code entirely.

| Aspect | Effect |
| --- | --- |
| U1-U4 | Work for std types only |
| U5 | `Meters` cannot have `+` |
| Libraries | `BigInt` (an ordinary package, STDLIB decision 9), money, and vector types get no operators |
| Evolution | Unsealing later is compatible: it only adds programs |

## Which Operators

| Operator | Trait | Method | Notes |
| --- | --- | --- | --- |
| `a + b` | `Add` | `add` | `string + string` stays built in |
| `a - b` | `Sub` | `sub` | |
| `a * b` | `Mul` | `mul` | |
| `a / b` | `Div` | `div` | |
| `a % b` | `Rem` | `rem` | Named for [`types.arith.remainder`](../spec/04-type-system.md#r-types.arith.remainder) |
| `-a` | `Neg` | `neg` | |
| `a & b`, `a \| b`, `a ^ b` | `BitAnd`, `BitOr`, `BitXor` | `bit_and`, `bit_or`, `bit_xor` | Flag sets such as `type Perms(u32)`; big integers |
| `~a` | `BitNot` | `bit_not` | Rust's `Not` also covers `!`; hd keeps `!` for `bool` only |
| `a << n`, `a >> n` | `Shl`, `Shr` | `shl`, `shr` | Big integers, bit vectors |

Out of the set: `**` (its integer exponent rules are special, and Rust has
no power operator), unary `+`, `!`, `&&`, `||`, `is`, and `[]` (see
[Indexing](#indexing)). A smaller set keeps only the first six rows.

## Primitive Types

| Choice | Meaning | Effect |
| --- | --- | --- |
| P1: built in, plus std implementations | Concrete primitive operands keep the built-in rules. Std also declares same-type implementations such as `impl Add[i32] for i32` | `i16 + i64` still widens; `sum[i32]` works; generic code gets no widening. Rust's model |
| P2: operators are the implementations | Every primitive operator is a trait call | Widening needs cross-width implementations; the literal rule `types.num.binary.literal` must be rebuilt on trait choice |
| P3: built in only | No std implementations for primitives | `sum[i32]` fails U4; generic code keeps `std.num.Integer` methods |

Under P1, std's implementation body behaves exactly like the built-in
operator: checked, panicking on overflow. Std declares no `Add[string]`, so
`string + string` stays a built-in form and `sum` over strings is rejected.

## Mixed Operands And Ownership

Coherence needs no change. Std owns the primitives and `Duration`, so it may
write both `Mul[i64] for Duration` and `Mul[Duration] for i64`. A library
that owns `Money` may write `impl Mul[Money] for i32`, as
[`trait.own.argument.example`](../spec/09-traits.md#r-trait.own.argument.example)
already allows. No package may write `impl Add[i32] for i32`: that is an
`orphan-impl`.

Dispatch is on the left operand, as in Rust and Kotlin. `d * 3` works by
shared rule 3. `3 * d` needs one more rule, because the left literal has no
type yet:

- **Left-literal rule.** When the left operand is an untyped literal and
  the right is not a primitive number, the literal takes the primitive type
  that implements the trait for the right operand's type. If several do,
  the default `i32` or `f64` wins when it is among them. Otherwise the call
  is `ambiguous-method`.

Rust gets the same effect from type inference: `2 * d` infers `u32`
because std has only `impl Mul<Duration> for u32`. Without the rule, users
write `d * 3` or `i64(3) * d`.

## Compound Assignment

hd has no compound assignment, so this is a separate feature. The choices
differ in what `total += item` does to aliases.

| Choice | Meaning | Effect |
| --- | --- | --- |
| C1: none | Keep writing `total = total + item` | No change; `+=` stays a derivation-line token |
| C2: rewrite | `p op= e` means `p = p op e`, with `p`'s receiver and index evaluated once | Scala and pre-14 C#. New tokens `-=`, `*=`, and so on change how `a-=b` lexes today |
| C3: assign traits | `AddAssign[Rhs]` with `fn add_assign(mut self, other: Rhs) -> void` | Rust and C# 14. `Money` is data, a reference: `let t = price; t += tax` would change `price` too. An `AnyVal` newtype cannot be changed in place at all |

C3 conflicts with hd's value model: data values are references with
identity ([`expr.is.heap`](../spec/05-expressions.md#r-expr.is.heap)), so an
in-place `+=` is visible through every alias. C1 and C2 build a new value
and store it, so aliases never see the change.

```text
fn total(items: List[i64]) -> i64:
    let sum: i64 = 0
    for item in items:
        sum = sum + item
    sum
```

## Newtypes

A newtype inherits no implementation
([`types.newtype.no-inherit`](../spec/04-type-system.md#r-types.newtype.no-inherit)),
and should not start: `Meters * Meters` is an area, not `Meters`. The
choices are how a newtype opts in.

| Choice | Meaning | Effect |
| --- | --- | --- |
| N1: hand-written | Write `impl Add[Meters] for Meters` (option 1's U5 block) | Four lines per operator; no new rule |
| N2: derivable | `@derive(Add, Sub, Neg)` on a newtype generates the same-type implementation from the base type's | Under options 1 and 2, M21 R3-5 does not cover trait arguments, so a new forwarding rule is needed. Under option 3 no rule is needed |
| N3: automatic | Every newtype gets its base's operators | Breaks `types.newtype.no-inherit` and allows `Meters * Meters` |

## Indexing

`receiver[index]` is built in for `List` and `Map`, with permission rules
for readonly roots ([`expr.index.list.read`](../spec/05-expressions.md#r-expr.index.list.read)).
User indexing would add `Index[K]` for reads and an `IndexSet[K, V]` with
`mut self` for writes, as Kotlin's `get` and `set` do. Both would need the
place and readonly-root rules extended. A `Grid` type meanwhile writes
`grid.get(r, c)` and `grid.set(r, c, v)`.

## Overflow And Effects

- An implementation must match the trait method's exact signature
  ([`trait.impl.signature`](../spec/09-traits.md#r-trait.impl.signature)).
  The std methods declare no requirement row and do not suspend. So an
  operator never needs a provider and never suspends; it can only compute
  or panic.
- Overflow is the implementation's choice. Std's `Duration` `+` should
  panic on `i64` overflow, as integers do, with `checked_add` beside it.
- A library may write a wrapping type, such as Rust's `Wrapping[T]`, whose
  `Add` wraps. The core needs nothing for it.

## Comparison Operators

`==` and `!=` already call `Eq.eq`, and the relational operators call
`PartialOrd.partial_cmp`. Both are same-type only, and `Eq` is the only
equality trait ([`trait.cmp.one-eq`](../spec/09-traits.md#r-trait.cmp.one-eq)).
This record keeps that: no `PartialEq[Rhs]`, so `money == 5` stays an
error. The comparison traits stay in `std.cmp`, not `std.ops`.

## Dynamic Safety And Representation

Operator traits are used as bounds, never as value types, so their dynamic
safety has no practical effect. Every operator on a non-primitive operand
is a static call, which the compiler may inline; nothing changes for Wasm
GC. `Duration` is drafted as `data`, so each `a + b` allocates. Declaring
it as a newtype over `i64` would make it an `AnyVal` with no allocation;
that is a std question, not an operator question.

## Comparison

| | 1: Operand arg, associated `Out` | 2: Operand and output args | 3: Same type | 4: Sealed |
| --- | --- | --- | --- | --- |
| U1 `a + b`, `-d` | yes | yes | yes | std types only |
| U2 `d * 3`, `3 * d` | yes; `3 * d` needs the left-literal rule | yes; same | methods | std types only |
| U3 `t2 - t1: Duration` | yes | yes | method | std types only |
| U4 generic `sum` | `T < Add[T, Out = T]` | `T < Add[T, T]` | `T < Add` | `T < Add[T, Out = T]` |
| U5 newtype | hand-written, or N2 with a new rule | hand-written, or N2 with a new rule | `@derive(Add)` under existing R3-5 | not possible |
| U6 loop | `total = total + item` | same | same | same |
| Rules added | 8 shared, 12 traits, left-literal rule | same, plus one-`Out` rule (2b) | 7 shared, 12 traits | shared rules for std, sealed rows |
| Rules removed or changed | `expr.op.traits`, `trait.cmp.operators`, `expr.unsupported.overloading` | same | same | same, narrower |
| Soundness | output fixed by existing uniqueness | 2a: later output can create ambiguity | nothing new | nothing new |
| Agent-writability | high: Rust's shape, widely known | high: C#'s shape | high: one form | high for users: no choice |
| Human readability | `type Out = Duration` in every impl | shorter impls and bounds | shortest | unchanged for users |
| Implementation cost | medium: projections in operator typing | medium | low | low |
| Evolution | can add supertrait bindings later | can add rule 2b later | breaking to widen later | unsealing is compatible |
| Matches `LiteralSuffix[In, Out]` | no | yes | n/a | depends on std's shape |

## Recommendation

**Recommendation:** option 1, operand argument with associated `Out`, with
P1 primitives, the full twelve-operator set, the left-literal rule, C1 (no
compound assignment), N1 (hand-written newtype operators), and no user
indexing yet.

- It is Rust's model, which the owner named in L5, and it serves every use
  case the other options serve.
- Output uniqueness comes from the existing `trait.impl.unique` rule, so
  `x := a + b` has one type and no new coherence rule is needed.
- Mixed operands reuse the spec's existing instantiation choice and
  trait-argument ownership, whose examples already use `Add[Money]`.
- P1 keeps widening, literal typing, and checked overflow for primitives
  exactly as specified.
- C1 avoids in-place `+=` on reference types, which aliases would observe.

It gives up the shorter C#-style bounds (`T < Add[T, T]`) and the shape
match with `LiteralSuffix[In, Out]`. Each impl also writes `type Out`. The
next best option is option 2 with rule 2b, which is nearly equivalent and
matches `LiteralSuffix`.

## Questions For The Owner

### 1. Which trait shape?

Effect: decides whether `d * 3`, `3 * d`, and `t2 - t1` can be operators,
and how generic bounds read. Options:

- (1) operand argument, associated output: `Add[Rhs]` with `type Out`;
- (2) operand and output arguments: `Add[Rhs, Out]`;
- (3) same type only: `Add` with `other: Self`;
- (4) sealed: only std implements operator traits.

**Recommended: 1.**

```text
impl Mul[i64] for Duration:
    type Out = Duration
    fn mul(self, other: i64) -> Duration:
        Duration { nanos: self.nanos * other }
```

### 2. How do primitive operators relate to the traits?

Effect: decides whether `sum[i32]` works and whether widening survives.
Options: (P1) built in, plus std same-type implementations; (P2) every
primitive operator is a trait call; (P3) built in only, no implementations.
**Recommended: P1.**

```text
fn total(items: List[i32]) -> i32:
    sum(items, 0)
```

### 3. Which operators?

Effect: decides whether flag sets and big integers get `|` and `<<`.
Options: (a) arithmetic only: `+ - * / %` and unary `-`; (b) also `& | ^ ~`
and `<< >>`; (c) also `**`. **Recommended: b.**

```text
type Perms(u32)

fn both(a: Perms, b: Perms) -> Perms:
    a | b
```

### 4. May a literal sit on the left, as in `3 * d`?

Effect: without a rule, `3 * d` is an error and users write `d * 3`.
Options: (a) the left-literal rule: the literal takes the one primitive type
implementing `Mul[Duration]`, preferring `i32` or `f64` on a tie; (b) no
rule: write `d * 3` or `i64(3) * d`. **Recommended: a.**

```text
fn triple(d: Duration) -> Duration:
    3 * d
```

### 5. Compound assignment?

Effect: decides whether `total += item` exists, and whether aliases see it.
Options: (C1) none; (C2) `p op= e` rewrites to `p = p op e`; (C3) assign
traits with `mut self`. **Recommended: C1.** C2 can come later as its own
feature; C3 changes shared data through aliases.

```text
fn add_all(items: List[i64]) -> i64:
    let total: i64 = 0
    for item in items:
        total = total + item
    total
```

### 6. How do newtypes get operators?

Effect: decides whether `type Meters(f64)` writes four lines per operator.
Options: (N1) hand-written implementations; (N2) `@derive(Add, Sub, Neg)`
on newtypes, with a new forwarding rule under options 1 and 2.
**Recommended: N1.** Revisit N2 if a stress test shows many unit types.

```text
type Meters(f64)

impl Sub[Meters] for Meters:
    type Out = Meters
    fn sub(self, other: Meters) -> Meters:
        Meters(f64(self) - f64(other))
```

### 7. User indexing with `[]`?

Effect: decides whether `grid[r]` works on a user type. Options: (a) not
now; `[]` stays for `List` and `Map`; (b) add `Index[K]` and
`IndexSet[K, V]`. **Recommended: a**, since it needs the place and
readonly-root rules reopened.

```text
fn cell(grid: Grid, row: i32) -> List[i32]:
    grid.row(row)
```

### 8. May a supertrait list bind `Out`?

Effect: only under option 1. Today `trait Integer < Add[Self, Out = Self]`
is a `syntax-error`, so every numeric bound repeats `Out = T`. Options:
(a) allow bindings in supertrait lists, as Rust does; (b) keep the ban.
**Recommended: a.**

```text
pub trait Integer < Add[Self, Out = Self] & Sub[Self, Out = Self]  # hypothetical syntax
```

## Owner Idea: Numeric Trait Families

Logged 2026-09-28; not decided. The owner would probably want numeric
trait families such as:

```text
i64 -> IntLike   -> Num -> AnyVal -> Any
f64 -> FloatLike -> Num -> AnyVal -> Any
```

Here `->` means "implements" or "is a subtrait of". `Any`, `AnyVal` and
`AnyRef` already exist:
[`types.any`](../spec/04-type-system.md#r-types.any) and
[`types.sealed.decl`](../spec/04-type-system.md#r-types.sealed.decl).
`AnyVal` is sealed and covers `bool`, `char`, the integer types, `f32`,
`f64`, `string`, `void`, tuples, and newtypes of those.

Precedent: Swift's `Numeric`, `BinaryInteger`, `FixedWidthInteger` and
`FloatingPoint`; Haskell's `Num`, `Integral` and `Fractional`; Rust's
`num-traits` crate (outside std); Scala's `AnyVal` with `Numeric[T]` as a
type class.

Pitfalls to settle before specifying it:

1. **`Num < AnyVal` locks out library numbers.** `AnyVal` is sealed and
   `data` types are `AnyRef`, so a `BigInt`, `Decimal` or `Rational` written
   as `data` could never implement `Num`. Swift and Haskell keep `Numeric`
   and `Num` independent of value versus reference. Option: make `Num < Any`
   and let `IntLike` and `FloatLike` be sealed to primitives.
2. **Literals in generic code.** In `fn sum[T < Num](xs: List[T]) -> T`,
   the accumulator's `0` has no type `T` today. It needs either
   `Num::zero()` and `Num::one()` (Rust `num-traits`) or polymorphic literals
   through `from_integer` (Haskell's `fromInteger`). The first is the
   simpler, proven model.
3. **What `Num` contains.** Haskell's `Num` is a known mistake: it bundles
   `abs`, `signum` and `fromInteger`, so vectors and matrices can't be
   `Num`. Keep `Num` to `+ - *`, zero and one.
4. **Division differs.** Integer `/` truncates and panics on zero; float
   `/` gives infinity or NaN. `/` and `%` belong on `IntLike` and
   `FloatLike` separately, as in Swift, not on `Num`.
5. **Equality and order.** Floats have NaN, so they are `PartialOrd`, not
   `Ord`. `Num` can't require `Ord`; `IntLike` can.
6. **Overflow behavior diverges.** i64 arithmetic panics on overflow, while
   f64 saturates to infinity. Generic `Num` code must document both.
7. **Widths.** `IntLike` covers i8 through u64. Operations stay same-type
   (`Self`); width changes need explicit conversions such as `T::from_i64`
   or `try_from`, or generic code silently narrows.
8. **Depends on operator traits.** `trait Num < Add[Self, Out = Self] & ...`
   needs question 1 (the trait shape) and question 8 (a supertrait binding
   `Out`).
9. **Performance.** Generic numeric code dispatches through trait
   dictionaries unless the compiler specializes it; a hot loop over
   `T < Num` would be slower than over `i64` on Wasm.
10. **Naming.** `IntLike` and `FloatLike` read as informal. Swift uses
    `BinaryInteger` and `FloatingPoint`; Rust `num-traits` uses `PrimInt`
    and `Float`; `Integer` and `Float` are shorter.

## Sources

- Rust `std::ops`: <https://doc.rust-lang.org/std/ops/index.html>; `Add`: <https://doc.rust-lang.org/std/ops/trait.Add.html>
- Rust Reference, operator expressions (built-in primitive operators, compound assignment, comparison traits): <https://doc.rust-lang.org/reference/expressions/operator-expr.html>
- C# operator overloading, including C# 14 compound assignment: <https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/operators/operator-overloading>
- .NET `IAdditionOperators<TSelf, TOther, TResult>`: <https://learn.microsoft.com/en-us/dotnet/api/system.numerics.iadditionoperators-3>
- Swift Advanced Operators (operator methods, `inout` compound assignment, custom operators, overflow operators): <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/advancedoperators/>
- Swift `Duration`: <https://developer.apple.com/documentation/swift/duration>
- Kotlin operator overloading (`plusAssign` resolution, `get` and `set`, `compareTo`): <https://kotlinlang.org/docs/operator-overloading.html>
- Kotlin `Int.times(Duration)`: <https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.time/times.html>
- Scala tour, operators: <https://docs.scala-lang.org/tour/operators.html>; Scala 2.13 specification 6.12.4, assignment operators: <https://scala-lang.org/files/archive/spec/2.13/06-expressions.html#assignment-operators>
- Haskell Prelude `Num`: <https://hackage.haskell.org/package/base/docs/Prelude.html#t:Num>
- MoonBit operator overloading: <https://docs.moonbitlang.com/en/latest/language/methods.html>
- Go FAQ, operator overloading: <https://go.dev/doc/faq#overloading>; `time.Duration`: <https://pkg.go.dev/time#Duration>
- Zig overview, no hidden control flow: <https://ziglang.org/learn/overview/#no-hidden-control-flow>

## Parse Log

Every `text` block was parsed with the reference parser on 2026-09-28.
Parsing checks syntax only; no block is claimed to type-check. `Duration`
and `Timestamp` follow the [STDLIB draft](STDLIB.md#stdtime); `Grid.row` is
a placeholder.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Option 1, `std.ops` | parses |
| 2 | Option 1, `std.time` | parses |
| 3 | Option 1, uses | parses |
| 4 | Option 2, `std.ops` | parses |
| 5 | Option 2, `std.time` | parses |
| 6 | Option 2, uses | parses |
| 7 | Option 3, `std.ops` | parses |
| 8 | Option 3, uses | parses |
| 9 | Option 4 | parses |
| 10 | Compound Assignment | parses |
| 11 | Question 1 | parses |
| 12 | Question 2 | parses |
| 13 | Question 3 | parses |
| 14 | Question 4 | parses |
| 15 | Question 5 | parses |
| 16 | Question 6 | parses |
| 17 | Question 7 | parses |
| 18 | Question 8 | `syntax-error` at the marked line 1, as `trait.binding.rejected` requires |
