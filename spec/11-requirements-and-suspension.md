# Requirements and Suspension

Status: language specification draft.

This chapter specifies static dependency requirements, provider injection, and
one-shot suspension. The mechanisms are related but deliberately separate.
The grammar and semantics in this chapter are part of hd-lang; additional
runtime and library policies are tracked as future work.

## Decomposed Model

hd-lang decomposes concerns often grouped under algebraic effects into:

1. **One-shot suspension.** `fn!`, `Suspend[T]`, bang calls, polling, and
   cancellation describe a resumable computation. This is not a general or
   multi-shot continuation system.
2. **Requirement checking.** `$` rows let the compiler prove that every
   dependency used by a function is available. A requirement is not
   necessarily a host capability.
3. **Dependency injection.** Contexts bind requirement keys to ordinary values
   that implement those requirement traits.

Normal errors are values represented by `Result[T, E]`. Dependency lookup does
not suspend, suspension does not raise ordinary errors, and declaring a
requirement does not by itself choose a provider.

Production, test, sandbox, and replay runtimes may bind different providers or
drivers at explicit requirement and suspension boundaries. Code outside those
boundaries is not generally reinterpreted.

## Requirement Rows

A **requirement row** is the normalized unordered set of requirement keys on a
callable signature. A **row parameter** is a generic parameter whose values are
requirement rows. These are the only terms used below for the concrete and
generic forms.

A function signature may end with `$` and an unordered row expression of
requirement traits:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database + Cache:
    ...
```

```ebnf
requirement_clause = "$", requirement_expression
                   | "$", "(", ")"
                   ;
requirement_expression = requirement_union,
                         { "-", requirement_key } ;
requirement_union = requirement_term, { "+", requirement_term } ;
requirement_term = requirement_key
                 | "(", requirement_expression, ")"
                 ;
requirement_key = trait_type ;
```

The clause `$()` writes the empty row explicitly.

Function declarations, closure expressions, and function types use the same
requirement clause:

```ebnf
function_decl = "fn", callable_name, [ generic_params ], parameter_clause,
                [ "->", type ], [ requirement_clause ], ":", suite_body ;

function_type = [ "mut" ], "fn", [ "!" ], "(", [ type_list ], ")",
                "->", type, [ requirement_clause ] ;

closure_expression = [ "mut" ], "fn", [ "!" ], closure_parameter_clause,
                     [ "->", type ], [ requirement_clause ],
                     ":", suite_body ;

callable_name = identifier, [ "!" ] ;
```

The same callable name and optional requirement clause apply to trait methods
and functions inside `impl` blocks. A trait requirement and its implementation
must agree on suspension and normalized requirement row behavior.

A public function, a trait method, or a method of a trait implementation
without a requirement clause has the empty row; its body may use only
requirements satisfied by an enclosing lexical provider scope. A non-public
function, inherent method, or local `fn` declaration without a requirement
clause has an inferred row, computed by the same rule as a closure's below.
Inferred rows of functions that call each other in a cycle are the least rows
that satisfy every member of the cycle. When a closure omits its requirement
clause, the compiler infers the least row
containing every requirement used by its body that is not satisfied by an
enclosing lexical provider scope. Calls through function parameters contribute
their normalized rows. If an expected function type contains a row parameter,
the inferred concrete row is unified with that parameter; omission never means
an empty row merely because the expected row is generic. A written `$` clause
is explicit and must entail the same body requirements.

Rows are sets: order does not affect type identity, and a key occurs at most
once after normalization. `+` forms set union. `r - Logger` removes `Logger`
from a row parameter `r`; removing an absent key is allowed and leaves the row
unchanged. Parentheses group row expressions. A function may call another
required function only when its own row includes those requirements or a
lexical provider scope satisfies them.

Requirement checking uses set entailment after alias expansion. For a body,
`available = declared_row + lexical_keys`, and every required key must be a
member of `available`. For an unknown row parameter `r`:

- `r` is entailed by `(r - K) + S` exactly when `K` is in `S`;
- `r` is not entailed by `r - K`;
- `K1 - K2` removes a generic key only when the two keys are identical after
  alias expansion; and
- inference for a parameter pattern `r + K` chooses the least row solution,
  so matching it against `{K}` infers the empty row for `r`.

Subtraction is legal on row parameters in parameter and result positions. The
compiler warns with `requirement-subtract-absent` when it can prove that the
subtracted key can never occur in the input row; the normalized row is still
unchanged.

A generic parameter used in requirement position is inferred to be a row
parameter. One parameter cannot be used as both an ordinary type and a
requirement row.

Requirement keys are traits, including interfaces such as `Database` and
host capabilities such as `Clock` or `Network`. The language does not introduce
a separate effect-declaration syntax.

## Provider Access

`$.use` retrieves providers from the statically known current context:

```text
db := $.use(Database)
db, cache := $.use(Database, Cache)
```

The result order matches the requested key order. A missing provider is a
compile-time error at every call site below an entry point. Entry-point rows
may contain only host capability traits declared by the selected runtime
profile. For a registered boundary, the registration contract's explicit
bindable-trait set is that boundary's profile. Failure to configure one of
those host providers is a pre-execution host configuration error. `$.use` is
non-suspending and performs no dynamic handler search that can fail at runtime
below that boundary.

A provider value returned by `$.use` is an ordinary value of its trait type. It
may flow anywhere an ordinary value of that type may flow, including fields,
collections, closure captures, return values, and suspension frames, and it
remains usable after its provider scope ends. A requirement row therefore
records unresolved provider lookup, not every authority a callable can
exercise through values it holds.

`$` is a special context namespace, not an ordinary value. Requirement keys in
its operations are type-level keys rather than named argument labels.

## Provider Scopes

`$.with` binds providers for one lexical trailing block:

```text
fn demo!() -> Result[User?, DbError] $ Cache:
    $.with(Database=mock_db):
        load_user!(UserId("user_123"))
```

Each provider expression is evaluated before entering the block and must have a
type implementing its named requirement trait. Nested scopes may replace an
outer provider for the same key within the nested block.

At each `$.with`, the compiler compares every newly bound key with every other
new key and with every declared-row or lexical key visible in the block. Two
distinct generic key expressions that can become identical under any valid
type-argument substitution are a `generic-requirement-key-collision` error.
The same check applies among entries of a `$.context` expression. For example,
a generic body may not make `Repo[T]` and `Repo[U]` concurrently visible,
because an instantiation can choose `T = U`; it likewise may not combine a
declared `Repo[T]` with a lexical `Repo[User]`, because `T` can be `User`.
An exact replacement written with the same key expression remains the ordinary
nested-scope override described above.
This check is performed before erasure or specialization, so compilation
strategy cannot change which provider a lookup selects. Distinct concrete keys
such as `Repo[User]` and `Repo[Post]` remain valid.

Reusable provider maps use `$.Context[Row]`:

```text
fn prod_context() -> $.Context[Metrics + Cache]:
    $.context(Metrics=metrics, Cache=cache)

$.with(Database=db, Logger=logger, ...prod_context()):
    ...
```

`$.Context[A + B]` is indexed by one unordered, duplicate-free requirement row;
it is not a variadic generic. `$.context` creates a context value. Context
spreads and explicit bindings are applied left to right, and the later binding
wins when the same key appears more than once. The resulting context still has
one provider per key.

```ebnf
context_use = "$", ".", "use", "(", requirement_key,
              { ",", requirement_key }, [ "," ], ")" ;

context_create = "$", ".", "context", "(", context_entries, ")" ;
context_type = "$", ".", "Context", "[", requirement_expression, "]" ;
context_scope = "$", ".", "with", "(", context_entries, ")",
                ":", suite_body ;

context_entries = context_entry, { ",", context_entry }, [ "," ] ;
context_entry = requirement_key, "=", expression
              | "...", expression
              ;
```

`context_use` and `context_create` are primary expressions; `context_type` is a
reference type; and `context_scope` is a suite expression.

Provider values are ordinary values and use ordinary trait implementations.
There is no separate `handler` declaration.

## Suspending Functions

A suspending declaration places `!` after its name:

```text
fn fetch_user!(id: UserId) -> Result[User, DbError]:
    ...
```

It introduces two call forms:

```text
let pending: mut Suspend[Result[User, DbError]] = fetch_user(id)
result := fetch_user!(id)   # Result[User, DbError]
```

The ordinary call constructs a cold suspension. It evaluates and captures
arguments and required providers but does not begin executing the body. The
bang call constructs that suspension and drives it as a child of the current
suspending computation. It is a possible suspension point.

The surface type of `fetch_user` is
`fn!(UserId) -> Result[User, DbError]`. A suspending function type is
definitionally equivalent to an ordinary constructor function returning a
suspension:

```text
fn!(A, B) -> T $ R
fn(A, B) -> mut Suspend[T] $ R
```

The first spelling preserves the source-level bang-call operation; the second
is its lowered callable type. Assignment or argument checking may convert the
first to the second, but not back. Calling either form without `!` constructs
the cold mutable `Suspend[T]`. Only the first form supports `callee!(...)`
directly. A `:=` binding weakens that fresh result to readonly `Suspend[T]`;
it cannot call `poll` or `cancel`. Store it with
`let pending: mut Suspend[T] = callee(...)` when it must be driven later.

Anonymous suspending callables use the same marker after `fn`:

```text
loader := fn!(id: UserId) -> Result[User, DbError] $ Database:
    db := $.use(Database)
    db.load_user!(id)
```

Requirement-bearing non-suspending closures omit `!` and place `$` after their
result type. Closure inference may infer a requirement row from an expected
function type, but it never silently makes a closure suspending.

```ebnf
suspension_call_suffix = "!", argument_clause ;
```

This suffix is part of `postfix_suffix` at ordinary call precedence. A bang call
is valid only in a **driver context**, which is exactly one of: a suspending
function or closure body, a `test` block, or the host executor driving
`main!`. Module top level is not a driver context. A driver is **active** while
its executor is evaluating or polling that driver context on the current
program-instance call stack. A pending invocation retained by the host between
polls is unfinished but not active. A test block counts as active throughout
its execution, including calls through non-suspending helpers. Merely using requirements
does not make a function suspending; a non-suspending function may have a `$`
row.

For any expression `s` of type `mut Suspend[T]`, `s!()` drives that stored
suspension to completion and has type `T`. The expression is evaluated once.
This is the same postfix bang suffix used for a direct `fn!` call; it is not a
method lookup. Non-suspending code imports `use std.task.block_on` and calls the
standard function `block_on[T](s: mut Suspend[T]) -> T`, which owns the driver
loop until the suspension completes or panics. `block_on` is forbidden in an
annotation builder, a default expression, a `defer` suite, or non-entry module
initialization; those contexts cannot start suspension work. This ban is
transitive through the statically known call graph. If a call through a
function value or dynamic trait method prevents the compiler from proving that
`block_on` is unreachable, the call is rejected in one of these contexts.
Every direct or transitive violation reports `suspension-forbidden-context`.
If any suspension
driver is already active in the program instance, calling `block_on` causes a
`suspension-nested-driver` panic before polling its argument. This includes a
call reached indirectly from a suspending body or during cancellation, and
prevents nested cooperative drivers from blocking one another.

The host executor is the driver for `main!`. This entry driver is
waker-driven: when a poll returns `Pending` because a host provider operation
is pending, the driver returns control to the host and polls again only after
a waker for that suspension is invoked. A driver that keeps polling a pending
host operation without returning to the host is not conforming.

The runtime-provided leaf
`std.task.host_wait![T](operation: std.task.HostWait[T]) -> T` maps an opaque
host wait operation to the WebAssembly Component Model async ABI as used by
WASI 0.3 host interfaces. User code obtains `HostWait[T]`
values only from host providers; the type has no public constructor.

## `Suspend[T]` Protocol

`Suspend[T]` is the stateful one-shot computation. `Poll[T]` describes one poll
result:

```text
enum Poll[T]:
    Pending
    Ready(T)

trait Suspend[T]:
    fn poll(mut self, context: PollContext) -> Poll[T]
    fn cancel(mut self) -> void
```

`Suspend[T]` is sealed. Only compiler-generated frames and implementations in
`std.task` may implement it; ordinary packages may consume the trait but cannot
declare an implementation. This makes the one-shot runtime checks part of the
protocol rather than an unenforceable user convention.

A `Suspend[T]` value is:

- cold until first polled;
- single-execution, not a reusable plan or memoized result;
- exclusively driven at runtime;
- stateful across normal successive polls while pending.

`PollContext` is constructed only by the host runtime and `std.task`; user code
cannot construct one. Standard-library suspension implementations access its
waker through this sealed surface:

```text
data PollContext: pass

impl PollContext:
    fn waker(self) -> Waker

trait Waker:
    fn wake(self) -> void
```

Ordinary user packages may receive a `PollContext` only inside APIs explicitly
provided by `std.task`; they cannot use it to implement the sealed `Suspend`
trait.

An operation returning `Pending` must arrange for the waker to be invoked when
another poll may make progress. A waker may be retained after `poll` returns,
invoked from a host callback, and invoked more than once; redundant wakes are
coalesced and never poll concurrently. The waker carries no result; state
remains in the suspension frame.

Competing drivers, reentrant polling, polling after `Ready`, or attempting a
second execution cause a runtime panic. Cancelling a suspension while it or
one of its descendants is active on the current poll stack causes
`suspension-reentrant-poll`; the cancellation performs no cleanup or state
transition. Cancellation is otherwise idempotent: cancelling an already
cancelled or completed suspension has no further effect. Polling a cancelled
suspension causes a runtime panic. These are runtime checks rather than
ownership rules in the type system. Competing drivers report
`suspension-competing-driver`; recursive polling and active-stack cancellation
report `suspension-reentrant-poll`; polling after completion or cancellation
and attempting a second execution report `suspension-invalid-state`.

Discarding a cold suspension that has never been polled has no cleanup work to
perform. Once polling has begun, a host or driver that stops owning the
suspension must cancel it before discarding it. Raw abandonment of a started
suspension does not run its registered `defer` suites.

There is no separate `Pollable` or public `Continuation[T]` abstraction.

Each program instance executes cooperatively on one thread. Bang calls are the
only language-level yield points; code between them does not interleave with a
sibling suspension in that instance. `std.task.all!` polls children in argument
order on its initial poll and again in argument order after every wake.

## Compilation Strategy

`fn name!(args) -> T` is source sugar for a compiler-generated cold state
machine implementing `Suspend[T]`. Conceptually, not as normative source code:

```text
fn adjusted(base: i32) -> mut Suspend[i32] $ Counter:
    counter := $.use(Counter)
    AdjustedFrame {
        state: AdjustedState.New(base=base, counter=counter),
    }
```

The frame stores captured arguments, construction-time providers, locals live
across suspension points, and a state discriminant. Its `poll` method advances
until it returns `Pending` or completes with `Ready(value)`:

```text
enum AdjustedState:
    New(base: i32, counter: Counter)
    Waiting(child: mut Suspend[i32], offset: i32)
    Complete

impl Suspend[i32] for AdjustedFrame:
    fn poll(mut self, context: PollContext) -> Poll[i32]:
        while true:
            match self.state:
                AdjustedState.New(base, counter) =>
                    let child: mut Suspend[i32] = counter.next()
                    self.state = AdjustedState.Waiting(child, offset=base)
                AdjustedState.Waiting(child, offset) =>
                    match child.poll(context):
                        Poll.Pending => return Poll.Pending
                        Poll.Ready(value) =>
                            self.state = AdjustedState.Complete
                            return Poll.Ready(offset + value)
                AdjustedState.Complete => panic("already completed")
```

The compiler may use a different representation, but it must preserve cold
construction, provider binding, suspension points, and single-execution
behavior.

## Construction-Time Requirement Binding

Requirements of a suspending function are resolved when its cold suspension is
constructed, not when a later driver first polls it. The chosen providers are
captured by the generated frame:

```text
let pending: mut Suspend[Result[User?, DbError]] = $.with(Database=mock_db):
    load_user(id)

# Driving pending later still uses mock_db.
```

This prevents a stored computation from silently changing dependencies when it
moves between drivers. A caller constructing a required suspension
must itself satisfy that requirement even if it does not immediately bang-call
the suspension.

## Cancellation

`cancel(mut self)` is synchronous and must not suspend. It marks the suspension
cancelled, synchronously propagates cancellation to an unfinished child, and
asks unfinished external operations to abort through their provider/runtime
contracts. After an unfinished child has completed its own cancellation
cleanup, registered `defer` suites in the suspension's unfinished frames run
synchronously in last-in, first-out order, from the innermost frame outward.

Cancellation is cooperative at suspension boundaries for language code, but a
runtime must not leave a known active HTTP request or equivalent external
operation running when its provider supports abort.

Cancellation runs already registered synchronous `defer` suites, but it does
not establish ownership or stop aliases from escaping. Cleanup that can fail or
requires asynchronous work needs a separate design; the cancellation hook
itself remains synchronous.

The `std.task` combinators `all!` and `race!` cancel their children when the
parent is cancelled. `race!` also synchronously cancels losing children before
returning the first completed value. A retry combinator in `std.task` cancels
its active attempt and starts no further attempt after parent cancellation.

The standard polling combinators in `std.task`, including `all!` and `race!`,
are compiler intrinsics. Each is imported and bang-called like an ordinary
`fn!` function and has an ordinary `fn!` signature, but the compiler supplies
its frame and polling behavior. They are not first-class control-flow syntax.
Because `Suspend[T]` is sealed, user code cannot define an equivalent polling
combinator; it composes the standard intrinsics instead. The concrete
signatures and the complete intrinsic set remain standard-library API design.

## Requirement Polymorphism

Higher-order code preserves callback requirements with a row parameter:

```text
fn transform[T, U, r](items: list[T], f: fn(T) -> U $ r) -> list[U] $ r:
    ...
```

A local provider may remove one key from a callback row:

```text
fn provide_logger[r](callback: fn(string) -> void $ r) -> void $ (r - Logger):
    $.with(Logger=logger):
        callback("message")
```

The compiler infers `r` as a row parameter from its use after `$`.
At a call, it infers the callback's requirement row for `r`. The
callee's own row is then normalized after union and subtraction. This mechanism
does not quantify over arbitrary type-level expressions; it is specific to
requirement rows.

## Runtime Boundary

A stored suspension is driven through a runtime or standard-library driver; no
additional source keyword is required. Drivers enforce exclusive access and the
panic rules above without exposing a way to upgrade an arbitrary readonly
reference. Provider selection is never implicit: a provider comes from an
enclosing `$.with` scope or from the host configuration of an entry point.

Scheduling APIs, durable replay, and affine resource ownership are runtime or
library concerns. Cancellation participates in synchronous `defer` cleanup but
does not replace an ownership or resource-lifetime design.
