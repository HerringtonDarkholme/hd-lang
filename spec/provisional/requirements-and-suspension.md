# Requirements and Suspension

Status: provisional design.

This chapter specifies the current design for static dependency requirements,
provider injection, and one-shot suspension. The mechanisms are related but
deliberately separate. This chapter is provisional because row polymorphism,
driver APIs, and some cancellation details remain open.

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

A function signature may end with `$` and an unordered row of requirement
traits:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database + Cache:
    ...
```

```ebnf
requirement_clause = "$", requirement_row ;
requirement_row = requirement_key, { "+", requirement_key } ;
requirement_key = qualified_name ;
```

This chapter extends function declarations and function types:

```ebnf
required_function_decl = "fn", suspendable_name, [ generic_params ],
                         parameter_clause, "->", type,
                         [ requirement_clause ], ":", suite_body ;

required_function_type = [ "mut" ], "fn", "(", [ type_list ], ")",
                         "->", type, [ requirement_clause ] ;

suspendable_name = identifier, [ "!" ] ;
```

The same `suspendable_name` and optional `requirement_clause` extend trait
method declarations and functions inside `impl` blocks. A trait requirement and
its implementation must agree on suspension and requirement-row behavior.

Rows are sets: order does not affect type identity, and a key occurs at most
once after normalization. A function may call another required function only
when its own row includes those requirements or a lexical provider scope
satisfies them.

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
context_type = "$", ".", "Context", "[", requirement_row, "]" ;
context_scope = "$", ".", "with", "(", context_entries, ")",
                ":", suite_body ;

context_entries = context_entry, { ",", context_entry }, [ "," ] ;
context_entry = requirement_key, "=", expression
              | "...", expression
              ;
```

`context_use` and `context_create` extend `primary_expression`; `context_type`
extends `non_optional_type`; and `context_scope` extends the control/trailing
block expression alternatives in the core grammar.

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

```ebnf
suspension_call_suffix = "!", argument_clause ;
```

This suffix extends `postfix_suffix` at ordinary call precedence. A bang call
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

Competing drivers, reentrant polling, polling after `Ready`, polling after
cancellation, or attempting a second execution cause a runtime panic. These are
runtime checks rather than ownership rules in the type system.

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

Higher-order code must preserve callback requirements. The current candidate
uses a requirement-row variable:

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

The exact row-variable kind, inference, normalization, and subtraction syntax
are not settled. These examples express the required relationship but are not
yet normative grammar.

## Open Issues

1. Stored-suspension driver APIs and syntax outside another `fn!` body.
2. The exact guarded mutable-access mechanism used by runtime drivers.
3. Whether cancellation is idempotent or repeated cancellation panics.
4. Provider defaulting and selection rules across package and host boundaries.
5. Full requirement-row polymorphism and subtraction.
6. Cleanup and resource lifetime; cancellation does not replace them.
