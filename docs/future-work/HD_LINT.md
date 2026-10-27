# hd lint

Architecture approved 2026-10-27; [#908](https://github.com/HerringtonDarkholme/hd-lang/issues/908). Slice 0 (#916) is done; the #855 migration follows the slices below.
The accepted command/policy rules remain in [the CLI spec](../../spec/cli/command-line.md#linting).
This replaces the implementation plan, not those decisions.

## The proposal

Register each lint once: stable code, default level, event kinds and check.
Event kinds derive their compiler-view needs. On a lint-cache miss, run every
registered rule over one verified, source-order walk, store policy-free owned
diagnostics, and then apply policy. Normal checking, building and the web compiler
do not run lint.

```rust
type Check = fn(&Event<'_>, &CheckedView<'_>, &mut DiagBuf);

fn unsigned_comparison(
    event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf,
) {
    // Inspect Events::COMPARISON with view.adjusted_type(...), then emit.
}

static RULES: &[Rule] = &[
    Rule::new(Code::UnsignedComparisonAlways, Level::Warn,
        Events::COMPARISON, unsigned_comparison),
];

const fn needs(events: Events) -> Needs {
    // COMPARISON => TYPES; BINDING => USES; STATEMENT => FLOW; ...
}

enum Event<'v> {
    Comparison { op: CmpOp, left: SourceExpr, right: SourceExpr },
    Binding(SourceBinding),
    Statement(SourceStatement),
    Pattern(PatternEvent<'v>),
    Discard(SourceCall),
    Ident(Span),
    Decl(DeclEvent<'v>),
    Module(ModuleEvent<'v>),
}
```

Adding a rule changes one registry entry, its check, and one firing plus one
quiet fixture. It does not add a `Policy` field, driver channel, cache kind or
manual name arm. The name comes from the stable diagnostic code. Needs are a
closed mapping from events, so a registry entry cannot under-declare them.

## The problems in programs

This valid comparison needs the checked operand type, not spelling alone:

```hd
fn expired(remaining: usize) -> bool:
    remaining < 0
```

Today four analysis calls collect three fact results and emit
`unsigned-comparison-always`. The proposal emits the same code, span, message
and fix from one `COMPARISON` event and `adjusted_type(remaining)`.

This valid suppression is accepted policy but is still a known failure today:

```hd
fn expired(remaining: usize) -> bool:
    # lint-allow: unsigned-comparison-always
    remaining < 0
```

Today `cli/lint-site-allow` can still report the warning. The proposal attaches
the full-line comment to the following statement and filters the owned finding
after loading or computing it; the result is quiet.

This program is invalid because the tail has type `i32`, not `void`:

```hd
struct Logger: pass

impl Logger:
    fn debug(&self, value: i32) -> i32: value

fn user_call() -> void:
    Logger {}.debug(+1)
```

Today `hd check` incorrectly accepts it and lint mistakes the method for prelude
`debug`. [#916](https://github.com/HerringtonDarkholme/hd-lang/issues/916) is a
checker soundness prerequisite: call checking records the selected declaration,
the void-tail rule reports `type-mismatch`, and lint emits nothing for a failed
prerequisite. The architecture must consume that identity, not reproduce the bug.

## Architecture

`hd_lint` owns rule selection, predicates, messages, fixes and policy.
The driver exposes a neutral optional analysis consumer under an `analysis`
feature and never imports `hd_lint`. `hd_cli` enables that feature; `hd_web`
does not, so the rows, callbacks and cache code are compiled out of the web build.
The checker/resolver publish compiler observations, not warning decisions.
The sink borrows completed compiler state while a callback runs; no owned type
graph is exported, and its references cannot escape the callback lifetime.

```rust
pub trait AnalysisSink {
    fn interests(&self) -> Events;
    fn event(&self, event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf);
}

fn check_module(..., analysis: Option<&dyn AnalysisSink>) -> Output;
```

The sink covers every registered event, because diagnostics are independent of
which rules policy currently allows. The driver walks each verified body's
frozen source syntax once, in source order, and calls interested rules. It also
delivers explicit declaration/module finish events. Rules query only the
completed state needed by the first ten lints:
`adjusted_type`, `binding`, `statement`, and the actual selected declaration of
a call, plus source syntax/text. Future type, trait, row, adjustment and generic
resolution APIs are not built until a lint demonstrates a need.

A rule emits stable codes, spans, messages, labels and fixes directly into a
`DiagBuf`. No body-local type, definition or symbol ID escapes. Diagnostic rows
carry Statement, Declaration, Module or Package owners tied to the source
snapshot. Worker-local buffers are globally sorted after collection. Compiler
errors discard lint diagnostics for the failed selected build; policy never
suppresses compiler errors.

Store each module's policy-free `DiagBuf` in a separate cache entry keyed by
`(EntryKind::Lint, CheckKey)`. The compiler build ID already covers `hd_lint`
code and the diagnostic schema. A miss checks only that module, runs every
registered rule and serializes its buffer; other modules remain cache hits. A
hit loads diagnostics without retaining or rebuilding a type graph. Cache spans
use the existing `Lines` encoding: code-line index plus offset, so inserting a
comment or blank line shifts decoded spans without invalidating `CheckKey`.
Default levels, manifest/CLI policy and source suppressions remain outside the
entry. The ordinary `Check` entry is unchanged. Thus the measured 5.4x warm
regression is not accepted as the steady-state design.

Resolve default → selected manifest → explicit CLI levels once. Validate exact
names/conflicts before cache lookup. Apply site allow last to calls directly
owned by its following statement, including semantic operator calls. It does not
flow into nested functions/closures or declaration/module diagnostics. A valid
allow that matches no finding is quiet. Naming a code that site allow can never
reach is a configuration error; implementation adds the new CLI-tier rule
`cli.lint.site-allow-reachability`. Render the existing codes, messages, spans,
fixes, ordering and statuses. No new command, fix mode, plugin mechanism or
suppression syntax is added.

For example, `# lint-allow: unused-local-binding` is a configuration error:
“site allow cannot reach `unused-local-binding`; allow it in `[lint]`.”

Share the existing parse/source/token view and optional neutral comment anchors
with the [formatter proposal](https://github.com/HerringtonDarkholme/hd-lang/pull/911).
The formatter must keep a full-line `# lint-allow:` immediately before its
following statement; its round-trip suite gets a suppression fixture. Formatter
never requests semantic facts, and neither tool causes analysis work when absent.

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
New architecture costs are unmeasured until approval. Publish corpus, allocation
and cold/warm receipts against the same source hashes and profile.

## A. Stress test

Real-code translations use local references and are checked with the current
compiler, not an implementation of the proposed views. The current compiler
accepts all six inputs; the sixth acceptance is the #916 soundness bug. Current
lint outcomes are recorded below.

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
    fn debug(&self, value: i32) -> i32: value

fn user_call() -> void:
    Logger {}.debug(+1)
```

Current `hd check` incorrectly accepts this non-void tail and `hd lint` also
emits `unused-debug-text`; [#916](https://github.com/HerringtonDarkholme/hd-lang/issues/916)
records the soundness bug. The path-based classifier drops a constructor
receiver. Fix #916 first by recording the checked call's selected declaration;
both the void-tail exception and lint then consume that identity. Preserve valid
prelude-call behavior without enshrining this false positive in the new API.

| Adversary | Required result/invariant |
| --- | --- |
| A module/local declaration named debug or dbg | Existing `prelude-name-shadow`; no lint findings from an invalid prerequisite. |
| Generic bound, trait value, builtin or indirect function call | No invented concrete declaration; only rules that need these forms may add a view. |
| Overload trials and M1 result replay | No rejected/duplicate source events or stale type IDs. |
| Captures/defer and nested closures | Final use/address facts; separate source ownership and suppression scope. |
| Two packages/types with the same displayed name | Nominal declaration identity, not string comparison. |
| All levels Allow plus an unknown suppression name | Configuration error, with no typed lint recheck. |
| Workspace member failure, warm Check/interface hits, changed source | Preserve per-build prerequisite scope, collect missing required views, no stale findings. |

Parse log: all three main `hd` programs and all six `text` blocks parse; the
current compiler accepts all nine fences, including the duplicated deliberately
invalid #916 control.
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
Borrowed callbacks are the chosen design for ten rules. The separate Lint cache
persists only an owned `DiagBuf`, so a warm hit needs neither a type graph nor a
body recheck.

Keeping independent fact channels repeats today's coupling. Exporting raw TIR
or a naked `Ty` makes ownership/source-loss problems the lint's responsibility.
Retaining `BodyCx` or rerunning its checker adds mutable inference state and
unnecessary lifetime/cost. Copying a standalone JS/Python name model supplies
no hd trait/type answers. None is the recommended first implementation.

## D. Today in hd

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

| Existing lint (all default Warn) | Shared view it reads | Diagnostic owner | Site allow reaches it? |
| --- | --- | --- | --- |
| `unsigned-comparison-always` | Checked expression type, operator and unsuffixed-zero syntax. | Statement | Yes: the operator is its directly owned semantic call. |
| `unused-local-binding` | Local-binding kind/identity/name and final reads/captures. | Statement | No: the binding is not a call. |
| `redundant-let-mut` | Binding annotation/permission, written mut span and mutable-address uses. | Statement | No: the binding is not a call. |
| `variant-binding-name-mismatch` | Pattern resolution and nominal variant field order. | Statement | No: the pattern is not a call. |
| `unreachable-code` | Source-statement reachability and sub-body owner. | Statement | No: reachability is not a call. |
| `unused-debug-text` | Implicit discard, actual selected declaration, call syntax and fixed prelude identities. | Statement | Yes: it is the directly owned call. |
| `mixed-script-identifier` | Valid plain identifier tokens; no type view. | Declaration or statement | No: the identifier is not a call. |
| `unselected-main` | Selected module role, public-main span and executable graph. | Declaration | No. |
| `unused-derivation-fact` | Resolved block/decorator occurrence and consumed marker. | Declaration | No. |
| `derivation-line-drift` | Nominal target/trait-package identity, member lines and sibling derives. | Declaration | No. |

The table preserves `cli.lint.site-allow`: a site allow reaches only calls
directly owned by its following statement. A valid allow that reaches a code but
matches no finding is quiet. Naming one of the other eight codes is a
configuration error under proposed `cli.lint.site-allow-reachability`.

Prepared per-file analysis ranged from **0.125–642.125 µs** over seven samples;
source IO, CLI startup, collection and module joins were excluded.


The earlier extended lint inventory, future-rule algorithms and plugin/export
ideas remain in #651 and git history. They are not new accepted behavior.
Operational decisions from #855 remain fixed: unknown manifest keys are load
errors; run/build without a public entry report `missing-entry-point` and name
the private `main`; failed `doc --open` is a command failure retaining its path.
Their implementation status does not turn them into registry-controlled warnings.
Manifest precedence and site allow do not work today; `cli/lint-manifest-precedence`
and `cli/lint-site-allow` remain in root `KNOWN_FAILURES.tsv` under #855. The
policy/suppression slice below is new implementation, not code moved from the
current command.

## E. Signatures, invariants and delivery

`AnalysisSink` belongs to `hd_driver`; neutral checked rows and `CheckedView`
belong to `hd_check`. Driver entry points take `Option<&dyn AnalysisSink>` and a
driver-supplied checker hook forwards
the completed view while its pool lives. Resolver/project events use existing
lower-layer data. `hd_lint` may depend on the driver; the compiler never depends
on `hd_lint`. No new crate or per-rule callback channel is required.

Illustrative Rust interfaces, not implemented APIs:

```rust
struct CheckedView<'v> { /* frozen source rows, live borrowed query context */ }
impl<'v> CheckedView<'v> {
    fn adjusted_type(&self, expr: SourceExpr) -> Option<TypeRef<'v>>;
    fn binding(&self, id: SourceBinding) -> BindingView<'v>;
    fn statement(&self, id: SourceStatement) -> StatementView<'v>;
    fn selected_declaration(&self, call: SourceCall) -> Option<DeclRef<'v>>;
    fn source(&self) -> SourceView<'v>;
}
```

`adjusted_type` supplies the effective operand type for unsigned comparisons.
`binding` supplies final use, capture, permission and mutable-address facts.
`statement` supplies reachability and sub-body ownership. Pattern identity and
field order travel in the pattern event; they do not justify a general
`pattern_type` query. `selected_declaration` is the concrete identity already
chosen by call checking and is the only call-resolution query needed now. The
first ten do not need generic trait facts, inferred rows, receiver-adjustment
lists, arbitrary name lookup or a public resolution enum. Those remain a future
sketch, with no API, until a registered lint requires one.

Private constructors bind `TypeRef` and `DeclRef` to one live view. The first
implementation exposes only primitive/reference facts needed by the rules, not
signatures, layouts, bounds or a solver. A method named `debug` is distinct from
the fixed prelude declaration; #916 makes the checker record that distinction.

The driver already moves completed `OnceLock<Parse>` values into shared position
inputs at finish. Expose only the selected-source syntax/span/text facade needed
for `SourcePlan`, reusing `Output.texts`; no deep clone or type-pool retention.
On a warm hit where source metadata is absent, materialize only the requested
missing syntax/tokens once; do not rerun bodies just for source policy. Reuse
an already-completed parse rather than reparsing it.
Retained run-interner token symbols remain opaque. Source keys include compilation
member, source snapshot and body/sub-body identity; a raw FileId/NodeIdx alone is
not a cross-run key. Type IDs and inference state never enter the cached
`DiagBuf`.

Each module's separate cache entry is `(EntryKind::Lint, CheckKey) -> DiagBuf`.
The compiler build ID already invalidates changes to rule/event semantics,
messages, fixes and the diagnostic schema. Default/manifest/CLI levels and site
allows remain outside the entry. A miss requests all event families, checks that
module, runs every rule and writes diagnostics only after success. A hit
deserializes diagnostics and applies current policy. The normal Check format and
key remain untouched.

Serialize every primary, label and fix-edit span through the existing `Lines`
code-line-relative encoding used by Check entries. Because `CheckKey` ignores
plain comments and blank lines, a comment-only insertion remains a hit and
decodes each cached span against the new line starts. A focused test inserts a
comment line and requires a hit with every reported span shifted to the same code.
Another edits one module and requires only that module's Lint entry to miss.

Derivation rules follow the same boundary: a miss performs module preparation,
checking and derivation joins before caching all findings; a hit loads them.
There is no silent derivation-lint omission and no warm-cache body recheck. A
focused `derivation-line-drift` test covers cold miss, warm hit and source change;
other rules rely on the common cache contract.

Freeze rows after verification and before source lowering erases distinctions.
Then walk syntax once and dispatch once per (snapshot, body/sub-body, node,
event kind), not per checker trial or TIR instruction. Rejected overload trials,
rollback and M1 replay cannot emit because no rule callback runs during inference.
Final module events contain only valid declaration backing.

Policy validation is a source-level floor even with zero active checks: unknown
suppression names and codes that site allow cannot reach error without typed
rechecking. A reachable allow with no matching finding is quiet. Call/operator
diagnostics use their direct statement owner. A declaration/module diagnostic
has no invented call site; this design does not broaden site suppression. In
particular, ordering operators are semantic PartialOrd calls
(`expr.ord.partial-cmp`), so the existing comparison-suppression fixture remains
valid without a CallExpr-only filter.

| Component | Change and acceptance budget |
| --- | --- |
| Registry/policy | Remove ten optional fields/name arms; add one entry/check plus indexed levels; assert unique codes. |
| Driver/checker | Delete three result channels and per-rule integration; add one optional sink and requested neutral rows. |
| Lint off | Zero optional rows, allocations, query/traversal work or cache payload; ordinary guest bytes unchanged. |
| Lint on | Miss: collect and run all rules once; hit: load an owned `DiagBuf`; measure both paths, peak memory and allocations. |
| Compile/runtime | No guest runtime changes; same-profile normal compile controls must show no meaningful regression. |
| Web compiler | Leave the driver/checker `analysis` feature off and keep no hd_lint dependency; frozen source-equivalent baseline 5,103,008 B raw / 1,707,892 B gzip level 9, below 2 MB gzip. |

The web control was built at `3b0e4cb4b`; its protected compiler sources match
`f4ddc82ee`. New registry, row-storage and web-size deltas remain unmeasured.

Implementation slices, each about an hour and independently reviewed:

0. Fix checker soundness prerequisite #916; both void-tail checking and lint read the checked call's selected declaration.
1. Registry/indexed policy and policy-free `DiagBuf`; exact ten-rule snapshots.
2. One optional sink, minimal source/module/declaration events and the `analysis` feature; off/web controls.
3. Minimal borrowed type/binding/flow/callee view; post-verification walk and type-lifetime controls.
4. Per-module policy-free Lint cache keyed by Check key, line-relative spans; cold miss, warm hit, comment-shift, module invalidation and derivation controls.
5. Move all ten checks to registered dispatch; delete old flags/results/analyzer entry points; exact diagnostic/fix parity.
6. Implement the one policy/suppression finish path, all-Allow validation and workspace error scopes; resume remaining #855 producers on these view families.

Adding a lint needs one metadata/check entry plus specification/fixture changes
for a genuinely new rule; stable diagnostic codes continue to be generated from
the spec. Every registered code must have at least one firing fixture and one
quiet fixture. Its `cases.tsv` firing row names the exact expected finding, for
example `lint:unsigned-comparison-always`; an accept-only case is not coverage.
The harness rejects a registry code without both cases and checks exact names,
messages, spans and fixes. Test enabled/disabled policy, lookalikes, prerequisite
failure and selected roots. Test cold miss/warm hit, comment-only span shifts,
single-module invalidation, build-ID invalidation, serial/pool equality and
deterministic dedup. Fix tests compare exact output, reparse/recheck it and
reject new syntax errors; no new applying command is proposed. The formatter's
round-trip fixtures must include a site allow that remains immediately attached.

A compile-fail API test ensures a TypeRef cannot escape its callback.
No-request controls assert zero optional rows/events/findings/allocations and no
new recheck/cache payload. Compare normal guest bytes, cold/warm compile times,
web raw/gzip and memory on the same sources before allowing rollout. Lint-on
controls separate collection/query time from rule time and include all std plus
adversarial deeply nested scopes and large derivation sets. No candidate speed,
allocation or size number is claimed by this design-only PR. All retired IDs: none.
