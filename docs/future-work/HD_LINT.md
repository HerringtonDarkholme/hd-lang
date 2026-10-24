# hd lint

Design proposal, 2026-10-23; not accepted. This revision addresses
[#651's review](https://github.com/HerringtonDarkholme/hd-lang/issues/651#issuecomment-6372359499)
and the [fix-command correction](https://github.com/HerringtonDarkholme/hd-lang/issues/651#issuecomment-6372223224).
It changes no specification, compiler, or accepted behavior.

## Contents

- [Purpose and current behavior](#purpose-and-current-behavior)
- [Other tools](#other-tools)
- [Semantics](#semantics)
- [First lint](#first-lint)
- [Later lint: lost write to a `mut` parameter](#later-lint-lost-write-to-a-mut-parameter)
- [Implementation choices](#implementation-choices)
- [Architecture](#architecture)
- [Command and configuration](#command-and-configuration)
- [Output and status](#output-and-status)
- [Custom lints](#custom-lints)
- [Fixing is an open option](#fixing-is-an-open-option)
- [Cost](#cost)
- [Stress test](#stress-test)
- [Implementation order and validation](#implementation-order-and-validation)
- [Recommendation](#recommendation)
- [Evidence and parse log](#evidence-and-parse-log)

## Purpose and current behavior

A lint reports a likely mistake or avoidable cost in an otherwise accepted
program. The owner keeps this advice separate from type checking.
Allowing a lint never makes an ill-typed program valid.

The fixed users and use cases are:

| Use case | Complete proposed interaction |
| --- | --- |
| U1: an agent investigating a lost update | Run `hd lint src/geometry.hd --format json`; read the actual receiver span and method name; bind a value if retaining it was intended; rerun check and lint. |
| U2: package CI | Put `lost-temporary-write` in the package's deny list; run `hd lint --all`; a denied finding exits 101; a documented site suppression permits intentional code. |
| U3: a writer using shared storage | Run lint on an Appender containing a reference; receive no lost-write finding; its output remains visible through that reference. |
| U4: a developer investigating string construction | Run `hd lint src/text.hd --warn repeated-string-concat`; read a performance finding on repeated concatenation; inspect intermediate observers; rewrite manually only after checking behavior and measuring the actual workload. |
| U5: a team enforcing a domain rule | Initially contribute a built-in pass; a later explicit facts export would let its own offline tool reject forbidden calls without a compiler plugin. |

Today `hd check` checks without running the program and reports
[JSON diagnostics](../../spec/cli/command-line.md#machine-output).
`hd fix` applies compiler fix-its under the existing
[`cli.fix.safe`](../../spec/cli/command-line.md#r-cli.fix.safe) rule.
Whether any fix command should remain is undecided in
[#805](https://github.com/HerringtonDarkholme/hd-lang/issues/805).

A fresh dev build at `a00c6b374358fb2992f046695ea0f6639b5ab542`
listed `check` and `fix`, without `lint`.
Invoking `hd lint` returned usage and status 101.
Consequently all lint findings below are proposed judgments, never output
from an existing lint implementation.

## Other tools

### Rust and Clippy

Rust separates early syntax passes from late passes with type information.
The lint store registers passes and lint definitions separately; a pass can
emit more than one lint.
This is the useful precedent for sharing resolved facts without making
lint findings ordinary type errors.

Clippy's command exposes allow, warn, and deny policy, including a
performance group. It integrates with rustc rather than reconstructing
Rust's type system from text.
That integration gives precision but couples passes to compiler internals;
it does not provide hd with a ready-made stable plugin ABI.
See the [compiler guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html),
[lint store](https://rustc-dev-guide.rust-lang.org/diagnostics/lintstore.html),
and [Clippy usage](https://doc.rust-lang.org/stable/clippy/usage.html).

### Go analysis and vet

Go's analysis API supplies syntax, type information, declared prerequisite
analyses, diagnostics, and optional cross-package facts.
Drivers choose how to run analyzers; analyzers do not need to implement
package loading themselves.
This separates reusable analysis from command policy.

The `lostcancel` pass depends on inspection and control-flow analyses.
It distinguishes variable objects, walks paths, and avoids treating nested
function bodies as the containing function's ordinary execution.
Its approximation counts some references as uses; this is useful advice,
not a complete proof of every cancellation obligation.
See [analysis](https://pkg.go.dev/golang.org/x/tools/go/analysis) and
[lostcancel](https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/lostcancel).

For hd, that precedent argues for explicit fact dependencies and binding
identities. It does not justify importing Go's control flow: hd cleanup,
propagation, suspension, and cancellation have their own edges.

### ESLint

An ESLint rule registers visitors and reports diagnostics through a context.
Its scope and code-path interfaces support analyses beyond token matching.
Fixes are edit ranges; overlapping edits and repeated passes require
coordination, and suggestions differ from automatic fixes.

JavaScript plugins are executable code loaded by the tool. That flexibility
also means executing project-supplied code carries the host process's trust.
An AST visitor alone does not provide resolved hd receiver permissions,
associated types, or requirement rows.
See [custom rules](https://eslint.org/docs/latest/extend/custom-rules) and
[code-path analysis](https://eslint.org/docs/latest/extend/code-path-analysis).

### Ruff

Ruff separates diagnostic selection from fix applicability. Its safe fixes
intend to preserve behavior; unsafe ones can change behavior or comments.
Its documented iterable example changes the exception category despite
improving performance, demonstrating why speed is insufficient evidence.

hd should preserve that separation, without treating Ruff's confidence
label as a proof system. This proposal offers no unsafe-fix opt-in and
makes no fix command a decided feature.
See [Ruff's fixes and safety](https://docs.astral.sh/ruff/linter/#fix-safety).

### Dylint

Dylint loads Rust lint libraries built for their associated compiler
toolchains. It allows independently distributed passes without a Clippy fork,
but exposes substantial compiler integration and toolchain coordination.
A native-library mechanism for hd would additionally grant in-process code
all compiler-process authority.
See [Dylint](https://github.com/trailofbits/dylint).

### Coverage carried into this proposal

| Domain item | Treatment |
| --- | --- |
| Resolved types, scope identity, and control flow | Explicit checked facts; never reconstruct types from names or text. |
| Levels, CI policy, and suppressions | Exact lint names, deterministic policy after analysis, statement-scoped ordinary comments. |
| Performance advice | Separate category and report-only concatenation candidate; neither speed nor an empty row proves a safe edit. |
| Custom rules | Built-ins first; explicit versioned data export is the preferred later extension. |
| Fixes and conflicting edits | Open option with a proof obligation and transactional validation; no edit engine in the initial command. |
| Incrementality | Reuse normal prerequisites; recheck selected bodies; measure before persisting lint facts. |
| Interprocedural effect/escape analysis | Excluded from the first lint: the owner's predicate is header-based. |
| Full symbolic execution and termination proofs | Excluded: unnecessary for advice and not generally decidable. |

## Semantics

A *place* identifies existing storage. A *temporary* is a computed value
whose automatic address creates a fresh object for the method call.
An empty requirement row says which providers a callable needs, not that
it is pure, terminating, or unable to retain a reference.

### Types, permissions, and requirements

The analysis uses semantic types, not RC layouts or visible field counts.
A string's private allocation does not make it a language reference.
An opaque host handle must remain observable indirection even if its
source declaration has no visible fields.

| Concept and normative basis | Handling | Counterexample to a naive lint |
| --- | --- | --- |
| Value copies: [value semantics](../../spec/lang/04-type-system.md#r-types.value.copy), [collections](../../spec/lang/04-type-system.md#r-types.coll.value) | Preserve value-versus-place classification from checking. Copies of collections are semantic values even when their representation shares memory. | `points[0].shift(+1)` updates an element place; reading `p` into `points` earlier made a separate value (S2). |
| Reference receivers: [automatic addressing](../../spec/lang/07-functions.md#reference-receivers) | Inspect the resolved receiver permission and the pre-address origin. | `alias.shift(+1)` changes an existing Point; a call expression returning `&mut Point` also names existing storage. |
| Aliases: [places and new objects](../../spec/lang/13-values-and-references.md#places-and-new-objects) | Any semantic reference at any depth excludes the first lint. `&mut` is not exclusive ownership. | Appender's reference field updates `values` despite its temporary wrapper (S5). |
| Generics: [generic types](../../spec/lang/04-type-system.md#generics), [generic functions](../../spec/lang/07-functions.md#generic-functions) | Normalize substituted types; an open parameter yields Unknown. Lint a generic source body once, without enumerating monomorphizations. | `Box[T].touch()` cannot assume `T` lacks references merely because one caller supplies an integer (S6). |
| Associated types: [associated types](../../spec/lang/09-traits.md#associated-types) | Normalize projections with checker evidence; unresolved projections remain Unknown. | `T::Item` could be a reference in another implementation (S6). |
| Trait values: [trait value references](../../spec/lang/09-traits.md#trait-value-references) | Recognize erased references, including nested ones; do not infer concrete payloads from one runtime construction. | A Box containing `&dyn Probe[Item = i32]` can reach shared state (S6). |
| Aggregate types: [data and enums](../../spec/lang/08-data-and-enums.md) | Inspect every field, tuple component, enum payload, and collection argument after substitution, including private fields. | `.Empty` is not proof that its enum type has no reference-bearing variant (S15). |
| Function values: [captures](../../spec/lang/07-functions.md#captures) | Function-typed members or arguments yield Unknown when their environment is not established reference-free; v1 skips all function types. | An empty-row callback mutates `total` through its captured cell (S8). |
| Defaults: [default values](../../spec/lang/07-functions.md#default-values) | Examine every formal parameter's substituted type, even when omitted; preserve evaluation order and effects. | A default may mutate globals despite requiring no provider. Omitting it cannot justify deleting the call. |
| Rows and bang: [row omission](../../spec/lang/11-requirements-and-suspension.md#requirement-rows) and [suspending functions](../../spec/lang/07-functions.md#declarations) | Resolve inferred rows completely; require the callee row itself to be empty and the call non-bang. | An outer `$.with` satisfying Console does not make a Console-requiring method pure; S13 remains quiet. |

The recursive type classifier returns Clear, Observable, or Unknown.
Scalars and strings are Clear; references, trait values, and host handles
are Observable. Aggregates are Clear only when every semantic child is Clear.
Parameters, unresolved projections, function types, poison, missing metadata,
and recursion cycles are Unknown in v1.

A memo indexed by normalized type plus the checking environment prevents
exponential repeated walks. An active recursion entry returns Unknown,
not Clear; no fuel exhaustion turns an unproved type into a warning.
Internal metadata is available across package privacy boundaries but is
not automatically exposed by a future public facts format.

### Bindings, scopes, and generated code

[Lexical scopes](../../spec/lang/03-names-and-scopes.md#lexical-scopes)
assign distinct identities to shadowed bindings.
The initializer precedes its binding's visibility; matching text is not
sufficient to identify the value being assigned or observed.
In S3, changing the inner `p` must not be attributed to the outer `p`.

[Shared captures](../../spec/lang/07-functions.md#copied-and-shared-captures)
retain variable identity across closures. A closure body has a separate
execution context, even when its declaration appears inside a loop.
S9's `snapshot` observes `out` through its cell, defeating a string-rewrite
analysis which only searches the loop for the text `out`.

Use resolved declaration IDs for imported aliases, promoted methods, and
trait-selected calls, with source provenance for the actual receiver.
Do not recognize `StringBuilder` or a method named `push` by spelling alone.
Generated wrappers and derive expansion bodies receive no independent
user finding; a source call of a generated method can still qualify.

The lint selects the same package, file, and test scopes as check.
It may inspect dependency signatures and private type structure needed
for correctness, but reports only selected source sites.
A public API can therefore be linted without executing dependency code.

### Control flow and observations

The first lint is a local signature heuristic, not a liveness analysis.
It reports each qualifying checked source call once, including calls in
branches or loops, without claiming the call executes on every path.
It does not add a reachability solver or duplicate unreachable-code diagnostics.

Performance advice needs more context. A future control-flow view must
represent source reads, writes, escapes, and exceptional exits, rather
than only successful straight-line instructions.

| Construct and rule | Required treatment | Naive failure |
| --- | --- | --- |
| [Branches and matches](../../spec/lang/06-control-flow.md#conditional-expressions) | Inspect both checked arms; merge identities and observations conservatively. | An observer in one arm still prevents delaying all string writes. |
| [Loops](../../spec/lang/06-control-flow.md#for-loops), break, continue | Track loop identity and backedges; a break targets its owning loop, never a nested function's loop. | S10 observes a prefix on break, and skips one item on continue. |
| [Return](../../spec/lang/06-control-flow.md#return) | Include each early exit and evaluate its result before cleanup. | S10 returns `a` before its final iteration. A post-loop-only materialization changes that result. |
| [Propagation](../../spec/lang/05-expressions.md#propagation) | Model both normal and propagating edges. | S10 returns None through `?`, yet its defer still records `a`. |
| [Defer](../../spec/lang/06-control-flow.md#registration-and-order) | Keep registration separate from execution; cleanup reads captured storage at exit in reverse registration order. | S10's deferred observer requires the current prefix on every exiting path. |
| [Using](../../spec/lang/06-control-flow.md#using-bindings) | Keep initialization, registration, and potentially suspending disposal edges. | S11's implicit dispose is an observable call, even without an explicit call expression at the exit. |
| [Suspension and cancellation](../../spec/lang/11-requirements-and-suspension.md) | Treat each suspension as potentially leaving via cancellation; preserve provider and cleanup context. | Deleting or moving a bang call can change which dispose runs and when; S11 must stay quiet. |
| [Panics](../../spec/lang/06-control-flow.md#panic-behavior) | Do not convert a runtime panic into normal completion or ordinary defer execution. | S12 has an empty row and a lost receiver, but must still panic. |
| [Element access](../../spec/lang/13-values-and-references.md#element-references) | Retain call-bounded access and runtime pinning semantics. | Replacing an element-place call with a local-copy call can change the collection and its pinning failures. |

The proposal requires no new ownership or move rule. hd values copy;
backend move/borrow optimizations are not a source-level license to drop
receiver construction, argument evaluation, a method body, or cleanup.

## First lint

Proposed code: `lost-temporary-write`, category likely-bug, default warning.
Keep the owner's tightened [header predicate](https://github.com/HerringtonDarkholme/hd-lang/issues/651#issuecomment-6369394128):

| Required fact | How it is established |
| --- | --- |
| Fresh temporary receiver | The successful checker path created an automatic address for a non-place value; a preexisting reference is excluded. |
| Writable reference receiver | Resolved method receiver is `&mut self`, rather than `self`, `mut self`, or `&self`. |
| Void result | Normalized result is exactly void, with no unresolved inference. |
| Empty row | The resolved method signature has no keys or open row tail. |
| Non-bang call | The source call does not suspend. |
| Receiver lacks observable indirection | Classifier returns Clear on the complete receiver referent type. |
| Every ordinary formal lacks observable indirection | Classifier returns Clear on all substituted formal types, including defaulted and vararg types. |
| Valid source | The call resolved successfully; poison, incomplete facts, and generated-only sites produce no finding. |

The method's *permission* to write is known; an actual write is not.
It can also store `self` into mutable top-level storage despite this header.
That accepted false positive is demonstrated by S7 and can be suppressed.
Neither fact permits an automatic fix or a categorical claim of uselessness.

Use the actual method name and underline its receiver expression.
Proposed wording: "`shift` takes a mutable temporary receiver; bind the value
if you meant to keep the change". This does not assert that the body wrote.
The finding contains no edit and invents no variable name.

## Later lint: lost write to a `mut` parameter

Proposed code: `lost-write`, category likely-bug, default warning. The owner moved this check out of the compiler on 2026-10-23: the language spec no longer has `fn.param.mut.lost-write`, and the checker no longer reports it. It belongs here, because whether a change to a function's own copy is a mistake depends on the program, not on the rules.

A `mut` parameter or a `mut self` receiver is the function's own copy (`fn.param.mut.copy`, `fn.method.receiver.mut-self`), so a change to it never reaches the caller. The lint reports a change whose copy is never observed afterwards. That covers whole reassignment, a struct field write, a `push`, a map store, an element store and a `&mut self` method call on the copy (#817).

It does not report a change that the function itself observes later. A read, a print, a return, a store, a pass to another call, or a capture by a closure or `defer` all count as observing it:

```text
fn report_len(list: mut List[i32]) -> void $ Console:
    list.push(123)
    println(list.len())    # the copy is read: no finding

fn add(list: mut List[i32]) -> void:
    list.push(123)         # finding: the copy is dropped unread
```

The compiler's version judged only statements directly in the body and stayed silent when any later statement named the parameter. The lint keeps that conservative form first, and can widen to branches and loops later. The message names the parameter, and the help suggests `&mut T` or `&mut self` when the caller's value was meant to change. It offers no fix.

## Implementation choices

The fixed use cases U1-U5 apply to each mechanism.
None needs new hd syntax, an intrinsic, or a std library API.
Under AGENTS.md's cost order, all are compiler/tooling additions; their
cost difference is fact collection and persistence, not language machinery.

**A: collect typed source facts during a lint-requested check.** Capture
receiver origin while resolving calls, then finalize types after inference.
Run a separate analysis over those facts; reuse the normal diagnostic renderer.
This handles U1-U3 faithfully, supports report-only U4, and leaves U5 to a later export.

**B: persist a checked-source fact graph.** Export typed calls, bindings,
uses, and control edges for a separate query consumer.
U1-U4 can work and U5 improves, but schema evolution, private metadata,
source relocation, and invalidation become first-release obligations.
There is no existing general snapshot API in `hd_driver::Output`.

**C: scan syntax only, the smallest mechanism.** An independent scanner
can detect repeated spelling such as `out = out + part`.
It can support explicitly approximate U4 advice but cannot establish U1's
receiver permission, empty row, or recursive type exclusions.
It misclassifies U3, so it cannot implement the first lint as requested.

| Criterion | A: ephemeral typed facts | B: persisted graph | C: syntax scanner |
| --- | --- | --- | --- |
| U1 / U2 | Correct predicate and shared policy; selected bodies rechecked | Same, if all keys invalidate correctly | Cannot meet predicate |
| U3 | Quiet from type and origin facts | Quiet from stored facts | Reference wrappers misidentified |
| U4 | Typed report, no fix | Same, with stronger later analyses | Approximate report only |
| U5 | Built-in contributions first | Offline custom queries | Only textual custom rules |
| Soundness limit | Heuristic warning; Unknown skips | Same plus stale-fact risks | Different, weaker rule |
| Author and reader cost | No hd forms; conditional message | Same plus schema documentation | Easy to author, misleading precision |
| First implementation | Collector, classifier, policy | Collector plus graph and serialization | Small but insufficient |
| Migration | Measure before adding persistence | Commits early to public facts | Must eventually acquire types |

## Architecture

This section is an implementation draft for approval, not permission to
edit protected compiler files during this docs-only job.

### Placement and existing facts

Add a CLI-only `hd_lint` crate and `lint_cmd.rs` for analysis and policy.
Keep checking authoritative: `hd_check` exposes an optional collection sink,
not lint-specific diagnostics or allow/deny configuration.
The sink is disabled for normal checking, builds, and the web compiler.

Place neutral observation types in a feature-gated `hd_check::observations`
module, not in `hd_lint`. `hd_driver` returns their owned results to the
CLI; only the CLI invokes `hd_lint`. This keeps the dependency direction
`hd_cli → hd_driver/hd_lint → hd_check/hd_types`, without a checker-to-linter cycle.

The checker request enables typed-call collection, not individual lint codes.
Origin is an explicit enum: ExistingPlace, ExistingReference,
FreshAutomaticAddress, or Unknown. Statement and sub-body identities refer
to source ownership, so nested closures cannot inherit declaration-site suppressions.

[`receiver.rs`](../../crates/hd_check/src/receiver.rs) distinguishes place
addressing and reference receivers. [`call.rs`](../../crates/hd_check/src/call.rs)
resolves method identity, arguments, substitutions, and suspension.
Record the successful source call only after method candidate trials settle;
trial failures must not leak duplicate candidates.

[`BodyCx` and body checking](../../crates/hd_check/src/body.rs) own inference
and the lexical checking environment. Finalize pending facts with that
body's inference table before its local type pool is discarded.
Omitted results and inferred rows must have settled before analysis.
`BodyFacts` owns its normalized type arena and retains immutable signature/interface
views; it never borrows a discarded inference table or exports dangling IDs.

[`hd_driver`](../../crates/hd_driver/src/lib.rs) already invokes
`hd_check::check_fn` and joins parallel/forked body results.
Extend that result path with off-band candidates, including bodies checked
through the omitted-result path; disabling selected-body cache hits alone
is insufficient if that path drops candidates.

[`TIR Body`](../../crates/hd_tir/src/ir.rs) has instruction tags, types,
source-node indices, local IDs and flags, captures, sub-body parentage,
and suspension records. These are useful supporting facts.
They are not a ready public lint API and cannot reliably recover the
original temporary receiver after all address and ownership lowering.

Use source binding IDs and checker-origin records as primary facts.
For a later performance analysis, correlate typed source nodes with
pre-ownership TIR control edges, rather than relying on emitted Wasm.
Do not put lint analysis after inlining, when source calls can disappear.

### Data structures and data flow

| Proposed structure | Fields |
| --- | --- |
| `LintRequest` | Selected source modules, requested built-in rule IDs, fact-schema revision; no severity policy. |
| `CallFact` | Body-local source node, owning statement, sub-body, call span, receiver span, resolved declaration, written name, receiver type, ordinary-formal range, result type, normalized row, origin, validity/bang/receiver flags. |
| `BodyFacts` | Source hash, body owner, compact call vector, shared formal-type vector, declaration/interface view, type environment; source-node and statement mapping. |
| `TypeClassMemo` | `(normalized type, environment)` to Active/Clear/Observable/Unknown; explicit traversal fuel. |
| `Finding` | Stable rule name, source span, actual method name, message arguments, related locations; optional future proof-backed edit, absent initially. |
| `Policy` | Exact-name defaults and package/CLI overrides; resolved statement-local suppressions. |
| Later `FlowFacts` | Binding IDs, reads/writes/escapes, capture edges, basic blocks, ordinary and cleanup/propagation/cancel/panic successors; no public guarantee initially. |

The proposed flow is:

```mermaid
flowchart LR
  C[CLI selection] --> D[Driver: normal parse and interfaces]
  D --> T[Selected bodies: check with optional fact sink]
  T --> F[Finalize facts after inference]
  F --> L[Built-in analyses]
  L --> P[Policy and source suppressions]
  P --> R[Existing diagnostic reporter]
```

Dependencies can use ordinary check caches. Selected bodies must re-run
when no lint facts were collected, including a completely warm check cache.
Each worker returns its own candidates; sort and deduplicate only after joining.
A sink-free normal run does not allocate candidate vectors.

### Driver pseudo-code

The following 32-line sketch uses proposed APIs, not current Rust exports.
It never compiles Wasm or executes the hd program.

```rust
fn lint(request, manifests, cli_policy) -> Report {
    let rules = registry.resolve_exact_names(request.rules)?;
    let policy = Policy::validate(manifests, cli_policy, &rules)?;
    let selected = check_selection(request)?;
    let mut findings = Vec::new();
    let mut errors = Vec::new();
    for module in selected.in_dependency_order() {
        let prerequisites = driver.interfaces_and_types(module)?;
        if !selected.reports(module) {
            driver.ensure_checked_dependency(module)?;
            continue;
        }
        let checked = driver.check_selected_with_sink(
            module, prerequisites, CacheMode::RecheckSelectedBodies);
        errors.extend(checked.compiler_diagnostics());
        for body in checked.successful_body_results() {
            let facts = body.finalize_lint_facts()?;
            let mut memo = TypeClassMemo::with_fuel_limit();
            for call in &facts.calls {
                if rules.contains(LOST_TEMPORARY_WRITE) {
                    findings.extend(lost_temporary_write(call, &facts, &mut memo));
                }
            }
        }
    }
    findings.sort_by_key(|f| (f.file(), f.span(), f.rule()));
    findings.dedup_by_key(|f| (f.file(), f.span(), f.rule()));
    let located = policy.apply_at_owning_statement(findings)?;
    let all = join_compiler_and_lint_diagnostics(errors, located);
    // A worker failure is not a successful partial report.
    reporter.render(all, checked_module_count())
}
```

### First-lint pseudo-code

This 32-line sketch assumes facts finalized by normal checking.
The recursive classifier inspects types only, never method bodies.

```rust
fn lost_temporary_write(call, facts, memo) -> Option<Finding> {
    if !call.successful || call.generated_only || call.bang { return None; }
    if call.origin != FreshAutomaticAddress { return None; }
    if call.receiver_permission != MutableReference { return None; }
    if facts.normalize(call.result) != Void { return None; }
    if !facts.row(call.row).is_provably_empty() { return None; }
    if classify(call.receiver_referent, facts, memo) != Clear { return None; }
    for formal in facts.formals(call.ordinary_formals) {
        if classify(formal.substituted_type, facts, memo) != Clear { return None; }
    }
    Some(Finding::conditional_temporary_write(
        call.receiver_span, call.written_method_name))
}
fn classify(ty, facts, memo) -> Class {
    let ty = facts.normalize(ty);
    if !memo.consume_fuel() { return Unknown; }
    match memo.lookup(ty, facts.environment) {
        Done(class) => return class,
        Active => return Unknown,
        Missing => {}
    }
    memo.mark_active(ty, facts.environment);
    let class = match facts.semantic_shape(ty) {
        Scalar | String => Clear,
        Reference | TraitValue | HostHandle => Observable,
        Parameter | Projection | Function | Opaque | Poison => Unknown,
        Aggregate(children) => join_classes(children.map(|t| classify(t, facts, memo))),
        Missing => Unknown,
    };
    memo.finish(ty, facts.environment, class);
    class
}
```

`join_classes` yields Observable if any child is Observable, otherwise
Unknown if any child is Unknown, otherwise Clear.
Do not serialize body-local `Ty`, `DefId`, or `NodeIdx` integers as
cross-run identities. Their meaning belongs to their owning tables.

### Caching and testing

V1 stores no lint result in a normal Check cache entry. Interface/type
prerequisites remain reusable, but selected bodies recheck for lint facts.
Policy changes rerun analysis/reporting without changing program acceptance.
A denied lint can never poison a later `hd check` cache hit.

If measurements justify persistence, add a separate versioned entry keyed
by toolchain, source content, dependency-interface closure, checking mode,
and fact-schema/rule revisions. Include source bytes or relocation identity
for spans; a whitespace-only edit must not reuse stale byte offsets.
Keep severity and suppression policy out of raw-fact keys.

Test classifier cycles, nested private fields, generic environments and
fuel exhaustion directly. Test collector rollback, omitted-result checking,
and parallel joins separately. CLI tests cover source spans, deterministic
ordering, cold/warm order permutations, package selection and suppressions.

Performance lints additionally need a control-flow oracle matrix: compare
all normal, early, cleanup, panic and cancellation observations before
any proposed edit. Runtime comparisons can falsify a proof, not establish
universal equivalence by themselves.

## Command and configuration

Proposed invocation retains the earlier command/config direction:

```sh
hd lint
hd lint src/geometry.hd --format json
hd lint --all --deny lost-temporary-write
hd lint -p geometry --allow lost-temporary-write
hd lint src/text.hd --warn repeated-string-concat
```

Selection follows check: optional FILE, workspace members through `-p`,
and the existing `--tests` and `--all` scopes.
Reuse its `--format`, `--summary`, `--max-errors`, and `--jobs` behavior.
`--allow`, `--warn`, and `--deny` accept repeatable exact lint names,
without group selectors. `--warn` enables opt-in performance advice without
turning that advice into a CI error.

```toml
[lint]
allow = []
warn = ["repeated-string-concat"]
deny = ["lost-temporary-write"]
```

The first lint defaults to warning. Allow suppresses it, warn reports
a warning, and deny reports an error. Conflicting entries, unknown names,
or non-list values are configuration errors; contradictory CLI settings are usage errors regardless
of argument order.

CLI settings override the selected package's manifest for that exact code.
No parent-directory or workspace policy is inherited implicitly.
A standalone file uses defaults plus CLI settings; dependencies cannot
impose lint policy on their caller.

A full-line `# lint-allow: CODE` immediately before a statement suppresses
that code for calls directly owned by the statement.
A function or closure body starts its own statement ownership; suppression
on a declaration never spreads into its nested executable body.
Unknown suppression codes are configuration errors.

A site allow overrides manifest and CLI deny, retaining the earlier escape
hatch for intentional global escapes. It is a proposed CLI interpretation
of an ordinary comment, without new hd grammar or an annotation.
Dead-suppression detection can be a later report-only lint, without initially
making suppression removal automatic.

## Output and status

Reuse check's diagnostic objects and final summary. JSON goes to stdout;
text diagnostics and summaries go to stderr. Sort by package-relative path,
source span, then lint code; diagnostic truncation does not alter total counts.

Proposed output for S1 uses its actual method and receiver:

```json
{"kind":"diagnostic","code":"lost-temporary-write","severity":"warning","message":"`shift` takes a mutable temporary receiver; bind the value if you meant to keep the change","file":"src/main.hd","line":11,"column":5,"fixes":[]}
{"kind":"summary","errors":0,"warnings":1,"passed":0,"failed":0,"ignored":0,"status":0,"modules_checked":1}
```

Warning-only runs exit 0. Deny changes the finding to error and exits 101;
allow removes that finding. Compiler errors remain ordinary errors that
lint policy cannot suppress. Invocation, configuration, and internal failures
use the existing failure status and summary behavior.

Categories belong in rule metadata: likely-bug, style, and performance.
They describe advice, not safety or fix applicability. Category-wide selectors
can wait until more than one lint warrants their configuration complexity.
The first performance candidate, `repeated-string-concat`, defaults to Allow
and is enabled by an exact-name warn or deny setting. It is
report-only; `lost-temporary-write` remains report-only.

## Custom lints

A team might prohibit a dependency method, require its logging wrapper,
or identify repeated application-specific allocations. These need declaration
identity or typed calls; source spelling is insufficient across aliases.
They do not need authority to change the compiler's type answers.

**Built-in Rust passes** get the best internal facts and cheapest first
implementation. They require compiler review and release, so teams cannot
publish an independent rule package. This is the initial boundary, rather
than an undocumented promise of dynamic loading.

**Queries over exported checked facts** decouple execution from the compiler.
A later explicit export can provide schema version, source hashes, public
symbol paths, resolved call rows, origins, and conservative type classes.
A team can read that data in hd or another ordinary program; no special
query syntax is needed.

The recommended later extension is this explicit data boundary.
The following Python sketch illustrates a domain rule over a proposed
JSON export; no such exporter exists today:

```python
import json, sys
facts = json.load(sys.stdin)
if facts["schema"] != "hd-lint-facts/1":
    raise SystemExit("unsupported fact schema")
for call in facts["calls"]:
    if call["target"] == "dep.net.raw_send":
        print(json.dumps({"rule": "team/no-raw-send", "span": call["span"]}))
```

The compiler never discovers or executes these scripts from a dependency
or manifest. The developer explicitly runs their chosen tool; its authority
belongs to that separate invocation. Exports reveal source structure, so
include only the selected packages and documented fields by default.
Unknown metadata must stay unknown rather than inviting guesses.

Schema changes require version negotiation, conservative forward behavior,
and stable symbol identities independent of intern-pool indices.
A custom rule's failures must not be reported as compiler errors or accepted
as built-in proof-backed edits. A later aggregator may validate their diagnostics
and namespaces, but does not certify their correctness.

**hd or Wasm plugins executed by lint** could receive only a snapshot and
consume fuel with no providers. They still need a host protocol, memory
limits, cancellation, schema compatibility and reproducible loading rules.
Current host embedding and registration are open in
[#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51); v1 does not depend on them.

**Native plugins** expose internal types efficiently but require exact
compiler compatibility and grant process authority. They are the highest
security/stability cost here and are not recommended.
This does not forbid a developer maintaining their own compiler fork.

## Fixing is an open option

The owner has not decided whether `hd fix`, `hd check --fix`, or
`hd lint --fix` should exist. This note neither removes today's command
nor adopts either replacement. It compares advice-only delivery with an
optional edit path after approval.

Advice-only delivery keeps command behavior read-only and exposes no edit
transaction or safety label. It gives agents enough source information
to propose a human-reviewable patch, but leaves repetitive edits to other tools.
This is the smallest first release.

An optional lint edit path would need a per-rule behavioral argument,
not merely a successful post-edit type check. A well-typed replacement
can change output, aliases, evaluation order, return values, divergence,
panics, suspension, cancellation or cleanup.

For each edit, the rule must state its observation model and prove its
preconditions from supported facts. If resource failures are excluded from
that model, the owner must approve that exclusion explicitly.
Today heap and time-limit failures are specified panic categories; performance
rewrites cannot silently redefine them away.

For `repeated-string-concat`, detect a resolved string concatenation whose
left value reads the same binding that the enclosing loop assignment writes.
Report: "repeated concatenation may copy the accumulated text; consider a
StringBuilder if intermediate values are not needed".
The candidate is advice about asymptotic copying, not a measured speedup.

Replacing all appends with builder pushes and building after the loop is
**report-only**. S9 observes each prefix through a closure, and S10 observes
prefixes on early exits and defer. Allocation timing and failure can also
differ even when the final text matches. No automatic edit is proposed.

`lost-temporary-write` is also **report-only**: S7 retains the receiver,
S12 panics, and receiver or argument evaluation can have independent effects.
Neither binding the receiver nor deleting the call has a universally safe
source transformation under the header-only predicate.

If a future approved lint supplies an edit, the executor should validate
source hashes and non-overlapping UTF-8 byte ranges before writing.
Apply an entire edit group transactionally, rerun check and analysis, and
roll back on stale inputs or failed validation. That validation is necessary,
but the behavioral proof must precede it.

## Cost

These are implementation budgets, not measured code size or speedups.
No Rust change is delivered by this revision. The literal line counts below
include comments and blanks; they were measured with `wc -l` at the pinned base.
No existing file is proposed for wholesale replacement.

| Existing component | Measured lines | Replaced/deleted by v1 |
| --- | ---: | ---: |
| `crates/hd_cli/src/check_cmd.rs` | 441 | 0; reuse selection/reporting with a small extraction if necessary |
| `crates/hd_cli/src/report.rs` | 192 | 0; reuse diagnostic renderer |
| `crates/hd_cli/src/fix_cmd.rs` | 526 | 0; fixing remains undecided |
| `crates/hd_check/src/receiver.rs` | 604 | 0; add optional source-origin capture |
| `crates/hd_check/src/call.rs` | 5005 | 0; hook successful resolution, not a second resolver |
| `crates/hd_tir/src/ir.rs` | 3767 | 0; no lint fields in emitted TIR |

As a scale reference, the downloaded Go `lostcancel.go` is 325 literal
lines, SHA-256 `faf49017f32edefbc12d758f94656cb066121e8610efaa788ce52d759e84f119`.
That excludes its CFG, type checker, loading, tests and diagnostics framework;
it is not a 325-line estimate for the whole hd feature.
The [upstream file](https://github.com/golang/tools/blob/master/go/analysis/passes/lostcancel/lostcancel.go)
was measured on 2026-10-23, with its exact bytes retained in the evidence.

| Addition or adjustment | Proposed production-line budget | Why this component exists |
| --- | ---: | --- |
| CLI options, manifest validation, selection adapter | 140 | One command plus exact-name policy, reusing check's selectors |
| Optional checker sink and inference finalization | 150 | Source provenance, formal types, trial rollback and alternate body-check paths |
| Driver collection/join and selected-body cache mode | 110 | Preserve candidates across parallel and inferred-result checking |
| Recursive type classifier and memo | 100 | Three-way proof, cycles, environment and fuel |
| Registry, first predicate and messages | 80 | One built-in lint without policy in checking |
| Policy, comment ownership and reporter adapter | 140 | Scope-correct overrides and existing output dialect |
| Opt-in concatenation report | 100 | Resolved binding/callee pattern; no fix or general CFG solver |
| **Total initial production allowance** | **820** | Sum of components, excluding tests/spec/docs |
| Focused tests and CLI fixtures | 320 | Predicate matrix, cache-order permutations, scopes and output |
| Later public fact schema/export | 260 | Separate approval/measurement phase, not hidden in v1 |

Each component stops for review above 1.5 times its allowance; moving lines
into another crate does not evade that limit. If the optional collector
requires compiler restructuring beyond these hooks, stop before implementation.
These allowances must be replaced by actual per-component diffs as work lands.

A compact candidate with sixteen 32-bit words is 64 bytes before vector
capacity and pointed-to data. Ten thousand calls therefore need 640,000
candidate bytes, plus `4 × P` bytes for P formal-type IDs and type-memo storage.
This is a layout budget, not a measured peak-RSS claim; owned names and
source text are shared through existing tables, not duplicated per candidate.

The collector is O(calls plus formals); recursive classification memoizes
normalized type/environment pairs and traverses each distinct semantic edge
within its fuel budget. Warm lint intentionally includes selected-body
checking cost. Measure cold/warm wall time, CPU, call/type counts, allocated
bytes and peak RSS on json/nbody plus an adversarial generated module.

The command emits no Wasm and runs no target program: it adds no application
runtime helper or expected emitted-module bytes. Verify this with exact-byte
build comparisons after a real implementation; this is currently an architectural
expectation, not an observed implementation result.

Keep `hd_lint` CLI-only and the collector feature off in the web graph.
The intended web compiler gzip delta is zero, but must be measured and
the existing 2 MB gate must stay green. If the dependency graph pulls in
lint policy or plugin machinery, stop rather than spending that budget silently.
No speedup, Wasm delta or gzip measurement of an unimplemented lint is claimed.

## Stress test

### Surface and method

This review fixes the owner's tightened first-lint predicate, read-only
performance advice, and optional rather than decided fixing.
It uses twelve adversarial groups covering the semantic cases above.
The nine complete hd programs below are small translations of public API
shapes and original counterexamples, not full ports of those libraries.

The reference shapes are written afresh, not copied library implementations:

```rust
// Vec-like mutation of a temporary; source shape for S1/S2.
fn discarded(mut xs: Vec<i32>) { xs.clear(); }
// Serializer/writer-like state passed through a reference; S4/S5.
fn append(out: &mut Vec<i32>, value: i32) { out.push(value); }
// Iterator/Serializer-style associated result; S6.
trait Source { type Item; fn item(&self) -> Self::Item; }
// Captured observer of an accumulator; S8/S9 are hd-specific adaptations.
fn observe(prefix: &str, seen: &mut Vec<String>) { seen.push(prefix.into()); }
```

[Rust Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.clear)
provides the mutable-collection shape;
[Serde Serializer](https://docs.rs/serde/latest/serde/ser/trait.Serializer.html)
provides associated results and caller-controlled writer state.
[clap Command](https://docs.rs/clap/latest/clap/struct.Command.html#method.arg)
returns a builder value, unlike a discarded void mutator.
[Go strings.Builder](https://pkg.go.dev/strings#Builder) motivates accumulated
text construction, without implying identical ownership or error behavior in hd.

A judgment of *works* means the proposed rule gives the intended advice
or conservative exclusion. *Friction* means intentional missed advice or an
accepted suppressible false positive. *Breaks* means a proposed transformation
changes observations; it must not be offered automatically.
None is an assertion that an unimplemented lint executed.

| Group | Cases and source shape | Judgment | Observation / what the naive design gets wrong |
| --- | --- | --- | --- |
| 1 | S1/S2, Vec-like temporary, place, alias and element mutations | Works | Only fresh temporary qualifies; output `2/3` distinguishes the original and copied list element. |
| 2 | S3, shadowed bindings | Works | Same spelling has different binding IDs; inner mutation leaves outer output unchanged. |
| 3 | S4, writer retaining a receiver through an argument | Works | Output `7/3` proves ordinary reference parameters must also exclude the warning. |
| 4 | S5, temporary Appender around a caller's sink | Works | A reference field preserves visible output despite temporary wrapper lifetime. |
| 5 | S6, generic/associated/erased data | Friction | Unknown types stay quiet; only the concrete integer Box qualifies in source. |
| 6 | S7, mutable module storage retaining self | Friction | Output `7/1` confirms the accepted false-positive escape; keep the site suppression. |
| 7 | S8, a callback with an empty row | Works | The callback mutates a captured variable; an empty row is insufficient. |
| 8 | S9, StringBuilder replacement with a captured prefix observer | Breaks as an automatic fix | Final texts match, but the observer must see both `a` and `ab`. |
| 9 | S10, branch, continue, break, return, propagation and defer | Breaks as an automatic fix | Different exits expose `ab`, `a`, and `a`; a single post-loop build loses them. |
| 10 | S11, timer and implicit disposal | Works / conservative | Bang and nonempty row exclude the warning; actual timeout cancellation runs the deferred observer and disposal. |
| 11 | S12, panic in a void empty-row method | Works as advice; breaks if deleted | Both engines still report explicit-panic; warning cannot justify removal. |
| 12 | S13-S15, nonempty row, non-void result and inactive reference variant | Works | All remain quiet; active enum variant and fulfilled outer providers do not weaken the exclusions. |

### Receiver places and shadowing: S1-S3

```text
struct Point:
    x: i32

impl Point:
    fn shift(&mut self, dx: i32) -> void:
        self.x += dx

fn make() -> Point: Point { x: +0 }

pub fn main() -> void $ Console:
    make().shift(+1)                 # S1: proposed warning
    let mut p = make()
    p.shift(+1)                      # S2: place, quiet
    alias := &mut p
    alias.shift(+1)                  # S2: reference, quiet
    let mut points = [p]
    points[0].shift(+1)               # S2: element place, quiet
    if true:
        let mut p = make()           # S3: distinct shadowed binding
        p.shift(+9)
    println("${p.x}/${points[0].x}")
```

The method on `make()` is the proposed finding. Copies, aliases and
nested writable places must keep their distinct behavior.
Both engines print `2/3`; the inner shadow's value is never assigned to
the outer Point.

### Writer and argument escape: S4-S5

```text
struct Point:
    x: i32

impl Point:
    fn remember(&mut self, into: &mut List[&mut Point]) -> void:
        self.x = +7
        into.push(self)

struct Appender:
    into: &mut List[i32]

impl Appender:
    fn add(&mut self, value: i32) -> void:
        self.into.push(value)

pub fn main() -> void $ Console:
    let mut kept: List[&mut Point] = []
    Point { x: +0 }.remember(&mut kept)   # S4: reference argument, quiet
    let mut values: List[i32] = []
    Appender { into: &mut values }.add(+3) # S5: reference field, quiet
    println("${kept[0].x}/${values[0]}")
```

Both engines print `7/3`. This is the earlier draft's genuine failed
loss argument: temporary storage can outlive its caller through another
parameter. The owner's revised formal-type exclusion repairs that case,
without claiming a full escape proof.

### Generic, associated and dynamic types: S6

```text
struct Box[T]:
    value: T

impl[T] Box[T]:
    fn touch(&mut self) -> void:
        pass

trait Probe:
    type Item
    fn sample(&self) -> Self::Item

struct Sample: pass

impl Probe for Sample:
    type Item = i32
    fn sample(&self) -> i32: +2

fn unresolved[T](value: T) -> void:
    Box { value: value }.touch()        # S6: generic, unknown, quiet

fn dynamic(value: &dyn Probe[Item=i32]) -> void:
    Box { value: value }.touch()        # S6: nested trait value, quiet

fn associated[T < Probe](value: T) -> void:
    Box { value: value.sample() }.touch() # S6: unresolved projection, quiet

pub fn main() -> void $ Console:
    Box { value: +1 }.touch()           # S6: known concrete, warning
    unresolved(+1)
    dynamic(&Sample {})
    associated(Sample {})
    println("types")
```

The source generic body sees an arbitrary T; specializing one invocation
does not justify findings for all callers. The dynamic Box contains a
trait reference; the associated value has an unresolved projection while
checking its generic owner. Both engines run the concrete calls and print `types`.

### Captured and global escape: S7-S8

```text
struct Point:
    x: i32

let mut saved: List[&mut Point] = []

impl Point:
    fn remember(&mut self) -> void:
        self.x = +7
        saved.push(self)

struct Callback:
    call: fn() -> void

impl Callback:
    fn invoke(&mut self) -> void:
        (self.call)()

pub fn main() -> void $ Console:
    Point { x: +0 }.remember()          # S7: accepted false positive
    let total = +0
    Callback { call: fn() -> void: total += 1 }.invoke() # S8: unknown captures, quiet
    println("${saved[0].x}/$total")
```

Both engines print `7/1`. The global-retaining method still meets the
header predicate: the warning is intentionally a heuristic, with a site allow.
The callback-bearing wrapper is excluded because function environments are
Unknown in the first classifier.

### Repeated text and intermediate observers: S9

```text
use std.text.StringBuilder

fn repeated(parts: List[string]) -> string:
    let out = ""
    for part in parts:
        out = out + part
    out

fn batched(parts: List[string]) -> string:
    out := StringBuilder::new()
    for part in parts:
        out.push(part)
    out.build()

fn observed(parts: List[string]) -> string:
    let out = ""
    let mut seen: List[string] = []
    snapshot := fn() -> void: seen.push(out)
    for part in parts:
        out = out + part
        snapshot()                    # S9: reads each partial result
    seen.join("/")

pub fn main() -> void $ Console:
    println("${repeated(["a", "b"])}/${batched(["a", "b"])}/${observed(["a", "b"])}")
```

Both engines print `ab/ab/a/ab`. The first two functions agree for this
input; that alone proves no general rewrite. The third function exposes
each intermediate string through a shared captured cell.

### Exits and cleanup: S10

```text
fn partial(parts: List[string], trace: &mut List[string]) -> Option[string]:
    let out = ""
    defer: trace.push(out)              # S10: observes on early exit
    for part in parts:
        if part == "skip":
            continue
        if part == "stop":
            break
        if part == "return":
            return .Some(out)
        if part == "error":
            let missing: string? = .None
            out = out + missing?
        out = out + part
    .Some(out)

pub fn main() -> void $ Console:
    let mut trace: List[string] = []
    a := partial(["a", "skip", "b", "stop", "c"], &mut trace)
    b := partial(["a", "return", "c"], &mut trace)
    c := partial(["a", "error"], &mut trace)
    println("${a.unwrap_or("none")}/${b.unwrap_or("none")}/${c.unwrap_or("none")}/${trace.join(",")}")
```

Both engines print `ab/a/none/ab,a,a`. The observer is reached on normal,
return and propagation paths. An implementation that only follows the final
expression will miss these observations, even when it recognizes the loop.

### Suspension and implicit cleanup: S11

```text
use std.ops.Disposable
use std.time.{Clock, sleep}
use std.task.timeout

struct Guard:
    trace: &mut List[string]

impl Disposable for Guard:
    fn dispose!(&mut self) -> void:
        self.trace.push("disposed")

struct Worker:
    value: i32

impl Worker:
    fn touch!(&mut self) -> void $ Clock:
        sleep!(200ms)
        self.value += 1

pub fn main!() -> void $ Clock + Console:
    let mut trace: List[string] = []
    outcome := timeout!(20ms):
        using guard = &mut Guard { trace: &mut trace }
        defer: trace.push("deferred")
        Worker { value: +0 }.touch!()   # S11: cancellation runs cleanup
    println("${outcome.is_err()}/${trace.join(",")}")
```

Both engines print `true/deferred,disposed`. The timeout cancels the work
during its longer sleep, then waits for its registered cleanup. A rewrite
cannot bypass the defer or using disposal on that cancellation edge.
These are observations from one run per engine, not a timing-stability claim.

### Panic and declared exclusions: S12-S15

```text
struct Point:
    x: i32

impl Point:
    fn fail(&mut self) -> void:
        self.x += 1
        panic("kept panic")

pub fn main() -> void:
    Point { x: +0 }.fail()              # S12: warn, never delete
```

Both engines exit 3 with `panic: explicit-panic: kept panic`.
The source call qualifies for advice, but deletion changes the program.

```text
struct Gadget:
    x: i32

impl Gadget:
    fn log(&mut self) -> void $ Console:
        println("logged")
    fn add(&mut self) -> i32:
        self.x += 1
        self.x

enum Holding:
    Empty
    Borrowed(point: &mut Gadget)

struct Envelope:
    value: Holding

impl Envelope:
    fn touch(&mut self) -> void:
        pass

pub fn main() -> void $ Console:
    Gadget { x: +0 }.log()             # S13: nonempty row, quiet
    value := Gadget { x: +0 }.add()    # S14: result, quiet
    Envelope { value: .Empty }.touch() # S15: another variant has a reference, quiet
    println("$value")
```

Both engines print `logged` and `1` on separate lines.
Console remains a declared requirement even when available at the caller.
The inactive enum variant demonstrates why a field walk considers the
complete type, not the particular constructor at one site.

### Problems found, ranked

| Rank | Problem | Cases | Design consequence |
| --- | --- | --- | --- |
| 1 | Delayed string construction changes visible intermediate state and exits | S9/S10/S11 | Performance candidate stays report-only; no general edit proof. |
| 2 | Header-only escape analysis cannot exclude global retention or panic | S7/S12 | Conditional message and site allow; lost-write stays report-only. |
| 3 | Generic and function environments hide indirection | S6/S8 | Unknown skips; accept missed advice instead of unsound certainty. |
| 4 | Lowering and caches can erase origin or lose facts | All groups | Collect at successful checking, finalize before pool disposal, and test cold/warm plus alternate body paths. |

The first draft's tight header filter remains useful, but its name must
not be read as a non-escape theorem. These tests found real boundaries;
they did not demonstrate a complete semantic proof or an implemented lint.

## Implementation order and validation

After approval, use hour-sized, independently green slices. A slice that
cannot meet its stated test in about an hour stops with a concrete
scope estimate, rather than silently widening compiler work.

| Slice | Deliverable and proving check |
| --- | --- |
| 1 | Exact-name registry and pure policy functions; tests for conflicting CLI/manifest/site settings and statement ownership. No compiler hook yet. |
| 2 | Read-only CLI shell, help and selection using existing reporter; CLI text/JSON/exit tests with an empty registry. Ship approved CLI spec rules and fixtures together. |
| 3 | Optional successful-call sink and source receiver origin; focused checker/driver tests prove no trial duplicates and no normal-check diagnostic changes. |
| 4 | Finalization through normal, inferred-result and parallel body paths; cold/warm/order tests prove no candidate loss or cache contamination. |
| 5 | Type classifier, memo and first predicate; the complete exclusion matrix plus recursive/fuel tests passes. Ship the approved rule/code and conformance cases. |
| 6 | Statement suppression and full output locations; UTF-8 offsets, parentheses, closures, generated bodies, workspace roles and diagnostic limits pass. |
| 7 | Opt-in resolved concatenation report; S9/S10 regression fixtures receive advice without edits; no general flow solver is promised. |
| 8 | Measure selected-body lint overhead, normal check/build exact-byte output and web gzip; stop on budget failures before considering persistence. |

Compiler scopes listed here describe future implementation work needing
explicit authorization. This revision edits documentation only.
Custom export, plugin execution and every fix command are separate
approval and implementation work, not prerequisites hidden in these slices.

## Recommendation

**Recommendation:** choose ephemeral typed facts, a separate read-only lint
command, and the header-based first lint with conservative Unknown results.
Keep the existing command/config direction, add a performance category with
report-only concatenation advice, and expose no custom-code loader initially.

This gives up a cheap warm-body cache hit and full generic coverage.
The next-best option is a separately versioned persisted fact graph when
measurements and real custom-rule demand justify its larger contract.
For eventual custom rules, prefer explicit offline facts queries over native
plugins or automatic execution of dependency code.

The owner must approve the collector/driver boundary, initial lint policy,
conditional message, performance candidate, implementation budgets and phasing.
Fix-command existence and its observational proof contract remain open in #805.
A public facts schema and plugin host remain separate approval gates.
Nothing here settles language semantics or permits protected-file edits.

## Evidence and parse log

The fresh dev compiler and parser were built from the proposal base
`a00c6b374358fb2992f046695ea0f6639b5ab542`, without the parked #407 patch.
Compiler SHA-256 `c79c3325b16b441b3e68d5c1b618b1eb2b68b2c117309c9d0cc1da89f40958d8`;
parser SHA-256 `714ee363867f02fd4819ed64599a7be7143c1c0630c0b5e14c6264b5cb846271`.
All caches and temporary probe sources stayed inside the isolated worktree.

| Complete hd block | Parse and check | Runtime evidence |
| --- | --- | --- |
| 1: receivers | Parse clean; zero check errors/warnings | Node and Wasmtime `2/3` |
| 2: escapes | Parse clean; zero check errors/warnings | Both `7/3` |
| 3: types | Parse clean; zero check errors/warnings | Both `types` |
| 4: captures/global | Parse clean; zero check errors/warnings | Both `7/1` |
| 5: string observations | Parse clean; zero check errors/warnings | Both `ab/ab/a/ab` |
| 6: control flow | Parse clean; zero check errors/warnings | Both `ab/a/none/ab,a,a` |
| 7: suspension/cleanup | Parse clean; zero check errors/warnings | Both `true/deferred,disposed`; actual timeout cancellation |
| 8: panic | Parse clean; zero check errors/warnings | Both expected explicit-panic, status 3 |
| 9: rows/results | Parse clean; zero check errors/warnings | Both `logged` then `1` |
| Proposed lint, fact export, configuration and edits | Not implemented | Not executed; no lint correctness or performance claim |

Rust driver/predicate blocks are pseudo-code over proposed internal APIs.
The Python query consumes a hypothetical schema; it is not an existing
integration. Original Rust shape examples are explanatory, not claimed Rust tests.

Raw commands, diagnostics, source hashes, exact prototypes, upstream reference
bytes and final docs checks are preserved under
`/private/tmp/muse-651-rewrite-1792782846927866000`.
The five construction diagnostics are recorded verbatim in the writing log.
Earlier 651 evidence remains historical, not a substitute for this fresh run.
