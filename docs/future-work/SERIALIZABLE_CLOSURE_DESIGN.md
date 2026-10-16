# Serializable Closures And Remote Calls: Design Study

## Table Of Contents

- [Status](#status)
- [A. Problem](#a-problem)
- [B. How Other Systems Do It](#b-how-other-systems-do-it)
- [C. Candidate Shapes](#c-candidate-shapes)
- [D. Stress Test](#d-stress-test)
- [E. Cost Model](#e-cost-model)
- [F. Findings, Settled Points, And Owner Questions](#f-findings-settled-points-and-owner-questions)
- [G. Recommended Design And Discussion Log](#g-recommended-design-and-discussion-log)
- [Parse Log](#parse-log)
- [Sources](#sources)

## Status

This is the design study for issue #166. It decides nothing, and the spec
is frozen. Every rule ID, diagnostic code, std name, and spelling below is
a placeholder written by the agent. The [Parse Log](#parse-log) records a
syntax-only parse of every example; nothing is type-checked.

A line marked `# error:` gives the diagnostic a proposed checker would
report.

The owner framed the study on 2026-10-15: the vision quoted in
[A.1](#a1-the-big-idea), the five goals of [A.2](#a2-requirements), and
the systems to compare. On 2026-10-16 the owner asked for heterogeneous
places beyond browser and server, and asked whether real closures should
travel ([G.1](#g1-discussion-log)). This revision answers both. These
owner decisions stand:

| Decision | Source |
| --- | --- |
| A serializable closure's code identity is a content hash. | [Open Issues](OPEN_ISSUES.md#serializable-closures-and-incremental-computation), 2026-09-27 |
| Captures must be boundary-safe values; this replaced a `Durable` bound. | same, citing Durable Replay decision 11 |
| Capturing a provider or mutable state is rejected. | same |
| Not in the new compiler's first release. | owner, 2026-10-06 |
| Durable workflows replay recorded host results; they never serialize a stack. Recording is opt-in per run. | Replay Rules (archived `RUNTIME_AND_LIBRARY.md`, in git history before commit `38560c7a`) |
| A handle never travels; authority is rebound by requirements at the destination. | issue #166, [NonEscapable design](NONESCAPE_LIFETIME_DESIGN.md) |
| Which code may run on a GPU is a separate study. This design only keeps the place model open to it. | owner, 2026-10-16 |
| Portable closures are same-build only. An identity a later build lacks fails with `Incompatible`; durable and cross-deploy work uses named `@remote` functions. | owner, 2026-10-16 |

## A. Problem

### A.1 The Big Idea

The owner's words:

> "Programming languages have always evolved to manage new layers of
> abstraction and complexity: Assembly managed raw CPU instructions. C
> abstracted away registers and control flow. Java automated memory
> management. Go simplified concurrency for multi-core processors. The
> next logical step in this evolution is to manage the complexities of
> data management. The challenge is no longer just managing memory or
> threads, but managing where computation happens, how data moves, and
> how to ensure that access is fast, persistent, and reliable."

Each step gave the language a mechanism that programmers stopped writing
by hand. Java's garbage collector replaced `free`. Go's scheduler replaced
thread pools and callbacks. The step this study looks for replaces the
hand-written parts of moving data and work: the endpoint, the `fetch`
call, the decoder, the hand-rolled token, the job queue, and the cache
with its invalidation.

So this study judges each shape by four questions. How well does the
language itself tell the reader:

| | Question | Today in hd |
| --- | --- | --- |
| W1 | **Where data lives** and where computation runs? | Requirement rows say what a function needs; runtime profiles say what a host can bind. Nothing says which place runs a function. |
| W2 | **How data moves** between places? | Registered boundaries move boundary-safe values; the adapter is generated from the signature. Calling across them is library work. |
| W3 | **Who may see or call** it? | Registration is explicit (`module.register.explicit`). Nothing protects data one place hands to an untrusted place and expects back. |
| W4 | **When a copy is stale**? | Nothing. A fetched value is a snapshot with no version. |

RPC, fetching, security, caching, and a minimal wire are the means to
answer these four questions.

### A.2 Requirements

The owner's five goals are the requirements. Every shape is judged
against them. The agent's restatement below names places, not a browser
and a server, after the owner's 2026-10-16 feedback.

| Goal | Requirement |
| --- | --- |
| G1 | **Protocol-transparent RPC.** Calling code at another place looks like a call. The transport (HTTP, WebSocket, worker message, service RPC, in-process) is not in user code. |
| G2 | **Easy data fetching.** One place asks another for data without hand-written endpoints, fetch code, or decoders. |
| G3 | **Secure by default.** Nothing is reachable unless the program says so. An untrusted caller cannot call an unexported function, reach arbitrary data, or forge captured values. |
| G4 | **A cache, ideally like Skip.** Results are memoized, incremental, and invalidated when their inputs change. |
| G5 | **A minimal wire.** Send a reference to known code plus boundary-safe data, rather than code. |

Shipping arbitrary closures is in scope only where it serves these goals.
[C.6](#c6-not-a-shape-shipping-code) explains why no shape ships code.

### A.3 What The Language Already Gives

| Existing rule | What it gives a design |
| --- | --- |
| [`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed) | The value types that already cross: scalars, `string`, tuples, lists, maps, data, enums, optionals, results. |
| [`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe) | Mutable types, `dyn` values, closures, and live handles never cross. |
| [`module.boundary.rows`](../../spec/lang/10-modules.md#r-module.boundary.rows) | Requirement rows are host bindings, never serialized. |
| [`module.register.row`](../../spec/lang/10-modules.md#r-module.register.row) | A registered entry's row is checked against the host's profile before user code runs. Applied per place, this is the placement rule ([C.9](#c9-placement-by-requirements)). |
| [`module.boundary.serialize.exact`](../../spec/lang/10-modules.md#r-module.boundary.serialize.exact) | A type's `Serialize` decides exactly what leaves hd, so a result type is an exposure decision. |
| [`module.boundary.deserialize.no-dyn`](../../spec/lang/10-modules.md#r-module.boundary.deserialize.no-dyn) | The target type comes from the signature, never from the data. |
| [`fn.capture.no-providers`](../../spec/lang/07-functions.md#r-fn.capture.no-providers) | A closure takes no provider from a `$.with` around it; keys go in its row. |
| [`fn.closure.monomorphic`](../../spec/lang/07-functions.md#r-fn.closure.monomorphic) | A closure has one monomorphic function type, so a closure literal names one body. |
| [`req.determinism.depends`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends) | Code between host calls is deterministic, so host results determine a run. A cache can be keyed by them. |
| [`cli.engine.same-behavior`](../../spec/cli/command-line.md#r-cli.engine.same-behavior) | One compiled module behaves the same on every engine. |
| [`cli.cap.table`](../../spec/cli/command-line.md#r-cli.cap.table) | `hd.toml` grants each host capability trait, with scopes inside a key. |
| [`annot.facts-of.target`](../../spec/lang/14-annotations.md#r-annot.facts-of.target) | An intrinsic may require its argument to name a module-level function. |
| [`req.combinator.all-typing`](../../spec/lang/11-requirements-and-suspension.md#r-req.combinator.all-typing) | An intrinsic may have a written typing rule instead of a signature. |
| [`ne.call.global`](NONESCAPE_LIFETIME_DESIGN.md#44-call-sites) | A NonEscapable value is rejected by category, whatever its reach; a fresh handle reaches `static` and is still rejected. |

The lifetime design's D5 keeps function types uniform: one rule for every
function-typed parameter. No shape below adds a second kind of function
type; a portable closure is a data type that holds an ordinary `fn(...)`
type ([C.10](#c10-portable-closures)).

### A.4 Motivating Programs

Programs 1 to 4 run between a browser and a server. Programs 5 to 8 run
between other places: two services, workers, a store, and a GPU.

**1. A page fetches its data.** A profile page needs the signed-in user's
name and recent orders. Today: an HTTP route, a JSON encoder, a client
`fetch`, a decoder, and two copies of the types. The goal is one function
and one call.

```hd
use std.remote.{Caller, Remote, RemoteError, remote}

pub data OrderLine:
    pub id: string
    pub total: i64

pub data Profile:
    pub name: string
    pub recent: List[OrderLine]

pub enum ProfileError:
    NotSignedIn
    NotFound

pub trait UserRead:
    fn profile!(self, user: string) -> Result[Profile, ProfileError]

# Shared module, compiled into the server and the browser.
@query
pub fn my_profile!() -> Result[Profile, ProfileError] $ UserRead + Caller:
    match $.use(Caller).user():
        .Some(user) => $.use(UserRead).profile!(user)
        .None => .Err(.NotSignedIn)

# Browser code: one call; the transport is the Remote provider's business.
fn greeting!() -> string $ Remote:
    match remote!(my_profile):
        .Ok(.Ok(profile)) => "Hello, ${profile.name}"
        .Ok(.Err(.NotSignedIn)) => "Please sign in"
        .Ok(.Err(.NotFound)) => "No profile yet"
        .Err(_) => "Offline; try again"
```

**2. A form carries a snapshot back.** Next.js's own example: a publish
button must fail if the document changed since the page was rendered. The
version is captured at render time and must come back unchanged. A
client must not be able to edit it.

```hd
use std.remote.{Caller, Remote, Seal, Sealed, remote, seal}

pub data Draft:
    pub title: string
    pub seen: Sealed[i64]           # the browser carries it; it cannot read or change it

pub enum PublishError:
    Forbidden
    Changed

pub trait Docs:
    fn title!(self, id: string) -> string
    fn version!(self, id: string) -> i64
    fn may_edit!(self, user: string, id: string) -> bool
    fn publish!(self, id: string) -> void

@query
pub fn open_draft!(id: string) -> Draft $ Docs + Seal:
    docs := $.use(Docs)
    Draft { title: docs.title!(id), seen: seal(docs.version!(id)) }

@remote
pub fn publish!(id: string, seen: Sealed[i64]) -> Result[void, PublishError] $ Docs + Caller + Seal:
    docs := $.use(Docs)
    user := match $.use(Caller).user():
        .Some(found) => found
        .None => return .Err(.Forbidden)
    if !docs.may_edit!(user, id):
        return .Err(.Forbidden)
    if seen.open() != docs.version!(id):
        return .Err(.Changed)
    docs.publish!(id)
    .Ok()
```

**3. A live view stays fresh.** A chat room shows the last 50 messages.
When anyone posts, every open view updates, and the server re-reads only
what changed. This is Convex's reactive query and Skip's resource.

```hd
pub data Message:
    pub author: string
    pub text: string

pub enum ChatError:
    NotSignedIn
    TooLong

pub trait MessageRead:
    fn latest!(self, room: string, count: usize) -> List[Message]

pub trait MessageWrite:
    fn append!(self, room: string, message: Message) -> void

@query
pub fn recent!(room: string) -> List[Message] $ MessageRead:
    $.use(MessageRead).latest!(room, 50)

@remote
pub fn post!(room: string, text: string) -> Result[void, ChatError] $ MessageWrite + Caller:
    if text.len() > 2000:
        return .Err(.TooLong)
    match $.use(Caller).user():
        .Some(user) => $.use(MessageWrite).append!(room, Message { author: user, text: text })
        .None => return .Err(.NotSignedIn)
    .Ok()

# Browser code. `watch` is an illustrative library call over Remote: each
# `next!` completes when the query's result changes.
fn follow!(room: string) -> void $ Remote + Console:
    let feed: mut Watch[List[Message]] = watch(recent, room)
    while true:
        messages := feed.next!()
        println("${messages.len()} messages")
```

**4. A mutation survives a crash.** An order is charged, then a receipt
is mailed. The server dies between the two and is redeployed. The run
must resume without charging twice, as Temporal does and as durable
resume (#18) plans.

```hd
pub data OrderId:
    pub value: string

pub enum FulfilError:
    Declined(reason: string)
    Undeliverable(address: string)

pub trait Payments:
    fn charge!(self, order: OrderId, key: string) -> Result[void, FulfilError]

pub trait Mailer:
    fn send_receipt!(self, order: OrderId) -> Result[void, FulfilError]

@remote
pub fn fulfil!(order: OrderId) -> Result[void, FulfilError] $ Payments + Mailer:
    $.use(Payments).charge!(order, "charge-${order.value}")?
    $.use(Mailer).send_receipt!(order)?
    .Ok()
```

**5. A service calls another service.** Checkout reserves stock from an
inventory service that another team deploys on its own schedule. The call
needs a deadline, safe retries, and a clear failure when the two deploys
disagree. gRPC gives deadlines and retry policies; Dapr gives service
identity and retries.

```hd
use std.remote.{Caller, Remote, RemoteError, remote}
use std.task.race
use std.time.{Clock, Duration, sleep}

# Inventory service: its own package, team, and deploy schedule.
pub data Reservation:
    pub sku: string
    pub count: usize
    pub key: string                 # idempotency key chosen by the caller

pub enum ReserveError:
    OutOfStock
    NotAService

pub trait Stock:
    fn reserve!(self, r: Reservation) -> Result[void, ReserveError]

@remote
pub fn reserve!(r: Reservation) -> Result[void, ReserveError] $ Stock + Caller:
    if $.use(Caller).service().is_none():
        return .Err(.NotAService)
    $.use(Stock).reserve!(r)

# Checkout service, built against the inventory package's interface.
pub enum CheckoutError:
    SoldOut
    Unavailable
    Misconfigured

fn hold!(order: string, sku: string) -> Result[void, CheckoutError] $ Remote + Clock:
    r := Reservation { sku: sku, count: 1, key: "hold-$order" }
    let tries = 0
    while tries < 3:
        tries = tries + 1
        match race!(call_reserve(r), deadline()):
            .Ok(.Ok()) => return .Ok()
            .Ok(.Err(.OutOfStock)) => return .Err(.SoldOut)
            .Ok(.Err(.NotAService)) => return .Err(.Misconfigured)
            .Err(.Incompatible) => return .Err(.Misconfigured)
            .Err(.Unreachable) => continue
            .Err(.OutcomeUnknown) => continue     # the same key makes a retry safe
            .Err(_) => return .Err(.Unavailable)
    .Err(.Unavailable)

fn call_reserve!(r: Reservation) -> Result[Result[void, ReserveError], RemoteError] $ Remote:
    remote!(reserve, r)

fn deadline!() -> Result[Result[void, ReserveError], RemoteError] $ Clock:
    sleep!(Duration::milliseconds(300))
    .Err(.OutcomeUnknown)
```

**6. A server fans work out to workers.** A ranking job scores 100,000
orders with weights the request chose. The scoring runs on a worker pool
of the same build, as Ray tasks run on a cluster. The weights travel; the
code does not.

```hd
use std.remote.{Portable, RemoteError, portable}

pub data Order:
    pub id: string
    pub total: i64

pub data Weights:
    pub size: f64
    pub bias: f64

pub fn score(order: Order, weights: Weights) -> f64:
    f64(order.total) * weights.size + weights.bias

# Illustrative library provider over a worker pool place.
pub trait Workers:
    fn map![A < Boundary, B < Boundary](self, items: List[A], job: Portable[fn(A) -> B]) -> Result[List[B], RemoteError]

fn rank!(orders: List[Order], weights: Weights) -> Result[List[f64], RemoteError] $ Workers:
    $.use(Workers).map!(orders, portable(fn(o: Order) -> f64: score(o, weights)))
```

**7. A predicate runs beside the data.** A report needs the orders above
a threshold. Sending the predicate to the store moves only the matching
rows. The store must be a place that runs the same build, such as a
storage service written in hd.

```hd
use std.remote.{Portable, portable}

pub trait OrderStore:
    fn where!(self, keep: Portable[fn(Order) -> bool]) -> List[Order]

fn large_orders!(min: i64) -> List[Order] $ OrderStore:
    $.use(OrderStore).where!(portable(fn(o: Order) -> bool: o.total > min))
```

**8. A kernel runs on a GPU (sketch only).** Brightening an image is one
small function over many pixels. Which code a GPU may run is a separate
study (owner, 2026-10-16). The sketch only shows that placement and the
closure tier leave room for it.

```hd
use std.remote.{Portable, portable}

# Sketch only: the kernel subset and the buffer path are the separate GPU study.
pub trait Gpu:
    fn map!(self, input: List[f32], kernel: Portable[fn(f32) -> f32]) -> List[f32]

fn brighten!(pixels: List[f32], gain: f32) -> List[f32] $ Gpu:
    $.use(Gpu).map!(pixels, portable(fn(p: f32) -> f32: p * gain))
```

### A.5 Places And Edges

Programs 1 to 4 are one pair of places, not the model. A **place** is an
execution context that runs a build of the program under a runtime profile
(agent term). A browser, a server, a worker pool, a peer service, a store,
and a GPU are places.

Places differ in three ways:

| Difference | Example | What decides it |
| --- | --- | --- |
| What it can bind | A server binds a database; a browser cannot; a GPU binds only its device. | The place's runtime profile, matched against rows ([C.9](#c9-placement-by-requirements)). |
| What code it can run | A worker runs any function of its build; a GPU runs a kernel subset. | The build it runs; the GPU subset is the separate study. |
| How far it trusts each caller | A browser may be anyone; a worker is the same operator; a peer service is authenticated. | The edge's trust. |

An **edge** joins a calling place to a called place. Each edge has its
own trust, principal, and data path. Security attaches to untrusted edges
only.

| Edge | Caller's trust | Principal in `Caller` | Protection | Builds | Data path |
| --- | --- | --- | --- | --- | --- |
| Browser to server | Untrusted | The signed-in user, or none | `Caller`, `Sealed`, Origin and CSRF checks | The browser may run an older build. | HTTP or WebSocket |
| Server to worker | Trusted: same operator | None needed | None | Same build | Worker message or shared memory |
| Service to service | Authenticated peer | The calling service | `Caller` names the service; the transport authenticates it, as Dapr does with mutual TLS. | Independent deploys | Service RPC |
| Server to store | Trusted, or a peer service | The server's service | As for a service | Same build, for a pushed-down closure | The store's protocol |
| Host to GPU | Trusted | None | None | Same build; kernel subset | Device buffers (separate study) |

The study keeps its names, which were never browser-specific. A **home**
is the place that runs a `@remote` function. "Client" in the web cases
means an untrusted calling place. One name changes: `Caller` gains a
service principal beside the user ([C.1](#c1-the-shared-core)).

## B. How Other Systems Do It

B.1 to B.3 compare the web systems that the five goals name, with Unison,
Cloud Haskell, and Temporal for code identity and durability. B.4 adds
systems that place work on other kinds of places or give closures an
identity. Spark, Ray, and Java serializable lambdas share one row in B.1,
as the cautionary case for shipping code.

### B.1 Against The Five Goals

| System | G1 RPC looks like a call | G2 Data fetching | G3 Exposure and forgery | G4 Cache | G5 Wire |
| --- | --- | --- | --- | --- | --- |
| React Server Functions | Yes. A `"use server"` function imported by client code becomes a reference; calling it sends a request. Works as a `<form action>`, before JavaScript loads (progressive enhancement), and with `useActionState`. | Through Server Components or by calling the function. | Every `"use server"` export is an endpoint. React: "Arguments to Server Functions are fully client-controlled." Extra arguments come from hidden fields or `.bind`. | None in React itself. | A reference ID plus serialized arguments. |
| Next.js Server Actions | Yes, as React. | Server Components fetch during render. | Encrypted, non-deterministic action IDs; unused actions stripped. Variables captured by an inline action are encrypted with a per-build key. Next.js: "We don't recommend relying on encryption alone." POST only, with an Origin check. | `revalidatePath`, `updateTag`: manual invalidation, then one response carries the new UI. | Action ID plus arguments plus encrypted captures. |
| React Server Components | Not RPC: the server renders, and only output reaches the client. | Yes: async components read data directly. | Server code never ships. Props crossing to the client must be serializable; only Server Functions may cross as functions. | Framework-level. | Rendered output, not data APIs. |
| tRPC | Yes: `api.users.getById({ id: 1 })`. | Queries, mutations, subscriptions; types inferred in a TypeScript monorepo. | Every procedure is an endpoint; input validators check shape. | Through client libraries. | Plain HTTP requests and responses. |
| Telefunc | Yes: import and call. The file is replaced by a thin HTTP client. | Yes. | Only exports of `.telefunc` files are callable. Arguments are validated at run time by a generated `shield()`; `getContext()` gives the user. | None. | Function name plus arguments. |
| React Router (Remix) | Loaders and actions per route, called by the router. | Yes: a route's `loader` runs on the server, typed data reaches the component; the loader is removed from client bundles. | Loaders and actions are endpoints. | After an action, "all loader data on the page is revalidated". | Serialized loader data. |
| Skip (SkipLabs) | Clients read or subscribe to resources over HTTP. | Yes, as resources. | Service-defined. | Yes, the model case: mappers "must be deterministic and side-effect free", so the runtime re-runs only what changed. | Updates to resources. |
| Convex | Yes: typed query and mutation functions. | Reactive queries. | Functions are endpoints; arguments validated. | Yes: deterministic queries; Convex tracks their reads, caches results, and pushes new results to subscribers when the data changes. | Function name plus arguments; pushed results. |
| Cap'n Proto RPC | Yes: calls on interface references. | Yes. | Capabilities: a reference "both designate[s] an object to call and confer[s] permission to call it". | None. | Messages plus capability table entries; promise pipelining saves round trips. |
| gRPC | Yes: generated stubs. | Yes; unary and streaming calls. | Every service method is an endpoint; the `.proto` schema is explicit. | None. | Protobuf messages. |
| Unison | Yes: `Remote` computations. | Typed storage in Unison Cloud. | Not a web-exposure model. | Test results cached by hash. | Code by hash; the receiver fetches what it lacks. |
| Cloud Haskell | Process messages. | No. | No exposure model. | None. | A static key plus captured values. |
| Temporal | Workflows call activities like functions. | No. | Workers expose registered workflows and activities. | Activity results memoized in the history. | Names plus payloads. |
| Spark, Ray, Java lambdas | Closures passed to library calls. | No. | Whatever closure arrives runs. Java's tutorial: lambda serialization "is strongly discouraged". | Spark lineage; Ray object store. | Serialized closures: class names or pickled code. |

### B.2 The Brief's Ten Questions

| System | 1. Opt-in | 2. Code identity | 3. Destination | 4. Captures | 5. Authority |
| --- | --- | --- | --- | --- | --- |
| React Server Functions | `"use server"` on a module or function. | A framework-made reference ID. | The same app build. | `.bind` arguments, serialized; React specifies no protection. | The server's; each function checks the user. |
| Next.js | as React | Encrypted ID, regenerated per build (cached at most 14 days). | Same build: "actions can only be invoked for a specific build". | Inline-action captures, encrypted. | as React |
| tRPC, Telefunc, gRPC | A router, a `.telefunc` file, a `.proto` service. | Procedure path, function name, method name. | Any client of a compatible API. | None: arguments only. | Context or metadata per request. |
| React Router | A route module's `loader` and `action` exports. | The route path. | The same app. | None. | The server's. |
| Skip, Convex | Service resources; query and mutation functions. | Resource or function name. | The service. | None. | The service's; Convex queries cannot fetch third-party APIs. |
| Cap'n Proto | Interfaces in a schema. | Interface and method IDs; capability references. | Any peer. | Server-side object state behind a reference. | The capability itself. |
| Unison | None: any value. | Content hash of the syntax tree with dependency hashes; names excluded. | Any program; missing code is fetched. | Any value. | Abilities in the type, handled at the destination. |
| Cloud Haskell | `static e`, closed; values added with `cap`. | A static key, stable only for "processes launched from the same program binary". | Same binary. | `Serializable` values. | None. |
| Temporal | Registered workflow and activity functions. | Type names; code on the worker. | The worker's build; Worker Versioning ties executions to builds. | Arguments only. | The worker's. |

| System | 6. Suspension | 7. Failure | 8. Versioning | 9. Incremental | 10. Wire format |
| --- | --- | --- | --- | --- | --- |
| React Server Functions | Async functions; `useActionState` replays submissions made before hydration. | Errors reach the client. | Bundler APIs "do not follow semver". | None. | RSC wire format. |
| Next.js | Actions dispatch one at a time per client. | "Failed to find Server Action" after a deploy. | New IDs per deploy; "surface the error as a retry path". | Tag and path revalidation. | as React |
| tRPC, Telefunc, gRPC | Async calls; gRPC streaming. | Typed errors; gRPC status codes and deadlines; either side may cancel. | Schema evolution rules (gRPC); none (Telefunc). | None. | JSON over HTTP; protobuf. |
| React Router | Async loaders. | Error boundaries. | None. | Revalidation after actions. | Serialized loader data: promises, maps, sets, dates. |
| Skip, Convex | Async; subscriptions. | Service errors. | Redeploy. | Yes: read-set or mapper based. | JSON. |
| Cap'n Proto | Promises; pipelining. | "disconnected" exceptions; persistent capabilities only where the host implements save and restore. | Schema evolution rules. | None. | Cap'n Proto encoding. |
| Unison | Abilities; no stack shipping. | Abilities. | Old hashes stay valid forever. | By hash. | One binary format. |
| Cloud Haskell | `Process` monad. | Monitors and links. | A new binary invalidates every key. | None. | `Binary`. |
| Temporal | Replay from the event history. | Retry policies, timeouts, cancellation. | `patched`/`GetVersion` or Worker Versioning, else a nondeterminism error. | Memoized activity results. | Data converter. |

### B.3 What The Comparison Shows

1. **The winning web designs name the code and send data.** React,
   Next.js, tRPC, Telefunc, React Router, Convex, and gRPC all send a
   reference to known server code plus arguments. None ships code to the
   server. This is G5.
2. **Exposure by a mark is the common contract, and arguments are always
   untrusted.** `"use server"`, `.telefunc` files, routers, and schemas
   decide what is reachable. Every system tells the function to check the
   caller itself.
3. **Captured server state is the hard part.** React's `.bind` arguments
   and hidden fields are client-controlled. Next.js encrypts inline
   closures, per build. Cap'n Proto keeps the state on the server behind a
   capability, which dies with the connection unless saved.
4. **Caches need determinism and a read set.** Skip requires pure
   mappers; Convex requires deterministic queries and tracks their reads.
   hd already promises determinism between host calls, and the Replay
   Rules already intercept every host call.
5. **Per-build identity breaks clients at every deploy.** Next.js IDs and
   Cloud Haskell keys change with the build. Unison's content hashes and
   gRPC's schema rules keep old callers working.

### B.4 Places, Placement, And Closure Identity

Each claim below cites its source in [Sources](#sources). Triton, Futhark,
and WebGPU are listed only as pointers for the separate GPU study.

| System | Places | How work is placed | Trust between places | Closure or code identity | Versions |
| --- | --- | --- | --- | --- | --- |
| Ray | Nodes of a cluster. | A task or actor declares logical resources, such as `num_gpus=1`; the scheduler runs it on a node that has them. | Nodes inside one cluster; the cited docs describe no per-call caller check. | Functions travel pickled (cloudpickle). | A failed task is retried up to `max_retries`; `ray.cancel` cancels one. |
| Unison | Locations in Unison Cloud. | The `Remote` ability forks computations to locations. | Not addressed in the cited docs. | Code moves by content hash; a receiver syncs the hashes it lacks. | Old hashes stay valid. |
| ScalaLoci | Peers in one program. | Placement types: a value of type `T on P` lives on peer `P`. Ties declare which peers may talk. | Not addressed; ties type the architecture, not trust. | The compiler splits the program per peer. | One program. |
| Hydro | Processes and clusters. | Location types on live collections; data moves between locations only by explicit network operators. | Not addressed in the cited docs. | One program compiled per location. | One program. |
| Orleans | Silos in a cluster. | Virtual actors (grains) exist logically and are activated on demand; a placement strategy picks the silo. | Silos inside one cluster; the cited docs describe no per-call caller check. | A grain is named by its type and key. | Grain interfaces carry versions; a compatibility strategy decides which versions may call each other. |
| Dapr | Apps with sidecars. | A call names the target app's ID; the sidecar resolves it. | Mutual TLS between sidecars; access control lists allow or deny callers by app ID. | Method names. | Resiliency policies add retries and timeouts. |
| Temporal | Workers. | A task queue routes workflow and activity tasks to workers. | Not addressed in the cited docs. | Workflow and activity type names. | Patching or Worker Versioning across deploys; activities take retry policies. |
| Cloud Haskell | Nodes running one binary. | Explicit `spawn` at a node. | Not addressed in the cited docs. | `static` pointers name closed code; a closure pairs one with serialized captures. Keys hold only for one binary. | A new binary breaks every key. |
| Scala Spores | JVM processes. | Library calls. | Not addressed in the cited docs. | A spore makes its captured environment explicit and lets types constrain the captures. Spores3 builds portable spores from top-level definitions plus serialized environments. | Same program. |
| Triton | GPU kernels from Python. | `@triton.jit` functions compile to GPU code. | Not applicable. | Pointer only. | Pointer only. |
| Futhark | GPU programs. | A purely functional array language compiled to GPU code. | Not applicable. | Pointer only. | Pointer only. |
| WebGPU | A browser's GPU. | Shaders in WGSL run through the WebGPU API. | Not applicable. | Pointer only. | Pointer only. |

Push-down has one more precedent. .NET LINQ turns a lambda into an
expression tree, which a provider may translate to SQL. hd does not
translate closures into another language. A store that runs a portable
closure must run the same build (Program 7).

### B.5 What The New Rows Show

1. **Placement by declared needs is common.** Ray matches declared
   resources to nodes; ScalaLoci and Hydro make the place part of a type.
   hd's requirement rows already declare needs, so placement can match
   rows to profiles ([C.9](#c9-placement-by-requirements)).
2. **Trust is a property of the edge.** Dapr authenticates services and
   filters them by app ID. For Ray and Orleans clusters, the cited docs
   describe no per-call caller check. Only the browser edge needs sealed
   values.
3. **Closure identity works without shipping code.** Cloud Haskell's
   `static` and Spores3 name code the receiver already holds, plus
   serialized captures. Both hold only for one binary, which matches the
   owner's same-build decision.
4. **Cross-version work uses named, versioned entry points.** Temporal
   patches workflows, Orleans versions grain interfaces, and protobuf has
   field rules. None relies on a closure surviving a deploy.
5. **GPU code is a language subset.** Triton, Futhark, and WGSL each limit
   what a kernel may do. That subset is the separate study.

## C. Candidate Shapes

The three shapes share a core ([C.1](#c1-the-shared-core)). Shape 2 adds
sealed values to it, and Shape 3 adds capability references. Two parts
apply to every shape: placement by requirements
([C.9](#c9-placement-by-requirements)) and portable closures, a second
tier beside named `@remote` functions ([C.10](#c10-portable-closures)).

### C.1 The Shared Core

**Exposure.** A function becomes callable from another place only by a
mark on its declaration. This is `"use server"` and Telefunc's export,
written as an hd decorator.

```hd
@remote
pub fn post!(room: string, text: string) -> Result[void, ChatError] $ MessageWrite + Caller:
    ...
```

1. r[remote.decl] `@remote` may precede a module-level function. It exposes the function to callers in other places. (agent)
2. r[remote.decl.signature] Every parameter type and the result type must be boundary-safe. Error: `remote-signature-not-boundary-safe`. (agent)
3. r[remote.decl.generic] A `@remote` function has no type parameters. Error: `remote-generic`. (agent)
4. r[remote.decl.table] Every `@remote` function in a module the program includes is on the program's remote table, under its call identity. Nothing else is. (agent)
5. r[remote.decl.home] A place **serves** its remote table when a library registration binds the table to a transport. Registration checks each function's row against the place's profile (`module.register.row`), so a missing key fails before any call (`module.register.failure`). (agent)

**Calling.** A call names the function and passes its arguments. The
transport is a requirement, so the caller's row says "this code talks to
another place" without naming HTTP, WebSocket, or a worker queue.

```hd
fn send!(room: string, text: string) -> Result[Result[void, ChatError], RemoteError] $ Remote:
    remote!(post, room, text)
```

1. r[remote.call] `std.remote` declares the compiler intrinsic `remote`. A use must be a bang call whose first argument names a `@remote` function, directly or through a qualifier or `use` import, as `annot.facts-of.target` requires. Error: `remote-not-exposed`. (agent)
2. r[remote.call.typing] For `f: fn!(P1, ..., Pn) -> O $ R`, or the plain form, `remote!(f, a1, ..., an)` checks each `ai` against `Pi` and has type `Result[O, RemoteError]`. It needs `$ Remote`, not `R`: the home binds `R`. A written typing rule, as `all!` has. (agent)
3. r[remote.call.transport] `std.remote.Remote` is a host capability trait. Its provider decides the transport per edge: HTTP, WebSocket, a worker message, service RPC, or in-process for tests. (agent)
4. r[remote.call.cancel] Cancelling the calling suspension cancels the call through the provider's abort contract (`req.cancel.external-abort`). A transport should carry the cancel to the home, as gRPC propagates deadlines. (agent)

```hd
pub enum RemoteError:           # illustrative
    Unreachable                 # the call never started
    OutcomeUnknown              # started; no result arrived
    Incompatible                # the home has no function or closure with this identity
    Rejected(message: string)   # the home refused the arguments
    Panicked(category: string)  # by spec diagnostic name, as the Replay Rules record panics
```

**The caller.** A function that needs to know who called it says so in
its row. The transport binds `Caller` per request. On a service edge the
principal is the calling service, so `Caller` has a second method; this
is the one name the reframe generalizes.

```hd
pub trait Caller:
    fn user(self) -> string?        # the authenticated user, or none
    fn service(self) -> string?     # the authenticated calling service, or none
```

1. r[remote.caller] `std.remote.Caller` is a requirement trait that the serving transport binds for each request. It holds the authenticated user, the calling service, or neither. (agent)
2. r[remote.caller.authz] Authorization is ordinary code in the function, as React, Next.js, and Telefunc require. The language guarantees exposure and typed arguments, not ownership checks. (agent)

**Arguments.** Every argument from an untrusted edge is untrusted. The
adapter decodes it by the declared parameter type
(`module.boundary.deserialize.exact`), which gives Telefunc's `shield()`
without a second declaration.

**Two identities.** A call and a cache need different identities.

| Identity | Made of | Used for |
| --- | --- | --- |
| **Call identity** | Module path, function name, and a fingerprint of the signature's types: parameter, result, and row layouts | Routing a call. A body change keeps it, so a client built before a bug fix still calls the fixed server. |
| **Content identity** | SHA-256 over the function's normalized semantic form, the content identities of every item it references (recursive groups hashed as one unit, as Unison does), type layouts, the compiler semantic version, and the build profile; names excluded | Cache keys, durable histories, and portable closures ([C.10](#c10-portable-closures)), where a body change must invalidate. |

Content identity is the 2026-09-27 decision's content hash, applied where
code meaning matters. The Replay Rules' whole-program identity stays for
histories; per-function identity lets an unchanged query keep its cache
across a deploy.

**Queries.** A query is a remote function whose result the home may
memoize and push to subscribers.

1. r[remote.query] `@query` marks a remote function as a query. It implies `@remote`. (agent)
2. r[remote.query.row] Every key in a query's row must be a trait that the runtime profile marks as a tracked input, or `Caller`. A query cannot write. Error: `query-row-not-tracked`. (agent)
3. r[remote.query.key] A query's memo key is its content identity, the canonical encoding of its arguments, the caller's principal when `Caller` is in its row, and the revisions of the reads it made. (agent)
4. r[remote.query.reads] The reads are the tracked host calls the run made. The runtime records them at the host boundary, where the Replay Rules already intercept calls. (agent)
5. r[remote.query.invalidate] When a tracked input reports a change that a memo entry read, the entry is stale. A subscriber gets the re-run result. (agent)

The language contributes three facts: determinism between host calls
(`req.determinism.depends`), the read set from host-boundary
interception, and a content identity for the key. The store's change
feed, the memo table, and the subscription API (`watch` in Program 3) are
runtime and library work.

**Durability.** A `@remote` mutation is an entry point. When the host
records the run, as the Replay Rules allow, the history holds the call
identity, the arguments, and the host events. After a crash the run
replays (#18), and an in-flight host call follows the in-flight policy:
an idempotent method re-runs with a stable key, and any other returns
`outcome-unknown` to the program. A transport that retries attaches a
request ID, so the home returns the recorded result instead of running
twice.

**Wire.** A call is its call identity plus its arguments; a result is a
boundary value. The transport encoding stays an implementation detail
([`module.host.boundary-abi`](../../spec/lang/10-modules.md#r-module.host.boundary-abi),
issue #51). Values that are hashed or sealed use one canonical encoding:
map entries in key order, floats by their bits
([`module.boundary.float-bits`](../../spec/lang/10-modules.md#r-module.boundary.float-bits)).

### C.2 Shape 1: Remote Functions

Shape 1 is the core alone: `@remote` and `@query` functions, `remote!`
calls, data in and data out. No value carries a home's state through an
untrusted place. It is tRPC, Telefunc, gRPC, React Router loaders, and
React Server Functions without `.bind`.

**What it cannot express.** State captured at render time and expected
back from an untrusted place, as Program 2's `seen`. The client sends it
as a plain argument, so the client can forge it. The fix in Shape 1 is to
keep the state at home under an ID and look it up, which needs a session
store the user writes.

### C.3 Shape 2: Remote Functions With Sealed Values

Shape 2 adds one std type. A **sealed value** is data a home hands to an
untrusted place and gets back unchanged. The holder can neither read nor
forge it. This is Next.js's closure encryption, made a type, and the
signed and encrypted tokens of Rails' `MessageEncryptor` and Django's
signing module.

```hd
pub data Draft:
    pub title: string
    pub seen: Sealed[i64]

@remote
pub fn publish!(id: string, seen: Sealed[i64]) -> Result[void, PublishError] $ Docs + Caller + Seal:
    ...
```

1. r[remote.sealed] `std.remote.Sealed[T < Boundary]` is a boundary-safe data type. `Boundary` is a sealed marker trait that a type implements exactly when `module.boundary.allowed` admits it, as `AnyVal` is implemented structurally (`module.prelude.anyval-values`). No NonEscapable type implements it. (agent)
2. r[remote.sealed.make] `seal[T < Boundary](value: T) -> Sealed[T] $ Seal` makes one. `Seal` is a host capability trait that holds the home's key; an untrusted place's profile has none. (agent)
3. r[remote.sealed.open] `s.open() -> T $ Seal` reads it. (agent)
4. r[remote.sealed.encoding] A sealed value encodes as authenticated encryption (an AEAD) of `T`'s canonical encoding, under the home's key, with a key identifier. (agent)
5. r[remote.sealed.verify] Decoding a sealed value at the home's registered boundary verifies it. A value that fails is a boundary failure, and the function never runs. Boundary failure: `sealed-invalid`. (agent)
6. r[remote.sealed.caller] `seal` may bind the value to the current caller's principal, so another user cannot replay it. (agent; [Q3](#q3-sealed-form))

Sealing is how state travels through an untrusted place under G3 and G5.
A React closure such as `addToCart.bind(null, productId)` becomes a named
`@remote` function plus a sealed argument. It may instead become a
portable closure whose captures are sealed ([C.10](#c10-portable-closures)).
Either way the reference names known code, and the captured value is data
only its home can read.

**What it adds.** No server state: a sealed value lives in the page, the
form's hidden field, or the client's storage, and survives a restart
while the key is kept. Keys and their rotation are deployment
configuration, as grants are. Trusted edges need no sealing.

### C.4 Shape 3: Remote Functions With Capability References

Shape 3 adds Cap'n Proto's model. A remote function may return a
reference to an object that stays at its home. Calls on the reference go
back to that object, and pipelining sends a call on a result before the
result arrives.

```hd
pub trait Session:
    fn draft!(self) -> Draft
    fn publish!(self) -> Result[void, PublishError]

@remote
pub fn open_session!(id: string) -> Cap[dyn Session] $ Docs + Caller:
    ...
```

1. r[remote.cap] `std.remote.Cap[dyn K]` is a boundary-safe reference to a `dyn K` value kept in the home's capability table. (agent)
2. r[remote.cap.call] A call on a capability is a remote call to its home. (agent)
3. r[remote.cap.lifetime] The home keeps the object while a peer holds the reference, or until the connection ends. A call on a dropped reference returns `RemoteError.Unreachable`, as Cap'n Proto's "disconnected". (agent)
4. r[remote.cap.reach] The object must reach `static`, so a `^` parameter's handle cannot be exported (`ne.call.global`). (agent)

**What it adds.** State stays at home, so nothing can be forged. Callbacks
from home to caller work, as Cloudflare's RPC stubs. Pipelining saves
round trips.

**What it costs.** State per connection, distributed reference counting,
references that die on a disconnect or a deploy unless the host
implements save and restore, and results that a cache cannot key (a
capability is a live object). A form submitted before JavaScript loads
has no connection to hold one.

### C.5 The Mechanism Each Shape Gives

The vision asks for a mechanism the programmer stops writing, as the
garbage collector and the goroutine scheduler were.

| | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- |
| Who decides where computation runs | The declaration's row picks the places that fit ([C.9](#c9-placement-by-requirements)); `remote!` and `$ Remote` show the hop at the call. | Same. | Same, and a capability names its object's home. |
| What moves data | Adapters generated from the signature, both sides: no endpoint, fetch, or decoder (G1, G2). | Same, plus sealing on untrusted edges. | Same, plus capability tables and release messages. |
| Who may see or call (W3) | The `@remote` table; the result type decides what leaves (`module.boundary.serialize.exact`). | Same, plus values only the home can read or make. | Same, plus object capabilities. |
| Fast: the cache (W4) | `@query` memo keyed by content identity, arguments, caller, and read revisions; invalidated from the read set. | Same; a sealed argument is part of the key. | Same for queries; capability results are not cacheable. |
| Persistent | Recorded mutations replay after a crash (#18). | Same, and sealed values persist without server state. | Capabilities are lost on restart unless saved. |
| Reliable | `RemoteError`; cancellation through `race!`; request IDs against double runs; call identity survives body changes. | Same. | Same, plus disconnect handling. |
| Left to libraries | Transports per edge, Origin and CSRF checks on untrusted edges, service authentication, subscription delivery, the store's change feed. | Same, plus key management. | Same, plus distributed reference counting and save/restore. |
| Left to the user | Authorization checks; a session store for state captured at render. | Authorization checks. | Authorization checks; reconnect logic. |

### C.6 Not A Shape: Shipping Code

Unison, Spark, Ray, Erlang, and Java ship closures. hd does not, in
either tier:

- **G3.** Code from the wire runs code the data chose. Java's gadget
  chains, and Spark's run-time "Task not serializable", show the cost.
- **G5.** A code bundle is far larger than an identity, and the receiver
  must compile it.
- **Neither tier needs it.** A named `@remote` function sends a call
  identity plus arguments. A portable closure
  ([C.10](#c10-portable-closures)) sends the content identity of a body
  the receiver already holds, plus encoded captures.

A receiver that lacks the code answers `Incompatible`; it never fetches
code. This is Cloud Haskell's `static` and Spores3's portable spores, not
Unison's code sync. A running page that needs new logic, or a GPU kernel
built at run time, stays out of scope.

### C.7 How The Shapes Use NonEscapable

| Concern | Rule that decides | Effect |
| --- | --- | --- |
| A handle as an argument, result, or sealed value | Boundary safety excludes live handles; `ne.call.global` rejects NonEscapable values by category | Rejected, even a fresh handle that reaches `static`. |
| A provider | `fn.capture.no-providers`; a `dyn K` is not boundary-safe | Never travels; keys stay in the row and the home binds them. |
| A `^` parameter's ordinary data | Encoding copies it | Fine: the encoded value carries no reach. |
| A capability (Shape 3) | `remote.cap.reach` | The object must reach `static` at its home. |
| A portable closure's captures | `portable.capture.boundary` | Boundary-safe captures copy, so they reach `static`. A handle is rejected by category, unlike an ordinary closure over a fresh handle (`ne.param.fn`). |

No shape adds a capture analysis beside the lifetime design. Copying ends
a reach, and the category rejects handles.

### C.8 The Ten Questions, Answered For hd

| Question | Answer (Shape 2, with both tiers) |
| --- | --- |
| 1. Opt-in | `@remote` or `@query` on a module-level function; or a closure literal marked with `portable(...)`. |
| 2. Code identity | Call identity for routing named functions; content identity (per-function SHA-256) for caches, histories, and portable closures. |
| 3. Destination | Any place whose profile binds the row and whose table holds the identity. Portable closures promise only the same build. Code is never fetched. |
| 4. Captures | Arguments and captures are boundary-safe data. State that comes back through an untrusted place is sealed. Handles, providers, and mutable state are rejected. |
| 5. Authority | The row is bound at the receiving place; placement checks it against the profile. The caller needs only `$ Remote` or the sending provider's key. |
| 6. Suspension | A remote function may be `fn!`. No stack travels; durable runs replay. |
| 7. Results and failure | Results are boundary-safe; transport failures are `RemoteError`, separate from domain errors; a remote panic does not poison the caller. |
| 8. Versioning | A body change keeps a call identity; a signature change gives `Incompatible`. A portable closure from another build may give `Incompatible`. |
| 9. Incremental | `@query` memo keyed by content identity, arguments, caller, and read revisions. |
| 10. Wire | An identity plus data. A canonical encoding for sealed values, captures, and cache keys; the transport encoding is free. |

### C.9 Placement By Requirements

On 2026-10-16 the owner asked whether requirement rows solve placement
([G.1](#g1-discussion-log)). A server reaches a database and a browser
cannot; a GPU reaches its device; a worker has limited resources. Rows
answer the main question with no new mechanism: a function fits a place
when its row is within the place's profile.

| Place | Profile binds (illustrative) | Fits | Does not fit |
| --- | --- | --- | --- |
| Server | `UserRead`, `Docs`, `MessageWrite`, `Seal`, `Caller`, `Clock`, `Remote` | `my_profile` (`$ UserRead + Caller`), `publish` | A function that needs a browser DOM key |
| Browser | `Remote`, `Console`, `Clock` | `greeting` (`$ Remote`), `too_short` (no row) | `my_profile`: no `UserRead` |
| Worker pool | `Clock` | The scoring closure of Program 6 (no row) | A closure with `Caller` or `Docs` in its row |
| GPU | Its device key only (separate study) | The kernel of Program 8 (no row), if the kernel subset admits it | Anything with a host key |

1. r[place.def] A place is an execution context that runs a build of the program under a runtime profile. (agent)
2. r[place.fit] A `@remote` function or a portable closure fits a place when the place's profile binds every key of its row. This is `module.register.row`, applied per place. (agent)
3. r[place.declared] When the build declares its places and their profiles, a `@remote` function that fits no declared place is an error. Error: `remote-no-place`. (agent; [Q7](#q7-where-places-are-declared))
4. r[place.send] A declared place may name the provider traits that send work to it. A `Portable` argument to such a provider is checked against the place's profile at the call. Error: `portable-row-not-bound`. (agent; [Q7](#q7-where-places-are-declared))
5. r[place.runtime] Without declared places, a place checks its table at registration (`module.register.failure`) and each portable value when it arrives (`RemoteError.Rejected`). (agent)
6. r[place.edge.trust] Each edge has a trust: trusted, service, or untrusted. `Sealed` values and Origin and CSRF checks apply on untrusted edges; a service edge binds `Caller.service()`. (agent; [Q11](#q11-default-trust-of-an-edge))

A sketch of declared places, if the owner picks the manifest
([Q7](#q7-where-places-are-declared)):

```toml
# hypothetical syntax: hd.toml
[places.server]
profile = ["UserRead", "Docs", "MessageWrite", "Seal", "Caller", "Clock", "Remote"]
trust = "untrusted"            # browsers call it

[places.workers]
profile = ["Clock"]
trust = "trusted"
sends = ["Workers"]            # Workers.map! sends portable closures here
```

Rows answer one question. The others need something else, and this design
does not stretch rows to cover them.

| Question | Do rows answer it? | What answers it |
| --- | --- | --- |
| Can this function run at that place? | Yes: row within profile. | `place.fit`, which is `module.register.row`. |
| Which resources inside a key, such as which tables or hosts? | No. | Grants per place, as `hd.toml`'s `[capabilities]` scopes do today (`cli.cap.table`). |
| Which place, when several fit? | No. | Deployment configuration, or a provider that names its place ([Q5](#q5-routing)). |
| Which code can a GPU run? | No. | The separate GPU study. |
| Can the caller be trusted? | No. | The edge's trust: `Caller`, `Sealed`, Origin checks (`place.edge.trust`). |
| How does bulk data move? | No. | The edge's data path: messages, shared memory, device buffers. |

### C.10 Portable Closures

The owner asked whether real closures should travel, and for what
([G.1](#g1-discussion-log)). Programs 6, 7, and 8 are the use cases: work
that captures request data and runs at a place of the same build. A named
`@remote` function per lambda would be ceremony. So closures form a second
tier beside named `@remote` functions.

| Tier | Names | Identity | Lives across deploys | Use |
| --- | --- | --- | --- | --- |
| Named `@remote` | A module-level function | Call identity | Yes, while the signature holds | Public endpoints, services, durable work |
| Portable closure | A closure literal marked `portable(...)` | Content identity of the lifted body, plus captures | No: same build only (owner) | Fan-out, push-down, actions handed through a page |

Program 2, written with a portable closure: the page carries the action,
and the browser runs it at home without seeing its captures.

```hd
use std.remote.{Caller, Portable, Remote, portable, remote}

pub data Page:
    pub title: string
    pub publish: Portable[fn!() -> Result[void, PublishError] $ Docs + Caller]

@remote
pub fn open_page!(id: string) -> Page $ Docs:
    docs := $.use(Docs)
    seen := docs.version!(id)
    action := portable(fn!() -> Result[void, PublishError] $ Docs + Caller: publish_if!(id, seen))
    Page { title: docs.title!(id), publish: action }

# Browser code: the captures `id` and `seen` arrive sealed.
fn on_publish!(page: Page) -> string $ Remote:
    match remote!(page.publish):
        .Ok(.Ok()) => "published"
        .Ok(.Err(.Changed)) => "someone edited it; reload"
        .Ok(.Err(.Forbidden)) => "not allowed"
        .Err(.Incompatible) => "the site was updated; reload"
        .Err(_) => "offline; try again"
```

`publish_if!` is a plain function, not `@remote`. It is reachable only
through a closure the home made, with the captures the home chose.

1. r[portable.literal] `std.remote.portable` is a compiler intrinsic whose one argument is a closure literal. The result has type `Portable[F]`, where `F` is the closure's function type. Error: `portable-not-literal`. (agent; spelling: [Q8](#q8-portable-closure-spelling))
2. r[portable.type] `Portable[F]` is a boundary-safe data type for each function type `F`, and implements `Boundary`. `F` stays an ordinary `fn(...)` type, so function types stay uniform (lifetime design D5). (agent)
3. r[portable.call] `p.call(a1, ..., an)` runs a portable value locally and needs `F`'s row. No call operator is added. (agent)
4. r[portable.remote] `remote!(p, a1, ..., an)` runs a portable value at the place that sealed it, or at the routed place on a trusted edge. It needs `$ Remote`, and has type `Result[O, RemoteError]`. (agent)
5. r[portable.capture.boundary] Every captured value must be boundary-safe, so captures copy and reach `static`. A NonEscapable capture is rejected by category, even a fresh handle. Error: `portable-capture-not-boundary-safe`. (agent)
6. r[portable.capture.provider] A portable closure captures no provider (`fn.capture.no-providers`); keys go in its row, and the receiving place binds them. (2026-09-27 decision)
7. r[portable.capture.mutable] A portable closure that assigns to or mutates a captured binding is rejected. Error: `portable-capture-mutable`. (2026-09-27 decision)
8. r[portable.generic] A `portable` literal whose type or captures mention a type parameter of an enclosing function is rejected, as `remote.decl.generic` rejects generic functions. Error: `portable-generic`. (agent)
9. r[portable.identity] A portable value encodes as the content identity of its lifted body plus the canonical encoding of its captures. The lifted body is a module-level function whose leading parameters are the captures. (agent; content hash: 2026-09-27)
10. r[portable.table] Every `portable` literal in the build is on the build's portable table under its identity. A place runs a portable value only when its table holds the identity; otherwise the call fails with `RemoteError.Incompatible`. (agent)
11. r[portable.same-build] A portable value is promised to run only at places running the build that made it. (owner, 2026-10-16)
12. r[portable.seal] A portable value sent over an untrusted edge is sealed whole, identity and captures, with the identity as the purpose label. The sending place's profile must bind `Seal`. (agent)
13. r[portable.untrusted] A place accepts a portable value from an untrusted edge only when it carries the place's own seal. Boundary failure: `portable-unsealed`. (agent; [Q9](#q9-closures-minted-by-an-untrusted-place))

**Why a table and a seal.** Without `portable.untrusted`, every `portable`
literal in the build is an endpoint with caller-chosen captures, which
breaks G3. With it, an untrusted place can only return closures a trusted
place made. Trusted edges skip the seal: a worker of the same operator
runs whatever its own server sends.

**Why same build.** A content identity outlives a deploy only when the body
and everything it calls are unchanged. The owner chose not to promise
that. Durable histories and cross-deploy calls use named `@remote`
functions, whose call identities survive body changes.

## D. Stress Test

### D.1 Verdicts

A = works, as the goals want. R = rejected with the right error. L = works
with a stated limit. H = hole: a goal is not met, or behavior is wrong.

Cases P29 to P39 test the new places and the portable tier. They assume
the tier of [C.10](#c10-portable-closures) beside each shape.

| # | Case | From | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- | --- | --- |
| P1 | Add to cart with a hidden field | React `<form>` | A | A | A |
| P2 | Add to cart with `.bind` | React `<form>` | A | A | A |
| P3 | `useActionState` with a permalink | React `"use server"` | A | A | A |
| P4 | A snapshot captured at render | Next.js closures | H | A | L |
| P5 | Delete a post the caller does not own | Next.js data security | A | A | A |
| P6 | Calling an unexported function | Next.js dead-code elimination | R | R | R |
| P7 | A typed query with input validation | tRPC | A | A | A |
| P8 | A function that reads the user | Telefunc `getContext` | A | A | A |
| P9 | A loader revalidated after an action | React Router | A | A | A |
| P10 | A reactive query | Convex | A | A | A |
| P11 | A resource built from mappers | Skip | L | L | L |
| P12 | Promise pipelining | Cap'n Proto | L | L | A |
| P13 | A deadline | gRPC | A | A | A |
| P14 | A mutation resumed after a crash | Temporal, #18 | A | A | A |
| P15 | A reference kept across a restart | Cap'n Proto persistent capabilities | H | A | L |
| P16 | A client from the previous deploy | Next.js version skew, Erlang `badfun` | A | A | A |
| P17 | A tampered captured value | Next.js closures | H | R | A |
| P18 | Capturing a handle | Ray (a lock) | R | R | L |
| P19 | Capturing a provider | Cloudflare bindings | R | R | H |
| P20 | Capturing mutable state | Spark counter | R | R | A |
| P21 | A generic remote function | Cloud Haskell `static id` | R | R | R |
| P22 | A callback from server to client | Cloudflare RPC stubs | R | R | A |
| P23 | A home missing a requirement | hd registration | R | R | R |
| P24 | Two builds with one content identity | Erlang `Uniq` | R | R | R |
| P25 | A mutation cancelled in flight | Temporal, gRPC | A | A | A |
| P26 | A result that leaks a field | Next.js "constrain return values" | R | R | R |
| P27 | One rule in browser and server | React Server Components | L | L | L |
| P28 | A query that reads module state | Skip's pure mappers | H | H | H |
| P29 | A service call with a deadline and retries | gRPC, Dapr | A | A | A |
| P30 | A service call after a signature change | Orleans versions, protobuf | L | L | L |
| P31 | A worker fan-out | Ray tasks | A | A | A |
| P32 | A worker fan-out cancelled midway | `ray.cancel` | A | A | A |
| P33 | A predicate pushed down to a store | LINQ | L | L | L |
| P34 | A pushed-down predicate that captures a provider | Cloudflare bindings | R | R | R |
| P35 | A closure whose identity a later build lacks | Cloud Haskell keys, Next.js | L | L | L |
| P36 | A closure capturing a handle | Ray (a lock) | R | R | R |
| P37 | A closure returned by a browser with forged captures | Next.js closures | L | R | L |
| P38 | A closure minted by a browser | React `.bind` | R | R | R |
| P39 | A closure sent to a worker that lacks its row | Ray resources | R | R | R |

| Tally | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- |
| Works (A) | 15 | 17 | 19 |
| Rejected with the right error (R) | 13 | 15 | 9 |
| Works with a limit (L) | 7 | 6 | 9 |
| Holes (H) | 4 | 1 | 2 |

Shape 3 counts more A because it accepts by design what the 2026-09-27
decision rejects for captures: shared mutable state and callbacks
(P20, P22). Its costs are not in the tally: per-connection state, no
caching of capabilities, and loss on restart.

**P1 to P28 under the reframe.** No verdict changes. P23 moves from
startup to build time when the build declares its places
([C.9](#c9-placement-by-requirements)). P16 is the browser case of
independent deploys; P30 is the service case. Shape 3's P19 hole now also
reaches service edges, where a capability could hand one service's
provider to another.

### D.2 Cases

The cases use the declarations of [A.4](#a4-motivating-programs). A
trait or error type that a case names without declaring, such as `Todos`
or `TodoError`, is an ordinary declaration left out for brevity.

```hd
pub enum CartError:
    NotSignedIn
```

#### P1. Add to cart with a hidden field

React's `<form>` reference, ported. A Server Function receives the hidden
field's `productId` from `FormData`.

```js
// React
async function addToCart(formData) {
  'use server'
  const productId = formData.get('productId')
  await updateCart(productId)
}
// <form action={addToCart}><input type="hidden" name="productId" value={productId} />...
```

```hd
pub data CartForm:
    pub product: string

pub trait Cart:
    fn add!(self, user: string, product: string) -> void

@remote
pub fn add_to_cart!(form: CartForm) -> Result[void, CartError] $ Cart + Caller:
    match $.use(Caller).user():
        .Some(user) => .Ok($.use(Cart).add!(user, form.product))
        .None => .Err(.NotSignedIn)
```

The form posts `CartForm`'s fields; a transport that serves HTML forms
decodes them by the declared type, before or without JavaScript. Any
product may go in a cart, so a client-chosen `product` is fine. A in every
shape.

#### P2. Add to cart with `.bind`

React's alternative: `addToCart.bind(null, productId)`. The bound value is
an ordinary argument that the client sends back.

```hd
@remote
pub fn add_product!(product: string) -> Result[void, CartError] $ Cart + Caller:
    match $.use(Caller).user():
        .Some(user) => .Ok($.use(Cart).add!(user, product))
        .None => .Err(.NotSignedIn)

fn on_click!(product: string) -> Result[Result[void, CartError], RemoteError] $ Remote:
    remote!(add_product, product)
```

A in every shape. React does not protect bound arguments either, so the
port loses nothing.

#### P3. `useActionState` with a permalink

React's `"use server"` reference: `requestUsername` returns a state, and
`useActionState(requestUsername, null, 'n/a')` shows it; a permalink keeps
the form working before JavaScript loads.

```hd
pub data UsernameForm:
    pub username: string

pub trait Names:
    fn can_request!(self, name: string) -> bool
    fn reserve!(self, name: string) -> void

@remote
pub fn request_username!(previous: string?, form: UsernameForm) -> string $ Names:
    names := $.use(Names)
    if names.can_request!(form.username):
        names.reserve!(form.username)
        return "successful"
    "failed"
```

The previous state is the first argument, as React passes it. The UI
hook and the permalink belong to the UI library and transport. A in every
shape.

#### P4. A snapshot captured at render

Next.js's closure example: `publish` captures `publishVersion` and checks
it against the latest version. Program 2 ports it.

Shape 1 sends `seen: i64` as a plain argument. A client can send the
current version and defeat the check: H. Shape 2 is Program 2: the client
cannot change `seen`: A. Shape 3 keeps the version in a session object:
it works while the connection lives, and fails for a form submitted
before JavaScript loads: L.

#### P5. Delete a post the caller does not own

Next.js's insecure direct object reference example: `deletePost(postId)`
must check that the caller owns the post.

```hd
pub enum DeleteError:
    NotSignedIn
    Forbidden

pub trait Posts:
    fn author!(self, post: string) -> string?
    fn delete!(self, post: string) -> void

@remote
pub fn delete_post!(post: string) -> Result[void, DeleteError] $ Posts + Caller:
    user := match $.use(Caller).user():
        .Some(found) => found
        .None => return .Err(.NotSignedIn)
    if $.use(Posts).author!(post) != .Some(user):
        return .Err(.Forbidden)
    $.use(Posts).delete!(post)
    .Ok()
```

A in every shape; the check is code, as every system requires. The row
shows `Caller`, so a reviewer sees that the function reads the caller.

#### P6. Calling an unexported function

Next.js strips unused actions so they have no endpoint. In hd only marked
functions are on the table.

```hd
fn wipe_all!() -> void $ Posts:
    ...

fn try_it!() -> Result[void, RemoteError] $ Remote:
    remote!(wipe_all)               # error: remote-not-exposed
```

A hand-made request that names `wipe_all` finds no table entry and gets
`Incompatible`. R in every shape. Unlike Next.js, an unused `@remote`
function stays exposed: the mark, not use, is the program saying so.

#### P7. A typed query with input validation

tRPC: `api.users.getById({ id: 1 })`, with an input validator.

```hd
pub data UserId:
    pub value: i64

pub data User:
    pub name: string

@query
pub fn user_by_id!(id: UserId) -> User? $ UserDirectory:
    $.use(UserDirectory).find!(id)
```

The declared type is the validator: a payload that does not decode as
`UserId` never reaches the body. A in every shape.

#### P8. A function that reads the user

Telefunc: `onNewTodo(text)` reads `getContext().user`.

```hd
@remote
pub fn new_todo!(text: string) -> Result[void, TodoError] $ Todos + Caller:
    match $.use(Caller).user():
        .Some(user) => $.use(Todos).add!(user, text)
        .None => .Err(.NotSignedIn)
```

`Caller` is in the row, where Telefunc hides it in an ambient context. A
in every shape.

#### P9. A loader revalidated after an action

React Router: a product loader; after any action, "all loader data on the
page is revalidated".

```hd
@query
pub fn product!(id: string) -> Product? $ CatalogRead:
    $.use(CatalogRead).product!(id)

@remote
pub fn rename_product!(id: string, name: string) -> void $ CatalogWrite:
    $.use(CatalogWrite).rename!(id, name)
```

hd invalidates only memo entries whose reads the write touched, not the
whole page. A in every shape, given the query rules of
[C.1](#c1-the-shared-core).

#### P10. A reactive query

Convex: a subscribed query re-runs when its data changes, and every
subscriber sees one snapshot. Program 3 ports it. A in every shape. hd
does not promise Convex's single snapshot across subscribers; that is the
store's job.

#### P11. A resource built from mappers

Skip: an output collection is built by mappers over input collections, and
an input change re-runs only the mappers for the changed keys.

```hd
@query
pub fn unread_count!(user: string) -> usize $ MessageRead:
    ...
```

hd's unit of reuse is one query call with its arguments, not one entry
of a collection. A query that calls other queries reuses their memo
entries, but a change inside a large result re-runs the whole query. L in
every shape: coarser than Skip.

#### P12. Promise pipelining

Cap'n Proto: call a method on a result before it arrives, in one round
trip.

Shapes 1 and 2 need two round trips, or one combined `@remote` function
that does both steps at home: L. Shape 3 pipelines: A.

#### P13. A deadline

gRPC: a call past its deadline fails with `DEADLINE_EXCEEDED`; either side
may cancel.

```hd
use std.task.race
use std.time.{Clock, Duration, sleep}

fn bounded!(id: UserId) -> User? $ Remote + Clock:
    race!(fetch_user(id), give_up())

fn fetch_user!(id: UserId) -> User? $ Remote:
    match remote!(user_by_id, id):
        .Ok(found) => found
        .Err(_) => .None

fn give_up!() -> User? $ Clock:
    sleep!(Duration::seconds(2))
    .None
```

When the timer wins, `race!` cancels the remote child, and the provider
aborts the call. A in every shape.

#### P14. A mutation resumed after a crash

Temporal replays a workflow from its history. Program 4's `fulfil!` runs
under recording; the server dies after the charge. Replay serves the
recorded charge result and runs the mailer live. A in every shape.

#### P15. A reference kept across a restart

Cap'n Proto can save some capabilities as persistent tokens, where the
host implements it. A draft page left open overnight must still publish
the right version in the morning.

Shape 1 has nothing protected to keep: H. Shape 2's sealed `seen` is
data in the page and stays valid while the key is kept: A. Shape 3's
session object is gone after the restart unless the host saves it: L.

#### P16. A client from the previous deploy

Next.js: a new deploy makes old action IDs fail with "Failed to find
Server Action". Erlang: a fun from another module version fails with
`badfun`.

A body change keeps the call identity, so a browser tab from yesterday
still calls `post!` after today's fix: A in every shape. A signature
change gives `RemoteError.Incompatible`, which the UI can turn into a
reload.

#### P17. A tampered captured value

A client edits Program 2's `seen` before posting it back.

Shape 1 has no captured value to protect: H. Shape 2 fails the
verification before `publish!` runs, with the boundary failure
`sealed-invalid`: R. Shape 3 never sent the value: A.

#### P18. Capturing a handle

Ray cannot pickle a lock: "copying a lock is meaningless". In hd a
connection is NonEscapable.

```hd
@remote
pub fn open_conn!() -> Conn $ Net:               # error: remote-signature-not-boundary-safe
    ...

fn keep!() -> Sealed[Conn] $ Net + Seal:
    seal(connect!("db"))                          # error: unsatisfied-trait-bound: Conn is not Boundary
```

R in Shapes 1 and 2, even for a fresh connection that reaches `static`.
Shape 3 may export `Cap[dyn Conn]` for a home-held connection that reaches
`static`, and rejects one that reaches a `^` parameter (`lifetime-mismatch`):
L, since the connection's life is now a network life.

#### P19. Capturing a provider

Cloudflare bindings belong to each Worker and never travel.

```hd
@remote
pub fn clock!() -> dyn Clock $ Clock:           # error: remote-signature-not-boundary-safe
    $.use(Clock)
```

R in Shapes 1 and 2. Shape 3 could return `Cap[dyn Clock]` and hand a
host capability to every client: H, unless a rule forbids capabilities
over provider values.

#### P20. Capturing mutable state

Spark's `counter` stays 0 because executors get copies.

```hd
@remote
pub fn bump!(counts: mut List[i64]) -> void:    # error: remote-signature-not-boundary-safe
    counts.push(1)
```

R in Shapes 1 and 2. Shape 3 exports a counter object whose state stays at
home and is changed only by calls: A, by its design.

#### P21. A generic remote function

GHC rejects `static id`: no `Typeable` for `a -> a`.

```hd
@remote
pub fn first![T](items: List[T]) -> T?:          # error: remote-generic
    ...
```

R in every shape. A home would have to compile every instantiation a
client might send.

#### P22. A callback from server to client

Cloudflare RPC turns a sent function into a stub that calls back to its
own Worker.

```hd
@remote
pub fn subscribe!(on_event: fn(string) -> void) -> void:   # error: remote-signature-not-boundary-safe
    ...
```

R in Shapes 1 and 2; Program 3's `watch` covers the common case by
pushing query results. Shape 3 passes `Cap[dyn Listener]`: A. A
`Portable[fn(string) -> void]` parameter would pass the signature check,
but it runs at the home, not back at the caller, so it is no callback.

#### P23. A home missing a requirement

A worker serves `post!`, whose row has `MessageWrite`, but the worker's
profile cannot bind `MessageWrite`. Registration fails at startup, before
any call (`module.register.failure`). With declared places, the build
fails instead (`remote-no-place`). R in every shape.

#### P24. Two builds with one content identity

Erlang guards funs with an MD5 of the module. hd's risk is an incomplete
hash input, not a SHA-256 collision.

```hd
@query
pub fn total!(orders: List[i64]) -> i64:
    let sum: i64 = 0
    for amount in orders:
        sum = sum + amount        # panics on overflow when checked, wraps in release
    sum
```

The build profile and the compiler semantic version are in the content
identity, so a checked build and a release build never share memo
entries or histories. R in every shape.

#### P25. A mutation cancelled in flight

The user closes the tab while `fulfil!` charges. The home finishes or
cancels by its own rules; the caller is gone. A retry with the same
request ID returns the recorded result, and a call whose result never
arrived reports `RemoteError.OutcomeUnknown`. A in every shape.

#### P26. A result that leaks a field

Next.js: "Action returns are serialized to the client. Shape them to what
the UI renders, not raw database records."

```hd
pub data UserRow:
    pub name: string
    password_hash: string     # private: crosses only through Serialize

@remote
pub fn me!() -> UserRow $ UserRead + Caller:   # error: boundary-private-field
    ...
```

A data type with a private field crosses only through its `Serialize`
(`module.boundary.out`), which decides what leaves; without one, the
signature is rejected. The result type is the exposure, and a reviewer
reads it in the signature. R in every shape.

#### P27. One rule in browser and server

React Server Components send output, not code. The signup check of the
first draft becomes a shared function and data.

```hd
pub data Rules:
    pub min_password: usize

pub fn too_short(rules: Rules, password: string) -> bool:
    password.len() < rules.min_password

@query
pub fn signup_rules!() -> Rules $ Settings:
    $.use(Settings).rules!()
```

The browser calls `too_short` locally on rules it fetched; the server
calls it again before saving. No code moves. L in every shape: the
determinism promise is per compiled program
([`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program)),
so two programs may disagree where map iteration order matters
([`types.map.order.deterministic`](../../spec/lang/04-type-system.md#r-types.map.order.deterministic)).
The server's answer is the one that counts.

#### P28. A query that reads module state

Skip requires mappers to be "deterministic and side-effect free".

```hd
let HITS: mut List[string] = []

@query
pub fn popular!() -> List[string]:
    HITS                       # not a tracked input: the memo never sees it change
```

A memo entry keeps the old list after `HITS` changes. Catching it needs
effect tracking across bodies, which hd avoids. H in every shape;
[F.2](#f2-settled-from-the-goals) settles a lint.

#### P29. A service call with a deadline and retries

gRPC propagates deadlines and retries by policy; Dapr retries through
resiliency policies. Program 5 is the port. Checkout calls `reserve!`
under a 300 ms deadline and reuses one idempotency key. It retries when
the call never started or its outcome is unknown.

The inventory service sees `Caller.service()` as the checkout service,
and `user()` as none. A retry with the same key reserves once. A in every
shape: no sealed value or capability is involved.

#### P30. A service call after a signature change

Inventory adds a field to `Reservation` and deploys first. Checkout still
runs the old build.

The fingerprint covers type layouts, so the old call's identity matches
no table entry: `RemoteError.Incompatible`, which Program 5 maps to
`Misconfigured`. Inventory must keep the old `reserve!` beside a new
`reserve_v2!` until every caller moves, as Orleans versions grain
interfaces. Protobuf's field rules would let the old call through. L in
every shape; [Q10](#q10-signature-evolution-across-independent-deploys).

#### P31. A worker fan-out

Ray runs `@ray.remote` tasks across a cluster. Program 6 is the port: the
closure captures `weights`, and its row is empty.

`Workers.map!` sends the closure's identity and its encoded captures to
each worker of the same build. Each worker finds the identity in its
portable table and runs it on its share of the orders. The edge is
trusted, so nothing is sealed. A in every shape.

#### P32. A worker fan-out cancelled midway

The request that started Program 6 is cancelled after half the orders
are scored. `ray.cancel` is Ray's counterpart.

```hd
use std.task.race
use std.time.{Clock, Duration, sleep}

fn rank_or_give_up!(orders: List[Order], weights: Weights) -> Result[List[f64], RemoteError] $ Workers + Clock:
    race!(rank(orders, weights), too_slow())

fn too_slow!() -> Result[List[f64], RemoteError] $ Clock:
    sleep!(Duration::seconds(5))
    .Err(.OutcomeUnknown)
```

`race!` cancels `rank!`, and the `Workers` provider aborts the items still
running. Finished scores are dropped. The closure captured only copies,
so no worker holds state to undo. A in every shape.

#### P33. A predicate pushed down to a store

LINQ translates a lambda's expression tree into SQL. Program 7 sends a
portable closure instead.

The store runs the closure beside its rows and returns only matches. This
works only for a store that is a place running the same build. A SQL
database is not one, and hd does not translate closures into SQL. L in
every shape.

#### P34. A pushed-down predicate that captures a provider

The report compares totals in dollars, through a currency-rate provider.

```hd
pub trait Rates:
    fn usd(self, amount: i64) -> i64

fn big_in_usd!(min: i64) -> List[Order] $ OrderStore + Rates:
    rates := $.use(Rates)
    $.use(OrderStore).where!(portable(fn(o: Order) -> bool: rates.usd(o.total) > min))   # error: portable-capture-not-boundary-safe
```

`rates` is a `dyn Rates`, which is not boundary-safe, and a closure takes
no provider (`fn.capture.no-providers`). The fix puts `Rates` in the
closure's row; the store must then bind `Rates` (`place.fit`). R in every
shape.

#### P35. A closure whose identity a later build lacks

A browser tab holds the sealed `publish` closure of
[C.10](#c10-portable-closures) overnight. A deploy changes
`publish_if!`.

The closure's content identity is no longer on the server's portable
table, so `remote!(page.publish)` returns `RemoteError.Incompatible`, and
the page offers a reload. Next.js gives "Failed to find Server Action" in
the same spot. L in every shape, as the owner decided: same build only.
Work that must survive a deploy takes a named `@remote` function.

#### P36. A closure capturing a handle

Ray cannot pickle a lock. A closure sent to a worker captures a fresh
connection.

```hd
fn warm!(urls: List[string]) -> Result[List[bool], RemoteError] $ Workers + Net:
    conn := connect!("cache")
    $.use(Workers).map!(urls, portable(fn(u: string) -> bool: conn.has(u)))   # error: portable-capture-not-boundary-safe
```

The connection is NonEscapable, so the category rejects it, although it
reaches `static`. An ordinary closure over the same fresh handle is fine
under `ne.param.fn`; the portable tier adds the boundary-safe bound. R in
every shape.

#### P37. A closure returned by a browser with forged captures

The browser edits the captured `seen` inside the `publish` closure of
[C.10](#c10-portable-closures), hoping to defeat the version check.

Shape 2 sealed the whole closure when it left the server. The AEAD check
fails on return, with `sealed-invalid`, and `publish_if!` never runs: R.
Shapes 1 and 3 have no `Seal`, so `portable.seal` forbids sending the
closure to an untrusted place at all: L. Shape 3 can keep the action at
home behind a capability instead.

#### P38. A closure minted by a browser

Shared code in the browser builds
`portable(fn!() -> void $ Posts: wipe_all!())` and sends it to the server.
React lets a client bind any argument it likes.

The closure is on the build's portable table, but it arrives unsealed
over an untrusted edge. The server refuses it with the boundary failure
`portable-unsealed`, and nothing runs. Without that rule, every portable
literal would be a public endpoint. R in every shape;
[Q9](#q9-closures-minted-by-an-untrusted-place).

#### P39. A closure sent to a worker that lacks its row

A closure that reads the caller goes to the worker pool.

```hd
fn tag_all!(orders: List[Order]) -> Result[List[string], RemoteError] $ Workers:
    $.use(Workers).map!(orders, portable(fn!(o: Order) -> string $ Caller: tag(o, $.use(Caller).user())))   # error: portable-row-not-bound
```

The worker profile binds `Clock` only. With declared places, the call
fails to compile (`place.send`); without them, the worker refuses the
value with `RemoteError.Rejected`. The fix reads the caller first and
captures the result. R in every shape.

### D.3 Holes And Limits Found

| Finding | Cases | Shapes | Fix |
| --- | --- | --- | --- |
| Captured server state is forgeable or lost | P4, P15, P17 | 1 | Shape 2's sealed values. |
| A capability may export a provider | P19 | 3 | A rule forbidding `Cap` over provider values, if Shape 3 is taken. |
| Queries may read untracked module state | P28 | all | A lint on `@query` bodies that read top-level `let` bindings. |
| Reuse is per query call, not per entry | P11 | all | Accepted limit; finer reuse is runtime work. |
| No pipelining | P12 | 1, 2 | Accepted limit; a combined `@remote` function. |
| Determinism is promised per program | P27 | all | Accepted limit; the server rechecks. |
| A signature change breaks callers in another deploy | P30 | all | Keep the old function beside the new one, or field evolution rules ([Q10](#q10-signature-evolution-across-independent-deploys)). |
| Push-down needs a store that runs the build | P33 | all | Accepted limit; translation to SQL is out of scope. |
| A closure from before a deploy fails | P35 | all | Accepted by the owner (same build); named `@remote` for durable work. |
| An untrusted place could mint closures | P38 | all | `portable.untrusted` ([Q9](#q9-closures-minted-by-an-untrusted-place)). |
| Closures cannot pass through an untrusted place | P37 | 1, 3 | Shape 2's sealing. |

### D.4 Holes Found In Review

The orchestrator's review of this study found four more. Each is in every
shape that has the feature.

| Finding | Effect | Proposed fix |
| --- | --- | --- |
| **A sealed value can be reused for another purpose.** `open_draft` seals a document version as `Sealed[i64]`. Any other `@remote` function that takes a `Sealed[i64]`, such as a discount, accepts it, because the cipher checks only the key. | A client turns a value the server minted for one meaning into another. | Bind the purpose in the AEAD associated data: the full type identity of `T` and a label fixed at the `seal` site, checked on decode (`sealed-invalid` on mismatch). Next.js binds captures to their action ID for the same reason. Guide: seal a named data type (`Sealed[DocVersion]`), never a bare scalar. |
| **A sealed value never expires.** | A token minted before a permission was revoked keeps working while the key lives. | An optional expiry inside the sealed payload, checked on decode; key rotation stays the hard stop. |
| **A dependency's `@remote` functions are exposed when its module is included** (`remote.decl.table`). | Adding a library silently adds public endpoints. This breaks G3: the program did not say so. | The serving registration names the modules whose remote functions it serves; other modules' marks are inert. See [Q6](#q6-which-modules-a-home-serves). |
| **A tracked input could write.** `remote.query.row` admits any trait the profile marks as tracked. | A query with a write method in its row breaks memoization. | A tracked input trait has no `mut` method and no method whose effect the Replay Rules classify as a write. |

Two costs are also missing from [E](#e-cost-model). The memo table is
keyed by the caller's principal when `Caller` is in the row, so it grows
with users times arguments and needs a runtime eviction policy. The Origin
and CSRF checks left to libraries in [C.5](#c5-the-mechanism-each-shape-gives)
must be on in the default transport for untrusted edges, or "secure by
default" (G3) does not hold.

## E. Cost Model

The numbers are estimates for planning, not measurements.

| Cost | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- |
| Code size, home | One adapter per `@remote` function: decode arguments, bind the row, encode the result. About the size of a registered boundary adapter, est. 0.3 to 2 KB of Wasm each. A table row of call identity and function reference, under 64 bytes. | Shape 1, plus a seal and open per `Sealed[T]` type, est. 0.2 to 1 KB. The cipher is in the host's `Seal` provider, not in Wasm. | Shape 1, plus a capability table, reference counting, and a stub per capability trait. |
| Code size, client | One stub per called function: encode arguments, decode the result, est. 0.2 to 1 KB. No server code ships. | Same; sealed values are opaque bytes to the client. | Same, plus capability stubs and release messages. |
| Hashing at build | Call identities hash signatures only: microseconds per function. Content identities only for `@query` functions, recorded entries, and `portable` literals, over their callees. A 100,000-line program has est. 10 to 20 MB of normalized form; SHA-256 at 0.5 to 1.5 GB/s is 10 to 40 ms, and only the reachable part is hashed. | Same. | Same. |
| Wire size | A call identity (a 16-byte hash prefix, or the name) plus canonical arguments. Program 1's call is under 40 bytes. | Plus, per sealed value, about 30 bytes of cipher overhead (a 12-byte nonce, a 16-byte tag, a key identifier) on top of its plaintext. Program 2's `seen` is about 40 bytes. | Plus capability IDs (4 to 8 bytes each) and release messages. |
| Server state | None between calls. | None between calls; one key per deployment. | Per connection: every exported object, until released or disconnected. |
| Startup | None: stateless calls. | None. | A connection and a bootstrap capability. |
| Cache memory | Per memo entry: a 32-byte key, the result, and the read set's revision tokens. | Same. | Same for queries; capabilities are not cached. |
| Incremental compile | A body edit changes no call identity, so clients need no rebuild. It changes the content identity of every `@query` above it, which invalidates their memo entries; recomputing costs one hash per changed function and ancestor, with early cutoff when the normalized form is unchanged. | Same. | Same. |

The portable tier costs the same in every shape:

| Cost | Portable closures |
| --- | --- |
| Code size | One adapter per `portable` literal, as for a `@remote` function: decode the captures, bind the row, run the lifted body. A portable table row per literal, under 64 bytes. |
| Wire size | A 16-byte identity prefix plus the canonical captures. Program 6's closure is about 40 bytes: two `f64` weights plus the identity. On an untrusted edge, add about 30 bytes of seal. |
| Fan-out | A provider such as `Workers` can send the closure once per worker and the items in batches, so the closure's bytes do not grow with the item count. This is library work. |
| Build | A changed body changes its literal's identity, which only matters across builds, and those are not promised. |

**Headline.** Shapes 1 and 2 cost one adapter and one stub per remote
function, no server state, and a wire of a short identity plus data.
Shape 2 adds about 30 bytes per sealed value and a key per deployment.
Shape 3's real cost is per-connection state and references that do not
survive restarts; that cost grows with users, not with code. Portable
closures cost one adapter per literal and tens of bytes per send.

## F. Findings, Settled Points, And Owner Questions

### F.1 Findings

1. A place is an execution context with a runtime profile. Places differ
   in what they bind, what code they run, and how far each edge trusts
   its caller. Browser and server are one pair among services, workers,
   stores, and GPUs.
2. Placement needs no new mechanism. A function fits a place when its row
   is within the place's profile, which is `module.register.row` applied
   per place. Grants, the choice among fitting places, the GPU subset,
   trust, and bulk data stay outside rows.
3. Security belongs to edges. Sealed values and Origin and CSRF checks
   guard untrusted edges; `Caller` names a user or a calling service; a
   worker edge needs neither.
4. A serializable closure decomposes into a reference to known code plus
   boundary-safe data, in both tiers. Named `@remote` functions carry a
   call identity that survives body changes. Portable closures carry a
   content identity, promised only within one build.
5. hd's existing parts do most of the work. Rows say what a place must
   bind and keep the transport out of user code. Boundary-safe types and
   the NonEscapable category keep handles and providers home. Determinism
   and host-call interception give a cache its key and read set.
6. Stress test over 39 cases: Shape 1 has 4 holes, Shape 2 has 1, Shape 3
   has 2. Shape 2's one hole, untracked module state in a query, is in
   every shape. The new cases add limits, not holes.
7. Against the vision, the programmer stops writing the endpoint, the
   fetch, the decoder, the hand-rolled token, the job payload, and the
   invalidation code. Transports, service authentication, the store's
   change feed, subscription delivery, and key management stay in
   libraries and the runtime.

### F.2 Settled From The Goals

These follow from the goals and the standing decisions. The owner may
overturn any of them.

| Point | Settled as | Reason |
| --- | --- | --- |
| Ship code? | No. Both tiers send an identity plus data; code fetching stays out. | G3 and G5 ([C.6](#c6-not-a-shape-shipping-code)). |
| Exposure | Only `@remote` and `@query` functions are on the table, used or not. Portable closures run only from a trusted edge or under the receiver's own seal. | G3: "nothing is exposed unless the program says so". |
| Arguments | Untrusted, decoded by declared types; authorization in code with `$ Caller`. | G3; every compared system does the same. |
| Routing identity | Module path, name, and signature fingerprint. | Reliability: body fixes do not break deployed clients (P16). |
| Cache and history identity | Per-function content hash, with the build profile and compiler semantic version. | The 2026-09-27 decision; P24. |
| Generic remote functions and closures | Not allowed. | A home cannot compile unknown instantiations (P21). |
| A bound for "boundary safe" | A sealed `Boundary` marker, implemented structurally as `AnyVal` is, excluding NonEscapable types. | `Sealed[T]`, `Portable[F]`, and transport libraries are generic and need it (P18, P36). |
| Transport errors | `Result[O, RemoteError]`, apart from domain errors. | Reliability: a caller must tell "declined" from "unreachable". |
| Recording | Opt-in per run, as the Replay Rules decide; transports dedup by request ID. | Already decided. |
| Wire | An identity plus data; canonical encoding only for sealed values, captures, and keys. | G5; #51 keeps the transport encoding free. |
| Module state in queries | A lint on `@query` bodies that read top-level `let` bindings, reading only that body. | P28; purity is documented, not checked (2026-09-27). |
| Placement | Row within profile, reusing `module.register.row` per place. | The owner's 2026-10-16 question; no new mechanism. |
| Function types | `Portable[F]` holds an ordinary `fn(...)` type; no second kind of function type. | Lifetime design D5. |

### F.3 Owner Questions

Core first. The owner's 2026-10-16 answers settle two points (GPU subset,
same build). They need no question here.

#### Q1. Shape

Options: (1) remote functions; (2) remote functions with sealed values;
(3) remote functions with capability references; (2) now and (3) later.

**Recommendation: (2).** It meets all five goals with no server state,
and its one hole is shared by every shape. Shape 1 cannot protect
captured state (P4, P15, P17), and cannot hand a portable closure through
a browser (P37). Shape 3's per-connection state, loss on restart, and
uncacheable results work against "persistent" and G4; if pipelining or
callbacks become needed, it can be added beside (2).

#### Q2. Call spelling

Options: (a) `remote!(f, args)`, an intrinsic whose type and row show the
hop; (b) a plain call `f!(args)` that the compiler turns into a stub in a
program that cannot bind `f`'s row, as Next.js and Telefunc do.

**Recommendation: (a).** (b) gives one name two types depending on which
program is built, which is action at a distance. (a) keeps the transport
out of user code (G1) and still tells the reader where the call runs
(W1, W2). The same intrinsic runs a portable value (`portable.remote`).

#### Q3. Sealed form

Effect: a sealed value bound to its caller cannot be replayed by another
user, but a shared link then stops working. Should `seal` bind a value to
the caller's principal by default? Options: (a) off by default, the
caller chooses; (b) on by default.

**Recommendation: (a).** Caller binding stops replay by another user but
breaks shared links, so the caller should choose it. The first draft's
`bind(f, captures)` option is dropped: portable closures, sealed on
untrusted edges, are the closure-shaped form ([C.10](#c10-portable-closures)).

#### Q4. Where the cache lives

Options: (a) the language defines `@query`, its row rule, and its memo
key, and the runtime records reads and invalidates; (b) all of it is a
library over plain remote functions.

**Recommendation: (a).** A library cannot see host-call reads or compute
a content identity. Those are compiler and runtime facts, and they make
the cache correct by construction. The subscription API and the store's
change feed stay library work.

#### Q5. Routing

Effect: when several places fit a function, something must pick one.
Options:

- (a) one `Remote` key, with deployment configuration mapping modules to
  places;
- (b) one key per place, so a row names the place, as in
  `$ Remote[Billing]`;
- (c) a provider that names its place, as `Workers` names its pool.

**Recommendation: (a), with (c) for providers that send portable
closures.** Rows stay short, and routing is deployment configuration, as
grants are. (b) tells the reader more about where data lives (W1), so it
is the candidate if programs with many places become common.

#### Q6. Which modules a home serves

Options: (a) every `@remote` function in every included module, as
`remote.decl.table` says now; (b) only the modules the serving
registration names; (c) only the serving package's own modules, with a
dependency's functions served by an explicit re-export.

**Recommendation: (b).** Exposure is then one list the reader can audit,
and adding a dependency never adds an endpoint. (c) is close, but a
re-export is a second meaning for `pub use`. Added by the orchestrator's
review ([D.4](#d4-holes-found-in-review)).

#### Q7. Where places are declared

Effect: a build that knows its places rejects mistakes before deploy. A
function that fits no place fails, and so does a closure sent to a pool
that lacks its row (`place.declared`, `place.send`). Options:

- (a) `[places]` tables in `hd.toml`, beside `[capabilities]`;
- (b) place declarations in hd source;
- (c) host configuration only, checked at run time.

**Recommendation: (a).** `hd.toml` already holds what a program always
needs, reviewed in a diff (`cli.cap.table`). (c) keeps every check at run
time, so P23 and P39 fail only in production. The sketch is in
[C.9](#c9-placement-by-requirements).

#### Q8. Portable closure spelling

Effect: the reader must see which closures may leave their place.
Options: (a) an intrinsic call around the literal, `portable(fn ...)`;
(b) a new keyword or decorator on the literal.

```hd
job := portable(fn(o: Order) -> f64: score(o, weights))
```

**Recommendation: (a).** It adds no syntax, matches `remote!`, and gets
its typing rule as `all!` does (`req.combinator.all-typing`).

#### Q9. Closures minted by an untrusted place

Effect: if a server runs closures a browser built, every `portable`
literal in the build becomes a public endpoint with browser-chosen
captures (P38). Options:

- (a) a place accepts a portable value from an untrusted edge only under
  its own seal;
- (b) accept any value on the table, as React accepts bound arguments.

**Recommendation: (a).** It keeps G3 without a list of exposed closures.
A browser that needs to start server work calls a named `@remote`
function.

#### Q10. Signature evolution across independent deploys

Effect: when a service adds a field to a parameter type, callers on the
old build get `Incompatible` until they redeploy (P30). Options:

- (a) the fingerprint stays exact, and a service keeps the old function
  beside the new one;
- (b) protobuf-style rules, where adding a defaulted field keeps the call
  identity.

```hd
@remote
pub fn reserve_v2!(r: ReservationV2) -> Result[void, ReserveError] $ Stock + Caller:
    $.use(Stock).reserve_v2!(r)
```

**Recommendation: (a) now.** It needs no compatibility rules and fails
loudly. (b) is the proven model for microservices, so it is the candidate
once services built in hd are common. This waits on Q1 and Q5.

#### Q11. Default trust of an edge

Effect: an edge treated as trusted skips sealing and Origin checks, so a
wrong default exposes a server. Options: (a) an edge with no declared
trust is untrusted; (b) edges inside one deployment are trusted unless
marked.

**Recommendation: (a).** It is "secure by default" (G3). A worker pool
says `trust = "trusted"` once, in the place declaration
([C.9](#c9-placement-by-requirements)).

#### Q12. Service principal spelling

Effect: code that checks for a signed-in user must not admit a calling
service by mistake. Options: (a) `Caller.service()` beside `user()`;
(b) one `Caller.principal()` returning an enum of user and service.

```hd
match $.use(Caller).service():
    .Some(name) => audit(name)
    .None => reject()
```

**Recommendation: (a).** Existing code that checks `user()` sees none on
a service edge, so it refuses services until it opts in. (b) reads well
but makes every existing check handle a new case.

## G. Recommended Design And Discussion Log

Status: the orchestrator's recommended design, revised 2026-10-16 for
heterogeneous places after the owner's feedback. The owner has not yet
answered the [F.3](#f3-owner-questions) questions; this section is the
design those recommendations add up to. G.10 and G.11 hold the parts the
revision added.

In short: declare the places and mark what is reachable. Call it with
`remote!`, and send closures as `portable(...)` within one build. Seal
what must come back through an untrusted place, and mark cacheable reads
`@query`. The language handles the endpoint, the encoding, the placement
check, the authority boundary, and the cache keys.

### G.1 Discussion Log

- 2026-10-15, owner: "let's first focus on being correct"; serializable
  closures are one of two big designs to settle, with lifetimes (#163).
- Owner's goals: (1) protocol-transparent RPC; (2) an easier way to fetch
  data; (3) secure, not exposing arbitrary data or procedures publicly;
  (4) ideally a cache, like Skip; (5) minimal: think about what to send.
- Owner: add React Server Actions to the study; "the big idea is that the
  language should help you manage data". The vision quote in
  [A.1](#a1-the-big-idea).
- Study committed (this file, sections A to F); the orchestrator's review
  added [D.4](#d4-holes-found-in-review) and Q6.
- 2026-10-16, owner asked for the whole design before the questions; the
  orchestrator answered with G.2 to G.9.
- 2026-10-16, owner feedback, verbatim:
  1. "it looks like too focusing on browswer/server. it is a big use
     case, but we should not limit to it. consider something like
     microservice calling, or running computation on gpu, it is truly
     heterogeneous computation"
  2. "it looks like you only supports top-level func as @remote target?
     is it desirable to support truly closure? if so, what's the use
     case?"
- 2026-10-16, owner answers: GPU kernel subset: "separate study"; this
  design only keeps the place model open to it. Portable closures:
  "same-build only"; an identity a later build lacks fails
  `Incompatible`, and durable or cross-deploy work uses named `@remote`.
- 2026-10-16, the orchestrator's plan, which the owner answered within.
  Places have runtime profiles, placement matches rows to profiles, and
  each edge has its own trust and data path. It also listed the four new
  programs, the prior art of
  [B.4](#b4-places-placement-and-closure-identity), and the two tiers.
- 2026-10-16, owner, verbatim: "for serializable closure, also think
  about this: a server function can access database, but a browser cannot
  / a gpu function can access gpu / a worker may have limited access to
  resources / i think requirement list can solve this?"
- 2026-10-16, the orchestrator's answer: yes, placement is row within
  profile. Rows do not answer grants, the choice among fitting places,
  the GPU subset, trust, or bulk data. A compile-time check fits when the
  build declares its places. This revision writes that answer up as
  [C.9](#c9-placement-by-requirements), with the open parts in Q7 to Q12.

### G.2 Expose A Function: `@remote`

```hd
@remote
pub fn post!(room: string, text: string) -> Result[void, ChatError] $ MessageWrite + Caller:
    user := match $.use(Caller).user():
        .Some(u) => u
        .None => return .Err(.NotSignedIn)
    if text.len() > 2000:
        return .Err(.TooLong)
    $.use(MessageWrite).append!(room, Message { author: user, text: text })
    .Ok()
```

- Only marked functions are reachable; an unmarked function never is, even
  a public one.
- Every parameter and the result are boundary-safe data. Handles,
  providers, closures and mutable values are rejected by type;
  NonEscapable values by category.
- No generic remote functions: a home cannot compile unknown
  instantiations.
- Arguments from an untrusted edge are untrusted and decoded by their
  declared types only. Authorization is ordinary code over `$ Caller`.

### G.3 Call It: `remote!`

```hd
fn send!(room: string, text: string) -> string $ Remote:
    match remote!(post, room, text):
        .Ok(.Ok()) => "sent"
        .Ok(.Err(.TooLong)) => "too long"
        .Ok(.Err(.NotSignedIn)) => "please sign in"
        .Err(e) => "network: ${e}"
```

- The caller needs only `$ Remote`; the receiving place binds the
  function's own row. No authority travels.
- The transport is the `Remote` provider, chosen per edge: HTTP,
  WebSocket, a worker message, service RPC, in-process for tests. User
  code never names it.
- The call is explicit, so its type and row show the hop: the protocol is
  hidden, the network is not.
- `RemoteError` is kept apart from domain errors: `Unreachable`,
  `OutcomeUnknown`, `Incompatible`, `Rejected`, `Panicked`. Cancelling the
  caller cancels the call, and the transport carries the cancel to the
  home.

### G.4 State The Client Cannot Forge: `Sealed[T]`

```hd
pub data DocVersion:
    pub value: i64

@query
pub fn open_draft!(id: string) -> Draft $ Docs + Seal:
    docs := $.use(Docs)
    Draft { title: docs.title!(id), seen: seal(DocVersion { value: docs.version!(id) }) }

@remote
pub fn publish!(id: string, seen: Sealed[DocVersion]) -> Result[void, PublishError] $ Docs + Caller + Seal:
    docs := $.use(Docs)
    if seen.open().value != docs.version!(id):
        return .Err(.Changed)
    docs.publish!(id)
    .Ok()
```

- Sealing guards untrusted edges only. An untrusted holder carries a
  sealed value but can neither read nor alter it. It is authenticated
  encryption under a key held by the home's `Seal` provider.
- The type identity and a purpose label are bound into the encryption, so
  a `Sealed[DocVersion]` cannot pass where a `Sealed[Discount]` is wanted
  ([D.4](#d4-holes-found-in-review)).
- Optional expiry in the payload; optional binding to the caller's
  principal (off by default, since it breaks shared links).
- No server state: a sealed value survives a restart while the key lasts.
  React `.bind` and Next.js closures port to a named function plus a
  sealed argument, or to a sealed portable closure (G.11).

### G.5 Fetch, Cached And Live: `@query`

```hd
@query
pub fn recent!(room: string) -> List[Message] $ MessageRead:
    $.use(MessageRead).latest!(room, 50)
```

- A query's row holds only tracked, read-only input traits and `Caller`,
  so it cannot write.
- Memo key: the function's content identity, the canonical arguments, the
  caller when `Caller` is in the row, and the revisions of the reads it
  made. The runtime records reads at the host boundary and invalidates
  entries whose reads change; subscribers (a library `watch`) get new
  results. This is Skip's and Convex's model; the language supplies
  determinism, the read set and the code hash.
- An eviction policy bounds the memo table; a lint flags queries that read
  top-level mutable state.

### G.6 Two Identities

- Call identity (module path, name, signature fingerprint) routes a call
  to a named function and survives body changes. A deployed client or
  service keeps working after a bug fix; a signature change gives
  `Incompatible`.
- Content identity hashes the function, everything it calls, type
  layouts, and the compiler version. It keys caches, durable histories,
  and portable closures, so a body change invalidates exactly what it
  should.

### G.7 Serving And Security

- A home serves the modules its registration names; a dependency's
  `@remote` functions are never exposed by accident (Q6).
- Security attaches per edge. Untrusted edges get `Sealed`, `Caller` with
  a user, and Origin and CSRF checks in the default transport. Service
  edges get `Caller.service()` from an authenticated transport. Worker
  edges get none.
- An edge with no declared trust is untrusted (Q11).
- One `Remote` key; deployment configuration maps modules to places
  (Q5).

### G.8 Durability And Failure

- A recorded `@remote` mutation replays after a crash under the Replay
  Rules; no stack is serialized.
- Retries carry a request ID or an idempotency key, so the home returns
  the recorded result instead of running twice.
- A home panic reaches the caller as `RemoteError.Panicked` and never
  poisons it.
- Durable and cross-deploy work uses named `@remote` functions, never
  portable closures.

### G.9 Left Out On Purpose

- Shipping code, in either tier. A receiver that lacks an identity
  answers `Incompatible`.
- Which code may run on a GPU: the separate study. The place model and
  the closure tier leave room for it.
- Translating closures into another language, such as SQL.
- Server-side references (Shape 3): no pipelining and no callbacks from
  home to caller; they can be added beside this design if needed.

### G.10 Places And Placement

```hd
fn rank!(orders: List[Order], weights: Weights) -> Result[List[f64], RemoteError] $ Workers:
    $.use(Workers).map!(orders, portable(fn(o: Order) -> f64: score(o, weights)))
```

- A place is an execution context that runs a build under a runtime
  profile. Browsers, servers, worker pools, peer services, stores, and
  GPUs are places.
- A function or portable closure fits a place when its row is within the
  place's profile. This is `module.register.row`, per place; it is the
  answer to the owner's "requirement list" question.
- When the build declares its places, a `@remote` function that fits
  none is a build error. So is a closure sent to a pool that lacks its
  row. Otherwise the checks run at registration and on arrival (Q7).
- Rows do not decide grants, the choice among fitting places, the GPU
  subset, trust, or the bulk data path. Grants, routing, the GPU study,
  edge trust, and the edge's transport do.

### G.11 Portable Closures

```hd
action := portable(fn!() -> Result[void, PublishError] $ Docs + Caller: publish_if!(id, seen))
```

- A closure literal wrapped in `portable(...)` becomes a
  `Portable[fn(...)]` value: boundary-safe data that holds an ordinary
  function type (D5). It runs locally with `.call`, or at another place
  through `remote!` or a provider such as `Workers`.
- It encodes as the content identity of its lifted body plus its
  captures. Captures are boundary-safe and reach `static`; providers,
  handles, mutable captures, and generic literals are rejected.
- Same build only (owner): a place whose table lacks the identity answers
  `Incompatible`.
- On an untrusted edge the whole closure is sealed, and a place accepts
  only closures under its own seal (Q9). Trusted edges skip the seal.

## Parse Log

Every `hd` block was parsed with the compiler's parser (`parse_check`, the
`parse` action of `scripts/hd-conformance.sh`) on 2026-10-16. This checks
syntax only; no block was type-checked, and most use names the study
proposes. The `toml` and `js` blocks are not parsed.

| Result | Count | Blocks |
| --- | --- | --- |
| Parses | 39 | Programs 1 to 8; the C.1 `send!` call, `RemoteError`, and `Caller`; the C.10 page example; the D.2 `CartError` block; P1, P2, P3, P5, P7, P8, P9, P13, P19, P20, P24, P27, P28, P32, P34, P36, P39; Q8, Q10, Q12; G.2 to G.5, G.10, G.11. |
| Fails only at an elided body `...` | 9 | The C.1 `@remote` declaration, C.3, C.4, P6, P11, P18, P21, P22, P26. The `...` stands for omitted code and is not hd syntax. |

## Sources

- React, Server Functions: <https://react.dev/reference/rsc/server-functions>
- React, `'use server'`: <https://react.dev/reference/rsc/use-server>
- React, `<form>` (hidden fields, `bind`, progressive enhancement): <https://react.dev/reference/react-dom/components/form>
- React, `useActionState`: <https://react.dev/reference/react/useActionState>
- React, Server Components: <https://react.dev/reference/rsc/server-components>
- Next.js, Server Actions: <https://nextjs.org/docs/app/guides/server-actions>
- Next.js, Data Security (closures and encryption, IDs, allowed origins): <https://nextjs.org/docs/app/guides/data-security>
- tRPC, Concepts: <https://trpc.io/docs/concepts>
- Telefunc: <https://telefunc.com/>
- Telefunc, `shield()`: <https://telefunc.com/shield>
- React Router, Data Loading: <https://reactrouter.com/start/framework/data-loading>
- React Router, Actions: <https://reactrouter.com/start/framework/actions>
- Skip (SkipLabs), Overview: <https://skiplabs.github.io/skip/docs/introduction>
- Convex, Realtime: <https://docs.convex.dev/realtime>
- Convex, Queries: <https://docs.convex.dev/functions/query-functions>
- Cap'n Proto, RPC Protocol: <https://capnproto.org/rpc.html>
- gRPC, Core concepts: <https://grpc.io/docs/what-is-grpc/core-concepts/>
- gRPC, Deadlines: <https://grpc.io/docs/guides/deadlines/>
- gRPC, Retry: <https://grpc.io/docs/guides/retry/>
- Protocol Buffers, Updating a message type: <https://protobuf.dev/programming-guides/proto3/#updating>
- Unison, "The big idea": <https://www.unison-lang.org/docs/the-big-idea/>
- Unison, Distributed datasets with the `Remote` ability: <https://www.unison-lang.org/articles/distributed-datasets/>
- GHC User's Guide, Static pointers: <https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/static_pointers.html>
- `distributed-closure`: <https://hackage.haskell.org/package/distributed-closure>
- Temporal, Workflows: <https://docs.temporal.io/workflows>
- Temporal, Workflow Definition (versioning): <https://docs.temporal.io/workflow-definition>
- Temporal, Retry Policies: <https://docs.temporal.io/encyclopedia/retry-policies>
- Cloudflare Workers, RPC: <https://developers.cloudflare.com/workers/runtime-apis/rpc/>
- Erlang, External Term Format: <https://www.erlang.org/doc/apps/erts/erl_ext_dist.html>
- erlang-questions, `spawn(Node, Fun)` needs the same module: <https://erlang.org/pipermail/erlang-questions/2016-September/090050.html>
- Spark, RDD Programming Guide (closures): <https://spark.apache.org/docs/latest/rdd-programming-guide.html>
- Ray, Serialization: <https://docs.ray.io/en/latest/ray-core/objects/serialization.html>
- Ray, Tasks: <https://docs.ray.io/en/latest/ray-core/tasks.html>
- Ray, Resources (logical resources, `num_gpus`): <https://docs.ray.io/en/latest/ray-core/scheduling/resources.html>
- Ray, Task fault tolerance (`max_retries`): <https://docs.ray.io/en/latest/ray-core/fault_tolerance/tasks.html>
- Oracle Java Tutorials, Lambda Expressions (serialization): <https://docs.oracle.com/javase/tutorial/java/javaOO/lambdaexpressions.html>
- Weisenburger, Köhler, Salvaneschi, "Distributed System Development with ScalaLoci", OOPSLA 2018: <https://doi.org/10.1145/3276499>
- ScalaLoci: <https://scala-loci.github.io/>
- Hydro, documentation (locations, live collections): <https://hydro.run/docs/hydro/reference/locations/>
- Microsoft Orleans, Grain placement: <https://learn.microsoft.com/en-us/dotnet/orleans/grains/grain-placement>
- Microsoft Orleans, Grain interface versioning: <https://learn.microsoft.com/en-us/dotnet/orleans/grains/grain-versioning/grain-versioning>
- Bernstein et al., "Orleans: Distributed Virtual Actors for Programmability and Scalability", MSR-TR-2014-41: <https://www.microsoft.com/en-us/research/publication/orleans-distributed-virtual-actors-for-programmability-and-scalability/>
- Dapr, Service invocation overview (app IDs, mTLS, retries): <https://docs.dapr.io/developing-applications/building-blocks/service-invocation/service-invocation-overview/>
- Dapr, Access control for service invocation: <https://docs.dapr.io/operations/configuration/invoke-allowlist/>
- Scala Spores3: <https://github.com/phaller/spores3>
- Miller, Haller, Odersky, "Spores: A Type-Based Foundation for Closures in the Age of Concurrency and Distribution", ECOOP 2014: <https://doi.org/10.1007/978-3-662-44202-9_13>
- .NET, Expression trees (LINQ providers): <https://learn.microsoft.com/en-us/dotnet/csharp/advanced-topics/expression-trees/>
- GPU study pointer: Triton: <https://triton-lang.org/>
- GPU study pointer: Futhark: <https://futhark-lang.org/>
- GPU study pointer: WebGPU (W3C): <https://www.w3.org/TR/webgpu/>; WGSL: <https://www.w3.org/TR/WGSL/>
