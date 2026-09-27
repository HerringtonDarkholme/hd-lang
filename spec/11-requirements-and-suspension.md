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

A function signature may end with `$` and an unordered list of requirement
traits. Each key names a separate injected value, so several keys form a
comma list:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database, Cache:
    ...
```

```ebnf
requirement_clause = "$", requirement_row ;
header_requirement_clause = requirement_clause
                          | "$", requirement_key, ",", requirement_key,
                            { ",", requirement_key }
                          ;
requirement_row = requirement_key
                | "(", [ requirement_list ], ")"
                ;
requirement_list = requirement_key, { ",", requirement_key }, [ "," ] ;
requirement_key = [ "mut" ], trait_type ;
```

A single key may be bare, as in `$ Console`. A declaration or closure header
may list several keys bare, because its clause ends at the header's `:`.
Inside a type, several keys are parenthesized, as in
`fn(UserId) -> User $(Database, Cache)` or
`Map[string, fn() -> i32 $(Clock, Log)]`. The clause `$()` writes the empty
row explicitly. A key written `mut K` requires mutable access to the
provider for `K`, as in `$(R, mut Logger)`; see
[Mutable Providers](#mutable-providers). A row has no operators; a `+` or
`-` between keys is an `old-row-operator` error
([Types](02-grammar.md#types)).

Function declarations, closure expressions, and function types use the same
requirement clause:

```ebnf
function_decl = "fn", callable_name, [ generic_params ], parameter_clause,
                [ "->", type ], [ header_requirement_clause ], ":",
                suite_body ;

function_type = [ "mut" ], "fn", [ "!" ], "(", [ type_list ], ")",
                "->", type, [ requirement_clause ] ;

closure_expression = [ "mut" ], "fn", [ "!" ], closure_parameter_clause,
                     [ "->", type ], [ header_requirement_clause ],
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
once after normalization. The row denoted by a list is the union of its
keys, and a row parameter listed beside other keys contributes every key of
its row. `$(Logger, Clock, Logger)` is the row `$(Clock, Logger)`. A
function may call another required function only when its own row includes
those requirements or a lexical provider scope satisfies them.

Requirement checking uses set entailment after alias expansion. For a body,
`available` is the union of the declared row and the lexical keys, and every
required key must be a member of `available`. Rows contain only unions, so
entailment reduces to membership:

- a concrete key `K` is entailed by a row exactly when the row lists `K`;
  an unknown row parameter listed in the row never entails `K`;
- an unknown row parameter `R` is entailed by a row exactly when that row
  lists `R` itself; no set of concrete keys entails `R`; and
- a generic key such as `Repo[T]` is entailed only by a key that is
  identical to it after alias expansion.

Inference for a parameter pattern that lists a row parameter beside concrete
keys chooses the least row solution. Matching `$(R, K)` against the row
`$(K)` infers the empty row for `R`, and matching it against `$(K, Clock)`
infers `$(Clock)`. When the matched row lacks `K`, the pattern has no
solution, and the argument is a `type-mismatch`. This least-solution rule is
also how a callee removes a key from a callback row; see
[Requirement Polymorphism](#requirement-polymorphism).

A generic parameter used in requirement position is inferred to be a row
parameter. One parameter cannot be used as both an ordinary type and a
requirement row.

Requirement keys are traits, including interfaces such as `Database` and
host capabilities such as `Clock` or `Network`. The language does not introduce
a separate effect-declaration syntax.

A trait that is `std.inspect.Inspectable` or has it as a direct or transitive
supertrait is never a requirement key. Writing one as a key, in a
requirement clause, a provider scope, or any other place a key is named, is
an `inspectable-requirement` error reported on the key. Such a trait remains
valid as a bound and as a value type. The rule keeps provider views
attenuated: a provider reached through a key can never be tested for its
concrete type ([Runtime Type Identity](09-traits.md#runtime-type-identity)).

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
bindable-trait set, with the access it binds for each trait, is that
boundary's profile. Failure to configure one of
those host providers is a pre-execution host configuration error. `$.use` is
non-suspending and performs no dynamic handler search that can fail at runtime
below that boundary.

A provider value returned by `$.use(K)` is an ordinary readonly value of type
`K`; `$.use(mut K)` returns `mut K` under the rules of
[Mutable Providers](#mutable-providers). It
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
fn prod_context() -> $.Context[$(Metrics, Cache)]:
    $.context(Metrics=metrics, Cache=cache)

$.with(Database=db, Logger=logger, prod_context()...):
    ...
```

`$.Context[$(A, B)]` is indexed by one unordered, duplicate-free requirement
row; it is not a variadic generic. A context with a single key may write it
bare, as in `$.Context[Clock]`, and `$.Context[$()]` is the empty context. `$.context` creates a context value. An entry
`ctx...` spreads the providers of the context value `ctx`; like every spread,
it is written with a suffix `...`, and a prefix `...ctx` is a syntax error,
because a prefix `...` means copy
([Data Expressions](05-expressions.md#data-expressions)). Context
spreads and explicit bindings are applied left to right, and the later binding
wins when the same key appears more than once. The resulting context still has
one provider per key.

```ebnf
context_use = "$", ".", "use", "(", requirement_key,
              { ",", requirement_key }, [ "," ], ")" ;

context_create = "$", ".", "context", "(", context_entries, ")" ;
context_type = "$", ".", "Context", "[",
               ( requirement_key | row_type_argument ), "]" ;
context_scope = "$", ".", "with", "(", context_entries, ")",
                ":", suite_body ;

context_entries = context_entry, { ",", context_entry }, [ "," ] ;
context_entry = requirement_key, "=", expression
              | continued_expression, "..."
              ;
```

`context_use` and `context_create` are primary expressions; `context_type` is a
reference type; and `context_scope` is a suite expression.

Provider values are ordinary values and use ordinary trait implementations.
There is no separate `handler` declaration.

## Mutable Providers

A provider may be installed and retrieved with mutable access, so a provider
written in hd can change its own state through `mut self` methods:

```text
trait Counter:
    fn count(self) -> i32
    fn bump(mut self) -> void

data MemoryCounter:
    value: i32

impl Counter for MemoryCounter:
    fn count(self) -> i32:
        self.value

    fn bump(mut self) -> void:
        self.value = self.value + 1

fn tick() -> i32 $ mut Counter:
    $.use(mut Counter).bump()
    $.use(Counter).count()

fn demo() -> i32:
    let counter: mut MemoryCounter = MemoryCounter { value: 0 }
    $.with(mut Counter=counter):
        _ := tick()
        tick()   # 2
```

The `mut` in a requirement key states the access with which a provider is
installed, required, or retrieved. It is not part of the key's identity:
`mut Counter` and `Counter` name the same key. Duplicate-key normalization,
nested-scope replacement, later-binding-wins for contexts, and the
`generic-requirement-key-collision` check all compare keys without `mut`.

**Installing.** A binding `mut K=expression` in `$.with` or `$.context`
installs a provider with mutable access. Its expression must have type `mut T`
for a type `T` implementing `K`; a readonly expression is a `mutable-upgrade`
error. A binding written `K=expression` installs readonly access, whatever the
access type of the expression. A nested binding for the same key replaces the
outer binding together with its access, so a readonly inner binding hides an
outer mutable one within its block.

**Retrieving.** `$.use(mut K)` yields a value of type `mut K`, on which `mut
self` methods of `K` may be called. `$.use(K)` yields readonly `K` even when
the provider was installed with mutable access. Requesting `mut K` where the
provider in effect for `K` has only readonly access is a `mutable-upgrade`
error. The ordinary binding rules still apply to the result: a `:=` binding
exposes a readonly view, so code that keeps a mutable provider in a local
writes `let counter: mut Counter = $.use(mut Counter)`.

**Rows.** A row entry `mut K` requires mutable access to `K`; an entry `K`
requires either access. A row that would contain both normalizes to `mut K`.
Mutable access available for `K` satisfies both entries; readonly access
satisfies only `K`. When a required `mut K` is available only with readonly
access, whether from the declared row or from a lexical provider scope, the
error is `mutable-upgrade`; when `K` is not available at all it is
`missing-requirement`. An inferred row contains `mut K` when its body retrieves
`mut K` or calls a callable whose row contains `mut K`. The access in a
trait method's row is part of the normalized row that its implementations must
match.

Removal by extension compares access after normalization. A pattern
`$(R, mut K)` removes a `mut K` entry, and the callee must supply `K` with
mutable access. A pattern `$(R, K)` removes a readonly `K` entry. Against a
row containing `mut K`, its least solution keeps `mut K` in `R`, because
`K` and `mut K` together normalize to `mut K`. That key is then not removed.

**Contexts.** A `$.Context[Row]` row may contain `mut` entries. The binding
`mut K=expression` in `$.context` contributes `mut K` to the created context's
row, and spreading that context installs `K` with mutable access.

**Entry points.** A runtime profile binds each host provider with readonly
access unless the profile marks the provider's trait mutable
([Wasm Boundary](10-modules.md#wasm-boundary)); a provider for a trait the
profile marks mutable is bound with mutable access. An entry-point row may
contain `mut K` when the selected runtime profile marks `K` mutable, and a
registered boundary's row may contain `mut K` when its registration contract
binds `K` with mutable access. Any other `mut K` entry in such a row is a
`mutable-upgrade` error. An entry `K` without `mut` accepts either binding and
gives the body readonly access.

A cold suspension captures each provider with the access its body requires, so
[Construction-Time Requirement Binding](#construction-time-requirement-binding)
also fixes the access a stored computation later uses.

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
directly. A bang call whose callee is neither a suspending function or
function value nor a `Suspend[T]` value is a `not-suspending` error. A `:=`
binding weakens that fresh result to readonly `Suspend[T]`;
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

This suffix is part of `postfix_suffix` at ordinary call precedence. It follows
a completed operand, so it never collides with prefix logical `!` at the start
of an operand: `!fetch!(id)` is a legal negation of a bang call's `bool`
result. A bang call is valid only in a **driver context**, which is exactly
one of: a suspending function or closure body, a `test` block, or the host
executor driving `main!`. Module top level is not a driver context. A driver
is **active** while its executor is evaluating or polling that driver
context on the current program-instance call stack. A pending invocation retained by the host between
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
fn transform[T, U, R](items: List[T], f: fn(T) -> U $ R) -> List[U] $ R:
    ...
```

A local provider removes one key from a callback row by extension. The
parameter row lists the row parameter beside the removed key, and the
callee's own row is the plain row parameter:

```text
fn provide_logger[R](callback: fn(string) -> void $(R, Logger)) -> void $ R:
    $.with(Logger=logger):
        callback("message")
```

The compiler infers `R` as a row parameter from its use after `$`, not from
the case of its name; row parameters follow the ordinary uppercase convention
for generic parameters.
At a call, it infers `R` as the least row solution of the callback pattern
([Requirement Rows](#requirement-rows)). Passing a callback with row
`$(Logger, Clock)` infers `R` as `$(Clock)`, so the call requires only
`Clock`. Passing a callback whose row lacks `Logger` is a `type-mismatch`.
Inside the body, calling `callback` requires `R` and `Logger`; the declared
row supplies `R`, and the `$.with` scope supplies `Logger`. Rows have no
subtraction operator. This mechanism does not quantify over arbitrary
type-level expressions; it is specific to requirement rows.

> **Note:** Extension in the input and the plain row variable in the output
> is the established form for handling one effect. Koka writes
> `<console, exn | e>`, Unison writes `{g, Exception}`, and Effekt writes
> `/ { Console, Exc }`.

## Runtime Boundary

A stored suspension is driven through a runtime or standard-library driver; no
additional source keyword is required. Drivers enforce exclusive access and the
panic rules above without exposing a way to upgrade an arbitrary readonly
reference. Provider selection is never implicit: a provider comes from an
enclosing `$.with` scope or from the host configuration of an entry point.

A program instance is deterministic in its inputs. Its observable behavior
depends only on its code identity, its runtime profile, its entry arguments,
and the ordered sequence of host-call results and waker and cancellation
deliveries it receives. Code between host calls has no other source of
nondeterminism; a runtime may therefore reproduce an instance by supplying the
same inputs in the same order. Two things are outside this guarantee: hash
values other than those of a `TypeId`, which are not guaranteed stable across
processes
([Comparison Traits](09-traits.md#comparison-traits)), and failures caused by
host stack or memory limits.

Scheduling APIs, durable replay storage and runners, and affine resource
ownership are runtime or library concerns. Cancellation participates in synchronous `defer` cleanup but
does not replace an ownership or resource-lifetime design.
