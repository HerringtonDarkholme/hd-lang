# Control Flow

Status: language specification draft.

This chapter defines suites, conditionals, loops, `match`, `return`, `defer`,
and runtime panics.

1. r[flow.suites] hd-lang control-flow constructs use indentation-delimited suites and may produce values.
2. r[flow.condition.bool] Conditions are always `bool`: there is no truthiness conversion from numbers, strings, collections, or optional values.

```hd
fn sign(n: i32) -> i32:
    if n > 0: +1
    else if n < 0: -1
    else: +0
```

## Blocks And Completion

A suite evaluates its statements in order, and its final expression may give
it a value.

### Suite Values

1. r[flow.block.order] A suite evaluates statements in source order.
2. r[flow.block.value] If control reaches the final expression statement normally, that expression is the suite's value.
3. r[flow.block.void] A suite whose final statement is not a value expression has type `void`.
4. r[flow.block.abrupt] `return`, `break`, `continue`, optional or result propagation with `?`, and a runtime panic complete the current control path abruptly. They do not produce the suite's ordinary final value.
5. r[flow.block.non-empty] An indented suite must contain at least one statement.
6. r[flow.block.pass] `pass` supplies an explicit no-op expression when a body is intentionally empty.

```hd
fn demo(flag: bool) -> i32:
    x := if flag:
        +1
    else:
        +2
    x
```

### Must-Use Values

1. r[flow.must-use.discard] An expression whose value is discarded and whose type is `Result[T, E]`, `T?`, or `mut Suspend[T]` is a compile-time error. Error: `discarded-must-use-value`.
2. r[flow.must-use.statement] This includes a non-final expression statement.
3. r[flow.must-use.discarded-suite] It also includes the final expression of any suite whose value is discarded:
   - a loop body;
   - an `if` without `else`;
   - a statement-position `match` arm;
   - a `defer` suite;
   - a module's top-level script.
4. r[flow.must-use.handled] Such a value must be propagated with `?`, inspected by `match`, returned, stored for later use, or explicitly discarded with `_ := expression` or `let _ = expression`.
5. r[flow.must-use.underscore] The `_` spelling does not bind a local name.

```text
fn save() -> Result[i32, SaveError]:
    .Err(SaveError { message: "failed" })

fn save_all(values: List[i32]) -> void:
    for value in values:
        save()  # error: discarded-must-use-value
    _ := save()  # valid: an explicit discard
    let _ = save()  # valid: the same discard
```

> **Why.** The `_` spelling makes the discard visible in review.

### Unused Bindings

1. r[flow.unused.warning] The compiler emits an `unused-local-binding` warning when an ordinary local binding is never read.
2. r[flow.unused.underscore] Names beginning with `_` suppress that warning.
3. r[flow.unused.must-use] The exception is an unread binding of a must-use value, which is an error. Error: `discarded-must-use-value`.
4. r[flow.unused.discard-forms] The two discard forms, `_ := expression` and `let _ = expression`, each explicitly discard a must-use value without binding it. No other form discards one.

```hd
fn demo() -> i32:
    _unused := +1   # names that begin with _ need no read
    +2
```

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

```hd
fn fee(rush: bool) -> i32:
    base := if rush: +10 else: +5
    base * 2
```

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
4. r[flow.for.pattern] The pattern of a `for` loop or a comprehension `for` clause matches each yielded value of type `T`. It matches as a `let` pattern matches an initializer of type `T`, under [Let Patterns](#let-patterns).
5. r[flow.for.pattern.irrefutable] The pattern must be irrefutable for `T`. A refutable pattern is an error, and a loop's `else` does not handle it. Error: `refutable-let-pattern`.

```text
data Point:
    x: i32
    y: i32

fn total(points: List[Point], scores: Map[string, i32]) -> i32:
    let sum = +0
    for Point { x, y } in points:
        sum = sum + x + y
    for (_, score) in scores:
        sum = sum + score
    sum
```

```text
fn bad(values: List[i32]) -> void:
    for (left, right) in values:  # error: type-mismatch
        pass

fn present(found: List[i32?]) -> void:
    for .Some(value) in found:  # error: refutable-let-pattern
        pass
```

> **Note.** A map yields `(K, V)` tuples, so `for (key, value) in entries`
> is a tuple pattern, and `for entry in entries` binds the whole tuple.

### Iteration Protocols

A `for` loop gets its iterator from the prelude trait `Iterable[T]`, or
takes a mutable iterator directly. The prelude `data` type `Iterator[T]` is
the one iterator type:

```text
trait Iterable[T]:
    fn iter(self) -> mut Iterator[T]

data Iterator[T]:
    step: fn() -> T?
```

1. r[flow.for.iterator-type] `Iterator[T]` is a concrete prelude `data` type, not a trait. Its one field, `step`, is a closure that yields the next item or `.None`.
2. r[flow.for.iterator-private] `step` is a private field. Code outside the module that declares `Iterator` cannot read it or build an `Iterator` with a data literal. [Field Visibility](08-data-and-enums.md#field-visibility) says the same of every private field.
3. r[flow.for.iterator-from-fn] The associated function `fn from_fn(step: fn() -> T?) -> mut Iterator[T]`, called as `Iterator::from_fn(step)`, builds an iterator whose `step` is the given closure.
4. r[flow.for.iterator-next] The method `fn next(mut self) -> T?` advances the iterator by calling `step` once.
5. r[flow.for.iter-independent] An ordinary iterable creates an independent mutable iterator on every `iter()` call.
6. r[flow.for.iterator-exhausted] The iterator stores traversal progress. Once `next` has returned `.None`, the iterator is **exhausted**.
7. r[flow.for.iterator-after-none] What `next` returns when it is called again on an exhausted iterator is unspecified, except for the panic that [Iterator Invalidation](#iterator-invalidation) requires.
8. r[flow.for.iterator-from-fn.no-fuse] An iterator that `from_fn` builds calls `step` on every `next`, even after `step` has returned `.None`.
9. r[flow.for.iterable-collections] `List[T]` and `Map[K, V]` implement `Iterable`: a list yields `T`, and a map `(K, V)`.
10. r[flow.for.iterator-not-iterable] `Iterator[T]` does not implement `Iterable[T]`.
11. r[flow.for.string-not-iterable] `string` does not implement `Iterable`, so a loop over a string is an error. Error: `unsatisfied-trait-bound`.
12. r[flow.for.string-explicit] A loop over a string's contents iterates `chars()`, `char_indices()`, or `bytes()`, which [String Methods](10-modules.md#string-methods) defines.
13. r[flow.for.accepts] `for` accepts a value whose type implements `Iterable[T]`, or an `Iterator[T]` expression with mutable access.
14. r[flow.for.iterable] When the iterable expression's type implements `Iterable[T]`, the loop calls `iter` once and advances the resulting cursor.
15. r[flow.for.iterator-direct] When the iterable expression is an `Iterator[T]` with mutable access, the loop advances that iterator itself, without calling `iter`, cloning it, or resetting it.
16. r[flow.for.iterator.position] Iteration therefore continues from the cursor's current position and leaves it exhausted.
17. r[flow.for.iterator-mut] An iterator must be accessed mutably to advance. A loop over an `Iterator[T]` expression that has only readonly access is an error. Error: `mutable-receiver-required`.
18. r[flow.for.not-iterable-or-iterator] An iterable expression whose type neither implements `Iterable[T]` nor is `Iterator[T]` is an error. Error: `unsatisfied-trait-bound`.
19. r[flow.for.comprehension] Comprehension `for` clauses accept the same values by the same rules.
20. r[flow.for.iterator-no-bound] A generic parameter bounded by `Iterable[T]` does not accept an iterator argument, readonly or mutable. Error: `unsatisfied-trait-bound`.

> **Why.** `Iterable[T]` promises a fresh, independent traversal on every
> `iter()` call. A function bounded by it may loop over its argument twice,
> or zip it with itself, and see every element each time. An iterator is a
> single cursor and cannot keep that promise. If it implemented `Iterable`,
> a second pass would silently see only what the first pass left, or
> nothing, as with Python generators. So iterators stay outside `Iterable`.
>
> `for` still consumes an iterator directly, by
> [`flow.for.iterator-direct`](#r-flow.for.iterator-direct), because there
> the consumption is visible at the call site. To pass an iterator where an
> `Iterable` is expected, collect it first (`let rest: List[T] =
> it.collect()`) or make it the receiver (`it.zip(other)`).

```text
fn countdown(start: i32) -> mut Iterator[i32]:
    let left: i32 = start
    Iterator::from_fn(fn() -> i32?:
        left = left - 1
        if left < 0: .None else: left + 1
    )
```

```text
fn drain(source: mut Iterator[i32]) -> i32:
    let total = +0
    for value in source:
        total = total + value
    total

fn twice(values: List[i32]) -> i32:
    let total = +0
    for value in values.iter():
        total = total + value
    for value in values.iter():
        total = total + value
    total
```

```text
fn main(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]  # error: mutable-receiver-required

fn bad() -> void:
    for value in 1:  # error: unsatisfied-trait-bound
        pass

fn letters(text: string) -> void:
    for letter in text:  # error: unsatisfied-trait-bound
        pass

fn peek(source: Iterator[i32]) -> fn() -> i32?:
    source.step  # error: private-member

fn count[I < Iterable[i32]](source: I) -> i32:
    let total = +0
    for item in source:
        total = total + item
    total

fn from_iterator(source: Iterator[i32]) -> i32:
    count(source)  # error: unsatisfied-trait-bound
```

> **Note.** An iterator is single-pass, and there is no `clone` or `tee`
> of an iterator. To traverse the items twice, call `iter()` on the
> collection again, as `twice` does, or collect the iterator into a list
> first ([Collect Targets](../std/iter.md#collect-targets)).

> **Why.** `next` takes `mut self` and `step` is private, so no readonly
> path advances an iterator. A readonly `Iterator[T]` parameter therefore
> promises that the function does not advance it. Taking a mutable
> iterator directly is the one exception in `for`, so `Iterator[T]` needs
> no `Iterable` impl.

> **Why.** One concrete iterator type lets adapters be ordinary methods,
> generic ones such as `map[U]` included, with no dynamic-safety question.
> The cost is one closure call per item.

See also: [Iterators](../std/iter.md), the stdlib-tier chapter that specifies
the iterator adapters, `collect`, and `FromIterator`.

### Built-In Collection Iteration

1. r[flow.for.list] The built-in `List[T]` iterable yields each element as `T`, including `mut U` when `T = mut U`, even through a readonly list.
2. r[flow.for.map] The built-in `Map[K, V]` iterable yields `(K, V)` tuples in insertion order.
3. r[flow.for.map.value] Destructuring each entry preserves `V`, including `mut U` when `V = mut U`, even through a readonly map.
4. r[flow.for.no-root-grant] Iteration does not grant mutable element access merely because the list root is mutable. The declared list or map value type determines that permission, and mutating a readonly element is an error. Error: `readonly-root`.
5. r[flow.for.library] Libraries may provide additional mutable-iteration APIs with separate aliasing rules.

```hd
fn total(items: List[i32]) -> i32:
    let sum = +0
    for item in items:
        sum = sum + item
    sum
```

### Range Iteration

A `for` loop iterates the integers of a [range](05-expressions.md#range-expressions):

```text
fn total(n: i32) -> i32:
    let sum = 0
    for i in 0..n:
        sum = sum + i
    for i in 1..=3:
        sum = sum + i
    sum

fn first_square_over(limit: i32) -> i32:
    let found = 0
    for i in 1..:
        if i * i > limit:
            found = i
            break
    found
```

1. r[flow.for.range.iterable-two] For each integer type `T`, `Range[T]` and `RangeFrom[T]` implement `Iterable[T]`.
2. r[flow.for.range.half-open] Iterating `a..b` yields `a`, `a + 1`, and so on, up to but not including `b`. It yields nothing when `a >= b`.
3. r[flow.for.range.inclusive] Iterating `a..=b` yields `a` through `b`, and yields `b` even when it is the type's largest value. It yields nothing when `a > b`.
4. r[flow.for.range.from] Iterating `a..` yields `a`, `a + 1`, and so on, with no end, so `for i in 0..:` runs until the loop exits another way.
5. r[flow.for.range.from-overflow] An iterator over `a..` has no end. Asking it for the item after the type's largest value is a checked runtime panic in a debug or test build, as `a + 1` is. It wraps in a release build, again as `a + 1` does. Panic: `integer-overflow`.
6. r[flow.for.range.fresh] Each `iter()` call on a range starts from its start bound, and iterating never changes the range value.
7. r[flow.for.range.to] `RangeTo[T]` does not implement `Iterable`, since it has no start, so a loop over `..b` is an error. Error: `unsatisfied-trait-bound`.
8. r[flow.for.range.to-through] `..=b` is a `RangeTo[T]` too, so a loop over it is an error. Error: `unsatisfied-trait-bound`.
9. r[flow.for.range.full] `RangeFull` does not implement `Iterable` either, so a loop over `..` is an error. Error: `unsatisfied-trait-bound`.

```text
fn invalid(n: i32) -> void:
    for i in ..n:  # error: unsatisfied-trait-bound
        pass
    for i in ..:   # error: unsatisfied-trait-bound
        pass
```

> **Note.** `std` writes one implementation per integer type, so no
> stepping trait is needed.

### Iterator Invalidation

1. r[flow.for.length] Built-in list and map iterators capture the collection's length when they are created.
2. r[flow.for.invalidate] An iterator is invalidated when the collection's length differs from the captured length.
3. r[flow.for.invalidate.panic] An invalidated iterator's next `next` call causes a checked runtime panic, even when the iterator was already exhausted. Panic: `iterator-invalidated`.
4. r[flow.for.replace] Replacing an existing list element or map value keeps the length, so it does not invalidate the iterator.
5. r[flow.for.replace.observed] Later visits observe the replacement.
6. r[flow.for.user-defined] User-defined iterables must document equivalent mutation behavior in their own contract.

```hd
fn demo() -> void:
    let items: mut List[i32] = [+1, +2]
    for item in items:
        items.push(item)   # panics with iterator-invalidated
```

> **Note.** The check compares lengths only, so a removal and an addition
> between two `next` calls that restore the length are not detected. This is
> memory-safe: the iterator reads the collection's current contents.

## While Loops

A `while` loop repeats its body under a `bool` condition:

```text
while index < names.len():
    index = index + 1
```

1. r[flow.while.condition] A `while` loop evaluates its `bool` condition before every iteration.
2. r[flow.while.initially-false] If the condition is initially `false`, the body does not execute.

### Infinite Loops

A `while true` loop runs until its body leaves it, so a function may return
from inside it with nothing after it:

```text
fn try_connect(attempt: i32) -> bool:
    attempt >= 3

fn connect() -> i32:
    let attempt = +1
    while true:
        if try_connect(attempt):
            return attempt
        attempt = attempt + 1
```

1. r[flow.while.infinite] A `while` loop whose condition is the literal `true`, alone or in grouping parentheses, is an **infinite loop**.
2. r[flow.while.infinite.literal] No other condition makes a loop infinite: not a named constant, not `!false`, and not a variable that holds `true`.
3. r[flow.while.infinite.exit] An infinite loop completes normally only through a `break` that targets it.
4. r[flow.while.infinite.never] An infinite loop that no `break` targets has type `never`, with or without `else`.
5. r[flow.while.infinite.else] The `else` suite of an infinite loop is never evaluated. It is still type-checked, and it still joins the loop result type, as [`flow.loop.else.type`](#r-flow.loop.else.type) states.

> **Why.** hd has no `loop` keyword; `while true` takes its place, with the
> rule Java and Go use for reachability. Only the literal counts, so the loop
> head alone shows that the loop is infinite. A constant would make
> reachability depend on a value defined elsewhere.

> **Note.** A `break` exits the nearest enclosing loop, by
> [`flow.break`](#r-flow.break). So a `break` inside a nested loop targets
> that nested loop, and leaves the outer infinite loop running.

See also: [Unreachable Code](#unreachable-code),
[Bodies And Control Paths](07-functions.md#bodies-and-control-paths),
[The `never` Type](04-type-system.md#the-never-type).

## Break, Continue, And Loop Else

`break` and `continue` leave the current loop body, and a loop with `else`
produces a value.

### Break And Continue

1. r[flow.continue] `continue` skips the remainder of the current loop body and begins the next iteration.
2. r[flow.break] `break` exits the nearest enclosing loop.
3. r[flow.break.boundary] Neither operation targets a loop across a function or closure boundary.
4. r[flow.break.outside-loop] `break` and `continue` outside a loop are compile-time errors, including at module top level and directly in a test body. Error: `break-outside-loop`.

```text
fn main() -> void: break  # error: break-outside-loop
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

1. r[flow.loop.void.type] A loop without `else` has type `void`, unless it is an infinite loop that no `break` targets, which has type `never` by [`flow.while.infinite.never`](#r-flow.while.infinite.never).
2. r[flow.loop.void.break] A loop without `else` permits plain `break` but not `break value`. A `break value` there is an error. Error: `break-value-context`.
3. r[flow.loop.else.value] A `for` or `while` loop with `else` is value-producing.
4. r[flow.loop.else.break-value] `break value` terminates the loop and supplies its value.
5. r[flow.loop.else.exhaustion] Normal exhaustion of a `for` loop, or a `false` `while` condition, evaluates the `else` suite and uses that suite's value.
6. r[flow.loop.else.plain-break] Plain `break` in a value-producing loop is an error. Error: `break-value-context`.
7. r[flow.loop.else.type] Every reachable `break value` and the `else` suite must produce one compatible loop result type.
8. r[flow.loop.else.continue] `continue` does not produce a loop value.
9. r[flow.loop.else.skipped] The `else` suite is not executed after any `break`, abrupt function return, propagation with `?`, cancellation, or runtime panic.

```text
fn invalid(values: List[i32]) -> void:
    for value in values:
        break value  # error: break-value-context

fn main() -> i32:
    while true:
        break  # error: break-value-context
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

```hd
fn grade(score: i32) -> string:
    match score:
        90 => "top"
        s if s >= 70 => "passing"
        _ => "needs work"
```

### Exhaustiveness

1. r[flow.match.exhaustive] A match must be exhaustive. Coverage is checked as the table below states. Error: `nonexhaustive-match`.

| Rule | Coverage |
| --- | --- |
| r[flow.match.cover.enum] Enum | A nominal enum, including `Option[T]` (written `T?`), requires every inhabitable variant, accounting for payload constraints, or a catch-all pattern. |
| r[flow.match.cover.bool] Bool | `bool` is covered by both `true` and `false`, or by a catch-all pattern. |
| r[flow.match.cover.tuple] Tuple | A tuple pattern covers the tuple values covered recursively by its element patterns. |
| r[flow.match.cover.unit] Unit | `void` is covered by the [unit pattern](#unit-pattern) `()`, or by a catch-all pattern. |
| r[flow.match.cover.data] Data | A data pattern covers its nominal data type when every listed field pattern is irrefutable, while unlisted fields are unconstrained. |
| r[flow.match.cover.integer] Integer | An integer type is covered when its literal and [range patterns](#range-patterns) together hold every value of the type, or by a catch-all pattern. |
| r[flow.match.cover.other-values] Other values | Floating-point, `char`, `string`, and other value spaces require an irrefutable catch-all after any literal cases. |

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
    match status:  # error: nonexhaustive-match
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
2. r[flow.match.bare-variant] If a bare identifier resolves to a variant of the subject enum, it is an error rather than a new catch-all binding. Write `.Variant` or a qualified variant name. Error: `bare-variant-pattern`.
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

```hd
enum Event:
    Click(x: i32, y: i32)
    Key(key: string)

fn describe(event: Event) -> string:
    match event:
        Event.Click(x, y) => "click at $x,$y"
        Event.Key(key) => "key $key"
```

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
8. r[flow.match.data.fields.unknown] A listed field that the data type does not declare is an error. Error: `unknown-data-field`.
9. r[flow.match.data.fields.duplicate] A field listed twice is an error. Error: `duplicate-data-pattern-field`.
10. r[flow.match.data.public] A cross-module pattern may name only public fields.
11. r[flow.match.data.empty] An empty data pattern is irrefutable for that data type.
12. r[flow.match.data.nominal] A named data pattern must match the subject's nominal type. It does not structurally match a different data type with the same fields.
13. r[flow.match.data.bind-primitive] Primitive fields bind by value.
14. r[flow.match.data.bind-composite] Composite fields bind reference access under the data subject's effective field types.
15. r[flow.match.data.readonly-mut] A direct `mut U` field in a readonly subject binds as `U`.
16. r[flow.match.data.generic] A generic field declared `field: P` binds as substituted `P`, including `mut U` when `P = mut U`.

```text
data User:
    name: string
    age: i32

fn read(user: User) -> i32:
    match user:
        User { missing } => 1  # error: unknown-data-field

fn invalid(user: User) -> string:
    match user:
        User { name, name: alias } => alias  # error: duplicate-data-pattern-field
```

See also: [Generalized Algebraic Data Types](13-gadts.md), which defines GADT
pattern refinement.

### Spread Patterns

A [spread pattern](02-grammar.md#r-grammar.pattern.tuple-spread) ends a
tuple pattern and binds the [rest element](04-type-system.md#rest-elements)
of a tuple to its list:

```text
fn total(t: (usize, usize, List[i32]...)) -> usize:
    let (a, b, xs...) = t
    a + b + xs.len()

fn lengths(rows: List[(string, List[i32]...)]) -> List[usize]:
    [for (_, xs...) in rows => xs.len()]

fn describe(t: (i32, List[i32]...)) -> string:
    match t:
        (0, _...) => "zero"
        (n, xs...) => "$n and ${xs.len()} more"
```

1. r[flow.match.spread.rest] A tuple pattern that ends in a spread pattern matches a tuple type with a rest element `List[T]...`. Its other subpatterns match the fixed elements one each, in order.
2. r[flow.match.spread.bind] The spread pattern `xs...` binds `xs` to the rest element's `List[T]`, so `xs` above is a `List[i32]`. The spread pattern `_...` binds nothing.
3. r[flow.match.spread.arity] The subpatterns before a spread pattern must be as many as the type's fixed elements. Error: `type-mismatch`.
4. r[flow.match.spread.fixed-tuple] A spread pattern against a tuple type without a rest element, or against any other type, is an error. Error: `type-mismatch`.
5. r[flow.match.spread.required] A tuple pattern without a spread pattern against a tuple type with a rest element is an error. Error: `type-mismatch`.
6. r[flow.match.spread.cover] A spread pattern covers every list, so a tuple pattern that ends in one is irrefutable when its other subpatterns are.

```text
fn fixed(pair: (i32, i32)) -> i32:
    let (first, rest...) = pair  # error: type-mismatch
    first

fn plain(t: (i32, List[i32]...)) -> i32:
    let (first, rest) = t  # error: type-mismatch
    first

fn short(t: (i32, i32, List[i32]...)) -> i32:
    let (first, rest...) = t  # error: type-mismatch
    first
```

> **Note.** A spread pattern stands wherever a tuple pattern does: in a
> `let`, a `for` loop or comprehension clause, and a `match` arm. There
> are no list patterns, so there is no `[a, rest...]` form.

> **Why.** The pattern mirrors the type `(i32, i32, List[i32]...)` and
> the expression `(a, b, xs...)`, so a rest tuple is built and taken apart
> in one shape.

### Unit Pattern

The unit pattern `()` matches the unit value, so a void success is matched
as it is built:

```text
fn save(ready: bool) -> Result[void, string]:
    if ready:
        return .Ok(())
    .Err("not ready")

fn report(ready: bool) -> string:
    match save(ready):
        .Ok(()) => "saved"
        .Err(message) => message
```

1. r[flow.match.unit] The [unit pattern](02-grammar.md#r-grammar.pattern.unit) `()` matches the unit value `()`, the one value of `void`.
2. r[flow.match.unit.irrefutable] It is irrefutable for `void`, so `.Ok(())` covers every success of a `Result[void, E]`.
3. r[flow.match.unit.binds] It binds no name.
4. r[flow.match.unit.type] A unit pattern against a value whose type is not `void` is an error. Error: `type-mismatch`.

```text
fn read() -> Result[i32, string]:
    .Ok(1)

fn report() -> i32:
    match read():
        .Ok(()) => 0  # error: type-mismatch
        .Err(_) => 1
```

> **Why.** Rust and Swift match `()` with `()`. Construction and matching
> then read alike, and `.Ok(())` needs no `_` that hides what it matches.

### Range Patterns

A [range pattern](02-grammar.md#r-grammar.pattern.range) matches the
integers that the range of the same form holds:

```text
fn bucket(n: u8) -> string:
    match n:
        0 => "zero"
        1..10 => "small"
        10..=99 => "medium"
        100.. => "large"

fn sign(n: i8) -> i32:
    match n:
        ..=-1 => -1
        0 => 0
        1.. => 1
```

| Rule | Pattern | Matches |
| --- | --- | --- |
| r[flow.match.range.half-open] Half-open | `a..b` | the integers from `a` up to, not including, `b` |
| r[flow.match.range.inclusive] Inclusive | `a..=b` | the integers from `a` through `b` |
| r[flow.match.range.from] From | `a..` | the integers from `a` through the type's largest value |
| r[flow.match.range.to-inclusive] To inclusive | `..=b` | the integers from the type's smallest value through `b` |
| r[flow.match.range.to] To | `..b` | the integers from the type's smallest value up to, not including, `b` |

1. r[flow.match.range.subject] A range pattern's subject must have an integer type. A range pattern against any other type is an error. Error: `type-mismatch`.
2. r[flow.match.range.bound-type] Each bound is checked with the subject's type as its expected type, so a bound outside that type is an error. Error: `integer-literal-range`.
3. r[flow.match.range.no-bind] A range pattern binds no name and builds no range value.
4. r[flow.match.range.cover] A range pattern covers exactly the values it matches, by [`flow.match.cover.integer`](#r-flow.match.cover.integer), so `..=-1`, `0`, and `1..` cover `i8`.
5. r[flow.match.range.empty] A range pattern that matches no value, as `5..5` or `3..=1`, is an unreachable arm. Error: `unreachable-match-arm`.
6. r[flow.match.range.covered] An arm whose every value earlier unguarded arms cover is unreachable, by [`flow.match.duplicate.covered`](#r-flow.match.duplicate.covered). Error: `unreachable-match-arm`.
7. r[flow.match.range.overlap] Arms that overlap only in part are not an error, and the first matching arm is selected.

```text
fn invalid(name: string) -> string:
    match name:
        0..=9 => "digit"  # error: type-mismatch
        _ => "other"

fn too_wide(n: u8) -> string:
    match n:
        0..=300 => "byte"  # error: integer-literal-range

fn missing(n: u8) -> string:
    match n:  # error: nonexhaustive-match
        0..=127 => "low"
        129.. => "high"

fn covered(n: i32) -> string:
    match n:
        0..=9 => "digit"
        3..5 => "never"  # error: unreachable-match-arm
        _ => "other"
```

> **Note.** A pattern names no constant, so each bound is a literal.

> **Why.** An integer type is a finite range of values, so ranges can
> cover it without a catch-all, as Rust's checker does.

## Let Patterns

A `let` pattern matches its initializer as a `match` arm's pattern would,
and a let-else block handles the values it does not match:

```text
data Point:
    x: i32
    y: i32

fn find(id: i32) -> Point?:
    if id == 0: .Some(Point { x: 1, y: 2 }) else: .None

fn read(id: i32) -> i32:
    let .Some(point) = find(id) else: return 0
    let Point { x, y } = point
    x + y
```

1. r[flow.let.match] A `let` pattern matches the initializer's value under the pattern rules of [Match Expressions](#match-expressions), as one unguarded arm would.
2. r[flow.let.bind] Its names bind as that arm's names would, as [Payload Bindings](#payload-bindings) and [Nested And Data Patterns](#nested-and-data-patterns) state.
3. r[flow.let.irrefutable] A pattern is **irrefutable** when it alone covers the initializer's type, under the coverage rules of [Exhaustiveness](#exhaustiveness). Any other pattern is **refutable**.
4. r[flow.let.irrefutable.no-else] A `let` with an irrefutable pattern needs no `else`.
5. r[flow.let.refutable.else] A `let` with a refutable pattern, such as a literal, an enum variant, or `.Some(v)`, must have an `else` block. A refutable pattern without one is an error. Error: `refutable-let-pattern`.
6. r[flow.let.else.order] A let-else evaluates its initializer once. When the pattern matches, it binds the names; otherwise the `else` block runs.
7. r[flow.let.else.diverge] The `else` block must diverge: control never reaches its end.
8. r[flow.let.else.diverge.forms] A block diverges when its final statement has type `never`, such as `return`, `break`, `continue`, or a call to `panic`. It also diverges when the final statement is an `if` or `match` whose every branch diverges.
9. r[flow.let.else.falls-through] An `else` block that may complete normally is an error. Error: `let-else-falls-through`.
10. r[flow.let.else.unreachable] An `else` block after an irrefutable pattern could never run, so it is an error. Error: `unreachable-match-arm`.
11. r[flow.let.tuple-arity] A tuple pattern needs a tuple of the same arity. Against any other value it is an error. Error: `type-mismatch`.

```text
fn find(id: i32) -> i32?:
    if id == 0: .Some(7) else: .None

fn missing_else(id: i32) -> i32:
    let .Some(value) = find(id)  # error: refutable-let-pattern
    value

fn falls_through(id: i32) -> i32:
    let .Some(value) = find(id) else:  # error: let-else-falls-through
        println("missing")
    value

fn arity() -> i32:
    let (first, second) = (1, 2, 3)  # error: type-mismatch
    first

fn always(pair: (i32, i32)) -> i32:
    let (low, high) = pair else: return 0  # error: unreachable-match-arm
    low + high
```

> **Why.** An `else` block that fell through would reach code that reads
> names the pattern never bound. Rust's let-else and Swift's `guard let`
> require the same divergence.
> An `else` that can never run tells the reader the pattern may fail. That
> is false, so it is rejected as an unreachable arm would be.

See also: [Let-Else Statements](02-grammar.md#let-else-statements), and
[Let-Else Scope](03-names-and-scopes.md#let-else-scope) for where the
names are visible.

## Return

`return` completes the nearest enclosing named function or closure.

1. r[flow.return.value] `return expression` immediately completes the nearest enclosing named function or closure with that value.
2. r[flow.return.type] The value must be assignable to the declared or inferred return type.
3. r[flow.return.bare] Bare `return` is valid only for a `void`-returning function or closure.
4. r[flow.return.outside] `return` outside a named function or closure is a compile-time error. Error: `return-outside-function`.
5. r[flow.return.script-only] A module script is not an implicit return target.
6. r[flow.return.fallthrough] Falling through a function body evaluates its final expression as the return value.
7. r[flow.return.void-fallthrough] A function declared `-> void` may fall through after a statement whose result is `void`.
8. r[flow.return.callback] Inside a trailing callback block, `return` completes the generated callback, not the function containing the call.
9. r[flow.return.non-local] hd-lang has no non-local return from a closure.

```text
return 1  # error: return-outside-function
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

```hd
fn first() -> void: pass

fn second() -> void: pass

fn work() -> void: pass

fn demo() -> void:
    defer: first()
    defer: second()
    work()   # work, then second, then first
```

### Cleanup Scopes

1. r[flow.defer.scopes] Cleanup scopes are function and closure bodies, loop bodies, each selected `if` or `else` suite, and match arms. Provider scopes and trailing callback blocks, including test bodies, are cleanup scopes too.
2. r[flow.defer.not-scopes] Module top level and declaration bodies that do not execute are not cleanup scopes.
3. r[flow.defer.outside] A `defer` there is an error. Error: `defer-outside-cleanup-scope`.

```text
defer:   # error: defer-outside-cleanup-scope
    pass
```

### Cleanup Suite Restrictions

1. r[flow.defer.void] A `defer` suite must produce `void`.
2. r[flow.defer.restricted] It cannot bang-call, otherwise suspend, propagate with `?`, or transfer control with `return`, `break`, or `continue`.
3. r[flow.defer.suspend] A bang call or other suspending operation in the suite is an error. Error: `suspension-forbidden-context`.
4. r[flow.defer.control] Propagation with `?`, or a `return`, `break`, or `continue` that would leave the suite, is an error. Error: `defer-control-flow`.
5. r[flow.defer.block-on] Direct or transitive use of `std.task.block_on` is an error. Error: `suspension-forbidden-context`.

```text
defer:  # error: defer-outside-cleanup-scope
    pass

fn cleanup!() -> void:
    pass

fn invalid!() -> void:
    defer:
        cleanup!()  # error: suspension-forbidden-context
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

```hd
fn cleanup() -> void: pass

fn demo() -> void:
    defer: cleanup()   # not run: a panic skips pending defer suites
    panic("boom")
```

> **Why.** Core hd-lang has no panic unwinding.

> **Note.** Ownership and alias-escape prevention remain
> [open design work](../../future-work/OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy).

## Unreachable Code

This section defines unreachable statements.

1. r[flow.unreachable.def] Statements following an unconditional `return`, `break`, or `continue` in the same suite are unreachable.
2. r[flow.unreachable.infinite-loop] Statements following an infinite loop that no `break` targets, in the same suite, are unreachable.
3. r[flow.unreachable.warning] The compiler must emit an `unreachable-code` warning, but the warning does not reject the program by default.
4. r[flow.unreachable.checked] Unreachable code is still parsed and type-checked.
5. r[flow.unreachable.policy] Tooling may provide a package policy that promotes this warning to an error without changing language semantics.

```text
fn accept(port: i32) -> void:
    pass

fn serve(port: i32) -> void:
    while true:
        accept(port)
    println("server stopped")  # warning: unreachable-code
```

## Runtime Panics

This section defines runtime panics.

1. r[flow.panic.def] A runtime panic is an abrupt, unrecoverable failure of the current program instance.
2. r[flow.panic.not-result] It is distinct from a recoverable `Result` error and is not catchable by core hd-lang source code.
3. r[flow.panic.sources] Integer overflow, division errors, invalid shifts, out-of-bounds indexing, and invalidated built-in iterators panic when their owning chapters require a checked runtime failure. Integer overflow and invalid shifts panic in a debug or test build only. Exhausting the call stack also panics, by [`flow.panic.stack-exhausted`](#r-flow.panic.stack-exhausted).
4. r[flow.panic.stack-exhausted] Exhausting the call stack panics. The report gives a source location when one is available, as [`flow.panic.report`](#r-flow.panic.report) says. Panic: `stack-exhausted`.

```hd
fn demo(a: i32, b: i32) -> i32:
    a / b   # b == 0 panics with integer-division-by-zero
```

### Panic Behavior

1. r[flow.panic.stop] When a panic occurs, ordinary evaluation stops immediately.
2. r[flow.panic.nothing-runs] No enclosing `else` suite, remaining expression, or caller statement executes.
3. r[flow.panic.runtime-cleanup] Runtime and library mechanisms may observe the complete exit and perform mandatory runtime cleanup.
4. r[flow.panic.no-handler] Core source has no panic handler or unwinding construct.
5. r[flow.panic.report] The runtime must report at least a stable failure category and source location when one is available.
6. r[flow.panic.encoding] Its Wasm trap, host error, and diagnostic encoding are ABI details.
7. r[flow.panic.report.fallback] When an operation that panics with `integer-overflow` has a type from the [`usize` default](04-type-system.md#r-types.literal.local.default), the report must say so. The category stays `integer-overflow`.
8. r[flow.panic.report.fallback.fix] That report must suggest a signed literal or a type annotation, and should name the binding whose type fell back when the operation reads one.

```hd
fn must(n: i32) -> i32:
    if n < 0: panic("negative")
    n
```

> **Note.** The report text is not normative. For `let balance = 100`
> followed by `balance = balance - 150`, one such message is
> "`balance` fell back to `usize` (no signed literal); write `+100` or
> `let balance: i32 = 100`".

### Program Instances

1. r[flow.panic.instance] One program instance is the instantiated module graph and execution state that [Module Initialization](10-modules.md#module-initialization) defines.
2. r[flow.panic.poison] A panic poisons that instance: the host must not invoke it again and must discard it after reporting the failure.
3. r[flow.panic.isolation] Hosts that require invocation isolation create a separate instance per invocation.

```hd
tests:
    it("one case"):
        _ := +1
    it("another"):
        _ := +2   # a fresh program instance; nothing carries over
```

### Panic Categories

1. r[flow.panic.stable-categories] Stable panic categories are exactly `assertion-failed`, `explicit-panic`, `host-contract`, `integer-overflow`, `integer-division-by-zero`, `invalid-shift`, `index-out-of-bounds`, `iterator-invalidated`, `structure-variant-mismatch`, `suspension-competing-driver`, `suspension-reentrant-poll`, `suspension-invalid-state`, and `stack-exhausted`.
2. r[flow.panic.explicit] The prelude function `panic(message: string) -> never` explicitly causes an `explicit-panic` failure.
3. r[flow.panic.never] A panic or other abrupt expression is valid in any value-producing arm without affecting the compatible result type of reachable normal arms.

```hd
fn must(n: i32) -> i32:
    if n < 0: panic("negative")
    n
```

> **Why.** `never` is assignable to every type.
