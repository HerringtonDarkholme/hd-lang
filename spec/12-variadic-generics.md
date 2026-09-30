# Variadic Generics

Status: language specification draft.

This chapter defines variadic generics.

1. r[pack.kind.definition] Variadic generics describe a statically known ordered pack of heterogeneous types and the corresponding pack of values.
2. r[pack.kind.purpose] They are intended for typed higher-order adapters and library concurrency combinators, not runtime dynamic lists.

## Pack Parameters

A generic parameter ending in `...` is a **type pack**:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)
```

```ebnf
generic_parameter = [ "reified" ], identifier, [ "..." ],
                    [ "<", trait_bounds ] ;
```

1. r[pack.param.type-pack] A generic parameter ending in `...` is a type pack.
2. r[pack.param.length] A pack has a compile-time length and ordered elements.
3. r[pack.param.not-collection] A pack is not a type whose runtime values are stored in a homogeneous collection.

## Pattern Expansion

Placing `...` at an expansion position repeats the containing type or
expression pattern once per pack element:

```text
fn all![Ts...](tasks: mut Suspend[Ts]...) -> (Ts...):
    ...
```

For `Ts... = User, i32, bool`, the patterns expand as follows:

| Pattern | Expansion |
| --- | --- |
| `mut Suspend[Ts]...` | parameter types `mut Suspend[User]`, `mut Suspend[i32]`, and `mut Suspend[bool]` |
| `(Ts...)` | `(User, i32, bool)` |
| a value pattern such as `start(tasks)...` | `start(tasks_0), start(tasks_1), ...` |

1. r[pack.expand.repeat] Placing `...` at an expansion position repeats the containing type or expression pattern once per pack element.
2. r[pack.expand.positions] Expansion is permitted in the positions in the table below.

| Expansion position |
| --- |
| function parameter type patterns |
| function and tuple type element positions |
| vararg value parameter patterns |
| call argument positions |
| type and expression patterns explicitly containing pack names |

The consolidated grammar exposes expansion positions directly:

```ebnf
type_argument = type, [ "..." ]
              | row_type_argument
              ;
type_element = type, [ "..." ] ;
value_parameter = identifier, ":", type, [ "=", expression ]
                | identifier, ":", type, "..."
                ;
positional_argument = expression
                    | continued_expression, "..."
                    ;
```

### Value-Pack Parameters

1. r[pack.value.positional] A function signature may contain at most one value-pack parameter that accepts positional arguments. Error: `multiple-positional-value-packs`.
2. r[pack.value.at-most-one] Because the current language has no named-only parameter separator, this means a signature may contain at most one value-pack parameter.
3. r[pack.value.final] Like an ordinary homogeneous vararg, that value-pack parameter must be the final positional parameter. A later positional parameter is an error. Error: `nonfinal-positional-value-pack`.
4. r[pack.value.no-partition] Calls never guess a partition between positional packs or between a pack and a later fixed parameter.

```text
fn split[As..., Bs...](left: As..., right: Bs...) -> ((As...), (Bs...)):  # error: multiple-positional-value-packs
    ((left...), (right...))

fn invalid[Ts...](values: Ts..., tail: i32) -> void:  # error: nonfinal-positional-value-pack
    pass
```

### Pack Ellipses

1. r[pack.ellipsis.expansion] An ellipsis is a pack expansion when the preceding subtree contains at least one pack reference.
2. r[pack.ellipsis.ordinary] Otherwise, parameter and argument ellipses retain their ordinary homogeneous-vararg and list-spread meanings.
3. r[pack.ellipsis.not-macro] Expansion is not an arbitrary syntax macro: only the designated type, parameter, tuple, and argument positions may repeat.

## Lockstep Expansion

One repeated pattern may reference several packs:

```text
fn zip_apply[As..., Bs..., Rs...](
    pairs: ((As, Bs)...),
    funcs: fn(As, Bs) -> Rs...,
) -> (Rs...):
    ...
```

1. r[pack.lockstep.equal-lengths] If one repeated pattern references multiple packs, expansion is positional and all referenced packs must have equal lengths. Error: `pack-length-mismatch`.
2. r[pack.lockstep.element] Element `i` from every pack is substituted into repetition `i`.

> **Note.** The `zip_apply` example is valid only when the three inferred
> packs have the same length. The ordinary `pairs` parameter carries a tuple
> whose repeated element type uses the same packs. The final `funcs`
> parameter is the signature's single positional value-pack parameter and
> expands the complete function-type subtree once per position.

The following call assumes the `zip_apply` declaration above:

```text
result := zip_apply(
    ((1, 2),),
    fn(a: i32, b: i32) -> i32: a + b,
    fn(a: string, b: string) -> string: a + b,  # error: pack-length-mismatch
)
```

## Pack Mapping

`pack.map` maps a statically known tuple to another tuple, and
`pack.map_list` maps it to a list.

1. r[pack.map.tuple] `pack.map(items, mapper, extras...)` maps a statically known tuple to another tuple.
2. r[pack.map.list] `pack.map_list(items, mapper, extras...)` maps the same input to a homogeneous `List[R]`.
3. r[pack.map.intrinsic] Both are compiler-recognized operations on tuples, not ordinary first-class functions or runtime reflection.
4. r[pack.map.tokens] The token sequences `pack.map(` and `pack.map_list(` always denote these operations, even where a local or parameter named `pack` is in scope.
5. r[pack.map.raw-method] A method named `map` on such a value is called through the raw identifier, as in `` `pack`.map(x) ``.

See also: [Keywords And Reserved Words](01-lexical-structure.md#keywords-and-reserved-words).

### Mapper Calls

1. r[pack.map.items] The first argument is evaluated once and must have tuple type `(T1, ..., Tn)`.
2. r[pack.map.mapper] The second argument names a non-suspending function; it is not evaluated as a function value.
3. r[pack.map.instantiate] The compiler type-checks and instantiates one call `mapper(item_i, extras...)` for each tuple element. A call that does not type-check is an error. Error: `pack-map-mapper-mismatch`.
4. r[pack.map.generic-mapper] A generic mapper may infer different type arguments for each call without requiring a first-class polymorphic function type.

```text
fn double(value: i32) -> i32:
    value * 2

let values: (i32, i32) = pack.map((1, "x"), double)  # error: pack-map-mapper-mismatch
```

### Mapping Results

1. r[pack.map.result] `pack.map` returns `(R1, ..., Rn)`, where `Ri` is the result type of mapper call `i`.
2. r[pack.map.list.element] `pack.map_list` requires all mapper results to be assignable to one element type `R` using the ordinary collection-literal inference rules.
3. r[pack.map.list.expected] An expected `List[R]` may provide that type.
4. r[pack.map.list.no-any] `pack.map_list` does not infer `Any` merely to combine heterogeneous results.
5. r[pack.map.empty] Mapping an empty tuple with `pack.map` returns `()`.
6. r[pack.map.list.empty] Empty `pack.map_list` requires an expected `List[R]` type.

### Tuple Expansion

1. r[pack.tuple.expand] The tuple expression `(values...)` expands a value pack into tuple elements; it is not a runtime pack object.
2. r[pack.tuple.no-pack] A tuple-element `...` without a pack reference is invalid.
3. r[pack.map.ordinary-tuple] Mapping also works on an ordinary tuple stored in a local variable, so a mapped tuple can be mapped again.

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
ready := pack.map_list(slots, poll_slot, context)  # List[bool]
results := pack.map(slots, take_ready)        # (Ts...)
```

> **Note.** The sketch shows only the typed transformations. Such a driver
> retains `slots` across polls, checks `ready`, and returns `results` only
> after all slots complete.

1. r[pack.all.protocol] In such a driver, suspension, waking, and cancellation follow the `Suspend[T]` protocol.
2. r[pack.all.intrinsic] The standard `std.task.all!` is a compiler intrinsic with this signature shape, not special control-flow syntax.
3. r[pack.all.driver] The driver representation of `std.task.all!` is not specified here.
4. r[pack.all.inputs] The child inputs are mutable `Suspend[T]` views so the driver can poll and cancel them.
5. r[pack.all.cold-call] An ordinary cold `fn!` call supplies such a view.

### Mapping Evaluation

1. r[pack.map.eval-order] The input tuple is evaluated first, then each extra argument once from left to right.
2. r[pack.map.call-order] Mapped calls execute left to right.
3. r[pack.map.failure] A failed mapper call stops evaluation just as an ordinary sequence of calls would.
4. r[pack.map.requirements] Each mapped call performs normal requirement checking.
5. r[pack.map.no-suspending-mapper] A suspending `fn!` cannot be the mapper.
6. r[pack.map.no-suspension-point] Mapping itself does not introduce a suspension point.
7. r[pack.map.no-heap] No heap list is created by `pack.map`.
8. r[pack.map.list.result] `pack.map_list` constructs its declared list result.

## Calls And Inference

This section defines how pack arguments are inferred and represented.

1. r[pack.infer.positions] Type-pack and value-pack inference uses the corresponding argument positions.
2. r[pack.infer.consistent] All uses of one pack in a signature must infer one length and one ordered type sequence.
3. r[pack.infer.mismatch] A mismatch is a compile-time error with element-position diagnostics.
4. r[pack.infer.inferred] Pack arguments are inferred.
5. r[pack.infer.no-explicit] Explicit pack arguments, partial explicit packs, and pack placeholders are not language constructs.

### Runtime Representation

1. r[pack.runtime.ordinary] At runtime, each expanded parameter is an ordinary parameter and `(Ts...)` is an ordinary tuple.
2. r[pack.runtime.no-hidden] No hidden `List[Any]`, reflection array, or allocation is required by the source semantics.

## Deliberate Limits

1. r[pack.limit.operations] The language does not provide general pack filtering, indexing, splitting or concatenation, length arithmetic, or iteration as a runtime sequence.

> **Why.** Pattern expansion and tuple mapping cover the accepted use cases
> without creating a general compile-time metaprogramming language.

## `all!` Motivation

The standard-library `all!` combinator needs heterogeneous input and output:

```text
let (user, count, ready) = all!(load_user(), load_count(), check_ready())
```

1. r[pack.all.signature] The signature of `all!` preserves each result type with one pack.
2. r[pack.all.child-state] Pack mapping lets its library driver build and inspect per-child state without erasing result types.
3. r[pack.all.scheduling] Scheduling and cancellation are not variadic-generic semantics; they belong to the concurrency library and suspension protocol.

## Expansion Validation

This section defines how the expansions of one list are checked.

1. r[pack.validate.multiple] More than one expansion may occur in one parameter, tuple, or argument list.
2. r[pack.validate.independent] Each expansion is checked independently.
3. r[pack.validate.lockstep] When multiple packs occur in one expanded subtree, they expand in lockstep and must have the same length.
4. r[pack.validate.constructors] When a pack occurs beneath multiple generic constructors, every occurrence contributes constraints to the same ordered sequence.
5. r[pack.validate.report] Incompatible element constraints are reported at the first differing pack position.
