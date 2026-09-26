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

An expression whose value is discarded and whose type is `Result[T, E]`, `T?`,
or `mut Suspend[T]` is a compile-time `discarded-must-use-value` error. This
includes a non-final expression statement and the final expression of any suite
whose value is discarded: a loop body, an `if` without `else`, a
statement-position `match` arm, a `defer` suite, a test body, or a module's
top-level script. Such a value must be propagated with `?`, inspected by
`match`, returned, stored for later use, or explicitly discarded with
`_ := expression`. The `_` spelling does not bind a local name and makes the
discard visible in review.

The compiler emits an `unused-local-binding` warning when an ordinary local
binding is never read. Names beginning with `_` suppress that warning, except
that an unread binding of a must-use value is a
`discarded-must-use-value` error. Only the exact `_ := expression` discard
form explicitly discards a must-use value without binding it.

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
compatible result type. Without an expected type, that type is the
[least common type](04-type-system.md#least-common-type) of the branch values.
In statement position, `else` may be omitted and the
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
value before the body executes. A binding list of several names, such as
`for key, value in entries`, destructures each yielded value as a tuple; when
the yielded type is not a tuple of that arity, the loop is a `type-mismatch`
error.

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
impl[T, I < mut Iterator[T]] Iterable[T] for I:
    fn iter(self) -> mut Iterator[T]: self
```

A readonly iterator view cannot advance and therefore does not gain this adapter.
`for` accepts an ordinary `Iterable[T]` or a mutable iterator through the
adapter and repeatedly calls `next` on the resulting cursor. An iterable
expression whose type implements neither `Iterable[T]` nor `Iterator[T]` is an
`unsatisfied-trait-bound` error.

The built-in `list[T]` iterable yields each element as `T`, including `mut U`
when `T = mut U`, even through a readonly list. The built-in `map[K, V]`
iterable yields `(K, V)` tuples in insertion order. Destructuring each
entry preserves `V`, including `mut U` when `V = mut U`, even through a
readonly map. Iteration
does not grant mutable element access merely because the list root is mutable;
the declared list or map value type determines that permission. Libraries may provide
additional mutable-iteration APIs with separate aliasing rules.

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

An arm may add `if` followed by a `bool` guard. After its pattern matches,
the guard is evaluated using that arm's pattern bindings. If the guard is
false, matching continues with the next arm; if it is true, that arm is
selected. A guard is not evaluated when its pattern fails, and only the
selected arm's body is evaluated. All arm results must have one compatible
type when the match value is used. Without an expected type, that type is the
[least common type](04-type-system.md#least-common-type) of the arm results.

A match must be exhaustive. Coverage is checked as follows:

- a nominal enum requires every inhabitable variant, accounting for payload
  constraints, or a catch-all pattern;
- `bool` is covered by both `true` and `false`, or by a catch-all pattern;
- a tuple pattern covers the tuple values covered recursively by its element
  patterns;
- a data pattern covers its nominal data type when every listed field pattern
  is irrefutable, while unlisted fields are unconstrained; and
- an optional is covered by `nil` plus an unguarded present-value pattern,
  or by a catch-all; integer, floating-point, `char`, `string`, and other value
  spaces require an irrefutable catch-all after any literal cases.

A guarded arm contributes no coverage to exhaustiveness, even when its pattern
would be irrefutable without the guard. A later unguarded arm must cover its
values. An unguarded irrefutable catch-all must be the final arm; a guarded
catch-all may be followed by other arms. Duplicate patterns are permitted when
earlier occurrences are guarded, but an arm already covered by an earlier
unguarded arm is unreachable.

`_` and a bare binding identifier are catch-all patterns for the subject type.
If a bare identifier resolves to a variant of the subject enum, it is a
`bare-variant-pattern` error rather than a new catch-all binding; write
`.Variant` or a qualified variant name.
For a subject of type `T?`, `value?` matches only the present case and binds
`value` as `T`; `nil` matches only absence. The suffix `?` in a pattern does
not propagate or unwrap an expression. It may also appear in a nested pattern
whose expected type is optional. A bare `value` still binds the entire `T?`,
including `nil`. Matching `T??` with `value?` removes only the outer optional
layer, so `value` has type `T?`.
Using a present-value pattern where the expected subject or nested payload type
is not optional is an `optional-pattern-requires-optional` error.
An unguarded irrefutable catch-all must be the final arm because every later
arm would be unreachable. Duplicate unguarded literals, duplicate fully
covered variants, arms after an unguarded catch-all, and other statically
provable unreachable arms are `unreachable-match-arm` errors, reported on the
unreachable arm.

Enum payload patterns use call-style parentheses. Positional patterns come
first, and `field=pattern` names a payload field:

```text
match expr:
    Expr.Add(l, r) => l + r
    Expr.Sub(left=l, right=r) => l - r
    Expr.Scale(left, factor=2) => left * 2
```

A pattern for a variant that carries a payload must list that payload, with
one pattern per payload field. A bare variant pattern for such a variant, or a
payload list of a different length, is a `pattern-arity` error. A named
pattern that names no payload field of the variant is an `unknown-data-field`
error.

A bare identifier in payload position binds a new arm-local name; it does not
need to match the payload field's declaration name. The compiler emits
`variant-binding-name-mismatch` when a positional bare binding equals a
different payload field's name in that same variant—for example, binding
`else_value` in the `then_value` position. Use named patterns to make an
intentional reorder explicit. A literal pattern requires
an equal value. An enum variant pattern may use `.Variant` when its subject or
enclosing payload position fixes one enum type; otherwise it uses the qualified
`Enum.Variant` form. The shorthand has the same exhaustiveness and GADT
refinement rules as the qualified form.

Matching does not transfer ownership. Primitive payloads bind by value;
composite payloads bind reference access after applying the matched subject's
viewpoint permission when the payload directly declares a non-generic mutable
type. A payload declared with a generic parameter instead binds its
substituted type, including `mut U`. Tuple pattern elements retain their
declared types. A literal-constrained payload pattern such as
`Expr.Scale(value, factor=2)` covers only that subset of the variant, so another
arm must cover the remaining `Scale` values unless a later catch-all does.

Nested variant and tuple patterns are permitted by the grammar. Data
destructuring patterns are also permitted. `User { name }` binds the `name`
field; `User { name=alias }` binds it as `alias`; `User { age=18 }` matches
only values with that field value. Unlisted fields are ignored. Listed fields
must be distinct and exist on the named data type. A cross-module pattern may
name only public fields. An empty data pattern is irrefutable for that
data type. A named data pattern must match the subject's nominal type;
it does not structurally match a different data type with the same fields.
Primitive fields bind by value; composite fields bind reference access under
the data subject's effective field types. A direct `mut U` field in a readonly
subject binds as `U`; a generic field declared `field: P` binds as substituted
`P`, including `mut U` when `P = mut U`.

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

## Deferred Cleanup

`defer:` registers a synchronous cleanup suite on the innermost executing
lexical block. Reaching the statement does not run its suite. Registered suites
run once in last-in, first-out order when that block exits normally, through
`return`, `break`, or `continue`, or because postfix `?` propagates. Each loop
iteration has its own body block, so its registered suites run before the next
iteration begins.

Cleanup scopes are function and closure bodies, loop bodies, each selected
`if` or `else` suite, match arms, provider scopes, trailing callback blocks,
and test bodies. Module top level and declaration bodies that do not execute
are not cleanup scopes; a `defer` there is a
`defer-outside-cleanup-scope` error.

```text
fn read_first!(path: string) -> Result[string, ResourceError[FileError]] $ Files:
    let handle: mut FileHandle = $.use(Files).open!(path)?
    defer:
        _ := handle.close()
    handle.read!()
```

A value-producing block evaluates and saves its result before running its
registered suites. Likewise, a `return` or `break` operand and a value being
propagated by `?` are evaluated before cleanup begins. A cleanup suite observes
captured lexical storage at cleanup time.

A `defer` suite must produce `void`. It cannot bang-call, otherwise suspend,
propagate with `?`, or transfer control with `return`, `break`, or `continue`.
A bang call or other suspending operation in the suite is a
`suspending-defer` error. Propagation with `?`, or a `return`, `break`, or
`continue` that would leave the suite, is a `defer-control-flow` error.
Direct or transitive use of `std.task.block_on` is a
`suspension-forbidden-context` error, as specified in
[Requirements and Suspension](11-requirements-and-suspension.md#suspending-functions).

A runtime panic does not run pending `defer` suites because core hd-lang has no
panic unwinding. If a cleanup suite itself panics, the instance is poisoned and
no remaining cleanup suite is guaranteed to run. `defer` does not stop an alias
to a closed handle from escaping; ownership and alias-escape prevention remain
[open design work](../future-work/OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy).

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
out-of-bounds indexing, and invalidated built-in iterators panic when their owning chapters require a checked runtime failure.

When a panic occurs, ordinary evaluation stops immediately. No enclosing
`else` suite, remaining expression, or caller statement executes. Runtime and
library mechanisms may observe the complete exit and perform mandatory runtime
cleanup, but core source has no panic handler or unwinding construct. The
runtime must report at least a stable failure category and source location when
one is available. Its Wasm trap, host error, and diagnostic encoding are ABI
details.

One program instance is the instantiated module graph and execution state
defined in [Modules and Packages](10-modules.md#module-initialization). A panic
poisons that instance: the host must not invoke it again and must discard it
after reporting the failure.
Hosts that require invocation isolation create a separate instance per
invocation. Stable panic categories are exactly `annotation-reference-unresolved`,
`annotation-resolution-reentry`, `assertion-failed`, `explicit-panic`,
`integer-overflow`,
`integer-division-by-zero`, `invalid-shift`,
`index-out-of-bounds`, `iterator-invalidated`, `suspension-competing-driver`,
`suspension-nested-driver`, `suspension-reentrant-poll`,
`suspension-invalid-state`, and `stack-exhausted`.

The prelude function `panic(message: string) -> never` explicitly causes an
`explicit-panic` failure. Because `never` is assignable to every type, a panic or
other abrupt expression is valid in any value-producing arm without affecting
the compatible result type of reachable normal arms.
