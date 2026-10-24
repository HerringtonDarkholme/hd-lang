# HTTP Server Handlers

Status: design record, 2026-10-24. The direction is decided; the SDK and its host capability are not built.
This answers [#266](https://github.com/HerringtonDarkholme/hd-lang/issues/266) under its [revised scope](https://github.com/HerringtonDarkholme/hd-lang/issues/266#issuecomment-6375211580), with the owner's decisions of 2026-10-24 on that issue.
It continues the unreviewed [research notes at 187e5a93e](https://github.com/HerringtonDarkholme/hd-lang/blob/187e5a93e/docs/future-work/http-server/NOTES.md).

**Decided design.**
An application is an ordinary `main!`, with no HTTP-specific entry ([#806](https://github.com/HerringtonDarkholme/hd-lang/issues/806), direction A).
One Wasm instance is retained per adapter; request state lives in one explicit state object.
`HttpServer` is a capability owned by a server SDK package and delivered through package-declared host capabilities, which [#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51) designs.
Packaging is done by SDK tasks such as `hd run deploy-cloudflare` and `hd run deploy-vercel`; there is no `hd build` flag.
The order is #51's minimum first, then the adapters of #266.

The records under review are [Http](../../spec/std/http.md), [Task](../../spec/std/task.md), [entry points](../../spec/lang/10-modules.md#entry-point), and [program instances](../../spec/lang/10-modules.md#instances-and-threads).
The application model [#806](https://github.com/HerringtonDarkholme/hd-lang/issues/806) is closed with direction A; the SDK boundary, deployment profile, and lifecycle policies stay open there.

## Contents

- [Problem And Fixed Use Cases](#problem-and-fixed-use-cases)
- [What Works Today](#what-works-today)
- [Prior Art](#prior-art)
- [Platform Limits And JSPI](#platform-limits-and-jspi)
- [Options](#options)
- [Application Entry](#application-entry)
- [Bodies And HTTP Mapping](#bodies-and-http-mapping)
- [Concurrency, Cancellation, And State](#concurrency-cancellation-and-state)
- [Capabilities And Grants](#capabilities-and-grants)
- [Architecture](#architecture)
- [Packaging And Deployment](#packaging-and-deployment)
- [Semantics And Counterexamples](#semantics-and-counterexamples)
- [Cost And Measurements](#cost-and-measurements)
- [Stress Test](#stress-test)
- [Phasing](#phasing)
- [Recommendation And Approval](#recommendation-and-approval)
- [Prototype And Parse Log](#prototype-and-parse-log)
- [Issue And Domain Review](#issue-and-domain-review)

## Problem And Fixed Use Cases

A handler accepts one request and produces one response.
A server owns listening, request admission, cancellation, and transport errors.
Cloudflare and Vercel already provide that server; hd supplies a handler and a small adapter.
Local development supplies the missing listener through Node.

The important boundary is the request lifetime.
An isolate may handle several requests while earlier handlers await I/O.
A compiled Wasm module is reusable code; a Wasm instance owns mutable memory and suspension state.
Sharing the former does not require sharing the latter.

| Case | End-to-end requirement |
| --- | --- |
| U1: Cloudflare JSON API | Build one hd handler; deploy a fetch adapter; decode a typed JSON body; return JSON or 400. |
| U2: same handler on Vercel | Rebuild the same source for Node or Edge; emit platform files; preserve the handler's responses and capability restrictions. |
| U3: local outgoing HTTP | `hd run api`; send a local request; await a granted upstream Http call; inspect the returned response and request log. |
| U4: timeout | A handler uses `timeout!`; cancelled work completes cleanup; the handler returns 504; disconnect also cancels outstanding work. |

Routing is ordinary hd code over `Request` for v1.
There is one registered handler per executable, not a router registration language.
Cron, queues, WebSockets, durable objects, TLS termination, and application startup hooks remain outside this job.

## What Works Today

A public function named `handle!` is an ordinary function.
[Entry rules](../../spec/lang/10-modules.md#entry-point) recognize parameterless `main` or `main!`, and the design adds no other entry.
The single runtime root runs one `main!`, which can join several worker loops with `all!`.

The [runnable control](http-server/prototype/app/src/main.hd) calls an ordinary typed `handle!` from `main!`.
It exchanges a JSON envelope through Args and Console, solely to exercise today's typed code and runtime.
Application logging would corrupt that bridge, and production uses the SDK capability instead.
The prototype `gap` package declares a `kind = "handler"` manifest key that the design rejects; the checker ignores it with `unknown-manifest-key`.

Current-compiler controls on Node and workerd 1.20261023.1 show the retained-instance shape working.
An ordinary `main!` with four joined worker loops and one state object serves sequential requests that count 1, 2, 3.
Four overlapping outgoing requests reach peak upstream concurrency four.
One request may start its second fetch while another request causes the poll, so each request needs its own I/O scope.
The SDK gives each request a lease-scoped I/O dispatcher, and an adversarial ordering then passes on both hosts.
These controls use a private Console bridge, not the public capability.
Disconnect and deadline cleanup, body limits, packaging, Vercel and hosted deployment are not yet proven.

## Prior Art

### Spin: Explicit Triggers And Evolving Instance Reuse

[Spin's HTTP trigger](https://spinframework.dev/v4/http-trigger) maps manifest routes to component handlers and supplies a local listener.
Its executor adapts the incoming HTTP interface rather than asking application code to accept sockets.
The Rust-facing handler returns a response; routing and middleware remain separate concerns.
Wagi additionally supports command-shaped programs through environment variables and output.

The current documentation distinguishes P2's fresh instances from P3's reusable instances.
P3 supports consecutive and concurrent reuse, with controls for reuse count, concurrency, and idle expiration.
Reuse makes caches useful but also permits accidental request data retention in globals.
Spin explicitly discourages depending on any guaranteed reuse degree.

[Outbound permissions](https://spinframework.dev/v4/http-outbound) are configured separately from receiving requests.
This separation fits hd's existing Http capability.
Borrow the explicit trigger and grant separation; do not borrow WIT, Component Model scheduling, or its reuse semantics implicitly.
A typed entry can be small while its scheduling policy remains explicit.

### wasmCloud: Stateless Components And Pinned Services Are Different Products

[wasmCloud's earlier HTTP component model](https://wasmcloud.com/docs/v1/developer/components/run/) implements a standard incoming handler behind an HTTP provider.
Its current [trigger-service model](https://wasmcloud.com/docs/overview/hosts/trigger-services/) deliberately pins a component in one store.
HTTP invocations and optional long-running work share that instance's memory.
Per-invocation tasks provide concurrent ingress; replicas each retain separate state.

This permits caches and connection pools without pretending they are globally durable.
It also requires supervision: a guest trap restarts the instance and fails its in-flight invocations.
Cancellation is cooperative, while runaway work needs a host backstop.
The current HTTP ingress requires WASI P3 rather than hd's core-Wasm ABI.

The useful lesson is the separation of request handlers from stateful application services.
The chosen design is the latter: one retained instance with a lease per request, including its trap and shared-state obligations.
Copying only the shared instance while omitting supervision would copy its costs without its protections.

### Fastly Compute: A Request Is A Program Invocation

[Fastly's Rust entry macro](https://docs.rs/fastly/latest/fastly/attr.main.html) accepts a request and returns a response or error.
It wraps obtaining the downstream request and sending the resulting response.
This gives ordinary command-shaped execution an HTTP boundary without requiring a general embedding registry.
The [Request API](https://www.fastly.com/documentation/reference/compute/sdks/rust/fastly/struct.Request/) exposes blocking and asynchronous backend operations, including pending-request selection.

Fastly's host performs networking; the guest selects configured backends and controls HTTP processing.
Streaming APIs avoid forcing every body through a whole-body allocation.
That synchronous interface works because the native Compute host can wait outside the JavaScript event-loop problem.
It does not prove that synchronously waiting inside a V8 isolate is safe.

Borrow the small request-to-response entry and host-owned transport.
Keep asynchronous hd calls as ordinary suspensions; do not emulate a native blocking host with a busy loop.
Platform-specific caches and backend names remain extensions, not common v1 dependencies.
Its SDK cannot simply be linked into hd's different host ABI.

### Rust `worker`: A Thin Fetch Binding Over JavaScript APIs

The [`event(fetch)` macro](https://docs.rs/worker-macros/latest/worker_macros/attr.event.html) converts a JavaScript fetch event into a Rust async function.
It accepts convertible request types and returns convertible responses or errors.
The [`http` feature](https://docs.rs/worker/latest/worker/) uses standard Rust HTTP types and a body backed by a JavaScript ReadableStream.
This allows adapters such as Axum without duplicating the entire HTTP model.

The host retains Workers' event loop and platform bindings.
Its `Env` and context expose platform services, including facilities beyond portable HTTP.
Its [build tooling](https://github.com/cloudflare/workers-rs) emits JavaScript glue and Wasm; platform compatibility and panic handling remain real deployment concerns.
It is a Workers integration, not an identical runtime on Vercel or native Wasmtime.

Borrow reuse of existing Request/Response types and thin platform adapters.
Keep portable capabilities separate from platform-specific bindings.
Do not copy its optional error-to-response behavior as an implicit promise to expose internal error text.
The proposed common adapter returns a generic 500 for an uncaught failure.

## Platform Limits And JSPI

These are documentation observations on 2026-10-23, not permanent constants in the language specification.
Deployment validation must report the selected profile's current limits.

| Platform | Relevant published limits and packaging |
| --- | --- |
| Cloudflare | [Limits](https://developers.cloudflare.com/workers/platform/limits/): 64 MiB uncompressed bundle, 128 MB per isolate including Wasm, one-second global startup; request CPU and subrequest quotas depend on plan. |
| Cloudflare Wasm | [Runtime support](https://developers.cloudflare.com/workers/runtime-apis/webassembly/): instantiate a precompiled module; ship a static Wasm import. |
| Vercel Node | [Function limits](https://vercel.com/docs/functions/limitations): ordinary uncompressed bundle limit 250 MB, 4.5 MB request/response payload limit; memory and duration depend on plan. |
| Vercel Edge | [Runtime](https://vercel.com/docs/functions/runtimes/edge): static Wasm imports, no runtime byte compilation; response start within 25 seconds; gzip limits 1/2/4 MB by plan. |

Vercel now documents that Next.js 16.3 removes the route/page `runtime = 'edge'` option.
Its standalone Edge Function interface remains documented on the same runtime page.
The proposed Edge artifact therefore targets the [Build Output API](https://vercel.com/docs/build-output-api/primitives), not a Next.js route convention.
Node is the default Vercel target; Edge remains an explicitly tested separate artifact.

### What The Probes Actually Establish

| Host tested | JSPI availability | Actual execution |
| --- | --- | --- |
| workerd 1.20261023.1, compatibility date 2026-10-23 | `Suspending` and `promising` are functions | A statically imported tiny module suspends on a Promise and returns 7. |
| `@edge-runtime/vm` 5.0.0, primitives 6.0.0, Node 24.19.0 | Both APIs are undefined | The hd async handler still passes JSON, outgoing fetch, timeout, and eight concurrent requests. |
| Hosted Cloudflare / Vercel | Not deployed in this study | No production availability or bundler claim follows from these local tests. |

[JSPI](https://github.com/WebAssembly/js-promise-integration/blob/main/proposals/js-promise-integration/Overview.md) suspends a Wasm stack when a wrapped import returns a Promise.
It requires a corresponding promising export boundary.
An ordinary hd bang call already returns pending from its poll function, allowing JavaScript to await readiness outside Wasm.
That path does not need JSPI.

`block_on` is different when its operation truly waits for JavaScript I/O.
Busy waiting or `Atomics.wait` on that thread prevents the Promise from completing.
An immediately ready operation can still invoke `rt.block`: the control's final `println` does so once.
Rejecting the import or every invocation would therefore reject working handlers.

**Recommendation:** support the portable async path first, with a nonblocking ready-completion check inside `rt.block`.
If JSPI is available, a later adapter can await pending synchronous calls through wrapped entry exports.
Without JSPI, an actually pending synchronous wait fails that request with an explicit host error; it never spins.
This restriction must appear in the profile documentation and runtime error, not as a vague deployment hang.

The workerd probe also rejected `fetch(..., {redirect: "error"})`.
Its [Request documentation](https://developers.cloudflare.com/workers/runtime-apis/request/) specifies `follow` and `manual`, and gates disconnect notifications behind `enable_request_signal`.
A portable provider must implement hd's redirect policy using manual redirects and recheck each destination's grant.
The control instead refuses redirect fixtures and makes no full Http-provider conformance claim.

## Options

- **Chosen: one retained instance per adapter.** An ordinary `main!` joins worker loops over one state object; the SDK scopes each request's I/O to a lease.
- **Rejected: a single instance with a queue.** One slow upstream call delays unrelated requests, so it cannot overlap I/O.
- **Rejected: a fresh instance per request.** It repeats initialization and gives no warm state, and the owner chose one retained instance.
- **Rejected: a root table with request handles.** It adds runtime root ownership that the ordinary-`main!` shape does not need.

No option needs new hd syntax or the Component Model.

## Application Entry

The entry is an ordinary `main!`, such as `pub fn main!() -> void $ HttpServer + Clock`; the application declares its concrete row.
There is no `handle!` entry, no manifest `kind = "handler"`, and no HTTP-specific compiler entry convention.
The compiler and the entry rules are unchanged.

The SDK package supplies `HttpServer`, a capability trait whose requests and responses are boundary-safe `Request` and `Response` values.
Package-declared host capabilities deliver it ([#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51)); the capability's exact signature belongs to that design.
Routing is ordinary hd code over `Request`.

Return application errors as ordinary responses.
A handler may use `Result` internally and map its own validation error to 400, or its upstream failure to 502.
Uncaught panic or trap becomes a generic host 500 and a private diagnostic event.

Reusing `Request` and `Response` needs no new std builder or convenience combination.
Their public field constructors are the atoms; existing JSON encode/decode and body APIs cover common handlers.
Streaming types wait until the whole-body boundary is implemented and measured.
This document adds or retires no rule IDs.

## Bodies And HTTP Mapping

Whole bodies are sufficient for the four fixed use cases with explicit limits.
Streaming is not required for v1; large uploads, SSE, proxy streaming, WebSockets, and trailers are unsupported.
They need a later API with host-owned stream lifetimes and backpressure.
A handler cannot obtain streaming behavior by smuggling a host handle into `List[u8]`.

**Proposed initial limits:** one MiB incoming body, one MiB response body, 64 KiB total headers, and eight admitted requests per isolate.
Reject an oversized incoming body while reading its stream, before materializing an unbounded array.
Check declared Content-Length early, but still count actual bytes.
An excessive response is a host failure, logged privately and answered with a generic 500 before committing headers.

| Value | Mapping |
| --- | --- |
| Method | Standard spellings use existing Method variants; other valid tokens use `Other`; malformed method tokens fail before hd. |
| URL | Absolute platform URL preserved in `Request.url`; `query = []`, because this field means additional outgoing pairs. |
| Headers | Preserve available pairs and duplicate values; never split a combined header heuristically; platform-normalized information cannot be reconstructed. |
| Body | Raw bytes, not implicit UTF-8; application decoding controls 400 responses. |
| Redirects | Incoming `Request.redirects = 0`; this outgoing-only field does not authorize an upstream effect. |
| Response URL | Ignore `Response.url` on the downstream response; it is metadata for client responses. |
| Response status | Accept final HTTP statuses 200–599; reject invalid codes and unsupported informational responses. |
| HEAD / bodyless statuses | Return no body for HEAD, 204, 205, or 304; validate response headers before constructing the platform response. |

Node's raw request headers can preserve more information than a Fetch Headers object.
The common guarantee is preservation of the pairs the platform exposes, not original wire casing or ordering already lost upstream.
Set-Cookie needs the platform's dedicated multi-value access and append path.
Hop-by-hop headers, Content-Length, and transfer framing are host-owned; the adapter strips or recomputes them.

For local origin construction, the default listener uses its configured origin.
It does not trust arbitrary forwarded headers to choose an upstream identity.
Production adapters use the platform's absolute Request URL.
A future trusted-proxy option must specify that trust boundary explicitly.

## Concurrency, Cancellation, And State

One instance is retained and runs one `main!` whose children are worker loops.
Each admitted request has a lease: its ID, deadline, cancellation controller, and the outgoing operations it owns.
Children use ordinary `all!`, `race!`, `timeout!`, and cleanup semantics.
JavaScript schedules requests concurrently at suspension boundaries; this does not add shared-memory threads to hd.
A lease is released only after response bytes are copied out and cleanup finishes or is abandoned.

**Proposed host deadline:** 20 seconds from admission, with a 100 ms cleanup bound.
A handler's shorter `timeout!` remains an ordinary library operation whose error the handler can map to a response.
The host backstop is independent, including time spent reading input and producing output.
The platform may terminate sooner; cleanup is best effort under an external hard kill.

Disconnect requests graceful cancellation once, then abandonment at the cleanup deadline.
Each outstanding fetch owns an AbortController; cancellation or disposal releases it even if the Promise has completed but its result was not consumed.
Late Promise callbacks inspect the closed lease generation and cannot wake a later request.
The SDK dispatches each outgoing operation to the coroutine of the request that owns it, even when another request caused the poll.
No request starts detached background work or relies on `waitUntil` in this first version.

The admission limit is eight active requests; there is no unbounded waiting queue.
Additional requests receive 503 with Retry-After and an overload event.
This avoids retaining whole untrusted request bodies while waiting for memory.
A later configurable limit must still respect the host's aggregate memory budget.

Application state lives in one explicit state object that the worker loops share.
It lasts as long as the retained instance and is not durable: a restart or an isolate eviction loses it.
Interleaving at suspension points can invalidate a read-modify-write sequence, so the state object must be written without a suspension in between.
Use a granted external HTTP service for persistent data until platform storage capabilities receive their own design.

A CPU loop never returns to JavaScript, so a timer cannot preempt it inside the same isolate.
Node local hosting uses a bounded worker pool, one active request per worker, to enforce a hard deadline; edge hosting relies on platform CPU limits.
Each worker keeps one retained instance; terminate and replace a worker that misses its hard deadline.
Worker startup and dispatch overhead are not measured by the sub-millisecond instantiation control.

Do not advertise the 20-second cooperative deadline as a bound on arbitrary non-yielding edge guest code.
A future metering design is separate from request cancellation.

## Capabilities And Grants

Inbound delivery is the selected host's authority, not an outgoing Http grant.
A local listener binds the loopback address by default; binding another interface requires an explicit option.
Handler rows remain concrete host-capability rows.
Library code can still replace capabilities with `$.with` providers for deterministic testing.

| Capability | Common v1 profile |
| --- | --- |
| Http | Platform fetch adapter; explicit destination grants checked before effects and on every redirect. |
| Clock | Host monotonic timing and timers; wall-clock APIs keep existing contracts. |
| Random | Platform cryptographic randomness; preserve existing row and grant behavior. |
| Console | Structured request-associated logs; never part of the response wire channel. |
| Env | Only named string bindings explicitly granted by the manifest/deployment configuration. |
| Args / ConsoleInput | No user-process command line or interactive stdin in a deployed handler. |
| FsRead / FsWrite / Process / Net / Sys | Unbound in the common server profile, including local parity mode. |

Local parity mode uses the same common capability set even though Node could provide more.
This prevents a handler from working locally because it accidentally reaches the filesystem or spawns a process.
A separate Node-only profile can be designed later; it must not silently widen the portable profile.
Denials preserve existing error values such as Http's NotGranted result.

Grants come from existing package configuration and explicit local CLI overrides.
An absent outgoing Http grant means deny in the new server profile; this is a proposed profile policy, not a change to current CLI defaults.
Deployment writes an allowlist into adapter configuration, never into Wasm.
A platform's own restrictions still apply and can be narrower.

Secrets remain named platform Env bindings and are read only when granted.
Do not serialize deployment environment values into generated JavaScript, caches, error responses, or build diagnostics.
Expose string bindings only; Cloudflare service objects and storage bindings need separately typed capabilities later.
Destination checking must cover scheme, host, redirects, ports, and literal/local addresses consistently with the selected profile.

## Architecture

The request crosses the host boundary as a [structured value](../../crates/hd_run_wasmtime/DESIGN.md#174-encoding-structured-values), not JSON.
`Request` and `Response` are boundary-safe values; no reference, suspension, or host object crosses.
The exchange buffer is copied before another export or callback can overwrite it.
The exact exports and descriptors come from the package host capability design of #51; this record fixes only the shape.

At runtime: the adapter admits a request, reads its body within the limits, creates a lease, and hands the request to the `HttpServer` capability.
A worker loop in the retained instance takes it, runs the handler, and returns the `Response`.
The adapter copies the response out, closes the lease's operations and timers, and answers the platform.

Cancellation, deadlines and cleanup use the existing root protocol (`poll`, `wake`, `cancel`, `abandon`) per lease.
The SDK is the extension point for platform bindings: new bindings need typed capabilities and grant checks before hd code sees them.

A trap ends the retained instance and every active request on it.
The adapter answers each with a generic 500, and the SDK supervisor starts a new instance.
A panic in one handler is contained by the handler's own `Result` mapping or by the supervisor, not by instance isolation.
A platform hard kill can bypass hd cleanup; neither RC nor the lease makes an external effect transactional.

The build cache key includes compiler, source and dependency digests, target profile, and relevant grant-name configuration.
Secret values never enter it, so changing only deployment secrets does not recompile Wasm.

## Packaging And Deployment

The server SDK package ships its own packaging tasks; hd's core stays platform-neutral and gains no `hd build` flag.
Each task takes the built `build/<profile>/NAME.wasm` and emits the platform wrapper, the Wasm asset, host modules and platform config.
If a JS wrapper or host module the SDK needs is missing from the CLI's embedded host files, report the exact gap.
Deployment stays an explicit platform-tool step; a packaging task does not upload or request account credentials.

| Use case | Commands and observable result |
| --- | --- |
| U1 | `hd build`; `hd run deploy-cloudflare`; validate with a Wrangler dry run; deploy; POST JSON; receive typed JSON or 400. |
| U2 Node | Same source; `hd run deploy-vercel` emits `.vercel/output`; `vercel deploy --prebuilt`; send the same JSON request. |
| U2 Edge | The same task with an Edge option emits the static Wasm import and Edge config; platform dry run; deploy prebuilt output. |
| U3 | `hd run` of the application starts the local Node listener; a request awaits an upstream call; denied destinations return the existing Http error to hd. |
| U4 | Call `/timeout`; read 504; inspect timeout and cleanup events; disconnect a slow request and confirm its fetch is aborted. |

Grants come from existing package configuration and explicit local CLI overrides.
The listener is local only and never changes production platform routing.
A native Wasmtime listener is not part of v1; native tests exercise the typed handler and cancellation through a test driver.
Build artifacts are written under the selected output directory with atomic replacement after successful validation.
A task never writes above the selected package or overwrites unrelated platform files silently.

| Artifact | Contents |
| --- | --- |
| Cloudflare | Entry ES module, static Wasm module, common host graph, Wrangler configuration with named bindings and compatibility settings. |
| Vercel Node | Build Output version 3 config, catch-all route, Node function directory, `.vc-config.json`, JS entry and Wasm. |
| Vercel Edge | Build Output version 3 config, Edge function directory, `runtime: "edge"`, entrypoint and explicit env names, static Wasm import. |
| Local | Cached Wasm plus Node listener adapter; request bodies stay in memory only for the request lifetime. |

The [Build Output API's function primitives](https://vercel.com/docs/build-output-api/primitives) distinguish runtime and entrypoint configuration.
Generated Edge packaging must be tested with Vercel's actual Wasm-module handling before shipping that target.
A local VM test does not establish that a raw static import will be bundled correctly by the hosted build pipeline.
Platform build validation is an explicit acceptance gate for the packaging slices.

Server events are versioned SDK output, not an application logging schema.
Request IDs, status, elapsed time, cancellation or abandonment, and sanitized source frames are useful by default.
Headers, bodies, Env values, and upstream credentials are excluded from default events.
Generated files must work on macOS, Linux, and Windows without shell-specific execution.

## Semantics And Counterexamples

| Existing rule area | Required handling | A naive implementation fails when… |
| --- | --- | --- |
| [Value/reference semantics](../../spec/lang/13-values-and-references.md) | Copy boundary values; no cross-instance references. | A Response stores a pointer into a discarded request heap. |
| [Entry points](../../spec/lang/10-modules.md#entry-point) | Keep `main!` as the only entry. | A checked public `handle!` is assumed to be an exported request handler. |
| [Program instances](../../spec/lang/10-modules.md#instances-and-threads) | One retained instance with one root; per-request state lives in the lease. | A second request overwrites the first request's lease state. |
| [Initialization](../../spec/lang/10-modules.md#initialization-order) | Initialize the retained instance once under existing requirement restrictions. | A state object is advertised as durable but is lost on restart. |
| [Boundary-safe values](../../spec/lang/10-modules.md#boundary-safe-values) | Use the existing Request/Response value encoding. | A platform Request object or mutable reference is passed as if it were an hd value. |
| [Requirement rows](../../spec/lang/10-modules.md#entry-point) | Explicit row; validate every imported capability against the profile. | Local Process access succeeds although a deployment cannot bind it. |
| [Http query semantics](../../spec/std/http.md#request-builder) | Preserve the full URL and leave incoming query pairs empty. | Forwarding appends the original query twice. |
| [Cancellation](../../spec/std/task.md) | Cancel the request root; bound cleanup; abandon only after the bound. | Disconnect drops the instance before `defer` or dispose runs. |
| [Panics](../../spec/lang/06-control-flow.md) | A trap ends the instance and answers every active request with 500; the supervisor restarts it. | A poisoned instance keeps serving with partially mutated state. |
| [Header pairs](../../spec/std/http.md#requests-and-responses) | Preserve exposed multiplicity and host framing. | Combining Set-Cookie values changes cookie meaning. |

Generics remain ordinary body-level generics; the host boundary itself is monomorphic.
Closures and their captures remain inside the instance.
Suspension does not extend a request beyond its lease, and copied `&mut` references are not exclusive locks.
Trait providers chosen through `$.with` retain normal lexical/dynamic behavior within that request.

The concrete counterexamples are also executable controls where today's surface permits them.
The gap package demonstrates that no registration exists; the query and invalid-UTF-8 tests demonstrate mapping; the blocking route demonstrates event-loop waiting.
Lease and cross-request behavior of the SDK is not claimed as implemented.
The future codec and multi-request fixtures must test those boundaries directly.

## Cost And Measurements

These are measurements of the current control, not estimates for completed handler support.
The control includes JSON/serde, timeout, outgoing Http, and an explicit blocking negative route.
Its debug build is intentionally not a minimal hello program.
Measurements must be repeated on the actual generated handler artifact before accepting an implementation.

### Current Reference Measurements

| Quantity | Observed value and scope |
| --- | --- |
| Control Wasm | 317,485 bytes; gzip level 9: 113,289 bytes. |
| Defined functions / code section | 704 functions; 270,004 section bytes. |
| Initial memory | Two private memories, each minimum one 64 KiB page; only exchange memory is normally exported. |
| Small completed request | Measurement-only export exposes heap: 64 KiB heap plus 64 KiB exchange after completion. |
| Fresh instantiation | Node 24.19.0, same compiled module, 100 sequential trials: median 0.046 ms, p95 0.070 ms. |
| Current JS graph | Six existing host modules plus the bridge: 116,692 bytes before bundling. |
| Size proxy | Concatenated JS plus Wasm: 434,177 bytes raw, 149,310 bytes gzip; not a Wrangler/Vercel admission measurement. |
| Concurrent fetch | Eight active upstream requests observed on Node, workerd, and the Edge VM; Node control completed the group in about 40 ms. |

The 64 KiB heap figure covers a small request, not an upper bound after memory growth.
Active requests hold copied bodies.
At eight requests and one MiB bodies, multiple input/output/exchange copies can dominate the initial one MiB of aggregate memory.
The implementation needs an aggregate byte budget, not merely a per-body check.

The SDK profile sets the memory maxima of the retained instance and the bounded host readers, including outgoing Http responses.
A guest allocation failure ends the instance; exceeding an isolate-wide platform limit can also fail sibling requests.

The remaining platform memory headroom must cover JavaScript objects, compiled code, engine overhead, and transient read chunks.
These are proposed limits, not a measured proof of Cloudflare's peak memory use.
Profile maximum-sized bodies with eight overlapping requests before acceptance; lower admission or body limits if peak isolate usage exceeds 100 MiB.
A runtime that cannot enforce or measure the required quota must not advertise the portable server profile.

### Bottom-Up Implementation Budget

The budget for the withdrawn handler entry, with its compiler entry lowering and per-request instances, no longer applies.
The SDK package, its host capability under #51, and the adapters are budgeted bottom-up in their own issues, before code.
Each component stops for review above 1.5 times its allocated added-line budget.

### Compile Time, Runtime, And Compiler Footprint

The prototype reuses a frozen compiler with zero source difference across crates, std, hosts, and Cargo inputs from this branch's base.
It does not measure a new compiler path's compile-time overhead because that path does not exist.
An implementation must compare interleaved baseline/candidate builds for hello and this handler, keeping all trials and source hashes.
Report parse/check/mono/emit time, total time, Wasm functions/code bytes, and request initialization separately.

Command hello must pay zero handler exports, helper functions, or imports when no HTTP SDK is linked.
This proposal changes no executable compiler bytes and makes no new hello size claim.
The implementation's web compiler gzip gate remains a hard two MB cap; added server support must be measured against it.
The much smaller deployable application bundle is a different artifact and a different limit.

## Stress Test

This applies the [stress-test method](../../.agents/skills/stress-test/SKILL.md) to the proposed boundary.
The surface under test is the monomorphic handler, the retained instance with request leases, whole-body limits, explicit grants, and existing task cleanup.
Translations are small behavioral ports, not compatibility implementations of the named libraries.
“Control” means executed today; “required” means a test for the missing implementation, not a green result.

| Case / real shape | Translation and generated behavior | Verdict and evidence |
| --- | --- | --- |
| S1: Hono JSON route | Typed Name decoding and Greeting serialization in `json_response`; ordinary hd body calls. | Control passes on all three JS hosts. |
| S2: Axum malformed JSON rejection | Invalid JSON returns 400 through an explicit match. | Node control passes; no automatic error-to-status magic. |
| S3: byte-oriented HTTP body | Invalid UTF-8 reaches bytes, then application's decoder rejects it. | Node control returns 400; adapters must not decode eagerly. |
| S4: fetch proxy | `get!` awaits one local upstream, returns its Response value. | All three controls pass without JSPI; redirect/provider completeness remains required. |
| S5: Tower timeout | `timeout!(1ms, cleanup=1ms)` wraps a 50 ms sleep; explicit error branch returns 504. | All three controls pass. |
| S6: Go request-context cancellation | Abort a live request; cancel root; abort fetch once. | Node control observes status 130 and one abort; downstream signal parity remains required. |
| S7: concurrent independent handlers | Eight requests each await the same 30 ms fixture. | All three controls observe eight simultaneous upstream requests. |
| S8: URL forwarding | Preserve `?x=1&x=2`, with no duplicate incoming query fields. | Node control preserves the exact URL string. |
| S9: synchronous foreign-style I/O | Explicit `block_on(get(...))` cannot let fetch complete under a blocking JS import. | Ready-only control refuses the actual pending wait; full JSPI hd driver remains required. |
| S10: Express cookie responses | Return two Set-Cookie pairs, including an Expires comma. | Required adapter test; never split a combined string on commas. |
| S11: net/http large upload | Chunked body crosses one MiB despite a smaller or absent declared length. | Required bounded-reader test; whole-body `arrayBuffer()` alone fails this case. |
| S12: HEAD and no-content response | Handler returns bytes with HEAD/204; host emits no body and correct framing. | Required mapping test; direct Fetch Response construction can throw for bodyless statuses. |
| S13: shared cache / counter | One explicit state object increments across requests. | Control passes on Node and workerd (counts 1, 2, 3); the state is not durable. |
| S14: CPU loop and panic | Non-yielding loop or trap during handling. | A trap ends every active request on the instance; supervision and Node hard timeouts required; the edge cooperative timer cannot bound CPU loops. |
| S15: redirect to denied service | Allowed upstream redirects to metadata/local address. | Required manual-redirect grant test; platform auto-follow would bypass the intended check. |
| S16: wake after disposal | Fetch completes after timeout/abandon and after its slot is reused. | Required generation test; late wake must not touch another instance. |

The prototype's exact translations live in the linked hd program and adapter scripts.
They approximate JSON handlers, context cancellation, timeout middleware, and proxying using existing hd APIs.
The remaining cases exercise missing transport code, so presenting them as checked hd examples would hide the actual gap.
Each maps to a proposed implementation fixture below.

The highest-risk failures are unbounded buffering, delayed cancellation, and capability bypass on redirects.
They can silently affect resources or authority despite correct handler typing.
The retained instance and its leases do not solve those host problems.
The lease keeps one request's outgoing operations from running in another request's platform context.

## Phasing

Each slice targets roughly one hour and leaves existing command behavior green.
A slice that needs protected compiler/runtime files first receives explicit scope approval on its implementation issue.
The slices follow the owner's order: #51's minimum, then the adapters.

| Slice | Deliverable | Focused acceptance evidence |
| --- | --- | --- |
| 0 | #51's minimum: package-declared host capabilities. | Owned by #51; the SDK's `HttpServer` capability builds on it. |
| 1 | `HttpServer` capability and SDK worker loops over an ordinary `main!`. | Typed request/response round trip; malformed lengths; one retained instance serves sequential requests. |
| 2 | Lease-scoped I/O dispatcher and cleanup. | Pending/wake/cancel/abandon, late completion, trap, no operation leaks; adversarial request interleaving. |
| 3 | Fetch provider over platform APIs. | Existing Http cases for redirects, grants, methods, errors and body preservation on Node/workerd/Edge VM. |
| 4 | Bounded request/response adapter and admission. | Chunked limit, header limit, cookies, HEAD/204/205/304, overload and byte accounting, eight concurrent requests. |
| 5 | Node development listener and supervision. | Local U3/U4, disconnect, process shutdown, CPU-loop termination; Windows/macOS/Linux targeted coverage. |
| 6 | `deploy-cloudflare` task. | Static Wasm dry-run bundle, local workerd U1-U4, request-signal compatibility, measured size/startup/memory. |
| 7 | `deploy-vercel` task, Node. | Build Output validation, local function adapter U1-U4, payload cap and deadline/error behavior. |
| 8 | `deploy-vercel` task, Edge. | Actual platform build validation plus Edge VM U1-U4 without JSPI; sanitized pending-sync-wait error. |
| 9 | Optional JSPI hd driver. | workerd pending `block_on` fetch success, cancellation during suspension, ready calls, traps, fallback host without JSPI. |

Spec rules and fixtures ship with the implementing code, not in a detached final semantics-only pass.
No full local test suite is required by this record; each implementing issue names its focused gates.
CI and the owner verify integration on current main.

## Recommendation And Approval

**Decided (owner, 2026-10-24).** An application is an ordinary `main!`.
One Wasm instance is retained per adapter, with one explicit state object and a lease per request.
`HttpServer` is owned by the server SDK and delivered through package-declared host capabilities (#51), which come first.
Packaging is SDK tasks such as `hd run deploy-cloudflare` and `hd run deploy-vercel`, with no `hd build` flag.

Whole bodies with explicit admission and byte budgets, common capability grants, and the existing single-root cleanup model stay as proposed.
The normal async path is independent of JSPI; optional JSPI support is a separately tested host feature.
Streaming and platform-specific services stay in later designs.

Open approvals: the SDK boundary, the deployment profile, the lifecycle and resource policies (#806), the explicit limits and overload policy, the server-profile grants, and the pending-sync-wait behavior.
The local VM and workerd results do not substitute for platform build validation or deployed acceptance tests.

## Prototype And Parse Log

Source base: `adaeaea81b15d6ec6c94ba436cab33f913b4a122`.
The compiler was built from `2e89fcb0bab43c22c009bd92871b35d1562b70fa`; all compiler/std/host/Cargo inputs compare byte-identically with this base.
The [prototype README](http-server/prototype/README.md) gives commands, versions, limitations, and scratch-directory requirements.
Tools were downloaded as integrity-verified archives into scratch; no package-manager installation, deployment, credential access, or production workflow ran.

| Artifact | Evidence |
| --- | --- |
| Ordinary typed handler / gap | Parses and checks; proposed manifest warns exactly as quoted; built export inventory confirms no handler. |
| Complete control | Checks without errors/warnings; executed on Node and local workerd, plus host code evaluated inside the Edge VM. |
| JSON and timeout | Exact 200 JSON / 400 invalid input / 504 deadline responses; declared subset per host shown above. |
| Cancellation | Node direct-control abort requests graceful cancellation and observes one fetch abort; not a production disconnect guarantee. |
| JSPI feature control | Real Promise suspension returns 7 in workerd; local Edge VM exposes neither JSPI function. |
| Measurements | Same compiled module for instantiation trials; private heap exported only in measurement module; gzip proxy method stated. |

Parsing alone establishes syntax, not checking or runtime behavior.
The proposed ABI, CLI flags, packaging, budgets, and unimplemented stress cases have no current compiler success claim.
The Args/Console bridge is intentionally not recommended for deployment.

The final prototype sources and extracted hd block are checked independently; their transport bridge remains experimental.
Ordinary CLI controls also run JSON, timeout, fetch, and synchronous blocking fetch on Node and native Wasmtime.
Those eight successful runs verify the typed program under existing hosts, not a native server listener.

## Issue And Domain Review

| Issue item or checklist | Section / explicit scope |
| --- | --- |
| One handler, existing Request/Response, limited registration | Application Entry; Architecture. |
| Node local server; Workers; Vercel Node and Edge | Packaging And Deployment; Platform Limits And JSPI. |
| Bundle limits and static Wasm | Platform Limits And JSPI; Cost And Measurements; packaging acceptance slices. |
| Request concurrency, cancellation, state, minimal #806 | Concurrency, Cancellation, And State; Options On The Same Use Cases. |
| Whole bodies; streaming requirement | Bodies And HTTP Mapping; streaming explicitly deferred for the fixed cases. |
| Capability rows and platform grants | Capabilities And Grants. |
| Core-Wasm stays, no Component Model reversal | Architecture; Prior Art distinguishes incompatible host models. |
| Library/glue before language changes | Options; no new syntax or std combinator; one compiler-known entry convention. |
| Four fixed end-to-end scenarios | Problem And Fixed Use Cases; workflows; controls and missing packaging gates. |
| Spin, wasmCloud, Fastly, Rust worker prior art | Four dedicated Prior Art sections with design/interface/coverage/limits. |
| workerd / Vercel Edge JSPI | Platform Limits And JSPI; actual local runtime probes and production uncertainty separated. |
| Data, algorithm, cache, tests | Architecture; driver pseudocode, cache keys, lease fields, Phasing. |
| Stdlib API shape, rows, error types, receivers, bounds | Application Entry; value parameter/return, monomorphic boundary, explicit row, response errors; existing atoms reused. |
| Stdlib performance/security/prior art | Cost And Measurements; Capabilities And Grants; Prior Art and Stress Test. |
| CLI commands/output/schema/status/config/scope/platforms | Packaging And Deployment; explicit unsupported native listener and platform build gate. |
| Runtime memory/RC/ABI/cleanup/parity | Architecture; Concurrency; Cost; Node/native wrapper tests in slice 2; production JS-only scope explicit. |
| Runtime failure, observability, grants, footprint | Cache/Failure; Output; Capabilities; measurements and future hello identity check. |
| Stress cases from real libraries and adversarial inputs | Stress Test; executed controls distinguished from required future tests. |
| Bottom-up cost and stop condition | Cost And Measurements; exact existing line counts, estimated added lines, 1.5× component stop. |
| Owner approval before spec/implementation; one-hour jobs | Recommendation And Approval; Phasing. |
| Extension points | Cache, Extensions, And Failure Isolation; typed capabilities, streaming ABI, stateful service deferred. |
