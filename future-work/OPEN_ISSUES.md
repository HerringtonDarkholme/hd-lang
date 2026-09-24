# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end.

## Language Design Decisions

### Provider Escape And Authority Visibility

**Problem.** A value obtained through `$.use(K)` is an ordinary value today. It
can escape into a global, field, closure, return value, or suspension frame. In
addition, a closure created inside `$.with` currently captures each lexical
provider its body uses, as specified by the closure capture rule. A callable's
row therefore records unresolved provider lookup rather than all authority the
callable can exercise. This limits capability auditing.

**Options.** (1) Make provider values second-class and forbid storing or
capturing them unless the enclosing callable row contains the key. (2) Put
capture rows on closure and `Suspend` types, following capture-set systems.
(3) Keep ordinary values, reword the claim, and require a compiler value-flow
report for authority audits.

**Recommendation.** Prototype option 1 first and move to option 2 if useful
provider-carrying abstractions cannot be expressed. Do not rely on wording
alone.

**Unblocks.** Sound per-tool capability reports, sandbox review, provider
escape diagnostics, and an end-to-end “no ambient authority” claim.

### Purity In Function Types

**Problem.** Purity checks for defaults, annotation
materialization, replay, and `std.incremental` cannot validate an indirect
function value or dynamic trait dispatch from its current type. The accepted
interim rule rejects such calls in purity-checked contexts unless a named
callable has a verified summary; this is sound but restricts higher-order APIs.

**Options.** (1) Make plain `fn` pure and use `mut fn` for all externally
observable effects. (2) Add an explicit `pure` qualifier to function and trait
method types. (3) Define purity solely from visible signature properties and
reject indirect calls whose signature cannot prove it.

**Recommendation.** Add an explicit `pure` qualifier. It communicates intent,
works for dynamic dispatch, and avoids changing the established meaning of
`mut fn` as mutable capture access.

**Unblocks.** Sound default arguments, annotation builders, deterministic
workflow checking, and safe incremental callbacks across package boundaries.

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

### Task Combinator Extensibility

**Problem.** Sealed `Suspend[T]` keeps one-shot checks enforceable, but ordinary
source cannot implement polling combinators such as heterogeneous `all!`,
`race!`, timeout, or select, and the current bang-call form applies only to
declared suspending functions.

**Options.** (1) Make the standard combinators compiler-known intrinsics with
`fn!` signatures. (2) Permit bang calls on any function returning
`mut Suspend[T]`. (3) Expose a safe user combinator protocol without unsealing
the low-level frame contract.

**Recommendation.** Start with option 1 and require user code to compose the
standard primitives; consider option 3 only after concrete combinator use cases
show that composition is insufficient.

**Unblocks.** Implementable standard scheduling, explicit user-library limits,
and portable timeout and racing behavior.

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

### Stable Hashing

**Problem.** `Hash` is intentionally process- and runtime-dependent, so
it cannot safely identify persisted cache entries, workflow inputs, or code.

**Options.** (1) Add `StableHash` in `std.fingerprint` with a fixed algorithm
and canonical field encoding. (2) Version the algorithm in every digest and
permit several standard algorithms. (3) Leave stable encoding entirely to
applications.

**Recommendation.** Option 2 with one mandatory default algorithm. Include the
algorithm/version identifier in every fingerprint and define evolution rules.

**Unblocks.** Persistent caches, idempotency keys, content-addressed code, and
cross-runtime replay validation.

### Access Control And Tenancy

**Problem.** Requirement rows show which service is reachable, not the
principal, tenant, delegation, or attenuation under which it is used.

**Options.** (1) Make `$ Principal` an explicit requirement key. (2) Require
provider attenuation such as `db.for_tenant(tenant)`. (3) Combine an explicit
principal requirement with typed attenuated provider views.

**Recommendation.** Option 3. A worked tool example must show authentication,
principal lookup, tenant attenuation, a database call, and redacted output.

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
per-tool capability reports—specify them end to end, mark the rest as initial
release non-goals in `USE_SCENARIOS.md`, and add chapter 11 fixtures before
expanding other chapters.

**Unblocks.** A smaller compiler, clearer teaching material, faster
conformance, and evidence that complexity serves the language thesis.

### Annotation Locality And Inspection

**Problem.** The effective annotation for a target can be assembled
across distant files and dependencies, making review and provenance difficult
even with global coherence.

**Options.** (1) Require `annotate X` in `X`'s defining module, with an explicit
foreign-target form. (2) Permit any module in the owning package. (3) Keep
global placement and rely on an inspection command.

**Recommendation.** Option 1 plus a compiler command that prints the complete
materialized `Info` plan and provenance for a target without executing
effectful code.

**Unblocks.** Local review, understandable generated schemas, annotation
debugging, and safer dependency upgrades.

### Requirement Sigils

**Problem.** `$`, `$.use`, `!`, and overloaded `?` produce dense syntax
and collide with strong conventions from other languages, increasing errors in
generated code.

**Options.** (1) Keep the sigils and improve diagnostics/formatting. (2) Spell
rows as `requires Database + Clock` and bind declared requirements by name.
(3) Keep `!`/`?` but replace only `$` and `$.use` with keywords.

**Recommendation.** Prototype option 3 in the parser and AI-writing study
before changing source syntax. Do not implement a migration until the study
shows a material error-rate improvement.

**Unblocks.** A data-backed syntax decision, simpler tool examples, and a
possible reduction in model repair loops.

### Requirements Vocabulary

**Problem.** Several `USE_SCENARIOS` requirements are phrased as chosen
mechanisms—annotation syntax and row operations—rather than user or reviewer
outcomes, making traceability circular.

**Options.** (1) Rewrite every need as an observable reviewer/developer
outcome and map mechanisms separately. (2) Keep both an outcome statement and
a non-normative mechanism note. (3) Retain mechanism-shaped requirements.

**Recommendation.** Option 2. For example: “a reviewer can list every external
system a tool can touch from its signature,” followed by a trace to requirement
rows.

**Unblocks.** Honest feature prioritization, alternative-design comparison,
and acceptance tests independent of syntax.

### Confirmed Deferred Type And Task Features

**Problem.** Four surfaces remain
intentionally incomplete: first-class bound methods, general runtime type tests
beyond `ShapeMetadata.metadata[M]()`, direct permission weakening combined with
generic variance, and a higher-level `Task[T]` API.

**Options.** (1) Keep all four unsupported and diagnose their syntax/typing.
(2) Design them together with closure capture, reflection, and suspension
work. (3) Standardize each independently as use cases appear.

**Recommendation.** Option 1 for now, then option 3 only with a motivating
requirement. Bound methods must settle receiver capture; downcasts must preserve
permission; weakening/variance must preserve representation; `Task[T]` must not
weaken one-shot `Suspend[T]` semantics. Negative implementations and additional
pack operations are likewise confirmed future work rather than implicit
extensions.

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

**Recommendation.** Prototype option 1 alongside the accepted `defer` behavior,
following the locality direction in
[Ownership and Escape Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md), before
committing to a full ownership system. Retain checked disposal errors under
every option.

**Unblocks.** Leak-resistant files/sockets, safe cancellation, fallible cleanup
design, and stronger sandbox guarantees.

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation and whether user-visible finalizers
  should ever be exposed;
- the complete `hd.toml` schema, executable-main selection, lockfile, version
  constraints, dependency resolver, and the concrete host binding for
  capabilities such as `Console`;
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes;
- property-testing strategies, shrinking, replay artifacts, and correlated or
  stateful generators in `std.testing`;
- final `std.task` signatures and behavior for racing, retry, timeout, and
  heterogeneous scheduling combinators;
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
