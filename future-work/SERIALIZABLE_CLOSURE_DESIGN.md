# Serializable Closures And Remote Calls: Design Study

## Table Of Contents

- [Status](#status)
- [A. Problem](#a-problem)
- [B. How Other Systems Do It](#b-how-other-systems-do-it)
- [C. Candidate Shapes](#c-candidate-shapes)
- [D. Stress Test](#d-stress-test)
- [E. Cost Model](#e-cost-model)
- [F. Findings, Settled Points, And Owner Questions](#f-findings-settled-points-and-owner-questions)
- [Sources](#sources)

## Status

This is the design study for issue #166. It decides nothing, and the spec
is frozen. Every rule ID, diagnostic code, std name, and spelling below is
a placeholder written by the agent. No compiler checks the examples. A
line marked `# error:` gives the diagnostic a proposed checker would
report.

The owner framed the study on 2026-10-15: the vision quoted in
[A.1](#a1-the-big-idea), the five goals of [A.2](#a2-requirements), and
the systems to compare. These earlier owner decisions stand:

| Decision | Source |
| --- | --- |
| A serializable closure's code identity is a content hash. | [Open Issues](OPEN_ISSUES.md#serializable-closures-and-incremental-computation), 2026-09-27 |
| Captures must be boundary-safe values; this replaced a `Durable` bound. | same, citing Durable Replay decision 11 |
| Capturing a provider or mutable state is rejected. | same |
| Not in the new compiler's first release. | owner, 2026-10-06 |
| Durable workflows replay recorded host results; they never serialize a stack. Recording is opt-in per run. | Replay Rules (archived `RUNTIME_AND_LIBRARY.md`, in git history before commit `38560c7a`) |
| A handle never travels; authority is rebound by requirements at the destination. | issue #166, [NonEscapable design](NONESCAPE_LIFETIME_DESIGN.md) |

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
hand-written parts of moving data: the endpoint, the `fetch` call, the
decoder, the hand-rolled token, and the cache with its invalidation.

So this study judges each shape by four questions. How well does the
language itself tell the reader:

| | Question | Today in hd |
| --- | --- | --- |
| W1 | **Where data lives** and where computation runs? | Requirement rows say what a function needs; runtime profiles say what a host can bind. Nothing says "this runs on the server". |
| W2 | **How data moves** between places? | Registered boundaries move boundary-safe values; the adapter is generated from the signature. Calling across them is library work. |
| W3 | **Who may see or call** it? | Registration is explicit (`module.register.explicit`). Nothing protects data a server hands to a client and expects back. |
| W4 | **When a copy is stale**? | Nothing. A fetched value is a snapshot with no version. |

RPC, fetching, security, caching, and a minimal wire are the means to
answer these four questions.

### A.2 Requirements

The owner's five goals are the requirements. Every shape is judged
against them.

| Goal | Requirement |
| --- | --- |
| G1 | **Protocol-transparent RPC.** Calling remote code looks like a call. The transport (HTTP, WebSocket, worker message, in-process) is not in user code. |
| G2 | **Easy data fetching.** A client asks the server for data without hand-written endpoints, fetch code, or decoders. |
| G3 | **Secure by default.** Nothing is reachable unless the program says so. A client cannot call an unexported function, reach arbitrary data, or forge captured values. |
| G4 | **A cache, ideally like Skip.** Results are memoized, incremental, and invalidated when their inputs change. |
| G5 | **A minimal wire.** Send a reference to known code plus boundary-safe data, rather than code. |

Shipping arbitrary closures is in scope only where it serves these goals.
[C.6](#c6-not-a-shape-shipping-code) explains why no shape ships code.

### A.3 What The Language Already Gives

| Existing rule | What it gives a design |
| --- | --- |
| [`module.boundary.allowed`](../spec/lang/10-modules.md#r-module.boundary.allowed) | The value types that already cross: scalars, `string`, tuples, lists, maps, data, enums, optionals, results. |
| [`module.boundary.not-safe`](../spec/lang/10-modules.md#r-module.boundary.not-safe) | Mutable types, `dyn` values, closures, and live handles never cross. |
| [`module.boundary.rows`](../spec/lang/10-modules.md#r-module.boundary.rows) | Requirement rows are host bindings, never serialized. |
| [`module.register.row`](../spec/lang/10-modules.md#r-module.register.row) | A registered entry's row is checked against the host's profile before user code runs. |
| [`module.boundary.serialize.exact`](../spec/lang/10-modules.md#r-module.boundary.serialize.exact) | A type's `Serialize` decides exactly what leaves hd, so a result type is an exposure decision. |
| [`module.boundary.deserialize.no-dyn`](../spec/lang/10-modules.md#r-module.boundary.deserialize.no-dyn) | The target type comes from the signature, never from the data. |
| [`fn.capture.no-providers`](../spec/lang/07-functions.md#r-fn.capture.no-providers) | A closure takes no provider from a `$.with` around it; keys go in its row. |
| [`req.determinism.depends`](../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends) | Code between host calls is deterministic, so host results determine a run. A cache can be keyed by them. |
| [`cli.engine.same-behavior`](../spec/cli/command-line.md#r-cli.engine.same-behavior) | One compiled module behaves the same on every engine. |
| [`annot.facts-of.target`](../spec/lang/14-annotations.md#r-annot.facts-of.target) | An intrinsic may require its argument to name a module-level function. |
| [`req.combinator.all-typing`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.all-typing) | An intrinsic may have a written typing rule instead of a signature. |
| [`ne.call.global`](NONESCAPE_LIFETIME_DESIGN.md#44-call-sites) | A NonEscapable value is rejected by category, whatever its reach; a fresh handle reaches `static` and is still rejected. |

The lifetime design's D5 keeps function types uniform: one rule for every
function-typed parameter. No shape below adds a second kind of function
type.

### A.4 Motivating Programs

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

## B. How Other Systems Do It

Unison, Cloud Haskell, and Temporal stay because they inform code identity
and durability. Spark, Ray, and Java serializable lambdas are kept in one
row, as the cautionary case for shipping code.

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

## C. Candidate Shapes

The three shapes share a core ([C.1](#c1-the-shared-core)). Shape 2 adds
sealed values to it, and Shape 3 adds capability references.

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
5. r[remote.decl.home] A program **serves** its remote table when a library registration binds the table to a transport. Registration checks each function's row against the profile (`module.register.row`), so a missing key fails before any call (`module.register.failure`). (agent)

**Calling.** A call names the function and passes its arguments. The
transport is a requirement, so the caller's row says "this code talks to
another place" without naming HTTP or WebSocket.

```hd
fn send!(room: string, text: string) -> Result[Result[void, ChatError], RemoteError] $ Remote:
    remote!(post, room, text)
```

1. r[remote.call] `std.remote` declares the compiler intrinsic `remote`. A use must be a bang call whose first argument names a `@remote` function, directly or through a qualifier or `use` import, as `annot.facts-of.target` requires. Error: `remote-not-exposed`. (agent)
2. r[remote.call.typing] For `f: fn!(P1, ..., Pn) -> O $ R`, or the plain form, `remote!(f, a1, ..., an)` checks each `ai` against `Pi` and has type `Result[O, RemoteError]`. It needs `$ Remote`, not `R`: the home binds `R`. A written typing rule, as `all!` has. (agent)
3. r[remote.call.transport] `std.remote.Remote` is a host capability trait. Its provider decides the transport: HTTP, WebSocket, a worker message, or in-process for tests. (agent)
4. r[remote.call.cancel] Cancelling the calling suspension cancels the call through the provider's abort contract (`req.cancel.external-abort`). (agent)

```hd
pub enum RemoteError:           # illustrative
    Unreachable                 # the call never started
    OutcomeUnknown              # started; no result arrived
    Incompatible                # the home has no function with this call identity
    Rejected(message: string)   # the home refused the arguments
    Panicked(category: string)  # by spec diagnostic name, as the Replay Rules record panics
```

**The caller.** A function that needs to know who called it says so in
its row. The transport binds `Caller` per request.

```hd
pub trait Caller:
    fn user(self) -> string?     # the authenticated principal, or none
```

1. r[remote.caller] `std.remote.Caller` is a requirement trait whose provider the serving transport binds for each request, with the authenticated principal or none. (agent)
2. r[remote.caller.authz] Authorization is ordinary code in the function, as React, Next.js, and Telefunc require. The language guarantees exposure and typed arguments, not ownership checks. (agent)

**Arguments.** Every argument is untrusted. The adapter decodes it by the
declared parameter type (`module.boundary.deserialize.exact`), which gives
Telefunc's `shield()` without a second declaration.

**Two identities.** A call and a cache need different identities.

| Identity | Made of | Used for |
| --- | --- | --- |
| **Call identity** | Module path, function name, and a fingerprint of the signature's types: parameter, result, and row layouts | Routing a call. A body change keeps it, so a client built before a bug fix still calls the fixed server. |
| **Content identity** | SHA-256 over the function's normalized semantic form, the content identities of every item it references (recursive groups hashed as one unit, as Unison does), type layouts, the compiler semantic version, and the build profile; names excluded | Cache keys and durable histories, where a body change must invalidate. |

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
([`module.host.boundary-abi`](../spec/lang/10-modules.md#r-module.host.boundary-abi),
issue #51). Values that are hashed or sealed use one canonical encoding:
map entries in key order, floats by their bits
([`module.boundary.float-bits`](../spec/lang/10-modules.md#r-module.boundary.float-bits)).

### C.2 Shape 1: Remote Functions

Shape 1 is the core alone: `@remote` and `@query` functions, `remote!`
calls, data in and data out. No value carries server state to the client.
It is tRPC, Telefunc, gRPC, React Router loaders, and React Server
Functions without `.bind`.

**What it cannot express.** Server state captured at render time and
expected back, as Program 2's `seen`. The client sends it as a plain
argument, so the client can forge it. The fix in Shape 1 is to keep the
state on the server under an ID and look it up, which needs a session
store the user writes.

### C.3 Shape 2: Remote Functions With Sealed Values

Shape 2 adds one std type. A **sealed value** is data a home hands to a
client and gets back unchanged. The client holds it but can neither read
nor forge it. This is Next.js's closure encryption, made a type, and the
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
2. r[remote.sealed.make] `seal[T < Boundary](value: T) -> Sealed[T] $ Seal` makes one. `Seal` is a host capability trait that holds the home's key; a client profile has none. (agent)
3. r[remote.sealed.open] `s.open() -> T $ Seal` reads it. (agent)
4. r[remote.sealed.encoding] A sealed value encodes as authenticated encryption (an AEAD) of `T`'s canonical encoding, under the home's key, with a key identifier. (agent)
5. r[remote.sealed.verify] Decoding a sealed value at the home's registered boundary verifies it. A value that fails is a boundary failure, and the function never runs. Boundary failure: `sealed-invalid`. (agent)
6. r[remote.sealed.caller] `seal` may bind the value to the current caller's principal, so another user cannot replay it. (agent; [Q3](#q3-sealed-form))

Sealing is how a closure's captures travel under G3 and G5. A React
closure such as `addToCart.bind(null, productId)`, or Next.js's inline
`publish` over `publishVersion`, becomes a named `@remote` function plus
a sealed argument. The reference names known code; the captured value is
data that only its home can read.

**What it adds.** No server state: a sealed value lives in the page, the
form's hidden field, or the client's storage, and survives a server
restart while the key is kept. Keys and their rotation are deployment
configuration, as grants are.

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
from server to client work, as Cloudflare's RPC stubs. Pipelining saves
round trips.

**What it costs.** Server state per connection, distributed reference
counting, references that die on a disconnect or a deploy unless the host
implements save and restore, and results that a cache cannot key (a
capability is a live object). A form submitted before JavaScript loads
has no connection to hold one.

### C.5 The Mechanism Each Shape Gives

The vision asks for a mechanism the programmer stops writing, as the
garbage collector and the goroutine scheduler were.

| | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- |
| Who decides where computation runs | The declaration: `@remote` says "runs at its home"; `remote!` and `$ Remote` show the hop at the call. | Same. | Same, and a capability names its object's home. |
| What moves data | Adapters generated from the signature, both sides: no endpoint, fetch, or decoder (G1, G2). | Same, plus sealing. | Same, plus capability tables and release messages. |
| Who may see or call (W3) | The `@remote` table; the result type decides what leaves (`module.boundary.serialize.exact`). | Same, plus values only the home can read or make. | Same, plus object capabilities. |
| Fast: the cache (W4) | `@query` memo keyed by content identity, arguments, caller, and read revisions; invalidated from the read set. | Same; a sealed argument is part of the key. | Same for queries; capability results are not cacheable. |
| Persistent | Recorded mutations replay after a crash (#18). | Same, and sealed values persist without server state. | Capabilities are lost on restart unless saved. |
| Reliable | `RemoteError`; cancellation through `race!`; request IDs against double runs; call identity survives body changes. | Same. | Same, plus disconnect handling. |
| Left to libraries | Transports, Origin and CSRF checks, subscription delivery, the store's change feed. | Same, plus key management. | Same, plus distributed reference counting and save/restore. |
| Left to the user | Authorization checks; a session store for server state captured at render. | Authorization checks. | Authorization checks; reconnect logic. |

### C.6 Not A Shape: Shipping Code

Unison, Spark, Ray, Erlang, and Java ship closures. No shape here does:

- **G3.** Code from the wire runs code the data chose. Java's gadget
  chains, and Spark's run-time "Task not serializable", show the cost.
- **G5.** A code bundle is far larger than a call identity, and the
  receiver must compile it.
- **The goals do not need it.** Every motivating program and every stress
  case below is a reference to known code plus data. A browser that runs
  the server's validation rule runs the shared function on data the
  server sent ([P27](#p27-one-rule-in-browser-and-server)).

The cases that would need it, such as a server pushing new logic into a
running page, are parked. If they return, the smallest design is the
one Cloud Haskell and Spores3 use: a `Portable[F]` value that holds a
content identity of a marked module-level function plus the encoded
values of its leading parameters, run by a receiver whose table holds
that identity. It still ships no code.

### C.7 How The Shapes Use NonEscapable

| Concern | Rule that decides | Effect |
| --- | --- | --- |
| A handle as an argument, result, or sealed value | Boundary safety excludes live handles; `ne.call.global` rejects NonEscapable values by category | Rejected, even a fresh handle that reaches `static`. |
| A provider | `fn.capture.no-providers`; a `dyn K` is not boundary-safe | Never travels; keys stay in the row and the home binds them. |
| A `^` parameter's ordinary data | Encoding copies it | Fine: the encoded value carries no reach. |
| A capability (Shape 3) | `remote.cap.reach` | The object must reach `static` at its home. |

No shape adds a capture analysis beside the lifetime design. Copying ends
a reach, and the category rejects handles.

### C.8 The Ten Questions, Answered For hd

| Question | Answer (Shape 2) |
| --- | --- |
| 1. Opt-in | `@remote` or `@query` on a module-level function. No closure literal travels. |
| 2. Code identity | Call identity for routing; content identity (per-function SHA-256) for caches and histories. |
| 3. Destination | Any program whose remote table holds the call identity. Code is never fetched. |
| 4. Captures | Arguments are boundary-safe data. Server state captured for later comes back sealed. Handles, providers, and mutable state are rejected by type. |
| 5. Authority | The function's row is bound at its home; registration checks it against the profile. The caller needs only `$ Remote`. |
| 6. Suspension | A remote function may be `fn!`. No stack travels; durable runs replay. |
| 7. Results and failure | Results are boundary-safe; transport failures are `RemoteError`, separate from domain errors; a remote panic does not poison the caller. |
| 8. Versioning | A body change keeps the call identity; a signature change gives `Incompatible`. A content identity change invalidates caches only. |
| 9. Incremental | `@query` memo keyed by content identity, arguments, caller, and read revisions. |
| 10. Wire | Call identity plus arguments. A canonical encoding for sealed values and cache keys; the transport encoding is free. |

## D. Stress Test

### D.1 Verdicts

A = works, as the goals want. R = rejected with the right error. L = works
with a stated limit. H = hole: a goal is not met, or behavior is wrong.

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

| Tally | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- |
| Works (A) | 12 | 14 | 16 |
| Rejected with the right error (R) | 9 | 10 | 5 |
| Works with a limit (L) | 3 | 3 | 5 |
| Holes (H) | 4 | 1 | 2 |

Shape 3 counts more A because it accepts by design what the 2026-09-27
decision rejects for captures: shared mutable state and callbacks
(P20, P22). Its costs are not in the tally: per-connection server state,
no caching of capabilities, and loss on restart.

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
pushing query results. Shape 3 passes `Cap[dyn Listener]`: A.

#### P23. A home missing a requirement

A worker serves `post!`, whose row has `MessageWrite`, but the worker's
profile cannot bind `MessageWrite`. Registration fails at startup, before
any call (`module.register.failure`). R in every shape.

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
([`req.determinism.same-program`](../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program)),
so two programs may disagree where map iteration order matters
([`types.map.order.deterministic`](../spec/lang/04-type-system.md#r-types.map.order.deterministic)).
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

### D.3 Holes And Limits Found

| Finding | Cases | Shapes | Fix |
| --- | --- | --- | --- |
| Captured server state is forgeable or lost | P4, P15, P17 | 1 | Shape 2's sealed values. |
| A capability may export a provider | P19 | 3 | A rule forbidding `Cap` over provider values, if Shape 3 is taken. |
| Queries may read untracked module state | P28 | all | A lint on `@query` bodies that read top-level `let` bindings. |
| Reuse is per query call, not per entry | P11 | all | Accepted limit; finer reuse is runtime work. |
| No pipelining | P12 | 1, 2 | Accepted limit; a combined `@remote` function. |
| Determinism is promised per program | P27 | all | Accepted limit; the server rechecks. |

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
must be on in the default transport, or "secure by default" (G3) does not
hold.

## E. Cost Model

The numbers are estimates for planning, not measurements.

| Cost | Shape 1 | Shape 2 | Shape 3 |
| --- | --- | --- | --- |
| Code size, home | One adapter per `@remote` function: decode arguments, bind the row, encode the result. About the size of a registered boundary adapter, est. 0.3 to 2 KB of Wasm each. A table row of call identity and function reference, under 64 bytes. | Shape 1, plus a seal and open per `Sealed[T]` type, est. 0.2 to 1 KB. The cipher is in the host's `Seal` provider, not in Wasm. | Shape 1, plus a capability table, reference counting, and a stub per capability trait. |
| Code size, client | One stub per called function: encode arguments, decode the result, est. 0.2 to 1 KB. No server code ships. | Same; sealed values are opaque bytes to the client. | Same, plus capability stubs and release messages. |
| Hashing at build | Call identities hash signatures only: microseconds per function. Content identities only for `@query` functions and recorded entries, over their callees. A 100,000-line program has est. 10 to 20 MB of normalized form; SHA-256 at 0.5 to 1.5 GB/s is 10 to 40 ms, and only the reachable part is hashed. | Same. | Same. |
| Wire size | A call identity (a 16-byte hash prefix, or the name) plus canonical arguments. Program 1's call is under 40 bytes. | Plus, per sealed value, about 30 bytes of cipher overhead (a 12-byte nonce, a 16-byte tag, a key identifier) on top of its plaintext. Program 2's `seen` is about 40 bytes. | Plus capability IDs (4 to 8 bytes each) and release messages. |
| Server state | None between calls. | None between calls; one key per deployment. | Per connection: every exported object, until released or disconnected. |
| Startup | None: stateless calls. | None. | A connection and a bootstrap capability. |
| Cache memory | Per memo entry: a 32-byte key, the result, and the read set's revision tokens. | Same. | Same for queries; capabilities are not cached. |
| Incremental compile | A body edit changes no call identity, so clients need no rebuild. It changes the content identity of every `@query` above it, which invalidates their memo entries; recomputing costs one hash per changed function and ancestor, with early cutoff when the normalized form is unchanged. | Same. | Same. |

**Headline.** Shapes 1 and 2 cost one adapter and one stub per remote
function, no server state, and a wire of a short identity plus data.
Shape 2 adds about 30 bytes per sealed value and a key per deployment.
Shape 3's real cost is per-connection server state and references that do
not survive restarts; that cost grows with users, not with code.

## F. Findings, Settled Points, And Owner Questions

### F.1 Findings

1. Under the five goals, a serializable closure decomposes into a
   reference to known code and boundary-safe data. Every web system that
   meets G1 to G3 works that way. No shape needs to ship code.
2. The one part of a closure that needs new protection is server state
   captured for later. Shape 2 seals it: the client carries it but can
   neither read nor forge it, and the server keeps no state.
3. hd's existing parts do most of the work. Requirement rows say what a
   home must bind and keep the transport out of user code. Registration
   checks a home's row. Boundary-safe types and the NonEscapable category
   keep handles and providers home. The determinism clause and the Replay
   Rules' host-call interception give a cache its key and read set.
4. Two identities are needed. Routing uses a call identity that survives
   a body change, which fixes Next.js's broken clients after each deploy.
   Caches and histories use the content hash the owner decided on.
5. Stress test over 28 cases: Shape 1 has 4 holes, Shape 2 has 1, Shape 3
   has 2. Shape 2's one hole, untracked module state in a query, is in
   every shape.
6. Against the vision: the mechanism the programmer stops writing is the
   endpoint, the fetch, the decoder, the hand-rolled token, and the
   invalidation code. Transports, the store's change feed, subscription
   delivery, and key management stay in libraries and the runtime.

### F.2 Settled From The Goals

These follow from the goals and the standing decisions. The owner may
overturn any of them.

| Point | Settled as | Reason |
| --- | --- | --- |
| Ship code? | No. `Portable[F]` and code fetching are parked. | G3 and G5; no case needs it ([C.6](#c6-not-a-shape-shipping-code)). |
| Exposure | Only `@remote` and `@query` functions are on the table, used or not. | G3: "nothing is exposed unless the program says so". |
| Arguments | Untrusted, decoded by declared types; authorization in code with `$ Caller`. | G3; every compared system does the same. |
| Routing identity | Module path, name, and signature fingerprint. | Reliability: body fixes do not break deployed clients (P16). |
| Cache and history identity | Per-function content hash, with the build profile and compiler semantic version. | The 2026-09-27 decision; P24. |
| Generic remote functions | Not allowed. | A home cannot compile unknown instantiations (P21). |
| A bound for "boundary safe" | A sealed `Boundary` marker, implemented structurally as `AnyVal` is, excluding NonEscapable types. | `Sealed[T]` and transport libraries are generic and need it (P18). |
| Transport errors | `Result[O, RemoteError]`, apart from domain errors. | Reliability: a caller must tell "declined" from "unreachable". |
| Recording | Opt-in per run, as the Replay Rules decide; transports dedup by request ID. | Already decided. |
| Wire | Call identity plus arguments; canonical encoding only for sealed values and keys. | G5; #51 keeps the transport encoding free. |
| Module state in queries | A lint on `@query` bodies that read top-level `let` bindings, reading only that body. | P28; purity is documented, not checked (2026-09-27). |

### F.3 Owner Questions

Core first.

#### Q1. Shape

Options: (1) remote functions; (2) remote functions with sealed values;
(3) remote functions with capability references; (2) now and (3) later.

**Recommendation: (2).** It meets all five goals with no server state,
and its one hole is shared by every shape. Shape 1 cannot protect
captured state (P4, P15, P17). Shape 3's per-connection state, loss on
restart, and uncacheable results work against "persistent" and G4; if
pipelining or callbacks become needed, it can be added beside (2).

#### Q2. Call spelling

Options: (a) `remote!(f, args)`, an intrinsic whose type and row show the
hop; (b) a plain call `f!(args)` that the compiler turns into a stub in a
program that cannot bind `f`'s row, as Next.js and Telefunc do.

**Recommendation: (a).** (b) gives one name two types depending on which
program is built, which is action at a distance. (a) keeps the transport
out of user code (G1) and still tells the reader where the call runs
(W1, W2).

#### Q3. Sealed form

Options: (a) a `Sealed[T]` type passed as data; (b) a `bind(f, captures)`
reference, closure-shaped, as React's `.bind`; (c) both. Also: should
`seal` bind a value to the caller's principal by default?

**Recommendation: (a), with caller binding available but not default.**
The signature then says which inputs only the home may make, and a sealed
value composes with fetched data, forms, and storage. (b) is sugar that
can come later; it would need a rule that bound parameters cannot be sent
raw. Caller binding stops replay by another user but breaks shared links,
so the caller should choose it.

#### Q4. Where the cache lives

Options: (a) the language defines `@query`, its row rule, and its memo
key, and the runtime records reads and invalidates; (b) all of it is a
library over plain remote functions.

**Recommendation: (a).** A library cannot see host-call reads or compute
a content identity. Those are compiler and runtime facts, and they make
the cache correct by construction. The subscription API and the store's
change feed stay library work.

#### Q5. Routing

Options: (a) one `Remote` key, with the host configuration mapping
modules to homes; (b) one key per home, so a row says which home, as in
`$ Remote[Billing]`.

**Recommendation: (a) now.** It keeps rows short, and routing is
deployment configuration, as grants are. (b) tells the reader more about
where data lives (W1), so it is the candidate if programs with several
homes become common.

#### Q6. Which modules a home serves

Options: (a) every `@remote` function in every included module, as
`remote.decl.table` says now; (b) only the modules the serving
registration names; (c) only the serving package's own modules, with a
dependency's functions served by an explicit re-export.

**Recommendation: (b).** Exposure is then one list the reader can audit,
and adding a dependency never adds an endpoint. (c) is close, but a
re-export is a second meaning for `pub use`. Added by the orchestrator's
review ([D.4](#d4-holes-found-in-review)).

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
- Unison, "The big idea": <https://www.unison-lang.org/docs/the-big-idea/>
- GHC User's Guide, Static pointers: <https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/static_pointers.html>
- `distributed-closure`: <https://hackage.haskell.org/package/distributed-closure>
- Temporal, Workflows: <https://docs.temporal.io/workflows>
- Temporal, Workflow Definition (versioning): <https://docs.temporal.io/workflow-definition>
- Cloudflare Workers, RPC: <https://developers.cloudflare.com/workers/runtime-apis/rpc/>
- Erlang, External Term Format: <https://www.erlang.org/doc/apps/erts/erl_ext_dist.html>
- erlang-questions, `spawn(Node, Fun)` needs the same module: <https://erlang.org/pipermail/erlang-questions/2016-September/090050.html>
- Spark, RDD Programming Guide (closures): <https://spark.apache.org/docs/latest/rdd-programming-guide.html>
- Ray, Serialization: <https://docs.ray.io/en/latest/ray-core/objects/serialization.html>
- Oracle Java Tutorials, Lambda Expressions (serialization): <https://docs.oracle.com/javase/tutorial/java/javaOO/lambdaexpressions.html>
