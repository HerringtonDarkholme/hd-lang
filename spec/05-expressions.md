# Expressions

Status: language specification draft.

Expressions compute values. Every expression has a static type and is evaluated
according to the order defined here and in the relevant feature chapter.

## Evaluation Order

Unless a construct states otherwise, subexpressions are evaluated from left to
right and exactly once. This rule applies to tuple and collection elements,
data fields, call arguments, operands, and indexing expressions.

`and`, `or`, `if`, `match`, loops, optional or result propagation with `?`, and
comprehension filters evaluate conditionally as described below. A compiler may
reorder only when it can prove that the program's observable behavior is
unchanged.

## Expression Categories

A **value expression** produces a value. A **place expression** identifies a
storage location and may be read or, when permissions allow, assigned.

The core place expressions are:

- a reassignable local introduced by `let`;
- a field selected through a mutable composite root;
- an index operation whose receiver and indexing protocol expose mutable
  storage.

`:=` bindings, literals, calls, arithmetic, and temporary values are not places.
A tuple element selection such as `pair.0` is not a place: tuples are
immutable, and a changed tuple is built as a new tuple value.
Assignment is a statement and requires a mutable place on its left side.
Assigning to an expression that is not a place is an
`invalid-assignment-target` error.

Assignment evaluates the place before the right-hand side, then performs one
store. A local place requires no subexpression evaluation; a field assignment
evaluates its receiver, then the right-hand expression; an indexed assignment
evaluates the receiver, the index, and the right-hand expression in that order.
If any step completes abruptly, no store occurs.

## Primary Expressions

### Names

An identifier expression evaluates the declaration or local binding selected
by lexical name resolution. A qualified name selects a declaration or enum
variant through a module or type namespace.

`.Variant` selects a variant only when the expression has an expected
type that fixes one nominal enum. It has the same construction and argument
rules as `Enum.Variant`; a payload-bearing variant still requires a call.
The compiler does not search all visible enums for a matching variant name.
Without a unique expected enum type, `.Variant` is a type error.

### Literals

Boolean, integer, floating-point, string, character, and `nil` literals are
defined lexically in [Lexical Structure](01-lexical-structure.md) and typed in
[Type System](04-type-system.md).

Interpreted strings support `$name` and `${expression}` interpolation. Embedded
expressions are evaluated from left to right at the position of their segment.
Each expression's type must implement the canonical `std.format.Display`
trait; the compiler appends the string returned by that implementation. An
embedded expression whose type does not implement `Display` is an
`unsatisfied-trait-bound` error. There
is no fallback conversion through `Any`, runtime reflection, or debug output.
The standard library provides `Display` implementations for ordinary
printable primitive types and `string`. Optional and user-defined values are
displayable only when the corresponding type implements `Display`.

The canonical signature is:

```text
trait Display:
    fn to_string(self) -> string
```

Raw strings never interpolate. A string with no interpolation segments is an
ordinary constant value and performs no `Display` calls.

`pass` is the no-op expression. It has type `void` and performs no operation.

### Parenthesized And Tuple Expressions

Parentheses group one expression without changing its value:

```text
value := (a + b) * c
```

A comma constructs a tuple. A one-element tuple requires a trailing comma:

```text
empty := ()
single := (1,)
point := (10, 20)
```

Tuple elements evaluate left to right. Tuple selection uses a zero-based
numeric member such as `point.0`.

### List And Map Expressions

A list literal evaluates its elements left to right and produces `list[T]`,
where every element is assignable to `T`:

```text
names := ["Ada", "Grace"]
```

A map literal evaluates each key and then its value, processing entries from
left to right:

```text
scores := {"Ada": 10, "Grace": 12}
```

If two evaluated entries have equal keys, the later value replaces the earlier
value without changing that key's first insertion position. Map iteration is
in insertion order and is not part of map equality.
Key validity and equality/hash requirements are defined in
[Type System](04-type-system.md#map-key-types).

Empty `[]` and `{}` literals require an expected collection type because they
contain no values from which to infer type arguments. Without one, the literal
is an `unresolved-generic-placeholder` error.

When an expected `list[T]` or `map[K, V]` type is available, each literal
element is checked directly against the corresponding expected type. Without
an expected type, the element type of a list, and the key type and the value
type of a map, are the
[least common type](04-type-system.md#least-common-type) of the corresponding
entries.

### Data Expressions

A data expression names its type and provides every required field:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
}
```

Fields may appear in any order. Every field may appear at most once, and an
unknown field is a compile-time error. Field initializers evaluate in source
order, not declaration order.

Copy-update syntax begins with one spread followed by explicit replacements:

```text
renamed := User {
    ...user,
    display_name: "Ada",
}
```

The spread expression is evaluated first and must have the exact data type
being constructed. For each field not explicitly replaced, the copied value is
type-checked as a read through the spread source's access view. A readonly
source reads a direct `mut U` field as `U`: that value can fill the same field
in a readonly result, but not in a `mut` result without an explicit `mut U`
replacement. Generic fields retain their substituted type, even when it is
`mut U`. A mutable source retains direct fields' declared permissions.
Copy-update is shallow: primitive fields are
copied by value, while composite field references continue to refer to the
same underlying objects. A fresh mutable outer result does not upgrade copied
child references. A data expression permits at most one data spread, and it
must precede every explicit field.

Embedded fields are initialized with their embedded type name as the field key.

## Postfix Expressions

Postfix operations bind more tightly than every infix operator.

### Member Access

`value.member` selects a field, and `tuple.0` selects a tuple element. When a
member suffix is immediately followed by an argument clause,
`value.method(arguments...)` performs method lookup and invocation. Whether a
bare `value.method` can form a bound function value is deferred; explicit
closures can adapt method calls where a function value is needed. Embedded
field and method promotion follows
[Names and Scopes](03-names-and-scopes.md).

Member access through a readonly data root weakens a direct `mut U` field to
`U`, but does not weaken a generic field's substituted type. Other member
forms follow their own access rules in [Type System](04-type-system.md).

### Indexing

`receiver[index]` evaluates the receiver, then the index, and invokes the
receiver type's indexing behavior.

For `list[T]`, an index may have any integer type. It must be non-negative and
less than the list length. A failed check causes the standard checked runtime
panic. Reading a list element yields its declared generic type `T`, including
`mut U` when `T = mut U`, regardless of the list root's permission. Assigning
`items[index] = value` still requires a mutable list root and an in-range index.

For `map[K, V]`, the index must have type `K`. Reading `entries[key]` returns
`V?`: `nil` means no equal key exists. The generic `V` is preserved through a
readonly map, including `mut U` when `V = mut U`; unwrapping the optional
returns `V`. Assigning
`entries[key] = value` requires `mut map[K, V]` and inserts or replaces the
entry. Removal and entry APIs are standard-library methods rather than special
syntax.

### Calls

A call evaluates the callable first, then arguments from left to right.
Positional arguments must precede named arguments:

```text
resize(640, height=480)
```

Each named argument identifies a parameter by its declared name; a named
argument that names no parameter is an `unknown-named-argument` error. A
parameter must receive exactly one argument after defaults are applied.
Supplying one parameter more than once, such as positionally and again by
name, is a `duplicate-argument` error. Evaluation order follows source argument
order, not parameter declaration order.

An argument ending in `...` is a positional spread. It is evaluated once, must
have `list[T]` compatible with the callee's final `T...` parameter, and supplies
that vararg's remaining positional elements. It cannot fill fixed parameters.
A positional spread in a call whose callee has no vararg parameter is a
`positional-spread-needs-vararg` error.
A call has at most one positional spread; it must be the final positional
argument and therefore precedes every named argument. Passing a vararg by name
uses one ordinary list value without `...`, and the same call must not also
supply positional values for that vararg.

Generic arguments, when explicit, occur before the call argument list:

```text
first[string](names)
```

The complete generic argument list must be supplied; partial explicit lists are
not supported. In hd-lang, explicit arguments may specialize a named module
function or qualified function introduced by a use declaration. The same
explicit-list rules apply to generic methods; see [Functions](07-functions.md).

Suspension calls with `!` construct and drive a child suspension as specified in
[Requirements and Suspension](11-requirements-and-suspension.md). The callee and
arguments are evaluated left to right before the child begins execution.

### Propagation

Postfix `?` handles either an optional or a `Result` value:

- for `T?`, a present value produces `T`; `nil` immediately returns `nil` from
  the nearest function;
- for `Result[T, E]`, `Ok(value)` produces the declared `T`, including a
  mutable type argument; `Err(error)` immediately returns a compatible `Err`
  from the nearest function.

The operand is evaluated once. `?` does not catch runtime panics and does not
interact with suspension by itself.

Postfix `?` is invalid when there is no enclosing named function or closure
with the required optional or `Result` return type. Module top-level statements
and `test` blocks do not provide an implicit propagation target. Both misuses
of `?` are `invalid-result-propagation` errors: an operand that is neither
optional nor a `Result`, and a `?` whose enclosing function or closure does not
return a compatible optional or `Result`.

## Unary And Binary Operators

Operators are ordered from highest to lowest precedence:

| Precedence | Operators | Associativity |
| --- | --- | --- |
| Postfix | `.`, `[]`, `()`, `!()`, postfix `?` | left |
| Power | `**` | right |
| Unary | `+`, `-`, `~`, `not` | right |
| Multiplicative | `*`, `/`, `%` | left |
| Additive | `+`, `-` | left |
| Shift | `<<`, `>>` | left |
| Bitwise AND | `&` | left |
| Bitwise XOR | `^` | left |
| Bitwise OR | `|` | left |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=`, `is` | non-associative |
| Logical AND | `and` | left, short-circuiting |
| Logical OR | `or` | left, short-circuiting |
| Control and closure | `if`, `match`, `for`, `while`, `fn` | structural |
| Binding | `:=` | right |

Suspension-call postfix `!` has the same precedence as an ordinary call.

Exponentiation binds less tightly on its right than unary negation, following
the grammar: `2 ** -3` is valid, while `-2 ** 2` means `-(2 ** 2)`.

Comparisons do not chain. Write `low <= value and value < high` rather than
`low <= value < high`.

Arithmetic operators require compatible numeric operands. Mixed-width result
types, overflow, division, and shifts are defined in
[Type System](04-type-system.md). Signed/unsigned and integer/floating mixing
requires an explicit cast.

For compatible integer operands, `+`, `-`, and `*` produce the common integer
type and use checked arithmetic. `/` truncates toward zero, `%` produces the
corresponding remainder, and a zero divisor panics. Unary `+` accepts all
numeric types, preserves its operand's type and value, and evaluates the operand
once. Unary `-` accepts signed integers and floating-point values, but not
unsigned integers. `~`, `&`, `|`,
and `^` accept integer values only and produce the operand common type.

Shifts are the exception to ordinary binary numeric unification. The left
operand may have any integer type, the right operand may have any integer type,
and the result preserves the left operand type. A negative count or a count at
least as large as the left operand's bit width panics. The shift itself is a
fixed-width bit operation; left-shifted high bits are discarded rather than
reported as arithmetic overflow.

For an integer base, `**` requires an exponent of an unsigned integer type,
so a negative exponent cannot occur. An unsuffixed integer literal in
exponent position has type `u32`. A signed integer exponent is a
`type-mismatch` error. Exponentiation uses checked multiplication in the
base's result type. For a floating-point base, the exponent must be floating point after
ordinary floating widening. Floating `**` computes IEEE 754-2019 `pow` as
specified in clause 9.2, including its special cases, and rounds the result
correctly to the destination format. Integer and floating operands do not mix
without an explicit cast; a mixed power expression is a
`mixed-numeric-types` error.

Floating `+`, `-`, `*`, and `/` use the corresponding required IEEE 754 basic
operation, including infinities, signed zero, and NaN. Floating `**` uses the
`pow` rule above. `%` is integer-only.

`not` requires `bool`. `and` and `or` require `bool` operands and produce
`bool`. They evaluate the right operand only when needed.

`==` calls `PartialEq.eq` and `!=` negates that result. Standard-library
implementations provide value equality for primitives, optional and result
values, tuples, lists, and maps when their elements support equality. Map
equality is independent of entry order. A user-defined data or enum type has
no implicit `PartialEq` implementation, even if all its members are
comparable: the author must explicitly implement or request derivation of
the trait. Equality never
silently falls back to reference identity. Floating-point equality follows
IEEE 754, so NaN is unequal even to itself.
Function and closure values do not implement `PartialEq`; applying `==` or
`!=` to them is an `unsupported-equality` error.

`<`, `<=`, `>`, and `>=` use `PartialOrd.partial_cmp`. The standard library
implements it for compatible numeric values, characters by Unicode scalar
value, strings lexicographically by scalar value, tuples and lists
lexicographically, and optionals with `nil` before every present value.
Composite ordering is available when the corresponding elements implement the
comparison trait, and it stops at the first unequal or unordered element.
Users can implement comparison traits for their own types. `Ord` is the total-order refinement; floating-point
types have `PartialOrd` but not `Ord` because NaN is unordered. An unordered
comparison makes all four relational operators false.

`is` compares identity without invoking user code. Data values, stored enum
payloads, lists, maps, and other heap composites have allocation identity;
access permission (`mut`) does not change it. Converting such a value to a
trait value or `Any` preserves the underlying identity. Each evaluation of a
closure expression creates one closure identity, retained by aliases. A
conversion of a primitive, tuple, or optional value to a dynamic trait value or
`Any` allocates one fresh immutable box; the resulting trait or `Any` value has
that box's identity, and aliases of the converted value share it. Repeating the
conversion allocates a distinct box even when the source values compare equal.
A direct conversion of a heap composite continues to preserve the composite's
underlying identity and does not allocate an identity wrapper. A
payload-free enum value is canonical for its variant, and a fieldless data
value is canonical for its data type: two occurrences of the same such value
have the same identity, and constructing one allocates nothing. An enum
variant that carries shared constructor data stores that data, so each
construction has its own allocation identity even when the variant has no
payload of its own. Tuples have no identity and using `is`
with a tuple is rejected even if it contains references. Primitive values,
`nil`, and optional values likewise cannot be compared with `is`. Both operands
must otherwise have compatible composite reference types: after removing
`mut` at every level, the two types are equal, or one is a trait value or
`Any` type that the other converts to. Permissions never affect identity, so
`list[User]` and `mut list[mut User]` are compatible. Two composite reference
operands that are not compatible, such as `list[User]` and `list[Order]`, are
an `incompatible-identity-operands` error. Use `not (a is b)` for distinct
identities.

Arithmetic and bitwise operators are built in for the numeric types specified
by this chapter and [Type System](04-type-system.md). `string + string`
concatenates strings. Comparison traits are the only operator traits; other
user-defined operator overloading is not part of the language.

## Binding Expressions

`:=` introduces one or more inferred, non-reassignable names and evaluates to
the initializer's value:

```text
if (trimmed := input.trim()) != "":
    println(trimmed)
```

It has the lowest precedence. Parentheses are required when a binding appears
as an operand of another expression, as above. Its scope is defined in
[Names and Scopes](03-names-and-scopes.md).

For tuple binding, the right side must be a tuple of the same arity; any
other value is a `type-mismatch` error. The value of the
whole binding expression is the original tuple value.

## Comprehensions

A list comprehension evaluates clauses from left to right and appends one
result for every path that reaches `=>`:

```text
pairs := [for x in xs for y in ys if x.id == y.owner_id => (x, y)]
```

This is equivalent in iteration shape to nested loops, with an `if` clause
filtering at the exact point where it appears. Names introduced by earlier
clauses are visible to later clauses and the result.

A map comprehension evaluates the key and then the value for every successful
path:

```text
by_id := {for user in users if user.active => user.id: user}
```

Duplicate keys use the later generated value. Comprehensions are eager and
cannot contain suspension calls. `return`, `break`, and
`continue` are not valid inside a comprehension.

There is no comprehension `let` clause. Use a parenthesized `:=` binding in a
guard or result expression:

```text
labels := [for user in users
           if (label := user.name.trim().lower()) != ""
           => label]
```

## Closures And Control Expressions

Closures are expressions described in [Functions](07-functions.md). `if`,
`match`, and loops are value-capable expressions described in
[Control Flow](06-control-flow.md).

## Unsupported Expression Extensions

hd-lang has no user-defined arithmetic or bitwise operator overloading, comparison chaining, or
fallback conversion of heterogeneous literals to `Any`.
