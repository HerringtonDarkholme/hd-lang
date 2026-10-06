# Functions

Status: language specification draft.

This chapter defines functions, hd-lang's primary unit of behavior.

1. r[fn.kind.ordinary] Methods, entry points, tests, tools, workflows, and generated adapters are ordinary functions with additional library metadata or calling conventions.

## Declarations

A named function declares typed parameters and a result type.

```text
fn add(a: i32, b: i32) -> i32:
    a + b

fn load_user!(id: UserId) -> Result[User, DbError] $ Database:
    ...
```

1. r[fn.decl.suspending] A name ending in `!` declares a suspending function.
2. r[fn.decl.requirements] A trailing `$` clause declares requirements.

### Parameter And Result Types

1. r[fn.decl.parameter-types] Named functions must declare every parameter type.
2. r[fn.decl.result-required-pub] A public function, a public inherent method, a trait method, and a method of a trait implementation must also declare its result type. Omitting it is an error. Error: `missing-result-type`.
3. r[fn.decl.result-omitted-private] A non-public function, a non-public inherent method, or a local `fn` declaration may omit `-> type`.
4. r[fn.decl.result-inferred] Its result type is then inferred from its body, as for a nonrecursive closure.
5. r[fn.decl.result-inferred.common] The inferred result is the [least common type](04-type-system.md#least-common-type) of the body's final value and every `return` operand.
6. r[fn.decl.result-inferred.void] The inferred result is `void` when the body produces no value.
7. r[fn.decl.row-omitted] Such a function may also omit its requirement clause. Its row is then inferred as specified in [Requirements and Suspension](11-requirements-and-suspension.md).
8. r[fn.decl.suspension-not-inferred] Suspension is never inferred: it is part of the function's name.

```text
pub fn identity(x: i32): x  # error: missing-result-type

data Cart:
    count: i32

impl Cart:
    pub fn total(self):  # error: missing-result-type
        self.count
```

> **Why.** Every signature that another file can see is written out. So a
> caller in another file never waits for a callee's body to be checked.
> Files can then be checked in parallel.

### Recursion Without A Result Type

1. r[fn.decl.omitted-recursion] A function whose result type is omitted must not be recursive through functions whose result types are all omitted, itself included.
2. r[fn.decl.omitted-cycle] A cycle of functions with omitted result types that call each other, directly or through one another, is an error. Error: `recursive-function-needs-result-type`.
3. r[fn.decl.omitted-cycle.report] The error is reported once, on the member of the cycle that appears first in source order.
4. r[fn.decl.omitted-cycle.resolve] Adding a result type to any member of the cycle resolves it.

```text
fn is_even(n: i32):  # error: recursive-function-needs-result-type
    if n == 0: true
    else: is_odd(n - 1)

fn is_odd(n: i32):
    if n == 0: false
    else: is_even(n - 1)
```

### Bodies And Control Paths

1. r[fn.body.final-value] The body's normal final value must be assignable to the declared or inferred result.
2. r[fn.body.early-return] Explicit `return` may complete the function earlier.
3. r[fn.body.paths] Every reachable control path must do one of three things. It returns a value assignable to the declared result, falls through with such a final value, or completes abruptly by propagation or panic.
4. r[fn.body.value-less-fallthrough] A non-`void` function with a reachable value-less fallthrough is an error. Error: `missing-return-value`.
5. r[fn.body.infinite-loop] A path that enters an [infinite loop](06-control-flow.md#r-flow.while.infinite) that no `break` targets never completes, so it needs no value.
6. r[fn.body.void-final] A `void` function likewise rejects a non-`void` final expression.

```text
fn invalid(flag: bool) -> i32:
    if flag:  # error: missing-return-value
        return 1
```

> **Note.** Use `return`, `pass`, or another `void` expression when a
> preceding value is intentionally ignored.

### Function Names

1. r[fn.name.unique] Function names are unique in their scope.
2. r[fn.name.no-overloading] hd-lang has no function or method overloading: one name resolves to one declaration in its scope.

### Same-Line Bodies

1. r[fn.body.same-line] A same-line body is permitted when it contains one simple statement.

```text
fn test() -> void $ Console: println("hi")
```

### Local Functions

A named function may also be declared inside an executable block suite:

```text
fn total_with_bonus(values: List[i32], bonus: i32) -> i32:
    fn add_bonus(value: i32) -> i32:
        value + bonus

    let total = 0
    for value in values:
        total = total + add_bonus(value)
    total
```

1. r[fn.local.declare] A named function may be declared inside an executable block suite.
2. r[fn.local.scope] Its name is visible from that declaration onward and within its own body.
3. r[fn.local.capture-rules] It can capture enclosing local values under the same capture rules as a closure.
4. r[fn.local.private] It cannot be marked `pub` or used from another module.

See also: [Captures](#captures).

## Parameters

This section defines how parameters are initialized and passed.

```text
fn inspect(user: User) -> string: user.name

fn rename(user: mut User, name: string) -> void:
    user.name = name
```

1. r[fn.param.init-order] Parameters are evaluated and initialized from left to right after argument mapping.
2. r[fn.param.non-reassignable] Parameters are non-reassignable bindings: their names cannot be reassigned. Reassigning one is an error. Error: `non-reassignable-parameter-binding`.
3. r[fn.param.access] For composite parameters, `T` permits readonly access and `mut T` requires mutable access.
4. r[fn.param.primitive] Primitive parameters pass by value.
5. r[fn.param.composite] Composite parameters pass shared reference access according to their declared type.
6. r[fn.param.no-transfer] hd-lang does not perform ownership transfer at a call.

```text
fn normalize(value: i32) -> i32:
    value = 0  # error: non-reassignable-parameter-binding
    value
```

### Positional And Named Arguments

A call may mix positional and named arguments:

```text
resize(640, height=480)
```

1. r[fn.arg.mix] Calls may mix positional and named arguments.
2. r[fn.arg.positional-first] Positional arguments must come first.
3. r[fn.arg.positional-after-named] A positional argument must not follow a named argument.
4. r[fn.arg.labels] Named argument labels are parameter names and are part of the source-level calling interface.
5. r[fn.arg.unknown-name] A named argument that names no parameter is an error. Error: `unknown-named-argument`.
6. r[fn.arg.once] A parameter must receive one value, either explicitly or from its default.
7. r[fn.arg.duplicate] Supplying a parameter more than once is an error. Error: `duplicate-argument`.

```text
fn resize(width: i32, height: i32) -> i32: width * height

fn area() -> i32: resize(640, depth=480)   # error: unknown-named-argument
fn twice() -> i32: resize(640, width=480)  # error: duplicate-argument
```

### Default Values

A parameter may declare a default:

```text
fn connect(host: string, port: i32 = 443, tls: bool = true) -> Connection:
    ...
```

1. r[fn.default.allowed] A parameter may declare a default.
2. r[fn.default.order-final-function] After the first parameter with a default, every following non-vararg parameter must also have a default. The one exception is a final parameter whose type is a function type. A later parameter without one is an error. Error: `default-order`.
3. r[fn.default.omit] Calls may omit only parameters that have defaults.
4. r[fn.default.eval] Defaults are evaluated for each call, in parameter declaration order, after all explicit argument expressions have been evaluated.
5. r[fn.default.scope] A default may refer to earlier parameters but not later parameters.
6. r[fn.default.later-parameter] A default that names a later parameter is an error. Error: `binding-not-yet-visible`.
7. r[fn.default.final-function] A call may supply such a final function-typed parameter by a [trailing block](#trailing-callback-blocks) or a named argument, and omit the defaulted parameters before it.

```text
fn retry(times: i32, backoff: i32 = 100, body: fn() -> void) -> void:
    body()

fn run() -> void:
    retry(3):
        pass
    retry(3, backoff=10):
        pass
    retry(3, body=fn(): pass)
```

> **Why.** As in Kotlin and Swift, defaulted options may come before a
> trailing body. A trailing block or a named argument always reaches
> the final parameter.

#### Requirement-Free Defaults

1. r[fn.default.requirement-free] A default expression must be **requirement-free**: it must not use a provider and must not suspend.
2. r[fn.default.requirement-free.syntax] Concretely, it must not contain `$.use`, a provider scope, or a bang call.
3. r[fn.default.requirement-free.calls] Every function, method, closure, or function value it calls must have an empty requirement row.
4. r[fn.default.requirement-free.signatures] The check reads only signatures, never function bodies, because both properties are part of every callable's type.
5. r[fn.default.requirement-free.uniform] The check applies equally to named functions, function values, and dynamic trait methods.
6. r[fn.default.effects] A default may otherwise evaluate any expression, including calls that mutate state.
7. r[fn.default.provider] A default that uses a provider, in a parameter, data field, or shared enum constructor parameter, is an error. Error: `requirement-in-default`.
8. r[fn.default.suspends] A default that suspends is an error. Error: `suspension-forbidden-context`.

```text
trait Clock:
    fn now(self) -> i32

fn current_time() -> i32 $ Clock:
    $.use(Clock).now()

fn wait!() -> i32: 1

fn bad(first: i32 = 1, second: i32) -> i32: second                     # error: default-order
fn span(start: i32 = finish, finish: i32 = 10) -> i32: finish - start  # error: binding-not-yet-visible
fn stamp(time: i32 = current_time()) -> i32 $ Clock: time              # error: requirement-in-default
fn late(value: i32 = wait!()) -> i32: value                            # error: suspension-forbidden-context
```

> **Note.** Because defaults run after all explicit arguments, in parameter
> declaration order, the effects of a default are ordered.

### Varargs

A final positional parameter may write `...` after its name. It collects
the call's remaining positional arguments into its declared type:

```text
fn sum(values...: List[i32]) -> i32:
    ...
```

1. r[fn.vararg.form] A final positional parameter may write `...` after its name, as in `values...: List[i32]`. It is then a **vararg**.
2. r[fn.vararg.type] The type after `:` is the parameter's type inside the body, as for any parameter, so `values` above is a `List[i32]`.
3. r[fn.vararg.type.kinds] A vararg whose type is not `List[T]`, a tuple type, or a type parameter bounded by `Tuple` is an error. Error: `type-mismatch`.
4. r[fn.vararg.collect] A call supplies a vararg zero or more separate positional arguments, which are collected into its type, or one [positional spread](05-expressions.md#positional-spreads) of that type.

```text
sum()
sum(1, 2, 3)
sum(items...)
```

1. r[fn.vararg.collect.list] For `List[T]`, each argument is checked against `T`, and the list holds the arguments in order.
2. r[fn.vararg.collect.tuple-expr] For a tuple type, the arguments are collected exactly as the tuple expression of them, against that type, by [Tuple Expressions](05-expressions.md#parenthesized-and-tuple-expressions) and [Tuple Rest Elements](05-expressions.md#tuple-rest-elements). So `call(g, 1, 2, xs...)` collects `(1, 2, xs...)`. Error: `type-mismatch`.
3. r[fn.vararg.tuple-param.expected] Another argument may solve the vararg's `Tuple`-bounded type parameter, as `f` solves `Args` below. Then the vararg's arguments are checked against that solved tuple as an expected type.
4. r[fn.vararg.tuple-param.infer] When no other argument solves it, inference solves it as the tuple of the argument types, one element per argument, with no join.
5. r[fn.vararg.no-auto-spread] A tuple argument is never spread automatically: it is one element of the collected tuple.

```text
use std.function.{Fn, Tuple}

fn add2(a: i32, b: i32) -> i32: a + b
fn neg(a: i32) -> i32: -a
fn zero() -> i32: 0
fn g(a: usize, b: usize, xs...: List[i32]) -> usize: a + b + xs.len()

fn call[Args < Tuple, O, $R](f: Fn[Args, O, $ R], args...: Args) -> O $ R:
    f(args...)

fn pack[Args < Tuple](args...: Args) -> Args: args

fn run(pair: (i32, i32)) -> i32:
    call(add2, 1, 2) + call(neg, 5) + call(zero) + call(add2, pair...)

fn tail(xs: List[i32]) -> usize: call(g, 1, 2, 3, 4) + call(g, 1, 2, xs...)
```

| Call | `Args` |
| --- | --- |
| `call(add2, 1, 2)` | `(i32, i32)`, from `add2` |
| `call(neg, 5)` | `(i32,)` |
| `call(zero)` | `()` |
| `call(add2, pair...)` | `(i32, i32)`, which the spread matches |
| `call(g, 1, 2, 3, 4)` | `(usize, usize, List[i32]...)`, from `g`; `3` and `4` form the rest |
| `call(g, 1, 2, xs...)` | `(usize, usize, List[i32]...)`, from `g`; `xs` is the rest |
| `call(add2, (1, 2))` | `(i32, i32)`, which the one argument `(1, 2)` does not fill |
| `pack(1, "a")` | `(i32, string)`, from the arguments |

1. r[fn.vararg.final] A vararg that is not the last positional parameter is an error. Error: `nonfinal-vararg`.
2. r[fn.vararg.no-default] A vararg has no default expression.
3. r[fn.vararg.named] Passing a vararg by name passes its collected value without spread syntax, as in `sum(values=[1, 2])` or `call(add2, args=(1, 2))`.

A function value keeps a `List[T]` vararg, as [Vararg Inputs](#vararg-inputs)
states:

```text
fn count(values...: List[i32]) -> usize: values.len()

fn main() -> usize:
    f := count
    f(3, 4, 5)
```

Here `f` has type `fn(List[i32]...) -> usize`.

```text
use std.function.{Fn, Tuple}

fn scaled(values...: List[i32], factor: i32) -> i32: factor  # error: nonfinal-vararg
fn count(values...: i32) -> i32: values                       # error: type-mismatch
fn apply(callback: fn(i32...) -> i32) -> i32: 0               # error: type-mismatch

fn add2(a: i32, b: i32) -> i32: a + b
fn call[Args < Tuple, O, R](f: Fn[Args, O, $ R], args...: Args) -> O $ R: f(args...)
fn nested() -> i32: call(add2, (1, 2))                         # error: type-mismatch
```

> **Why.** Declaring `args...` mirrors spreading `args...`. The type after
> `:` is the type the body sees, so a vararg needs no type rule of its own.
> A `List[T]` vararg shows in the function's type as a rest element, so a
> function value keeps its varargs.

See also: [Positional Spreads](05-expressions.md#positional-spreads).

## Function Types And Values

Functions are values. A plain function type lists parameter types and a
result:

```text
fn(string) -> string
fn!(UserId) -> Result[User, DbError] $ Database
```

1. r[fn.type.values] Functions are values.
2. r[fn.type.form] A plain function type lists parameter types and a result.
3. r[fn.type.no-names] Parameter names and default values are not part of a function value type.
4. r[fn.type.positional] Calling through a function value therefore uses positional arguments only and does not inherit declaration defaults.
5. r[fn.type.parts] Suspension and requirement rows are part of the type.

### Function Type Constructors

Every function type is exact sugar for one of two standard constructors that
`std.function` declares:

| Sugar | Constructor form |
| --- | --- |
| `fn(A, B) -> O $ R` | `Fn[(A, B), O, $ R]` |
| `fn!(A, B) -> O $ R` | `SuspendFn[(A, B), O, $ R]` |
| `fn(A) -> O` | `Fn[(A,), O, $()]` |
| `fn() -> O $ Db + Cache` | `Fn[(), O, $ Db + Cache]` |

```text
use std.function.{Fn, SuspendFn}

trait Database

fn check(id: i32, name: string) -> bool: id > 0
fn load!(id: i32) -> string $ Database: "user"

fn spelled() -> Fn[(i32, string), bool, $()]:
    check

fn sugared(callback: Fn[(i32, string), bool, $()]) -> fn(i32, string) -> bool:
    callback

fn suspending() -> SuspendFn[(i32,), string, $ Database]:
    load
```

1. r[fn.type.ctor.decl] `std.function` declares the function type constructors `Fn` and `SuspendFn`. Each takes three arguments: the inputs, the output, and the requirement row.
2. r[fn.type.ctor.inputs] The inputs argument is one tuple type whose elements are the parameter types, such as `()`, `(A,)`, or `(A, B)`. It may also be a type parameter bounded by `Tuple`.
3. r[fn.type.ctor.no-flatten] A tuple is never flattened into parameters: `Fn[((A, B),), O, $ R]` takes one pair, and `Fn[(A, B), O, $ R]` takes two values.
4. r[fn.type.ctor.row] The row argument is row-kinded. A function type without a requirement clause has the empty row `$()`, and several keys are joined with `+`, as in `$ Db + Cache`.
5. r[fn.type.ctor.row.dollar] The row argument is a [row slot](11-requirements-and-suspension.md#row-slots), so one key or a row alias is written after `$` too, as in `Fn[(), O, $ Db]` and `Fn[(), O, $ AppRow]`. A bare one is an error. Error: `generic-kind-mismatch`.
6. r[fn.type.ctor.tuple-trait] `std.function` also declares `Tuple`, an empty sealed marker trait that every tuple type implements, `()` and `(A,)` included.
7. r[fn.type.ctor.tuple-trait.rest] A tuple type with a [rest element](04-type-system.md#rest-elements) is a tuple type and implements `Tuple`, so `Args` may be `(i32, List[i32]...)`.
8. r[fn.type.ctor.inputs-tuple] An inputs argument that is not a tuple type or a type parameter bounded by `Tuple` is an error. Examples are `Fn[i32, i32, $()]`, and `Fn[Args, O, $ R]` with an unbounded `Args`. Error: `generic-kind-mismatch`.
9. r[fn.type.ctor.sugar] `fn(A) -> O $ R` and `Fn[(A,), O, $ R]` denote the same type, and so do `fn!(A) -> O $ R` and `SuspendFn[(A,), O, $ R]`.
10. r[fn.type.ctor.anywhere] Either spelling is valid anywhere a type may appear, including implementation targets.
11. r[fn.type.ctor.diagnostics] Diagnostics print a function type in its sugar form, as they print `T?` for `Option[T]`.
12. r[fn.type.ctor.imports] The sugar needs no import. The names `Fn`, `SuspendFn`, and `Tuple` are imported where they are written, as in `use std.function.Fn`.
13. r[fn.type.ctor.opaque-sources] The constructors have no fields and no construction syntax. Function values come only from function names, closures, one-payload variant constructors, instantiated generic functions, and [method references](#method-references).

```text
use std.function.{Fn, Tuple}

trait Describe:
    fn describe(self) -> string

impl[Args < Tuple, O, $R] Describe for Fn[Args, O, $ R]:
    fn describe(self) -> string: "function"

fn invalid(callback: Fn[i32, i32, $()]) -> void: pass             # error: generic-kind-mismatch
fn unbounded[Args, O](callback: Fn[Args, O, $()]) -> void: pass  # error: generic-kind-mismatch
```

#### Vararg Inputs

A function with a `List[T]` vararg has inputs that end in the
[rest element](04-type-system.md#rest-elements) `List[T]...`, and its
function value keeps the vararg:

```text
fn g(a: usize, b: usize, xs...: List[i32]) -> usize: a + b + xs.len()
fn h(a: usize, b: usize, xs: List[i32]) -> usize: a + b + xs.len()

fn apply(callback: fn(usize, usize, List[i32]...) -> usize) -> usize:
    f := g
    f(1, 2, 3, 4) + callback(1, 2) + h(1, 2, [3, 4])
```

| Function | Type | Constructor form |
| --- | --- | --- |
| `g` | `fn(usize, usize, List[i32]...) -> usize` | `Fn[(usize, usize, List[i32]...), usize, $()]` |
| `h` | `fn(usize, usize, List[i32]) -> usize` | `Fn[(usize, usize, List[i32]), usize, $()]` |

1. r[fn.type.vararg-rest] A `List[T]` vararg is the rest element `List[T]...` of its function's inputs tuple. So `g` above has type `fn(usize, usize, List[i32]...) -> usize`, which differs from `h`'s type.
2. r[fn.type.rest-call] Calling a function value whose inputs end in a rest element `List[T]...` treats that element as a `List[T]` vararg, as in `f(1, 2, 3, 4)` above.
3. r[fn.type.tuple-vararg-input] A vararg whose type is a tuple type or a `Tuple`-bounded type parameter is one ordinary input of its function's type. That input takes the collected tuple. `call` has type `fn(Fn[Args, O, $ R], Args) -> O $ R`.

```text
fn sum(values...: List[i32]) -> i32: values[0]
fn apply_pair(callback: fn(i32, i32) -> i32) -> i32: callback(1, 2)
fn apply_list(callback: fn(List[i32]) -> i32) -> i32: callback([1, 2])

fn first() -> i32: apply_pair(sum)  # error: type-mismatch
fn second() -> i32: apply_list(sum)  # error: type-mismatch
```

#### No Access Permission

1. r[fn.type.no-permission] A function value carries no access permission, and calling one never needs mutable access to it.
2. r[fn.type.no-mut] `mut` applied to a function type, as in `mut (fn() -> i32)` or `mut Fn[(), i32, $()]`, is invalid.
3. r[fn.type.no-mut.syntax] `mut` written directly before `fn`, in a type or a closure header, is an error. Error: `syntax-error`.

```text
fn counter() -> mut fn() -> i32:  # error: syntax-error
    fn() -> i32: 1
```

> **Why.** A function value has no fields to take a permission on. A closure
> may mutate its captures freely, so the type needs no mutation capability.

### Passing Function Values

1. r[fn.type.named-value] A named function value may be passed anywhere its function type is expected.
2. r[fn.type.variant-value] So may a variant constructor with exactly one payload field, such as `SyncError.Fs` of type `fn(FsError) -> SyncError`.
3. r[fn.type.declared-variance] Function types follow the declared variance of `Fn` and `SuspendFn`: contravariant in each input element, covariant in the output, and invariant in the row.
4. r[fn.type.variance-repr] Like every variance conversion, a function-type conversion must be representation-preserving, so it changes only access permissions.
5. r[fn.type.variance-repr.excluded] Trait-value construction and optional injection therefore never convert a parameter or result of a function type. Error: `variance-representation-change`.

```text
data User:
    name: string

fn fresh() -> mut User: User { name: "Ada" }
fn count() -> i32: 1

fn readonly_maker() -> fn() -> User:
    fresh

fn widen(callback: fn(User) -> mut User) -> fn(mut User) -> User:
    callback

fn erase() -> fn() -> Display:
    count  # error: variance-representation-change
```

> **Why.** The polarity rules of [Variance](04-type-system.md#variance)
> already treat parameters as negative and results as positive. Declaring
> that variance on the constructors removes a special case.

> **Note.** A function value also fits a function type whose row is wider
> than its own. That is [row subsumption](11-requirements-and-suspension.md#row-subsumption),
> not a variance conversion, and it may adapt the value.

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations),
[Representation-Preserving Variance](04-type-system.md#representation-preserving-variance).

### Generic Function Values

A generic function passed as an argument takes its type arguments from the
call:

```text
fn identity[T](value: T) -> T:
    value

fn apply[A, B](value: A, f: fn(A) -> B) -> B:
    f(value)

count := apply(3, identity)
```

1. r[fn.type.generic.not-polymorphic] Generic functions are not first-class polymorphic values.
2. r[fn.type.generic.instantiate-every] Referring to one as a value must instantiate every generic parameter.
3. r[fn.type.generic.instantiate-from] The type arguments come from an expected monomorphic function type, from an explicit type-argument list, or from the call the value is an argument of.
4. r[fn.type.generic.placeholder] A placeholder in that list may be solved from the expected monomorphic type.
5. r[fn.type.generic.argument] When the value is an argument of a call, its type arguments are solved together with the call's other type variables.
6. r[fn.type.generic.argument.sources] Those variables are solved from the other arguments, the expected result type, and the called function's constraints, as for the call itself.
7. r[fn.type.generic.default] A parameter that those sources leave unsolved takes its [default](04-type-system.md#type-argument-defaults), as at a call. For `fn empty[C = List[i32]]() -> C`, `make := empty` has type `fn() -> List[i32]`.
8. r[fn.type.generic.unsolved] A generic parameter of the value that remains unsolved is an error. Error: `cannot-infer-type`.
9. r[fn.type.generic.monomorphic] The resulting value has an ordinary monomorphic function type.

In `count := apply(3, identity)`, `A` is `i32` from the first argument, and
`identity`'s `T` and the call's `B` are solved as `i32` with it.

```text
fn identity[T](value: T) -> T:
    value

fn call_with_nothing[A, B](f: fn(A) -> B) -> void:
    pass

fn main() -> void:
    call_with_nothing(identity)  # error: cannot-infer-type
```

> **Why.** Only use sites infer. A function declaration's own generic
> parameters and signature are always written, never inferred from its body
> or its callers.

See also: [Type Inference Boundaries](04-type-system.md#type-inference-boundaries),
[Variant Constructors As Function Values](08-data-and-enums.md#variant-constructors-as-function-values).

### Suspending Function Values

1. r[fn.type.suspend.call] A value of type `fn!(A) -> T $ R` constructs `mut Suspend[T]` when called normally.
2. r[fn.type.suspend.bang] It may be bang-called inside a suspending body.
3. r[fn.type.suspend.weaken] It may be weakened to the lowered constructor type `fn(A) -> mut Suspend[T] $ R`.
4. r[fn.type.suspend.no-reverse] The reverse conversion is not implicit.
5. r[fn.type.suspend.ctor] In constructor form, the weakening converts `SuspendFn[I, O, $ R]` to `Fn[I, mut Suspend[O], $ R]`.

### Method References

A **method reference** names a method or an associated function as a
function value. It is the qualified call without its arguments:

```text
data Counter:
    value: i32

impl Counter:
    fn bump(mut self, by: i32) -> void:
        self.value = self.value + by

    fn read(self) -> i32:
        self.value

    fn zero() -> Counter:
        Counter { value: 0 }

fn reader() -> fn(Counter) -> i32:
    Counter::read

fn maker() -> fn() -> Counter:
    Counter::zero

fn bumper(counter: mut Counter) -> fn(i32) -> void:
    counter::bump
```

1. r[fn.ref.unbound] `Owner::name` without an argument clause, where `Owner` names a type, a trait, or a type parameter, is an **unbound method reference**.
2. r[fn.ref.unbound.receiver] For a method, the reference takes the receiver first, then the method's own parameters: `Counter::bump` has type `fn(mut Counter, i32) -> void`.
3. r[fn.ref.unbound.mut-self] A `mut self` method gives a `mut` first parameter, and a `self` method a readonly one.
4. r[fn.ref.associated] For an associated function, the parameters are the function's own: `Counter::zero` has type `fn() -> Counter`.
5. r[fn.ref.lookup] A reference resolves `name` as the qualified call `Owner::name(...)` does. That is inherent members of a type first, then its available traits, and a type parameter through its bounds.
6. r[fn.ref.lookup.ambiguous] Two trait candidates for one name are an error. Error: `ambiguous-method`.
7. r[fn.ref.trait-self] For `Trait::name`, `Self` is inferred from the expected function type, as a generic function value's parameters are.
8. r[fn.ref.trait-self.unsolved] A trait reference whose `Self` nothing determines is an error. Error: `cannot-infer-type`.
9. r[fn.ref.generic] A generic member's reference writes its type arguments after the name, as in `Json::decode::[User]`, and the owner's before the member's `::`, as in `Box::[i32]::get`.
10. r[fn.ref.generic.instantiate] Every type parameter of the member is instantiated as for a [generic function value](#generic-function-values).
11. r[fn.ref.bound] `value::name`, where `value` names a value rather than a type or trait, is a **bound method reference**.
12. r[fn.ref.bound.capture] It evaluates `value` once, when the reference is created, and captures that receiver. Calling the reference later calls the method on the captured receiver, whatever `value` names by then.
13. r[fn.ref.bound.type] Its parameters are the method's own, without the receiver: `counter::bump` has type `fn(i32) -> void`.
14. r[fn.ref.bound.mut] A bound reference to a `mut self` method needs mutable access to the receiver when it is created. Error: `mutable-receiver-required`.
15. r[fn.ref.call] A reference followed by an argument clause is an ordinary call, so `user::domain()` calls `domain` on `user`.
16. r[fn.ref.suspending] A suspending method is referenced without `!`, as `Store::load`, and the reference has a suspending function type such as `fn!(Store, Key) -> Blob`.
17. r[fn.ref.value] A reference is an ordinary function value: it has no parameter names or defaults, and it carries the member's row and suspension.
18. r[fn.ref.no-fields] `::` names only methods and associated functions, never fields. `User::email` for a field `email` is an error. Error: `unknown-method`.
19. r[fn.ref.bound.associated] A bound reference names a method, since an associated function has no receiver to bind. `counter::zero` for an associated function `zero` is an error. Error: `unknown-method`.

```text
data Counter:
    value: i32

impl Counter:
    fn bump(mut self, by: i32) -> void:
        self.value = self.value + by

    fn zero() -> Counter:
        Counter { value: 0 }

data User:
    email: string

fn emails() -> fn(User) -> string:
    User::email  # error: unknown-method

fn frozen(counter: Counter) -> fn(i32) -> void:
    counter::bump  # error: mutable-receiver-required

fn restart(counter: Counter) -> fn() -> Counter:
    counter::zero  # error: unknown-method
```

> **Why.** A reference reads as the call it stands for, as in Rust, Java,
> and Go. A bound reference fixes its receiver when it is made, as Kotlin
> and Go method values do. A closure such as `fn(): counter.read()`
> instead reads the variable when it runs. Fields stay closures, so a field
> and a method may share a name without a clash.

See also: [Associated Function Calls](09-traits.md#associated-function-calls),
[Trait-Qualified Calls](09-traits.md#trait-qualified-calls).

## Closures

A closure uses `fn` without a name:

```text
initial := fn(text: string) -> string:
    text.slice(0, 1)
```

1. r[fn.closure.form] A closure uses `fn` without a name.
2. r[fn.closure.no-other-syntax] There is no separate arrow or shorthand-argument closure syntax.
3. r[fn.closure.one-line] A one-line closure uses the same `:` suite syntax.

```text
inc := fn(x: i32) -> i32: x + 1
```

4. r[fn.closure.monomorphic] A closure declares no type parameters and always has a monomorphic function type.
5. r[fn.closure.monomorphic.sources] Its parameter and result types come from its annotations or from the expected function type, as [Closure Annotations](#closure-annotations) states.
6. r[fn.closure.no-type-parameters] A closure header has no place for a type-parameter list, so `fn[T](x: T): x` is an error. Error: `syntax-error`.

```text
fn run() -> i32:
    same := fn[T](value: T) -> T: value  # error: syntax-error
    0
```

> **Why.** A closure is a value, and generic functions are not first-class
> polymorphic values
> ([`fn.type.generic.not-polymorphic`](#r-fn.type.generic.not-polymorphic)).
> A method reference is instantiated the same way
> ([`fn.ref.generic.instantiate`](#r-fn.ref.generic.instantiate)).

### Suspending Closures And Clauses

A suspending closure places `!` after `fn`:

```text
loader := fn!(id: UserId) -> Result[User, DbError] $ Database:
    db := $.use(Database)
    db.load_user!(id)
```

1. r[fn.closure.suspending] A suspending closure places `!` after `fn`.
2. r[fn.closure.requirements] A requirement clause follows its result type.
3. r[fn.closure.clause-owner] The clause directly before a header's `:` always belongs to the function or closure being declared, even when its result is a function type.
4. r[fn.closure.clause-grouped] A returned function type with its own row is parenthesized, as in `fn make() -> (fn() -> i32 $ Log) $ Console:`.

See also: [Types](02-grammar.md#types).

### Closure Annotations

When an expected function type is available, an inline closure may omit
parameter and result annotations:

```text
fn apply_all(values: List[i32], transform: fn(i32) -> i32) -> List[i32]:
    [for value in values => transform(value)]

doubled := apply_all(counts, fn(count): count * 2)
```

1. r[fn.closure.annotations-omitted] When an expected function type is available, an inline closure may omit parameter and result annotations.
2. r[fn.closure.needs-annotation] Without a sufficient expected type, parameters must be annotated. An unannotated parameter is then an error. Error: `closure-parameter-needs-annotation`.
3. r[fn.closure.result-inferred] A nonrecursive closure may infer its result type from its body.
4. r[fn.closure.result-inferred.common] The inferred result is the [least common type](04-type-system.md#least-common-type) of the body's final value and every `return` operand. When they have no common type, the closure is an error. Error: `no-common-type`.
5. r[fn.closure.result-expected] An expected function type may instead supply the result type.
6. r[fn.closure.recursive-result] A recursive local closure must always write its result type explicitly, even if an expected function type could supply it. Error: `recursive-closure-needs-result-type`.

```text
fn run() -> i32:
    closure := fn(value) -> i32: value  # error: closure-parameter-needs-annotation
    0
```

### Captures

This section defines what a closure captures and how it may use its captures.

1. r[fn.capture.locals] A closure captures local bindings that it references from enclosing lexical scopes.
2. r[fn.capture.no-providers] A closure captures no provider from an enclosing `$.with` scope: each requirement key its body uses goes into its row ([`req.row.omitted.outer-scope`](11-requirements-and-suspension.md#r-req.row.omitted.outer-scope)).
3. r[fn.capture.provider-value] A provider value bound to a local by `$.use` is captured like any other local, and stays bound after its provider scope ends.

```text
trait Clock:
    fn now(self) -> i32

fn make_reader() -> fn() -> i32 $ Clock:
    clock := $.use(Clock)
    fn() -> i32: clock.now()
```

See also: [Lexical And Dynamic Providers](11-requirements-and-suspension.md#lexical-and-dynamic-providers).

#### Plain Closures

Every closure is a plain `fn` closure, and it may mutate through its
captures:

```text
data User:
    name: string

fn rename(user: mut User) -> void:
    user.name = "renamed"

fn plain_closure(user: mut User) -> fn() -> void:
    fn() -> void:
        rename(user)

fn make_appender(items: mut List[i32]) -> fn(i32) -> void:
    fn(value: i32) -> void:
        items.push(value)
```

1. r[fn.capture.access] A closure uses each capture with the access that the captured binding has in the enclosing scope.
2. r[fn.capture.mutate] A closure may assign captured `let` storage and may obtain mutable access from a captured `mut T` binding.
3. r[fn.capture.mutate.forms] Calling a `mut self` method on a captured list or mutable child is such a use. So are passing a captured `mut T` binding to a `mut T` parameter, and returning that access.
4. r[fn.capture.readonly] A readonly capture stays readonly. Assigning a field through a captured readonly root is an error. Error: `readonly-root`.
5. r[fn.capture.result-not-weakened] A callable's declared `mut T` result is not itself weakened when the callable is read through a readonly reference.

```text
data User:
    name: string

fn rename(user: User) -> void:
    change := fn() -> void:
        user.name = "Grace"  # error: readonly-root
    change()
```

#### Mutable Closures

A closure that mutates captured state is an ordinary closure with an ordinary
function type:

```text
let count = +0

let next: fn() -> i32 = fn() -> i32:
    count = count + 1
    count
```

1. r[fn.capture.no-mut-form] There is no mutable closure type or literal. A closure that mutates captured state has an ordinary `fn(...) -> T` type.
2. r[fn.capture.storage.shared] Captured `let` storage is shared with its defining scope and other closures that capture the same binding.
3. r[fn.capture.storage.lifetime] If a closure outlives the original stack activation, the runtime preserves its captured storage through garbage collection.

> **Why.** hd has no unique borrows, so a mutation capability on function
> types would protect nothing. Swift, Kotlin, and Go closures also mutate
> their captures freely.

#### Scope Of This Section

1. r[fn.capture.in-process] This section defines ordinary in-process closure behavior only.
2. r[fn.capture.serializable-deferred] Serializable closure capture, how a restored closure names its code beyond [the same compiled program](11-requirements-and-suspension.md#r-req.determinism.same-program), and restoration semantics are deferred to the runtime design.

## Multiple Inline Closures

Multiple multiline closures may be passed by parenthesizing each closure
expression and separating the arguments with commas:

```text
choice(
    (
        fn(a):
            println(a)
    ),
    (
        fn(b):
            println(b)
    ),
)
```

1. r[fn.multi.parenthesized] Multiple multiline closures may be passed by parenthesizing each closure expression and separating the arguments with commas.
2. r[fn.multi.commas] Newlines do not replace commas in argument lists.
3. r[fn.multi.unparenthesized] Without parentheses, the line after an indented closure body must start with `,` or a closing delimiter. It must be indented no farther than the line holding the closure header.
4. r[fn.multi.next-closure] The next closure may start on that line, as in `, fn(b):`.
5. r[fn.multi.syntax-error] A later argument written at body indentation, or a closing delimiter at the end of a body line, is a `syntax-error`.

> **Why.** Parentheses make each multiline closure's boundary explicit.

See also: [Physical And Logical Lines](01-lexical-structure.md#physical-and-logical-lines).

## Trailing Callback Blocks

When the final parameter has a zero-argument function type, a call may
supply it as an indented trailing block:

```text
result := when(a, b):
    compute_result()

transaction:
    save_user()
```

1. r[fn.trailing.form] When the final parameter has a zero-argument function type, a call may supply it as an indented trailing block.
2. r[fn.trailing.arguments] Ordinary arguments remain in parentheses.
3. r[fn.trailing.empty-parentheses] If there are no ordinary arguments, empty `()` is omitted.
4. r[fn.trailing.closure] The trailing block is equivalent to a zero-argument closure whose result and behavior are contextually inferred from the final parameter.
5. r[fn.trailing.position] A trailing block call may be a complete statement or the complete right-hand side of `:=`, `let ... =`, `=`, `_ :=`, `return`, or `break`. Examples are `total = sum_of(items):` and `return retry(3):`, each followed by the block.
6. r[fn.trailing.one] Only one trailing block is permitted, and only for a zero-argument final parameter. Error: `trailing-block-position`.
7. r[fn.trailing.parameterized] Parameterized callbacks use explicit closure syntax.
8. r[fn.trailing.return] `return` inside the block returns from the generated callback, not from the enclosing function.
9. r[fn.trailing.suspending] When the final parameter has a suspending function type, as in `body: fn!() -> T`, the trailing block is a suspending closure. This holds for every callee.
10. r[fn.trailing.suspending.body] Such a block is a suspending body, so it may make bang calls.

```text
transaction:
    save_user()
:  # error: trailing-block-position
    save_again()
```

A trailing block for an `fn!` parameter may make bang calls:

```text
fn fetch_count!() -> i32: 3

fn twice!(body: fn!() -> i32) -> i32:
    body!() + body!()

fn total!() -> i32:
    count := twice!():
        fetch_count!()
    count
```

> **Why.** The `!` in the callee's parameter type marks the block as
> suspending, as a Kotlin `suspend` function-type parameter does for its
> trailing lambda.

## Generic Functions

Generic parameters follow the function name:

```text
fn first[T](items: List[T]) -> T?:
    ...
```

Trait bounds compose with `&`:

```text
fn audit[T < Display & Named](value: T) -> string:
    ...
```

Callers may rely on inference or write an explicit type argument list:

```text
first(names)
first::[string](names)
```

1. r[fn.generic.parameters] Generic parameters follow the function name.
2. r[fn.generic.bounds-and] Trait bounds compose with `&`.
3. r[fn.generic.call-list] Callers may rely on inference or write an explicit type argument list, which may omit trailing slots.
4. r[fn.generic.call-marker] In an expression, the explicit list follows `::`, as in `first::[string](names)`, while a type keeps `List[string]`.
5. r[fn.generic.erased] Generic parameters are erased.

### Explicit Type Arguments

1. r[fn.generic.explicit.trailing] An explicit type-argument list may omit trailing slots. Each omitted slot is inferred as a `_` slot is, then takes its default if inference leaves it unsolved.
2. r[fn.generic.explicit.too-long-count] A list with more slots than the function has generic parameters is an error. Error: `argument-count`.
3. r[fn.generic.explicit.row-dollar] The slot of a row parameter takes a [row type argument](02-grammar.md#row-type-arguments), written after `$` even for one key or a row alias. Examples are `::[$ Log]`, `::[$ AppRow]`, and `::[$()]`. A bare key or row alias there is an error. Error: `generic-kind-mismatch`.

```text
fn pair[Left, Right](left: Left, right: Right) -> (Left, Right):
    (left, right)

value := pair::[string]("left", 1)  # Right is inferred as usize, the fallback
```

```text
fn pair[Left, Right](left: Left, right: Right) -> (Left, Right):
    (left, right)

fn triple() -> (string, i32):
    pair::[string, i32, bool]("left", 1)  # error: argument-count
```

> **Why.** A list names only the leading arguments the reader should see or
> inference cannot find, as C++ deduces trailing template arguments. The
> rest are inferred, or take their defaults.

An explicit list may write `_` in any slot to infer that argument:

```text
fn convert[From, To](value: From) -> To:
    ...

user := convert::[_, User](payload)
```

1. r[fn.generic.placeholder] An explicit list may write `_` in any slot to infer that argument.
2. r[fn.generic.placeholder.solve] A placeholder is solved from call arguments, the expected result type, and the function's generic constraints.
3. r[fn.generic.placeholder.default] When those constraints do not determine one type, the parameter's default applies.
4. r[fn.generic.placeholder.unsolved] A placeholder that neither the constraints nor a default determine is an error. Error: `cannot-infer-type`.
5. r[fn.generic.placeholder.not-type] `_` is a call-site inference instruction, not a type, and cannot appear in an ordinary type argument list such as `List[_]`.

```text
fn make[T]() -> T:
    panic("not implemented")

value := make::[_]()  # error: cannot-infer-type
```

### Generic Methods And Qualified Calls

The same explicit-list rules apply to generic methods:

```text
parser.parse::[User](text)
parser.convert::[_, User](payload)
```

1. r[fn.generic.methods] The same explicit-list rules apply to generic methods.
2. r[fn.generic.qualified] They also apply to qualified calls of generic associated functions and trait methods.
3. r[fn.generic.qualified.member-list] The method-level list follows the member name, as in `Type::name::[T](...)` and `Trait::name::[T](receiver, ...)`.
4. r[fn.generic.qualified.owner-list] Type arguments of the qualifying type or trait stay before the member's `::`, as in `Add::[Money]::add(left, right)`.
5. r[fn.generic.bang] A bang call keeps the `!` on the name, as the declaration does, and writes the list after it.
6. r[fn.generic.bang.examples] `fn fetch![T](...)` is called as `fetch!::[User](key)`, and suspending methods as `parser.load!::[User](text)` and `Store::load!::[User](key)`.
7. r[fn.generic.method-inference] A generic method may still rely entirely on inference by omitting the list.
8. r[fn.generic.dot-member-value] A dot member with type arguments and no call, as in `parser.parse::[User]`, is not a function value; the reference form is `parser::parse::[User]`.

## Methods And Receivers

Functions declared in `impl` blocks are methods when their first parameter
is `self` or `mut self`:

```text
impl User:
    fn domain(self) -> string:
        self.email.slice(1, self.email.len())

    fn tagged[T](self, value: T) -> T:
        value

label := user.tagged::[string]("admin")
```

1. r[fn.method.form] Functions declared in `impl` blocks are methods when their first parameter is `self` or `mut self`.
2. r[fn.method.self] `self` is readonly access to the receiver, so mutating the receiver through it is an error. Error: `readonly-root`.
3. r[fn.method.mut-self] `mut self` is shorthand for `self: mut Self`.
4. r[fn.method.no-sigil] There is no reference sigil or ownership-taking receiver form.
5. r[fn.method.associated] Receiverless members are associated functions and are called with qualified `Type::function(...)` or `Trait::function(...)` syntax.
6. r[fn.method.eval-order] Method-call syntax evaluates the receiver first and then ordinary arguments.
7. r[fn.method.equivalence] It is semantically equivalent to selecting the resolved method and supplying the receiver as its first argument.
8. r[fn.method.value-deferred] The treatment of bare `receiver.method` as a function value is deferred.

```text
data Counter:
    value: i32

impl Counter:
    fn reset(self) -> void:
        self.value = 0  # error: readonly-root
```

See also: [Member Resolution](03-names-and-scopes.md#member-resolution), which
defines member lookup, including promotion;
[Traits](09-traits.md), which defines dynamic dispatch;
[Member Access](05-expressions.md#member-access).

## Recursion

This section defines recursive functions and closures.

1. r[fn.recursion.named-cycle] Named functions may call themselves or other visible named functions recursively, provided at least one function in each recursive cycle has a declared result type. The others may omit theirs.
2. r[fn.recursion.closure.no-self-name] Closures do not acquire an implicit self-name.
3. r[fn.recursion.closure.binding] A closure directly initialized by a statement-form local `:=` or `let` binding may refer to that binding's name inside its body.
4. r[fn.recursion.closure.result] Its result type after `->` is mandatory.
5. r[fn.recursion.closure.parameters] Parameter types may be supplied by an expected function type or written on the closure.
6. r[fn.recursion.closure.check] The compiler checks recursive calls against that established function type, not against a result inferred from the recursive body.
7. r[fn.recursion.closure.outer] Ordinary references in the initializer outside the closure body still resolve in the outer scope.
8. r[fn.recursion.closure.ordinary-binding] The self-reference uses the ordinary binding.
9. r[fn.recursion.closure.let] With `let`, reassignment changes which function a later recursive call invokes.
10. r[fn.recursion.closure.initialized] A closure body cannot execute until its binding's initializer completes.
11. r[fn.recursion.closure.no-forward] This exception does not enable general forward references or mutual recursion between local closures.

```text
fn sum_to(limit: i32) -> i32:
    sum := fn(n: i32):  # error: recursive-closure-needs-result-type
        if n == 0: 0
        else: n + sum(n - 1)
    sum(limit)
```

See also: [Declarations](#declarations).

## Program Entry Functions

This section defines program entry functions.

1. r[fn.entry.main] `pub fn main() -> void` or `pub fn main() -> Result[void, E]` is the default non-suspending executable entry point.
2. r[fn.entry.no-parameters] It has no source-level parameters.
3. r[fn.entry.suspending] A suspending entry point is named `main!`.
4. r[fn.entry.requirements] Entry points may declare host requirements with the ordinary `$` clause.
5. r[fn.entry.pub] `pub` controls module visibility and does not itself create a Wasm host export.

## Unsupported Function Extensions

1. r[fn.unsupported.features] hd-lang has no general recursive local binding facility, shorthand-argument closures, or non-local returns from closures.

### Method Values

Method values are written as `::` [method references](#method-references).
The rules below still hold:

1. r[fn.unsupported.qualified-call] A qualified call such as `User::guest()`, `Display::to_string(value)`, or `Add::[Money]::add(left, right)` remains an ordinary call.
2. r[fn.unsupported.closure-adapter] An explicit closure, such as `fn(user: User) -> string: user.domain()`, adapts a method where a function value is needed.
3. r[fn.unsupported.variant-value] A variant constructor with exactly one payload field, written `Enum.Variant` with a `.` and no argument clause, is already a function value.
4. r[fn.unsupported.variant-multi] A constructor with two or more payload fields stays an error. Error: `unsaturated-enum-constructor`.

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations).
