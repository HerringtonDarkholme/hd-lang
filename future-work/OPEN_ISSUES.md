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

**Applied.** The owner's ten evening follow-ups (2026-09-29) are in the
specification:

| # | Decision | Where |
| --- | --- | --- |
| 1 | A multi-name `let` with `mut` needs parentheses: `let (mut log, db) = pair` | [Let Statements](../spec/02-grammar.md#let-statements) |
| 2 | `mut` on a primitive, and `let mut n = 0`, are `mut-on-primitive` | [`types.prim.no-mut.error`](../spec/04-type-system.md#r-types.prim.no-mut.error), [`types.bind.let-mut-primitive`](../spec/04-type-system.md#r-types.bind.let-mut-primitive) |
| 3 | `let mut u = find()` is valid when `find` returns `mut User?` | [`types.bind.let-mut-optional`](../spec/04-type-system.md#r-types.bind.let-mut-optional) |
| 4 | The redundant `mut` in `let mut a: mut T` warns | [`types.bind.let-mut-annotated.warning`](../spec/04-type-system.md#r-types.bind.let-mut-annotated.warning) |
| 5 | `m[k] op= v` on a `Map` reads the entry as `V` and panics when it is missing | [`expr.assign.compound.map-present`](../spec/05-expressions.md#r-expr.assign.compound.map-present) |
| 6 | Unwrapping a newtype carries its permission | [`types.newtype.unwrap-permission`](../spec/04-type-system.md#r-types.newtype.unwrap-permission) |
| 7 | The ten compound-assignment tokens are confirmed | [`grammar.stmt.compound-assign`](../spec/02-grammar.md#r-grammar.stmt.compound-assign) |
| 8 | Every decorator before a derivation block warns | [`annot.fact.unused-block-decorator.any-type`](../spec/14-annotations.md#r-annot.fact.unused-block-decorator.any-type) |
| 9 | The readable-targets Note covers enums | the Note under [Target Kinds](../spec/14-annotations.md#target-kinds) |
| 10 | Optionals need no conversion for an `AnyRef` parameter | [`types.trait.safe.convert-value`](../spec/04-type-system.md#r-types.trait.safe.convert-value) |

**Apply-pass answers (owner decisions, 2026-09-29).** Applied. The owner
answered the six points the apply pass raised:

| # | Decision | Where |
| --- | --- | --- |
| Let 1 | A multi-name `let` always uses parentheses, with or without `mut`: `let (a, b) = pair` is valid, and `let a, b = pair` is a `syntax-error` with a fix-it. A list still needs two names; `a, b := pair` is unchanged. The owner picked this over the recommendation, since one shape beats two spellings. | [`grammar.stmt.let-list`](../spec/02-grammar.md#r-grammar.stmt.let-list), [`grammar.stmt.let-list.bare`](../spec/02-grammar.md#r-grammar.stmt.let-list.bare), [`grammar.stmt.let-list.two-names`](../spec/02-grammar.md#r-grammar.stmt.let-list.two-names) |
| Let 2 | `let mut log, db = pair` stays a `syntax-error`, with a fix-it that adds the parentheses | [`grammar.stmt.let-list.bare`](../spec/02-grammar.md#r-grammar.stmt.let-list.bare) |
| Let 3+4 | `mut` on a primitive is `mut-on-primitive` wherever the type is written, `let mut n: i32 = 0` and `x: mut f64` included | [`types.prim.no-mut.error`](../spec/04-type-system.md#r-types.prim.no-mut.error), [`types.bind.let-mut-primitive`](../spec/04-type-system.md#r-types.bind.let-mut-primitive) |
| Let 5 | The warning keeps the name `redundant-let-mut` | [`types.bind.let-mut-annotated.warning`](../spec/04-type-system.md#r-types.bind.let-mut-annotated.warning) |
| Map 6 | A missing key in `m[k] op= v` panics with `index-out-of-bounds`; no new category | [`expr.assign.compound.map-missing`](../spec/05-expressions.md#r-expr.assign.compound.map-missing) |

**Still open (raised by applying Let 1).** The spec keeps the current
reading; nothing here is decided.

| Point | Applied | **Recommendation** |
| --- | --- | --- |
| `let (a, b) = pair` in a same-line suite, as `if ok: let (a, b) = pair` | `syntax-error`: [`grammar.inline.multi-name-let-for`](../spec/02-grammar.md#r-grammar.inline.multi-name-let-for) still asks for an indented body | Allow it. The rule exists because a bare comma closes a same-line suite, and the parentheses remove that comma, as `(a, b := pair)` already shows. |

### Local Mutability: `let mut` As An Inference Helper

The owner's decision (2026-09-29) is applied as
[Let Statements](../spec/02-grammar.md#let-statements) and
[Binding Forms](../spec/04-type-system.md#binding-forms), with the code
`let-mut-readonly-type`. The four questions its apply pass raised were
answered by the [evening follow-ups](#follow-ups-decided-2026-09-29-evening)
1-4, which are applied.

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
separate `@error` intrinsic, listed below.

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

### Error Derivation (`@error`)

**Decided 2026-09-27, not yet applied.** Error Conversion decision 10 makes
error derivation one compiler intrinsic, `@error`, Rust's `thiserror`
moved into hd: messages, `@from`, `@source`, and `@error(transparent)`.
Chapter 14 mentions it only in prose, as an intrinsic beside `@derive`.
The decision text and what the apply pass must specify are in
[Error Conversion](ERROR_CONVERSION.md#owner-decisions).

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

### Associated Type Bindings Beyond Direct Bounds

**Problem.** An associated type binding such as `I < Supplier[Item = T]`
names only an associated type the bound trait itself declares, and it is
accepted only in generic parameter bounds and supertrait lists. Binding a
supertrait's associated type through a subtrait
(`I < NamedSupplier[Item = T]`) is rejected, and dynamic trait value types
and `impl` headers take no bindings.

**Options.** (1) Keep the current rule; users add a separate bound on the
supertrait. (2) Let a binding name any associated type reachable through the
supertrait graph, rejecting ambiguous names. (3) Also accept bindings in
supertrait lists, so `trait Names < Supplier[Item = string]` fixes the item
type for every implementation.

**Recommendation.** Option 1 until a library needs option 2.

**Option 3 decided and applied (2026-09-29)** by
Operator Traits OP8: a supertrait list
binds associated types, as in `trait Summable < Add[Self, Out = Self]`
([Supertrait Bindings](../spec/09-traits.md#supertrait-bindings)).

**Option 2 and a trait-value extension decided by the owner (2026-09-29)
and applied (2026-09-29):** [Binding Names](../spec/09-traits.md#binding-names),
[Binding Positions](../spec/09-traits.md#binding-positions), and
[Bound Associated Types](../spec/09-traits.md#bound-associated-types).

1. **AT1 (option 2, recommended): a binding may name an associated type
   reachable through the bound trait's supertraits,** as in Rust:
   `I < NamedSupplier[Item = T]`. When two supertraits declare the same
   name, the binding is ambiguous and an error, reusing an existing code if
   one fits. AT1 retires `trait.binding.own-trait` and
   `trait.binding.supertrait`.
2. **AT2 (trait value types bind associated types; the owner chose it over
   the recommendation):**
   - A trait value type may bind associated types, as Rust's
     `dyn Iterator<Item = T>` does. `Supplier[Item = i32]` is a trait value
     type when every associated type is bound, including those that
     supertraits declare.
   - A trait value that leaves an associated type unbound stays not
     dynamically safe, and associated functions stay banned.
   - When a trait value satisfies its own trait's bound, `T::Item` is the
     bound type.
   - AT2 relaxes `trait.dyn.safe.no-assoc` and `trait.binding.rejected-other`
     for trait value types only. `impl` headers and trait-qualified calls
     still reject bindings.
   - The owner asked why hd could not do this while Rust can. The answer:
     it had simply not been added, since the binding makes every signature
     concrete. The specification's Why note says so.

**Decided 2026-09-29, answering the four points of the AT apply pass.**
AT3 and AT5 are not the recommendation.

3. **AT3 (point 1; the owner chose it over the recommendation): a new
   code, `ambiguous-associated-type`,** reports a binding name that two
   supertraits declare, instead of `ambiguous-method`. The ambiguous
   projection rule (`trait.binding.ambiguous`: `I::Item` with two bounds
   that both declare `Item`) uses the new code too if the specification
   text shows it is clearly the same case. Otherwise it stays, and the
   question goes to Still Open.
4. **AT4 (points 2 and 4): kept and confirmed as applied.**
   `List[i32, Item = i32]` is `unknown-associated-type`. Widening a bound
   trait value to a supertrait keeps the bindings that the supertrait
   reaches.
5. **AT5 (point 3; the owner chose it over the recommendation):
   requirement keys accept associated type bindings now,** as bounds do:
   `fn load() -> User $ Store[Item = User]`.
   - This covers rows, row aliases, row subsumption, and provider
     satisfaction: a provider must bind the associated type to the stated
     type.
   - Grammar: a requirement key becomes the binding-capable trait form.

**AT3-AT5 applied (2026-09-29).** AT3 is
[`trait.binding.name-reach.ambiguous-type`](../spec/09-traits.md#r-trait.binding.name-reach.ambiguous-type),
with `ambiguous-associated-type` in the diagnostic table. AT4 changed
nothing. AT5 is [Bound Requirement Keys](../spec/11-requirements-and-suspension.md#bound-requirement-keys),
[`req.row.alias.bound-key`](../spec/11-requirements-and-suspension.md#r-req.row.alias.bound-key),
[`req.with.type.binding`](../spec/11-requirements-and-suspension.md#r-req.with.type.binding),
[`req.with.collision.bindings`](../spec/11-requirements-and-suspension.md#r-req.with.collision.bindings),
and [`grammar.type.row.key-binding`](../spec/02-grammar.md#r-grammar.type.row.key-binding).

**Still open from the AT3-AT5 apply pass.** Each point waits for the owner.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 5 | Is an ambiguous projection the same case as AT3's ambiguous binding name? [`trait.assoc.ambiguous`](../spec/09-traits.md#r-trait.assoc.ambiguous) and [`trait.binding.ambiguous`](../spec/09-traits.md#r-trait.binding.ambiguous) cover `I::Item` when two bounds on `I` declare `Item`. The specification keeps them apart: they concern a projection, not a binding name, and name no code. Audit finding TY-32 proposes a separate `ambiguous-projection`. | Not changed: the projection rules still name no code | Use `ambiguous-associated-type` for both, and close TY-32 with it. In both cases one associated type name selects two declarations. |
| 6 | Must a requirement key bind every associated type of its trait? `$ Store` with `Store` declaring `Item` is not addressed. | [`req.key.binding.identity`](../spec/11-requirements-and-suspension.md#r-req.key.binding.identity) makes `Store` and `Store[Item = User]` different keys; nothing rejects the unbound one | Yes: an unbound key is `trait-not-dynamically-safe`, as its trait value type is, because its provider value has that type ([`req.use.value.ordinary`](../spec/11-requirements-and-suspension.md#r-req.use.value.ordinary)). |

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

### Literal Suffixes

**Decided and applied.** Owner decisions L1-L22: a suffix is a function
marked `@num_suffix`, and `250ms` calls `ms(250)`
([Literal Suffixes](../spec/05-expressions.md#literal-suffixes)); a string
prefix is a function marked `@str_prefix`
([Prefixed Strings](../spec/05-expressions.md#prefixed-strings)).

**Closed.** The last three points (16, 17, 23) are decided; see
[Literal Suffixes](LITERAL_SUFFIXES.md#status). Nothing remains to apply.

### Operator Traits

**Decided and applied.** Owner decisions OP1-OP13: Rust-shaped `std.ops`
traits with an associated `Out`, std implementations for the primitive
numbers, `a op= b` meaning `a = a op b`, `Index` and `IndexSet`,
supertrait `Out` bindings, and the sealed `std.num` traits
([Operator Traits](../spec/05-expressions.md#operator-traits),
[Compound Assignment](../spec/05-expressions.md#compound-assignment),
[Numeric Traits](../spec/09-traits.md#numeric-traits)). Type-Argument
Defaults TD10 gives each binary operator trait `Rhs = Self`
([`expr.op.trait.shape-default`](../spec/05-expressions.md#r-expr.op.trait.shape-default)).

**Open.** Three points from the apply passes, each with a recommendation,
are in [Operator Traits](OPERATOR_TRAITS.md#still-open): compound
assignment's tokens and suite right sides, `counts[w] += 1` on a built-in
`Map`, and the permission of an unwrapped newtype.

### Iterator Adapters

**Decided and applied (owner, 2026-09-29).** [Chaining Study](CHAINING_STUDY.md#owner-decisions)
CS7 and CS8 make `Iterator[T]` a concrete data type holding a `step`
closure, so `map` and `fold` are ordinary methods beside `filter`, `take`,
`enumerate`, and `collect`, and `for` uses the prelude `Iterable[T]`
([Iteration Protocols](../spec/06-control-flow.md#iteration-protocols),
[Iterator Adapters](../spec/06-control-flow.md#iterator-adapters)).

**Open.** Three readings, each with a recommendation, are in
[Chaining Study Still Open](CHAINING_STUDY.md#still-open): how user code
builds an iterator, readonly `iter()` on an iterator, and exhaustion of a
user source.

### Pipe Operator

**Decided and applied (owner, 2026-09-29).** [Pipe Operator](PIPE_OPERATOR.md#owner-decisions)
PL3-PL13, as amended by [Chaining Study](CHAINING_STUDY.md#owner-decisions)
CS2, are in [Pipe Expressions](../spec/05-expressions.md#pipe-expressions):
a step is a `_` step or a bare name or path, and `_` means nothing outside
a step. PL1 and PL2 are superseded by CS7.

**Open.** Three readings, each with a recommendation, are in
[Pipe Operator Still Open](PIPE_OPERATOR.md#still-open). CS6 (key-function
helpers) is std-only, in [STDLIB](STDLIB.md#stditer).

### Method And Field References

**Decided and applied (owner, 2026-09-29).** [Method References](METHOD_REFERENCES.md#owner-decisions)
MR1-MR4 are in [Method References](../spec/07-functions.md#method-references):
`Type::name` is unbound and receiver first, `value::name` captures its
receiver when made, fields have no reference form, and a reference is a
bare pipe step. The three points left to the apply pass are applied as
the record recommends; see its [Still Open](METHOD_REFERENCES.md#still-open).

### Collecting Iterators

**Decided and applied (owner, 2026-09-29).** [Collect](COLLECT.md#owner-decisions)
CO1-CO4 are in [Collect Targets](../spec/06-control-flow.md#collect-targets)
and [Comprehension Restrictions](../spec/05-expressions.md#comprehension-restrictions):
a generic `collect` over `FromIterator`, last-wins keys, std-only helpers,
and `?` inside comprehensions.

**Decided and applied (owner, 2026-09-29).** CO5 and CO6: `collect`
declares its target's default, `collect[C < FromIterator[T] = List[T]]`,
through the general
[Type-Argument Defaults](../spec/04-type-system.md#type-argument-defaults)
of [TD1-TD7](TYPE_ARG_DEFAULTS.md#owner-decisions), and `FromIterator` is
imported. Three points of the defaults apply pass are in
[Type-Argument Defaults Still Open](TYPE_ARG_DEFAULTS.md#still-open).

### Iterator Performance

**Open.** [Iterator Performance Study](ITERATOR_PERF.md) compares the
CS7 closure iterator, Rust-style nested adapters, and a flat iterator with
one composed stage. It asks five questions and specifies stage 2
benchmarks, which a cheap-model agent runs next. CS7 stands until the
owner decides otherwise.

### Cross-Feature Stress Test (2026-09-29)

**Closed.** The two readings of stress decisions 5 and 6 were confirmed
by the [evening follow-ups](#follow-ups-decided-2026-09-29-evening) 8
and 9, and are applied.

### Stale Wording

**Applied.** The one entry, `types.trait.safe.convert`, was fixed by
evening follow-up 10: it is retired, and
[`types.trait.safe.convert-value`](../spec/04-type-system.md#r-types.trait.safe.convert-value)
lists only primitive and tuple values.

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
  [Dependencies decisions DEP1-DEP7](DEPENDENCIES.md#owner-decisions)
  are applied in [Package Manifest](../spec/10-modules.md#package-manifest)
  (version tags, minimal version selection, `hd.sum`, workspaces,
  pseudo-versions). Nine points the apply pass met wait for the owner in
  [Dependencies Still Open](DEPENDENCIES.md#still-open), and the tooling
  work is in [Package Tooling](RUNTIME_AND_LIBRARY.md#package-tooling);
- a `package-cycle` conformance fixture, which waits until the manifest
  schema exists (Dependency Cycles DC12,
  [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package));
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, as [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules)
  state);
- property-testing strategies, shrinking, replay artifacts, and correlated or
  stateful generators in `std.testing`;
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
