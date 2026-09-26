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
fn all![Ts...](tasks: mut Suspend[Ts]...) -> (Ts...):
    ...
```

For `Ts... = User, i32, bool`:

- `mut Suspend[Ts]...` expands to parameter types `mut Suspend[User]`,
  `mut Suspend[i32]`, and `mut Suspend[bool]`;
- `(Ts...)` expands to `(User, i32, bool)`;
- a value pattern such as `start(tasks)...` expands to
  `start(tasks_0), start(tasks_1), ...`.

Expansion is permitted in:

1. function parameter type patterns;
2. function and tuple type element positions;
3. vararg value parameter patterns;
4. call argument positions;
5. type and expression patterns explicitly containing pack names.

The consolidated grammar exposes expansion positions directly:

```ebnf
type_argument = type, [ "..." ]
              | row_type_argument
              ;
type_element = type, [ "..." ] ;
value_parameter = identifier, ":", type, [ "=", expression ]
                | identifier, ":", type, [ "=", continued_expression ], "..."
                ;
positional_argument = expression
                    | continued_expression, "..."
                    ;
```

A function signature may contain at most one value-pack parameter that accepts
positional arguments. Because the current language has no named-only parameter
separator, this means a signature may contain at most one value-pack parameter.
Like an ordinary homogeneous vararg, that value-pack parameter must be the
final positional parameter; a later positional parameter is a
`nonfinal-positional-value-pack` error. Calls never guess a partition between
positional packs or between a pack and a later fixed parameter.

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
    pairs: ((As, Bs)...),
    funcs: fn(As, Bs) -> Rs...,
) -> (Rs...):
    ...
```

This example is valid only when the three inferred packs have the same length.
The ordinary `pairs` parameter carries a tuple whose repeated element type uses
the same packs. The final `funcs` parameter is the signature's single
positional value-pack parameter and expands the complete function-type subtree
once per position.

## Pack Mapping

`pack.map(items, mapper, extras...)` maps a statically known tuple to another
tuple. `pack.map_list(items, mapper, extras...)` maps the same input to a
homogeneous `list[R]`. Both are compiler-recognized operations on tuples, not
ordinary first-class functions or runtime reflection. The first argument is
evaluated once and must have tuple type `(T1, ..., Tn)`. The second argument
names a non-suspending function; it is not evaluated as a function
value. The compiler type-checks and instantiates one call
`mapper(item_i, extras...)` for each tuple element. A generic mapper may infer
different type arguments for each call without requiring a first-class
polymorphic function type.

`pack.map` returns `(R1, ..., Rn)`, where `Ri` is the result type of mapper
call `i`. `pack.map_list` requires all mapper results to be assignable to one
element type `R` using the ordinary collection-literal inference rules; an
expected `list[R]` may provide that type. It does not infer `Any` merely to
combine heterogeneous results. Mapping an empty tuple with `pack.map` returns
`()`. Empty `pack.map_list` requires an expected `list[R]` type.

The tuple expression `(values...)` expands a value pack into tuple elements;
it is not a runtime pack object. A tuple-element `...` without a pack reference
is invalid. Mapping also works on an ordinary tuple
stored in a local variable, so a mapped tuple can be mapped again:

```text
data Slot[T]:
    task: mut Suspend[T]

fn make_slot[T](task: mut Suspend[T]) -> mut Slot[T]:
    Slot { task: task }

fn poll_slot[T](slot: mut Slot[T], context: PollContext) -> bool:
    ...

fn take_ready[T](slot: mut Slot[T]) -> T:
    ...

# Illustrative typed steps of an all!-style driver, with Ts... from its tasks:
slots := pack.map((tasks...), make_slot)     # (mut Slot[Ts]...)
ready := pack.map_list(slots, poll_slot, context)  # list[bool]
results := pack.map(slots, take_ready)        # (Ts...)
```

The sketch shows only the typed transformations. Such a driver retains
`slots` across polls, checks `ready`, and returns `results` only after all
slots complete; suspension, waking, and cancellation follow the `Suspend[T]`
protocol. The standard `std.task.all!` is a compiler intrinsic with this
signature shape, not special control-flow syntax; its driver representation is
not specified here.
The child inputs are mutable `Suspend[T]` views so the driver can poll and
cancel them; an ordinary cold `fn!` call supplies such a view.

The input tuple is evaluated first, then each extra argument once from left
to right. Mapped calls execute left to right. A failed mapper call stops
evaluation just as an ordinary sequence of calls would. Each mapped call
performs normal requirement checking. A suspending `fn!` cannot be the mapper;
mapping itself does not introduce a suspension point. No heap list is created
by `pack.map`; `pack.map_list` constructs its declared list result.

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

The language does not provide general pack:

- filtering;
- indexing;
- splitting or concatenation;
- length arithmetic;
- iteration as a runtime sequence.

Pattern expansion and tuple mapping cover the accepted use cases without
creating a general compile-time metaprogramming language.

## `all!` Motivation

The standard-library `all!` combinator needs heterogeneous input and output:

```text
user, count, ready := all!(load_user(), load_count(), check_ready())
```

Its signature preserves each result type with one pack. Pack mapping lets its
library driver build and inspect per-child state without erasing result types.
Scheduling and cancellation are not variadic-generic semantics; they belong to
the concurrency library and suspension protocol.

## Expansion Validation

More than one expansion may occur in one parameter, tuple, or argument list.
Each expansion is checked independently. When multiple packs occur in one
expanded subtree, they expand in lockstep and must have the same length. When a
pack occurs beneath multiple generic constructors, every occurrence contributes
constraints to the same ordered sequence; incompatible element constraints are
reported at the first differing pack position.
