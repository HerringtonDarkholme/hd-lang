# `std.ops`

Generated from `hd doc std.ops`. Run `hd doc std.ops.NAME` in a package for one item's full text.

## `NumSuffix`

```text
pub struct NumSuffix[F]
```

## `num_suffix`

```text
pub fn num_suffix[N < Num, R, $ Q]() -> NumSuffix[fn(N) -> R $ Q]
```

Returns the `@num_suffix` marker for a suffix function of that signature.

## `StrPrefix`

```text
pub struct StrPrefix[F]
```

## `str_prefix`

```text
pub fn str_prefix[T, R, $ Q]() -> StrPrefix[fn(Template[T]) -> R $ Q]
```

Returns the `@str_prefix` marker for a prefix function of that signature.

## `Template`

```text
pub struct Template[T]
```

The pieces of a string prefix's literal: the raw parts and the values between them, which the compiler recognizes by its qualified name.

- `pub raw_parts: List[string]`: The literal's raw text pieces, in order.
- `pub values: List[T]`: The interpolated values between the raw parts, in order.

## `Add`

```text
pub trait Add[Rhs = Self]
```

The trait of the `+` operator: `a + b` is the call `Add::add(a, b)`.

## `Sub`

```text
pub trait Sub[Rhs = Self]
```

The trait of the binary `-` operator: `a - b` is the call `Sub::sub(a, b)`.

## `Mul`

```text
pub trait Mul[Rhs = Self]
```

The trait of the `*` operator: `a * b` is the call `Mul::mul(a, b)`.

## `Div`

```text
pub trait Div[Rhs = Self]
```

The trait of the `/` operator: `a / b` is the call `Div::div(a, b)`.

## `Rem`

```text
pub trait Rem[Rhs = Self]
```

The trait of the `%` operator: `a % b` is the call `Rem::rem(a, b)`.

## `Neg`

```text
pub trait Neg
```

The trait of the unary `-` operator: `-a` is the call `Neg::neg(a)`.

## `BitAnd`

```text
pub trait BitAnd[Rhs = Self]
```

The trait of the `&` operator: `a & b` is the call `BitAnd::bitand(a, b)`.

## `BitOr`

```text
pub trait BitOr[Rhs = Self]
```

The trait of the `|` operator: `a | b` is the call `BitOr::bitor(a, b)`.

## `BitXor`

```text
pub trait BitXor[Rhs = Self]
```

The trait of the `^` operator: `a ^ b` is the call `BitXor::bitxor(a, b)`.

## `Not`

```text
pub trait Not
```

The trait of the `!` operator: `!a` is the call `Not::not(a)`.

## `Shl`

```text
pub trait Shl[Rhs = Self]
```

The trait of the `<<` operator: `a << b` is the call `Shl::shl(a, b)`.

## `Shr`

```text
pub trait Shr[Rhs = Self]
```

The trait of the `>>` operator: `a >> b` is the call `Shr::shr(a, b)`.

## `Index`

```text
pub trait Index[K] < ?Copy
```

Lends the element place of `r[k]`. Reading copies its value; `&r[k]` borrows it.

## `IndexMut`

```text
pub trait IndexMut[K] < ?Copy & Index[K]
```

Lends a mutable element place for `&mut r[k]`, field writes, and compound assignment.

## `IndexSet`

```text
pub trait IndexSet[K, V < ?Copy] < ?Copy
```

The trait of assigning `r[k] = v`: it is the call `IndexSet::[K, V]::index_set(r, k, v)`.

## `Apply`

```text
pub trait Apply
```

The trait of calling a value: `v()` is the call `Apply::apply(v)`.

## `Update`

```text
pub trait Update[V]
```

The trait of assigning to a call: `v() = x` is the call `Update::[V]::update(v, x)`.

## `Disposable`

```text
pub trait Disposable[$ R = $()]
```

A value that releases what it holds synchronously when a `using` binding of it ends. `dispose` is best-effort and reports nothing. Its cleanup may require the row `R`, which defaults to the empty row.

## `AsyncDisposable`

```text
pub trait AsyncDisposable[$ R = $()]
```

A value that releases what it holds when a `using!` binding of it ends. `dispose!` may suspend, is best-effort, and reports nothing. Its cleanup may require the row `R`, which defaults to the empty row.

## `AutoClosing`

```text
pub trait AutoClosing
```

Marks a guard-like value that an `@auto_closing` call registers immediately. The type also implements `Disposable` or `AsyncDisposable` to select its cleanup protocol.

## `DeferGuard`

```text
pub struct DeferGuard[F < FnMut() -> void $ R & 'a, $ R, 'a]
```

A guard registered by `defer`; its action runs when the calling scope exits.

## `defer`

```text
pub fn defer[F < FnMut() -> void $ R & 'a, $ R, 'a](action: F) -> DeferGuard[F, $ R, 'a]
```

Registers `action` to run when the calling scope exits.

## `ScopeGuard`

```text
pub struct ScopeGuard[F < FnMut() -> void $ R & 'a, $ R, 'a]
```

A guard that runs `action` when a `using` binding of it ends. The action may require `R` and may capture references of lifetime `'a`.

## `on_exit`

```text
pub fn on_exit[F < FnMut() -> void $ R & 'a, $ R, 'a](action: F) -> ScopeGuard[F, $ R, 'a]
```

Builds a scope guard whose cleanup runs `action`.

## `Range`

```text
pub struct Range[T]
```

The range `a..b` or `a..=b`: a lower and an upper bound.

- `pub start: T`: The range's lower bound.
- `pub end: T`: The range's upper bound.
- `pub inclusive: bool`: True for the `..=` form, false for the `..` form.

## `RangeFrom`

```text
pub struct RangeFrom[T]
```

The range `a..`: a lower bound and no end.

- `pub start: T`: The range's lower bound.

## `RangeTo`

```text
pub struct RangeTo[T]
```

The range `..b` or `..=b`: an upper bound and no start.

- `pub end: T`: The range's upper bound.
- `pub inclusive: bool`: True for the `..=` form, false for the `..` form.

## `RangeFull`

```text
pub struct RangeFull
```

The range `..`: the whole extent.

## `RangeIter`

```text
pub struct RangeIter[T]
```

The cursor of a range's `iter`: the integers from its start up to its end.

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `RangeFromIter`

```text
pub struct RangeFromIter[T]
```

The cursor of an `a..` range's `iter`: the integers from its start on, with no end.

## `Default`

```text
pub trait Default
```

A type's default value, which `T::default()` returns.

## `DefaultVariant`

```text
pub struct DefaultVariant
```

## `default`

```text
pub fn default() -> DefaultVariant
```

The marker function of `@default`: it attaches a `DefaultVariant` fact to the variant.

## `Tuple`

```text
pub trait Tuple < ?Copy
```

The empty sealed marker trait that every tuple type implements, `()` and `(A,)` included.

## `FnMut`

```text
pub trait FnMut[I < ?Copy & Tuple, O < ?Copy, $ R = $()]
```

The static-dispatch trait implemented by every function value, for a callee that may change its captures.

- `fn call_mut(&mut self, args: I) -> O $ R`: Calls this function with the parameter tuple `args`; the call may change its captures.

## `Fn`

```text
pub trait Fn[I < ?Copy & Tuple, O < ?Copy, $ R = $()] < FnMut[I, O, $ R]
```

The static-dispatch trait for a function value that changes none of its captures; it extends `FnMut`.

- `fn call(&self, args: I) -> O $ R`: Calls this function with the parameter tuple `args`.
