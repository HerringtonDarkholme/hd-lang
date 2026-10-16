# Serializable Closures And Remote Calls: Design Study

This is the design study for issue #166. Nothing in it is accepted
behavior, and the spec is frozen. Every spelling, std name, and
diagnostic name is a placeholder. It builds on the boundary rules of
[Modules](../../spec/lang/10-modules.md), the requirement rows of
[Requirements And Suspension](../../spec/lang/11-requirements-and-suspension.md),
and the borrows and owned values of
[Ownership And Borrows](OWNERSHIP_DESIGN.md).

- [Decisions So Far](#decisions-so-far)
- [1. Motivation](#1-motivation)
- [2. The Design On One Page](#2-the-design-on-one-page)
- [3. A Guided Tour](#3-a-guided-tour)
- [4. The Rules](#4-the-rules)
- [5. How Other Systems Do It](#5-how-other-systems-do-it)
- [6. Security](#6-security)
- [7. Cost](#7-cost)
- [8. Open Points](#8-open-points)
- [Appendix: Holes And Limits](#appendix-holes-and-limits)

## Decisions So Far

On 2026-10-15 the owner set the aim: the language should help manage
data, as earlier languages took over memory and threads. Five goals
follow from it. Remote calls look like calls, fetching data is easy,
nothing is exposed by default, results are cached as Skip does, and the
wire carries little.

A serializable closure's code identity is a content hash. Its captures
must be boundary-safe values, which replaced an earlier `Durable` bound.
Capturing a provider or mutable state is rejected. None of this is in
the new compiler's first release.

A durable run replays recorded host results; it never serializes a
stack, and recording is opt-in per run. No handle travels. The
receiving place binds authority again through its own requirements.

On 2026-10-16 the owner widened the study from browser and server to
any mix of places: services, workers, stores, GPUs. Which code a GPU may
run is a separate study. Portable closures are same-build only, so an
identity a later build lacks fails with `Incompatible`. Durable and
cross-deploy work uses named `@remote` functions.

The owner also proposed that requirement rows decide placement, and the
draft adopts it. Everything else here is the draft's proposal and needs
the owner's review. [Open Points](#8-open-points) lists what is
undecided. The full discussion, with the owner's words, is in this
file's git history.

## 1. Motivation

Each step in language history took over a job programmers did by hand.
Java's collector replaced `free`, and Go's scheduler replaced thread
pools and callbacks. The owner sees the next step in data: where
computation runs, how data moves, and how access stays fast, persistent,
and reliable.

Today an hd programmer who shows a profile page writes all of this:

- a server route that reads the user and encodes the result as JSON;
- a `fetch` call and a decoder in the browser, over a second copy of the
  types;
- a signed token for state the server hands out and expects back, such
  as the version a form was rendered from;
- a job payload, a queue, and a worker loop for work sent elsewhere;
- a cache, and the code that decides when its entries are stale.

hd already has half the pieces. Boundary-safe types cross registered
boundaries through adapters generated from the signature. Requirement
rows say what a function needs, and runtime profiles say what a host can
bind. Missing are a remote call, a way to say where a function runs,
protection for data sent to an untrusted place, and stale-copy tracking.

The idea in one sentence: **a remote call sends a name for code the
receiver already has, plus boundary-safe data; the receiver binds the
requirements.**

## 2. The Design On One Page

A **place** is an execution context that runs a build of the program
under a runtime profile. A browser, a server, a worker pool, a peer
service, a store, and a GPU are places. An **edge** joins a calling
place to a called one, and each edge has its own trust. A function fits
a place when the place's profile binds every key of its row.

```text
use std.remote.{Caller, Remote, remote}

pub data Message:
    pub author: string
    pub text: string

# Fits only the server: the browser cannot bind the message store.
@query
pub fn recent!(room: string) -> List[Message] $ MessageRead:
    $.use(MessageRead).latest!(room, 50)

@remote
pub fn post!(room: string, text: string) -> Result[void, ChatError] $ MessageWrite + Caller:
    user := match $.use(Caller).user():
        .Some(found) => found
        .None => return .Err(.NotSignedIn)
    $.use(MessageWrite).append!(room, Message { author: user, text: text })
    .Ok()

# Runs in the browser: it needs only a transport.
fn send!(room: string, text: string) -> string $ Remote:
    match remote!(post, room, text):
        .Ok(.Ok()) => "sent"
        .Ok(.Err(.NotSignedIn)) => "please sign in"
        .Err(_) => "offline; try again"
```

The browser's `send!` needs only `$ Remote`. The server binds
`MessageWrite` and `Caller` for each call, so no authority travels. No
line names HTTP, a route, or a decoder. The result type shows the hop:
`RemoteError` on the outside, the function's own errors inside.

| Piece | What it does | Spelling |
| --- | --- | --- |
| `@remote` | Exposes a module-level function to other places | `@remote pub fn post!(...)` |
| `remote!` | Calls a remote function or a portable closure | `remote!(post, room, text)` |
| `@query` | Marks a read-only remote function whose result the home caches and pushes | `@query pub fn recent!(...)` |
| `Sealed[T]` | Data a home hands to an untrusted place and gets back unchanged | `seal(DocVersion { value: 7 })` |
| `portable(...)` | Turns a closure literal into data: a code identity plus captures | `portable(fn(o: Order) -> f64: score(o, w))` |
| `Caller` | A requirement that names the calling user or service | `$.use(Caller).user()` |
| `[places]` | Declares each place's profile and trust in `hd.toml` | `[places.workers]` |

Two identities name code. A **call identity** is the module path, the
function name, and a fingerprint of the signature's types. It routes a
call and survives a body change. A **content identity** hashes the body
and everything it calls, and keys caches, histories, and portable
closures.

No code ever travels, in either form. The place that runs a remote
function is its **home**.

## 3. A Guided Tour

Each program leaves out ordinary trait and error declarations it does
not need to show.

### 3.1 A Page Fetches Its Data

A profile page needs the signed-in user's name and recent orders. Today
that is a route, an encoder, a `fetch`, a decoder, and two copies of
the types. Here it is one function and one call.

```text
use std.remote.{Caller, Remote, remote}

pub data Profile:
    pub name: string
    pub recent: List[OrderLine]

# Shared module, compiled into the server and the browser.
@query
pub fn my_profile!() -> Result[Profile, ProfileError] $ UserRead + Caller:
    match $.use(Caller).user():
        .Some(user) => $.use(UserRead).profile!(user)
        .None => .Err(.NotSignedIn)

# Browser code: the transport is the Remote provider's business.
fn greeting!() -> string $ Remote:
    match remote!(my_profile):
        .Ok(.Ok(profile)) => "Hello, ${profile.name}"
        .Ok(.Err(.NotSignedIn)) => "Please sign in"
        .Ok(.Err(.NotFound)) => "No profile yet"
        .Err(_) => "Offline; try again"
```

The browser decodes `Profile` from its declared type, so a payload of
another shape never reaches user code. The server decodes arguments the
same way, which gives Telefunc's run-time `shield()` without a second
declaration.

Only marked functions can be reached. `remote!` on an unmarked function
is `remote-not-exposed` at build time. A hand-made request that names
one finds no table entry and gets `Incompatible`.

The result type decides what leaves the server. A data type with a
private field, such as a password hash, crosses only through its own
`Serialize`. Without one, the signature is rejected with
`boundary-private-field`.

### 3.2 A Form Sends Back Server State

Next.js's own example: a publish button must fail if the document
changed since the page was rendered. The version is read at render time
and must come back unchanged. With a plain `seen: i64` argument, a
client sends the current version and defeats the check.

```text
use std.remote.{Seal, Sealed, seal}

pub data DocVersion:
    pub value: i64

pub data Draft:
    pub title: string
    pub seen: Sealed[DocVersion]         # the browser carries it but cannot read or change it

@query
pub fn open_draft!(id: string) -> Draft $ Docs + Seal:
    docs := $.use(Docs)
    Draft { title: docs.title!(id), seen: seal(DocVersion { value: docs.version!(id) }) }

@remote
pub fn publish!(id: string, seen: Sealed[DocVersion]) -> Result[void, PublishError] $ Docs + Seal:
    docs := $.use(Docs)
    if seen.open().value != docs.version!(id):
        return .Err(.Changed)
    docs.publish!(id)
    .Ok()
```

A sealed value is authenticated encryption under a key that only the
server's `Seal` provider holds. A tampered value fails as it is decoded,
so `publish!` never runs. Its type and a purpose label are part of the
encryption. A `Sealed[DocVersion]` therefore cannot pass where a
`Sealed[Discount]` is expected.

The server keeps no state for it. The value lives in the page or a
hidden form field, and survives a restart while the key is kept. A seal
may also carry an expiry, and may be bound to the caller so that
another user cannot replay it.

The same form can carry a closure instead. The page holds the publish
action itself, and the browser runs it at the server without seeing
what it captured:

```text
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

# Browser code: the captures `id` and `seen` travel sealed.
fn on_publish!(page: Page) -> string $ Remote:
    match remote!(page.publish):
        .Ok(.Ok()) => "published"
        .Ok(.Err(.Changed)) => "someone edited it; reload"
        .Ok(.Err(.Forbidden)) => "not allowed"
        .Err(.Incompatible) => "the site was updated; reload"
        .Err(_) => "offline; try again"
```

`publish_if!` is a plain function, not `@remote`. It is reachable only
through a closure the server made, with the captures the server chose.
On an untrusted edge the whole closure is sealed, so a browser can
neither edit `seen` nor build its own closure. After a deploy that
changes `publish_if!`, the old closure answers `Incompatible`, and the
page offers a reload.

React's `addToCart.bind(null, productId)` ports to either form: a named
function with a sealed argument, or a sealed portable closure.

### 3.3 A Service Calls A Service

Checkout reserves stock from an inventory service that another team
deploys on its own schedule. The call needs a deadline, safe retries,
and a clear failure when the two deploys disagree.

```text
use std.remote.{Caller, Remote, RemoteError, remote}
use std.task.race
use std.time.{Clock, Duration, sleep}

# Inventory service: its own package and deploy schedule.
@remote
pub fn reserve!(r: Reservation) -> Result[void, ReserveError] $ Stock + Caller:
    if $.use(Caller).service().is_none():
        return .Err(.NotAService)
    $.use(Stock).reserve!(r)

# Checkout service, built against the inventory package's interface.
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

On a service edge, `Caller.service()` names the calling service, which
the transport authenticates, as Dapr does with mutual TLS. `user()` is
none, so code that checks for a signed-in user refuses services until
it opts in. When the timer wins, `race!` cancels the call, and the
transport carries the cancel to inventory.

Now inventory adds a field to `Reservation` and deploys first. The
fingerprint covers type layouts, so the old call matches nothing and
gets `Incompatible`. Checkout maps that to `Misconfigured`. Inventory
keeps the old `reserve!` beside a new `reserve_v2!` until every caller
has moved, as Orleans versions grain interfaces.

A body change, unlike a signature change, keeps the call identity. A
browser tab from yesterday still calls `post!` after today's bug fix.

### 3.4 A Worker Fan-Out

A ranking job scores 100,000 orders with weights the request chose. The
scoring runs on a worker pool of the same build, as Ray runs tasks on a
cluster. Without the feature, the job needs a payload type, a queue,
and a worker loop that dispatches on a job name.

```text
use std.remote.{Portable, RemoteError, portable}

pub data Weights:
    pub size: f64
    pub bias: f64

pub fn score(order: Order, weights: Weights) -> f64:
    f64(order.total) * weights.size + weights.bias

# A library provider over a worker pool place.
pub trait Workers:
    fn map![A < Boundary, B < Boundary](self, items: List[A], job: Portable[fn(A) -> B]) -> Result[List[B], RemoteError]

fn rank!(orders: List[Order], weights: Weights) -> Result[List[f64], RemoteError] $ Workers:
    $.use(Workers).map!(orders, portable(fn(o: Order) -> f64: score(o, weights)))
```

The weights travel; the code does not. `map!` sends the closure's
content identity and its encoded captures to each worker. Each worker
finds the identity in its own table of portable closures. The edge is
trusted, so nothing is sealed.

If the request is cancelled midway, the provider aborts the items
still running. The captures were copies, so no worker has state to undo.

A capture must be plain boundary-safe data, never a borrow (`^`) or an
owned (`own`) value. A connection is owned, so it cannot ride along:

```text
fn warm!(urls: List[string]) -> Result[List[bool], RemoteError] $ Workers + Net:
    conn := connect!("cache")
    $.use(Workers).map!(urls, portable(fn(u: string) -> bool: conn.has(u)))   # error: portable-capture-not-boundary-safe
```

A closure that assigns to a captured binding is rejected too. Spark
shows why: a counter bumped on executors stays 0 at the driver.

### 3.5 A Predicate Pushed Down To A Store

A report needs the orders above a threshold. Sending the predicate to
the store moves only the matching rows. Without the feature, the store
grows one filter endpoint per report.

```text
use std.remote.{Portable, portable}

pub trait OrderStore:
    fn where!(self, keep: Portable[fn(Order) -> bool]) -> List[Order]

fn large_orders!(min: i64) -> List[Order] $ OrderStore:
    $.use(OrderStore).where!(portable(fn(o: Order) -> bool: o.total > min))
```

This works only for a store that is a place running the same build,
such as a storage service written in hd. .NET LINQ translates a lambda
into SQL; hd translates closures into no other language.

A predicate cannot take a provider with it:

```text
fn big_in_usd!(min: i64) -> List[Order] $ OrderStore + Rates:
    rates := $.use(Rates)
    $.use(OrderStore).where!(portable(fn(o: Order) -> bool: rates.usd(o.total) > min))   # error: portable-capture-not-boundary-safe
```

`rates` is a provider, which is not boundary-safe and is never
captured. The fix puts `Rates` in the closure's own row. The store must
then bind `Rates` itself, or the closure does not fit it.

### 3.6 Placement By Requirement Rows

A server reaches a database and a browser cannot. A GPU reaches only
its device, and a worker has limited resources. Rows already state
these needs, so placement needs no new mechanism.

| Place | Profile binds (illustrative) | Fits | Does not fit |
| --- | --- | --- | --- |
| Server | `UserRead`, `Docs`, `MessageWrite`, `Seal`, `Caller`, `Clock`, `Remote` | `my_profile`, `publish` | A function that needs a browser DOM key |
| Browser | `Remote`, `Console`, `Clock` | `greeting`, `send` | `my_profile`, which needs `UserRead` |
| Worker pool | `Clock` | The scoring closure, which has no row | A closure with `Caller` or `Docs` in its row |
| GPU | Its device key only | A kernel with no row, if the GPU subset admits it | Anything with a host key |

A function that needs `$ Db` cannot run in the browser, and the build
can say so before deploy. The build declares its places beside the
capability grants:

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

With declared places, a `@remote` function that fits no place fails the
build. So does a closure sent to a pool that cannot bind its row:

```text
fn tag_all!(orders: List[Order]) -> Result[List[string], RemoteError] $ Workers:
    $.use(Workers).map!(orders, portable(fn!(o: Order) -> string $ Caller: tag(o, $.use(Caller).user())))   # error: portable-row-not-bound
```

The fix reads the caller first and captures the result. Without
declared places, the same checks run later. A place checks its remote
functions when it starts serving them, and each portable closure when
it arrives.

Rows answer one question: can this code run there? Grants inside a key,
such as which tables, stay in `hd.toml`'s capability scopes. Deployment
configuration picks among several fitting places. Trust belongs to the
edge, the data path to the transport, and the GPU subset to its own
study.

### 3.7 Caching And Live Queries

A chat room shows the last 50 messages. When anyone posts, every open
view updates, and the server re-reads only what changed. Today this
means a cache, a pub-sub channel, and hand-written invalidation. With
the `recent` query of [section 2](#2-the-design-on-one-page):

```text
# Browser code. `watch` is a library call over Remote: each `next!`
# completes when the query's result changes.
fn follow!(room: string) -> void $ Remote + Console:
    let feed: mut Watch[List[Message]] = watch(recent, room)
    while true:
        messages := feed.next!()
        println("${messages.len()} messages")
```

A query's row holds only read-only inputs that the runtime tracks, plus
`Caller`, so a query cannot write. hd code is deterministic between host
calls, so the host results decide a run. The runtime records those
reads at the host boundary, where the replay rules already intercept
calls.

The memo key holds the query's content identity, its arguments, and the
revisions of what it read. It adds the caller when `Caller` is in the
row. When a
post changes a read, the entry is stale, and subscribers get the new
result. This is the model of Skip and Convex.

A body edit changes the content identity of that query and every query
above it. Their entries are dropped, and unchanged queries keep theirs
across a deploy.

Two gaps remain. A query that reads a top-level `mut` binding keeps a
stale result, since that read is not tracked; a lint flags it. Reuse is
per query call, so a change inside a large result re-runs the whole
query, which is coarser than Skip.

### 3.8 Failure And Durability

Every remote call can fail in ways a local call cannot. Those failures
are kept apart from the function's own errors:

```text
pub enum RemoteError:
    Unreachable                 # the call never started
    OutcomeUnknown              # it started, and no result arrived
    Incompatible                # the receiver has no function or closure with this identity
    Rejected(message: string)   # the receiver refused the arguments
    Panicked(category: string)  # the remote run panicked
```

A remote panic reaches the caller as `Panicked` and does not poison it.
Cancelling the caller cancels the call. A retry carries a request ID or
an idempotency key, so the receiver returns its recorded result instead
of running twice.

An order is charged, then a receipt is mailed. The server dies between
the two and is redeployed. The run must resume without charging twice:

```text
@remote
pub fn fulfil!(order: OrderId) -> Result[void, FulfilError] $ Payments + Mailer:
    $.use(Payments).charge!(order, "charge-${order.value}")?
    $.use(Mailer).send_receipt!(order)?
    .Ok()
```

When the host records the run, the history holds the call identity, the
arguments, and each host result. After the crash the run replays: the
charge comes from the history, and the mailer runs live. No stack is
serialized. A host call caught in flight re-runs only when it is
idempotent, and otherwise reports `OutcomeUnknown`.

### 3.9 GPU Kernels

Which code a GPU may run is a separate study. Triton, Futhark, and WGSL
each limit what a kernel may do, and hd would need such a subset too.
This design only keeps room for it. A GPU is a place whose profile
binds only its device, and a kernel would be a portable closure with no
row.

```text
use std.remote.{Portable, portable}

# Sketch only: the kernel subset and the buffer path belong to the GPU study.
pub trait Gpu:
    fn map!(self, input: List[f32], kernel: Portable[fn(f32) -> f32]) -> List[f32]

fn brighten!(pixels: List[f32], gain: f32) -> List[f32] $ Gpu:
    $.use(Gpu).map!(pixels, portable(fn(p: f32) -> f32: p * gain))
```

## 4. The Rules

This section states each rule from the tour once, grouped by piece.

### 4.1 Places And Edges

1. A place is an execution context that runs a build of the program
   under a runtime profile.
2. A remote function or portable closure fits a place when the place's
   profile binds every key of its row. This is the existing check on a
   registered entry's row, applied per place.
3. When the build declares its places, a remote function that fits none
   is `remote-no-place`.
4. A declared place may name the providers that send work to it. A
   portable argument to such a provider must fit the place, or it is
   `portable-row-not-bound`.
5. Without declared places, a place checks its remote functions when it
   starts serving them, and each portable closure on arrival.
6. Each edge is trusted, a service edge, or untrusted. An edge with no
   declared trust is untrusted.

### 4.2 Remote Functions

1. `@remote` may precede a module-level function and exposes it to
   other places. `@query` implies `@remote`.
2. Every parameter type and the result type is boundary-safe. Otherwise
   the signature is `remote-signature-not-boundary-safe`.
3. A remote function has no type parameters (`remote-generic`). A
   receiver cannot compile instantiations it never saw.
4. A place serves the remote functions of the modules its serving
   registration names. A dependency's marks are inert until named.
5. Arguments from an untrusted edge are untrusted. They are decoded by
   the declared parameter types only.
6. Authorization is ordinary code in the function. The language
   guarantees exposure and typed arguments, not ownership checks.

### 4.3 Calls And The Caller

1. `std.remote.remote` is a compiler intrinsic. Its first argument names
   a remote function or is a portable value; otherwise it is
   `remote-not-exposed`.
2. For a function of type `fn!(P1, ..., Pn) -> O $ R`, the call
   `remote!(f, a1, ..., an)` checks each argument against its parameter.
3. The call has type `Result[O, RemoteError]` and needs `$ Remote`, not
   `R`. The receiving place binds `R`.
4. `Remote` is a host capability trait. Its provider picks the transport
   per edge: HTTP, WebSocket, a worker message, service RPC, or
   in-process for tests.
5. Cancelling the calling suspension cancels the call. A transport
   should carry the cancel to the receiver.
6. `Caller` is a requirement trait that the serving transport binds per
   request. `user()` and `service()` return the authenticated user and
   calling service, either of which may be none.

### 4.4 Identities

1. A call identity is the module path, the function name, and a
   fingerprint of the parameter, result, and row types. It routes calls.
2. A content identity is a SHA-256 over the function's normalized form.
   It also covers the content identities of what it calls, type
   layouts, the compiler version, and the build profile.
3. Names are not part of a content identity. A recursive group is hashed
   as one unit, as Unison does.
4. Content identities key memo entries, durable histories, and portable
   closures.

### 4.5 Sealed Values

1. `Sealed[T < Boundary]` is a boundary-safe data type. `Boundary` is
   implemented exactly by the boundary-safe types, as `AnyVal` is.
2. No owned position, no type with an `own` part, and no type that
   carries a lifetime implements `Boundary`.
3. `seal(value)` makes a sealed value and `s.open()` reads it. Both need
   `$ Seal`, a host capability that holds the home's key.
4. A sealed value is authenticated encryption of `T`'s canonical
   encoding, with a key identifier. `T`'s type identity and a label
   fixed at the `seal` site are bound into it.
5. A seal may carry an expiry, and may be bound to the caller's
   principal.
6. A sealed value is verified when it is decoded at the home's boundary.
   A value that fails is the boundary failure `sealed-invalid`, and the
   function never runs.

### 4.6 Queries

1. Every key in a query's row is a tracked input of the runtime profile,
   or `Caller`. Otherwise it is `query-row-not-tracked`.
2. A tracked input trait has no `mut` method and no method the replay
   rules classify as a write.
3. A query's memo key is its content identity, its canonical arguments,
   and the revisions of its reads. It adds the caller when `Caller` is in
   the row.
4. The reads are the tracked host calls the run made, recorded at the
   host boundary.
5. An entry whose reads change is stale, and each subscriber gets the
   re-run result. An eviction policy bounds the memo table.
6. A lint flags a query body that reads a top-level `mut` binding.

### 4.7 Portable Closures

1. `std.remote.portable` is a compiler intrinsic whose one argument is a
   closure literal; otherwise it is `portable-not-literal`.
2. Its result has type `Portable[F]`, where `F` is the closure's
   ordinary function type. No second kind of function type is added.
3. `Portable[F]` is boundary-safe. `p.call(...)` runs it locally and
   needs `F`'s row; `remote!(p, ...)` runs it at another place.
4. Every capture is plain boundary-safe data, never a borrow or an owned
   value. Otherwise it is `portable-capture-not-boundary-safe`.
5. A portable closure captures no provider; its keys go in its row. One
   that assigns to or mutates a capture is `portable-capture-mutable`.
6. A literal whose type or captures mention an enclosing type parameter
   is `portable-generic`.
7. A portable value encodes as the content identity of its lifted body,
   plus its canonical captures. The lifted body is a module-level
   function whose leading parameters are the captures.
8. Every `portable` literal in the build is in the build's portable
   table. A place without the identity answers `Incompatible`.
9. A portable value is promised to run only at places running the build
   that made it.
10. On an untrusted edge, a portable value is sealed whole, with its
    identity as the purpose label. A place accepts one from an
    untrusted edge only under its own seal (`portable-unsealed`).

### 4.8 Durability And The Wire

1. A recorded remote run keeps its call identity, its arguments, and its
   host results. After a crash it replays; no stack is serialized.
2. A host call in flight at the crash re-runs only when idempotent, with
   a stable key. Otherwise the program sees `OutcomeUnknown`.
3. A retrying transport attaches a request ID. The receiver returns the
   recorded result for a repeated ID.
4. A call is its identity plus its arguments, and a result is a boundary
   value. The transport encoding stays free.
5. Hashed and sealed values use one canonical encoding: map entries in
   key order, floats by their bits.

## 5. How Other Systems Do It

| System | What it ships | How it names code | What it trusts |
| --- | --- | --- | --- |
| React Server Functions, Next.js Server Actions | A reference plus arguments; Next.js also encrypts an inline action's captures | A framework ID; Next.js IDs change every build | Nothing from the client; each function checks the user |
| tRPC, Telefunc, gRPC | Arguments and results | Procedure path, function name, or `.proto` method | Arguments are validated by schema or a generated `shield()` |
| React Router | Loader data and form posts | The route path | Loaders and actions check the user |
| Skip, Convex | Query results and pushed updates | Resource or function name | The service; queries must be deterministic |
| Cap'n Proto, Cloudflare RPC | Calls on references to objects kept at home | Interface IDs and capability references | Whoever holds a reference may call it |
| Unison | Code by content hash; the receiver fetches what it lacks | A hash of the syntax tree, names excluded | Abilities handled at the destination |
| Cloud Haskell, Scala Spores3 | A static code key plus serialized captures | A key valid only within one binary | The same binary |
| Temporal, Orleans, Dapr | Names plus payloads | Registered workflow, grain interface, or app ID | Dapr authenticates services by mutual TLS and filters them by app ID |
| Ray, Spark, Java lambdas | Serialized closures, as pickled code or class names | Whatever arrives | Whatever arrives runs |
| ScalaLoci, Hydro | One program split per location | The compiler places each value or collection | Not addressed |

The web systems that succeeded name known code and send data. None
ships code to the server, and hd follows them for both named functions
and closures. Exposure by an explicit mark, as `"use server"` and
`.telefunc` files do, becomes `@remote`.

Captured state is the hard part. React's bound arguments are
client-controlled, and Next.js encrypts captures per build. hd makes
that encryption a type, `Sealed[T]`, instead of hidden framework work.
Skip and Convex show that a cache needs determinism and a read set, and
hd already has both.

Ray matches declared resources to nodes; hd matches rows to profiles.
Cloud Haskell keys hold only for one binary, as hd's closures hold for
one build. Temporal, Orleans, and protobuf keep cross-version work on
named entry points, as hd does.

## 6. Security

An untrusted client can call any `@remote` function in a served module,
with any arguments that decode as the declared types. It can carry a
sealed value or a portable closure, return it, and replay it unless a
seal binds the caller or carries an expiry.

A client cannot reach an unmarked function, and it cannot read, edit,
or forge a sealed value. It cannot build a portable closure the server
will run, since only the server's own seal is accepted. It never
receives a handle, a provider, or a `Seal` key.

Adding a dependency adds no endpoint, because a place serves only the
modules its registration names. The default transport for an untrusted
edge checks the request's Origin and guards against CSRF. Without those
checks, "secure by default" does not hold.

The language does not check who owns the data. A function that deletes
a post must check that the caller wrote it. `Caller` in its row shows
that it reads the caller.

## 7. Cost

The numbers are estimates for planning, not measurements.

| Piece | Cost at run time |
| --- | --- |
| A remote function | One adapter at home, about 0.3 to 2 KB of Wasm, and a table row under 64 bytes. One stub per caller, about 0.2 to 1 KB. |
| A call on the wire | A 16-byte identity prefix plus canonical arguments. Program 3.1's call is under 40 bytes. |
| A sealed value | About 30 bytes over its plaintext: a 12-byte nonce, a 16-byte tag, and a key ID. One key per deployment. |
| Server state | None between calls. |
| A memo entry | A 32-byte key, the result, and the revision tokens it read. With `Caller` in the row, entries grow with users times arguments. |
| A portable closure | One adapter per literal. About 40 bytes per send in the fan-out of 3.4, plus 30 on an untrusted edge. |
| Hashing at build | Only for queries, recorded entries, and portable literals. A 100,000-line program hashes in about 10 to 40 ms. |

A body edit changes no call identity, so deployed clients need no
rebuild. A fan-out provider sends the closure once per worker.

## 8. Open Points

1. **Sealed values or server-side references.** Draft: sealed values,
   with no server state. Alternative: Cap'n Proto references, which add
   pipelining and callbacks but hold state per connection.
2. **Call spelling.** Draft: an explicit `remote!(f, args)`.
   Alternative: a plain `f!(args)` that becomes a stub wherever `f`'s
   row cannot be bound, as in Next.js.
3. **Caller binding of a seal.** Draft: off by default, since it breaks
   shared links. Alternative: on by default, which stops replay by
   another user.
4. **Routing among fitting places.** Draft: one `Remote` key, with
   deployment configuration mapping modules to places. Alternative: one
   key per place, as `$ Remote[Billing]`.
5. **Which modules a place serves.** Draft: the modules the serving
   registration names. Alternative: every included module, or the
   package's own modules plus explicit re-exports.
6. **Where places are declared.** Draft: `[places]` in `hd.toml`.
   Alternatives: declarations in hd source, or host configuration
   checked only at run time.
7. **Portable closure spelling.** Draft: the intrinsic
   `portable(fn ...)`. Alternative: a keyword or decorator on the
   closure literal.
8. **Signature changes across deploys.** Draft: an exact fingerprint,
   with the old function kept beside the new. Alternative: protobuf
   rules, where adding a defaulted field keeps the identity.
9. **Service principal spelling.** Draft: `Caller.service()` beside
   `user()`. Alternative: one `Caller.principal()` returning an enum.

The draft treats three earlier questions as settled by the goals; the
owner may reopen any of them. The cache belongs to the language and
runtime, since a library cannot see host-call reads. A place runs a
closure from an untrusted edge only under its own seal; otherwise every
`portable` literal is a public endpoint. An edge with no declared trust
is untrusted, which is "secure by default".

## Appendix: Holes And Limits

The full stress test, 39 cases over three shapes, is in git history.
These remain for the recommended design.

Hole:

- A query that reads a top-level `mut` binding keeps a stale result; the lint is not a full check ([3.7](#37-caching-and-live-queries)).

Limits:

- Cache reuse is per query call, coarser than Skip's per entry ([3.7](#37-caching-and-live-queries)).
- No promise pipelining: two round trips, or one combined remote function.
- No callback from home to caller; a pushed query result covers the common case ([3.7](#37-caching-and-live-queries)).
- Determinism is promised per compiled program, so browser and server may disagree on map order; the server rechecks.
- A signature change breaks callers on another deploy until they move ([3.3](#33-a-service-calls-a-service)).
- Push-down needs a store that runs the same build; nothing is translated to SQL ([3.5](#35-a-predicate-pushed-down-to-a-store)).
- A portable closure from before a deploy fails with `Incompatible` ([3.2](#32-a-form-sends-back-server-state)).
- Without declared places, a missing requirement fails at startup or on arrival, not at build ([3.6](#36-placement-by-requirement-rows)).
- The memo table grows with users times arguments when `Caller` is in the row, so it needs eviction ([7](#7-cost)).

## Sources

- React, Server Functions: <https://react.dev/reference/rsc/server-functions>
- React, `<form>` (hidden fields, `bind`, progressive enhancement): <https://react.dev/reference/react-dom/components/form>
- Next.js, Server Actions: <https://nextjs.org/docs/app/guides/server-actions>
- Next.js, Data Security (closures and encryption, IDs, allowed origins): <https://nextjs.org/docs/app/guides/data-security>
- tRPC, Concepts: <https://trpc.io/docs/concepts>
- Telefunc: <https://telefunc.com/>
- Telefunc, `shield()`: <https://telefunc.com/shield>
- React Router, Data Loading: <https://reactrouter.com/start/framework/data-loading>
- React Router, Actions: <https://reactrouter.com/start/framework/actions>
- Skip (SkipLabs), Overview: <https://skiplabs.github.io/skip/docs/introduction>
- Convex, Realtime: <https://docs.convex.dev/realtime>
- Cap'n Proto, RPC Protocol: <https://capnproto.org/rpc.html>
- Cloudflare Workers, RPC: <https://developers.cloudflare.com/workers/runtime-apis/rpc/>
- gRPC, Core concepts: <https://grpc.io/docs/what-is-grpc/core-concepts/>
- gRPC, Deadlines: <https://grpc.io/docs/guides/deadlines/>
- gRPC, Retry: <https://grpc.io/docs/guides/retry/>
- Protocol Buffers, Updating a message type: <https://protobuf.dev/programming-guides/proto3/#updating>
- Unison, "The big idea": <https://www.unison-lang.org/docs/the-big-idea/>
- Unison, Distributed datasets with the `Remote` ability: <https://www.unison-lang.org/articles/distributed-datasets/>
- GHC User's Guide, Static pointers: <https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/static_pointers.html>
- Scala Spores3: <https://github.com/phaller/spores3>
- Miller, Haller, Odersky, "Spores: A Type-Based Foundation for Closures in the Age of Concurrency and Distribution", ECOOP 2014: <https://doi.org/10.1007/978-3-662-44202-9_13>
- Temporal, Workflows: <https://docs.temporal.io/workflows>
- Temporal, Workflow Definition (versioning): <https://docs.temporal.io/workflow-definition>
- Microsoft Orleans, Grain interface versioning: <https://learn.microsoft.com/en-us/dotnet/orleans/grains/grain-versioning/grain-versioning>
- Dapr, Service invocation overview (app IDs, mTLS, retries): <https://docs.dapr.io/developing-applications/building-blocks/service-invocation/service-invocation-overview/>
- Dapr, Access control for service invocation: <https://docs.dapr.io/operations/configuration/invoke-allowlist/>
- Spark, RDD Programming Guide (closures): <https://spark.apache.org/docs/latest/rdd-programming-guide.html>
- Ray, Serialization: <https://docs.ray.io/en/latest/ray-core/objects/serialization.html>
- Ray, Resources (logical resources, `num_gpus`): <https://docs.ray.io/en/latest/ray-core/scheduling/resources.html>
- Oracle Java Tutorials, Lambda Expressions (serialization): <https://docs.oracle.com/javase/tutorial/java/javaOO/lambdaexpressions.html>
- Weisenburger, Köhler, Salvaneschi, "Distributed System Development with ScalaLoci", OOPSLA 2018: <https://doi.org/10.1145/3276499>
- Hydro, documentation (locations, live collections): <https://hydro.run/docs/hydro/reference/locations/>
- .NET, Expression trees (LINQ providers): <https://learn.microsoft.com/en-us/dotnet/csharp/advanced-topics/expression-trees/>
- GPU study pointer: Triton: <https://triton-lang.org/>
- GPU study pointer: Futhark: <https://futhark-lang.org/>
- GPU study pointer: WebGPU (W3C): <https://www.w3.org/TR/webgpu/>; WGSL: <https://www.w3.org/TR/WGSL/>

## Parse Log

Every `text` block, 14 in all, was parsed on 2026-10-16 with
`scripts/hd-conformance.sh parse`, and all 14 parse as written. Parsing
checks syntax only. Nothing was type-checked, and the blocks use std
names and declarations this study only proposes. The `toml` sketch in
3.6 is hypothetical syntax and was not parsed.
