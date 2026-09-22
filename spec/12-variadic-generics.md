# Variadic Generics

Status: language specification draft.

Variadic generics describe a statically known ordered pack of heterogeneous
types and the corresponding pack of values. They are intended for typed
higher-order adapters and library concurrency combinators, not runtime dynamic
lists.

## Pack Parameters

A generic parameter ending in `...` is a type pack:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)
```

```ebnf
generic_parameter = [ "reified" ], identifier, [ "..." ],
                    [ ":", trait_bounds ] ;
```

A pack has a compile-time length and ordered elements. It is not a type whose
runtime values are stored in a homogeneous collection.

## Pattern Expansion

Placing `...` at an expansion position repeats the containing type or
expression pattern once per pack element:

```text
fn all![Ts...](tasks: Suspend[Ts]...) -> (Ts...):
    ...
```

For `Ts... = User, i32, bool`:

- `Suspend[Ts]...` expands to parameter types `Suspend[User]`,
  `Suspend[i32]`, and `Suspend[bool]`;
- `(Ts...)` expands to `(User, i32, bool)`;
- a value pattern such as `start(tasks)...` repeats `start(task)` once for each
  corresponding value.

Expansion is permitted in:

1. function parameter type patterns;
2. function and tuple type element positions;
3. vararg value parameter patterns;
4. call argument positions;
5. type and expression patterns explicitly containing pack names.

The consolidated grammar exposes expansion positions directly:

```ebnf
type_argument = type, [ "..." ] ;
type_element = type, [ "..." ] ;
value_parameter = identifier, ":", type, [ "=" , expression ], [ "..." ] ;
positional_argument = expression, [ "..." ] ;
```

An ellipsis is a pack expansion when the preceding subtree contains at least
one pack reference. Otherwise, parameter and argument ellipses retain their
ordinary homogeneous-vararg and list-spread meanings. Expansion is not an
arbitrary syntax macro: only the designated type, parameter, tuple, and
argument positions may repeat.

## Lockstep Expansion

If one repeated pattern references multiple packs, expansion is positional and
all referenced packs must have equal lengths. Element `i` from every pack is
substituted into repetition `i`.

```text
fn zip_apply[As..., Bs..., Rs...](
    funcs: fn(As, Bs) -> Rs...,
    left: As...,
    right: Bs...,
) -> (Rs...):
    ...
```

This example is valid only when the three inferred packs have the same length.
The `funcs` parameter expands the complete function-type subtree once per
position. No additional grouping is required.

## Calls And Inference

Type-pack and value-pack inference uses the corresponding argument positions.
All uses of one pack in a signature must infer one length and one ordered type
sequence. A mismatch is a compile-time error with element-position diagnostics.

Pack arguments are inferred. Explicit pack arguments, partial explicit packs,
and pack placeholders are not language constructs.

At runtime, each expanded parameter is an ordinary parameter and `(Ts...)` is
an ordinary tuple. No hidden `list[Any]`, reflection array, or allocation is
required by the source semantics.

## Deliberate Limits

The minimal design does not provide general pack:

- mapping;
- filtering;
- indexing;
- splitting or concatenation;
- length arithmetic;
- iteration as a runtime sequence.

Pattern expansion covers the accepted initial use cases without creating a
compile-time metaprogramming language.

## `all!` Motivation

The standard-library `all!` combinator needs heterogeneous input and output:

```text
user, count, ready := all!(load_user(), load_count(), check_ready())
```

Its signature can preserve each result type with one pack. Scheduling and
cancellation are not variadic-generic semantics; they belong to the concurrency
library and suspension protocol.

## Expansion Validation

More than one expansion may occur in one parameter, tuple, or argument list.
Each expansion is checked independently. When multiple packs occur in one
expanded subtree, they expand in lockstep and must have the same length. When a
pack occurs beneath multiple generic constructors, every occurrence contributes
constraints to the same ordered sequence; incompatible element constraints are
reported at the first differing pack position.
