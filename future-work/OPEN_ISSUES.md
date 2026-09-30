# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end. Resolved entries are
removed; the specification holds what they decided, and git history holds
their text.

## Language Design Decisions

### Follow-Ups Decided 2026-09-29 (Evening)

**Applied.** The ten evening follow-ups of 2026-09-29, the apply-pass
answers Let 1-5 and Map 6, batch 7 (Let 7), and batch 13 (Q1) are in the
specification; the [Revision Notes](../spec/README.md#revision-notes) list
each. The two that batch 15 below builds on:
- **Let 7**: a parenthesized `let` list may be a same-line suite body, as
  in `if ok: let (a, b) = pair`
  ([`grammar.inline.let-list`](../spec/02-grammar.md#r-grammar.inline.let-list)).
- **Q1**: a multi-name `:=` binding always uses parentheses, as in
  `(dt, key) := case`, and a line that starts with `(` begins a new
  statement ([Short Binding Lists](../spec/02-grammar.md#short-binding-lists)).

**Batch 15 (owner decision, 2026-09-30).** The owner answered the two
points left open by applying Q1, settled two `let mut` points (LM), and
stated three clarifications. Not yet applied:

| # | Decision |
| --- | --- |
| Q1a | A parenthesized `:=` list may be a same-line suite body, as in `if ok: (a, b) := pair`, as Let 7 allowed `let (a, b)`. `unused-local-binding` reports the unread names. |
| Q1b | The grouped form `(a, b := value)` is dropped. The one shape is `(a, b) := value`, and a nested use writes `((a, b) := value)`. The old form is a `syntax-error` whose fix-it writes `(a, b) := value`. |
| LM-a | A list name written `mut` whose annotated element is already `mut` warns `redundant-let-mut`, as the single-name form does. In both forms the fix-it removes the name-level `mut`, never the annotation. |
| LM-a note | With a generic right side, as in `make_pair[A, B]() -> (A, B)`, the annotation solves the type parameters; a name-level `mut` never supplies a type. `let (a, b): (Read, mut Mut) = make()` is the guide example. |
| LM-b | `mut self` in an impl whose `Self` is primitive is not `mut-on-primitive`; only a `mut` written before a primitive type is. The rationale: an impl repeats its trait method's signature, so a trait with `mut self` must stay implementable for primitives. |
| CLO1 | A closure is monomorphic: it declares no type parameters, and its types come from its annotations or the expected function type. `fn[T](x: T): x` is a `syntax-error`. This clarifies the rules; no behavior changes. |
| Q-? | The operand of `x?` gets an expected type as an inference hint, never a coercion. From an expected `T` for `x?`, the hint is `Result[T, E]` with the enclosing function's error type `E`, or `T?`. So `let ports: List[i32] = it.collect()?` works. |
| Q-map | There is no std-only exception: std writes `impl[K < Eq & Hash, V] Iterable[(K, V)] for Map[K, V]`, and `FromIterator` likewise. No new rule. |

### Bound And Row Operators

The owner's decisions (2026-09-28) are applied: bounds join with `&`, rows
join with `+`, and the old spellings are `old-row-separator` and
`old-bound-operator` ([Multiple Bounds](../spec/02-grammar.md#multiple-bounds),
[Requirement Clauses](../spec/02-grammar.md#requirement-clauses),
[Row Operators](../spec/02-grammar.md#row-operators),
[Least Row Solutions](../spec/11-requirements-and-suspension.md#least-row-solutions)).

**Questions from applying them.** Each needs an owner answer; the spec
states the current behavior.

| Question | Applied now | Recommendation |
| --- | --- | --- |
| Does `$ A + B` inside `[...]` or a parameter list need precedence rules? | A row ends at the first `,`, `)`, or `]`; nested function types keep the innermost-owner rule | None needed. No ambiguity was found in type arguments, parameters, `$.Context[...]`, or closure headers. |
| Codes for other old row spellings | `$(A + B)`, `$(A)`, a `-` between keys, and the pre-2026-09-27 `Job[A + B]` and `$.Context[A + B]` are `syntax-error` | Keep `syntax-error`. Only the two decided codes carry fix-its. |
| `$.Context[$ A + B]` keeps its inner `$`, while one key is `$.Context[A]` | Kept: the context type takes a key or a row type argument | Keep it. It matches row type arguments such as `Job[$ A + B]`. |

### Mutable Host Providers

The owner's decisions (2026-09-27 and 2026-09-28) are applied: a
requirement trait with a `mut self` method is a mutable requirement trait
([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)),
`Console.write_line!` takes `mut self`, and `println` is an ordinary std
function that drives `write_line!` with `block_on`
([`module.console.println-write`](../spec/10-modules.md#r-module.console.println-write)
through
[`module.console.println-script`](../spec/10-modules.md#r-module.console.println-script)).
Console calls stay out of replay recording.

**Still open (raised 2026-09-28).** Nothing here is decided:

| Question | Effect | **Recommendation** |
| --- | --- | --- |
| A fixture for a pending host write | No [runtime profile](../spec/conformance/README.md#runtime-profiles) holds a `write_line!` pending and then completes it: `console` is ready on the first poll, and `pending-gate` never completes. So `module.console.println-drive.pending` and `block_on`'s own wait have only a prototype unit test. | Add a conformance profile whose gate is pending on its first poll and ready on the next. |
| A fixture for the `.Err` panic | `module.console.println-error.category` has no fixture, since no code can build a `ConsoleError` (follow-up 2 defers its constructor). | Add the fixture when `ConsoleError`'s constructor is settled. |

```text
fn report!() -> void $ Console:
    defer:
        println("done")   # error: suspension-forbidden-context
    println("working")    # panics: suspension-nested-driver under a driver
```

### Typed Derivation, Tool Adapters, And Secrets

**Decided.** Owner decisions M1-M30 are applied in
[Typed Derivation](../spec/14-annotations.md#typed-derivation), and M30
confirms the readings of the M26 apply pass. Error derivation is the
separate `@error` intrinsic, applied in
[Error Derivation](../spec/14-annotations.md#error-derivation) and listed
below.

**Waiting on other areas.** The spec lists these as
[undecided parts](../spec/14-annotations.md#undecided-parts); each waits
for the owner, and [Typed Derivation](TYPED_DERIVATION.md#remaining-open)
gives their background:

| Question | What is undecided |
| --- | --- |
| Non-escaping handles (M18 R5) | Whether the parked NonEscapable design (TQ-24 to TQ-26) makes handles non-escaping. |
| `Clone`'s module (M24) | Which standard module declares `Clone`; chosen with the standard library ([STDLIB](STDLIB.md#clone)). |
| Derived-function cache (M24) | The cache's API and module; chosen with the standard library ([STDLIB](STDLIB.md#derived-function-cache)). |
| Function targets | Deriving for functions, as tool adapters need ([FN_TYPE](FN_TYPE.md) questions 9 and 10). A decorator before a function attaches a plain value that `shape_of(f).metadata[M]()` reads ([Prefix Decorators](../spec/14-annotations.md#prefix-decorators)). |

M30 deferred template constants, typed shared constants, and composing
templates until a real template needs them; they are not in the spec.

**Secret values (removed for now).** `Secret[T]` and `Redact` were removed
from the standard-library design as too early
([STDLIB decision 12](STDLIB.md#owner-decisions), 2026-09-26). Revisit them
together with typed derivation. Options already discussed: whether standard
capability traits may take `Secret[T]` parameters so the host receives the
real value without an `expose()` in hd code; whether exported functions may
take `Secret[T]` inputs; and that a secret never encodes or appears in
outputs.

### Provider Scope Overlap

**Owner direction (batch 13, Q4, 2026-09-30).**
[`req.with.nearest.forced`](../spec/11-requirements-and-suspension.md#r-req.with.nearest.forced)
lets a callee's provider serve a callback's lookup of a key that the caller
fixed in `R`. A research pass compared lexical row keys, as Effekt's
tunneling does, with making that overlap an error, on hd's own requirement
examples; then the owner was asked.

**Batch 14 (owner decision, 2026-09-30).** Applied in
[Lexical And Dynamic Providers](../spec/11-requirements-and-suspension.md#lexical-and-dynamic-providers)
and the rules it links. PS1: row keys stay dynamically scoped. PS2:
lexical scoping is explicit, by capturing `$.use(K)` in the closure. PS3:
no closure captures a `$.with`-bound key implicitly; its row resolves at
each call.

**Batch 15 (owner decision, 2026-09-30).** Not yet applied. PS3a:
inside a closure,
[`req.with.collision.compared`](../spec/11-requirements-and-suspension.md#r-req.with.collision.compared)
counts only keys that a lookup in the closure body can select. Those are
the closure's declared or inferred row and the `$.with` blocks inside the
closure. A `$.with` block outside the closure does not count: since batch
14 removed implicit capture, no lookup in the closure reaches it.

### Serializable Closures And Incremental Computation

**Problem.** Closures have unspecified identity
([Identity](../spec/05-expressions.md#identity)) and no stable code
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
([`data.repr.runtime-only`](../spec/08-data-and-enums.md#r-data.repr.runtime-only)).

**Unblocks.** Persisted callbacks, safe incremental caches, distributed work,
and bounded graph lifetimes.

**Decided 2026-09-27, not yet applied: option 1 now.** A serializable
closure's code identity is a content hash. Its captures must be
boundary-safe values (Durable Replay decision 11, in
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules), replaces the `Durable` bound), and capturing a provider or mutable state is
rejected. The design still needs a record: the hash input, how a closure
opts in, and graph lifetimes.

### Observability Hooks

**Problem.** There is no task-local carrier for trace context and no
specified point where suspension/provider activity can be instrumented without
rewriting user code.

**Decided.** Observability and replay use separate hooks, and both derive
their IDs from the execution ID and the event index
(Durable Replay decision 14, in
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules)).

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

### Confirmed Deferred Type Features

**Problem.** One surface remains intentionally unsupported and must be
diagnosed: direct permission weakening combined with generic variance.
Bound methods are now `value::name` references
([Method References](../spec/07-functions.md#method-references), MR1). Runtime type tests beyond exact-type recovery from
`Inspectable` values stay unsupported
([Runtime Type Identity](../spec/09-traits.md#runtime-type-identity)).

**Direction.** Keep weakening with variance deferred, and design it only
with a motivating requirement; it must preserve representation.
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

### Closure Shorthand

**Deferred (Pipe Operator PL10, 2026-09-29).** Closures stay
`fn(v): v * 2`; there is no `_` lambda shorthand, and no `f(_, a)` capture
(PL13). The pipe owns `_` inside a step
([Pipe Expressions](../spec/05-expressions.md#pipe-expressions)), `it` is
the prelude test function, and `$0` collides with requirements and
interpolation. `fn: _ * 2` would parse but needs a "not inside a pipe
step" exception. Revisit if [the hd writing log](../audit/hd-writing-log.md)
shows demand from cheap-model agents; adding `fn: _` then breaks no code.

### Iterator Performance

**Deferred.** Chaining Study CS8 (2026-09-29) keeps the closure-backed
data `Iterator[T]` as the one public iterator type. The flat
composed-stage design C is a later option, once the compiler specializes
and inlines closures. [Iterator Performance Study](ITERATOR_PERF.md) keeps
the analysis, the stage 2 benchmarks to rerun, and C's two open questions.

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([`data.repr.runtime-only`](../spec/08-data-and-enums.md#r-data.repr.runtime-only));
- the prototype's replay experiments in the
  [Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) predate the
  decided [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules): their
  identity is per function rather than per program, their site IDs contain
  byte offsets, and they stop at the end of a history instead of resuming;
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the final `hd.toml` schema and the concrete host binding for capabilities
  such as `Console`. Executable-main selection is drafted in
  [Packages](PACKAGES.md#26-entry-points);
- dependencies through version control hosts, with no registry:
  Dependencies decisions DEP1-DEP7
  are applied in [Package Manifest](../spec/10-modules.md#package-manifest)
  (version tags, minimal version selection, `hd.sum`, workspaces,
  pseudo-versions), with DEP8-DEP19 after them. The manifest diagnostics
  wait for the manifest schema (DEP14,
  [`module.tooling.package-schema`](../spec/10-modules.md#r-module.tooling.package-schema)),
  and the tooling work is in
  [Package Tooling](RUNTIME_AND_LIBRARY.md#package-tooling);
- a `package-cycle` conformance fixture, which waits until the manifest
  schema exists (Dependency Cycles DC12,
  [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package));
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, as [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules)
  state);
- stateful property testing in `std.testing`, which waits for the event
  log. The property-test API is decided and applied
  ([Testing PT1-PT9](TESTING.md#owner-decisions),
  [Property Tests](../spec/10-modules.md#property-tests));
- doc tests and benchmarks, which no decision covers yet. The testing
  stress test (TS-15) found a direction: doc tests as fenced `hd` blocks in
  `##` comments of `pub` items, run with the `tests/` view, and benchmarks
  with a host clock and their own registration, like Go's `b.Loop` or a
  `benches/` root;
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
