# Functions

Status: language specification draft.

Functions are hd-lang's primary unit of behavior. Methods, entry points, tests,
tools, workflows, and generated adapters are ordinary functions with additional
library metadata or calling conventions.

## Declarations

A named function declares typed parameters and an explicit result type. A name
ending in `!` declares a suspending function, and a trailing `$` clause declares
requirements:

```text
fn add(a: i32, b: i32) -> i32:
    a + b

fn load_user!(id: UserId) -> Result[User, DbError] $ Database:
    ...
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

Function names are unique in their scope. hd-lang has no function or method
overloading; one name resolves to one declaration in its scope.

A same-line body is permitted when it contains one simple statement:

```text
fn test() -> void $ Console: println("hi")
```

A named function may also be declared inside an executable block suite. Its
name is visible from that declaration onward and within its own body; it can
capture enclosing local values under the same readonly capture rules as a
plain closure. It cannot be marked `pub` or used from another module:

```text
fn total_with_bonus(values: list[i32], bonus: i32) -> i32:
    fn add_bonus(value: i32) -> i32:
        value + bonus

    let total = 0
    for value in values:
        total = total + add_bonus(value)
    total
```

## Parameters

Parameters are evaluated and initialized from left to right after argument
mapping. Parameters are non-reassignable bindings: their names cannot be reassigned.
For composite parameters, `T` permits readonly access and `mut T` requires mutable
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
reassign a top-level `let`, call a `mut fn` value, pass non-fresh mutable access
to any call, require an injected provider, call `std.task.block_on`, or suspend. It may evaluate ordinary
expressions and call other pure functions. The compiler verifies this
transitively from available function bodies and exported purity summaries. A
call through a function value or dynamic trait method is rejected in a
purity-checked context because function types do not carry purity; named
callables with verified summaries remain usable. Purity here restricts
observable writes, provider access, and suspension; it does not claim
referential transparency. The containing default reports
its ordinary context-specific impurity diagnostic.

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
positional argument; it does not fill fixed parameters. Homogeneous varargs and
heterogeneous type-pack expansion share the ellipsis token; name resolution
distinguishes them as specified in [Variadic Generics](12-variadic-generics.md).

## Function Types And Values

Functions are values. A plain function type lists parameter types and a result:

```text
fn(string) -> string
fn!(UserId) -> Result[User, DbError] $ Database
```

Parameter names and default values are not part of a function value type.
Calling through a function value therefore uses positional arguments only and
does not inherit declaration defaults. Vararg calling convention, function
mutability, suspension, and requirement rows are part of the type; a vararg
function type writes an ellipsis after its final element type, such as
`fn(string, i32...) -> i32`.

A named function value may be passed anywhere its function type is expected.
Function types are invariant in parameter and result types. Parameter and
result types must therefore match after transparent alias expansion; ordinary
numeric or reference-view coercions do not create a different function value
type.

Generic functions are not first-class polymorphic values. Referring to one as
a value must instantiate every generic parameter, either from an expected
monomorphic function type or with a complete explicit type-argument list. A
placeholder in that list may be solved from the expected monomorphic type.
The resulting value has an ordinary monomorphic function type. A reified
instantiation captures the required runtime type descriptors in that value.

A value of type `fn!(A) -> T $ R` constructs `mut Suspend[T]` when called normally
and may be bang-called inside a suspending body. It may be weakened to the
lowered constructor type `fn(A) -> mut Suspend[T] $ R`; the reverse conversion is
not implicit.

## Closures

A closure uses `fn` without a name:

```text
slugify := fn(text: string) -> string:
    text.trim().lower().replace(" ", "-")
```

There is no separate arrow or shorthand-argument closure syntax.
A one-line closure uses the same `:` suite syntax:

```text
inc := fn(x: i32) -> i32: x + 1
```

A suspending closure places `!` after `fn`; a requirement clause follows its
result type:

```text
loader := fn!(id: UserId) -> Result[User, DbError] $ Database:
    db := $.use(Database)
    db.load_user!(id)
```

When an expected function type is available, an inline closure may omit
parameter and result annotations:

```text
lower := names.map(fn(name): name.lower())
```

Without a sufficient expected type, parameters must be annotated. A
nonrecursive closure may infer its result type from its body; an expected
function type may instead supply the result type. A recursive local closure
must always write its result type explicitly, even if an expected function
type could supply it.

### Captures

A closure captures local bindings that it references from enclosing lexical
scopes. It also captures, when created, every lexical provider from an
enclosing `$.with` scope that its body uses. Those provider values remain bound
to the closure after the provider scope ends, exactly as providers captured by
a suspension frame remain bound after construction.

A plain `fn(...) -> T` closure may read captures but must not mutate through
them. Within a plain closure, captured mutable access `mut T` is viewed as
readonly `T`. A closure must be `mut fn` when it assigns captured `let` storage
or obtains mutable access from a capture—for example, by calling a `mut self`
method on a captured list or on a captured mutable child.

Returning mutable access obtained from a capture therefore also requires a
`mut fn` closure. A callable's declared `mut T` result is not itself weakened
when the callable is read through a readonly reference. Calling a `mut fn`
closure still requires mutable access to the closure itself.

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

An explicit type-argument list must supply every generic parameter. Partial
prefix lists are not permitted, even when inference could determine the
remaining arguments.

An explicit list may write `_` in any slot to infer that argument:

```text
fn convert[From, To](value: From) -> To:
    ...

user := convert[_, User](payload)
```

The list still has exactly one slot per generic parameter. A placeholder is
solved from call arguments, the expected result type, and the function's
generic constraints. If those constraints do not determine one type, the call
is rejected as ambiguous. `_` is a call-site inference instruction, not a type,
and cannot appear in an ordinary type argument list such as `list[_]`.

The same explicit-list rules apply to generic methods:

```text
parser.parse[User](text)
parser.convert[_, User](payload)
```

Name resolution distinguishes the brackets from an indexing operation. A
generic method may still rely entirely on inference by omitting the list. Bare
generic bound-method values remain unsupported; the explicitly instantiated
member must be called.

Generic parameters are erased by default. `reified T` requests runtime type
metadata, as defined in [Type System](04-type-system.md).

## Methods And Receivers

Functions declared in `impl` blocks are methods when their first parameter is
`self` or `mut self`:

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]

    fn tagged[T](self, value: T) -> T:
        value

label := user.tagged[string]("admin")
```

`self` is readonly access to the receiver. `mut self` is shorthand for
`self: mut Self`. There is no reference sigil or ownership-taking receiver form.
Receiverless members are associated functions and are called with qualified
`Type::function(...)` or `Trait::function(...)` syntax.

Method-call syntax evaluates the receiver first and then ordinary arguments.
It is semantically equivalent to selecting the resolved method and supplying
the receiver as its first argument; promotion and dynamic dispatch are defined
in [Traits](09-traits.md). The treatment of bare `receiver.method` as a
function value is deferred; see [Member Access](05-expressions.md#member-access).

## Recursion

Named functions may call themselves or other visible named functions recursively.
Closures do not acquire an implicit self-name. A closure directly initialized
by a statement-form local `:=` or `let` binding may refer to that binding's
name inside its body. Its result type after `->` is mandatory. Parameter types
may be supplied by an expected function type or written on the closure. The
compiler checks recursive calls against that established function type, not
against a result inferred from the recursive body. Ordinary references in the
initializer outside the closure body still resolve in the outer scope.

The self-reference uses the ordinary binding. With `let`, reassignment changes
which function a later recursive call invokes. A closure body cannot execute
until its binding's initializer completes; this exception does not enable
general forward references or mutual recursion between local closures.

## Program Entry Functions

`pub fn main() -> void` or `pub fn main() -> Result[void, E]` is the default
non-suspending executable entry point. It has no source-level parameters.
A suspending entry point is named `main!`; entry points may declare host
requirements with the ordinary `$` clause.

`pub` controls module visibility and does not itself create a Wasm host export.

## Unsupported Function Extensions

hd-lang has no general recursive local binding facility, partial generic
argument lists, shorthand-argument closures, or non-local
returns from closures. The binary encoding of exported purity summaries is a
compiler ABI detail, but their checked semantics are defined above.
