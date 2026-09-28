# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end.

## Language Design Decisions

### Replay Determinism And Durable Workflows

**Decided.** Every question in
[Durable Replay](DURABLE_REPLAY.md#owner-decisions) is decided (decisions 1
to 15). The runtime rules are in
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules), and the determinism
clause is in
[Runtime Boundary](../spec/11-requirements-and-suspension.md#runtime-boundary).

**Status.** The replay experiments in the
[Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) predate the
decided rules: their identity is per function rather than per program, their
site IDs contain byte offsets, and they stop at the end of a history instead
of resuming.

### Mutable Host Providers

**Decided and applied 2026-09-27: access follows from the trait.** A
requirement is one provider shared by its whole call tree, so no row or call
tracks its access with `mut`. A requirement trait that declares or inherits a
`mut self` method is a mutable requirement trait: `$.use(K)` yields `mut K`,
a `$.with` or `$.context` binding for it needs a `mut` value
(`mutable-upgrade` otherwise), and a runtime profile binds its host provider
mutable. Every other provider is readonly. `$ mut K`, `$.use(mut K)`, and
`mut K=value` are `syntax-error`, and profiles no longer mark traits. The
rules are in
[Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers).
This supersedes the `$ mut K` rows of [STDLIB decision 14](STDLIB.md#owner-decisions)
and the same-day "always mutable" answer.

**Decided and applied 2026-09-27: `Console.write_line!` takes `mut self`**
(option 2 of the former question). `Console` is therefore a mutable
requirement trait, and a recording [`BufferConsole`](STDLIB.md#stdconsole)
appends through it. `println` and its `$ Console` row are unchanged. Code
that keeps the console in a local writes `let console: mut Console`, since a
`:=` binding stays readonly; no new rule was added. The rule is
[`module.console.write-line-mut`](../spec/10-modules.md#r-module.console.write-line-mut).
The other options were keeping `self`, so a recording console was not
expressible, and letting a `:=` binding of `$.use(K)` keep mutable access.

```text
fn greet!() -> void $ Console:
    let console: mut Console = $.use(Console)
    _ := console.write_line!("hi")
```

**Decided and applied 2026-09-27: two tasks may share one mutable
provider,** with no new rule. Children of `std.task.all!` in one provider
scope retrieve the same `mut K`, and each sees the others' changes between
its `!` calls; code that needs isolation installs a provider per task. The
spec states this as a note in
[Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers),
beside a second note: adding a `mut self` method to a published requirement
trait is a breaking change
([Packages](PACKAGES.md#33-the-checked-compatibility-rule)).

**Raised by the prototype pass (2026-09-28), now decided below.** The prototype checks
`Console` as a prelude trait. It runs direct `write_line!` calls on the host
console and on a program-defined provider, such as the toy
`std.console.BufferConsole`. `println` still writes only through the host
console: when it reaches a program-defined provider, the run stops with
`unsupported-console-provider`, because the spec leaves two questions open.

1. **How `println` calls `write_line!`.** `println` is non-suspending
   (`-> void $ Console`), but `write_line!` suspends, so a recording
   `BufferConsole` has no specified way to receive the line. Options: drive
   the call to completion inside `println`, as `block_on` does, with a
   pending host suspension a panic; or make `println` suspending
   (`println!`). **Recommendation:** drive it to completion, since Rust's
   `println!` and Go's `fmt.Println` are synchronous and every caller keeps
   its signature.
2. **What `println` does with `.Err(ConsoleError)`.** The result is dropped
   today. Options: ignore it, or panic. **Recommendation:** panic, as Rust's
   `println!` does when writing to stdout fails; code that must handle the
   error calls `write_line!` itself.

```text
fn greet() -> void $ Console:
    println("hello")   # which write_line! runs, and what if it fails?
```

**Decided and applied (owner, 2026-09-28).** Both recommendations are accepted.
1. `println` drives `write_line!` to completion inside itself, as
   `block_on` does, and stays non-suspending. A pending host suspension
   there is a panic.
2. `println` panics when `write_line!` returns `.Err(ConsoleError)`. Code
   that must handle the error calls `write_line!` directly.

**Decided follow-ups (owner, 2026-09-28).**
1. `println` is an ordinary std prelude function. Its panics are ordinary
   panics raised by std code, with a message std defines. The language adds
   no panic category for them.
2. `ConsoleError`'s variants and constructor are settled together with the
   other std error types.
3. `println` inherits all of `block_on`'s rules. That includes the panic
   when another driver is already running, and the ban in `defer` suites
   and default expressions. So `println` inside `main!` or a test body
   (both run under a driver) panics. Suspending code writes with
   `$.use(Console).write_line!` instead. The owner confirmed this
   consequence.

The rules are
[`module.console.println-write`](../spec/10-modules.md#r-module.console.println-write)
through
[`module.console.println-error`](../spec/10-modules.md#r-module.console.println-error).
The prototype runs `println` through a program-defined provider such as
`BufferConsole`. It stops a pending or failed call with the prototype code
`unsupported-println-panic`, because the category is open (question 1
below).

**Still open after applying MHP-1 (2026-09-28).** None of these is decided:

| Question | Effect | **Recommendation** |
| --- | --- | --- |
| 1. The panic category | [`flow.panic.category-set`](../spec/06-control-flow.md#r-flow.panic.category-set) is closed, and neither `println` panic has a category, so no `# panic:` fixture or `expect_panic` test can name one. | One new category per cause, such as `console-write-failed` and `console-write-pending`, so a test can tell them apart. The other answer is `explicit-panic` for both, as if `println` called `panic`. |
| 2. Constructing a `ConsoleError` | [`module.console.error`](../spec/10-modules.md#r-module.console.error) gives `ConsoleError` no variants or constructor, so a program-defined provider cannot return `.Err`, and only a host console reaches the error panic. | Decide it with the standard library's error types ([STDLIB](STDLIB.md#stdconsole)); a recording console for failure tests needs a public constructor. |
| 3. `block_on`'s restrictions | "As `block_on` does" could carry over the [nested-driver panic](../spec/11-requirements-and-suspension.md#r-req.drive.block-on.nested) and the ban in `defer` suites and default expressions. The spec applies neither: `println` inside `main!` or a test, where a driver is active, still prints. | Confirm neither applies. The nested-driver panic would break every `println` in `main!` and in tests. |

```text
fn report!() -> void $ Console:
    defer:
        println("done")   # valid today; question 3 asks whether it stays valid
    println("working")
```

### Typed Derivation, Tool Adapters, And Secrets

**Decided.** Owner decisions M1-M26 in
[Typed Derivation: Survey And Design Options](TYPED_DERIVATION.md) are
applied in [Typed Derivation](../spec/14-annotations.md#typed-derivation);
M26 replaced the `annotate` block with the
[trait-less derivation block](../spec/14-annotations.md#trait-less-derivation-blocks).
The prototype implements them by lowering; its gaps are rows of
`test/portable/KNOWN_FAILURES.tsv`, mostly `K1` (the shape intrinsics) and
`F-250`, and the questions the prototype pass
raised are in
[Still Open](TYPED_DERIVATION.md#still-open-after-the-prototype-pass). Error derivation is the separate
`@error` intrinsic
([Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions)).

**Open questions.** The spec lists these as
[undecided parts](../spec/14-annotations.md#undecided-parts); each waits
for the owner:

| Question | What is undecided |
| --- | --- |
| Fact check hook (M15) | The form of a fact type's compile-time `check`, and whether it covers cross-member and type-level checks (round 2 R8). |
| Non-escaping handles (M18 R5) | Whether the parked NonEscapable design (TQ-24 to TQ-26) makes handles non-escaping. |
| Plan constants (M21 R3-4) | The declaration and reference syntax of a template's compile-time constant, and the evaluator's limits. |
| Typed shared constants (M21 R3-7) | Typed handles for shared constructor data, beyond `(name, Any)` pairs. |
| `T -> U` mapping (M14) | Whether derivation between two types is in scope. |
| Name clashes (round 2 R13) | How `walk`, `describe`, and `build` interact with trait methods of the same name. |
| Derived bound (round 2 R14) | Whether a derived bound names the trait or the walker's strengthened bound, where they differ. |
| `default()` allocation (round 2 R15) | Whether `h.default()` may allocate for every member type. |
| Composing templates (round 1 P16) | How a wrapper walker forwards to an inner walker's `member`. |
| `Clone`'s module (M24) | Which standard module declares `Clone`; chosen with the standard library ([STDLIB](STDLIB.md#clone)). |
| Derived-function cache (M24) | The cache's API and module; chosen with the standard library ([STDLIB](STDLIB.md#derived-function-cache)). |
| Generic trait-less blocks (M26) | How a trait-less derivation block for a generic type is written, and whether its header may name other type arguments. |
| Several trait-less blocks (M26) | Whether one type may have more than one trait-less block, and in which order their lines apply. |
| Unused facts from a block (M26) | Whether a type-level fact from a trait-less block's `Self` line gets `unused-derivation-fact`, and where. |
| Function targets | Deriving for functions, and what a decorator before a function means ([FN_TYPE](FN_TYPE.md) questions 9 and 10). Chapter 14's facets and annotators are removed (decision 10). |

**Secret values (removed for now).** `Secret[T]` and `Redact` were removed
from the standard-library design as too early
([STDLIB decision 12](STDLIB.md#owner-decisions), 2026-09-26). Revisit them
together with typed derivation. Options already discussed: whether standard
capability traits may take `Secret[T]` parameters so the host receives the
real value without an `expose()` in hd code; whether exported functions may
take `Secret[T]` inputs; and that a secret never encodes or appears in
outputs.

### Serializable Closures And Incremental Computation

**Problem.** Closures have unspecified identity
([FN_TYPE decision 9](FN_TYPE.md#owner-decisions)) and no stable code
identity, serializable capture contract, cache invalidation rule, or
graph-lifetime mechanism. Since `mut fn` was removed, a function type also
does not say whether a callback mutates its captures, so an incremental
computation cannot demand a write-pure callback through its type.

**Options.** (1) Use a content hash for code identity, require a `Durable`
capture bound, and reject captured providers or mutable state. (2) Require
explicit user IDs and an explicit capture record. (3) Keep closures
process-local and expose only named registered computations.

**Recommendation.** Begin with option 3 for a small dependable surface, then
adopt option 1 when durable replay identity is settled. Provide weak references
inside the standard runtime, or explicit disposal, for incremental graph
nodes; user-visible finalizers are ruled out
([Durable Replay decision 12](DURABLE_REPLAY.md#owner-decisions)).

**Unblocks.** Persisted callbacks, safe incremental caches, distributed work,
and bounded graph lifetimes.

**Decided 2026-09-27, not yet applied: option 1 now.** A serializable
closure's code identity is a content hash. Its captures must be
boundary-safe values ([Durable Replay decision 11](DURABLE_REPLAY.md#owner-decisions)
replaces the `Durable` bound), and capturing a provider or mutable state is
rejected. The design still needs a record: the hash input, how a closure
opts in, and graph lifetimes.

### Observability Hooks

**Problem.** There is no task-local carrier for trace context and no
specified point where suspension/provider activity can be instrumented without
rewriting user code.

**Decided.** Observability and replay use separate hooks, and both derive
their IDs from the execution ID and the event index
([Durable Replay decision 14](DURABLE_REPLAY.md#owner-decisions)).

**Options.** (1) Carry task-local storage in `PollContext`, with hooks at
compiler-generated adapters for registered boundaries plus host-boundary
events. (2) Model tracing only as explicit requirement providers. (3) Let
hosts instrument Wasm calls without language-level correlation.

**Recommendation.** Option 1, while keeping exporters and policy behind
ordinary providers. The hook must honor `Secret[T]`/`Redact` once defined.

**Unblocks.** Trace propagation across suspension, workflow event correlation,
structured metrics, and enforceable redaction.

**Decided 2026-09-27, not yet applied: option 1.** The runtime carries trace
context task-locally in the poll context; hooks fire at host-boundary calls
and suspension points; exporters and policy stay ordinary providers. The
redaction clause waits for `Secret[T]`, which is removed for now.

### Access Control And Tenancy Expressibility

**Problem.** Requirement rows show which service is reachable, not the
principal, tenant, delegation, or attenuation under which it is used.

**Direction.** Access control and tenancy are modeled in hd-lang code, such as
requirement traits, provider values, and library types, rather than by
dedicated language features. The concrete library design is deferred.

**Open question.** Whether the current language can express the needed
patterns without new features: an explicit principal requirement, attenuated
provider views such as `db.for_tenant(tenant)`, delegation, and redacted
output. A worked tool example should show authentication, principal lookup,
tenant attenuation, a database call, and redacted output. Any gap it exposes
becomes a separate language issue.

**Unblocks.** Multi-tenant tools, least-privilege review, delegated authority,
and access-control testing.

**Status.** Deferred 2026-09-27 until the core specification settles; the
redacted-output part also waits for `Secret[T]`.

### Scope Reduction And Distinctive Requirements

**Problem.** GADTs, declared variance,
variadic generics, much of the permission system, loop-`else` values, binding
expressions, trailing blocks, and the multiple-closure form carry substantial
implementation and teaching cost without demonstrated product requirements.
Conformance effort is concentrated away from requirement/suspension behavior.

**Options.** (1) Keep the surface and demand motivating end-to-end examples and
proportionate conformance coverage. (2) Move GADTs out, make user generics
invariant, replace packs with homogeneous task lists, simplify `mut`, preserve
mutable freshness for new suspensions, and forbid `return` in trailing blocks.
(3) Define a smaller initial language profile while retaining the designs as
documented experiments.

**Recommendation.** Option 2 unless a concrete requirement justifies each
feature. Select two distinguishing outcomes—recommended: durable replay and
per-tool requirement reports—specify them end to end, mark the rest as initial
release non-goals in `USE_SCENARIOS.md`, and add chapter 11 fixtures before
expanding other chapters.

**Unblocks.** A smaller compiler, clearer teaching material, faster
conformance, and evidence that complexity serves the language thesis.

**Status.** Closed 2026-09-27: every feature stays. The first
[Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) deliberately does
not implement these features, and they are implemented soon after it rather
than removed. The recommendation above relied on per-tool requirement
reports, which the owner dropped.

### Annotation Locality And Inspection

**Status.** Superseded 2026-09-27. Typed derivation removes `Annotation` and
`Annotate` ([TYPED_DERIVATION](TYPED_DERIVATION.md) rule 10), derivation
blocks live in the type's module, a foreign type is derived through a local
mirror type or newtype, and the root-application orphan exception is
dropped. So there are no foreign-target annotations to place or mark, and
TQ-18's marker question is moot. The text below is kept as history.

**Problem.** The effective annotation for a target can be assembled
across distant files and dependencies, making review and provenance difficult
even with global coherence.

**Direction.** An annotation for `X` must be declared in `X`'s defining module,
with an explicit form for foreign targets. The compiler provides a command that
prints the complete materialized `Info` plan and its provenance for a target
without executing effectful code. Until the rule is specified, the existing
package-global placement rules in Annotations (since removed) stay in
force.

**Open questions.**

1. Where a foreign-target annotation may appear. Candidates: the facet's
   defining module, mirroring the trait orphan rule at module granularity, and
   the root application package's existing orphan exception.
2. How a foreign-target annotation is marked. Candidates: no marker when the
   facet's module declares it, and an explicit marker only for the root
   orphan case, or one marker for every foreign target.
3. Whether the locality rule applies equally to an explicit
   `impl Annotate[Facet] for Target`, which occupies the same coherence slot.
   Without that, the rule could be bypassed.

**Unblocks.** Local review, understandable generated schemas, annotation
debugging, and safer dependency upgrades.

### Runtime Type Identity And `reified`

**Status.** The direction is specified in
[Runtime Type Identity](../spec/09-traits.md#runtime-type-identity): the
sealed `std.inspect.Inspectable` with `runtime_type() -> TypeId` and the
default methods `downcast` and `downcast_mut` (bounded by
`T < AnyRef + Inspectable`, no `reified`), `TypeId::of[T]()`,
`downcast_val` for value types, exact matching in which an inner `mut`
counts and the outer `mut` is ignored, generic erasure through
`T < Inspectable`, `inspectable-requirement`, and
`std.error.Error < Display + Inspectable`.
[INSPECTABLE.md](INSPECTABLE.md) keeps the design record and owner decisions
1 to 16.

Two former items are now specified. Static (receiverless) calls resolve
through a bound, as in `T::create()` with `T < Factory`, and never through a
`TypeId` (TQ-9,
[Associated Function Calls](../spec/09-traits.md#associated-function-calls)).
`TypeShape` has `Trait(decl, args)` and `Any` cases (TQ-23,
[Common Shape Representation](../spec/14-annotations.md#common-shape-representation)).

What remains open is the checked-downcast option in
[Typed Derivation](#typed-derivation-tool-adapters-and-secrets).

**Unblocks.** Typed reflection that recovers a concrete value from an erased
one.

### Confirmed Deferred Type Features

**Problem.** Two surfaces remain intentionally unsupported and must be
diagnosed: first-class bound methods, and direct permission weakening combined
with generic variance. Runtime type tests beyond exact-type recovery from
`Inspectable` values stay unsupported
([Runtime Type Identity](../spec/09-traits.md#runtime-type-identity)).

**Direction.** Keep bound methods and weakening with variance deferred, and
design each only with a motivating requirement. Bound methods must settle
receiver capture; weakening with variance must preserve representation. The
spellings `Type::name` (the unbound method function, receiver first) and
`x::name` (the bound method value) are reserved for them and diagnosed today
as `deferred-method-value`
([Unsupported Function Extensions](../spec/07-functions.md#unsupported-function-extensions)).
Negative implementations and additional pack operations are likewise confirmed
future work rather than implicit extensions.

**Unblocks.** Implementer certainty today and a checklist for future proposals.

### Associated Type Bindings Beyond Direct Bounds

**Problem.** An associated type binding such as `I < Supplier[Item = T]`
names only an associated type the bound trait itself declares, and it is
accepted only in generic parameter bounds. Binding a supertrait's associated
type through a subtrait (`I < NamedSupplier[Item = T]`) is rejected, and
supertrait lists, dynamic trait value types, and `impl` headers take no
bindings.

**Options.** (1) Keep the current rule; users add a separate bound on the
supertrait. (2) Let a binding name any associated type reachable through the
supertrait graph, rejecting ambiguous names. (3) Also accept bindings in
supertrait lists, so `trait Names < Supplier[Item = string]` fixes the item
type for every implementation.

**Recommendation.** Option 1 until a library needs option 2; option 3 needs
its own coherence review.

**Unblocks.** Shorter bounds for trait hierarchies with associated types.

### Resource Non-Escape And Cleanup Policy

**Problem.** Block-scoped `defer` provides deterministic synchronous cleanup on
ordinary control-flow exits and cancellation, but it does not stop a handle
alias from escaping into a global, field, closure, or suspension. The language
also has no settled policy for asynchronous or fallible cleanup.

**Options.** (1) Add a compiler-recognized `NonEscapable` locality category,
propagate it through containers and captures, and use `defer` at the cleanup
boundary. General dependent returns would also need provenance rather than only
a binary marker. (2) Add affine/owned handle types with borrow checking. (3)
Add a scoped callback protocol whose handle cannot escape. (4) Keep unrestricted
aliasing and rely on checked `ResourceError.Disposed` results.

**Direction.** Option 1 is chosen, with `defer` as the cleanup mechanism.
A suspension frame may hold a `NonEscapable` value across a suspension point;
the frame is then itself non-escapable (Shape B in
[Ownership and Escape Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md#shape-b-kotlin-style-locality-plus-a-suspension-exception)).
Still open: the propagation rules, dependent-return provenance, whether
provider values can be `NonEscapable`, and asynchronous or fallible cleanup.
Retain checked disposal errors.

**Unblocks.** Leak-resistant files/sockets, safe cancellation, fallible cleanup
design, stronger sandbox guarantees, and possibly complete per-tool authority
reports: provider values are ordinary values that may escape today, and a
`NonEscapable` provider category is the likely way to close that gap.

### Error Entry-Point Follow-Ups

**Superseded 2026-09-27 by [Testing T8](TESTING.md#owner-decisions), and
applied.** Exit codes follow Rust's model: `std.process` declares
`type ExitCode(u8)` and `Termination`, whose `report` gives `main`'s or a
test's exit code
([Exit Status](../spec/10-modules.md#exit-status)). A failing
`Result[void, E]` prints the error chain
([Entry Results](../spec/10-modules.md#entry-results)) and exits with 1; a
program that wants another code returns `ExitCode` or `Result[ExitCode, E]`.
[Error conversion decisions](ERROR_CONVERSION.md#owner-decisions) 13 and 14
still hold; 16 and 17 are superseded by Testing T4 and T8.

The four questions of this entry are closed:

| Question | Earlier answer | Now |
| --- | --- | --- |
| 1: the status type | `StatusCode`, a `u8` wrapper that is never 0 | removed; `ExitCode(u8)` holds any `u8`, and 0 means success |
| 2: `ExitStatus` on an erased `Error` | the static error type decides | removed; no error type picks a code |
| 3: how `StatusCode` keeps out 0 | open | moot: no zero check |
| 4: spelling a constant status | open | `ExitCode(2)` |

```text
use std.process.ExitCode

pub fn main() -> ExitCode:
    ExitCode(2)
```

### Literal Suffixes

**Decided and applied (2026-09-28).** Owner decisions L1-L9: a suffix is a
newtype implementing `std.ops.LiteralSuffix`, imported by `use`, and
`250ms` calls its `from_literal`
([Literal Suffixes](../spec/05-expressions.md#literal-suffixes)). L12, L13
and L15-L17 are applied too: only decimal and float literals take a suffix,
`5else` is `invalid-token`, a suffixed literal has no compile-time rule of
its own, the test `timeout` option takes any `Duration`, and `Duration` is
whole milliseconds with the suffixes `ms s min h`.

**Open.** L11 (`@suffix fn` in place of `LiteralSuffix`) waits for the
[decorator redesign](DECORATORS.md). Nine points, each with a recommendation, are in
[Literal Suffixes](LITERAL_SUFFIXES.md#still-open).

### Operator Traits

**Decided direction (2026-09-28).** hd plans operator traits in `std.ops`
(Rust's model: `Add`, `Sub`, `Mul`, `Div`, `Neg`, comparison already via
`Eq`/`Ord`), so library types such as `Duration` support `5s + 3s` and `-d`.
Today hd has no operator overloading. `std.ops.LiteralSuffix` is the first
member ([Literal Suffixes](LITERAL_SUFFIXES.md#owner-decisions) L5).

**Open.** Which operators, their signatures (same-type or mixed operands,
output types), coherence for primitives, compound assignment, and whether
comparison operators map to the existing comparison traits. The brainstorm
[Operator Traits](OPERATOR_TRAITS.md) compares four trait shapes and asks
eight questions; it recommends Rust's `Add[Rhs]` with an associated `Out`.

### Decorators

**Open (2026-09-28).** A general decorator design: which declarations take
decorators, how a decorator declares its targets and signatures, and what
the compiler must know. Literal Suffixes L11's `@suffix` is on hold until
it is decided. The brainstorm [Decorators](DECORATORS.md) compares five
options and asks fourteen questions; it recommends a `Decorator[Target]`
trait with std target types.

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([Durable Replay decision 12](DURABLE_REPLAY.md#owner-decisions));
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the final `hd.toml` schema and the concrete host binding for capabilities
  such as `Console`. The manifest shape, executable-main selection, caret
  version ranges, the PubGrub-style resolver, and the lockfile are decided in
  [Packages decisions 1-14](PACKAGES.md#owner-decisions) and drafted there;
  the root-application orphan exception for a package with both a library
  and executables (decision 4) is still open;
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, [Durable Replay decision 15](DURABLE_REPLAY.md#owner-decisions));
- property-testing strategies, shrinking, replay artifacts, and correlated or
  stateful generators in `std.testing`;
- final signatures, behavior, and the complete intrinsic set for the
  compiler-intrinsic `std.task` combinators, such as racing, retry, timeout,
  and heterogeneous scheduling;
- the final `std.task` structured-scope API: `Task[T]` is decided as
  structured scopes only, with `scope!`, `start`, and `join!`
  ([STDLIB decision 11](STDLIB.md#owner-decisions)), and must not weaken
  one-shot `Suspend[T]` semantics;
- the complete standard host capability-trait catalog and provider
  configuration format;
- exporter configuration, sampling, storage, and operational privacy policy
  after the observability hook is designed; and
- which generated artifacts—JSON Schema, OpenAPI, MCP, clients, or
  documentation—ship first after typed derivation is resolved.

## Resolution Process

After resolving an item:

1. update the owning specification or design document;
2. remove or narrow the item here;
3. add valid and invalid conformance fixtures where applicable;
4. record the resolution in the owning document's history when applicable; and
5. run `spec/check.sh`.
