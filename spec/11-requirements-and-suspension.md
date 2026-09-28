# Requirements and Suspension

Status: language specification draft.

This chapter defines static dependency requirements, provider injection, and
one-shot suspension. The mechanisms are related but deliberately separate.
The grammar and semantics in this chapter are part of hd-lang; additional
runtime and library policies are tracked as future work.

## Decomposed Model

hd-lang decomposes concerns often grouped under algebraic effects into three
mechanisms:

| Mechanism | What it does |
| --- | --- |
| One-shot suspension | `fn!`, `Suspend[T]`, bang calls, polling, and cancellation describe a resumable computation. |
| Requirement checking | `$` rows let the compiler prove that every dependency used by a function is available. |
| Dependency injection | Contexts bind requirement keys to ordinary values that implement those requirement traits. |

1. r[req.model.one-shot] Suspension is not a general or multi-shot continuation system.
2. r[req.model.not-capability] A requirement is not necessarily a host capability.
3. r[req.model.errors] Normal errors are values represented by `Result[T, E]`.
4. r[req.model.lookup-no-suspend] Dependency lookup does not suspend.
5. r[req.model.suspend-no-errors] Suspension does not raise ordinary errors.
6. r[req.model.no-provider-choice] Declaring a requirement does not by itself choose a provider.
7. r[req.model.runtimes] Production, test, sandbox, and replay runtimes may bind different providers or drivers at explicit requirement and suspension boundaries.
8. r[req.model.no-reinterpretation] Code outside those boundaries is not generally reinterpreted.

## Requirement Rows

This section defines requirement rows, the `$` clause that writes them, and
how the compiler checks them.

1. r[req.row.definition] A **requirement row** is the normalized unordered set of requirement keys on a callable signature.
2. r[req.row.parameter] A **row parameter** is a generic parameter whose values are requirement rows.

These are the only terms used below for the concrete and generic forms.

### Row Syntax

A function signature may end with `$` and a list of requirement keys:

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
requirement_key = trait_type ;
```

1. r[req.row.syntax.signature] A function signature may end with `$` and an unordered list of requirement traits.
2. r[req.row.syntax.comma-list] Each key names a separate injected value, so several keys form a comma list.
3. r[req.row.syntax.single-bare] A single key may be bare, as in `$ Console`.
4. r[req.row.syntax.header-bare] A declaration or closure header may list several keys bare, because its clause ends at the header's `:`.
5. r[req.row.syntax.type-parenthesized] Inside a type, several keys are parenthesized, as in `fn(UserId) -> User $(Database, Cache)` or `Map[string, fn() -> i32 $(Clock, Log)]`.
6. r[req.row.syntax.empty] The clause `$()` writes the empty row explicitly.
7. r[req.row.syntax.no-mut] A key has no `mut` prefix; a trait's methods decide the provider's access, as [Mutable Providers](#mutable-providers) describes.
8. r[req.row.syntax.no-operators] A row has no operators: a `+` or `-` between keys is an error. Error: `old-row-operator`.

```text
trait Clock

trait Logger

fn both() -> void $ Clock + Logger: pass  # error: old-row-operator
```

See also: [Mutable Providers](#mutable-providers),
[Types](02-grammar.md#types).

### Callable Forms

Function declarations, closure expressions, and function types use the same
requirement clause:

```ebnf
function_decl = "fn", callable_name, [ generic_params ], parameter_clause,
                [ "->", type ], [ header_requirement_clause ], ":",
                suite_body ;

function_type = "fn", [ "!" ], "(", [ type_list ], ")",
                "->", type, [ requirement_clause ] ;

closure_expression = "fn", [ "!" ], closure_parameter_clause,
                     [ "->", type ], [ header_requirement_clause ],
                     ":", suite_body ;

callable_name = identifier, [ "!" ] ;
```

1. r[req.row.callable.same-clause] Function declarations, closure expressions, and function types use the same requirement clause.
2. r[req.row.callable.methods] The same callable name and optional requirement clause apply to trait methods and functions inside `impl` blocks.
3. r[req.row.callable.impl-agrees] A trait requirement and its implementation must agree on suspension and normalized requirement row behavior.

### Omitted Requirement Clauses

1. r[req.row.omitted.empty] A public function, a trait method, or a method of a trait implementation without a requirement clause has the empty row.
2. r[req.row.omitted.empty.body] The body of such a callable may use only requirements satisfied by an enclosing lexical provider scope.
3. r[req.row.omitted.inferred] A non-public function, inherent method, or local `fn` declaration without a requirement clause has an inferred row, computed by the same rule as a closure's below.
4. r[req.row.omitted.cycle] Inferred rows of functions that call each other in a cycle are the least rows that satisfy every member of the cycle.
5. r[req.row.omitted.closure] When a closure omits its requirement clause, the compiler infers the least row containing every requirement used by its body that is not satisfied by an enclosing lexical provider scope.
6. r[req.row.omitted.parameter-calls] Calls through function parameters contribute their normalized rows.
7. r[req.row.omitted.expected-parameter] If an expected function type contains a row parameter, the inferred concrete row is unified with that parameter.
8. r[req.row.omitted.not-empty] Omission never means an empty row merely because the expected row is generic.
9. r[req.row.omitted.written] A written `$` clause is explicit and must entail the same body requirements.

```text
trait Logger:
    fn write(self, message: string) -> void

fn log(message: string) -> void $ Logger:
    logger := $.use(Logger)
    logger.write(message)

trait Job:
    fn run(self) -> void

data Nightly: pass

impl Job for Nightly:
    fn run(self) -> void:
        log("nightly")  # error
```

### Row Sets

1. r[req.row.set.order] Rows are sets: order does not affect type identity.
2. r[req.row.set.once] A key occurs at most once after normalization.
3. r[req.row.set.union] The row denoted by a list is the union of its keys.
4. r[req.row.set.parameter] A row parameter listed beside other keys contributes every key of its row.
5. r[req.row.set.duplicate] `$(Logger, Clock, Logger)` is the row `$(Clock, Logger)`.
6. r[req.row.set.call] A function may call another required function only when its own row includes those requirements or a lexical provider scope satisfies them.

```text
trait Database

fn query() -> void $ Database:
    pass

pub fn invalid() -> void:
    query()  # error
```

### Entailment

1. r[req.row.entail.sets] Requirement checking uses set entailment after alias expansion.
2. r[req.row.entail.available] For a body, `available` is the union of the declared row and the lexical keys.
3. r[req.row.entail.member] Every required key must be a member of `available`.
4. r[req.row.entail.membership] Rows contain only unions, so entailment reduces to membership.
5. r[req.row.entail.concrete] A concrete key `K` is entailed by a row exactly when the row lists `K`.
6. r[req.row.entail.concrete.parameter] An unknown row parameter listed in the row never entails `K`.
7. r[req.row.entail.parameter] An unknown row parameter `R` is entailed by a row exactly when that row lists `R` itself.
8. r[req.row.entail.parameter.concrete] No set of concrete keys entails `R`.
9. r[req.row.entail.generic] A generic key such as `Repo[T]` is entailed only by a key that is identical to it after alias expansion.

### Least Row Solutions

1. r[req.row.least.solution] Inference for a parameter pattern that lists a row parameter beside concrete keys chooses the least row solution.
2. r[req.row.least.examples] Matching `$(R, K)` against the row `$(K)` infers the empty row for `R`, and matching it against `$(K, Clock)` infers `$(Clock)`.
3. r[req.row.least.no-solution] When the matched row lacks `K`, the pattern has no solution, and the argument is an error. Error: `type-mismatch`.
4. r[req.row.least.removal] This least-solution rule is also how a callee removes a key from a callback row.

See also: [Requirement Polymorphism](#requirement-polymorphism).

### Row Parameters

1. r[req.row.param.inferred] A generic parameter used in requirement position is inferred to be a row parameter.
2. r[req.row.param.one-kind] One parameter cannot be used as both an ordinary type and a requirement row.

### Requirement Keys

1. r[req.key.traits] Requirement keys are traits, including interfaces such as `Database` and host capabilities such as `Clock` or `Network`.
2. r[req.key.no-effect-syntax] The language does not introduce a separate effect-declaration syntax.
3. r[req.key.inspectable] A trait that is `std.inspect.Inspectable` or has it as a direct or transitive supertrait is never a requirement key.
4. r[req.key.inspectable.error] Writing one as a key, in a requirement clause, a provider scope, or any other place a key is named, is an error reported on the key. Error: `inspectable-requirement`.
5. r[req.key.inspectable.bound] Such a trait remains valid as a bound and as a value type.

```text
use std.inspect.Inspectable

trait Storage < Inspectable:
    fn get(self, key: string) -> string?

fn load() -> void $ Storage:  # error: inspectable-requirement
    pass
```

> **Why.** The rule keeps provider views attenuated: a provider reached
> through a key can never be tested for its concrete type.

See also: [Runtime Type Identity](09-traits.md#runtime-type-identity).

## Provider Access

`$.use` retrieves providers from the statically known current context:

```text
db := $.use(Database)
db, cache := $.use(Database, Cache)
```

1. r[req.use.context] `$.use` retrieves providers from the statically known current context.
2. r[req.use.order] The result order matches the requested key order.
3. r[req.use.missing] A missing provider is a compile-time error at every call site below an entry point.
4. r[req.use.entry-row] Entry-point rows may contain only host capability traits declared by the selected runtime profile.
5. r[req.use.registered-profile-set] For a registered boundary, the registration contract's explicit bindable-trait set is that boundary's profile.
6. r[req.use.host-configuration] Failure to configure one of those host providers is a pre-execution host configuration error.
7. r[req.use.no-dynamic-search] `$.use` is non-suspending and performs no dynamic handler search that can fail at runtime below that boundary.

```text
pub fn main() -> void: _ := $.use(Clock)  # error

trait Clock
```

### Provider Values

1. r[req.use.value.ordinary] A provider value returned by `$.use(K)` is an ordinary value of type `K`.
2. r[req.use.value.access] Its access follows from its trait, as [Mutable Providers](#mutable-providers) describes: `mut K` for a mutable requirement trait, readonly `K` otherwise.
3. r[req.use.value.flow] A provider value may flow anywhere an ordinary value of that type may flow, including fields, collections, closure captures, return values, and suspension frames.
4. r[req.use.value.outlives-scope] A provider value remains usable after its provider scope ends.
5. r[req.use.value.row-meaning] A requirement row therefore records unresolved provider lookup, not every authority a callable can exercise through values it holds.

### Context Namespace

1. r[req.use.namespace] `$` is a special context namespace, not an ordinary value.
2. r[req.use.namespace.keys] Requirement keys in its operations are type-level keys rather than named argument labels.

## Provider Scopes

`$.with` binds providers for one lexical trailing block:

```text
fn demo!() -> Result[User?, DbError] $ Cache:
    $.with(Database=mock_db):
        load_user!(UserId("user_123"))
```

1. r[req.with.block] `$.with` binds providers for one lexical trailing block.
2. r[req.with.evaluated] Each provider expression is evaluated before entering the block.
3. r[req.with.type] Each provider expression must have a type implementing its named requirement trait.
4. r[req.with.nested] Nested scopes may replace an outer provider for the same key within the nested block.

### Generic Key Collisions

1. r[req.with.collision.compared] At each `$.with`, the compiler compares every newly bound key with every other new key and with every declared-row or lexical key visible in the block.
2. r[req.with.collision] Two distinct generic key expressions that can become identical under any valid type-argument substitution are an error. Error: `generic-requirement-key-collision`.
3. r[req.with.collision.context] The same check applies among entries of a `$.context` expression.
4. r[req.with.collision.exact] An exact replacement written with the same key expression remains the ordinary nested-scope override described above.
5. r[req.with.collision.before-erasure] This check is performed before erasure or specialization, so compilation strategy cannot change which provider a lookup selects.
6. r[req.with.collision.concrete] Distinct concrete keys such as `Repo[User]` and `Repo[Post]` remain valid.

For example, a generic body may not make `Repo[T]` and `Repo[U]` concurrently
visible, because an instantiation can choose `T = U`. It likewise may not
combine a declared `Repo[T]` with a lexical `Repo[User]`, because `T` can be
`User`:

```text
trait Repo[T]

data User: pass

fn choose[T, U](first: Repo[T], second: Repo[U]) -> void:
    $.with(Repo[T]=first):
        $.with(Repo[U]=second):  # error: generic-requirement-key-collision
            pass

fn mixed[T](local: Repo[User]) -> void $ Repo[T]:
    $.with(Repo[User]=local):  # error: generic-requirement-key-collision
        pass
```

### Reusable Contexts

Reusable provider maps use `$.Context[Row]`:

```text
fn prod_context() -> $.Context[$(Metrics, Cache)]:
    $.context(Metrics=metrics, Cache=cache)

$.with(Database=db, Logger=logger, prod_context()...):
    ...
```

1. r[req.context.row] `$.Context[$(A, B)]` is indexed by one unordered, duplicate-free requirement row; it is not a variadic generic.
2. r[req.context.single-bare] A context with a single key may write it bare, as in `$.Context[Clock]`.
3. r[req.context.empty] `$.Context[$()]` is the empty context.
4. r[req.context.create] `$.context` creates a context value.
5. r[req.context.spread] An entry `ctx...` spreads the providers of the context value `ctx`.
6. r[req.context.spread.suffix] Like every spread, a context spread is written with a suffix `...`, and a prefix `...ctx` is a syntax error.
7. r[req.context.order] Context spreads and explicit bindings are applied left to right, and the later binding wins when the same key appears more than once.
8. r[req.context.one-per-key] The resulting context still has one provider per key.

```text
trait Tag:
    fn name(self) -> string

fn run(ctx: $.Context[Tag]) -> void:
    $.with(...ctx):  # error
        pass
```

> **Why.** A prefix `...` means copy.

See also: [Data Expressions](05-expressions.md#data-expressions).

### Provider Grammar

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

1. r[req.context.grammar.categories] `context_use` and `context_create` are primary expressions; `context_type` is a reference type; and `context_scope` is a suite expression.
2. r[req.context.ordinary-values] Provider values are ordinary values and use ordinary trait implementations.
3. r[req.context.no-handler] There is no separate `handler` declaration.

## Mutable Providers

A requirement trait with a `mut self` method is always provided with mutable
access, so a provider written in hd can change its own state:

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

fn tick() -> i32 $ Counter:
    $.use(Counter).bump()
    $.use(Counter).count()

fn demo() -> i32:
    let counter: mut MemoryCounter = MemoryCounter { value: 0 }
    $.with(Counter=counter):
        _ := tick()
        tick()   # 2
```

1. r[req.mut.trait] A **mutable requirement trait** is a trait that declares or inherits at least one `mut self` method.
2. r[req.mut.trait-access] A provider's access follows from its trait alone: mutable for a mutable requirement trait, readonly for any other trait.
3. r[req.mut.no-spelling] Requirement syntax never writes `mut`: a row key `mut K`, `$.use(mut K)`, and a binding `mut K=expression` are errors. Error: `syntax-error`.

```text
trait Counter:
    fn bump(mut self) -> void

fn tick() -> void $ mut Counter:  # error: syntax-error
    $.use(Counter).bump()
```

> **Why.** `mut` marks where code may change a value. A requirement is one
> provider shared by its whole call tree, so its trait already says whether
> the provider can change.

> **Note.** Adding a `mut self` method, even one with a default, to a
> published requirement trait that had none is a breaking change: `$.use`
> then yields `mut K`, and installing a readonly value becomes
> `mutable-upgrade`.

> **Note.** Two tasks in one provider scope, such as the children of
> `std.task.all!`, share one mutable provider. Each sees the other's changes
> between its `!` calls, which are the only yield points
> ([Cooperative Scheduling](#cooperative-scheduling)). Code that needs
> isolation installs a provider per task with `$.with`.

### Installing

1. r[req.mut.install-mutable] For a mutable requirement trait `K`, the expression of a binding `K=expression` in `$.with` or `$.context` must have type `mut T` for a type `T` implementing `K`. A readonly expression is an error. Error: `mutable-upgrade`.
2. r[req.mut.install-readonly-trait] For any other trait, the expression may have either access, and the provider is installed with readonly access.

```text
pub fn main() -> void:
    counter := MemoryCounter { value: 0 }
    $.with(Counter=counter):  # error: mutable-upgrade
        _ := $.use(Counter).count()
```

> **Note.** A fresh value such as `MemoryCounter { value: 0 }` has mutable
> access, so it may be installed directly
> ([Bindings And Fresh Values](04-type-system.md#bindings-and-fresh-values)).

### Retrieving

1. r[req.mut.use-mutable] For a mutable requirement trait `K`, `$.use(K)` yields a value of type `mut K`, on which the `mut self` methods of `K` may be called.
2. r[req.mut.use-readonly-trait] For any other trait, `$.use(K)` yields readonly `K`.
3. r[req.mut.use.binding] The ordinary binding rules still apply to the result: a `:=` binding exposes a readonly view.

```text
fn tick() -> void $ Counter:
    counter := $.use(Counter)
    counter.bump()  # error: mutable-receiver-required
```

> **Note.** Code that keeps a mutable provider in a local writes
> `let counter: mut Counter = $.use(Counter)`.

### Access In Rows

1. r[req.mut.row.plain] A row entry is always the plain key `K`, and it requires the provider for `K` with its trait's access.
2. r[req.mut.row.no-access-rules] Rows, `$.Context[Row]` rows, and removal by extension therefore compare and remove keys by trait alone.
3. r[req.mut.row.missing-key] A required key with no available provider is an error, whatever its trait's access. Error: `missing-requirement`.

### Entry-Point Access

1. r[req.mut.entry.trait-access] A runtime profile binds the host provider for a mutable requirement trait with mutable access, and every other host provider with readonly access.
2. r[req.mut.entry.registered-access] A registration contract binds each trait it lists the same way.
3. r[req.mut.entry.no-marking] Neither a runtime profile nor a registration contract marks a trait mutable.

See also: [Wasm Boundary](10-modules.md#wasm-boundary).

### Captured Access

1. r[req.mut.capture-trait] A cold suspension captures each provider with its trait's access.
2. r[req.mut.capture.fixed] [Construction-Time Requirement Binding](#construction-time-requirement-binding) therefore also fixes the access a stored computation later uses.

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

1. r[req.suspend.marker] A suspending declaration places `!` after its name.
2. r[req.suspend.two-forms] A suspending declaration introduces two call forms: the ordinary call and the bang call.
3. r[req.suspend.cold] The ordinary call constructs a cold suspension.
4. r[req.suspend.cold.captures] The ordinary call evaluates and captures arguments and required providers but does not begin executing the body.
5. r[req.suspend.bang] The bang call constructs that suspension and drives it as a child of the current suspending computation.
6. r[req.suspend.bang.point] A bang call is a possible suspension point.

### Suspending Function Types

The surface type of `fetch_user` is `fn!(UserId) -> Result[User, DbError]`.
A suspending function type is definitionally equivalent to an ordinary
constructor function returning a suspension:

```text
fn!(A, B) -> T $ R
fn(A, B) -> mut Suspend[T] $ R
```

1. r[req.suspend.type.equivalent] A suspending function type is definitionally equivalent to an ordinary constructor function returning a suspension.
2. r[req.suspend.type.spellings] The first spelling preserves the source-level bang-call operation; the second is its lowered callable type.
3. r[req.suspend.type.conversion] Assignment or argument checking may convert the first to the second, but not back.
4. r[req.suspend.type.plain-call] Calling either form without `!` constructs the cold mutable `Suspend[T]`.
5. r[req.suspend.type.bang] Only the first form supports `callee!(...)` directly.
6. r[req.suspend.type.not-suspending] A bang call whose callee is neither a suspending function or function value nor a `Suspend[T]` value is an error. Error: `not-suspending`.
7. r[req.suspend.type.readonly-binding] A `:=` binding weakens that fresh result to readonly `Suspend[T]`, which cannot call `poll` or `cancel`.

```text
fn plain() -> i32: 42
fn run!() -> i32: plain!()  # error: not-suspending
```

> **Note.** Store the result with
> `let pending: mut Suspend[T] = callee(...)` when it must be driven later.

### Suspending Closures

Anonymous suspending callables use the same marker after `fn`:

```text
loader := fn!(id: UserId) -> Result[User, DbError] $ Database:
    db := $.use(Database)
    db.load_user!(id)
```

1. r[req.suspend.closure.marker] Anonymous suspending callables use the same marker after `fn`.
2. r[req.suspend.closure.requirements] Requirement-bearing non-suspending closures omit `!` and place `$` after their result type.
3. r[req.suspend.closure.row-inference] Closure inference may infer a requirement row from an expected function type.
4. r[req.suspend.closure.trailing-only] Inference makes a closure suspending only for a trailing block passed for a suspending parameter, whose `fn!` type is the visible marker ([Trailing Callback Blocks](07-functions.md#trailing-callback-blocks)). Every other suspending closure is written `fn!`.

### Bang Calls And Driver Contexts

```ebnf
suspension_call_suffix = "!", argument_clause ;
```

1. r[req.bang.suffix] This suffix is part of `postfix_suffix` at ordinary call precedence.
2. r[req.bang.not-negation] It follows a completed operand, so it never collides with prefix logical `!` at the start of an operand: `!fetch!(id)` is a legal negation of a bang call's `bool` result.
3. r[req.bang.driver-contexts] A bang call is valid only in a **driver context**, which is exactly one of: a suspending function or closure body, or the host executor driving `main!`. A test body is a suspending closure.
4. r[req.bang.top-level] Module top level is not a driver context.
5. r[req.bang.active] A driver is **active** while its executor is evaluating or polling that driver context on the current program-instance call stack.
6. r[req.bang.pending-not-active] A pending invocation retained by the host between polls is unfinished but not active.
7. r[req.bang.test-driven] The test runner drives each test body as a suspension, so a driver is active throughout the test, including calls through non-suspending helpers.
8. r[req.bang.requirements-not-suspending] Merely using requirements does not make a function suspending; a non-suspending function may have a `$` row.

```text
fn work!() -> i32: 42
fn main() -> i32: work!()  # error
```

### Driving A Stored Suspension

1. r[req.drive.bang] For any expression `s` of type `mut Suspend[T]`, `s!()` drives that stored suspension to completion and has type `T`.
2. r[req.drive.once] The expression `s` is evaluated once.
3. r[req.drive.not-method] This is the same postfix bang suffix used for a direct `fn!` call; it is not a method lookup.
4. r[req.drive.block-on] Non-suspending code imports `use std.task.block_on` and calls the standard function `block_on[T](s: mut Suspend[T]) -> T`, which owns the driver loop until the suspension completes or panics.
5. r[req.drive.block-on.forbidden-contexts] `block_on` is forbidden in a default expression, a `defer` suite, or non-entry module initialization; those contexts cannot start suspension work.
6. r[req.drive.block-on.transitive] This ban is transitive through the statically known call graph.
7. r[req.drive.block-on.unprovable] If a call through a function value or dynamic trait method prevents the compiler from proving that `block_on` is unreachable, the call is rejected in one of these contexts.
8. r[req.drive.block-on.error] Every direct or transitive violation is an error. Error: `suspension-forbidden-context`.
9. r[req.drive.block-on.nested] If any suspension driver is already active in the program instance, calling `block_on` causes a panic before polling its argument, and the panic is `suspension-nested-driver`.
10. r[req.drive.block-on.indirect] This includes a call reached indirectly from a suspending body or during cancellation.

```text
use std.task.block_on
fn ready!() -> i32: 42
fn main() -> void:
    let pending: mut Suspend[i32] = ready()
    defer:
        _ := block_on(pending)  # error: suspension-forbidden-context
```

> **Why.** The nested-driver panic prevents nested cooperative drivers from
> blocking one another.

### Entry Driver

1. r[req.entry.driver] The host executor is the driver for `main!`.
2. r[req.entry.waker-driven] This entry driver is waker-driven.
3. r[req.entry.pending] When a poll returns `Pending` because a host provider operation is pending, the driver returns control to the host and polls again only after a waker for that suspension is invoked.
4. r[req.entry.busy-poll] A driver that keeps polling a pending host operation without returning to the host is not conforming.

### Host Waits

1. r[req.host-wait.leaf] The runtime-provided leaf `std.task.host_wait![T](operation: std.task.HostWait[T]) -> T` maps an opaque host wait operation to the WebAssembly Component Model async ABI as used by WASI 0.3 host interfaces.
2. r[req.host-wait.source] User code obtains `HostWait[T]` values only from host providers; the type has no public constructor.

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

1. r[req.protocol.sealed] `Suspend[T]` is sealed.
2. r[req.protocol.implementers] Only compiler-generated frames and implementations in `std.task` may implement it; ordinary packages may consume the trait but cannot declare an implementation.
3. r[req.protocol.no-other] There is no separate `Pollable` or public `Continuation[T]` abstraction.

> **Why.** Sealing makes the one-shot runtime checks part of the protocol
> rather than an unenforceable user convention.

### Suspension Values

| Rule | A `Suspend[T]` value is |
| --- | --- |
| r[req.value.cold] Cold | cold until first polled |
| r[req.value.single] Single execution | single-execution, not a reusable plan or memoized result |
| r[req.value.exclusive] Exclusive | exclusively driven at runtime |
| r[req.value.stateful] Stateful | stateful across normal successive polls while pending |

### Poll Context

Standard-library suspension implementations access the waker of a
`PollContext` through this sealed surface:

```text
data PollContext: pass

impl PollContext:
    fn waker(self) -> Waker

trait Waker:
    fn wake(self) -> void
```

1. r[req.poll-context.construction] `PollContext` is constructed only by the host runtime and `std.task`; user code cannot construct one.
2. r[req.poll-context.waker] Standard-library suspension implementations access its waker through this sealed surface.
3. r[req.poll-context.user] Ordinary user packages may receive a `PollContext` only inside APIs explicitly provided by `std.task`; they cannot use it to implement the sealed `Suspend` trait.

### Wakers

1. r[req.waker.arrange] An operation returning `Pending` must arrange for the waker to be invoked when another poll may make progress.
2. r[req.waker.retained] A waker may be retained after `poll` returns, invoked from a host callback, and invoked more than once.
3. r[req.waker.coalesced] Redundant wakes are coalesced and never poll concurrently.
4. r[req.waker.no-result] The waker carries no result; state remains in the suspension frame.

### Runtime Checks

1. r[req.check.panics] Competing drivers, reentrant polling, polling after `Ready`, or attempting a second execution cause a runtime panic.
2. r[req.check.cancel-active] Cancelling a suspension while it or one of its descendants is active on the current poll stack causes `suspension-reentrant-poll`.
3. r[req.check.cancel-active.no-effect] That cancellation performs no cleanup or state transition.
4. r[req.check.cancel-idempotent] Cancellation is otherwise idempotent: cancelling an already cancelled or completed suspension has no further effect.
5. r[req.check.poll-cancelled] Polling a cancelled suspension causes a runtime panic.
6. r[req.check.runtime-only] These are runtime checks rather than ownership rules in the type system.
7. r[req.check.code.competing] Competing drivers report `suspension-competing-driver`.
8. r[req.check.code.reentrant] Recursive polling and active-stack cancellation report `suspension-reentrant-poll`.
9. r[req.check.code.invalid-state] Polling after completion or cancellation and attempting a second execution report `suspension-invalid-state`.

### Discarding A Suspension

1. r[req.discard.cold] Discarding a cold suspension that has never been polled has no cleanup work to perform.
2. r[req.discard.started] Once polling has begun, a host or driver that stops owning the suspension must cancel it before discarding it.
3. r[req.discard.abandoned] Raw abandonment of a started suspension does not run its registered `defer` suites.

### Cooperative Scheduling

1. r[req.schedule.one-thread] Each program instance executes cooperatively on one thread.
2. r[req.schedule.yield-points] Bang calls are the only language-level yield points; code between them does not interleave with a sibling suspension in that instance.
3. r[req.schedule.all-order] `std.task.all!` polls children in argument order on its initial poll and again in argument order after every wake.

## Compilation Strategy

`fn name!(args) -> T` is source sugar for a compiler-generated cold state
machine implementing `Suspend[T]`.

1. r[req.lowering.sugar] `fn name!(args) -> T` is source sugar for a compiler-generated cold state machine implementing `Suspend[T]`.
2. r[req.lowering.frame] The frame stores captured arguments, construction-time providers, locals live across suspension points, and a state discriminant.
3. r[req.lowering.poll] The frame's `poll` method advances until it returns `Pending` or completes with `Ready(value)`.
4. r[req.lowering.representation] The compiler may use a different representation, but it must preserve cold construction, provider binding, suspension points, and single-execution behavior.

Conceptually, not as normative source code, a suspending function lowers to a
constructor:

```text
fn adjusted(base: i32) -> mut Suspend[i32] $ Counter:
    counter := $.use(Counter)
    AdjustedFrame {
        state: AdjustedState.New(base=base, counter=counter),
    }
```

and to a frame whose `poll` method advances its state:

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

## Construction-Time Requirement Binding

Requirements of a suspending function are resolved when its cold suspension is
constructed:

```text
let pending: mut Suspend[Result[User?, DbError]] = $.with(Database=mock_db):
    load_user(id)

# Driving pending later still uses mock_db.
```

1. r[req.bind.construction] Requirements of a suspending function are resolved when its cold suspension is constructed, not when a later driver first polls it.
2. r[req.bind.captured] The chosen providers are captured by the generated frame.
3. r[req.bind.caller] A caller constructing a required suspension must itself satisfy that requirement even if it does not immediately bang-call the suspension.

```text
trait Tag:
    fn name(self) -> string

fn tagged!() -> string $ Tag:
    $.use(Tag).name()

pub fn rejected_construction() -> void:
    let pending: mut Suspend[string] = tagged()  # error
    pending.cancel()
```

> **Why.** Binding at construction prevents a stored computation from
> silently changing dependencies when it moves between drivers.

## Cancellation

This section defines what `cancel(mut self)` does and how the standard
combinators cancel their children.

1. r[req.cancel.synchronous] `cancel(mut self)` is synchronous and must not suspend.
2. r[req.cancel.steps] It marks the suspension cancelled, synchronously propagates cancellation to an unfinished child, and asks unfinished external operations to abort through their provider/runtime contracts.
3. r[req.cancel.defer] After an unfinished child has completed its own cancellation cleanup, registered `defer` suites in the suspension's unfinished frames run synchronously in last-in, first-out order, from the innermost frame outward.
4. r[req.cancel.cooperative] Cancellation is cooperative at suspension boundaries for language code.
5. r[req.cancel.external-abort] A runtime must not leave a known active HTTP request or equivalent external operation running when its provider supports abort.
6. r[req.cancel.no-ownership] Cancellation runs already registered synchronous `defer` suites, but it does not establish ownership or stop aliases from escaping.
7. r[req.cancel.hook-synchronous] Cleanup that can fail or requires asynchronous work needs a separate design; the cancellation hook itself remains synchronous.

### Standard Combinators

1. r[req.combinator.cancel-children] The `std.task` combinators `all!` and `race!` cancel their children when the parent is cancelled.
2. r[req.combinator.race-losers] `race!` also synchronously cancels losing children before returning the first completed value.
3. r[req.combinator.retry] A retry combinator in `std.task` cancels its active attempt and starts no further attempt after parent cancellation.
4. r[req.combinator.intrinsic] The standard polling combinators in `std.task`, including `all!` and `race!`, are compiler intrinsics.
5. r[req.combinator.ordinary-call] Each is imported and bang-called like an ordinary `fn!` function and has an ordinary `fn!` signature, but the compiler supplies its frame and polling behavior.
6. r[req.combinator.not-syntax] They are not first-class control-flow syntax.
7. r[req.combinator.user] Because `Suspend[T]` is sealed, user code cannot define an equivalent polling combinator; it composes the standard intrinsics instead.
8. r[req.combinator.library] The concrete signatures and the complete intrinsic set remain standard-library API design.

## Requirement Polymorphism

Higher-order code preserves callback requirements with a row parameter:

```text
fn transform[T, U, R](items: List[T], f: fn(T) -> U $ R) -> List[U] $ R:
    ...
```

A local provider removes one key from a callback row by extension:

```text
fn provide_logger[R](callback: fn(string) -> void $(R, Logger)) -> void $ R:
    $.with(Logger=logger):
        callback("message")
```

1. r[req.poly.row-parameter] Higher-order code preserves callback requirements with a row parameter.
2. r[req.poly.extension] A local provider removes one key from a callback row by extension.
3. r[req.poly.extension.form] The parameter row lists the row parameter beside the removed key, and the callee's own row is the plain row parameter.
4. r[req.poly.kind] The compiler infers `R` as a row parameter from its use after `$`, not from the case of its name.
5. r[req.poly.naming] Row parameters follow the ordinary uppercase convention for generic parameters.
6. r[req.poly.least] At a call, the compiler infers `R` as the least row solution of the callback pattern.
7. r[req.poly.least.example] Passing a callback with row `$(Logger, Clock)` infers `R` as `$(Clock)`, so the call requires only `Clock`.
8. r[req.poly.absent] Passing a callback whose row lacks `Logger` is an error. Error: `type-mismatch`.
9. r[req.poly.body] Inside the body, calling `callback` requires `R` and `Logger`; the declared row supplies `R`, and the `$.with` scope supplies `Logger`.
10. r[req.poly.one-body] A row parameter's providers are passed as one bundle, so a row-polymorphic body is compiled once and never specialized per row.
11. r[req.poly.no-subtraction] Rows have no subtraction operator.
12. r[req.poly.rows-only] This mechanism does not quantify over arbitrary type-level expressions; it is specific to requirement rows.

```text
trait Logger:
    fn write(self, message: string) -> void

trait Clock

data SilentLogger: pass

impl Logger for SilentLogger:
    fn write(self, message: string) -> void: pass

fn provide_logger[R](callback: fn() -> void $(R, Logger)) -> void $ R:
    $.with(Logger=SilentLogger {}):
        callback()

fn tick() -> void $ Clock: pass

fn run() -> void $ Clock:
    provide_logger(tick)  # error: type-mismatch
```

> **Note.** Extension in the input and the plain row variable in the output
> is the established form for handling one effect. Koka writes
> `<console, exn | e>`, Unison writes `{g, Exception}`, and Effekt writes
> `/ { Console, Exc }`.

See also: [Least Row Solutions](#least-row-solutions).

## Runtime Boundary

This section defines how a stored suspension reaches a driver, where
providers come from, and what a program instance's behavior depends on.

1. r[req.runtime.driver] A stored suspension is driven through a runtime or standard-library driver; no additional source keyword is required.
2. r[req.runtime.exclusive] Drivers enforce exclusive access and the panic rules above without exposing a way to upgrade an arbitrary readonly reference.
3. r[req.runtime.explicit-provider] Provider selection is never implicit: a provider comes from an enclosing `$.with` scope or from the host configuration of an entry point.
4. r[req.runtime.library] Scheduling APIs, durable replay storage and runners, and affine resource ownership are runtime or library concerns.
5. r[req.runtime.cancel-cleanup] Cancellation participates in synchronous `defer` cleanup but does not replace an ownership or resource-lifetime design.

### Determinism

1. r[req.determinism.inputs] A program instance is deterministic in its inputs.
2. r[req.determinism.depends] Its observable behavior depends only on its code identity, its runtime profile, its entry arguments, and the ordered sequence of host-call results and waker and cancellation deliveries it receives.
3. r[req.determinism.no-other-source] Code between host calls has no other source of nondeterminism.
4. r[req.determinism.replay] A runtime may therefore reproduce an instance by supplying the same inputs in the same order.
5. r[req.determinism.hash-seeded] The standard `Hasher` is seeded from the code identity and the runtime profile, so the hash values it produces are within this guarantee.
6. r[req.determinism.limits] Failures caused by host stack or memory limits are outside this guarantee.
7. r[req.determinism.limits-profile] The host's stack and memory limits are part of the runtime profile.
8. r[req.determinism.weak] Garbage collection timing is not an input: user code cannot observe a weak reference clearing or a finalizer running.

> **Note.** A program that needs hash-flooding defense chooses a keyed
> hasher explicitly; the standard `Hasher` does not provide it.

See also: [Comparison Traits](09-traits.md#comparison-traits),
[Representation And Garbage Collection](08-data-and-enums.md#representation-and-garbage-collection).
