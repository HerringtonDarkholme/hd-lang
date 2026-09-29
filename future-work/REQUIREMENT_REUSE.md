# Requirement Reuse: Stress Test And Design Options

Status: design review, 2026-09-28; changes no decision, design record, spec
text, or prototype code. Nothing here is accepted language behavior, and
every design choice below is a question for the owner.

The owner called requirement reuse "a real serious issue" and asked four
questions:

1. How is a long requirement row reused?
2. How is a bundle of providers reused where `$.with` installs it?
3. Can rows or bundles be abstracted, and is there a strong need?
4. How do other languages do it?

This record stress-tests the current design on production-shaped code and
then compares six options, including the owner's proposal to make rows
type-level expressions. It reviews these sections:

- in chapter 11, [Requirement Rows](../spec/11-requirements-and-suspension.md#requirement-rows)
  (with [Omitted Requirement Clauses](../spec/11-requirements-and-suspension.md#omitted-requirement-clauses),
  [Entailment](../spec/11-requirements-and-suspension.md#entailment),
  [Least Row Solutions](../spec/11-requirements-and-suspension.md#least-row-solutions) and
  [Row Parameters](../spec/11-requirements-and-suspension.md#row-parameters)),
  [Provider Access](../spec/11-requirements-and-suspension.md#provider-access),
  [Provider Scopes](../spec/11-requirements-and-suspension.md#provider-scopes),
  [Reusable Contexts](../spec/11-requirements-and-suspension.md#reusable-contexts),
  [Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers) and
  [Requirement Polymorphism](../spec/11-requirements-and-suspension.md#requirement-polymorphism);
- in chapter 7, [Function Type Constructors](../spec/07-functions.md#function-type-constructors)
  and [Generic Function Values](../spec/07-functions.md#generic-function-values);
- in chapter 4, [Variance](../spec/04-type-system.md#variance) and
  [Transparent Aliases And Newtypes](../spec/04-type-system.md#transparent-aliases-and-newtypes);
- in chapter 10, [Member Visibility](../spec/10-modules.md#member-visibility),
  [Executable Entry Point](../spec/10-modules.md#executable-entry-point) and
  [Standard Testing](../spec/10-modules.md#standard-testing);
- the `std.testing.hermetic` bundle in
  [Standard Library, Testing Layer](STDLIB.md#testing-layer).

**Spelling.** Every hd example uses the operators decided in
[Bound And Row Operators](OPEN_ISSUES.md#bound-and-row-operators). They are
not yet in the spec or the parser. Rows are `$ Db + Clock` everywhere,
including type arguments such as `$.Context[$ Db + Clock]`, and bounds are
`T < A & B`.

The spelling leaves today's row semantics unchanged, so examples of
today's behavior use it too. The one semantic change in that decision is
item 4: a callback that lacks a key its callee installs still matches. The
[Parse Log](#parse-log) says how the examples were checked.

## Owner Decisions

Decided 2026-09-28.

1. **RU1 (question 1): installation reuse needs no new feature.** An
   installer function is the reusable bundle, for example
   `with_stack!(config): ...`, which extends its callback's row and
   installs providers with `$.with`. Build-once reuse keeps a context value.
2. **RU2 (question 4): Option F, row aliases.** `type AppRow = Db + Cache +
   Clock + Log` is an ordinary transparent alias over requirement keys. It
   is used only where a row is written, after `$` in function headers and
   function types, because of RU3.
3. **RU3 (questions 8 and 9): row parameters on functions only.** Data
   types, enums and traits take no row parameters, and `$.Context[R]` takes
   none. A shared handler table uses a concrete row
   (`List[fn(Request) -> Response $ AppRow]`) or the open-row idiom
   (question 2).
4. **RU4 (question 10): at most one unknown row variable per row
   pattern.** So `f: fn() $ R1 + R2` is rejected.

5. **RU5 (question 2): row subsumption.** A function value whose row is a
   subset of another function type's row fits that type, everywhere:
   `[health!, get_order!]` fits `List[fn!(Request) -> Response $ AppRow]`.
   A function needing a key outside the wider row is still an error. This
   extends Bound And Row Operators item 4 (a callback lacking a key still
   matches) to all function values. The compiler may adapt the value
   invisibly. The open-row idiom is not needed.
6. **RU6 (question 3): moot.** Under RU5, a closure whose inferred row is
   narrower already fits a wider expected row, so closures don't need to
   take the expected row.
7. **RU7 (question 5): the empty row stays `$()`.**
8. **RU8 (question 6): diagnostics print an aliased row as written,** and
   a mismatch error lists the expanded keys and names the missing one.
9. **RU9 (question 7): `pub` functions keep explicit rows** in every
   package. Aliases (RU2) reduce the typing instead of inference.

All ten questions are decided.

**Follow-up decisions (owner, 2026-09-28), from the apply pass's Still
Open:**
10. **RU10: a bare row alias is allowed in a one-key slot,** like `Fn`:
    `$.Context[AppRow]` works, and the checker reads `AppRow` as a row
    because it is a row alias. This reverses Still Open 2's applied
    reading.
11. **RU11: impl methods keep their trait's row** for now
    (`req.row.callable.impl-agrees` is unchanged).
12. **RU12: a list or collection literal with no expected type infers the
    union row** of its function elements: `[health, orders]` gets
    `fn(...) $ Clock + Db + ...`. This is a new inference rule.
13. **RU13: when a forced `R` overlaps a key the callee installs, the
    nearest provider wins** (dynamic scoping), as a nested `$.with` does.
    The owner chose this over rejecting the overlap and over lexical
    (tunneling) semantics.
14. **RU14: the other applied readings stay** (Still Open 1, 3-9, 11 and
    13, as amended by RU10 and RU12). The owner confirmed two of them:
    - `split`-style declarations are rejected (`ambiguous-row-pattern`),
      even when explicit type arguments are available;
    - containers of functions don't convert. A list is invariant, as the
      variance rules already say, so a wider list is made by an explicit
      copy (`narrow.map(fn(f): f)`).

**Applied 2026-09-28.** The specification now states each decision:

| Decision | Specification |
| --- | --- |
| RU1, RU6 | No language change; the [tour](../guide/LANGUAGE_TOUR.md#requirements-and-suspension) shows an installer, and `runtime/valid/installer-function-runs.hd` runs one. |
| RU2 | [Row Aliases](../spec/11-requirements-and-suspension.md#row-aliases), [Type Declarations](../spec/02-grammar.md#type-declarations), and the alias cycle rule [`types.alias.cycle`](../spec/04-type-system.md#r-types.alias.cycle) (new code `alias-cycle`). |
| RU3 | [Row Parameters](../spec/11-requirements-and-suspension.md#row-parameters) (new code `row-parameter-in-context`). |
| RU4 | [`req.row.least.ambiguous`](../spec/11-requirements-and-suspension.md#r-req.row.least.ambiguous) (new code `ambiguous-row-pattern`). |
| RU5 | [Row Subsumption](../spec/11-requirements-and-suspension.md#row-subsumption) and [`types.assign.row-subsumption`](../spec/04-type-system.md#r-types.assign.row-subsumption). |
| RU7 | No change: [`req.row.syntax.empty`](../spec/11-requirements-and-suspension.md#r-req.row.syntax.empty). |
| RU8 | [Aliases In Diagnostics](../spec/11-requirements-and-suspension.md#aliases-in-diagnostics). |
| RU9 | No change: [`req.row.omitted.empty`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.empty), with a Why callout. |

## Still Open

Points the apply pass met (2026-09-28). Each waits for the owner; the
specification states the reading in the Applied column, so each can change
without breaking a decision.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | RU2 names "function headers and function types". Does a row alias also work in `$.Context[$ AppRow]` and `Fn[(), O, $ AppRow]`? | Yes: any row that follows `$` ([`req.row.alias.where`](../spec/11-requirements-and-suspension.md#r-req.row.alias.where)) | Keep: case 6 shares the prod and test contexts this way. |
| 2 | Is `$.Context[AppRow]`, without `$`, a row alias use? | No: the bare form takes one key, so a row alias there is `generic-kind-mismatch` ([`req.row.alias.kind`](../spec/11-requirements-and-suspension.md#r-req.row.alias.kind)) | Keep: RU2 places aliases after `$`, and Option F's `$`-less type argument was not adopted. |
| 3 | Edge case 8 calls the alias cycle rule "shared". Does it cover ordinary aliases? | Yes: every alias cycle is `alias-cycle`, reported once on the first declaration ([`types.alias.cycle`](../spec/04-type-system.md#r-types.alias.cycle)) | Keep: an ordinary cycle was silently undiagnosed. |
| 4 | Which code rejects a data, enum or trait parameter used in a row (RU3)? | `generic-kind-mismatch`: those parameters are type-kinded ([`req.row.param.no-data.error`](../spec/11-requirements-and-suspension.md#r-req.row.param.no-data.error)) | Keep: no new code. |
| 5 | Which code rejects a row parameter in `$.Context[...]` (RU3)? | The new `row-parameter-in-context` ([`req.row.param.context`](../spec/11-requirements-and-suspension.md#r-req.row.param.context)) | Keep: the parameter is row-kinded, so a kind error would mislead. |
| 6 | Edge case 12 asks for "one error" for `$.use(AppRow)` and `AppRow=value`. Which code? | `generic-kind-mismatch`, as for a row alias used as a type (edge case 10) | Keep: one code for every row used where a type or one key is needed. |
| 7 | RU3 says "only functions". Do implementation heads keep row parameters? | Yes: [`trait.target.row-argument`](../spec/09-traits.md#r-trait.target.row-argument) is unchanged, and [`req.row.param.callables`](../spec/11-requirements-and-suspension.md#r-req.row.param.callables) lists implementations | Keep: `impl[R] Marker for Fn[(), i32, $ R]` needs one. |
| 8 | May a function-type alias take a row parameter, as in `type Handler[R] = fn(Request) -> Response $ R`? | Yes: RU3 names data types, enums and traits only, and a transparent alias expands at its use | Keep. |
| 9 | Under RU4, do explicit type arguments rescue a declaration such as `split[R1, R2]`? | No: the check is at the declaration and counts only other parameters ([`req.row.least.ambiguous`](../spec/11-requirements-and-suspension.md#r-req.row.least.ambiguous)) | Keep: RU4 rejects `f: fn() $ R1 + R2` outright. |
| 10 | When two fixed row parameters share a key (the record's soundness point), which provider does the callback see? | Not stated | State that the nearer scope provides it, as a nested `$.with` does ([`req.with.nested`](../spec/11-requirements-and-suspension.md#r-req.with.nested)). |
| 11 | Does RU5 convert a container, such as a `List[fn() $ Db]` value to `List[fn() $ Db + Cache]`? | No: it converts a function value; variance keeps the row invariant ([Row Subsumption](../spec/11-requirements-and-suspension.md#row-subsumption), Note) | Keep: converting a container would copy it. |
| 12 | Does RU5 let an implementation's method declare a narrower row than its trait's? | No: [`req.row.callable.impl-agrees`](../spec/11-requirements-and-suspension.md#r-req.row.callable.impl-agrees) still requires agreement | Keep for now; a method is part of a trait contract, not a function value. |
| 13 | Does `[health, orders]` without an expected type get the union row? | No: RU5 applies only against an expected function type, so the list needs one | Keep: inferring a new row is not decided. |

## Contents

- [Owner Decisions](#owner-decisions)
- [Still Open](#still-open)
1. [Problem](#problem)
2. [What hd Has Today](#what-hd-has-today)
3. [Method](#method)
4. [Summary](#summary)
5. [Cases](#cases) (1-12)
6. [Survey](#survey)
7. [Problems, Ranked](#problems-ranked)
8. [Options](#options) (A-F)
9. [Comparison](#comparison)
10. [Ranking By Design Cost](#ranking-by-design-cost)
11. [Recommendation](#recommendation)
12. [Questions For The Owner](#questions-for-the-owner)
13. [Sources](#sources)
14. [Parse Log](#parse-log)

## Problem

A production service reaches many dependencies. A handler may touch a
database, a cache, a clock, a logger, metrics, configuration, an HTTP client
and an authenticator. In hd each is a requirement key, so the full row has
eight keys.

The owner's questions split into two sides:

| Side | Where the list is written | Owner question |
| --- | --- | --- |
| Declaration | the `$` clause of a function, closure, or function type | 1: how is a long row reused? |
| Installation | `$.with(...)`, `$.context(...)`, and `$.Context[Row]` | 2: how is a provider bundle reused? |
| Abstraction | either side | 3: aliases, bundles, layers, generic rows? |
| Prior art | both | 4: how do other languages do it? |

## What hd Has Today

hd already has six reuse tools. None of them was designed as a "row alias",
but together they cover more than the question suggests.

| Tool | Side | Rule | What it reuses |
| --- | --- | --- | --- |
| Inferred rows | declaration | [`req.row.omitted.inferred`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.inferred) | A non-public function, inherent method, local `fn`, or closure writes no row at all. |
| Row parameters | declaration | [`req.poly.row-parameter`](../spec/11-requirements-and-suspension.md#r-req.poly.row-parameter) | Higher-order code passes a caller's row through as one bundle, and extension removes one key. |
| Open rows | declaration | [`req.row.least.solution`](../spec/11-requirements-and-suspension.md#r-req.row.least.solution), [`fn.type.generic.instantiate-sources`](../spec/07-functions.md#r-fn.type.generic.instantiate-sources) | A function declared `[R]` with row `$ R + Clock` fits any wider function type; `R` is solved as the other keys. This is Koka's `<clock\|e>` idiom. |
| Function-type aliases | declaration | [`types.alias.same`](../spec/04-type-system.md#r-types.alias.same) | `type Handler = fn!(Request) -> Response $ Db + Cache` names a callable type with its row. |
| Context values | installation | [`req.context.create`](../spec/11-requirements-and-suspension.md#r-req.context.create) | A function returns `$.Context[Row]`, and `ctx...` spreads it, with later entries winning. |
| Context-type aliases | installation | [`types.alias.same`](../spec/04-type-system.md#r-types.alias.same) | `type AppContext = $.Context[$ Db + Cache]` names a bundle type. |

Four rules limit these tools, and the cases below keep hitting them:

| Limit | Rule | Effect |
| --- | --- | --- |
| Public rows are written | [`req.row.omitted.empty`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.empty) | A `pub` function without a clause has the empty row. |
| No package-private visibility | [`module.vis.no-package-private`](../spec/10-modules.md#r-module.vis.no-package-private) | Every function called from another module of the same app is `pub`, so it writes its row. |
| Rows are invariant | [`types.variance.function`](../spec/04-type-system.md#r-types.variance.function) | `fn() $ Db` is not a `fn() $ Db + Cache`, so values stored under one function type share one exact row. |
| A key is a trait | [`req.key.traits`](../spec/11-requirements-and-suspension.md#r-req.key.traits) | No name can stand for several keys in a `$` clause. `type AppRow = $ Db + Cache` is not a declaration form. |

The last limit is the gap the owner is asking about. A context-type alias
names the row on the installation side, but a header cannot mention it.

## Method

**Cases.** Twelve cases approximate one web service. They follow Rust
[axum](https://docs.rs/axum/latest/axum/) handlers and routers, Spring
controllers, Guice and Dagger modules, and ZIO and Effect-TS layers. The
library shapes are approximations, not ports. Trait and type names such as
`Db`, `Cache`, `Metrics`, `Request` and `Response` are placeholders.

**Counting rules.** A *key mention* is one trait name inside a `$` clause or
inside the row of a `$.Context[...]` type. A *row site* is one `$` clause or
one `$.Context[...]` row. Churn is the number of row sites that change when
one key is added.

**Verdicts.** *Works*: the design expresses the case directly. *Friction*:
it works with repetition, a wrapper, an idiom, or a visible cost. *Breaks*:
the design rejects the natural code, or the needed rule is unspecified.

**Limits.** Every `text` block was checked with the reference parser, after
the operator rewrite that the [Parse Log](#parse-log) describes. Parsing
checks syntax only; no block is claimed to type-check. `# error:` and
`# proposed:` comments state existing or proposed behavior, not a parser
result.

## Summary

| # | Case | Approximates | Verdict | Element at fault |
| --- | --- | --- | --- | --- |
| 1 | Eight-key handler rows | axum handlers, Spring controllers | Friction | public rows are written; no package-private visibility |
| 2 | One router table, mixed handler rows | axum `Router::route` | Friction | invariance rejects closed rows; the open-row idiom works |
| 3 | Add `Metrics` to a shared helper | any service | Friction | churn on every public caller |
| 4 | Composition root and layers | Guice modules, ZIO `ZLayer` | Works | none |
| 5 | Request-scoped bundle | Dagger request subcomponent, tower layers | Works | none |
| 6 | Test fixture swapping six providers | Spring `@TestConfiguration`, Guice `Modules.override` | Friction | the app row is spelled twice |
| 7 | Generic retry and middleware | tower `Layer`, ZIO aspects | Works | none |
| 8 | Generic router and job queue | axum `Router<S>`, job runners | Breaks | row parameters on data and in contexts unspecified |
| 9 | One bundle trait as the only key | Rust "god trait", cake pattern | Friction | rebinding per call; rows lose meaning |
| 10 | Service record, no rows | Go structs, Java constructor injection | Works | rows no longer show reach |
| 11 | Provider-installing functions | Koka `with` handlers, ZIO `provideLayer` | Works, with friction | rebuilds per call; `fn` and `fn!` twins |
| 12 | Two generic rows | `std.task.all!`, ZIO `provideSome` | Works under a proposed rule | no solving rule for several row variables |

The installation side works today. Context values are ordinary values
returned by ordinary functions, so layers, request bundles and fixtures
compose without new features. Provider-installing functions, which item 4
of the operator decision enables, add scoped reuse on top (case 11).

The declaration side hurts, but less than it first looks. Row invariance
rejects a table of handlers with closed rows, and the open-row idiom, which
hd already has, fixes that. What remains is that no name can stand for a
wide row, so the app row is spelled in full wherever it must stay wide.
Several row variables in one pattern also lack a solving rule (case 12).

## Cases

The cases share these declarations. The key list is the owner's example.

| Key | Kind | Provider in production | Provider in tests |
| --- | --- | --- | --- |
| `Db` | application trait, mutable | `PgPool` | `MemoryDb` |
| `Cache` | application trait, mutable | `RedisCache` | `MemoryCache` |
| `Clock` | host capability (`std.time`) | host | `ManualClock` |
| `Log` | application trait | `JsonLog` | `BufferLog` |
| `Metrics` | application trait, mutable | `StatsdMetrics` | `RecordingMetrics` |
| `Config` | application trait | `EnvConfig` | `FixedConfig` |
| `HttpClient` | application trait over the host `Network` | `HostHttp` | `ScriptedHttp` |
| `Auth` | application trait | `JwtAuth` | `AllowAll` |

### 1. Eight-Key Handler Rows

An axum handler takes its dependencies through extractors such as
`State<AppState>`, and a Spring controller takes them as constructor
parameters. Neither lists them per method:

```rust
async fn create_order(State(app): State<AppState>, Json(body): Json<NewOrder>) -> Response {
    // app.db, app.cache, app.metrics, ...
}
```

In hd, handlers live in `handlers/orders.hd` and the router lives in
`server.hd`. With no package-private visibility, every handler is `pub`,
so each writes its row:

```text
use std.time.Clock

pub fn create_order!(req: Request) -> Response $ Db + Cache + Clock + Log + Metrics + Config + HttpClient + Auth:
    pass

pub fn get_order!(req: Request) -> Response $ Db + Cache + Log + Auth:
    pass

pub fn health!(req: Request) -> Response $ Clock:
    pass
```

Each row is exact and useful: a reviewer sees that `health!` touches only
the clock. Private helpers inside `orders.hd` write no rows. A service with
30 handlers averaging five keys has 30 row sites and about 150 key
mentions.

**Verdict: friction.** The rows carry information, but a module boundary
inside one application forces them.

### 2. One Router Table, Mixed Handler Rows

axum stores handlers of different types in one `Router` by erasing them.
In hd, a table of handlers has one function type, and that type has one
row:

```text
pub type Handler = fn!(Request) -> Response $ Db + Cache + Clock + Log + Metrics + Config + HttpClient + Auth

pub data Route:
    path: string
    handler: Handler

pub fn routes() -> List[Route]:
    [
        Route { path: "/orders", handler: create_order },
        Route { path: "/orders/id", handler: get_order },  # error: type-mismatch
        Route { path: "/health", handler: health },        # error: type-mismatch
    ]
```

Rows are invariant, so `get_order`, whose row is
`$ Db + Cache + Log + Auth`, is not a `Handler`. The conformance fixture
`typing/invalid/row-inference-conflict.hd` shows the same rejection for two
callbacks sharing one row parameter.

There are three ways out today. The first is that every handler declares
the full eight-key row, which erases the information that case 1 had. The
second wraps each route in a closure that writes the full row again:

```text
pub fn routes() -> List[Route]:
    [
        Route { path: "/orders", handler: create_order },
        Route {
            path: "/health",
            handler: fn!(req: Request) -> Response $ Db + Cache + Clock + Log + Metrics + Config + HttpClient + Auth:
                health!(req)
        },
    ]
```

An unannotated closure `fn!(req): health!(req)` would not help.
[`req.row.omitted.closure`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.closure)
infers the least row, `$ Clock`, and only a row *parameter* in the expected
type unifies with it
([`req.row.omitted.expected-parameter`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.expected-parameter)).

The third way is the open-row idiom. Each handler declares a row parameter
beside its own keys. Passing it where a `Handler` is expected instantiates
it, and the least row solution makes `R` the remaining keys:

```text
pub fn get_order![R](req: Request) -> Response $ R + Db + Cache + Log + Auth:
    pass

pub fn health![R](req: Request) -> Response $ R + Clock:
    pass

pub fn routes() -> List[Route]:
    [
        Route { path: "/orders/id", handler: get_order },  # R is Clock + Metrics + Config + HttpClient
        Route { path: "/health", handler: health },        # R is the other seven keys
    ]
```

This uses only existing rules. A generic function value takes its type
arguments from the expected monomorphic type
([`fn.type.generic.instantiate-sources`](../spec/07-functions.md#r-fn.type.generic.instantiate-sources)).
Matching `$ R + K` against a row that lists `K` solves `R` as the rest
([`req.row.least.examples`](../spec/11-requirements-and-suspension.md#r-req.row.least.examples)).
The body is compiled once
([`req.poly.one-body`](../spec/11-requirements-and-suspension.md#r-req.poly.one-body)).

One point is unstated. A direct call such as `health!(req)` must solve `R`
as the empty row. The least-solution rule is written for parameter
patterns, not for the callee's own row.

**Verdict: friction.** Closed rows break. The open-row idiom works but
costs `[R]` and `R +` on every handler, and a closed library function still
needs the wrapper.

### 3. Add `Metrics` To A Shared Helper

A shared `respond` helper starts counting responses:

```text
pub fn respond(status: i32, body: string) -> Response $ Metrics:
    $.use(Metrics).count("http.responses")
    Response { status: status, body: body }
```

Every public handler that calls `respond` must add `Metrics`, whether its
row is closed or open. Private helpers between them update themselves,
because their rows are inferred. With the 30 handlers of case 1, churn is
up to 30 row sites.

Handlers that declare the full app row, the first way out in case 2, have
a churn of one alias. They have already lost least privilege.

Koka, ZIO and Effect-TS have the same churn for the same reason: the
requirement set is part of the type. An app that writes one named
environment on every handler edits one line ([Survey](#survey)).

**Verdict: friction.** Agents make the edits mechanically, and the diff
shows the new authority. The cost is diff size and review noise.

### 4. Composition Root And Layers

Guice installs modules into an injector, and ZIO builds an environment
from `ZLayer` values. In hd, `main!` may list only host capability traits
([`module.entry.row.host`](../spec/10-modules.md#r-module.entry.row.host)),
so it builds the application providers and installs them:

```text
use std.host.Env
use std.net.Network
use std.time.Clock

pub type AppContext = $.Context[$ Db + Cache + Log + Metrics + Config + HttpClient + Auth]

pub fn app_context!(config: EnvConfig) -> Result[AppContext, StartError] $ Network + Clock:
    log := JsonLog::new(config.log_level)
    let db: mut PgPool = $.with(Log=log):
        PgPool::connect!(config.database_url)?
    let cache: mut RedisCache = RedisCache::connect!(config.redis_url)?
    let metrics: mut StatsdMetrics = StatsdMetrics::new(config.statsd)
    .Ok($.context(
        Db=db,
        Cache=cache,
        Log=log,
        Metrics=metrics,
        Config=config,
        HttpClient=HostHttp::new(),
        Auth=JwtAuth::new(config.jwt_key),
    ))

pub fn main!() -> Result[void, StartError] $ Env + Network + Clock + Console:
    config := EnvConfig::from_env($.use(Env))?
    app := app_context!(config)?
    $.with(app...):
        serve!(config.port, routes())
    .Ok()
```

A provider that needs other providers is built inside a `$.with` for them,
as `PgPool::connect!` is built with `Log`. That is a ZIO layer with inputs,
written as a function with a row. The order is written by hand; nothing
solves a dependency graph as `ZLayer.make` does. The `AppContext` alias
names the bundle once.

**Verdict: works.** The layer order is explicit, which matches the
[explicit provider selection](../spec/11-requirements-and-suspension.md#r-req.runtime.explicit-provider)
rule.

### 5. Request-Scoped Bundle

Dagger gives each request a subcomponent, and tower wraps each request in
layers. In hd the server builds a small context per request and spreads it
over the application context:

```text
pub fn request_context!(req: Request) -> Result[$.Context[$ Log + Principal], AuthError] $ Log + Auth:
    principal := $.use(Auth).verify!(req)?
    log := $.use(Log).with_field("request_id", req.id)
    .Ok($.context(Log=log, Principal=principal))

fn dispatch!(route: Route, req: Request) -> Response:
    match request_context!(req):
        .Ok(scope) => $.with(scope...): route.handler!(req)
        .Err(error) => unauthorized(error)
```

The nested scope replaces `Log` for this request only
([`req.with.nested`](../spec/11-requirements-and-suspension.md#r-req.with.nested)).
`Principal` also fits the direction of
[Access Control And Tenancy](OPEN_ISSUES.md#access-control-and-tenancy-expressibility).

**Verdict: works.** The `Handler` type must now include `Principal`, which
adds one key to the wide row of case 2.

### 6. Test Fixture Swapping Six Providers

Spring swaps beans with `@TestConfiguration`, and Guice uses
`Modules.override(prod).with(test)`. In hd a fixture is a record of fakes
that keeps `mut` aliases, so the test can inspect them afterwards:

```text
data Fakes:
    db: mut MemoryDb
    cache: mut MemoryCache
    clock: mut ManualClock
    log: mut BufferLog
    metrics: mut RecordingMetrics
    http: mut ScriptedHttp

type TestContext = $.Context[$ Db + Cache + Clock + Log + Metrics + Config + HttpClient + Auth]

impl Fakes:
    fn new() -> mut Fakes:
        Fakes {
            db: MemoryDb::new(),
            cache: MemoryCache::new(),
            clock: ManualClock::starting_at(Timestamp::from_unix_seconds(0)),
            log: BufferLog::new(),
            metrics: RecordingMetrics::new(),
            http: ScriptedHttp::new(),
        }

    fn context(mut self) -> TestContext:
        $.context(
            Db=self.db,
            Cache=self.cache,
            Clock=self.clock,
            Log=self.log,
            Metrics=self.metrics,
            HttpClient=self.http,
            Config=FixedConfig::default(),
            Auth=AllowAll {},
        )

tests:
    it("a denied order is not counted"):
        let fakes: mut Fakes = Fakes::new()
        $.with(fakes.context()..., Auth=DenyAll {}):
            _ := create_order!(sample_request())
        assert_equal(fakes.metrics.count("orders.created"), 0, reason="denied")
```

Swapping works: a later entry wins
([`req.context.order`](../spec/11-requirements-and-suspension.md#r-req.context.order)).

The friction is the row. `AppContext` in case 4 has seven keys, because the
host supplies `Clock` in production. A unit test gets no host providers
([`module.testing.unit-row`](../spec/10-modules.md#r-module.testing.unit-row)),
so `TestContext` repeats those seven keys and adds `Clock`. No form writes
"the app row plus `Clock`".

**Verdict: friction.** Two near-identical rows must be kept in step by
hand.

### 7. Generic Retry And Middleware

tower's `Layer` wraps any service, and ZIO aspects wrap any effect. hd row
parameters do both. A retry keeps the caller's row and adds `Clock`; an
authentication middleware removes `Principal` by extension:

```text
pub fn retry![T, E, R](times: i32, body: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock:
    pass

pub fn authenticated[R](next: fn!(Request) -> Response $ R + Principal) -> (fn!(Request) -> Response $ R + Auth):
    fn!(req: Request) -> Response $ R + Auth:
        match $.use(Auth).verify!(req):
            .Ok(principal) => $.with(Principal=principal): next!(req)
            .Err(_) => Response { status: 401, body: "" }
```

**Verdict: works.** Row parameters are the reuse tool for "whatever else
the caller needs", as in Koka and Unison.

### 8. Generic Router And Job Queue

A router library should not know the app's keys, so it takes a row
parameter. A job queue stores work to run later with the providers it was
queued under:

```text
pub data Route[R]:
    path: string
    handler: fn!(Request) -> Response $ R

pub fn serve![R](port: i32, routes: List[Route[R]]) -> void $ R + Network:
    pass

pub data Job[R]:
    providers: $.Context[R]
    run: fn!() -> void $ R
```

With open-row handlers, `List[Route[AppRow]]` holds handlers of different
rows, as in case 2. Two points are unspecified:

| Point | Status |
| --- | --- |
| A data type or trait taking a row parameter, as `Route[R]` does. | `req.row.param.inferred` (now [retired](../spec/STYLE.md#retired-rule-ids)) reads as general, but [Runtime Type Identity](INSPECTABLE.md) says "if data types ever take row parameters". |
| `$.Context[R]` indexed by a row parameter, and whether spreading it provides `R`. | The grammar accepts it; no rule says what it means. |

**Verdict: breaks.** Both are gaps in the spec, not design failures, but a
generic library cannot be written against the spec until they close.

### 9. One Bundle Trait As The Only Key

A common workaround names the environment as one trait with getters, like
the Scala cake pattern or a Rust "app" trait:

```text
trait AppEnv:
    fn db(self) -> mut Db
    fn cache(self) -> mut Cache
    fn log(self) -> Log

pub fn get_order!(req: Request) -> Response $ AppEnv:
    env := $.use(AppEnv)
    $.with(Db=env.db(), Cache=env.cache(), Log=env.log()):
        load_order!(req.order_id)
```

The row is short, but `load_order!` comes from a library with row
`$ Db + Cache + Log`. Every call into it rebinds each key by hand. The row
`$ AppEnv` says nothing about what the handler reaches.

**Verdict: friction.** It moves the list from the header into the body.

### 10. Service Record, No Rows

Go and Java pass dependencies as fields. hd allows it, because provider
values are ordinary values
([`req.use.value.flow`](../spec/11-requirements-and-suspension.md#r-req.use.value.flow)):

```text
pub data Services:
    db: mut Db
    cache: mut Cache
    log: Log

impl Services:
    pub fn get_order!(mut self, req: Request) -> Response:
        order := self.db.find!(req.order_id)
        pass
```

**Verdict: works**, but rows stop describing reach
([`req.use.value.row-meaning`](../spec/11-requirements-and-suspension.md#r-req.use.value.row-meaning)).
Tests build `Services` with fakes, as in Go. This is the escape hatch any
option must stay better than.

### 11. Provider-Installing Functions

The owner's framing for question 2: a function that installs providers is
the reusable bundle. It follows item 4 of
[Bound And Row Operators](OPEN_ISSUES.md#bound-and-row-operators). The
installer extends its callback's row, and a callback that lacks some of
the installed keys still matches, with `R` set to its own row. The row
aliases are those of [Option F](#option-f-rows-as-type-expressions):

```text
type Stack = Db + Cache + Log + Metrics  # hypothetical syntax

pub fn with_stack![T, R](config: EnvConfig, f: fn!() -> T $ R + Stack) -> Result[T, StartError] $ R + Network:
    log := JsonLog::new(config.log_level)
    let db: mut PgPool = $.with(Log=log):
        PgPool::connect!(config.database_url)?
    let cache: mut RedisCache = RedisCache::connect!(config.redis_url)?
    let metrics: mut StatsdMetrics = StatsdMetrics::new(config.statsd)
    $.with(Db=db, Cache=cache, Log=log, Metrics=metrics):
        .Ok(f!())

pub fn main!() -> Result[void, StartError] $ Env + Network + Clock + Console:
    config := EnvConfig::from_env($.use(Env))?
    return with_stack!(config):
        serve!(config.port, routes())
```

The trailing block is the callback
([`fn.trailing.form`](../spec/07-functions.md#r-fn.trailing.form)), so an
installer reads like a scope. Without the relaxed rule of item 4, a
callback that used only `Db` would be a `type-mismatch`
(`req.poly.absent`, now [retired](../spec/STYLE.md#retired-rule-ids)).
The owner's three hard spots:

**Construction that needs configuration.** The installer takes `config`
as an argument and lists `Network` in its own row, as above. Every call
constructs the providers again. That suits `main!`, but not a
per-request scope, where the database pool must be built once.

Build-once,
install-many still needs a value: a context, or a record of providers. An
installer over a prebuilt context is `$.with(app...)` with a signature
around it, so it adds nothing.

**Request-scoped values.** A per-request installer works and nests:

```text
pub fn with_request![T, R](req: Request, f: fn!() -> T $ R + Principal + Log) -> Result[T, AuthError] $ R + Auth + Log:
    principal := $.use(Auth).verify!(req)?
    $.with(Principal=principal, Log=$.use(Log).with_field("request_id", req.id)):
        .Ok(f!())

fn dispatch!(route: Route, req: Request) -> Response:
    result := with_request!(req):
        route.handler!(req)
    match result:
        .Ok(response) => response
        .Err(error) => unauthorized(error)
```

`Log` sits on both sides: the installer reads the outer `Log` and installs
a request `Log` for the callback. That is extension, not subtraction.

**A fixture overriding one provider out of six.** The inner scope wins
([`req.with.nested`](../spec/11-requirements-and-suspension.md#r-req.with.nested)),
so an override nests inside the installer:

```text
fn with_fakes![T, R](fakes: mut Fakes, f: fn!() -> T $ R + AppRow + Clock) -> T $ R:
    $.with(fakes.context()...):
        f!()

tests:
    it("a denied order is not counted"):
        let fakes: mut Fakes = Fakes::new()
        with_fakes!(fakes):
            $.with(Auth=DenyAll {}):
                _ := create_order!(sample_request())
        assert_equal(fakes.metrics.count("orders.created"), 0, reason="denied")
```

Two limits remain. A test that inspects a fake afterwards must create it
and pass it in, because a trailing block takes no parameters. An override
reaches only lookups: a provider built from another at construction time,
such as `PgPool` built with the old `Log`, keeps what it captured. Context
values have the same limit.

**One more cost.** A suspending installer takes an `fn!` callback, and a
non-suspending caller cannot bang-call it. Code that installs the same
bundle around both kinds of body needs two installers, one per kind.

| Spot | Installer | Context value | Together |
| --- | --- | --- | --- |
| Construction with config | rebuilds per call | built once | build a context once; install it per scope |
| Request scope | nests cleanly | spread per request | either |
| Override one of six | inner `$.with` | later entry wins | either |
| Inspect fakes afterwards | pass them in | keep `mut` aliases | either |
| `fn` and `fn!` bodies | two installers | one value | context |

**Verdict: works, with friction.** Installers plus row aliases cover
scoping, and context values cover build-once reuse. Neither alone covers
question 2; together they do, with no new feature beyond item 4.

### 12. Two Generic Rows

The owner asked how two generic requirements combine when rows have only
`+`. Three shapes cover it.

**(a) A union of independent row parameters.** Each parameter is fixed by
its own argument, so the union needs no solving:

```text
pub fn both![A, B, R1, R2](left: fn!() -> A $ R1, right: fn!() -> B $ R2) -> (A, B) $ R1 + R2:
    (left!(), right!())
```

This is the typed form of `std.task.all!` over two different rows. It also
repairs the rejected `combine(clock, logger)` of
`typing/invalid/row-inference-conflict.hd`: with two row parameters the
call's row is `Clock + Logger`. When `R1` and `R2` share a key, both
bundles come from the caller's one scope, so the key has one provider.

**(b) A bundle fixed by another argument.** Item 5 of the operator
decision gives `provide[T, K, R](provider: K, f: fn() -> T $ R + K) -> T $ R`
as its example. That signature cannot be written today. `K` is used as a
value type and as a key, and one parameter cannot be both
([`req.row.param.one-kind`](../spec/11-requirements-and-suspension.md#r-req.row.param.one-kind)).
hd has no parameter that ranges over traits. A context does the same job,
for one key or a bundle:

```text
pub fn provide[T, S, R](providers: $.Context[S], f: fn() -> T $ R + S) -> T $ R:
    $.with(providers...):
        f()

fn report() -> string $ Db + Clock + Log:
    "ok"

fn run(ctx: $.Context[$ Db + Log]) -> string $ Clock:
    provide(ctx, report)
```

`S` is fixed by the type of `providers`, so `R` is the only unknown in
`R + S`. The least solution for the callback row `Db + Clock + Log` is
`R = Clock`. Inference has subtracted `S` without a `-` operator.

With item 4, a callback that lacks some keys of `S` also matches. This
needs question 9 (`$.Context[R]`).

**(c) Two unknowns in one pattern.** Nothing fixes the split:

```text
fn split[R1, R2](f: fn() -> void $ R1 + R2) -> void $ R1:  # proposed: error, two unknown row variables
    pass
```

For the callback row `Db + Clock`, `R1 = Db, R2 = Clock` and `R1 = $()`,
`R2 = Db + Clock` both solve the pattern, and they give `split` different
rows. No least solution exists, because the pattern has two free
variables.

**Proposed rule.** A row pattern has at most one unknown row variable.
Every other row variable in it must be fixed first, by another argument,
the expected type, or an explicit type argument. Patterns are then solved
one at a time, and each has one least solution. A declaration whose
pattern can never meet the rule, as `split` above, is rejected at the
declaration.

| System | Row variables in one row | Removing an effect | Source |
| --- | --- | --- | --- |
| Koka | A row is labels plus at most one tail variable, `<exn\|e>`. | Handlers extend: `(action: () -> <exn\|e> a) -> e a`. | [Leijen 2014](https://arxiv.org/abs/1406.2061) |
| Links | Rémy-style rows, closed or ending in one row variable. | Presence types mark a label absent, as `Move-`. | [Hillerström and Lindley 2016](https://homepages.inf.ed.ac.uk/slindley/papers/links-effect.pdf) |
| Effekt | No effect variables in user signatures; effects are capabilities in scope. | Handlers bind capabilities; functions are second-class. | [Brachthäuser et al. 2020](https://se.informatik.uni-tuebingen.de/publications/brachthaeuser20effects/) |
| Flix | Set expressions over several variables. | An explicit `ef1 - ef2`. | [effect polymorphism](https://doc.flix.dev/effect-polymorphism.html) |
| Effect-TS | Unions of service tags. | `Effect.provide` computes `Exclude<R, ROut>` at the type level. | [`Effect.ts` source](https://github.com/Effect-TS/effect/blob/main/packages/effect/src/Effect.ts) |
| ZIO | Intersections of services. | `provideSome[R0]` makes the caller write the remainder `R0`. | [automatic layer construction](https://zio.dev/reference/di/automatic-layer-construction) |

Koka and Links build the one-unknown rule into the syntax: a row has one
tail. The proposed rule keeps hd's flat `+` spelling and states the same
limit as a solving rule.

**Subtraction.** The owner decided on 2026-09-28 that rows have no `-`
(item 5 of [Bound And Row Operators](OPEN_ISSUES.md#bound-and-row-operators)).
The cases agree. Every case in this record removes keys by extension
(cases 7, 11, 12b). Effect-TS's `Exclude` and ZIO's `provideSome` are the
same removal, done when the provided set is a known type.

The one shape extension cannot state is a deny list: "run this with
anything except `Network`". A closed allow list, such as
`f: fn() -> T $ Clock + Log`, states that sandbox more safely, because a
new capability is denied by default. No case here needs an explicit `-`.

**The rule against every pattern in this record.**

| Case | Pattern | Unknown row variables when solved | Result |
| --- | --- | --- | --- |
| 2, open rows | `$ R + Clock` against the `Handler` row | `R` | the other seven keys |
| 7, retry | `$ R` in `body` | `R` | the callback's row |
| 7, middleware | `$ R + Principal` in `next` | `R` | the callback's row less `Principal` |
| 8, router | `$ R` in `Route[R]` | `R` | fixed by the expected `Route` type |
| 11, installers | `$ R + Stack`, `$ R + Principal + Log`, `$ R + AppRow + Clock` | `R` | the callback's row less the installed keys |
| 12a, `both!` | `$ R1` and `$ R2`, one per argument | one each | independent |
| 12b, `provide` | `$ R + S` | `R`, after `S` is fixed by `providers` | `Clock` |
| 12c, `split` | `$ R1 + R2` | `R1` and `R2` | rejected |
| Q9, `run_job!` | `$ R` in `job` | `R`, also fixed by `providers` | must agree |

Every accepted pattern has one unknown when it is solved, and the only
rejected one is `split`.

**Soundness point.** When fixed row variables overlap, as `R` and `S` can
under explicit type arguments, a key would reach `f` from two bundles. The
rule needs one sentence: the nearer scope provides it, as a nested
`$.with` does today.

**Verdict: works** for (a) and (b) under the proposed rule; (c) is
rejected by it.

## Survey

| Language | Declaration side | Installation side |
| --- | --- | --- |
| Koka | Effect rows with aliases: `alias pure = <div,exn>` is used inside rows such as `<pure,ndet>` ([book, effect types](https://koka-lang.github.io/koka/doc/book.html#sec-effect-types)). Functions are written open, as `<console\|e>` ([book, polymorphic effects](https://koka-lang.github.io/koka/doc/book.html#sec-polymorphic-effects)). | Handlers are values. `with f(e1,...,eN)` desugars to `f(e1,...,eN, fn(){ body })`, so a bundle of handlers is an ordinary function ([book, with](https://koka-lang.github.io/koka/doc/book.html#sec-with)). |
| Unison | Ability lists such as `{Abort, Exception, g}`, with ability variables ([abilities, part 2](https://www.unison-lang.org/docs/fundamentals/abilities/using-abilities-pt2/)). The docs show no alias for an ability list. | Handlers nest, each removing one ability ([abilities and handlers](https://www.unison-lang.org/docs/language-reference/abilities-and-ability-handlers/)). |
| Scala 3 and ZIO | `ZIO[R, E, A]`, with `R` an intersection such as `Db & Cache`, named by an ordinary `type` alias ([intersection types](https://docs.scala-lang.org/scala3/reference/new-types/intersection-types.html)). Context functions alias too: `type Executable[T] = ExecutionContext ?=> T` ([context functions](https://docs.scala-lang.org/scala3/reference/contextual/context-functions.html)). | `ZLayer` values; `ZLayer.make[R](...)` and `provide(...)` build the graph at compile time and report missing layers ([automatic layer construction](https://zio.dev/reference/di/automatic-layer-construction)). |
| Effect-TS | `Effect<A, E, R>`, with `R` a union of service tags, nameable with a TypeScript `type`. | `Layer<ROut, E, RIn>`, `Layer.merge`, `Layer.provide`, and `Effect.provide(program, MainLive)` ([layers](https://effect.website/docs/requirements-management/layers/)). |
| Haskell mtl | Constraint synonyms with `ConstraintKinds`, such as `type Stringy a = (Read a, Show a)` ([GHC guide](https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/constraint_kind.html)). | Run functions composed by hand. |
| effectful, polysemy | `(Db :> es, Cache :> es)`. effectful deprecated its list operator `:>>` because it "slows down GHC too much" ([Effectful](https://hackage.haskell.org/package/effectful-core/docs/Effectful.html)). polysemy offers `Members '[...] r` ([Polysemy](https://hackage.haskell.org/package/polysemy/docs/Polysemy.html)). | Interpreters composed as functions. |
| Kotlin | Context parameters list each dependency: `context(db: Db, cache: Cache)`. The proposal has no grouping form, and its DI section advises injecting "everything you need in one go" ([KEEP-367](https://github.com/Kotlin/KEEP/blob/master/proposals/context-parameters.md)). | `context(a, b) { ... }` calls. |
| Rust | Bounds `T: Db + Cache`. Trait aliases (`trait App = Db + Cache;`) remain unstable ([RFC 1733](https://rust-lang.github.io/rfcs/1733-trait-alias.html), [tracking issue](https://github.com/rust-lang/rust/issues/41517)); the workaround is a supertrait plus a blanket impl. | A struct of services or generic state, as in axum's `State` ([axum](https://docs.rs/axum/latest/axum/)). |
| Go | Interface embedding: `interface { Reader; Writer }` ([spec](https://go.dev/ref/spec#Interface_types)). | Wire provider sets: `wire.NewSet(...)` may include other sets, and `wire.Build` generates the injector ([Wire guide](https://github.com/google/wire/blob/main/docs/guide.md)). |
| Java and Kotlin DI | Constructor parameters; nothing per method. | Guice modules `install` others, and `Modules.override(prod).with(test)` swaps bindings ([Guice](https://github.com/google/guice/wiki/GettingStarted), [Modules](https://google.github.io/guice/api-docs/latest/javadoc/com/google/inject/util/Modules.html)). Dagger components list modules and use subcomponents for narrower scopes ([Dagger subcomponents](https://dagger.dev/dev-guide/subcomponents)). Spring composes `@Configuration` classes with `@Import` ([Spring](https://docs.spring.io/spring-framework/reference/core/beans/java/composing-configuration-classes.html)). |
| OCaml 5 | Effects are untyped: "the compiler does not statically ensure that all the effects performed by the program are handled" ([manual](https://ocaml.org/manual/5.3/effects.html)). Nothing to name. | Handlers nested by ordinary functions. |
| Flix | Effect sets `\ {IO, Net}` with set operators `+`, `-`, `&` and `~` ([effect polymorphism](https://doc.flix.dev/effect-polymorphism.html)). The type-alias chapter shows only value types ([type aliases](https://doc.flix.dev/type-aliases.html)). | Handlers installed per block. |

**Takeaways.**

1. **Installation-side bundles are ordinary values everywhere.** Koka
   handler functions, ZIO and Effect layers, Wire sets, and Guice modules
   are all first-class values or functions. hd's `$.Context` values
   returned by functions are the same model, so question 2 needs no new
   feature.
2. **Where the requirement set is a type, users name it with the ordinary
   alias.** Koka, Scala, Effect-TS and Haskell all do so, and none added a
   separate "row alias" construct. In hd a row is not a type-level
   expression, so the ordinary alias cannot reach a `$` clause. That is
   the one real gap.
3. **Narrow functions fit wide types through polymorphism.** Koka writes
   functions with an open row variable; hd can write the same `$ R + K`.
   hd's `Fn` is invariant in its row by design, so a closed row still needs
   a wrapper.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | RR-1 | No name can stand for several keys in a `$` clause. | Medium: repetition and churn wherever a row stays wide. | 2, 3, 5, 6 |
| 2 | RR-2 | Row parameters on data types and traits, `$.Context[R]`, and the own-row solution at a direct call are unspecified. | Medium: generic libraries and the open-row idiom rest on unwritten rules. | 2, 8 |
| 3 | RR-3 | Invariance rejects closed-row handlers in a shared table; the open-row idiom and wrappers cost noise. | Medium: the natural code is rejected. | 2 |
| 4 | RR-4 | Every cross-module function in an application is `pub`, so it writes its row. | Low: the rows carry information. | 1, 3 |
| 5 | RR-5 | The workarounds that avoid rows hide what rows show. | Low: a style risk. | 9, 10 |
| 6 | RR-6 | A pattern with several row variables has no solving rule. | Medium: `split`-shaped signatures are ambiguous, and the spec does not reject them. | 12 |

### RR-1: No Name For Several Keys

**Effect.** The eight-key app row is written in full four ways (cases 2,
5 and 6). It appears in the `Handler` type, `AppContext`, `TestContext`,
and every wrapper around a closed function. The prod and test rows differ by one
host key and cannot share a definition. An app that prefers one
environment on every handler, as ZIO apps do, has no way to name it.

**Candidates.**

1. Should rows be type-level expressions, so that `type AppRow = Db + Cache`
   is an ordinary alias ([Option F](#option-f-rows-as-type-expressions))?
2. Should a key that names a `$.Context[Row]` type stand for its row
   ([Option C](#option-c-named-rows))?
3. Accept: repetition is visible authority.

### RR-2: Unwritten Rules Under Generic Rows

**Effect.** `Route[R]`, a `Handler[R]` trait, and a `$.Context[R]` field
are natural in a generic library (case 8). The open-row idiom of case 2
also needs a direct call to solve an own-row parameter as the empty row.
The spec states none of these, so an implementation must guess.

**Candidates.**

1. Specify that data types, enums and traits may take row parameters.
2. Specify that `$.Context[R]` is valid and that spreading it provides `R`.
3. Specify that the least row solution also applies to a callee's own row,
   so an unconstrained `R` is the empty row.

### RR-3: Closed Rows In A Shared Table

**Effect.** A router, a job queue, or a plugin list holds function values
under one type (case 2). Closed-row handlers are rejected. The open-row
idiom works but adds `[R]` and `R +` to each handler. A closed function from
a library, such as a `$ Clock` helper, still needs a wrapper that writes
the whole wide row.

**Candidates.**

1. Teach the open-row idiom; no rule change.
2. Should an unannotated closure take a concrete expected row that entails
   its body's requirements
   ([Option B](#option-b-adapting-closures))?
3. With named rows, the wrapper's row is one name.

### RR-4: Public Rows Inside One Application

**Effect.** Module boundaries inside one app are not API boundaries, but
`pub` forces an explicit row on each crossing (cases 1 and 3).

**Candidates.**

1. Keep explicit public rows; the row is the reviewer's summary.
2. Infer the rows of `pub` functions in some packages
   ([Option E](#option-e-wider-row-inference)).

### RR-5: Workarounds Hide Reach

**Effect.** A bundle trait (case 9) or a service record (case 10) shortens
rows by hiding them. Both are legal. If wide rows stay long, apps will
drift to case 10, and rows will describe little.

**Candidates.**

1. Document case 10 as legal but not the recommended style.
2. No action.

### RR-6: Several Row Variables In One Pattern

**Effect.** `R1 + R2` in a function's own row is harmless, and a pattern
with one unknown solves by least row (case 12a and 12b). A pattern with two
unknowns, as in `split`, has several solutions that give different rows
(case 12c). The spec states the least-solution rule for one row parameter
beside concrete keys only.

**Candidates.**

1. Adopt the one-unknown rule: every other row variable in a pattern is
   fixed by another argument, the expected type, or an explicit type
   argument.
2. Restrict rows to one row variable each, as Koka and Links do.

Row subtraction is not a candidate: the owner ruled it out (item 5).

## Options

Every option is shown on the same use: the router's handler type, one
narrow handler, and the test context of case 6. Each lists the kinds of
change it needs, in the [Design Cost Order](../AGENTS.md#design-cost-order).
Option F is the owner's proposal of 2026-09-28, stress-tested in full.

### Option A: Nothing New

Functions, context values and row parameters already suffice. The
recommended style is a convention, taught in the guide and `std`:

- one `pub type AppContext = $.Context[$ A + B]` per app, next to the
  function that builds it;
- layers as functions that return contexts, with their own rows (case 4);
- request bundles spread over the app context (case 5);
- fixture records with a `context` method, overridden by later entries
  (case 6);
- row parameters for libraries (case 7);
- open rows, `[R]` with `$ R + K`, for handlers stored in a table
  (case 2).

```text
pub type Handler = fn!(Request) -> Response $ Db + Cache + Clock + Log + Metrics + Config + HttpClient + Auth

pub fn health![R](req: Request) -> Response $ R + Clock:
    pass

type TestContext = $.Context[$ Db + Cache + Clock + Log + Metrics + Config + HttpClient + Auth]
```

**Changes.** None to the language; documentation and `std` examples. The
three unwritten rules of RR-2 should be specified under any option.

**Gives up.** RR-1 stays: the wide row is spelled in each alias and
wrapper.

### Option B: Adapting Closures

An unannotated closure whose expected type has a concrete row takes that
row, provided the row entails every requirement of its body. This extends
[`req.row.omitted.expected-parameter`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.expected-parameter)
from row parameters to concrete rows. A closed function then adapts in
place, without the open-row idiom:

```text
pub fn health!(req: Request) -> Response $ Clock:
    pass

pub fn routes() -> List[Route]:
    [
        Route { path: "/orders", handler: create_order },
        Route { path: "/health", handler: fn!(req): health!(req) },  # proposed: takes Handler's row
    ]
```

**Rules.** One rule changes: closure row inference uses a concrete expected
row when that row entails the inferred least row. A closure whose body
needs a key the expected row lacks is still `missing-requirement`.

**Soundness.** Equivalent to writing the expected row on the closure, which
[`req.row.omitted.written`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.written)
already allows. The closure receives the wider bundle and ignores the
extra keys, so representation is unchanged.

**Gives up.** `health` itself is still not a `Handler`: implicit widening
of a named function would be a representation-changing conversion. The
open-row idiom already covers handlers the app writes itself.

**Changes.** One semantic rule change (a refinement of closure inference).

### Option C: Named Rows

A name stands for several keys wherever a key may appear. Expansion happens
before entailment, which
[`req.row.entail.sets`](../spec/11-requirements-and-suspension.md#r-req.row.entail.sets)
already describes as "set entailment after alias expansion". This option
reaches that without new grammar.

**C1: a context type names its row.** A key that resolves, directly or
through a transparent alias, to `$.Context[Row]` stands for `Row`. The
alias that already names the bundle now also names the row:

```text
pub type AppEnv = $.Context[$ Db + Cache + Log + Metrics + Config + HttpClient + Auth]

pub type Handler = fn!(Request) -> Response $ AppEnv + Clock + Principal

pub fn create_order!(req: Request) -> Response $ AppEnv + Clock:
    pass

type TestContext = $.Context[$ AppEnv + Clock]

type WithLog[R] = $.Context[$ R + Log]
```

`$ AppEnv + Clock` flattens to eight keys, so the prod and test rows share
one definition. A generic alias such as `WithLog[R]` names an extended row.

| Rule | Content |
| --- | --- |
| Expansion | A key naming a context type is replaced by that context's keys before normalization. |
| Not a provider | `$.use(AppEnv)` and a binding `AppEnv=value` are errors; a context is installed only by spreading it. |
| Visibility | A `pub` alias used in a `pub` row must name only public traits, as [`module.vis.signature.coverage`](../spec/10-modules.md#r-module.vis.signature.coverage) already requires of alias targets. |
| Collisions | [Generic Key Collisions](../spec/11-requirements-and-suspension.md#generic-key-collisions) apply after expansion. |

**Changes.** One semantic rule exception (a key is a trait, except a
context type) and one new error for using it as a provider. No grammar
change: every line above parses once the decided `+` spelling is applied.

**C2** was a row alias declaration, `type AppRow = $(...)`. After the
decided `+` spelling it is the alias part of
[Option F](#option-f-rows-as-type-expressions), so it is folded in there.

**Gives up** the local reading of a row. A reviewer must look up `AppEnv`
to see its keys, as with Koka's `io`. C1 also reads a bundle type as a
row, which is a pun. A change to a `pub` alias changes every signature that
names it; in a library that is a breaking change for callers.

### Option D: Bundle Traits

A trait with supertraits and no members bundles its supertrait keys, like
Go interface embedding or a Rust trait-alias workaround:

```text
trait AppEnv < Db & Cache & Log

pub fn get_order!(req: Request) -> Response $ AppEnv:
    db := $.use(Db)  # proposed: satisfied through AppEnv
    pass
```

Installing `AppEnv=value` would install `value` for each supertrait key.

**Rules.** Entailment through supertraits, installation fan-out, and a
rule for a bundle beside one of its own supertraits.

**Evidence against.** It changes what a supertrait key means today. The
fixture `runtime/valid/context-values-install-providers.hd` installs
`Clock` and `Backup < Clock` as two separate keys. One production value
must implement all eight traits, so tests need delegation
(`impl Db for TestEnv by db`) for every key. Rust has left trait aliases
unstable since 2017, and its workaround is exactly this shape.

**Changes.** Two or three semantic rule exceptions.

### Option E: Wider Row Inference

A `pub` function in a package with no library target gets an inferred
row, as a private function does today. The compiler and tooling print the
inferred rows:

```text
pub fn create_order!(req: Request) -> Response:  # proposed: row inferred in an application package
    pass
```

**Rules.** An exception to
[`req.row.omitted.empty`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.empty),
keyed on a package property.

**Evidence against.** A body edit deep in a helper silently changes a
handler's row, which is action at a distance for a human reader. The
handler table of case 2 still needs one exact row. It reverses a written
public-API rule rather than adding a tool.

**Changes.** One semantic rule exception.

### Option F: Rows As Type Expressions

**The idea.** Bounds now use `&`, so `+` in a type position can only build
a row. A row becomes a type-level expression. It is usable as a type
argument without `$`, as in `Fn[(A, B), O, R1 + R2]`. A row alias is then
an ordinary transparent alias:

```text
type AppRow = Db + Cache + Log + Metrics + Config + HttpClient + Auth  # hypothetical syntax

pub fn get_order!(req: Request) -> Response $ Db + Cache + Log + Auth:
    pass

pub fn create_order!(req: Request) -> Response $ AppRow + Clock:
    pass

pub type Handler = fn!(Request) -> Response $ AppRow + Clock + Principal

pub type Spelled = SuspendFn[(Request,), Response, AppRow + Clock + Principal]  # hypothetical syntax

pub type AppContext = $.Context[AppRow]

type TestContext = $.Context[AppRow + Clock]  # hypothetical syntax
```

`Handler` and `Spelled` are the same type. The prod and test contexts
share `AppRow`, and the test adds the host key `Clock`.

**Grammar.** A sketch, beside the decided `$ A + B` clause:

```ebnf
row_expression = row_operand, { "+", row_operand } ;
row_operand = type ;
type_decl = "type", identifier, [ type_params ],
            ( "=", ( type | row_expression ) | "(", type, ")" ), NEWLINE ;
type_argument = type, [ "..." ] | row_expression | "$", "(", ")" ;
requirement_clause = "$", ( row_expression | "(", ")" ) ;
```

A lone name, such as `Fn[(A,), O, Db]`, parses as a `type`; the parameter
it fills gives its kind. That is already how
`SuspendFn[(UserId,), User, Database]` works
([Function Type Constructors](../spec/07-functions.md#function-type-constructors)).

#### F On The Production Cases

| Case | Under F | Verdict |
| --- | --- | --- |
| 1, handler rows | Each handler chooses its least row or `$ AppRow + Clock`. | Works |
| 2, mixed table | `Handler` is one line. Least-row handlers still need the open-row idiom or B. | Friction, as today |
| 3, add `Metrics` | One alias edit when handlers name `AppRow`; otherwise as today. | Works |
| 4, composition root | `$.Context[AppRow]` names the bundle from the row. | Works |
| 5, request bundle | `$ AppRow + Clock + Principal` in `Handler`. | Works |
| 6, fixtures | `$.Context[AppRow + Clock]`: one definition. | Works |
| 7, retry and middleware | Unchanged; `type WithPrincipal[R] = R + Principal` may name the extension. | Works |
| 8, generic router | `Route[AppRow + Clock]` is a plain type argument; RR-2 still applies. | Breaks until RR-2 |
| 9, 10, workarounds | No longer needed to shorten rows. | Works |

F answers question 1. It does not answer question 2: `$.with` and `$.use`
still name keys one at a time, and a bundle is still a context value. F
only lets that value's type name the row, as `$.Context[AppRow]`.

#### Edge Cases

| # | Point | Proposed answer | New rule? |
| --- | --- | --- | --- |
| 1 | `type Store = Db`: a row or the trait-value type `Db`? | Both, chosen by the use site. The alias names the trait, which is already a value type, a bound, and a key. `x: Store` is the trait value; `$ Store` is the key. | No |
| 2 | `type Store = mut Db` used as a key. | Error at the use: a key has no `mut` ([`req.mut.no-spelling`](../spec/11-requirements-and-suspension.md#r-req.mut.no-spelling)). Access comes from the trait. | No |
| 3 | The empty row. | `()` is the unit tuple type, so the empty row keeps its own literal, `$()`. `type NoRow = $()` is a row alias. | Keeps one form |
| 4 | `type WithLog[R] = R + Log`. | `R` is row-kinded by its use beside `+`, as `req.row.param.inferred` (now [retired](../spec/STYLE.md#retired-rule-ids)) infers it after `$`. Extension through the alias works, because expansion comes before least-row solving. | Extends inference |
| 5 | `type Only[T] = T`, then `$ Only[AppRow]`. | `T` is type-kinded, so a row argument is `generic-kind-mismatch`. `$ Only[Db]` is the key `Db`. | No |
| 6 | Nesting: `type All = AppRow + Metrics`. | Flattens; `Metrics` is already in `AppRow`, so `All` equals `AppRow` ([`req.row.set.duplicate`](../spec/11-requirements-and-suspension.md#r-req.row.set.duplicate)). No diagnostic, as today. | No |
| 7 | Overlap: `AppRow + WebRow` sharing keys. | Set union; no diagnostic. | No |
| 8 | Cycles: `type A = B + Db` and `type B = A + Cache`. | An alias cycle error. The spec states no alias-cycle rule for ordinary aliases either, so both need one. | Yes, shared |
| 9 | Generic keys: `Repo[T] + Repo[U]` through aliases. | [Generic Key Collisions](../spec/11-requirements-and-suspension.md#generic-key-collisions) run after expansion. | No |
| 10 | A multi-key alias as a value type or bound: `x: AppRow`, `T < AppRow`. | `generic-kind-mismatch`: a row is not a type of values. | Reuses a code |
| 11 | `type Both = Db & Cache`. | Error: `&` exists only in bounds. `$ Db + Cache` is two providers; `T < Db & Cache` is one value with both traits. | No |
| 12 | `$.use(AppRow)` and `$.with(AppRow=value)`. | Errors: a row is a set with no order to destructure, and it is not one provider. Bundles install by spreading a context. | One error |
| 13 | Diagnostics. | Print rows as written, alias names kept, and add the expansion on a mismatch or a missing key. | Tooling |
| 14 | Visibility. | A `pub` row alias may name only public traits, as [`module.vis.signature.coverage`](../spec/10-modules.md#r-module.vis.signature.coverage) already requires. | No |

A diagnostic under point 13 might read:

```console
error[missing-requirement]: `respond` requires `Metrics`
  --> handlers/orders.hd:12:5
   | pub fn get_order!(req: Request) -> Response $ WebRow + Auth
   |                                               ------ WebRow = Db + Cache + Clock + Log
```

#### Where `$` Stays

| Position | Today (decided spelling) | Under F |
| --- | --- | --- |
| Declaration or closure header | `-> Response $ Db + Clock:` | unchanged |
| Function-type sugar | `fn() -> O $ Db + Clock` | unchanged |
| Row type argument | `Task[T, $ Db + Clock]`, `$.Context[$ Db + Clock]` | `Task[T, Db + Clock]`, `$.Context[Db + Clock]` |
| Empty row | `$()` | `$()` |
| Alias right side | not allowed | `type AppRow = Db + Clock` |
| Provider operations | `$.use`, `$.with`, `$.context`, `$.Context` | unchanged |

The `$` in a header and in the sugar separates the result from the row.
Without it, `fn() -> Response + Db` would read the result type as part of
a row. In a type argument the parameter's kind already says "row", so `$`
adds nothing there.

`fn(A) -> O $ AppRow + Clock` stays exact sugar for
`Fn[(A,), O, AppRow + Clock]`
([`fn.type.ctor.sugar`](../spec/07-functions.md#r-fn.type.ctor.sugar)),
and `fn(A) -> O` for `Fn[(A,), O, $()]`. F changes no function type; it
only lets the third argument be any row expression.

#### Parsing Risk

An explicit row argument in expression position has the shape of an index.
`run[Clock + Log](job)` looks like `fs[i + 1](x)`. Today the reference
parser accepts that shape as an index and rejects `f[i32, Clock + Log](x)`
outright.

F therefore needs the parser to accept a row expression inside a
generic argument list, and name resolution to settle the one-argument
case. Explicit row arguments are rare, because use sites infer them
([`req.poly.least`](../spec/11-requirements-and-suspension.md#r-req.poly.least)).

**Changes.** One syntax change: row expressions in type-argument and alias
positions, with `$` dropped from row type arguments. It also needs one
kind rule for row aliases and one error for a row in `$.use` or a
binding. The alias cycle rule is shared with ordinary aliases. The grammar change lands on
the productions that the decided `+` spelling already rewrites.

**Gives up.** The local reading of a row, as C does. The cost order ranks
it last, because it is the only option that needs grammar. The empty row
keeps a special literal.

## Comparison

| | A: nothing new | B: adapting closures | C1: context type as key | D: bundle traits | E: wider inference | F: rows as type expressions |
| --- | --- | --- | --- | --- | --- | --- |
| Case 1, handler rows | least rows | least rows | least rows, or one name | one key | none written | least rows, or one name |
| Case 2, mixed table | open-row idiom | short wrapper | wide row is one name | one key | still exact | wide row is one name |
| Case 3, churn | up to 30 sites | up to 30 sites | one alias if handlers use it | one trait | none | one alias if handlers use it |
| Case 6, prod and test rows | two lists | two lists | shared | shared | two lists | shared |
| Case 8, generic libraries | needs RR-2 | needs RR-2 | needs RR-2 | needs RR-2 | needs RR-2 | needs RR-2 |
| Question 2, `$.with` reuse | context values | context values | context values | fan-out install | context values | context values |
| Rules added | 0 | 1 change | 1 exception, 1 error | 2-3 exceptions | 1 exception | 1 syntax, 1 kind rule, 1 error |
| Soundness risk | none | none | none | changes an existing meaning | none | none |
| Reads locally | yes | yes | no, look up the name | no | no | no, look up the name |
| Plain reading | yes | yes | a bundle type read as a row | a trait read as a row | yes | yes |
| Agent-writability | long rows, easy | easy | easy | delegation boilerplate | easiest | easy |
| Evolution | can add B, C or F later | combines with C or F | F later is a respelling | hard to undo | hard to undo | final |

Options B and C or F solve different problems and combine. B adapts closed
functions to a wide type; C and F shorten the rows that must stay wide. C1
and F are two mechanisms for one goal: C1 reads an existing type as a row,
and F makes rows type-level expressions.

## Ranking By Design Cost

The [Design Cost Order](../AGENTS.md#design-cost-order) prefers a library
addition, then an intrinsic, then a rule exception, then syntax. Ties are
broken by counting changes.

| Rank | Option | Costliest change | Count |
| --- | --- | --- | --- |
| 1 | A: nothing new | library and documentation | 0 language changes |
| 2 | B: adapting closures | semantic rule change | 1 |
| 3 | E: wider inference | semantic rule exception | 1, but it reverses a written rule |
| 4 | C1: context type as key | semantic rule exception | 2 (exception plus error) |
| 5 | D: bundle traits | semantic rule exception | 2-3, and it changes an existing meaning |
| 6 | F: rows as type expressions | syntax change | 3 (grammar, kind rule, error), plus the shared alias-cycle rule |

E ranks above C1 on count alone. The recommendation below sets it aside on
the owner principle of no action at a distance.

## Recommendation

**Recommendation.** Take A as the base, specify the rules of RR-2 and the
one-unknown rule of RR-6, and add F. Leave B for later.

- **A covers question 2.** Installation-side reuse is already the model
  every surveyed language uses: bundles are values made by functions.
  Context values give build-once reuse, and provider-installing functions
  give scoped reuse (case 11). No new feature is needed for layers,
  request scopes, or fixtures. F lets both name their row.
- **A covers most of question 1.** Private rows are inferred, libraries
  take row parameters, and handlers in a table use open rows, as Koka
  does. The rows that remain are the reviewer's summary of reach.
- **RR-2 must be written down anyway.** The open-row idiom and generic
  routers rest on three rules the spec implies but never states.
- **F closes the one real gap with the proven model.** Koka, Scala,
  Effect-TS and Haskell name a requirement set with the language's
  ordinary alias over a type-level set expression. F is exactly that, and
  every edge case above resolves with existing rules or one kind rule.
- **F over C1, against the cost order.** C1 is cheaper, but it reads a
  bundle type as a row, which is a pun a reader must learn. F's grammar
  change lands on the productions the decided `+` spelling already
  rewrites, so its marginal cost is small.
- **B can wait.** The open-row idiom covers the app's own handlers. B
  becomes worth its rule only if wrappers around closed library functions
  turn out to be common.
- **The one-unknown rule fits the decided "no subtraction".** With it,
  extension removes keys in every case found (cases 7, 11, 12). Koka and
  Links impose the same limit through syntax.

**What it gives up.** A named row must be looked up to be read. F is the
costliest option by the cost order, and the empty row keeps `$()`.
Handlers in a table carry `[R]` and `R +`.

**Next best.** C1 instead of F: the same reuse with no grammar change, at
the price of the pun. After that, A with the RR-2 rules alone.

Set aside: D changes an existing meaning and forces one provider to
implement every trait. E trades a written public row for action at a
distance.

Waits, per core-before-advanced: capturing the current row as a context
value, and a `ZLayer.make`-style graph solver in `std`.

## Questions For The Owner

### 1. Is installation-side reuse settled with no new feature?

**Effect.** Layers, request bundles and fixtures are functions returning
`$.Context` values, spread with `ctx...` and overridden by later entries
(cases 4-6). Provider-installing functions wrap a scope (case 11).

**Options.** (a) Yes: context values for build-once reuse, installers for
scoped reuse, documented in the guide and `std`. (b) Add a `std` helper
for layers or fixtures. (c) Add a language form.

**Recommendation.** (a).

```text
with_fakes!(fakes):
    $.with(Auth=DenyAll {}):
        _ := create_order!(sample_request())
```

### 2. Is the open-row idiom the answer for handlers in a shared table?

**Effect.** Each handler keeps its least row plus `R`, and still fits a
wide `Handler` type. A direct call must then solve an own-row `R` as the
empty row, which the spec does not yet say.

**Options.** (a) Yes; state that the least row solution applies to a
callee's own row. (b) No; handlers in a table declare the full row.

**Recommendation.** (a).

```text
pub fn health![R](req: Request) -> Response $ R + Clock:
    pass
```

### 3. Does an unannotated closure take a concrete expected row?

**Effect.** A closed function, such as a library helper, adapts to a wide
function type with `fn!(req): helper!(req)` instead of a wrapper that
writes the whole row.

**Options.** (a) Yes, when the expected row entails the body's
requirements. (b) Not now; the wrapper writes its row.

**Recommendation.** (b), revisited if such wrappers become common.

```text
Route { path: "/health", handler: fn!(req): health!(req) }  # proposed: takes Handler's row
```

### 4. Can one name stand for several keys in a row?

**Effect.** A wide app row is defined once and named in headers, function
types and context types. The prod and test rows share it.

**Options.** (a) No; rows are always written out. (b) C1: a key that names
a `$.Context[Row]` type stands for its row. (c) F: rows are type-level
expressions, and `type AppRow = Db + Cache` is an ordinary alias.

**Recommendation.** (c).

```text
type AppRow = Db + Cache + Log + Metrics + Config + HttpClient + Auth  # hypothetical syntax

pub fn create_order!(req: Request) -> Response $ AppRow + Clock:
    pass
```

### 5. Under F, is the empty row still written `$()`?

**Effect.** `Fn[(A,), O, $()]` and `$.Context[$()]` need an empty row, and
`()` is already the unit tuple type. The decided `+` spelling drops
`$(A, B)` but does not say what happens to `$()`.

**Options.** (a) Keep `$()` as the one empty-row literal. (b) Add a
prelude alias such as `type NoRow = $()` and write that. (c) Require the
empty row to be omitted wherever possible, with `$()` only in type
arguments.

**Recommendation.** (a).

```text
use std.function.Fn

fn plain() -> Fn[(i32,), i32, $()]:
    double
```

### 6. How do diagnostics print a row that uses an alias?

**Effect.** A missing-requirement error on `$ WebRow + Auth` must say which
key is missing and where it would come from.

**Options.** (a) Print rows as written, and add the expansion on a
mismatch or a missing key. (b) Always print the expanded row. (c) Always
print as written.

**Recommendation.** (a), as TypeScript and Rust report aliases in their
diagnostics.

```text
pub fn get_order!(req: Request) -> Response $ WebRow + Auth:
    respond(200, "ok")  # error: missing-requirement, Metrics is not in WebRow
```

### 7. Do public functions keep explicit rows in every package?

**Effect.** An application with many modules writes a row on every
cross-module function (RR-4).

**Options.** (a) Yes, keep `req.row.omitted.empty` for every `pub`
function. (b) Infer the rows of `pub` functions in packages with no library
target.

**Recommendation.** (a). The row is the reviewer's summary, and open rows
and named rows keep it short.

```text
pub fn health![R](req: Request) -> Response $ R + Clock:
    pass
```

### 8. May data types, enums and traits take row parameters?

**Effect.** A generic router or plugin list is declared as `Route[R]` or
`Handler[R]` (case 8). Today the spec is silent, though
[Nominal Function Types](FN_TYPE.md) lists row-kinded parameters on data
declarations as a follow-up that `data Tool[Rq]` already assumes.

**Options.** (a) Yes; `req.row.param.inferred` applies to every generic
parameter list. (b) No; row parameters stay on callables.

**Recommendation.** (a).

```text
pub data Route[R]:
    path: string
    handler: fn!(Request) -> Response $ R
```

### 9. May `$.Context[R]` take a row parameter?

**Effect.** A queue stores the providers a job should run with. Spreading
`$.Context[R]` would provide `R` inside the block.

**Options.** (a) Yes, and a spread of `$.Context[R]` makes `R` available.
(b) No; a context's row is always concrete.

**Recommendation.** (a). A row parameter is already passed as one bundle
([`req.poly.one-body`](../spec/11-requirements-and-suspension.md#r-req.poly.one-body)),
which is what a context value is.

```text
pub fn run_job![R](providers: $.Context[R], job: fn!() -> void $ R) -> void:
    $.with(providers...):
        job!()
```

### 10. Does a row pattern allow at most one unknown row variable?

**Effect.** `f: fn() -> T $ R1 + R2` with neither variable fixed has many
solutions that give different rows (case 12c). With the rule, `R + S` still
solves when another argument fixes `S` (case 12b).

**Options.** (a) Yes; other row variables in a pattern must be fixed by
another argument, the expected type, or an explicit type argument, and an
unmeetable pattern is rejected at the declaration. (b) No rule; reject
ambiguity only at each call.

**Recommendation.** (a).

```text
pub fn provide[T, S, R](providers: $.Context[S], f: fn() -> T $ R + S) -> T $ R:
    $.with(providers...):
        f()
```

## Sources

- Koka book: [effect types](https://koka-lang.github.io/koka/doc/book.html#sec-effect-types),
  [polymorphic effects](https://koka-lang.github.io/koka/doc/book.html#sec-polymorphic-effects)
  and [with statements](https://koka-lang.github.io/koka/doc/book.html#sec-with).
- Unison: [using abilities, part 2](https://www.unison-lang.org/docs/fundamentals/abilities/using-abilities-pt2/)
  and [abilities and ability handlers](https://www.unison-lang.org/docs/language-reference/abilities-and-ability-handlers/).
- Scala 3: [context functions](https://docs.scala-lang.org/scala3/reference/contextual/context-functions.html)
  and [intersection types](https://docs.scala-lang.org/scala3/reference/new-types/intersection-types.html).
- ZIO: [automatic layer construction](https://zio.dev/reference/di/automatic-layer-construction).
- Effect-TS: [layers](https://effect.website/docs/requirements-management/layers/).
- GHC: [ConstraintKinds](https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/constraint_kind.html);
  [effectful-core](https://hackage.haskell.org/package/effectful-core/docs/Effectful.html);
  [polysemy](https://hackage.haskell.org/package/polysemy/docs/Polysemy.html).
- Kotlin: [KEEP-367, context parameters](https://github.com/Kotlin/KEEP/blob/master/proposals/context-parameters.md).
- Rust: [RFC 1733, trait aliases](https://rust-lang.github.io/rfcs/1733-trait-alias.html),
  [tracking issue 41517](https://github.com/rust-lang/rust/issues/41517), and
  [axum](https://docs.rs/axum/latest/axum/).
- Go: [interface types](https://go.dev/ref/spec#Interface_types) and the
  [Wire guide](https://github.com/google/wire/blob/main/docs/guide.md).
- Guice: [getting started](https://github.com/google/guice/wiki/GettingStarted)
  and [`Modules`](https://google.github.io/guice/api-docs/latest/javadoc/com/google/inject/util/Modules.html).
- Dagger: [subcomponents](https://dagger.dev/dev-guide/subcomponents).
- Spring: [composing configuration classes](https://docs.spring.io/spring-framework/reference/core/beans/java/composing-configuration-classes.html).
- OCaml: [effect handlers](https://ocaml.org/manual/5.3/effects.html).
- Flix: [effect polymorphism](https://doc.flix.dev/effect-polymorphism.html)
  and [type aliases](https://doc.flix.dev/type-aliases.html).
- Row-variable limits: Leijen,
  [Koka: Programming with Row Polymorphic Effect Types](https://arxiv.org/abs/1406.2061);
  Hillerström and Lindley,
  [Liberating Effects with Rows and Handlers](https://homepages.inf.ed.ac.uk/slindley/papers/links-effect.pdf);
  Brachthäuser et al.,
  [Effects as Capabilities](https://se.informatik.uni-tuebingen.de/publications/brachthaeuser20effects/);
  Effect-TS [`Effect.ts`](https://github.com/Effect-TS/effect/blob/main/packages/effect/src/Effect.ts)
  (`provide`).

## Parse Log

Every `text` block was parsed with the reference parser on 2026-09-28.
The examples use the decided but unapplied spelling of
[Bound And Row Operators](OPEN_ISSUES.md#bound-and-row-operators), so each
block was first rewritten to today's forms.

| Written in this record | Rewritten for today's parser |
| --- | --- |
| a multi-key row `$ A + B` | `$(A, B)`, valid in every row position |
| a bound `T < A & B` or `trait X < A & B` | `A + B` |
| Option F: a row type argument without `$`, as in `$.Context[AppRow + Clock]` | `$.Context[$(AppRow, Clock)]` |
| Option F: a row alias `type AppRow = Db + Cache` | no form exists; the line stays and is marked `# hypothetical syntax` |

A block whose only errors fall on unrewritable hypothetical lines was
parsed a second time with those lines removed. No other text changed.
The diagnostic sample in Option F is compiler output in a `console` fence
and was not parsed.

Parsing checks syntax only; no block is claimed to type-check. Names such
as `Db`, `Request`, `Route` and `PgPool` are placeholders. `# error:` and
`# proposed:` comments state existing or proposed behavior, not a parser
result.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Case 1 | parses |
| 2 | Case 2, router table | parses |
| 3 | Case 2, wrapping closure | parses (see finding 2) |
| 4 | Case 2, open rows | parses |
| 5 | Case 3 | parses |
| 6 | Case 4 | parses |
| 7 | Case 5 | parses |
| 8 | Case 6 | parses |
| 9 | Case 7 | parses |
| 10 | Case 8 | parses |
| 11 | Case 9 | parses |
| 12 | Case 10 | parses |
| 13 | Case 11, installer | `syntax-error` at line 1, the row alias marked `# hypothetical syntax`; parses with that line removed |
| 14 | Case 11, request installer | parses |
| 15 | Case 11, fixture installer | parses |
| 16 | Case 12a | parses |
| 17 | Case 12b | parses |
| 18 | Case 12c | parses |
| 19 | Option A | parses |
| 20 | Option B | parses |
| 21 | Option C, C1 | parses |
| 22 | Option D | parses |
| 23 | Option E | parses |
| 24 | Option F | `syntax-error` at line 1, the row alias marked `# hypothetical syntax`; parses with that line removed, including the rewritten lines 11 and 15 |
| 25 | Question 1 | parses |
| 26 | Question 2 | parses |
| 27 | Question 3 | parses |
| 28 | Question 4 | `syntax-error` at line 1, the row alias marked `# hypothetical syntax`; parses with that line removed |
| 29 | Question 5 | parses |
| 30 | Question 6 | parses |
| 31 | Question 7 | parses |
| 32 | Question 8 | parses |
| 33 | Question 9 | parses |
| 34 | Question 10 | parses |

**Reference-parser findings.**

1. A body written as a bare `...` line is a `syntax-error`. Chapter 11
   writes it in illustrative examples such as
   [Requirement Polymorphism](../spec/11-requirements-and-suspension.md#requirement-polymorphism),
   so this record writes `pass` instead.
2. In today's comma spelling, take a closure header with a bare
   multi-key row, such as `fn!(req: Request) -> Response $ Db, Cache:`. It
   parses as a statement but is a `syntax-error` as a call argument or a
   data-literal field; the parenthesized `$(Db, Cache)` parses there.
   `grammar.type.row.header-bare`, now [retired](../spec/STYLE.md#retired-rule-ids),
   states no exception for list positions. The decided `+` spelling removes
   the comma clash.
3. Option F forms against today's parser, unrewritten:

   | Form | Result |
   | --- | --- |
   | `type WebRow = Db + Cache` | `syntax-error` |
   | `type WithLog[R] = R + Log` | `syntax-error` |
   | `type NoRow = $()` | `syntax-error` |
   | `type Store = Db` | parses |
   | `let f: Fn[(i32,), i32, Db + Clock] = g` | `old-row-operator` |
   | `let f: Fn[(i32,), i32, Db] = g` | parses |
   | `type C = $.Context[Db + Clock]` | `old-row-operator` |
   | `x := transform[Clock + Log](items)` | parses, as an index expression |
   | `x := transform[i32, Clock + Log](items)` | `syntax-error` |

   The last two rows are the parsing risk that Option F describes.
