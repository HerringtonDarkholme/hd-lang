# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end. A resolved entry is removed: the specification holds what it decided, and
git history holds its decision table.

## Language Design Decisions

### Readings Waiting For Confirmation

Applying earlier batches left these points. The specification applies
the reading in the third column, and each point asks the owner to confirm
it.

| # | From | Question | Applied reading and **Recommendation** |
| --- | --- | --- | --- |
| INF-lit | batch 17 | Does an integer literal argument take the type solved from the other arguments in any position? Without that, `pick(1, large)` with an `i64` `large` is a `type-mismatch`, since the literal alone is `i32`, while `pick(large, 1)` checks. | **Recommendation:** yes: a literal is not a conversion, so it takes the solved type as its expected type in any position, as Rust's integer literals do. |
| INF-code | batch 17 | Which code does any other conflict get, such as a `List[mut User]` and a `List[User]` (variance), a `T` and a `T?`, or two child-trait values? The decision names `type-mismatch` for numbers and `no-common-type` for trait values. | **Recommendation:** `no-common-type` where the least common type also fails (trait values, supertrait widening); `type-mismatch` otherwise, as `choose(1, true)` already is. |
| SR-omit | batch 17 | Does a variant's `self_ref` count a member that the derivation block omits (`cache = pass`)? | **Recommendation:** no: count only the members the derivation sees, since an omitted member takes its default and is never walked or built. |
| Q7-code | batch 26 | The decision says a `mut` key "stays an error" but names no code. | It keeps `invalid-map-key` ([`types.map-key.no-mut`](../spec/04-type-system.md#r-types.map-key.no-mut)), as variant B of Special Cases C8 proposed. **Recommendation:** keep it; the code names a rule no bound states. |
| VA-unbounded-code | batch 31a | An unbounded `Args` used as `Fn`'s inputs needs a code. | `generic-kind-mismatch`, as a non-tuple there already is, rather than `unsatisfied-trait-bound`. **Recommendation:** keep it; one rule covers both. |
| Q5-cycle-site | batch 32a | "Not just `embedding-too-deep`" leaves open whether a cycle also reports the depth code, and on which types. | `embedding-cycle` replaces `embedding-too-deep` for every type in the cycle, reported once per cycle on its first declared type, as `alias-cycle` is. A type outside the cycle that embeds into it still gets `embedding-too-deep`. **Recommendation:** keep it. |
| Q5-self-id | batch 32a | STYLE says a rule whose meaning changes gets a new ID, but the owner said to keep `data.embed.depth.self` while its code changes. | The ID is kept, as directed. **Recommendation:** keep it; the commit message records the code change. |
| Q5-hash | batch 34 | Lists have no `Hash` ([`types.map-key.no-hash`](../spec/04-type-system.md#r-types.map-key.no-hash)), so hashing the rest as its list gives a rest tuple no `Hash`. | Applied as stated: a rest tuple is not a map key; a Note says so. **Recommendation:** keep it until lists get `Hash`. |
| Q6-plain | batch 34 | The owner chose a spread pattern over a plain subpattern for the rest. Whether `let (a, b, xs) = t` on a rest tuple is then an error is not stated. | `type-mismatch` ([`flow.match.spread.required`](../spec/06-control-flow.md#r-flow.match.spread.required)), mirroring a spread pattern against a fixed tuple. **Recommendation:** keep it; the pattern then always shows the type's shape. |
| Q6-count | batch 34 | Whether a spread pattern may also take trailing fixed elements, as in `let (a, xs...) = t` with two fixed elements, is not stated. | No: the subpatterns before it must match the fixed elements one each, or `type-mismatch` ([`flow.match.spread.arity`](../spec/06-control-flow.md#r-flow.match.spread.arity)). **Recommendation:** keep it; collecting would build a new list in a pattern. |
| Q6-form | batch 34 | The decision shows `xs...` only. | A spread pattern is a name or `_` before `...` ([`grammar.pattern.tuple-spread`](../spec/02-grammar.md#r-grammar.pattern.tuple-spread)); `_...` binds nothing, and `mut xs...` works in `let`. hd has no list patterns, so no other pattern could match the list. **Recommendation:** keep it. |
| Q4-lone | batch 34 | A lone list spread into a rest-only `Args`, as in `call(k, xs...)` with `k(xs...: List[i32])`, is a positional spread of the whole vararg, so `xs` must be `Args`. | Kept: a lone spread passes the whole collected value ([`expr.call.spread.at-vararg`](../spec/05-expressions.md#r-expr.call.spread.at-vararg)), so this is `type-mismatch`, and `call(k, (xs...,)...)` passes it. **Recommendation:** keep it; one spread meaning per position. |
| Q4-infer | batch 34 | With no other argument solving `Args`, [`fn.vararg.tuple-param.infer`](../spec/07-functions.md#r-fn.vararg.tuple-param.infer) says one element per argument, but `pack(1, xs...)` ends in a list spread. | The tuple expression decides, as Q4 says: `Args` is `(i32, List[i32]...)`. **Recommendation:** keep it; reword `.infer` only if a reader finds it unclear. |
| Q6-default-char | batch 35 | The decision lists `char` with "(decide)". | `char` has no `Default`. **Recommendation:** give it `'\u{0}'`, as Rust does, only when a use needs one; no other char is more neutral. |
| Q6-default-derive | batch 35 | Whether `@derive(Default)` exists is not stated. | Not added: a data type implements `Default` by hand. **Recommendation:** wait; a template would need a rule for which enum variant is the default. |
| Q6-one-rest | batch 35 | Whether a rest tuple counts its rest's items when it decides on `(1,)` is not stated. | Yes: a rest tuple holding one value in all writes `(7,)`, as `debug` already wrote it the same as `(7,)` ([`expr.interp.std.tuple.one`](../spec/05-expressions.md#r-expr.interp.std.tuple.one)). **Recommendation:** keep it; the text then mirrors the value's literal. |
| O7-mut | batch 36 | A walk or describe handle of a `hits: mut Counter` member has `F = Counter`, but the fact binds to `mut Counter`. | `h.fact` finds only an exact type, so that read misses ([`annot.handle.fact.exact`](../spec/14-annotations.md#r-annot.handle.fact.exact)). **Recommendation:** keep it; `Arbitrary` reads through `build`'s declared-type handles. |
| O7-inspectable | batch 36 | Derived `Arbitrary` still requires every member to be `Inspectable`, a bound that existed only for the downcast. | Kept ([`std-testing.arbitrary.derive.member-bounds`](../spec/std/testing.md#r-std-testing.arbitrary.derive.member-bounds)); `with` itself dropped it. **Recommendation:** drop the member bound too. |
| O3-blocks | batch 36 | Templates allow derivation blocks, but comparison derivations were never configurable. | Still unconfigurable ([`trait.derive.cmp-every-member`](../spec/09-traits.md#r-trait.derive.cmp-every-member)), so law partners stay consistent. **Recommendation:** keep it. |
| O3b-rest | batch 36 | A rest member typed `List[T]` cannot meet a `Display` walker's bound, and text writes its items inline. | `Walker` gains `rest`, whose default calls `member` ([`annot.walk.rest`](../spec/14-annotations.md#r-annot.walk.rest)). **Recommendation:** keep it; the alternative is `Display` for `List`. |
| O3b-user | batch 36 | Whether a trait outside `std` may declare a tuple template is not stated. | Yes, under the template rules ([`annot.template.tuple.form`](../spec/14-annotations.md#r-annot.template.tuple.form)). **Recommendation:** keep it; it adds no `std` special case. |
| FO-code | batch 42 | The decision lets a bad `facts_of` argument reuse `unknown-shape-target`, renamed, or an existing code. | Renamed to `invalid-facts-of-target` ([`annot.facts-of.target.error`](../spec/14-annotations.md#r-annot.facts-of.target.error)). It also covers `facts_of` used as a value, as the old code covered `shape_of`. **Recommendation:** keep it; no existing code names a bad target. |
| FO-facts-type | batch 42 | [`annot.structure.facts-type`](../spec/14-annotations.md#r-annot.structure.facts-type) says a `Facts` holds the facts of a type, member, or variant, but `facts_of` also returns one for a function. | Left unchanged; [`annot.facts-of.result`](../spec/14-annotations.md#r-annot.facts-of.result) states the function case. **Recommendation:** widen it under a new ID only if one statement is wanted. |
| FO-traitless | batch 42 | [`annot.traitless.declaration-facts`](../spec/14-annotations.md#r-annot.traitless.declaration-facts) also said the field and variant shapes of `X` see a trait-less block's result. The record did not list it among the rules to reword. | Reworded to drop that clause, keeping its ID, since the clause named only removed values. **Recommendation:** keep it. |
| FO-prelude-list | batch 42 | [`module.prelude.annotation-targets`](../spec/10-modules.md#r-module.prelude.annotation-targets) lists the imported names of `std.annotation` and omits `facts_of`. | Unchanged; [`annot.facts-of.declared`](../spec/14-annotations.md#r-annot.facts-of.declared) states the import. **Recommendation:** add `facts_of` there under a new ID if chapter 10 should list every import. |

### Codes Waiting For The Code Revamp

These readings name a diagnostic code that no decision chose. Each waits
for the error-code revamp, task #101, which may merge codes.

| # | From | Question | Applied reading |
| --- | --- | --- | --- |
| AT-code | batch 20 | [`annot.walker.obligation.error`](../spec/14-annotations.md#r-annot.walker.obligation.error) gives `member-not-derivable` for a member that fails a source's bound. AT-with names `unsatisfied-trait-bound`, which [`std-testing.arbitrary.derive.not-derivable`](../spec/std/testing.md#r-std-testing.arbitrary.derive.not-derivable) states. So two rules name different codes for one check. | **Recommendation:** `member-not-derivable`, the code every other template reports at the opt-in, naming the member. |
| LP-codes | LP1 | The decisions name no codes for a refutable pattern without `else`, an `else` block that falls through, or a pattern before `:=`. | Two new codes, `refutable-let-pattern` and `let-else-falls-through` ([Let Patterns](../spec/06-control-flow.md#let-patterns)); a pattern before `:=` reuses `missing-let`. |
| TU2-code | batch 27 | TU2 names no code for `mut (A, B)`. | A new code, `mut-on-tuple`; `mut-on-primitive` would misname a tuple. |
| VA-type-code | batch 31a | The decisions name no code for a vararg of another type, as in `values...: i32`. | `type-mismatch` ([`fn.vararg.type.kinds`](../spec/07-functions.md#r-fn.vararg.type.kinds)). |
| Q3-codes | batch 32b | The record lists `data.embed.unique` and `trait.by.invalid` as error detail, but each is the only rule that names its code. | Both stay numbered; the other seven error-detail rules became Notes. |
| TR-code | batch 33a | A rest element that is not a `List`, as in `(i32, i32...)`, needs a code. | `type-mismatch` ([`types.tuple.rest.list`](../spec/04-type-system.md#r-types.tuple.rest.list)), as for a vararg of another type. Deferred to #101 by the owner (batch 34 Q7). |
| SC-Q2 | Special Cases Q2 | Four codes duplicate a partner: `suspending-defer`, `identity-needs-reference-bound`, `recursive-closure-needs-result-type`, and `mutable-embedded-field`. | All eight codes kept. **Recommendation:** merge all four into their partners; messages keep the context word. |
| SC-Q3 | Special Cases Q3 | Six codes report an operator with no meaning for its operands: `missing-eq`, `missing-partial-ord`, `unsupported-equality`, `nonnumeric-unary-plus`, `unsigned-negation`, and `mixed-numeric-types`. Every other operator reports `type-mismatch`. | All six kept. **Recommendation:** all six become `type-mismatch`, and `assert_equal`'s missing `Eq` becomes `unsatisfied-trait-bound`. |

### Ideas Noted For Later

None of these is decided.

- **A cover grammar for `:=`.** As in JavaScript, Python, Rust's
  destructuring assignment, and Elixir, it would let `:=` take patterns
  too. With it would come data-literal field shorthand, `Point { x, y }`
  for `Point { x: x, y: y }`, so a pattern and a literal read the same.
  The owner deferred both in batch 26 ("we can add in future").

### Bound And Row Operators

**Questions from applying them.** Each needs an owner answer; the spec
states the current behavior.

| Question | Applied now | Recommendation |
| --- | --- | --- |
| Does `$ A + B` inside `[...]` or a parameter list need precedence rules? | A row ends at the first `,`, `)`, or `]`; nested function types keep the innermost-owner rule | None needed. No ambiguity was found in type arguments, parameters, `$.Context[...]`, or closure headers. |
| Codes for other old row spellings | `$(A + B)`, `$(A)`, a `-` between keys, and the pre-2026-09-27 `Job[A + B]` and `$.Context[A + B]` are `syntax-error` | Keep `syntax-error`. Only the two decided codes carry fix-its. |
| `$.Context[$ A + B]` keeps its inner `$`, while one key is `$.Context[A]` | Kept: the context type takes a key or a row type argument | Keep it. It matches row type arguments such as `Job[$ A + B]`. |

### Mutable Host Providers

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

**Waiting on other areas.** The spec lists these as
[undecided parts](../spec/14-annotations.md#undecided-parts); each waits
for the owner, and Typed Derivation
gives their background:

| Question | What is undecided |
| --- | --- |
| Non-escaping handles (M18 R5) | Whether the parked NonEscapable design (TQ-24 to TQ-26) makes handles non-escaping. |
| `Clone`'s module (M24) | Which standard module declares `Clone`; chosen with the standard library (STDLIB). |
| Derived-function cache (M24) | The cache's API and module; chosen with the standard library (STDLIB). |
| Function targets | Deriving for functions, as tool adapters need ([parked](#parked-tool-adapters)). A decorator before a function attaches a plain value that `facts_of(f).find::[M]()` reads ([Function Facts](../spec/14-annotations.md#function-facts)). |

M30 deferred template constants, typed shared constants, and composing
templates until a real template needs them; they are not in the spec.

#### Parked: Tool Adapters

Parked with typed derivation (FN_TYPE decision 10); tools register
functions by hand for now. Background is in the archived
Nominal Function Types.

| Question | Options | **Recommendation** |
| --- | --- | --- |
| FN-Q9: how does a tool adapter get per-declaration data about a function? | A1, `shape_of(f)` passed beside the value; A2, a `fn_view(f)` intrinsic; B, per-declaration item types with a compiler-generated `FnStructure`, so `@derive(mcp.Tool)` works on functions. | B, or A2 if item types are too much surface. |
| FN-Q10: where are item types visible? | A, only where a generic parameter is inferred from the argument and in heads written `fn name`; B, everywhere, as in Rust. | A: bindings and list literals keep their function types. |

**Member-typed facts.** Testing AT-with (batch 20) chose option B, and
option D, member-typed facts, stayed open. Batch 36 (O7) then accepted
typed member facts, and batch 39 gave them their final form,
[Member-Typed Facts](../spec/14-annotations.md#member-typed-facts).

**Secret values (removed for now).** `Secret[T]` and `Redact` were removed
from the standard-library design as too early
(STDLIB decision 12, 2026-09-26). Revisit them
together with typed derivation. Options already discussed: whether standard
capability traits may take `Secret[T]` parameters so the host receives the
real value without an `expose()` in hd code; whether exported functions may
take `Secret[T]` inputs; and that a secret never encodes or appears in
outputs.

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
Replay Rules, replaces the `Durable` bound), and capturing a provider or mutable state is
rejected. The design still needs a record: the hash input, how a closure
opts in, and graph lifetimes.

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

The flat-stage iterator design waits for a specializing compiler, task
#86 (Iterator Performance Study).

### Testing Open Points

From the archived Testing Redesign.

- **Generator parameter style.** Generators take `mut Choices` today. The
  owner is comparing a requirement-row style, `fn() -> T $ Choices`. It
  waits for task #76, re-evaluation on a working compiler.
- **Two deferred fixtures** (T54). Test-layout fixture packages for
  `cyclic-test-dependency` and for a `tests` root inside `tests/` are
  added when those rules need coverage. The `# fixture-test-layout:`
  header exists ([Test Layouts](../spec/conformance/README.md#test-layouts)).

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([`data.repr.runtime-only`](../spec/08-data-and-enums.md#r-data.repr.runtime-only));
- the prototype's replay experiments in the
  Wasm GC compiler plan predate the
  decided Replay Rules: their
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
  Package Tooling;
- a `package-cycle` conformance fixture, which waits until the manifest
  schema exists (Dependency Cycles DC12,
  [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package));
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, as Replay Rules
  state);
- stateful property testing in `std.testing`, which waits for the event
  log. The property-test API is decided and applied
  (Testing PT1-PT9,
  [Property Tests](../spec/std/testing.md#property-tests));
- doc tests and benchmarks, which no decision covers yet. The testing
  stress test (TS-15) found a direction: doc tests as fenced `hd` blocks in
  `##` comments of `pub` items, run with the `tests/` view, and benchmarks
  with a host clock and their own registration, like Go's `b.Loop` or a
  `benches/` root;
- final signatures, behavior, and the complete intrinsic set for the
  compiler-intrinsic `std.task` combinators, such as racing, timeout,
  and heterogeneous scheduling (`retry!` is a library loop, decided in
  batch 29: [Task](../spec/std/task.md#retry));
- the final `std.task` structured-scope API: `Task[T]` is decided as
  structured scopes only, with `scope!`, `start`, and `join!`
  (STDLIB decision 11), and must not weaken
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
