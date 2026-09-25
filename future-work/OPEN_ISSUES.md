# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end.

## Language Design Decisions

### Replay Determinism And Durable Workflows

**Problem.** Cold suspensions and provider capture do
not define durable event identity, replay interception, deterministic code
between suspension points, or provider compatibility after a restart.

**Options.** (1) Add a compiler/runtime workflow mode with stable suspension
site IDs, an interception hook at each provider bang call, `Durable` result
serialization, nondeterminism checks, and provider-configuration identity.
(2) Expose only low-level event-log APIs and make libraries assign IDs and
police determinism. (3) Treat durable replay as out of scope and support only
in-memory suspension.

**Recommendation.** Option 1. Site IDs should derive from explicit stable
labels when present and compiler-maintained source identity otherwise; resume
must reject an incompatible provider configuration. Review tooling should
render provider-call and suspension-point order and flag non-idempotent provider
calls before a suspension point when the provider contract exposes that fact.

**Unblocks.** Crash recovery, workflow upgrades, deterministic replay tests,
and durable orchestration as a defining use case.

**Status.** The first experiment in the
[Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) records frame-poll
events with function-name identities, per-function source identities, and a
provider-configuration identity. It demonstrates that replay can tolerate an
unrelated declaration insertion while rejecting a changed executed function or
provider configuration. The issue remains open for explicit source labels,
suspending provider-call interception, durable argument/result encoding, and
the determinism checks between provider calls.

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
invocation, validation at the boundary, and reliable secret redaction. These
are language-design tasks, not merely library work.

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
runtime hook shared with durable replay. (2) Model tracing only as explicit
requirement providers. (3) Let hosts instrument Wasm calls without
language-level correlation.

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
5. A type test is a downcast method, `value.downcast[T]() -> T?`, with `T`
   reified. It works on a dynamic `Inspectable` value and on a parameter
   bounded by `T: Inspectable`. `downcast` is not a trait method: like Rust's
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
    inspectable.

**Open questions.**

1. Where `downcast` is declared. Candidates: inherent methods on a trait value
   type, which would be a new kind of `impl` target, or a compiler-provided
   method limited to `Inspectable`.
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

**Unblocks.** Error-chain inspection, plugin registries, typed extension maps,
and a reviewable parametricity guarantee for erased generics.

### Confirmed Deferred Type Features

**Problem.** Two surfaces remain intentionally unsupported and must be
diagnosed: first-class bound methods, and direct permission weakening combined
with generic variance. General runtime type tests have their own issue,
[Runtime Type Identity And `reified`](#runtime-type-identity-and-reified).

**Direction.** Keep bound methods and weakening with variance deferred, and
design each only with a motivating requirement. Bound methods must settle
receiver capture; weakening with variance must preserve representation.
Negative implementations and additional pack operations are likewise confirmed
future work rather than implicit extensions.

**Unblocks.** Implementer certainty today and a checklist for future proposals.

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
