# Control Flow

Status: language specification draft.

hd-lang control-flow constructs use indentation-delimited suites and may
produce values. Conditions are always `bool`; there is no truthiness conversion
from numbers, strings, collections, or optional values.

## Blocks And Completion

A suite evaluates statements in source order. If control reaches the final
expression statement normally, that expression is the suite's value. A suite
whose final statement is not a value expression has type `void`.

`return`, `break`, `continue`, optional or result propagation with `?`, and a
runtime panic complete the current control path abruptly rather than producing
the suite's ordinary final value.

An indented suite must contain at least one statement. `pass` supplies an
explicit no-op expression when a body is intentionally empty.

## Conditional Expressions

`if`, `else if`, and `else` select exactly one suite:

```text
label := if score >= 90:
    "excellent"
else if score >= 70:
    "passing"
else:
    "needs work"
```

Conditions are evaluated from top to bottom until one is `true`. Later
conditions and unselected suites are not evaluated.

When an `if` is used where a value is required, it must have an `else`, every
reachable branch must produce a value, and those values must have one
compatible result type. In statement position, `else` may be omitted and the
conditional then has type `void`.

`else if` is one conditional-chain form. It is not parsed as an `else` suite
containing an unrelated nested `if`.

## For Loops

A `for` loop obtains an iterator from its iterable expression and repeatedly
advances that iterator:

```text
for value in values:
    println(value)
```

The iterable expression is evaluated exactly once. A new iterator is obtained
for each execution of the loop. The binding pattern receives each yielded
value before the body executes.

The prelude protocols distinguish `Iterable[T]` from `Iterator[T]`:

```text
trait Iterable[T]:
    fn iter(self) -> mut Iterator[T]

trait Iterator[T]:
    fn next(mut self) -> T?
```

An ordinary iterable creates an independent mutable iterator on every `iter()`
call. The iterator stores traversal progress and `next` returns `nil` after
exhaustion.

Every mutable iterator has a compiler-provided `Iterable[T]` conformance whose
`iter` returns that same mutable cursor without cloning or resetting it. The
prelude provides the generic implementation below:

```text
impl[T, I: Iterator[T]] Iterable[T] for mut I:
    fn iter(self) -> mut Iterator[T]: self
```

A const iterator view cannot advance and therefore does not gain this adapter.
`for` accepts an ordinary `Iterable[T]` or a mutable iterator through the
adapter and repeatedly calls `next` on the resulting cursor.

The built-in read-only `list[T]` iterable yields the const viewpoint of each
stored element. The built-in `map[K, V]` iterable yields `(K, V)` tuples in an
unspecified order, with the ordinary const viewpoint applied to `V`. Iteration
does not grant mutable element access merely because the original expression
has a mutable root; libraries may provide explicit mutable-iteration APIs with
separate aliasing rules.

Built-in list and map iterators capture a structural-version counter. Inserting,
removing, clearing, or otherwise changing collection shape invalidates existing
iterators; their next `next` call causes a checked runtime panic. Replacing an
existing list element or map value without changing collection shape does not
invalidate the iterator, and later visits observe the replacement. User-defined
iterables must document equivalent mutation behavior in their own contract.

## While Loops

A `while` loop evaluates its `bool` condition before every iteration:

```text
while index < names.len():
    index = index + 1
```

If the condition is initially `false`, the body does not execute.

## Break, Continue, And Loop Else

`continue` skips the remainder of the current loop body and begins the next
iteration. `break` exits the nearest enclosing loop. Neither operation targets
a loop across a function or closure boundary.

`break` and `continue` outside a loop are compile-time errors, including at
module top level and directly in a test block.

A loop without `else` has type `void`. It permits plain `break` but not
`break value`.

A `for` or `while` loop with `else` is value-producing:

```text
found := for value in values:
    if value > 100:
        break value
else:
    nil
```

- `break value` terminates the loop and supplies its value.
- Normal exhaustion of a `for` loop, or a `false` `while` condition, evaluates
  the `else` suite and uses that suite's value.
- Plain `break` is invalid in a value-producing loop.

Every reachable `break value` and the `else` suite must produce one compatible
loop result type. `continue` does not produce a loop value.

The `else` suite is not executed after any `break`, abrupt function return,
propagation with `?`, cancellation, or runtime panic.

## Match Expressions

`match` evaluates its subject once and compares arms in source order:

```text
message := match status:
    JobStatus.Queued => "waiting"
    JobStatus.Running => "working"
    _ => "unknown"
```

The first matching arm is selected, and only its body is evaluated. All arm
results must have one compatible type when the match value is used.

A match must be exhaustive. Coverage is checked as follows:

- a nominal enum requires every inhabitable variant, accounting for payload
  constraints, or a catch-all pattern;
- `bool` is covered by both `true` and `false`, or by a catch-all pattern;
- a tuple pattern covers the tuple values covered recursively by its element
  patterns; and
- integer, floating-point, `char`, `string`, optional, and other value spaces
  require an irrefutable catch-all after any literal or `nil` cases.

`_` and a bare binding identifier are catch-all patterns for the subject type.
An irrefutable catch-all must be the final arm because every later arm would be
unreachable. Duplicate literals, duplicate fully covered variants, arms after a
catch-all, and other statically provable unreachable arms are compile-time
errors.

Enum payload patterns use call-style parentheses. Positional patterns come
first, and `field=pattern` names a payload field:

```text
match expr:
    Expr.Add(l, r) => l + r
    Expr.Sub(left=l, right=r) => l - r
    Expr.Scale(left, factor=2) => left * 2
```

A bare identifier in payload position binds a new arm-local name; it does not
need to match the payload field's declaration name. A literal pattern requires
an equal value. An enum variant pattern may use `.Variant` when its subject or
enclosing payload position fixes one enum type; otherwise it uses the qualified
`Enum.Variant` form. The shorthand has the same exhaustiveness and GADT
refinement rules as the qualified form.

Matching does not transfer ownership. Primitive payloads bind by value;
composite payloads bind reference access after applying the matched subject's
viewpoint permission. A literal-constrained payload pattern such as
`Expr.Scale(value, factor=2)` covers only that subset of the variant, so another
arm must cover the remaining `Scale` values unless a later catch-all does.

Nested variant and tuple patterns are permitted by the grammar. Struct
destructuring patterns are not supported.

GADT pattern refinement is defined in [Generalized Algebraic Data
Types](13-gadts.md).

## Return

`return expression` immediately completes the nearest enclosing named function
or closure with that value. The value must be assignable to the declared or
inferred return type. Bare `return` is valid only for a `void`-returning
function or closure.

`return` outside a named function or closure is a compile-time error. A module
script and a `test` block are not implicit return targets.

Falling through a function body evaluates its final expression as the return
value. A function declared `-> void` may fall through after a statement whose
result is `void`.

Inside a trailing callback block, `return` completes the generated callback,
not the function containing the call. hd-lang has no non-local return from a
closure.

## Unreachable Code

Statements following an unconditional `return`, `break`, or `continue` in the
same suite are unreachable. The compiler must emit an `unreachable-code`
warning, but the warning does not reject the program by default. Unreachable
code is still parsed and type-checked. Tooling may provide a package policy that
promotes this warning to an error without changing language semantics.

## Runtime Panics

A runtime panic is an abrupt, unrecoverable failure of the current program
instance. It is distinct from a recoverable `Result` error and is not catchable
by core hd-lang source code. Integer overflow, division errors, invalid shifts,
failed checked casts, out-of-bounds indexing, and invalidated built-in
iterators panic when their owning chapters require a checked runtime failure.

When a panic occurs, ordinary evaluation stops immediately. No enclosing
`else` suite, remaining expression, or caller statement executes. Runtime and
library mechanisms may observe the complete exit and perform mandatory runtime
cleanup, but core source has no panic handler or unwinding construct. The
runtime must report at least a stable failure category and source location when
one is available. Its Wasm trap, host error, and diagnostic encoding are ABI
details.

## Unsupported Control-Flow Extensions

Match guards are not part of the language. Filtering that depends on additional
conditions belongs in the selected arm body or in an enclosing conditional.
