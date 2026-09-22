# Functions

Status: core specification draft.

Functions are hd-lang's primary unit of behavior. Methods, entry points, tests,
tools, workflows, and generated adapters are ordinary functions with additional
library metadata or calling conventions.

## Declarations

A named function declares typed parameters and an explicit result type:

```text
fn add(a: i32, b: i32) -> i32:
    a + b
```

Named functions must declare every parameter type and their result type. The
body's normal final value must be assignable to the declared result. Explicit
`return` may complete the function earlier.

Every reachable control path must either return a value assignable to the
declared result, fall through with such a final value, or complete abruptly by
propagation or panic. A non-`void` function with a reachable value-less
fallthrough is rejected. A `void` function likewise rejects a non-`void` final
expression; use `return`, `pass`, or another `void` expression when a preceding
value is intentionally ignored.

Function names are unique in a module. hd-lang has no function or method
overloading; one name resolves to one declaration in its scope.

A same-line body is permitted when it contains one simple statement:

```text
fn test() -> void: println("hi")
```

## Parameters

Parameters are evaluated and initialized from left to right after argument
mapping. Parameters are immutable bindings: their names cannot be reassigned.
For composite parameters, `T` permits const access and `mut T` requires mutable
access:

```text
fn inspect(user: User) -> string: user.name

fn rename(user: mut User, name: string) -> void:
    user.name = name
```

Primitive parameters pass by value. Composite parameters pass shared reference
access according to their declared type. hd-lang does not perform ownership
transfer at a call.

### Positional And Named Arguments

Calls may mix positional and named arguments. Positional arguments must come
first:

```text
resize(640, height=480)
```

Named argument labels are parameter names and are part of the source-level
calling interface. A positional argument must not follow a named argument. A
parameter must receive one value, either explicitly or from its default.

### Default Values

A parameter may declare a default:

```text
fn connect(host: string, port: i32 = 443, tls: bool = true) -> Connection:
    ...
```

After the first parameter with a default, every following non-vararg parameter
must also have a default. Calls may omit only parameters that have defaults.

Defaults are evaluated for each call, in parameter declaration order, after
all explicit argument expressions have been evaluated. A default may refer to
earlier parameters but not later parameters.

A default expression must be pure: it must not mutate a parameter or capture,
call a mutable function, require injected context, or suspend. It may evaluate
ordinary expressions and call other pure functions. The compiler verifies this
transitively from function signatures and bodies available to it.

Purity permits allocation and mutation of newly created local values when those
values and mutable aliases do not escape the default expression. This is local
construction, not an externally observable write. A separately compiled
package exports a compiler-generated purity summary for every callable; the
importing compiler verifies and consumes that summary as part of the package's
typed interface. A package cannot self-assert an unchecked purity summary.

### Varargs

A final positional parameter may end in `...`:

```text
fn sum(values: i32...) -> i32:
    ...
```

Within the function, `values` is a `list[i32]`. A call may supply zero or more
positional elements or spread one compatible list:

```text
sum()
sum(1, 2, 3)
sum(items...)
```

A vararg must be the last positional parameter. Passing it by name supplies a
list without spread syntax. A vararg has no default expression. At a call site,
one list spread may supply the remaining vararg elements and must be the final
positional argument; it does not fill fixed parameters. Core varargs are
homogeneous; heterogeneous packs belong to provisional variadic generics.

## Function Types And Values

Functions are values. A plain function type lists parameter types and a result:

```text
fn(string) -> string
```

Parameter names and default values are not part of a function value type.
Calling through a function value therefore uses positional arguments only and
does not inherit declaration defaults. Vararg calling convention, function
mutability, and provisional requirement rows are part of the type; a vararg
function type writes an ellipsis after its final element type, such as
`fn(string, i32...) -> i32`.

A named function value may be passed anywhere its function type is expected.
Function types are invariant in parameter and result types in v1. Parameter and
result types must therefore match after transparent alias expansion; ordinary
numeric or reference-view coercions do not create a different function value
type.

Generic functions are not first-class polymorphic values. Referring to one as
a value must instantiate every generic parameter, either from an expected
monomorphic function type or with the complete explicit type-argument list.
The resulting value has an ordinary monomorphic function type. A reified
instantiation captures the required runtime type descriptors in that value.

## Closures

A closure uses `fn` without a name:

```text
slugify := fn(text: string) -> string:
    text.trim().lower().replace(" ", "-")
```

There is no separate arrow or shorthand-argument closure syntax in v1.
A one-line closure uses the same `:` suite syntax:

```text
inc := fn(x: i32) -> i32: x + 1
```

When an expected function type is available, an inline closure may omit
parameter and result annotations:

```text
lower := names.map(fn(name): name.lower())
```

Without a sufficient expected type, the closure must state enough annotations
to determine all parameter and result types.

### Captures

A closure captures local bindings that it references from enclosing lexical
scopes. A plain `fn(...) -> T` closure may read captures but must not mutate
them.

A closure that mutates captured state has type `mut fn(...) -> T` and uses the
same marker in its literal:

```text
let count: i32 = 0

let next: mut fn() -> i32 = mut fn() -> i32:
    count = count + 1
    count
```

Calling a mutable closure requires a value with mutable function access.
Captured `let` storage is shared with its defining scope and other closures that
capture the same binding. If a closure outlives the original stack activation,
the runtime preserves its captured storage through garbage collection.

This section defines ordinary in-process closure behavior only. Serializable
closure capture, code identity, and restoration semantics are deferred to the
runtime design.

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

Newlines do not replace commas in argument lists. Parentheses make each
multiline closure's boundary explicit.

## Trailing Callback Blocks

When the final parameter has a zero-argument function type, a call may supply it
as an indented trailing block:

```text
result := when(a, b):
    compute_result()

transaction:
    save_user()
```

Ordinary arguments remain in parentheses. If there are no ordinary arguments,
empty `()` is omitted. The trailing block is equivalent to a zero-argument
closure whose result and behavior are contextually inferred from the final
parameter.

Only one trailing block is permitted, and only for a zero-argument final
parameter. Parameterized callbacks use explicit closure syntax. `return` inside
the block returns from the generated callback, not from the enclosing function.

## Generic Functions

Generic parameters follow the function name:

```text
fn first[T](items: list[T]) -> T?:
    ...
```

Trait bounds use `+` composition:

```text
fn audit[T: Display + Named](value: T) -> string:
    ...
```

Callers may rely on inference or provide the complete type argument list:

```text
first(names)
first[string](names)
```

Explicit type arguments apply to named module functions in v1. Generic method
calls rely on inference.

Generic parameters are erased by default. `reified T` requests runtime type
metadata, as defined in [Type System](04-type-system.md).

## Methods And Receivers

Functions declared in core `impl` blocks are methods. Their first parameter
must be `self` or `mut self`:

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]
```

`self` is const access to the receiver. `mut self` is shorthand for
`self: mut Self`. There is no reference sigil or ownership-taking receiver form
in v1. Receiverless associated functions in traits or implementation blocks are
not part of the stable core; use a module-level function. A provisional feature
may add a narrowly scoped associated-function form with its own call rules.

Method-call syntax evaluates the receiver first and then ordinary arguments.
It is semantically equivalent to selecting the resolved method and supplying
the receiver as its first argument; promotion and dynamic dispatch are defined
in [Traits](09-traits.md). Core v1 does not create a bound function value from
bare `receiver.method`; use an explicit closure when a first-class adapter is
needed.

## Recursion

Named functions may call themselves or other module functions recursively.
Closures do not acquire an implicit self-name. A recursively used closure must
be expressed through a separately designed recursive binding facility; direct
self-reference in its own initializer is invalid under ordinary binding rules.

## Program Entry Functions

`pub fn main() -> void` or `pub fn main() -> Result[void, E]` is the default
non-suspending executable entry point. It has no source-level parameters.
Suspending entry points and host requirements are provisional extensions.

`pub` controls module visibility and does not itself create a Wasm host export.

## Unsupported Function Extensions

v1 has no recursive local binding facility, partial generic argument lists,
placeholder generic arguments, shorthand-argument closures, or non-local
returns from closures. The binary encoding of exported purity summaries is a
compiler ABI detail, but their checked semantics are defined above.
