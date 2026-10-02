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
| FIELD-VARIANCE | batch 64 | `Field[-S, +F]` had a covariant `F`, but `default` returns `F?`, and an optional is now invariant. | `F` is invariant ([`annot.handle.field-variance`](../spec/lang/14-annotations.md#r-annot.handle.field-variance)), so a declared-type handle no longer converts to its read-type handle; no fixture or template used that. **Recommendation:** keep it. The other answer changes `default`'s signature to restore `+F`. |
| ERROR-LINE-DEFAULT | batch 64 | The decision adds an error-line method to `Console`, but says nothing about providers written before it. | `write_error_line!` has a default body that calls `write_line!` ([`module.console.write-error-line.default`](../spec/lang/10-modules.md#r-module.console.write-error-line.default)), so every existing provider stays valid. **Recommendation:** keep the default. The other answer makes it required, which breaks every recording console. |
| INPUT-MUT | batch 64 | `ConsoleInput.read_line!` takes `self`, as `lib/std` has it. A scripted test provider then cannot advance through its lines. | Kept `self` ([`std-console.input.decl`](../spec/std/console.md#r-std-console.input.decl)). **Recommendation:** `mut self`, as `Console.write_line!` has: `fn read_line!(mut self) -> Result[string?, ConsoleError]`. |
| HOST-SURFACE | batch 64 | The sketches do not give every item a minimal trait needs. These are agent-chosen: `Instant::from_millis` and `Instant.since`, so a test `Clock` can build an `Instant`; `Path`'s `Eq` and `Display`; no order for `list_dir!`. | As listed, in [Time](../spec/std/time.md#timestamps-and-instants), [Path](../spec/std/path.md#paths), and [Fs](../spec/std/fs.md#reading). **Recommendation:** keep them; leave `list_dir!` unordered until a use needs an order. |
| HASH-SEED | batch 64 | `Map` now buckets by the fixed `DefaultHasher`. Nothing in `std` uses a runtime hash seed, yet [`trait.derive.hash.seeded`](../spec/lang/09-traits.md#r-trait.derive.hash.seeded) and [`req.determinism.hash-seeded`](../spec/lang/11-requirements-and-suspension.md#r-req.determinism.hash-seeded) still describe one. | Both kept. **Recommendation:** retire both rules and the hash-flooding Note beside the second. |
| HASH-BYTES | batch 64 | The algorithm is fixed, but which bytes each standard `Hash` implementation writes is not specified, so `hash_of(42)` may differ between implementations. | Unspecified. **Recommendation:** state `lib/std`'s encoding in [Hash](../spec/std/hash.md): UTF-8 for a string, little-endian bytes at its own width for an integer, one byte for a `bool`, and a list's length before its items. |

### Codes Waiting For The Code Revamp

These readings name a diagnostic code that no decision chose. Each waits
for the error-code revamp, task #101, which may merge codes.

| # | From | Question | Applied reading |
| --- | --- | --- | --- |
| AT-code | batch 20 | [`annot.walker.obligation.error`](../spec/lang/14-annotations.md#r-annot.walker.obligation.error) gives `member-not-derivable` for a member that fails a source's bound. AT-with names `unsatisfied-trait-bound`, which [`std-testing.arbitrary.derive.not-derivable`](../spec/std/testing.md#r-std-testing.arbitrary.derive.not-derivable) states. So two rules name different codes for one check. | **Recommendation:** `member-not-derivable`, the code every other template reports at the opt-in, naming the member. |
| LP-codes | LP1 | The decisions name no codes for a refutable pattern without `else`, an `else` block that falls through, or a pattern before `:=`. | Two new codes, `refutable-let-pattern` and `let-else-falls-through` ([Let Patterns](../spec/lang/06-control-flow.md#let-patterns)); a pattern before `:=` reuses `missing-let`. |
| TU2-code | batch 27 | TU2 names no code for `mut (A, B)`. | A new code, `mut-on-tuple`; `mut-on-primitive` would misname a tuple. |
| VA-type-code | batch 31a | The decisions name no code for a vararg of another type, as in `values...: i32`. | `type-mismatch` ([`fn.vararg.type.kinds`](../spec/lang/07-functions.md#r-fn.vararg.type.kinds)). |
| Q3-codes | batch 32b | The record lists `data.embed.unique` and `trait.by.invalid` as error detail, but each is the only rule that names its code. | Both stay numbered; the other seven error-detail rules became Notes. |
| TR-code | batch 33a | A rest element that is not a `List`, as in `(i32, i32...)`, needs a code. | `type-mismatch` ([`types.tuple.rest.list`](../spec/lang/04-type-system.md#r-types.tuple.rest.list)), as for a vararg of another type. Deferred to #101 by the owner (batch 34 Q7). |
| SC-Q2 | Special Cases Q2 | Four codes duplicate a partner: `suspending-defer`, `identity-needs-reference-bound`, `recursive-closure-needs-result-type`, and `mutable-embedded-field`. | All eight codes kept. **Recommendation:** merge all four into their partners; messages keep the context word. |
| SC-Q3 | Special Cases Q3 | Six codes report an operator with no meaning for its operands: `missing-eq`, `missing-partial-ord`, `unsupported-equality`, `nonnumeric-unary-plus`, `unsigned-negation`, and `mixed-numeric-types`. Every other operator reports `type-mismatch`. | All six kept. **Recommendation:** all six become `type-mismatch`, and `assert_equal`'s missing `Eq` becomes `unsatisfied-trait-bound`. |
| SINGLE-CODE | batch 46 | The decision names no code for a `pkg`, `dep`, `self`, or `super` use in a single-file program. | `unknown-module` ([`module.single-file.roots`](../spec/lang/10-modules.md#r-module.single-file.roots)), as a `super` above the test root is. |
| CLI-CODES | batches 46 to 48, 53 | These errors have no code: `hd run` or `hd build` outside a package, `hd check` or `hd test` without a FILE outside a package, `hd run FILE`, `hd run` with no or several executables, an unknown `NAME`, a workspace `NAME` that no member or several members have, a bare `hd run` at a workspace root, a task and an executable with one name, `hd new` over an existing file ([`cli.new.existing`](../spec/cli/command-line.md#r-cli.new.existing)), an unlisted `src/main.hd` beside `[[executable]]` tables ([`cli.exe.main-unlisted`](../spec/cli/command-line.md#r-cli.exe.main-unlisted)), an `x.hd` beside `x/` under `tests` or `tasks` ([`module.test.integration.beside-dir`](../spec/lang/10-modules.md#r-module.test.integration.beside-dir), [`cli.task.beside-dir`](../spec/cli/command-line.md#r-cli.task.beside-dir)), a directory or an extra word as a positional word, an unknown `-p` member, `hd test FILE` with no test case ([`cli.test.file-empty`](../spec/cli/command-line.md#r-cli.test.file-empty)), a `--filter` that matches no test case of a FILE ([`cli.test.filter.none`](../spec/cli/command-line.md#r-cli.test.filter.none)), a package under a workspace manifest that does not list it ([`cli.mode.member.unlisted`](../spec/cli/command-line.md#r-cli.mode.member.unlisted)), two dependency keys with one `dep.NAME`, a `src/mod.hd` ([`module.path.no-root-mod`](../spec/lang/10-modules.md#r-module.path.no-root-mod)), and a dependency on a package with no library ([`module.path.no-lib-dependency`](../spec/lang/10-modules.md#r-module.path.no-lib-dependency)). Readings from batches 47 and 53 wait here too: a use of `src/main.hd`, of another executable's entry module ([`cli.exe.entry-no-use`](../spec/cli/command-line.md#r-cli.exe.entry-no-use)), of an integration test program, or of a task from another module, and a `super` in a root file, report `unknown-module` ([`module.path.main-no-use`](../spec/lang/10-modules.md#r-module.path.main-no-use), [`module.test.integration.program-use`](../spec/lang/10-modules.md#r-module.test.integration.program-use), [`cli.task.program-use`](../spec/cli/command-line.md#r-cli.task.program-use)); and `cyclic-test-dependency` kept its name when `[test-dependencies]` became `[dev-dependencies]` ([`module.test.cyclic-dev-unit`](../spec/lang/10-modules.md#r-module.test.cyclic-dev-unit)). | None named for the CLI and manifest errors ([Command Line](../spec/cli/command-line.md)); `unknown-module` and `cyclic-test-dependency` kept. **Recommendation:** name the CLI and manifest errors with the manifest diagnostics, keep `unknown-module` with a message that says the file is a separate program or a root, and rename `cyclic-test-dependency` to `cyclic-dev-dependency`. |
| GR-24 | grammar audit | A line in a bracketed closure body that dedents to a column between the header and the body matches both [`lex.indent.unknown-column`](../spec/lang/01-lexical-structure.md#r-lex.indent.unknown-column) (`invalid-dedent`) and [`lex.closure.between`](../spec/lang/01-lexical-structure.md#r-lex.closure.between) (`syntax-error`). | The prototype reports `syntax-error`. **Recommendation:** `syntax-error`, the more specific rule; say so in a Note. |
| VIEW-CODE | batch 49 | The decision names no panic code for using an invalidated `ListView`. | `iterator-invalidated` ([`std-collections.view.invalid-use`](../spec/std/collections.md#r-std-collections.view.invalid-use)): the view checks the same structural-version counter as a list iterator, as Java's `subList` throws the same exception as its iterator. **Recommendation:** keep it, or rename the category `collection-invalidated` in the revamp. |
| VA-unbounded-code | batch 31a | An unbounded `Args` used as `Fn`'s inputs needs a code. | `generic-kind-mismatch`, as a non-tuple there already is, rather than `unsatisfied-trait-bound`. **Recommendation:** keep it; one rule covers both. |
| K1-code | batch 43 | The decision says a generic `find::[M]()` requires `M < Inspectable`, but names no code. | `unsatisfied-trait-bound` ([`annot.structure.find-key`](../spec/lang/14-annotations.md#r-annot.structure.find-key)), as for any type argument that fails a bound. **Recommendation:** keep it. |
| TY-29 | type audit | `trait-method-visibility`, `local-impl-nonlocal-pair`, `missing-partial-eq`, `missing-partial-ord`, `duplicate-annotation-impl`, and `overlapping-annotation-impl` appear in no chapter. | Settle each in the error-code revamp, task #101: give it a rule or merge it. |
| DEFAULT-CODE | batch 51 | The decision names no code for zero or several `@default` variants. | A new code, `invalid-default-variant` ([`std-ops.default.derive.one-variant`](../spec/std/ops.md#r-std-ops.default.derive.one-variant)), reported on the `@derive` line or the second `@default`. |
| RACE-PANIC | batch 51 | The decision leaves the panic code of a `race!` over a list that is empty at run time to the agent. | `explicit-panic` ([`req.combinator.race-empty-run`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-empty-run)), as for `take` with a negative count ([`std-iter.adapter.take.negative`](../spec/std/iter.md#r-std-iter.adapter.take.negative)). |

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
    println("working")    # valid: writes under the caller's driver
```

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
[Member-Typed Facts](../spec/lang/14-annotations.md#member-typed-facts).

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

**Parked questions.** The owner does not want to discuss NonEscapable now.
These are the initial answers, not to be applied until the owner reopens
the topic.

| # | Question | Initial answer |
| --- | --- | --- |
| TQ-24 | How does NonEscapable propagate to a type holding a NonEscapable field, as `data HiddenFile` with `file: File`? Automatically, like Rust's auto traits, or declared and checked? | Declared and checked: such a type must itself be declared NonEscapable, and a generic type is NonEscapable exactly when an argument is. Erasure to `Any`, or to a trait value whose trait does not extend NonEscapable, is rejected. |
| TQ-25 | Does a generic parameter accept a NonEscapable argument by default? | No: a parameter opts in, as Swift's `~Escapable` does, so existing generic code stays valid. |
| TQ-26 | How does a NonEscapable result, as in `fn first_line(file: File) -> Line`, say which parameters it depends on? | No NonEscapable returns for now; later, depend on every NonEscapable parameter, and name them only when a real API needs it. |

### Closure Shorthand

**Deferred (Pipe Operator PL10, 2026-09-29).** Closures stay
`fn(v): v * 2`; there is no `_` lambda shorthand, and no `f(_, a)` capture
(PL13). The pipe owns `_` inside a step
([Pipe Expressions](../spec/lang/05-expressions.md#pipe-expressions)), `it` is
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
- **A deferred fixture** (T54). A test-layout fixture package for
  `cyclic-test-dependency` is added when that rule needs coverage. The `# fixture-test-layout:`
  header exists ([Test Layouts](../spec/conformance/README.md#test-layouts)).

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([`data.repr.runtime-only`](../spec/lang/08-data-and-enums.md#r-data.repr.runtime-only));
- the prototype's replay experiments in the
  Wasm GC compiler plan predate the
  decided Replay Rules: their
  identity is per function rather than per program, their site IDs contain
  byte offsets, and they stop at the end of a history instead of resuming;
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the final `hd.toml` schema and the concrete host binding for capabilities
  such as `Console`. Executables and their selection are specified in
  [Command Line](../spec/cli/command-line.md#executables), and the traits
  the default profile binds in
  [Host Capabilities](../spec/cli/command-line.md#host-capabilities);
- dependencies through version control hosts, with no registry:
  Dependencies decisions DEP1-DEP7
  are applied in [Package Manifest](../spec/lang/10-modules.md#package-manifest)
  (version tags, minimal version selection, `hd.sum`, workspaces,
  pseudo-versions), with DEP8-DEP19 after them. The manifest diagnostics
  wait for the manifest schema (DEP14,
  [`cli.tooling.package-schema`](../spec/cli/command-line.md#r-cli.tooling.package-schema)),
  and the tooling work is in
  Package Tooling;
- conformance fixtures for `missing-entry-point` and `unselected-main`,
  which need manifest input in the fixture format, so they wait for the
  manifest schema like the other manifest diagnostics;
- the fuzzer ([spec/tools/fuzz](../spec/tools/fuzz/CONTRACT.md)) still runs
  `run FILE` and `build FILE`, which [Command Line](../spec/cli/command-line.md) makes
  package-only; the CLI update should move it to `hd FILE`;
- a `package-cycle` conformance fixture, which waits until the manifest
  schema exists (Dependency Cycles DC12,
  [`module.cycle.package`](../spec/lang/10-modules.md#r-module.cycle.package));
- whether a panic's source location is "available"
  ([`flow.panic.report`](../spec/lang/06-control-flow.md#r-flow.panic.report))
  for a trap inside a runtime helper, which decides whether a conformance
  runner can judge a panic marker's line (F-155 in
  [src/KNOWN_ISSUES.md](../src/KNOWN_ISSUES.md));
- whether the specification defines one portable "unsupported feature"
  diagnostic category, so a conformance runner can tell "not implemented"
  from "wrong" (F-250);
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
- the host extensions after the default profile, `Process` and an HTTP
  client, and the provider configuration format
  ([Host Capabilities](../spec/cli/command-line.md#host-capabilities));
- what `hd build` produces, and where (CLI-21): one Wasm file per
  executable, and anything for a library-only package, wait on package
  tooling and the Wasm boundary;
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
