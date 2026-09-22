# Requirements and Suspension

Status: language specification draft.

This chapter specifies static dependency requirements, provider injection, and
one-shot suspension. The mechanisms are related but deliberately separate.
The grammar and semantics in this chapter are part of hd-lang; runtime and
library policies are specified separately.

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

A function signature may end with `$` and an unordered row expression of
requirement traits:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database + Cache:
    ...
```

```ebnf
requirement_clause = "$", requirement_expression ;
requirement_expression = requirement_union,
                         { "-", requirement_key } ;
requirement_union = requirement_term, { "+", requirement_term } ;
requirement_term = requirement_key
                 | "(", requirement_expression, ")"
                 ;
requirement_key = trait_type ;
```

Function declarations, closure expressions, and function types use the same
requirement clause:

```ebnf
function_decl = "fn", callable_name, [ generic_params ], parameter_clause,
                "->", type, [ requirement_clause ], ":", suite_body ;

function_type = [ "mut" ], "fn", [ "!" ], "(", [ type_list ], ")",
                "->", type, [ requirement_clause ] ;

closure_expression = [ "mut" ], "fn", [ "!" ], closure_parameter_clause,
                     [ "->", type ], [ requirement_clause ],
                     ":", suite_body ;

callable_name = identifier, [ "!" ] ;
```

The same callable name and optional requirement clause apply to trait methods
and functions inside `impl` blocks. A trait requirement and its implementation
must agree on suspension and normalized requirement-row behavior.

Rows are sets: order does not affect type identity, and a key occurs at most
once after normalization. `+` forms set union. `r - Logger` removes `Logger`
from a row parameter `r`; removing an absent key is allowed and leaves the row
unchanged. Parentheses group row expressions. A function may call another
required function only when its own row includes those requirements or a
lexical provider scope satisfies them.

A generic parameter used in requirement position is inferred to have the
requirement-row kind. One parameter cannot be used as both an ordinary type and
a requirement row.

Requirements are usually traits, including interfaces such as `Database` and
host capabilities such as `Clock` or `Network`. The language does not introduce
a separate effect-declaration syntax.

## Provider Access

`$.use` retrieves providers from the statically known current context:

```text
db := $.use(Database)
db, cache := $.use(Database, Cache)
```

The result order matches the requested key order. A missing provider is a
compile-time error. `$.use` is non-suspending and performs no dynamic handler
search that can fail at runtime.

`$` is a special context namespace, not an ordinary value. Requirement keys in
its operations are type-level keys rather than named argument labels.

## Provider Scopes

`$.with` binds providers for one lexical trailing block:

```text
$.with(Database=mock_db, Cache=memory_cache):
    result := load_user!(UserId("user_123"))
```

Each provider expression is evaluated before entering the block and must have a
type implementing its named requirement trait. Nested scopes may replace an
outer provider for the same key within the nested block.

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
pending := fetch_user(id)   # Suspend[Result[User, DbError]]
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
fn(A, B) -> Suspend[T] $ R
```

The first spelling preserves the source-level bang-call operation; the second
is its lowered callable type. Assignment or argument checking may convert the
first to the second, but not back. Calling either form without `!` constructs
the cold `Suspend[T]`. Only the first form supports `callee!(...)` directly.

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
is valid only in a suspending function body or another explicitly defined
driver context. Merely using requirements does not make a function suspending;
a non-suspending function may have a `$` row.

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

A `Suspend[T]` value is:

- cold until first polled;
- single-execution, not a reusable plan or memoized result;
- exclusively driven at runtime;
- stateful across normal successive polls while pending.

`PollContext` contains at least a waker. An operation returning `Pending` must
arrange for the waker to be invoked when another poll may make progress. The
waker carries no result; state remains in the suspension frame.

Competing drivers, reentrant polling, polling after `Ready`, or attempting a
second execution cause a runtime panic. Cancellation is idempotent: cancelling
an already cancelled or completed suspension has no further effect. Polling a
cancelled suspension causes a runtime panic. These are runtime checks rather
than ownership rules in the type system.

There is no separate `Pollable` or public `Continuation[T]` abstraction.

## Compilation Strategy

`fn name!(args) -> T` is source sugar for a compiler-generated cold state
machine implementing `Suspend[T]`. Conceptually, not as normative source code:

```text
fn adjusted(base: i32) -> Suspend[i32] $ Counter:
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
                    child := counter.next()
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
pending := $.with(Database=mock_db):
    load_user(id)

# Driving pending later still uses mock_db.
```

This prevents a stored computation from silently changing dependencies when it
moves between runtime contexts. A caller constructing a required suspension
must itself satisfy that requirement even if it does not immediately bang-call
the suspension.

## Cancellation

`cancel(mut self)` is synchronous and must not suspend. It marks the suspension
cancelled, synchronously propagates cancellation to an active child, and asks
active external operations to abort through their provider/runtime contracts.

Cancellation is cooperative at suspension boundaries for language code, but a
runtime must not leave a known active HTTP request or equivalent external
operation running when its provider supports abort.

Source-level cleanup and resource lifetime are not solved by cancellation.
Cleanup that can fail or requires asynchronous work needs a separate design;
the current cancellation hook itself remains synchronous.

The library combinators `all!` and `race!` cancel their children when the parent
is cancelled. `race!` also synchronously cancels losing children before
returning the first completed value. A retry combinator cancels its active
attempt and starts no further attempt after parent cancellation.

These combinators are standard-library functions, not first-class control-flow
syntax.

## Requirement Polymorphism

Higher-order code preserves callback requirements with a requirement-row
variable:

```text
fn map[T, U, r](items: list[T], f: fn(T) -> U $ r) -> list[U] $ r:
    ...
```

A local provider may remove one key from a callback row:

```text
fn provide_logger[r](callback: fn(string) -> void $ r) -> void $ (r - Logger):
    $.with(Logger=logger):
        callback("message")
```

The compiler infers `r` as a requirement-row parameter from its use after `$`.
At a call, it infers the callback's normalized requirement set for `r`. The
callee's own row is then normalized after union and subtraction. This mechanism
does not quantify over arbitrary type-level expressions; it is specific to
requirement rows.

## Runtime Boundary

A stored suspension is driven through a runtime or standard-library driver; no
additional source keyword is required. Drivers enforce exclusive access and the
panic rules above without exposing a way to upgrade an arbitrary const
reference. Provider selection is never implicit: a provider comes from an
enclosing `$.with` scope or from the host configuration of an entry point.

Scheduling APIs, durable replay, and source-level resource cleanup are runtime
or library concerns. Cancellation does not replace deterministic cleanup.
