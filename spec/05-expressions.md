# Expressions

Status: language specification draft.

Expressions compute values. Every expression has a static type and is evaluated
according to the order defined here and in the relevant feature chapter.

## Evaluation Order

Unless a construct states otherwise, subexpressions are evaluated from left to
right and exactly once. This rule applies to tuple and collection elements,
struct fields, call arguments, operands, and indexing expressions.

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
Assignment is a statement and requires a mutable place on its left side.

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

`.Variant` selects a variant only when the expression has a contextual expected
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
trait; the compiler appends the string returned by that implementation. There
is no fallback conversion through `Any`, runtime reflection, or debug output.
The standard library provides `Display` implementations for ordinary
printable primitive types and `string`. Optional and user-defined values are
displayable only when the corresponding type implements `Display`.

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
value. Map iteration order is not specified and is not part of map equality.
Key validity and equality/hash requirements are defined in
[Type System](04-type-system.md#map-key-types).

Empty `[]` and `{}` literals require an expected collection type because they
contain no values from which to infer type arguments.

When an expected `list[T]` or `map[K, V]` type is available, each literal
element is checked directly against the corresponding expected type. Without
an expected type, the compiler computes a unique least common type using only
the implicit conversions in [Type System](04-type-system.md). Numeric widening,
permission weakening, and declared read-only variance may contribute. If no
unique least type exists, inference fails and the user must add an expected
type. The compiler never falls back to `Any` merely to make a heterogeneous
literal type-check. Unconstrained inference also does not introduce a dynamic
trait-value conversion, because a concrete type may satisfy multiple unrelated
traits; an expected `list[Display]` or `map[K, Display]` may request that
conversion explicitly. When `nil` occurs with non-`nil` elements having one
unique least type `T`, the inferred element or value type is `T?`.

### Struct Expressions

A struct expression names its type and provides every required field:

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

The spread expression is evaluated first and must have the exact struct type
being constructed. Explicit fields replace the corresponding copied values.
Copy-update is shallow: primitive fields are copied by value, while composite
field references continue to refer to the same underlying objects with the
permissions declared by their field types. A struct expression permits at most one
struct spread, and it must precede every explicit field.

Embedded fields are initialized with their embedded type name as the field key.

## Postfix Expressions

Postfix operations bind more tightly than every infix operator.

### Member Access

`value.member` selects a field, and `tuple.0` selects a tuple element. When a
member suffix is immediately followed by an argument clause,
`value.method(arguments...)` performs method lookup and invocation. A bare
method selection is not a first-class bound-method value; wrap the call
in a closure when a function value is required. Embedded field and method
promotion follows
[Names and Scopes](03-names-and-scopes.md).

Member access through a const composite root applies viewpoint weakening to
nested mutable edges, as defined in [Type System](04-type-system.md).

### Indexing

`receiver[index]` evaluates the receiver, then the index, and invokes the
receiver type's indexing behavior.

For `list[T]`, an index may have any integer type. It must be non-negative and
less than the list length. A failed check causes the standard checked runtime
panic. Reading through a const list yields the const viewpoint of `T`; reading
through `mut list[T]` preserves the stored element permission. Assigning
`items[index] = value` requires a mutable list root and an in-range index.

For `map[K, V]`, the index must have type `K`. Reading `entries[key]` returns
`V?`: `nil` means no equal key exists. Reading through a const map applies the
ordinary const viewpoint to the contained value. Assigning
`entries[key] = value` requires `mut map[K, V]` and inserts or replaces the
entry. Removal and entry APIs are standard-library methods rather than special
syntax.

### Calls

A call evaluates the callable first, then arguments from left to right.
Positional arguments must precede named arguments:

```text
resize(640, height=480)
```

Each named argument identifies a parameter by its declared name. A parameter
must receive exactly one argument after defaults are applied. Evaluation order
follows source argument order, not parameter declaration order.

An argument ending in `...` is a positional spread. It is evaluated once, must
have `list[T]` compatible with the callee's final `T...` parameter, and supplies
that vararg's remaining positional elements. It cannot fill fixed parameters.
A call has at most one positional spread; it must be the final positional
argument and therefore precedes every named argument. Passing a vararg by name
uses one ordinary list value without `...`, and the same call must not also
supply positional values for that vararg.

Generic arguments, when explicit, occur before the call argument list:

```text
first[string](names)
```

The complete generic argument list must be supplied; partial explicit lists are
not supported. In hd-lang, explicit arguments may specialize a named module function
or qualified imported function. Generic methods rely on inference; explicit
method type arguments are not supported.

Suspension calls with `!` construct and drive a child suspension as specified in
[Requirements and Suspension](11-requirements-and-suspension.md). The callee and
arguments are evaluated left to right before the child begins execution.

### Propagation

Postfix `?` handles either an optional or a `Result` value:

- for `T?`, a present value produces `T`; `nil` immediately returns `nil` from
  the nearest function;
- for `Result[T, E]`, `Ok(value)` produces `T`; `Err(error)` immediately returns
  a compatible `Err` from the nearest function.

The operand is evaluated once. `?` does not catch runtime panics and does not
interact with suspension by itself.

Postfix `?` is invalid when there is no enclosing named function or closure
with the required optional or `Result` return type. Module top-level statements
and `test` blocks do not provide an implicit propagation target.

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
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=` | non-associative |
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

For an integer base, `**` requires an integer exponent. A negative exponent
panics, and exponentiation uses checked multiplication in the base's result
type. For a floating-point base, the exponent must be floating point after
ordinary floating widening, and the operation follows IEEE 754 power behavior.
Integer and floating operands do not mix without an explicit cast.

Floating `+`, `-`, `*`, `/`, and `**` follow IEEE 754, including infinities,
signed zero, and NaN. `%` is integer-only.

`not` requires `bool`. `and` and `or` require `bool` operands and produce
`bool`. They evaluate the right operand only when needed.

`==` and `!=` use structural value equality for:

- booleans, characters, strings, and compatible numeric values;
- transparent aliases and nominal newtypes of an equality-capable type;
- optional values, tuples, lists, maps, structs, enums, and `Result` values when
  every contained value is equality-capable.

Map equality is unordered key/value equality. Composite equality follows
declared field or payload order and handles recursive object graphs without
infinite recursion by tracking already-compared identity pairs. Function values
and dynamic trait values do not support equality. Equality never silently
uses reference identity. Floating equality and ordering follow IEEE 754, so a
NaN compares unequal to every value, including itself.

`<`, `<=`, `>`, and `>=` are defined for compatible numeric values,
characters by Unicode scalar value, strings by lexicographic Unicode scalar
order, and nominal newtypes of an orderable type. General tuple, collection,
struct, enum, and user-defined ordering is not supported.

Arithmetic and bitwise operators are built in for the numeric types specified
by this chapter and [Type System](04-type-system.md). `string + string`
concatenates strings. User-defined operator overloading and operator traits are
not part of the language.

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

For tuple binding, the right side must have the same arity. The value of the
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

hd-lang has no user-defined operator overloading, comparison chaining, match guards,
first-class bound-method values, or fallback conversion of heterogeneous
literals to `Any`.
