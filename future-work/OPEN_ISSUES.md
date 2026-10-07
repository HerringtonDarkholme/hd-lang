# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end. A resolved entry is removed: the specification holds what it decided, and
git history holds its decision table.

## Language Design Decisions

### Typed Derivation, Tool Adapters, And Secrets

**Waiting on other areas.** The spec lists these as
[undecided parts](../spec/lang/14-annotations.md#undecided-parts); each waits
for the owner, and Typed Derivation
gives their background:

| Question | What is undecided |
| --- | --- |
| Non-escaping handles (M18 R5) | Whether the parked NonEscapable design (TQ-24 to TQ-26) makes handles non-escaping. |
| `Clone`'s module (M24) | Which standard module declares `Clone`; chosen with the standard library (STDLIB). |
| Derived-function cache (M24) | The cache's API and module; chosen with the standard library (STDLIB). |
| Function targets | Deriving for functions, as tool adapters need ([parked](#parked-tool-adapters)). A decorator before a function attaches a plain value that `facts_of(f).find::[M]()` reads ([Function Facts](../spec/lang/14-annotations.md#function-facts)). |

M30 deferred template constants, typed shared constants, and composing
templates until a real template needs them; they are not in the spec.

#### Parked: Dead-Fact Warnings

The commit titled `BA: Defer dead-fact warnings` removed these four rules
from [Facts](../spec/lang/14-annotations.md#facts):

> 9. r[annot.fact.unused-non-std] A type-level fact whose type comes from a package other than `std`, where that package supplies no template that the type derives, gets a warning, reported on its decorator. Warning: `unused-derivation-fact`.
>
> 10. r[annot.fact.unused-std] A fact of a primitive or standard type, such as `@"internal"`, never gets this warning.
>
> 11. r[annot.fact.unused-self-line] A type-level fact that a trait-less block's `Self` line writes gets the same warning under the same conditions, reported on that line. Warning: `unused-derivation-fact`.
>
> 12. r[annot.fact.unused-self-line.per-trait] A type-level fact that a `Self` line of a derivation block for a trait writes gets the same warning, reported on that line, when the fact's package does not supply that trait. Warning: `unused-derivation-fact`.

That commit renamed three warning fixtures as acceptance fixtures:
`typing/warnings/unused-derivation-fact.hd` to
`typing/valid/type-level-fact-without-template.hd`,
`typing/warnings/trait-less-self-line-unused-fact.hd` to
`typing/valid/trait-less-self-line-fact.hd`, and
`typing/warnings/per-trait-self-line-unused-fact.hd` to
`typing/valid/per-trait-self-line-foreign-fact.hd`. Use
`git show ':/^BA: Defer dead-fact warnings$'` to restore the removed rules,
checker code, and former fixtures.

Dead facts are worth a warning, but the accurate rule is read-set based.
Warn only when no template the type derives reads facts of that type; for a
`Self` line, consider the block's template. A template's read set is the fact
types it looks up with `find::[F]`; a dynamic fact lookup counts as reading all
fact types.

Revisit the warning when the checker can see template read sets across
packages, such as from a dependency's checked interface. The motivating false
positive was a shared vocabulary package that only defines fact types: a
`Label` fact read by both `Form` and `Grid` templates was incorrectly warned as
dead.

#### Parked: Tool Adapters

Parked with typed derivation (FN_TYPE decision 10); tools register
functions by hand for now. Background is in the archived
Nominal Function Types.

| Question | Options | **Recommendation** |
| --- | --- | --- |
| FN-Q9: how does a tool adapter get per-declaration data about a function? | A1, `shape_of(f)` passed beside the value; A2, a `fn_view(f)` intrinsic; B, per-declaration item types with a compiler-generated `FnStructure`, so `@derive(mcp.Tool)` works on functions. | B, or A2 if item types are too much surface. |
| FN-Q10: where are item types visible? | A, only where a generic parameter is inferred from the argument and in heads written `fn name`; B, everywhere, as in Rust. | A: bindings and list literals keep their function types. |

**Secret values (removed for now).** `Secret[T]` and `Redact` were removed
from the standard-library design as too early
(STDLIB decision 12, 2026-09-26). Revisit them
together with typed derivation. Options already discussed: whether standard
capability traits may take `Secret[T]` parameters so the host receives the
real value without an `expose()` in hd code; whether exported functions may
take `Secret[T]` inputs; and that a secret never encodes or appears in
outputs.

### Serialization Formats

Owner decision, 2026-10-05: one format-neutral consent per type, many
formats, as in Rust's serde and Swift's `Codable`
([Serialization](../spec/lang/14-annotations.md#serialization)). These
parts wait:

| Question | State |
| --- | --- |
| Per-format overriding | Deferred by the owner. The plan below needs no new mechanism. |
| Boundary encoding through the consent | A consented value crosses a host boundary as its field tree ([`module.boundary.consent.tree`](../spec/lang/10-modules.md#r-module.boundary.consent.tree)). So a hand-written consent, as `Duration`'s one `int`, changes JSON but not the boundary. Whether the boundary should encode through `serialize` and `deserialize` is open. |
| Schemas | Which consent carries a `describe` for schemas, and the data model a schema describer reads. |
| More standard consents | Tuples, `Result`, `Set`, and maps whose keys are not `string` have no standard implementation yet. |

**Per-format overriding, planned through facts.** Facts are sufficient; no
per-format trait and no specialization is needed. The owner considered a
blanket implementation with a per-type override and ruled it out of scope.

- Format-neutral facts cover the common customizations:
  `@serde(rename="mail")`, `@serde(skip)`, and `@serde(default)` on a
  member, read through the `Member` facts that every format receives.
- A format-scoped fact covers one format: `@json(repr="string")` writes an
  `i64` member as a string, for large integer IDs, since JSON in
  JavaScript has no `i64`. `@json(repr="display")` round trips a value
  through its `Display` text and a parse, which covers custom date or money
  text without a function-valued fact.
- The format's writer or reader reads the fact, as `std.json`'s does for
  `@json(...)`. Derivation blocks scope facts per derivation, so a
  `Serialize` block and a `Deserialize` block may differ.
- Facts are values, not code, so a fact never runs user code inside a
  format.
- Known limitation, as in serde: a program cannot restyle a type it does
  not own. It can only put a fact on its own field of that type, as
  `@json(repr="display")` on a `Timestamp` member.

### Serializable Closures And Incremental Computation

**Problem.** Closures have unspecified identity
([Identity](../spec/lang/05-expressions.md#identity)) and no stable code
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
([`data.repr.runtime-only`](../spec/lang/08-data-and-enums.md#r-data.repr.runtime-only)).

**Unblocks.** Persisted callbacks, safe incremental caches, distributed work,
and bounded graph lifetimes.

**Decided 2026-09-27, not yet applied: option 1 now.** A serializable
closure's code identity is a content hash. Its captures must be
boundary-safe values (Durable Replay decision 11, in
Replay Rules, replaces the `Durable` bound), and capturing a provider or mutable state is
rejected. The design still needs a record: the hash input, how a closure
opts in, and graph lifetimes.

**Not in the first release (owner, 2026-10-06).** The new compiler's
first release has no serializable closures. The decision above waits
until after it.

### Observability Hooks

**Problem.** There is no task-local carrier for trace context and no
specified point where suspension/provider activity can be instrumented without
rewriting user code.

**Decided.** Observability and replay use separate hooks, and both derive
their IDs from the execution ID and the event index
(Durable Replay decision 14, in
Replay Rules).

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

**Hook points first (owner, 2026-10-06).** The new compiler reserves no-op
hook points in suspension lowering from day one. The hook API and replay
come later.

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

### Confirmed Deferred Type Features

**Problem.** One surface remains intentionally unsupported and must be
diagnosed: direct permission weakening combined with generic variance.
Bound methods are now `value::name` references
([Method References](../spec/lang/07-functions.md#method-references), MR1). Runtime type tests beyond exact-type recovery from
`Inspectable` values stay unsupported
([Runtime Type Identity](../spec/lang/09-traits.md#runtime-type-identity)).

**Direction.** Keep weakening with variance deferred, and design it only
with a motivating requirement; it must preserve representation.
Negative implementations are likewise confirmed future work rather than an
implicit extension.

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

**After the first release (owner, 2026-10-06).** NonEscapable is planned
after the new compiler's first release.

**Parked questions.** The owner does not want to discuss NonEscapable now.
These are the initial answers, not to be applied until the owner reopens
the topic.

| # | Question | Initial answer |
| --- | --- | --- |
| TQ-24 | How does NonEscapable propagate to a type holding a NonEscapable field, as `data HiddenFile` with `file: File`? Automatically, like Rust's auto traits, or declared and checked? | Declared and checked: such a type must itself be declared NonEscapable, and a generic type is NonEscapable exactly when an argument is. Erasure to `Any`, or to a trait value whose trait does not extend NonEscapable, is rejected. |
| TQ-25 | Does a generic parameter accept a NonEscapable argument by default? | No: a parameter opts in, as Swift's `~Escapable` does, so existing generic code stays valid. |
| TQ-26 | How does a NonEscapable result, as in `fn first_line(file: File) -> Line`, say which parameters it depends on? | No NonEscapable returns for now; later, depend on every NonEscapable parameter, and name them only when a real API needs it. |

**Parked idea: `@pure` functions (owner, 2026-10-07).** An annotation such
as `@pure` that bans capturing: the function reads and writes no module
state and captures nothing, directly or through its callees. It came up
when shared enum data moved to module initialization: requirement-free
does not mean pure, because a requirement-free function may still write a
top-level `let`. The owner wants to discuss it together with capturing in
NonEscapable (how a closure that captures a NonEscapable value is itself
non-escapable). Considered and rejected for now: making module writes a
requirement (it propagates through every caller's written signature).
Not in the spec; nothing depends on it.

### Closure Shorthand

**Deferred (Pipe Operator PL10, 2026-09-29).** Closures stay
`fn(v): v * 2`; there is no `_` lambda shorthand, and no `f(_, a)` capture
(PL13). The pipe owns `_` inside a step
([Pipe Expressions](../spec/lang/05-expressions.md#pipe-expressions)), `it` is
the prelude test function, and `$0` collides with requirements and
interpolation. `fn: _ * 2` would parse but needs a "not inside a pipe
step" exception. Revisit if [the hd writing log](../audit/hd-writing-log.md)
shows demand from cheap-model agents; adding `fn: _` then breaks no code.

### ContextError Construction

**Deferred (CONTEXT-FIELDS, batch 72, 2026-10-03).** The fields of
[`ContextError`](../spec/std/error.md#error-context) stay private.
Revisit: a public `ContextError::new(message, cause)` and
`message(self) -> string`, keeping the layout private, if users need to
wrap by hand or match on it.

### GADTs

**Removed 2026-10-07; revisit only if a real need appears** (owner). A
variant cannot declare its own result type or type parameters
([`grammar.enum.no-result-type`](../spec/lang/02-grammar.md#r-grammar.enum.no-result-type)).
A typed request and its response pair through a trait with an associated
type:

```text
trait Request:
    type Response

data GetUser:
    id: i64

impl Request for GetUser:
    type Response = User

fn send[R < Request](request: R) -> R::Response: ...
```

A typed interpreter uses a runtime value enum (`enum Value: Int(i64);
Bool(bool)`) or a trait per node type. Revisit with a motivating program
that neither form handles; the removed chapter is in git history
(`git show f0e4b2b1:spec/lang/13-gadts.md`).

### API Compatibility Checking (from the deleted PACKAGES.md)

**Deferred.** Whether the checker is an `hd` command or a third-party
tool is decided later (owner, 2026-10-06). It decides whether two versions on one
compatibility line are compatible by classifying each difference between
their interface files. An unclassified difference counts as breaking:
patch when the signatures are equal, minor when every difference is a
compatible addition, a new line otherwise. Owner decisions 6 and 7
(2026-09-26) fix two classes. Adding an implementation of a foreign trait
is minor, and adding an enum variant is breaking, for now. The full
starting classification table is in git history
(`git show 382a3c1d:future-work/PACKAGES.md`, section 3.3). Also open: which
targets take the root-application role. The package model itself is
specified in [Package Manifest](../spec/lang/10-modules.md#package-manifest).

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([`data.repr.runtime-only`](../spec/lang/08-data-and-enums.md#r-data.repr.runtime-only));
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the final `hd.toml` schema. The default profile's host binding and
  `[capabilities]` are specified in
  [Host Capabilities](../spec/cli/command-line.md#host-capabilities);
- the manifest diagnostics that wait for the manifest schema (DEP14,
  [`cli.tooling.package-schema`](../spec/cli/command-line.md#r-cli.tooling.package-schema)).
  Dependencies themselves (fetching, the cache, `hd.sum`, selection,
  workspaces, pseudo-versions, path requirements) are specified and
  implemented ([Dependencies](../spec/cli/command-line.md#dependencies));
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, as Replay Rules
  state);
- stateful property testing in `std.testing`, which waits for the event
  log. The property-test API is decided and applied
  (Testing PT1-PT9,
  [Property Tests](../spec/std/testing.md#property-tests));
- benchmarks, which no decision covers yet. The testing stress test
  (TS-15) found a direction: benchmarks with a host clock and their own
  registration, like Go's `b.Loop` or a `benches/` root;
- final signatures, behavior, and the complete intrinsic set for the
  compiler-intrinsic `std.task` combinators, such as racing, timeout,
  and heterogeneous scheduling (`retry!` is a library loop, decided in
  batch 29: [Task](../spec/std/task.md#retry));
- the final `std.task` structured-scope API: `Task[T]` is decided as
  structured scopes only, with `scope!`, `start`, and `join!`
  (STDLIB decision 11), and must not weaken
  one-shot `Suspend[T]` semantics;
- host capability grants are specified: the default profile and
  `[capabilities]` in
  [Host Capabilities](../spec/cli/command-line.md#host-capabilities) and
  [Capability Grants](../spec/cli/command-line.md#capability-grants), the
  test grant in
  [Test Environments](../spec/cli/command-line.md#test-environments), and
  [Http](../spec/std/http.md), [Net](../spec/std/net.md) and
  [Sys](../spec/std/sys.md). The first release's host ABI is hd's own core-Wasm
  imports, not the Component Model, which can't carry Wasm GC values
  (owner, 2026-10-06,
  [research](compiler/research.md#open-questions-for-the-owner)).
  Where a point below names the component ABI, read it as the hd host ABI.
  These points stay open:
  - **Runtime code loading (parked, 2026-10-06).** A host trait `Loader`
    would load a built `.wasm` plugin at run time. It would check the
    plugin's exports against an hd interface trait, bind its row from an
    explicit `$.Context`, copy every value as boundary-safe data, and run
    it under memory and time limits, behind a `load` permission. It waits
    on:
    - the component ABI, so two builds agree on a trait's types;
    - an intrinsic that reifies a trait for a run-time check;
    - cross-instance provider handles (`own` and `borrow`);
    - the component-model async ABI for suspending plugin calls.

    Until then a plugin is a program started through `Process` or a
    service called through `Http`. Design:
    [Host Capabilities](HOST_CAPABILITIES.md#runtime-code-loading).
  - **Opaque host handles (with the resource design).** A host trait
    can't return a native object, such as a database connection, as a
    boundary value, because live handles are not boundary-safe. Custom
    host traits are hd's FFI ([Host Capabilities](HOST_CAPABILITIES.md#ffi)),
    so this limits every embedder. Target: component-model `resource`
    types.
  - **`std.net` after the minimal API.** The spec gives lookup, TCP and
    UDP over closable handle traits. Open: a deterministic `Net`
    provider, as `ScriptedHttp` is for `Http`; whether socket handles are
    `NonEscapable`; read and connect timeouts; socket options; the port
    that `listen!` picks for port 0; TLS over a stream; and how a handle
    trait that a provider returns crosses the component ABI.
  - **An HTTP server**, as a registered boundary that exports the
    `wasi:http` handler, with `Net` covering its listen address.
  - **A code for the total-deny refusal.** Under `--format json`, a
    diagnostic object needs a stable code
    ([`cli.json.diagnostic`](../spec/cli/command-line.md#r-cli.json.diagnostic)),
    and the startup refusal has none;
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
