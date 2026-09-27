# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end.

## Language Design Decisions

### Replay Determinism And Durable Workflows

**Decided.** Durable replay is a runtime feature with a small specification
and compiler contract; storage, runners, retry, and workflow APIs are library
work. Calls are intercepted at the host boundary only. Recording is opt-in per
run. Runs are pinned to their artifact and reach new code through
continue-as-new. Reaching the end of a history resumes live execution, and
in-flight host calls follow a per-method runtime-profile policy. These rules
are in [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules); the determinism
clause is in
[Runtime Boundary](../spec/11-requirements-and-suspension.md#runtime-boundary).

**Problem.** The remaining questions are listed, with options and
recommendations, in [Durable Replay](DURABLE_REPLAY.md#questions-for-the-owner):

- question 2: which host calls enter the history, and what each recording
  level records;
- question 3: whether the standard `Hasher` is deterministic within one code
  identity and runtime profile;
- question 4: whether code identity covers transitive dependencies and the
  compiler's semantic version;
- question 6: how events are matched to calls;
- question 7: whether a `Durable` bound is needed or boundary-safe types
  suffice;
- question 9: whether weak references or finalizers may exist;
- question 10: whether resource limits are part of configuration identity;
- question 11: whether observability and replay share one hook;
- question 12: how a panic is recorded in a history.

**Unblocks.** Crash recovery, workflow upgrades, deterministic replay tests,
and durable orchestration as a defining use case.

**Status.** The first experiment in the
[Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) records frame-poll
events, per-function source identities, and a provider-configuration identity.
A second experiment intercepts suspending host-provider calls with scalar or
string arguments and scalar, string, or void results; it records a
function-relative site, provider and method key, encoded arguments, readiness,
and optional result, and replay restores the result while bypassing the live
provider. Both experiments predate the decided rules: their identity is per
function rather than per module, their site IDs contain byte offsets, and
they stop at the end of a history instead of resuming.

### Mutable Host Providers

**Decided.** A runtime profile may bind a host provider with `mut` access for
a trait it marks mutable, and an entry-point row may then contain `$ mut K`
for that trait; a `mut K` entry for a trait the profile does not mark mutable
stays `mutable-upgrade` ([STDLIB decision 14](STDLIB.md#owner-decisions)).
The rules are in
[Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)
and [Wasm Boundary](../spec/10-modules.md#wasm-boundary).

**Problem.** Two questions remain:

- which traits each toolchain profile marks mutable. The decision names
  `Clock`, `Random`, `FsWrite`, and `Console`, but the standard capability
  catalog is not fixed, and the conformance profiles mark only `Console`
  ([Runtime Profiles](../spec/conformance/README.md#runtime-profiles));
- whether the prelude `Console.write_line!` takes `mut self`. A recording
  `BufferConsole` needs it to append, but it would put `mut Console` in the
  row of `println` and of every caller.

**Options.** For `write_line!`: (1) keep `self`, so a recording console is
not expressible through `Console`; (2) take `mut self`, so printing code
requires `mut Console`.

**Unblocks.** The `std.time`, `std.random`, and `std.fs` host providers once
their profiles are named, and a recording
[`BufferConsole`](STDLIB.md#stdconsole).

### Typed Derivation, Tool Adapters, And Secrets

**Problem.** Current shapes describe declarations but
cannot generically get fields, construct a target, or produce target-indexed
information. Libraries therefore cannot implement typed serializers or tool
adapters, and the boundary lacks a redaction contract.

**Options.** (1) Open `@derive` to library traits through a typed compiler
protocol. (2) Add typed reflection operations for field access, construction,
and checked downcast. (3) Make annotation `Info` target-indexed so a facet can
produce `Decoder[T]` or `ToolAdapter[F]`. Mock-provider generation can either
add a read-only `TraitShape` to this surface or remain an external code generator
that reads package interface files.

**Recommendation.** Combine options 1 and 3: a narrow derivation protocol with
target-indexed output. Generate boundary adapters in the compiler and include
`Secret[T]`/`Redact` in the contract; avoid general mutable reflection.

**Unblocks.** Serializers, property generators, schema-backed RPC/tool
invocation, validation at the boundary, reliable secret redaction, and
derived `std.convert.From` implementations for error enums
([Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions)). These
are language-design tasks, not merely library work.

**Design notes.** [Typed Derivation: Survey And Design Options](TYPED_DERIVATION.md)
surveys other languages, compares four candidate designs, and lists questions
for the owner. Nothing there is decided.

**Secret values (deferred).** The owner removed `Secret[T]` from the current
standard-library design as too early (2026-09-26). Revisit it together with
typed derivation. Options already discussed: whether standard capability
traits may take `Secret[T]` parameters so the host receives the real value
without an `expose()` in hd code; whether exported functions may take
`Secret[T]` inputs; and that a secret never encodes or appears in outputs.

### Complete Runtime Shape Coverage

**Problem.** `TypeShape` does not yet represent mutable access, dynamic trait
values, `Any`, suspensions, or a newtype's own declaration identity. Generic
annotation and schema walkers therefore cannot describe every legal field type.
The accepted interim rule rejects shape materialization that would require one
of these missing cases.

**Options.** (1) Add `Mut(inner)`, `Trait(decl, args)`, `Any`,
`Suspend(result)`, and `Newtype(decl, base)` cases. (2) Expose a lower-level
opaque type descriptor for unsupported forms. (3) Reject those forms from
shape-driven adapters.

**Recommendation.** Use option 1, keeping declaration identities explicit and
the surface closed and navigable; use option 3 temporarily until the cases are
specified.

**Unblocks.** Complete schema generation, annotation traversal, and reliable
diagnostics for unsupported boundary types.

### Shape Intrinsic Coverage

**Problem.** `shape[T]()` gives typed `fields` and `variants` members, but
three reflection targets have no typed spelling. Parameter shapes are reached
only through `shape_of(f).params`, and variant payload fields only through a
`VariantShape`'s `payload` list, both by position. `shape_of` of a generic
function is unspecified: the old `shape(get_user)` form never said how a
generic function's type parameters are supplied.

**Options.** (1) Add typed `params` and `payload` records the same way as
`fields`, keeping `param_list` and `payload_list` for iteration. (2) Accept
`shape_of(identity[i32])` with a complete explicit type-argument list and
reject a bare generic function name. (3) Reject generic functions in
`shape_of` outright.

**Recommendation.** Option 1 for consistency with `fields`, and option 2 for
generic functions, since it reuses the generic function reference syntax.

**Unblocks.** Statically checked parameter metadata lookups and tool adapters
over generic functions.

### Serializable Closures And Incremental Computation

**Problem.** Closures have per-evaluation identity but no
stable code identity, serializable capture contract, cache invalidation rule,
or graph-lifetime mechanism.

**Options.** (1) Use a content hash for code identity, require a `Durable`
capture bound, and reject captured providers or mutable state. (2) Require
explicit user IDs and an explicit capture record. (3) Keep closures
process-local and expose only named registered computations.

**Recommendation.** Begin with option 3 for a small dependable surface, then
adopt option 1 when durable replay identity is settled. Provide weak references
or explicit disposal for incremental graph nodes; user-visible finalizers
remain a separate question.

**Unblocks.** Persisted callbacks, safe incremental caches, distributed work,
and bounded graph lifetimes.

### Observability Hooks

**Problem.** There is no task-local carrier for trace context and no
specified point where suspension/provider activity can be instrumented without
rewriting user code.

**Options.** (1) Carry task-local storage in `PollContext` and expose one
runtime hook shared with durable replay. Replay now intercepts at the host
boundary only, so a shared hook would sit there, below semantic boundaries
such as registered tools. (2) Model tracing only as explicit requirement
providers. (3) Let hosts instrument Wasm calls without language-level
correlation. Whether observability shares the replay hook is
[Durable Replay](DURABLE_REPLAY.md) question 11.

**Recommendation.** Option 1, while keeping exporters and policy behind
ordinary providers. The hook must honor `Secret[T]`/`Redact` once defined.

**Unblocks.** Trace propagation across suspension, workflow event correlation,
structured metrics, and enforceable redaction.

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

**Status.** Still open. The first
[Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) deliberately does
not implement these features, but they are planned for implementation soon
after it rather than treated as removed.

### Annotation Locality And Inspection

**Problem.** The effective annotation for a target can be assembled
across distant files and dependencies, making review and provenance difficult
even with global coherence.

**Direction.** An annotation for `X` must be declared in `X`'s defining module,
with an explicit form for foreign targets. The compiler provides a command that
prints the complete materialized `Info` plan and its provenance for a target
without executing effectful code. Until the rule is specified, the existing
package-global placement rules in
[Annotations](../spec/14-annotations.md#coherence-and-package-rules) stay in
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

**Problem.** `Any` and dynamic trait values erase the concrete type, and the
only runtime type test is the sealed `ShapeMetadata.metadata[reified M]()`.
Error-chain inspection, plugin registries, and typed extension maps all need
to recover a concrete type from an erased value.

**Direction.**

1. The target of a type test needs a runtime descriptor, and that descriptor
   comes from `reified`.
2. Runtime identity comes from a new trait, `Inspectable`, whose method
   returns the value's runtime type object. Most types implement it
   automatically, and the compiler supplies the implementation as an
   intrinsic, as Rust does for `TypeId`. User code cannot write or override
   it, so a value cannot claim another type's identity.
3. Opting in happens at the use site. Erasing a value to `Any` stays one-way;
   a value that should be recoverable is erased to `Inspectable`, or to a
   trait that extends it. The separate trait makes runtime inspection a
   visible, deliberate choice and discourages casual use.
4. A generic type's runtime type object contains its type arguments, so
   `Box[User]` and `Box[Post]` are distinguishable.
5. A type test is the generic free function
   `std.inspect.downcast[T](value) -> T?` (owner decision, superseding the
   earlier method spelling `value.downcast[T]()`), with `T` reified. It works
   on a dynamic `Inspectable` value and on a parameter bounded by
   `T: Inspectable`. `downcast` is not a trait method: like Rust's
   `downcast_ref` on `dyn Any`, it is defined outside `Inspectable` on top of
   the trait's non-generic runtime-type method. The dynamic-safety rule is
   unchanged, and trait methods on trait values still take no generic
   parameters.
6. An erased, unbounded type parameter never supports a type test, so a bare
   `fn f[T](x: T)` cannot branch on `T`.
7. Go-style trait-to-trait assertions are ruled out. Testing whether a value
   also implements another trait could recover authority that an attenuated
   provider view deliberately hides, such as `FileWrite` behind a `FileRead`
   view.
8. A downcast preserves permission (`mut` to `mut`, never readonly to `mut`),
   and its target type must be nameable at the call site, so a private type
   cannot be recovered outside its module.

9. Closures, function values, and suspension frames do not implement
   `Inspectable`, because they may carry requirements, and a recovered
   callable could not be called soundly. Local declarations and
   `NonEscapable` values do not implement it either.
10. The runtime type object supports equality and a printable name. It never
    answers whether a type implements a trait, which would reintroduce the
    ruled-out assertions.
11. Erasing a value to `Inspectable` needs its full type, including generic
    arguments, at the erasure site. Erasing an erased parameter `x: T`
    therefore needs the descriptor from a `T: Inspectable` dictionary or from
    `reified T`. Whether generic objects also store their arguments per object
    is an implementation choice.
12. The standard error trait extends `Inspectable`, so error chains are
    inspectable. The specification already declares `std.error.Error` with
    every member defaulted ([Error Trait](../spec/09-traits.md#error-trait)),
    so adding the compiler-provided `Inspectable` supertrait breaks no
    implementation. `downcast` and a chain search (`find[T]`) on errors wait
    on this issue ([Error Conversion, Still To Do](ERROR_CONVERSION.md#still-to-do));
    conversion at `?` is specified in
    [Propagation](../spec/05-expressions.md#propagation).

**Open questions.**

1. Where `downcast` is declared. Decided: a generic free function in
   `std.inspect` (see [INSPECTABLE.md](INSPECTABLE.md) decision 13).
2. Calling static (receiverless) functions. Under a bound, `T::create()` with
   `T: Factory` could be served by the bound's dictionary, but the
   specification only shows concrete `Type::function(...)` calls. Through a
   runtime type object, calling a trait's static function first requires
   knowing the type implements that trait, which conflicts with the
   no-conformance-query rule.
3. The matching `TypeShape` case, shared with
   [Complete Runtime Shape Coverage](#complete-runtime-shape-coverage), and
   the checked-downcast option in
   [Typed Derivation](#typed-derivation-tool-adapters-and-secrets).

**Design draft.** [Runtime Type Identity](INSPECTABLE.md) proposes the
trait, the runtime type object, and `downcast` semantics, and lists the
remaining questions.

**Unblocks.** Error-chain inspection, plugin registries, typed extension maps,
and a reviewable parametricity guarantee for erased generics.

### Confirmed Deferred Type Features

**Problem.** Two surfaces remain intentionally unsupported and must be
diagnosed: first-class bound methods, and direct permission weakening combined
with generic variance. General runtime type tests have their own issue,
[Runtime Type Identity And `reified`](#runtime-type-identity-and-reified).

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

### Value-Category Coverage Of `AnyVal` And `AnyRef`

**Problem.** Every value type implements exactly one of the sealed
`AnyVal` and `AnyRef`
([Trait Values And `Any`](../spec/04-type-system.md#trait-values-and-any)).
The two lists name the primitives, tuples, and reference values, but not
`void`, `never`, or nominal newtypes such as `type Mile(i32)`. Separately,
`std.inspect.downcast_val` in
[Inspectable decision 15](INSPECTABLE.md) is bounded by `T < Inspectable`
only, although it exists to recover `AnyVal` values.

**Options.** (1) `void` joins `AnyVal` like `()`, and `never` is exempt
from the rule because it has no values. (2) A newtype follows its
underlying type, or (3) a newtype is always `AnyRef`, like a fieldful data
type. (4) Bound `downcast_val` by `T < AnyVal + Inspectable`, so a
reference target is a compile error instead of `.None`.

**Recommendation.** None yet; the owner decides each point.

**Unblocks.** A complete partition statement and exact bounds for the
downcast functions.

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

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation and whether user-visible finalizers
  should ever be exposed;
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the complete `hd.toml` schema, executable-main selection, lockfile, version
  constraints, dependency resolver, and the concrete host binding for
  capabilities such as `Console`;
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes;
- property-testing strategies, shrinking, replay artifacts, and correlated or
  stateful generators in `std.testing`;
- final signatures, behavior, and the complete intrinsic set for the
  compiler-intrinsic `std.task` combinators, such as racing, retry, timeout,
  and heterogeneous scheduling;
- a higher-level `std.task.Task[T]` API, which must not weaken one-shot
  `Suspend[T]` semantics;
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
