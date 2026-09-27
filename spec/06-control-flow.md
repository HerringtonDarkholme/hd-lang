# Control Flow

Status: language specification draft.

This chapter defines suites, conditionals, loops, `match`, `return`, `defer`,
and runtime panics.

1. r[flow.suites] hd-lang control-flow constructs use indentation-delimited suites and may produce values.
2. r[flow.condition.bool] Conditions are always `bool`: there is no truthiness conversion from numbers, strings, collections, or optional values.

## Blocks And Completion

A suite evaluates its statements in order, and its final expression may give
it a value.

### Suite Values

1. r[flow.block.order] A suite evaluates statements in source order.
2. r[flow.block.value] If control reaches the final expression statement normally, that expression is the suite's value.
3. r[flow.block.void] A suite whose final statement is not a value expression has type `void`.
4. r[flow.block.abrupt] `return`, `break`, `continue`, optional or result propagation with `?`, and a runtime panic complete the current control path abruptly rather than producing the suite's ordinary final value.
5. r[flow.block.non-empty] An indented suite must contain at least one statement.
6. r[flow.block.pass] `pass` supplies an explicit no-op expression when a body is intentionally empty.

### Must-Use Values

1. r[flow.must-use.discard] An expression whose value is discarded and whose type is `Result[T, E]`, `T?`, or `mut Suspend[T]` is a compile-time error. Error: `discarded-must-use-value`.
2. r[flow.must-use.statement] This includes a non-final expression statement.
3. r[flow.must-use.suite-final] It also includes the final expression of any suite whose value is discarded:
   - a loop body;
   - an `if` without `else`;
   - a statement-position `match` arm;
   - a `defer` suite;
   - a test body;
   - a module's top-level script.
4. r[flow.must-use.handle] Such a value must be propagated with `?`, inspected by `match`, returned, stored for later use, or explicitly discarded with `_ := expression`.
5. r[flow.must-use.underscore] The `_` spelling does not bind a local name.

```text
fn save() -> Result[i32, SaveError]:
    .Err(SaveError { message: "failed" })

fn save_all(values: List[i32]) -> void:
    for value in values:
        save()  # error: discarded-must-use-value
    _ := save()  # valid: an explicit discard
```

> **Why.** The `_` spelling makes the discard visible in review.

### Unused Bindings

1. r[flow.unused.warning] The compiler emits an `unused-local-binding` warning when an ordinary local binding is never read.
2. r[flow.unused.underscore] Names beginning with `_` suppress that warning.
3. r[flow.unused.must-use] The exception is an unread binding of a must-use value, which is an error. Error: `discarded-must-use-value`.
4. r[flow.unused.discard-form] Only the exact `_ := expression` discard form explicitly discards a must-use value without binding it.

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

1. r[flow.if.select] `if`, `else if`, and `else` select exactly one suite.
2. r[flow.if.order] Conditions are evaluated from top to bottom until one is `true`.
3. r[flow.if.lazy] Later conditions and unselected suites are not evaluated.
4. r[flow.if.else-if] `else if` is one conditional-chain form. It is not parsed as an `else` suite containing an unrelated nested `if`.

### Conditional Values

1. r[flow.if.value.else] When an `if` is used where a value is required, it must have an `else`.
2. r[flow.if.value.branches] Every reachable branch must then produce a value, and those values must have one compatible result type.
3. r[flow.if.value.least-common] Without an expected type, that type is the [least common type](04-type-system.md#least-common-type) of the branch values.
4. r[flow.if.statement] In statement position, `else` may be omitted, and the conditional then has type `void`.

### Same-Line Conditionals

1. r[flow.if.same-line] A same-line `if` cannot be written directly inside another same-line suite.
2. r[flow.if.same-line.error] `if a: if b: 1 else: 2 else: 3` and `fn sign(x: i32) -> i32: if x < 0: -1 else: 1` are errors. Error: `syntax-error`.

```text
fn pick(a: bool, b: bool) -> i32:
    v := if a: if b: 1 else: 2 else: 3  # error: syntax-error
    v

fn sign(x: i32) -> i32: if x < 0: -1 else: 1  # error: syntax-error
```

> **Why.** Each `else` on a line then has one owner.

> **Note.** Nest the inner conditional in parentheses, as in
> `if a: (if b: 1 else: 2) else: 3`, or give the outer suite an indented body.

See also: [Statements](02-grammar.md#statements).

## For Loops

A `for` loop obtains an iterator from its iterable expression and repeatedly
advances that iterator:

```text
for value in values:
    println(value)
```

1. r[flow.for.iterable-once] The iterable expression is evaluated exactly once.
2. r[flow.for.fresh-iterator] A new iterator is obtained for each execution of the loop.
3. r[flow.for.binding] The binding pattern receives each yielded value before the body executes.
4. r[flow.for.tuple-binding] A binding list of several names, such as `for key, value in entries`, destructures each yielded value as a tuple.
5. r[flow.for.tuple-arity] When the yielded type is not a tuple of that arity, the loop is an error. Error: `type-mismatch`.

```text
fn bad(values: List[i32]) -> void:
    for left, right in values:  # error: type-mismatch
        pass
```

### Iteration Protocols

The prelude protocols distinguish `Iterable[T]` from `Iterator[T]`:

```text
trait Iterable[T]:
    fn iter(self) -> mut Iterator[T]

trait Iterator[T]:
    fn next(mut self) -> T?
```

1. r[flow.for.iter-independent] An ordinary iterable creates an independent mutable iterator on every `iter()` call.
2. r[flow.for.iterator-progress] The iterator stores traversal progress, and `next` returns `.None` after exhaustion.
3. r[flow.for.protocols] `for` accepts a value of either protocol.
4. r[flow.for.iterable] When the iterable expression's type implements `Iterable[T]`, the loop calls `iter` once and advances the resulting cursor.
5. r[flow.for.iterator] When it implements `Iterator[T]`, the loop advances that same cursor directly, without cloning or resetting it.
6. r[flow.for.iterator.position] Iteration therefore continues from the cursor's current position and leaves it exhausted.
7. r[flow.for.both] When the type implements both `Iterable[T]` and `Iterator[T]`, the loop uses `Iterable[T]`: it calls `iter` once and never advances the value itself.
8. r[flow.for.iterator-mut] An iterator must be accessed mutably to advance. An expression whose type implements `Iterator[T]` but has only readonly access is an error. Error: `mutable-receiver-required`.
9. r[flow.for.neither] An iterable expression whose type implements neither `Iterable[T]` nor `Iterator[T]` is an error. Error: `unsatisfied-trait-bound`.
10. r[flow.for.comprehension] Comprehension `for` clauses accept the same two protocols by the same rules.
11. r[flow.for.no-blanket] No implementation makes every iterator an `Iterable`.
12. r[flow.for.no-blanket.bound] A generic parameter bounded by `Iterable[T]` therefore does not accept an iterator argument.

```text
data Counter:
    current: i32

impl Iterator[i32] for Counter:
    fn next(mut self) -> i32?: .None

fn main(counter: Counter) -> List[i32]:
    [for value in counter => value]  # error: mutable-receiver-required

fn bad() -> void:
    for value in 1:  # error: unsatisfied-trait-bound
        pass
```

### Built-In Collection Iteration

1. r[flow.for.list] The built-in `List[T]` iterable yields each element as `T`, including `mut U` when `T = mut U`, even through a readonly list.
2. r[flow.for.map] The built-in `Map[K, V]` iterable yields `(K, V)` tuples in insertion order.
3. r[flow.for.map.value] Destructuring each entry preserves `V`, including `mut U` when `V = mut U`, even through a readonly map.
4. r[flow.for.no-root-grant] Iteration does not grant mutable element access merely because the list root is mutable. The declared list or map value type determines that permission.
5. r[flow.for.library] Libraries may provide additional mutable-iteration APIs with separate aliasing rules.

### Iterator Invalidation

1. r[flow.for.version] Built-in list and map iterators capture a structural-version counter.
2. r[flow.for.invalidate] Inserting, removing, clearing, or otherwise changing collection shape invalidates existing iterators.
3. r[flow.for.invalidate.panic] An invalidated iterator's next `next` call causes a checked runtime panic, even when the iterator was already exhausted.
4. r[flow.for.replace] Replacing an existing list element or map value without changing collection shape does not invalidate the iterator.
5. r[flow.for.replace.observed] Later visits observe the replacement.
6. r[flow.for.user-defined] User-defined iterables must document equivalent mutation behavior in their own contract.

## While Loops

A `while` loop repeats its body under a `bool` condition:

```text
while index < names.len():
    index = index + 1
```

1. r[flow.while.condition] A `while` loop evaluates its `bool` condition before every iteration.
2. r[flow.while.initially-false] If the condition is initially `false`, the body does not execute.

## Break, Continue, And Loop Else

`break` and `continue` leave the current loop body, and a loop with `else`
produces a value.

### Break And Continue

1. r[flow.continue] `continue` skips the remainder of the current loop body and begins the next iteration.
2. r[flow.break] `break` exits the nearest enclosing loop.
3. r[flow.break.boundary] Neither operation targets a loop across a function or closure boundary.
4. r[flow.break.outside-loop] `break` and `continue` outside a loop are compile-time errors, including at module top level and directly in a test block.

```text
fn main() -> void: break  # error
```

### Loop Values

A `for` or `while` loop with `else` is value-producing:

```text
found := for value in values:
    if value > 100:
        break value
else:
    .None
```

1. r[flow.loop.void] A loop without `else` has type `void`.
2. r[flow.loop.void.break] A loop without `else` permits plain `break` but not `break value`.
3. r[flow.loop.else.value] A `for` or `while` loop with `else` is value-producing.
4. r[flow.loop.else.break-value] `break value` terminates the loop and supplies its value.
5. r[flow.loop.else.exhaustion] Normal exhaustion of a `for` loop, or a `false` `while` condition, evaluates the `else` suite and uses that suite's value.
6. r[flow.loop.else.plain-break] Plain `break` is invalid in a value-producing loop.
7. r[flow.loop.else.type] Every reachable `break value` and the `else` suite must produce one compatible loop result type.
8. r[flow.loop.else.continue] `continue` does not produce a loop value.
9. r[flow.loop.else.skipped] The `else` suite is not executed after any `break`, abrupt function return, propagation with `?`, cancellation, or runtime panic.

```text
fn invalid(values: List[i32]) -> void:
    for value in values:
        break value  # error

fn main() -> i32:
    while true:
        break  # error
    else:
        1
```

## Match Expressions

`match` evaluates its subject once and compares arms in source order:

```text
message := match status:
    JobStatus.Queued => "waiting"
    JobStatus.Running => "working"
    _ => "unknown"
```

### Arms And Guards

1. r[flow.match.subject] `match` evaluates its subject once and compares arms in source order.
2. r[flow.match.guard] An arm may add `if` followed by a `bool` guard.
3. r[flow.match.guard.eval] After its pattern matches, the guard is evaluated using that arm's pattern bindings.
4. r[flow.match.guard.result] If the guard is false, matching continues with the next arm; if it is true, that arm is selected.
5. r[flow.match.guard.skipped] A guard is not evaluated when its pattern fails.
6. r[flow.match.body] Only the selected arm's body is evaluated.
7. r[flow.match.result] All arm results must have one compatible type when the match value is used.
8. r[flow.match.result.least-common] Without an expected type, that type is the [least common type](04-type-system.md#least-common-type) of the arm results.

### Exhaustiveness

1. r[flow.match.exhaustive] A match must be exhaustive. Coverage is checked as the table below states.

| Rule | Coverage |
| --- | --- |
| r[flow.match.cover.enum] Enum | A nominal enum, including `Option[T]` (written `T?`), requires every inhabitable variant, accounting for payload constraints, or a catch-all pattern. |
| r[flow.match.cover.bool] Bool | `bool` is covered by both `true` and `false`, or by a catch-all pattern. |
| r[flow.match.cover.tuple] Tuple | A tuple pattern covers the tuple values covered recursively by its element patterns. |
| r[flow.match.cover.data] Data | A data pattern covers its nominal data type when every listed field pattern is irrefutable, while unlisted fields are unconstrained. |
| r[flow.match.cover.values] Other values | Integer, floating-point, `char`, `string`, and other value spaces require an irrefutable catch-all after any literal cases. |

1. r[flow.match.guard.coverage] A guarded arm contributes no coverage to exhaustiveness, even when its pattern would be irrefutable without the guard.
2. r[flow.match.guard.later-arm] A later unguarded arm must cover its values.
3. r[flow.match.catch-all.final] An unguarded irrefutable catch-all must be the final arm.
4. r[flow.match.catch-all.guarded] A guarded catch-all may be followed by other arms.
5. r[flow.match.duplicate.guarded] Duplicate patterns are permitted when earlier occurrences are guarded.
6. r[flow.match.duplicate.covered] An arm already covered by an earlier unguarded arm is unreachable.
7. r[flow.match.unreachable] Duplicate unguarded literals, duplicate fully covered variants, arms after an unguarded catch-all, and other statically provable unreachable arms are errors. Error: `unreachable-match-arm`.
8. r[flow.match.unreachable.arm] The `unreachable-match-arm` error is reported on the unreachable arm.

```text
enum Status:
    Ready
    Waiting

enum Flag:
    On(value: i32)
    Off

fn invalid(status: Status) -> string:
    match status:  # error
        Status.Ready => "ready"

fn choose(flag: Flag) -> i32:
    match flag:
        _ => 1
        Flag.Off => 0  # error: unreachable-match-arm
```

> **Why.** An unguarded irrefutable catch-all must be the final arm because
> every later arm would be unreachable.

### Catch-All And Variant Patterns

1. r[flow.match.catch-all] `_` and a bare binding identifier are catch-all patterns for the subject type.
2. r[flow.match.bare-variant] If a bare identifier resolves to a variant of the subject enum, it is an error rather than a new catch-all binding: write `.Variant` or a qualified variant name. Error: `bare-variant-pattern`.
3. r[flow.match.bare-payload] An unqualified identifier followed by a payload list, such as `Some(value)` or `Ok(value)`, is also an error. Error: `bare-variant-pattern`.
4. r[flow.match.optional] An optional is matched like any other enum.
5. r[flow.match.optional.some] For a subject of type `T?`, `.Some(value)` matches only the present case and binds `value` as `T`.
6. r[flow.match.optional.none] `.None` matches only absence.
7. r[flow.match.optional.qualified] `Option.Some(value)` and `Option.None` are the qualified forms.
8. r[flow.match.optional.bare] A bare `value` binds the entire `T?`.
9. r[flow.match.optional.bare-variant] A bare `None` or `Some(value)` is an error. Error: `bare-variant-pattern`.
10. r[flow.match.optional.nested] Matching `T??` with `.Some(value)` removes only the outer layer, so `value` has type `T?`. `.Some(.Some(value))` reaches the inner value.
11. r[flow.match.optional.no-pattern] There is no optional-specific pattern: `value?` is not a pattern.
12. r[flow.match.contextual] A `.Variant` pattern whose expected subject or nested payload type is not an enum is an error, as for `.Some(value)` against an `i32` subject. Error: `missing-contextual-enum-type`.

```text
fn value_or_zero(value: i32?) -> i32:
    match value:
        Some(number) => number  # error: bare-variant-pattern
        .None => 0

fn nonoptional(value: i32) -> i32:
    match value:
        .Some(present) => present  # error: missing-contextual-enum-type
        _ => 0
```

### Payload Patterns

Enum payload patterns use call-style parentheses. Positional patterns come
first, and `field=pattern` names a payload field:

```text
match expr:
    Expr.Add(l, r) => l + r
    Expr.Sub(left=l, right=r) => l - r
    Expr.Scale(left, factor=2) => left * 2
```

1. r[flow.match.payload.parentheses] Enum payload patterns use call-style parentheses.
2. r[flow.match.payload.order] Positional patterns come first, and `field=pattern` names a payload field.
3. r[flow.match.payload.required] A pattern for a variant that carries a payload must list that payload, with one pattern per payload field.
4. r[flow.match.payload.arity] A bare variant pattern for such a variant, or a payload list of a different length, is an error. Error: `pattern-arity`.
5. r[flow.match.payload.unknown] A named pattern that names no payload field of the variant is an error. Error: `unknown-data-field`.
6. r[flow.match.payload.binding] A bare identifier in payload position binds a new arm-local name. It does not need to match the payload field's declaration name.
7. r[flow.match.payload.name-mismatch] The compiler emits `variant-binding-name-mismatch` when a positional bare binding equals a different payload field's name in that same variant.
8. r[flow.match.payload.name-mismatch.example] For example, binding `else_value` in the `then_value` position emits `variant-binding-name-mismatch`.
9. r[flow.match.literal] A literal pattern requires an equal value.
10. r[flow.match.variant.shorthand] An enum variant pattern may use `.Variant` when its subject or enclosing payload position fixes one enum type; otherwise it uses the qualified `Enum.Variant` form.
11. r[flow.match.variant.shorthand-rules] The shorthand has the same exhaustiveness and GADT refinement rules as the qualified form.

```text
enum Flag:
    On(value: i32)
    Off

enum Choice:
    Pair(left: i32, right: i32)

fn choose(flag: Flag) -> i32:
    match flag:
        Flag.On => 1  # error: pattern-arity
        Flag.Off => 0

fn bad(choice: Choice) -> i32:
    match choice:
        Choice.Pair(missing=value, right=other) => value  # error: unknown-data-field
```

> **Note.** Use named patterns to make an intentional reorder explicit.

### Payload Bindings

1. r[flow.match.no-ownership] Matching does not transfer ownership.
2. r[flow.match.bind.primitive] Primitive payloads bind by value.
3. r[flow.match.bind.composite] Composite payloads bind reference access after applying the matched subject's viewpoint permission when the payload directly declares a non-generic mutable type.
4. r[flow.match.bind.generic] A payload declared with a generic parameter instead binds its substituted type, including `mut U`.
5. r[flow.match.bind.tuple] Tuple pattern elements retain their declared types.
6. r[flow.match.literal-payload] A literal-constrained payload pattern such as `Expr.Scale(value, factor=2)` covers only that subset of the variant.
7. r[flow.match.literal-payload.rest] Another arm must therefore cover the remaining `Scale` values unless a later catch-all does.

### Nested And Data Patterns

A data pattern destructures a data value by field:

```text
match value:
    Shape.Dot(Point { x: 0, y }) => y
    Shape.Dot(Point { x: px, y: _ }) => px
    Shape.Line(start=Point { x: 0 }, end=_) => 0
    Shape.Line(start=_, end=_) => 1
```

1. r[flow.match.nested] Nested variant and tuple patterns are permitted by the grammar.
2. r[flow.match.data] Data destructuring patterns are also permitted.
3. r[flow.match.data.bind] `User { name }` binds the `name` field, and `User { name: alias }` binds it as `alias`.
4. r[flow.match.data.literal] `User { age: 18 }` matches only values with that field value.
5. r[flow.match.data.unlisted] Unlisted fields are ignored.
6. r[flow.match.data.label] As in a data expression, a field label inside the braces is followed by `:`. The `=` label belongs to payload patterns in parentheses.
7. r[flow.match.data.fields] Listed fields must be distinct and exist on the named data type.
8. r[flow.match.data.public] A cross-module pattern may name only public fields.
9. r[flow.match.data.empty] An empty data pattern is irrefutable for that data type.
10. r[flow.match.data.nominal] A named data pattern must match the subject's nominal type. It does not structurally match a different data type with the same fields.
11. r[flow.match.data.bind-primitive] Primitive fields bind by value.
12. r[flow.match.data.bind-composite] Composite fields bind reference access under the data subject's effective field types.
13. r[flow.match.data.readonly-mut] A direct `mut U` field in a readonly subject binds as `U`.
14. r[flow.match.data.generic] A generic field declared `field: P` binds as substituted `P`, including `mut U` when `P = mut U`.

```text
data User:
    name: string
    age: i32

fn read(user: User) -> i32:
    match user:
        User { missing } => 1  # error

fn invalid(user: User) -> string:
    match user:
        User { name, name: alias } => alias  # error
```

See also: [Generalized Algebraic Data Types](13-gadts.md), which defines GADT
pattern refinement.

## Return

`return` completes the nearest enclosing named function or closure.

1. r[flow.return.value] `return expression` immediately completes the nearest enclosing named function or closure with that value.
2. r[flow.return.type] The value must be assignable to the declared or inferred return type.
3. r[flow.return.bare] Bare `return` is valid only for a `void`-returning function or closure.
4. r[flow.return.outside] `return` outside a named function or closure is a compile-time error.
5. r[flow.return.script] A module script and a `test` block are not implicit return targets.
6. r[flow.return.fallthrough] Falling through a function body evaluates its final expression as the return value.
7. r[flow.return.void-fallthrough] A function declared `-> void` may fall through after a statement whose result is `void`.
8. r[flow.return.callback] Inside a trailing callback block, `return` completes the generated callback, not the function containing the call.
9. r[flow.return.non-local] hd-lang has no non-local return from a closure.

```text
return 1  # error
```

## Deferred Cleanup

`defer:` registers a cleanup suite:

```text
fn read_first!(path: string) -> Result[string, ResourceError[FileError]] $ Files:
    let handle: mut FileHandle = $.use(Files).open!(path)?
    defer:
        _ := handle.close()
    handle.read!()
```

### Registration And Order

1. r[flow.defer.register] `defer:` registers a synchronous cleanup suite on the innermost executing lexical block.
2. r[flow.defer.not-run] Reaching the statement does not run its suite.
3. r[flow.defer.run] Registered suites run once in last-in, first-out order when that block exits normally, through `return`, `break`, or `continue`, or because postfix `?` propagates.
4. r[flow.defer.loop] Each loop iteration has its own body block, so its registered suites run before the next iteration begins.
5. r[flow.defer.result-saved] A value-producing block evaluates and saves its result before running its registered suites.
6. r[flow.defer.operands] Likewise, a `return` or `break` operand and a value being propagated by `?` are evaluated before cleanup begins.
7. r[flow.defer.captures] A cleanup suite observes captured lexical storage at cleanup time.

### Cleanup Scopes

1. r[flow.defer.scopes] Cleanup scopes are function and closure bodies, loop bodies, each selected `if` or `else` suite, match arms, provider scopes, trailing callback blocks, and test bodies.
2. r[flow.defer.not-scopes] Module top level and declaration bodies that do not execute are not cleanup scopes.
3. r[flow.defer.outside] A `defer` there is an error. Error: `defer-outside-cleanup-scope`.

### Cleanup Suite Restrictions

1. r[flow.defer.void] A `defer` suite must produce `void`.
2. r[flow.defer.restricted] It cannot bang-call, otherwise suspend, propagate with `?`, or transfer control with `return`, `break`, or `continue`.
3. r[flow.defer.suspend] A bang call or other suspending operation in the suite is an error. Error: `suspending-defer`.
4. r[flow.defer.control] Propagation with `?`, or a `return`, `break`, or `continue` that would leave the suite, is an error. Error: `defer-control-flow`.
5. r[flow.defer.block-on] Direct or transitive use of `std.task.block_on` is an error. Error: `suspension-forbidden-context`.

```text
defer:  # error: defer-outside-cleanup-scope
    pass

fn cleanup!() -> void:
    pass

fn invalid!() -> void:
    defer:
        cleanup!()  # error: suspending-defer
    pass

fn run() -> i32:
    defer:
        return 1  # error: defer-control-flow
    0
```

See also: [Suspending Functions](11-requirements-and-suspension.md#suspending-functions).

### Panics And Escaping Aliases

1. r[flow.defer.panic] A runtime panic does not run pending `defer` suites.
2. r[flow.defer.cleanup-panic] If a cleanup suite itself panics, the instance is poisoned and no remaining cleanup suite is guaranteed to run.
3. r[flow.defer.alias-escape] `defer` does not stop an alias to a closed handle from escaping.

> **Why.** Core hd-lang has no panic unwinding.

> **Note.** Ownership and alias-escape prevention remain
> [open design work](../future-work/OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy).

## Unreachable Code

This section defines unreachable statements.

1. r[flow.unreachable.def] Statements following an unconditional `return`, `break`, or `continue` in the same suite are unreachable.
2. r[flow.unreachable.warning] The compiler must emit an `unreachable-code` warning, but the warning does not reject the program by default.
3. r[flow.unreachable.checked] Unreachable code is still parsed and type-checked.
4. r[flow.unreachable.policy] Tooling may provide a package policy that promotes this warning to an error without changing language semantics.

## Runtime Panics

This section defines runtime panics.

1. r[flow.panic.def] A runtime panic is an abrupt, unrecoverable failure of the current program instance.
2. r[flow.panic.not-result] It is distinct from a recoverable `Result` error and is not catchable by core hd-lang source code.
3. r[flow.panic.sources] Integer overflow, division errors, invalid shifts, out-of-bounds indexing, and invalidated built-in iterators panic when their owning chapters require a checked runtime failure.

### Panic Behavior

1. r[flow.panic.stop] When a panic occurs, ordinary evaluation stops immediately.
2. r[flow.panic.nothing-runs] No enclosing `else` suite, remaining expression, or caller statement executes.
3. r[flow.panic.runtime-cleanup] Runtime and library mechanisms may observe the complete exit and perform mandatory runtime cleanup.
4. r[flow.panic.no-handler] Core source has no panic handler or unwinding construct.
5. r[flow.panic.report] The runtime must report at least a stable failure category and source location when one is available.
6. r[flow.panic.encoding] Its Wasm trap, host error, and diagnostic encoding are ABI details.

### Program Instances

1. r[flow.panic.instance] One program instance is the instantiated module graph and execution state that [Module Initialization](10-modules.md#module-initialization) defines.
2. r[flow.panic.poison] A panic poisons that instance: the host must not invoke it again and must discard it after reporting the failure.
3. r[flow.panic.isolation] Hosts that require invocation isolation create a separate instance per invocation.

### Panic Categories

1. r[flow.panic.categories] Stable panic categories are exactly `annotation-reference-unresolved`, `annotation-resolution-reentry`, `assertion-failed`, `explicit-panic`, `integer-overflow`, `integer-division-by-zero`, `invalid-shift`, `index-out-of-bounds`, `iterator-invalidated`, `suspension-competing-driver`, `suspension-nested-driver`, `suspension-reentrant-poll`, `suspension-invalid-state`, and `stack-exhausted`.
2. r[flow.panic.explicit] The prelude function `panic(message: string) -> never` explicitly causes an `explicit-panic` failure.
3. r[flow.panic.never] A panic or other abrupt expression is valid in any value-producing arm without affecting the compatible result type of reachable normal arms.

> **Why.** `never` is assignable to every type.
