# hd lint

Architecture proposal, 2026-10-27; not accepted; [#908](https://github.com/HerringtonDarkholme/hd-lang/issues/908).
The accepted command/policy rules remain in [the CLI spec](../../spec/cli/command-line.md#linting).
This replaces the implementation plan, not those decisions. No implementation ships here.

## The proposal

Register each lint once: name, code, default level, required views, event kinds
and check. Union enabled requirements into **one driver request**. Reuse the
compiler's completed syntax/resolution/type information through borrowed views;
collect owned findings, then apply policy once after prerequisite checks succeed.
Normal checking, building and the web compiler do not run lint.

```rust
static RULES: &[Rule] = &[
    Rule::new(Code::UnsignedComparisonAlways, Level::Warn,
        Needs::TYPES, Events::COMPARISON, unsigned_comparison),
];
let session = LintSession::new(RULES, selection, policy);
let output = build_with_analysis(host, packages, goal, session.request());
let report = session.finish(&output); // compiler errors bypass lint policy
```

Adding a rule changes its check, registry entry and fixtures; no new `Policy`
field, driver output channel, mutex or cache revision follows its name.
Needs describe reusable compiler views, not one fact struct per lint. The
name comes from the stable diagnostic code, without another name string table.

## Open questions

1. Borrow completed compiler state during callbacks instead of exporting an owned type graph? Recommendation: yes; types never escape.
2. Keep selected-body rechecking for typed lint initially, with optional persistent analysis caching deferred? Recommendation: yes; make this cost explicit.

## Purpose and current behavior

| Current problem | Proposed behavior |
| --- | --- |
| Ten optional fields and a hand-written name match in `Policy` | Indexed levels derived from one registry. |
| Four CLI analysis calls and three driver fact results | One request/sink lifecycle with stage-specific views. |
| Body-local type IDs cannot survive the compile run | Read them only inside a lifetime-bound query facade. |
| A normal Check cache hit has no source observations | One central recheck rule; never silently skip typed findings. |

## Architecture

`hd_lint` owns rule selection, predicates, messages, fixes and policy.
The driver exposes a neutral optional analysis consumer and never imports
`hd_lint`. The checker/resolver publish compiler observations, not warning decisions.
The same request covers syntax, declaration/module metadata and checked bodies.

```rust
pub struct AnalysisRequest<'s> { needs: Needs, sink: &'s dyn AnalysisSink }
pub trait AnalysisSink: Sync {
    fn visit(&self, event: AnalysisEvent<'_>);
}
pub struct Finding {
    lint: LintId, primary: Span, owner: FindingOwner,
    message: String, notes: Vec<Note>, fixes: Vec<ProposedFix>,
}
```

The request unions **enabled** registry entries before compilation and closes
view dependencies: types, written names/calls, uses and flow require checked
bodies/source; module/declaration metadata requires resolution; tokens need no
body view. These are reusable families, not flags for individual rules. Driver selection remains authoritative
for package/file/test roots and reachability; dependency interfaces are inputs,
not separately linted sources. Skip unneeded stage/view families. Within a requested stage, dispatch one source-order
traversal to interested rules, plus explicit body/module-finish hooks.
Complexity is traversal plus actual rule/query work; one traversal does not
make every possible lint linear. Derivation joins use indexed nominal keys.

For checked bodies, retain requested source-node rows before reference/place
lowering erases them. Retain binding/use information before checker scopes
are drained; finalize row types after inference/defaulting. Publish a borrowed
view only after verification succeeds, while its type pool remains alive.
Rejected trials roll back rows; nested closure ownership remains explicit.
Some inferred requirement rows finish later at module level: expose Pending,
then deliver final declaration rows with module/run-owned backing. That event
never retains expired body-local handles. Do not invent a successful trait answer.

The view reads expression/pattern types, written-name resolution, declaration
identity, settled call choices, receiver adjustments, binding uses and reachability. A call
choice distinguishes a concrete impl, generic trait bound, trait-value dispatch
and builtin; a generic bound is not an unresolved concrete impl. The prelude
guarantees bare `debug`/`dbg` identity in valid programs: they cannot
be rebound. No scope-history graph is needed just to offer the existing fix.
No rule repeats inference or guesses a call target from its spelling.
The first ten need settled results and declaration metadata; arbitrary new
trait-obligation solving is outside this proposal.

A `TypeRef<'v>` borrows its query context, not just a copyable raw `Ty`.
No usable run-local semantic IDs or type backing escape callbacks.
A rule emits owned text, source spans and stable lint identity. A compact owned
`SourcePlan` retains statement/sub-body ownership, suppression anchors and text
identity for finish-time policy. Reuse the driver's existing finish-time movement
of completed parses into shared position inputs; do not deep-clone or reparse them.
`FindingOwner` is Statement, Declaration, Module or Package, keyed to that source
snapshot; it never contains a checker local/definition ID. Each worker
buffers findings locally and appends a chunk; global ordering is independent
of completion order. Discard optional findings for a failed selected build;
preserve today's per-member workspace error scope. Body-local callbacks may
run before an unrelated module fails, but nothing
is reported from that failed build. Verified-body and final-report gates differ.

There is one cache boundary: a checked-view request bypasses selected root
Check **reads**, as today's typed lint already does. Reuse parse/interface and
dependency work; ordinary Check entries remain unchanged. Syntax/module-only
requests keep true warm Check hits. No policy, severity, suppression, message or
rule name enters a compiler key. Existing metadata is decoded only on demand.
Persistent typed analysis is deferred; it needs owned/remapped type backing,
source identity and an independent schema key before it can replay safely.

Resolve default → selected manifest → explicit CLI levels once. Validate exact
names/conflicts even when all checks are allowed. Selected-source suppression
validation still runs in that case, without a typed recheck. Apply site allow
last for calls directly owned by the target statement, including semantic
operator calls; never equate every non-call finding with a suppressible call.
A declaration's allow
never flows into its nested function/closure. Policy cannot suppress compiler
errors. Render existing diagnostics, messages, spans, fixes, ordering and statuses.
No new lint command, fix mode, plugin mechanism or suppression syntax is added.
The syntax facade hides run-interner token symbols; policy reads spans/text only.

Share the existing parse/source/token view and optional neutral comment anchors
with the [formatter proposal](https://github.com/HerringtonDarkholme/hd-lang/pull/911).
Formatting keeps attachment private initially; extract common neutral anchors
only when the second implemented consumer needs them, not in anticipation.
Lint suppression ownership is statement-based, not formatter print order.
Formatter never requests type facts; neither tool causes work when absent.

## Cost

| Current corpus, native dev opt-level 1 | Median | Scope |
| --- | ---: | --- |
| Normal check, empty compiler store | 215.977 ms | 3 samples; 44 modules checked. |
| Lint, empty compiler store | 221.357 ms | 3 samples; check + collection + all ten analyses. |
| Normal check, warm store | 40.038 ms | 7 samples; zero modules rechecked. |
| Lint, warm store | 216.832 ms | 7 samples; selected bodies are rechecked. |
| Analysis only | 1.712 ms | 7 samples over already-collected facts. |

All **42 raw std files / 879,422 B**, plus the compiler's two virtual modules,
were analyzed serially at `f4ddc82ee`. Inputs were loaded before timing;
“empty” means a new in-memory compiler store, not a cold OS process. Default
policy emits zero findings over real, nonempty checked facts (1,034 comparisons,
2,558 bindings, 1,032 patterns, 10,272 statements and 4,283 discards). Raw samples,
per-file figures and hashes remain in scratch. These are baseline costs, not a
speed claim for the proposal; three empty-store samples are especially limited.


“Off” means no extra allocation, traversal, type query, finding construction,
cache payload or recheck beyond today's ordinary path. It does not claim that
an inactive guard has literally no machine instructions. Keep `hd_web` free of
`hd_lint`; measure normal compile time and raw/gzip web size before rollout.
New architecture costs are unmeasured until approval. Publish per-file, corpus,
allocation and cold/warm receipts against the same source hashes and profile.

## A. Stress test

Real-code translations use local references and are checked with the current
compiler, not an implementation of the proposed views. All six final inputs
check successfully. Current lint outcomes are recorded below.

[Rust Vec::is_empty](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/library/alloc/src/vec/mod.rs)
uses a zero equality. Its hd translation stays quiet: this is not an ordering
comparison and the rule must not broaden it.

```text
fn empty(xs: &List[i32]) -> bool:
    xs.len() == 0
```

[Go sort.Search](https://github.com/golang/go/blob/6c49ac311e5642fa5399b46c736295a9dc4eeec9/src/sort/search.go)
translated with an overflow-avoiding midpoint also stays quiet. Local reads,
mutation and predicate calls must retain their own types and uses.

```text
fn search(length: usize, accept: fn(usize) -> bool) -> usize:
    let low = 0
    let high = length
    while low < high:
        mid := low + (high - low) / 2
        if accept(mid):
            high = mid
        else:
            low = mid + 1
    low
```

Three existing type-aware checks, with their exact stable code:

```text
fn expired(remaining: usize) -> bool:
    remaining < 0
```

`adjusted_type(remaining)` is usize and the right operand is an unsuffixed literal
zero; emit `unsigned-comparison-always`, using the real expression span/name.
A signed type or a suffixed zero must retain the current quiet outcome.

```text
fn total() -> i32:
    let mut count = +1
    count
```

`binding(count)` is primitive, with no writable address taken; emit
`redundant-let-mut` and its exact keyword edit. A writable reference annotation,
address-taking, capture or read affects the checked binding view, not text matching.

```text
fn dump() -> void $ Console:
    debug(+1)
    pass
```

The implicit discard resolves to the canonical prelude function; emit
`unused-debug-text`. Preserve current argument-shape/fix conditions. No full
lexical-scope snapshot is required to establish the unshadowable bare `dbg` name.

The adversarial method lookalike exposes a **current bug**, not an accepted
warning or a fix in this PR:

```text
struct Logger: pass

impl Logger:
    fn debug(&self, value: i32) -> void:
        _ := value
        pass

fn user_call() -> void:
    Logger {}.debug(+1)
    pass
```

Current `hd lint` incorrectly emits `unused-debug-text` here although this method
returns void; [#916](https://github.com/HerringtonDarkholme/hd-lang/issues/916)
records the exact reproducer. The path-based classifier drops a constructor
receiver, so a real selected-declaration view is essential. Preserve valid
prelude-call behavior without enshrining this false positive in the new API.

| Adversary | Required result/invariant |
| --- | --- |
| A module/local declaration named debug or dbg | Existing `prelude-name-shadow`; no lint findings from an invalid prerequisite. |
| Generic bound, trait value, builtin or indirect function call | Distinct settled Resolution variants; absence of a concrete impl is not Pending. |
| Overload trials and M1 result replay | No rejected/duplicate source events or stale type IDs. |
| Captures/defer and nested closures | Final use/address facts; separate source ownership and suppression scope. |
| Two packages/types with the same displayed name | Nominal declaration identity, not string comparison. |
| All levels Allow plus an unknown suppression name | Configuration error, with no typed lint recheck. |
| Workspace member failure, warm Check/interface hits, changed source | Preserve per-build prerequisite scope, collect missing required views, no stale findings. |

Parse log: all six `text` blocks parse; all six final inputs also pass `hd check`.
The three positive lint controls each report their expected code
for each of the three examples; the method case's false positive is explicitly
tracked. No idempotence, fix correctness or new-view performance is inferred
from these small probes.


## B. Prior art, with code

The study uses actual source at **oxc `8e00068a`**, **Ruff `ba65abf8`** and
**Rust/Clippy 1.97.1 `8bab26f4`**. A name/scope model is not a checked type model.
None proves hd's off-path cost or cache correctness; hd must test those separately.

### Oxlint

`Rule` supplies node and whole-file hooks. `declare_oxc_lint!` records metadata;
a generated inventory drives dispatch. Enabled rules are bucketed by AST-kind
interests and one node traversal dispatches them. A debug reference traversal
checks that filtering preserves complete diagnostics. Take one registry and
interest buckets, not a plugin framework/code generator for ten rules.
[Rule](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_linter/src/rule.rs#L20),
[dispatch/reference](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_linter/src/lib.rs#L186).

Illustrative abbreviated Rust shape:

```rust
trait Rule {
    fn run(&self, node: &AstNode<'_>, cx: &LintContext<'_>);
    fn run_once(&self, cx: &LintContext<'_>);
}
// declare_oxc_lint! supplies metadata; generated RuleEnum supplies dispatch.
Tester::new(NoDebugger::NAME, NoDebugger::PLUGIN, pass, fail)
    .expect_fix(fix).test_and_snapshot();
```

The shared `Semantic` stores AST nodes, scopes, references and optional CFG;
this lint host currently requires CFG. Its native model does not infer complete
TypeScript expression types: typed rules use a separate `tsgolint` integration.
Reuse hd's already-checked results instead of recreating that model/process or
forcing CFG work on syntax-only rules.
[Semantic](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_semantic/src/lib.rs#L61),
[host](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_linter/src/context/host.rs#L38),
[typed process](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_linter/src/tsgolint.rs#L26).

Severity/directives meet at central emission. Fix APIs distinguish capabilities,
but fix construction is not universally lazy. Fixtures snapshot exact diagnostics
and fixed text. Take policy separation and exact-source tests; retain hd's own
allow/warn/deny and statement ownership, not JavaScript directives or fix rules.
[Emission/fixes](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_linter/src/context/mod.rs#L264),
[tester](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_linter/src/tester.rs#L328).

### Ruff

The code map registers rule identities and their phases. Its `SemanticModel`
tracks Python scopes/bindings/references, not hd-style checked expression types.
Ruff runs token/line/import passes plus a shared AST checker and deferred
binding/scope/reference checks; it is not one universal Rule::run dispatcher.
Take phase dependencies and completed shared views; do not copy a second scope
resolver or Python's deferred-evaluation semantics.
[Registry](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_linter/src/registry.rs#L234),
[checker](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_linter/src/checkers/ast/mod.rs#L1),
[model](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_semantic/src/model.rs#L59).

Adapted Rust declaration/dispatch sketch:

```rust
#[derive(ViolationMetadata)]
struct UndefinedName { name: String }
// The registry maps codes to violation types; checker sites still dispatch.
if checker.is_rule_enabled(Rule::UnusedVariable) {
    // Inspect completed bindings, report payload, attach an applicable fix.
}
```

`RuleTable` holds enabled/fixable sets, not hd's severity model. `noqa` filtering
happens centrally after checks; source directives reuse parser tokens. Fix
application orders/deduplicates edits and avoids overlaps. Tests compare exact
fixed output, bounded convergence and syntax validity. Take that reliability
shape, but never run compiler errors through suppression or add a fix command.
[Rule table](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_linter/src/settings/rule_table.rs#L9),
[pipeline/suppression](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_linter/src/linter.rs#L156),
[fixes](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_linter/src/fix/mod.rs#L30),
[test harness](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_linter/src/test.rs#L243).

### Rust and Clippy

Early passes see expanded AST; late passes see HIR and typed context.
`declare_lint!`/`declare_clippy_lint!` declare metadata, and registered passes
consume the compiler's work. Clippy installs registration through
`rustc_driver::Callbacks::config`; it does not do every check in after_analysis.
Take stage-appropriate borrowed views; no mutable Ck or second checker.
[Passes](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/compiler/rustc_lint/src/passes.rs#L6),
[Clippy registration](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/src/tools/clippy/src/driver.rs#L135).

Illustrative typed-query usage (not a complete lint):

```rust
fn inspect<'tcx>(cx: &LateContext<'tcx>, e: &hir::Expr<'tcx>) {
    let checked = cx.typeck_results();
    let original = checked.expr_ty(e);
    let adjusted = checked.expr_ty_adjusted(e);
    let method = checked.type_dependent_def_id(e.hir_id);
    let changes = checked.expr_adjustments(e);
    let _ = (original, adjusted, method, changes);
}
```

`LateContext` borrows `TyCtxt` and lazily caches the current body's TypeckResults;
expression/pattern types, selected methods and adjustments are separate tables.
A Copy context handle still borrows its arenas. Driver callbacks can read it
while live, but only owned reports may outlive it. Hd should freeze source rows
before its lowering, not pretend post-run raw IDs are an equivalent context.
[Context](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/compiler/rustc_lint/src/context.rs#L481),
[type tables](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/compiler/rustc_middle/src/ty/typeck_results.rs#L349),
[callback lifetime](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/compiler/rustc_driver_impl/src/lib.rs#L119).

Trait/impl queries include declared members and compiler obligations. Clippy's
`implements_trait` creates an inference context and evaluates an obligation;
it is not a free lookup. Rustc also disk-caches typeck queries, unlike hd's
current ephemeral source observations. Take settled declaration/evidence views
for the first ten; defer new solver probes and persistence until explicitly scoped.
[Clippy trait probe](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/src/tools/clippy/clippy_utils/src/ty/mod.rs#L226),
[cached type checking](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/compiler/rustc_middle/src/queries.rs#L1229).

### Go analysis

An Analyzer declares prerequisites; Pass supplies AST/type information and
results from required analyses. Cross-package Facts are explicit serialized
data, not a retained compiler session. The API does not promise concurrent
mutation safety. Take declared view dependencies, supplied types and diagnostic/
fix goldens; omit extensible analyzer graphs and per-rule serialized channels.
[Pinned API](https://github.com/golang/tools/blob/5dd9c334831769ea0c3029c08a1702b9cc0a9be3/go/analysis/analysis.go#L18),
[diagnostics/fixes](https://github.com/golang/tools/blob/5dd9c334831769ea0c3029c08a1702b9cc0a9be3/go/analysis/diagnostic.go#L18),
[golden tests](https://github.com/golang/tools/blob/5dd9c334831769ea0c3029c08a1702b9cc0a9be3/go/analysis/analysistest/analysistest.go#L84).

```go
var Check = &analysis.Analyzer{
    Name: "example", Doc: "example", Requires: []*analysis.Analyzer{inspect.Analyzer},
    Run: func(p *analysis.Pass) (any, error) {
        nodes := p.ResultOf[inspect.Analyzer].(*inspector.Inspector)
        _ = nodes; _ = p.TypesInfo
        return nil, nil
    },
}
```

### ESLint

Rules have metadata and create(context), which returns node/selector callbacks.
SourceCode supplies tokens/comments and scope queries. Fixable metadata is not
proof of semantic safety. RuleTester compares valid/invalid diagnostics and exact
fixed output. Take one metadata/check entry and source queries, not dynamic JS
plugins, a selector DSL, or an additional name/type checker.
[Rule API](https://github.com/eslint/eslint/blob/161558ae1391a8649a8d69a8b2c2e555b40121b9/docs/src/extend/custom-rules.md#L290),
[example rule](https://github.com/eslint/eslint/blob/161558ae1391a8649a8d69a8b2c2e555b40121b9/lib/rules/no-debugger.js#L15),
[RuleTester](https://github.com/eslint/eslint/blob/161558ae1391a8649a8d69a8b2c2e555b40121b9/docs/src/integrate/nodejs-api.md#L957).

```javascript
const rule = {
  meta: { type: "problem", schema: [], messages: { bad: "Remove debugger." } },
  create(ctx) { return {
    DebuggerStatement(node) { ctx.report({ node, messageId: "bad" }); }
  }; }
};
```


## C. Alternatives considered

An owned normalized `AnalysisSnapshot` would permit after-run queries and future
warm replay, but needs a type/definition/symbol arena, remapping, serialization
and explicit copy budgets. Existing `Output` owns none of that backing.
Borrowed callbacks are smaller for ten rules; warm typed checks still recheck.

Keeping independent fact channels repeats today's coupling. Exporting raw TIR
or a naked `Ty` makes ownership/source-loss problems the lint's responsibility.
Retaining `BodyCx` or rerunning its checker adds mutable inference state and
unnecessary lifetime/cost. Copying a standalone JS/Python name model supplies
no hd trait/type answers. None is the recommended first implementation.

## D. Today in hd

<a id="contents"></a>
<a id="warning-migration-inventory"></a>
<a id="operational-exceptions-needing-approval"></a>
<a id="what-users-see"></a>
<a id="spec-placement-and-conformance"></a>
<a id="other-tools"></a>
<a id="rust-and-clippy"></a>
<a id="go-analysis-and-vet"></a>
<a id="eslint"></a>
<a id="ruff"></a>
<a id="dylint"></a>
<a id="coverage-carried-into-this-proposal"></a>
<a id="semantics"></a>
<a id="types-permissions-and-requirements"></a>
<a id="bindings-scopes-and-generated-code"></a>
<a id="control-flow-and-observations"></a>
<a id="first-lint"></a>
<a id="later-lint-lost-write-to-a-mut-parameter"></a>
<a id="porting-the-deleted-analysis"></a>
<a id="later-lint-unused-capability-in-hdtoml"></a>
<a id="implementation-choices"></a>
<a id="placement-and-existing-facts"></a>
<a id="data-structures-and-data-flow"></a>
<a id="driver-pseudo-code"></a>
<a id="first-lint-pseudo-code"></a>
<a id="caching-and-testing"></a>
<a id="command-and-configuration"></a>
<a id="output-and-status"></a>
<a id="custom-lints"></a>
<a id="fixing-is-an-open-option"></a>
<a id="stress-test"></a>
<a id="surface-and-method"></a>
<a id="receiver-places-and-shadowing-s1-s3"></a>
<a id="writer-and-argument-escape-s4-s5"></a>
<a id="generic-associated-and-dynamic-types-s6"></a>
<a id="captured-and-global-escape-s7-s8"></a>
<a id="repeated-text-and-intermediate-observers-s9"></a>
<a id="exits-and-cleanup-s10"></a>
<a id="suspension-and-implicit-cleanup-s11"></a>
<a id="panic-and-declared-exclusions-s12-s15"></a>
<a id="problems-found-ranked"></a>
<a id="implementation-order-and-validation"></a>
<a id="recommendation"></a>
<a id="evidence-and-parse-log"></a>

Legacy fragments above remain for existing links; their earlier text is in git history.

The current registry is ten constants plus ten optional `Policy` fields
(`hd_lint/src/lib.rs:22–80`). CLI invokes four analyzers after its error gate
(`hd_cli/src/check_cmd.rs:268–301`). The driver has **two** collection switches,
not four: derivation facts share `collect_source_facts`. Three result channels
use separate backing (`hd_driver/src/lib.rs:602,1399`). New rule names have
repeated those integration sites.

The existing tables are intentionally narrow: primitive comparison types,
binding use/mutability, variant field names, reachability and debug identities.
They are not a general typed query context. `Output` does not retain the type,
path or semantic-symbol pools. Local type IDs have generations; copying their
numbers cannot turn them into owned results. TIR types/source nodes help only
before reference/place transformations erase distinctions.

| Existing lint (all default Warn) | Shared view it reads |
| --- | --- |
| `unsigned-comparison-always` | Checked expression type, operator and unsuffixed-zero syntax. |
| `unused-local-binding` | Local-binding kind/identity/name and final reads/captures. |
| `redundant-let-mut` | Binding annotation/permission, written mut span and mutable-address uses. |
| `variant-binding-name-mismatch` | Pattern resolution and nominal variant field order. |
| `unreachable-code` | Source-statement reachability and sub-body owner. |
| `unused-debug-text` | Implicit discard, actual selected declaration, call syntax and fixed prelude identities. |
| `mixed-script-identifier` | Valid plain identifier tokens; no type view. |
| `unselected-main` | Selected module role, public-main span and executable graph. |
| `unused-derivation-fact` | Resolved block/decorator occurrence and consumed marker. |
| `derivation-line-drift` | Nominal target/trait-package identity, member lines and sibling derives. |

Per-file baseline below isolates checked-body + syntax analyses on prepared
file facts; it excludes collection, module/derivation joins and fact filtering.
Those omissions are included in the corpus figures above. Seven samples each:

| File | µs | File | µs |
| --- | ---: | --- | ---: |
| `annotation.hd` | 0.334 | `cli.hd` | 5.292 |
| `cmp.hd` | 187.625 | `collections.hd` | 9.167 |
| `console.hd` | 0.709 | `convert.hd` | 0.125 |
| `digest.hd` | 1.708 | `encoding.hd` | 1.792 |
| `error.hd` | 0.583 | `format.hd` | 250.500 |
| `fs.hd` | 5.542 | `function.hd` | 0.125 |
| `hash.hd` | 3.458 | `host.hd` | 0.834 |
| `http.hd` | 642.125 | `http_wire.hd` | 0.125 |
| `inspect.hd` | 0.167 | `iter.hd` | 2.250 |
| `json.hd` | 20.416 | `net.hd` | 1.584 |
| `num.hd` | 6.250 | `ops.hd` | 1.459 |
| `option.hd` | 0.333 | `path.hd` | 0.584 |
| `prelude/testing.hd` | 0.125 | `prelude.hd` | 0.167 |
| `process.hd` | 0.834 | `random.hd` | 1.625 |
| `regex.hd` | 16.542 | `result.hd` | 0.334 |
| `rt/alloc.hd` | 104.916 | `rt/checked.hd` | 31.000 |
| `rt/checked_report.hd` | 0.208 | `rt/cycle.hd` | 73.583 |
| `rt/memory.hd` | 0.375 | `serde.hd` | 2.500 |
| `structure.hd` | 0.708 | `task.hd` | 112.625 |
| `testing/arbitrary.hd` | 0.166 | `testing.hd` | 4.417 |
| `text.hd` | 10.959 | `time.hd` | 5.334 |

All timed facts were frozen outside the analysis clock; no source IO or CLI startup was timed.


The earlier extended lint inventory, future-rule algorithms and plugin/export
ideas remain in #651 and git history. They are not new accepted behavior.
Operational decisions from #855 remain fixed: unknown manifest keys are load
errors; run/build without a public entry report `missing-entry-point` and name
the private `main`; failed `doc --open` is a command failure retaining its path.
Their implementation status does not turn them into registry-controlled warnings.

## E. Signatures, invariants and delivery

`AnalysisRequest`/`AnalysisSink` belong to `hd_driver`; neutral checked rows
and `CheckedView` belong to `hd_check`. A driver-supplied checker hook forwards
the completed view while its pool lives. Resolver/project views use existing
lower-layer data. `hd_lint` may depend on the driver; the compiler never depends on `hd_lint`. No new crate or per-rule callback channel is required.

Illustrative Rust interfaces, not implemented APIs:

```rust
struct CheckedView<'v> { /* frozen source rows, live borrowed query context */ }
impl<'v> CheckedView<'v> {
    fn expr_type(&self, expr: SourceExpr) -> Option<TypeRef<'v>>;
    fn adjusted_type(&self, expr: SourceExpr) -> Option<TypeRef<'v>>;
    fn pattern_type(&self, pat: SourcePattern) -> Option<TypeRef<'v>>;
    fn source_permission(&self, expr: SourceExpr) -> Permission;
    fn binding(&self, id: SourceBinding) -> BindingView<'v>;
    fn statement(&self, id: SourceStatement) -> StatementView<'v>;
    fn resolution(&self, call: SourceExpr) -> Option<Resolution<'v>>;
    fn adjustments(&self, expr: SourceExpr) -> &[ReceiverAdjustment<'v>];
    fn written_name(&self, expr: SourceName) -> Option<NameResolution<'v>>;
    fn declaration(&self, id: DeclRef<'v>) -> DeclarationView<'v>;
    fn trait_facts(&self, id: DeclRef<'v>) -> TraitView<'v>;
    fn inferred_row(&self, owner: SourceBody) -> RowState<'v>;
}
enum Resolution<'v> {
    Concrete { method: DeclRef<'v>, impl_: Option<DeclRef<'v>>, args: TypeArgs<'v> },
    Bound { method: DeclRef<'v>, bound: BoundRef<'v>, args: TypeArgs<'v> },
    TraitValue { method: DeclRef<'v>, receiver: TypeRef<'v>, args: TypeArgs<'v> },
    Builtin { operation: Builtin, result: TypeRef<'v> },
    Indirect { signature: TypeRef<'v> }, // valid function-value call
}
enum RowState<'v> { Known(RowRef<'v>), Pending, Unavailable }
```

`expr_type` describes the checked source type; `adjusted_type` describes the
effective type after recorded coercions/reads. Unsigned comparisons use that
effective operand type; source reference permissions remain separate.
Private constructors bind source/body/type handles to one live view. Types expose
primitive/nominal/argument/reference/permission information. Declaration and trait
views expose signatures, layouts, bounds, associated items and already-settled
evidence in the actual parameter/impl environment. Pending is neither No nor Yes.
A later solver query needs its own environment, budget and side-effect contract.

`written_name` exposes the checker's result for a source name, not a new name
resolver. An arbitrary `lookup_at` query and retained scope-history graph are
deferred until a real lint needs them. The first ten do not: `debug` and `dbg`
are unshadowable prelude names (`module.prelude.no-shadow`). A method with the
same name is different; consult its actual call target, never a shortened path.

The driver already moves completed `OnceLock<Parse>` values into shared position
inputs at finish. Expose only the selected-source syntax/span/text facade needed
for `SourcePlan`, reusing `Output.texts`; no deep clone or type-pool retention.
On a warm hit where source metadata is absent, materialize only the requested
missing syntax/tokens once; do not rerun bodies just for source policy. Reuse
an already-completed parse rather than reparsing it.
Retained run-interner token symbols remain opaque. Source keys include compilation
member, source snapshot and body/sub-body identity; a raw FileId/NodeIdx alone is
not a cross-run key. Type IDs and inference state never enter an owned Finding.

Keep requests to syntax/module metadata on true warm Check paths. A checked-view
request rechecks selected root bodies once; dependency interfaces remain reusable.
An optional-data miss must collect/rebuild, never silently lose findings. No new
normal Check key is needed solely for the registry. If an existing serialized
neutral format changes, version that format centrally. Future persistent typed
artifacts need owned backing/remapping before policy-free caching can be proposed.

Trial checkpoints include updates to earlier rows, not only vector truncation.
Freeze before source lowering; publish only after verification while the pool
lives. Dispatch once per (source snapshot, body/sub-body, node, event kind), not
per TIR instruction. M1 result replay never re-emits callbacks. Final module
events contain only their own valid declaration backing.

Policy validation is a source-level floor even with zero active checks: unknown
suppression names still error without typed rechecking. Call/operator findings
use their direct statement owner. A declaration/module finding has no invented
call site; this design does not broaden site suppression. In particular, ordering
operators are semantic PartialOrd calls (`expr.ord.partial-cmp`), so the existing
comparison-suppression fixture remains valid without a CallExpr-only filter.

| Component | Change and acceptance budget |
| --- | --- |
| Registry/policy | Remove ten optional fields/name arms; add one entry/check plus indexed levels; assert unique codes. |
| Driver/checker | Delete three result channels and per-rule integration; add one optional sink and requested neutral rows. |
| Lint off | Zero optional rows, allocations, query/traversal work or cache payload; ordinary guest bytes unchanged. |
| Lint on | Collect requested families once; measure collection, rules, peak memory and allocations separately. |
| Compile/runtime | No guest runtime changes; same-profile normal compile controls must show no meaningful regression. |
| Web compiler | No hd_lint dependency; frozen source-equivalent baseline 5,103,008 B raw / 1,707,892 B gzip level 9, below 2 MB gzip. |

The web control was built at `3b0e4cb4b`; its protected compiler sources match
`f4ddc82ee`. New registry, row-storage and web-size deltas remain unmeasured.

Implementation slices, each about an hour and independently reviewed:

1. Registry/indexed policy and owned Finding; keep a reference dispatcher and exact ten-rule snapshots.
2. One request/sink + selected source/module/declaration views; cold/warm cache and scope tests.
3. Borrowed type/binding/flow view; freeze/verify timing and type-lifetime controls.
4. Written-name/call/declaration views; rollback, M1, closures and fixed-prelude controls.
5. Move all ten checks to registered dispatch; delete old flags/results/analyzer entry points; exact diagnostic/fix parity.
6. One policy/suppression finish path, all-Allow validation and workspace error scopes; resume remaining #855 producers on these view families.

Adding a lint needs one metadata/check entry plus specification/fixture changes
for a genuinely new rule; stable diagnostic codes continue to be generated from
the spec. Test enabled/disabled behavior, exact names/messages/spans/fixes, type
and name lookalikes, prerequisites and selected roots. Compare optimized buckets
with an unfiltered reference; test cold/warm orderings, source/iface/Check hits,
serial/pool equality and deterministic dedup. Fix tests compare exact output,
reparse/recheck it and reject new syntax errors; no new applying command is proposed.

A compile-fail API test ensures a TypeRef cannot escape its callback.
No-request controls assert zero optional rows/events/findings/allocations and no
new recheck/cache payload. Compare normal guest bytes, cold/warm compile times,
web raw/gzip and memory on the same sources before allowing rollout. Lint-on
controls separate collection/query time from rule time and include all std plus
adversarial deeply nested scopes and large derivation sets. No candidate speed,
allocation or size number is claimed by this design-only PR. All retired IDs: none.
