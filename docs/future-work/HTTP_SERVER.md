# HTTP Server Handlers

Status: design proposal, 2026-10-23; domains: stdlib, CLI and tooling, runtime and host.
Nothing here is accepted behavior or a specification change.
This answers [#266](https://github.com/HerringtonDarkholme/hd-lang/issues/266) under its [revised scope](https://github.com/HerringtonDarkholme/hd-lang/issues/266#issuecomment-6375211580).
It continues the unreviewed [research notes at 187e5a93e](https://github.com/HerringtonDarkholme/hd-lang/blob/187e5a93e/docs/future-work/http-server/NOTES.md).

The records under review are [Http](../../spec/std/http.md), [Task](../../spec/std/task.md), [entry points](../../spec/lang/10-modules.md#entry-point), and [program instances](../../spec/lang/10-modules.md#instances-and-threads).
[CLI executable selection](../../spec/cli/command-line.md#executables) also needs extension.
General embedding [#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51) stays deferred.
The larger application model [#806](https://github.com/HerringtonDarkholme/hd-lang/issues/806) remains open beyond the minimal lifetime described here.

## Contents

- [Problem And Fixed Use Cases](#problem-and-fixed-use-cases)
- [What Works Today And The Registration Gap](#what-works-today-and-the-registration-gap)
- [Prior Art](#prior-art)
- [Platform Limits And JSPI](#platform-limits-and-jspi)
- [Options On The Same Use Cases](#options-on-the-same-use-cases)
- [A Handler Entry](#a-handler-entry)
- [Bodies And HTTP Mapping](#bodies-and-http-mapping)
- [Concurrency, Cancellation, And State](#concurrency-cancellation-and-state)
- [Capabilities And Grants](#capabilities-and-grants)
- [Architecture And Core-Wasm ABI](#architecture-and-core-wasm-abi)
- [CLI And Deployment Workflows](#cli-and-deployment-workflows)
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

## What Works Today And The Registration Gap

The ordinary signature already checks:

```text
use std.http.{Request, Response}

pub fn handle!(request: Request) -> Response:
    Response { status: 200, body: request.url.to_utf8() }
```

A public function named `handle!` is currently an ordinary function.
[Entry rules](../../spec/lang/10-modules.md#entry-point) recognize parameterless `main` or `main!`.
The backend's [Entry enum](../../crates/hd_wasm_linear/src/lib.rs) likewise describes existing program, script, test, and session entry forms.
The single runtime root cannot represent overlapping handlers in one instance.

The [minimal gap package](http-server/prototype/gap/hd.toml) supplies this proposed manifest:

```toml
[package]
name = "handler-gap"

[[executable]]
name = "handler"
module = "handler"
kind = "handler" # proposed key; ignored today
```

The current checker and builder both report this exact warning:

```console
unknown-manifest-key: `executable.0.kind` has no meaning in `hd.toml`, so it is ignored
```

Building succeeds as an empty script: 402 bytes, no imports, exports `hd.init`, `hd.poll`, and `hd.x`.
There is no handler export and no request delivered to the ordinary function.
Successful checking is therefore not evidence that handler registration works.

The [runnable control](http-server/prototype/app/src/main.hd) calls `handle!` from `main!` instead.
It exchanges a JSON envelope through Args and Console, solely to exercise today's typed handler and runtime.
Production uses the typed ABI below; application logging must never be interpreted as a response.
No compiler, stdlib, host, or spec implementation changes accompany this proposal.

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
A root table would move hd toward the latter, including its trap and shared-state obligations.
That is a valid later #806 design, not an unavoidable prerequisite for stateless handlers.
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

## Options On The Same Use Cases

### Smallest: One Instance And A Queue

Keep the single root and admit one request at a time.
This reuses current runtime ownership and allows warm state, but one slow U3 request delays unrelated U1/U2 requests.
A CPU loop also blocks cancellation and the entire queue.
It cannot provide useful overlapping request handling on an isolate.

This is a useful control, not the recommended server.
A queue still needs bounded length, deadlines measured before admission, and a defined overload response.
Removing a root table does not remove those server obligations.

### Fresh Instance Per Request, Shared Compiled Module

Compile or statically import one module, then instantiate it separately for each admitted request.
Each instance retains the existing single root and its own host-operation table.
Its memories have explicit profile maxima; request isolation does not provide process or isolate isolation.

U1/U2 need the same typed handler; U3 awaits platform fetch; U4 cancels only its own instance.
No request can retain an hd reference into another request's heap.

This repeats initialization and allocates memories per request.
It deliberately provides no warm hd globals or connection pool shared between requests.
The host may cache immutable compiled code and configuration, while durable data lives in an external service.
Admission and body limits bound concurrent memory rather than hoping the isolate limit is sufficient.

### One Reusable Instance With A Root Table

Add request handles to root ownership, poll, wake, cancel, result storage, and cleanup.
Every request operation needs association with its root; a trapped instance invalidates every active request.
U1–U4 can then share application caches and overlap I/O with less per-request initialization.
That is attractive when actual workload measurements justify it.

It also creates shared mutable hd state across suspension points.
An async interleaving can invalidate a read-modify-write sequence even without threads.
The design must prevent stale handles, cross-request cancellation, retained secret data, and reentrant entry while another root is active.
Those costs belong in #806 before this becomes the default.

| Case | Single queued instance | Fresh instance | Shared roots |
| --- | --- | --- | --- |
| U1/U2 JSON | Works; later requests queue. | Works; state starts fresh. | Works; shared-state policy required. |
| U3 outgoing HTTP | Serial bottleneck during upstream waits. | Concurrent requests demonstrated in controls. | Concurrent roots need new runtime machinery. |
| U4 timeout/disconnect | Cancels current request; queue policy still needed. | Current single-root cleanup applies independently. | Must address exactly one root; trap affects siblings. |
| Across-request state | Warm but not durable. | No hd sharing; external state explicit. | Warm but not durable; supervision required. |
| Costliest new mechanism | Entry registration. | Entry registration. | Entry registration plus runtime root ownership. |

No option needs new hd syntax or the Component Model.
The fresh-instance option adds fewer mechanisms than shared roots while satisfying the four fixed cases.
Its memory and initialization costs must still pass the measured acceptance conditions below.

## A Handler Entry

**Proposed complete application signature:** `pub fn handle!(request: Request) -> Response $ Http + Clock`.
The application explicitly declares its concrete row; it has no generic row parameter.
There are no generic parameters, borrowed boundary values, extra arguments, implicit context object, or overloads.
The selected entry must be public, top-level, named `handle!`, and return exactly `std.http.Response`.

The manifest's proposed `kind = "handler"` selects this convention for one executable module.
Omitting `kind` preserves existing executable behavior.
The handler module has no executable `main`; an explicit handler selection determines its entry.
Unknown or inconsistent entry declarations produce a diagnostic before output is written.

Return application errors as ordinary responses.
A handler may use `Result` internally and map its own validation error to 400, or its upstream failure to 502.
The entry itself returns `Response`, avoiding a new server error hierarchy or conflating errors with client `HttpError`.
Uncaught panic or trap becomes a generic host 500 and a private diagnostic event.

Reusing `Request` and `Response` needs no new std builder or convenience combination.
Their public field constructors are the atoms; existing JSON encode/decode and body APIs cover common handlers.
The 80% path is one function plus one manifest entry.
Streaming types wait until the whole-body boundary is implemented and measured.

The compiler necessarily knows this entry convention.
Future entry typing and registration rules belong in the language tier; executable selection and artifacts belong in the CLI tier.
Any ordinary server library added later belongs in the stdlib tier if it passes the tier test.
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

An admitted request owns one instance, one driver, one operation table, and one cancellation controller.
Its children continue to use ordinary `all!`, `race!`, `timeout!`, and cleanup semantics.
JavaScript schedules requests concurrently at suspension boundaries; this does not add shared-memory threads to hd.
A slot is released only after response bytes are copied out and cleanup finishes or is abandoned.

**Proposed host deadline:** 20 seconds from admission, with a 100 ms cleanup bound.
A handler's shorter `timeout!` remains an ordinary library operation whose error the handler can map to a response.
The host backstop is independent, including time spent reading input and producing output.
The platform may terminate sooner; cleanup is best effort under an external hard kill.

Disconnect requests graceful cancellation once, then abandonment at the cleanup deadline.
Each outstanding fetch owns an AbortController; cancellation or disposal releases it even if the Promise has completed but its result was not consumed.
Late Promise callbacks inspect the closed request generation and cannot wake a replacement request.
No request starts detached background work or relies on `waitUntil` in this first version.

The admission limit is eight active requests; there is no unbounded waiting queue.
Additional requests receive 503 with Retry-After and an overload event.
This avoids retaining whole untrusted request bodies while waiting for memory.
A later configurable limit must still respect the host's aggregate memory budget.

Top-level bindings and references exist for one request only.
No application cache or counter survives by promise across requests; even a reusable host isolate is not durable storage.
The compiled module and immutable grant configuration may survive in JavaScript.
Use a granted external HTTP service for persistent data until platform storage capabilities receive their own design.

A CPU loop never returns to JavaScript, so a timer cannot preempt it inside the same isolate.
Node local hosting uses a bounded worker pool, one active request per worker, to enforce a hard deadline; edge hosting relies on platform CPU limits.
Workers reuse compiled modules but create fresh instances; terminate and replace a worker that misses its hard deadline.
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

## Architecture And Core-Wasm ABI

The request ABI reuses [structured-value encoding](../../crates/hd_run_wasmtime/DESIGN.md#174-encoding-structured-values), not JSON.
Request and Response are boundary-safe values; no reference, suspension, or host object crosses the boundary.
The exchange buffer is copied before another export or callback can overwrite it.
A frame owns the decoded Request and the eventual Response under ordinary value and RC rules.

| Proposed export | Contract |
| --- | --- |
| `hd.init()` | Existing module initialization; exactly once for the fresh request instance. |
| `hd.serve.start(request_len: i32)` | Decode one Request from `hd.x`; create its cold root; reject repeated start. |
| `hd.poll(ctx: i32) -> i32` | Existing pending/completed/cancelled status protocol; context zero belongs to this request. |
| `hd.wake(count: i32)` | Existing completion-handle delivery within this instance. |
| `hd.cancel()` / `hd.abandon()` | Existing root cleanup protocol. |
| `hd.serve.finish() -> i32` | After successful completion, encode Response into `hd.x`, release the guest result, return length; exactly once. |
| `hd.x` | Existing exchange memory; no host pointers or shared heap. |

The descriptor version is an explicit `hd.server` custom section containing ABI version 1 and the handler's capability row.
The adapter validates it before accepting requests.
Versioning avoids interpreting a command module as a handler because a few export names happen to match.
Inputs are length-checked before allocation; malformed encoded values are host contract failures, not hd application errors.

### Components And Data Structures

| Component | Fields / reused facts |
| --- | --- |
| `Entry::Handler` | Handler DefId and instance key, init keys, Request decoder and Response encoder keys; reuse existing entry lowering. |
| `HandlerDescriptor` | ABI version, row trait IDs, module digest; derives from checked signature and manifest selection. |
| `ServerConfig` | Target, executable, allowed destinations and Env names, body/header limits, concurrency, deadline, cleanup bound. |
| `RequestLease` | Request ID, generation, instance, host, signal listener, slot token, deadline timer, closed flag. |
| `PendingFetch` | Operation handle, AbortController, ready flag, result/error, owning lease generation. |
| `ModuleCache` | Immutable compiled module by source/build digest and profile; no instance or request value retained. |

Flow: select executable → validate typed entry → monomorphize its ordinary body → emit entry wrappers and descriptor → package adapter.
At runtime: admit → bounded body read → instantiate → initialize → encode/start → poll/wake → copy result → close lease → platform response.
No root table, shared heap, new scheduler, or new std task primitive is introduced.
Fresh instances preserve the existing cancellation tree rather than adding a second task system.

### Driver Pseudocode

```rust
async fn serve(platform_request, module, config) -> PlatformResponse {
    let slot = match config.slots.try_acquire() {
        Some(slot) => slot,
        None => return overload_response(),
    };
    let mut lease = RequestLease::new(slot, platform_request.signal(), config);
    let result = async {
        let request = read_bounded_request(platform_request, lease.deadline).await?;
        let host = portable_host(config.grants, lease.generation);
        let instance = instantiate(module, host.imports());
        lease.attach(instance, host);
        lease.call_init().await?;
        lease.copy_request_into_exchange(request)?;
        lease.call_start().await?;
        loop {
            match lease.call_poll(0).await? {
                Complete => break,
                Cancelled | Abandoned => return Err(RequestCancelled),
                Pending => {
                    match lease.wait_for_ready_or_cancel().await {
                        Ready(handles) => lease.call_wake(handles).await?,
                        Stop => lease.call_cancel_once().await?,
                        CleanupExpired => lease.call_abandon_once().await?,
                    }
                }
            }
        }
        let length = lease.call_finish().await?;
        let response = lease.copy_response_from_exchange(length)?;
        validate_downstream_response(response, config)
    }.await;
    lease.close_all_operations_and_timers();
    lease.release_instance_and_slot();
    map_result_to_platform_response(result)
}
```

The calls are asynchronous wrappers even when a normal export completes synchronously.
That permits an optional JSPI implementation without changing the HTTP mapping or application signature.
The no-JSPI wrapper completes ready `rt.block` operations and refuses only a truly pending synchronous wait.
It must not retain a stale exchange-buffer view across memory growth or suspension.

### Cache, Extensions, And Failure Isolation

The build cache key includes compiler, source/dependency digests, target profile, ABI version, and relevant grant-name configuration.
Secret values never enter it.
Changing only deployment secrets does not recompile Wasm; changing a row or adapter ABI does.
Package and workspace selection reuse existing executable selection rather than scanning arbitrary modules for public handlers.

A panic or trap discards one instance and aborts its host operations.
The compiled module remains reusable; another request does not inherit partially mutated globals.
Catchable allocation failures and invalid responses produce generic 500s with private request-associated events.
A platform hard kill can bypass hd cleanup; neither RC nor a fresh instance makes an external effect transactional.

The first extension seam is the platform adapter, not user-provided arbitrary imports.
New platform bindings need typed capabilities and grant checks before being exposed to hd.
A future streaming boundary may add host handles and resource lifetime rules under a new ABI version.
A future stateful service may choose shared roots after #806, without changing stateless handlers' guarantees silently.

## CLI And Deployment Workflows

These are proposed commands and generated files, not commands available today.
Keep deployment as an explicit platform-tool step; `hd build` does not upload or request account credentials.

| Use case | Commands and observable result |
| --- | --- |
| U1 | Declare handler executable; `hd build api --target cloudflare`; validate with Wrangler dry-run; deploy generated Worker; POST JSON; receive typed JSON or 400. |
| U2 Node | Same source; `hd build api --target vercel-node`; inspect `.vercel/output`; `vercel deploy --prebuilt`; send the same JSON request. |
| U2 Edge | `hd build api --target vercel-edge`; inspect static Wasm import and Edge config; platform dry-run/build; deploy prebuilt output. |
| U3 | `hd run api --listen 127.0.0.1:3000 --cap Http=example.test`; listener prints URL; request awaits upstream; denied destinations return the existing Http error to hd. |
| U4 | Call `/timeout`; read 504; inspect timeout and cleanup events; disconnect a slow request and confirm its fetch is aborted. |

The proposed `--target` values select an artifact profile, not an hd language target or Component Model world.
Existing executable `--engine` behavior remains unchanged for command programs.
A handler defaults to the Node development server; requesting a native Wasmtime listener in v1 is an explicit unsupported selection.
Native tests still exercise the typed handler, codecs, and cancellation through a test driver.

**Configuration precedence:** existing manifest grants, then explicit local `--cap`; build target from explicit `--target` or a handler's configured default.
Reject mutually exclusive output targets and a handler combined with command-only stdin/argument options.
Use the selected package's executable in a workspace; require explicit selection when ambiguous.
The listener flag is local only and never changes production platform routing.

Initial operational limits above are profile constants, recorded in emitted metadata.
Do not add an unbounded list of tunables before measuring them.
Build artifacts are written under the selected output directory with atomic replacement after successful validation.
Never write above the selected package or overwrite unrelated platform files silently.

| Artifact | Contents |
| --- | --- |
| Cloudflare | Entry ES module, static Wasm module, common host graph, Wrangler configuration with named bindings and compatibility settings. |
| Vercel Node | Build Output version 3 config, catch-all route, Node function directory, `.vc-config.json`, JS entry and Wasm. |
| Vercel Edge | Build Output version 3 config, Edge function directory, `runtime: "edge"`, entrypoint and explicit env names, static Wasm import. |
| Local | Cached Wasm plus Node listener adapter; request bodies stay in memory only for the request lifetime. |

The [Build Output API's function primitives](https://vercel.com/docs/build-output-api/primitives) distinguish runtime and entrypoint configuration.
Generated Edge packaging must be tested with Vercel's actual Wasm-module handling before shipping that target.
A local VM test does not establish that a raw static import will be bundled correctly by the hosted build pipeline.
Platform build validation is an explicit acceptance gate for the implementation slice.

### Output And Exit Statuses

Human output reports the selected executable, target, output directory, size, and local listener URL where applicable.
JSON mode emits one record per build or server event; it never mixes application response bodies into that stream.
Preserve existing diagnostic records and final build summaries.

```json
{"kind":"server-listening","executable":"api","url":"http://127.0.0.1:3000"}
{"kind":"server-request","id":"17","status":504,"duration_ms":12,"outcome":"handler-response"}
{"kind":"server-error","id":"18","reason":"unsupported-sync-wait","message":"pending block_on requires JSPI on this host"}
```

Event fields are versioned CLI output, not an application logging schema.
Request IDs, status, elapsed time, cancellation/abandonment, and sanitized source frames are useful by default.
Headers, bodies, Env values, and upstream credentials are excluded from default events.
Console output carries the request ID and remains separate from transport response data.

Build success exits 0; invalid source/configuration uses existing failure status 101.
A listener startup or bind failure reports a diagnostic and exits 101.
An individual 400/500/504 response does not terminate the local server.
Graceful process termination stops admission, cancels active requests, applies the cleanup bound, then closes the listener.

Generated files must work on macOS, Linux, and Windows without shell-specific execution.
Node's local hard-timeout supervisor and platform adapters receive separate focused tests.
No native-host behavior is inferred from a Node-only pass.
The absence of a native production listener is explicit v1 scope, not implied engine parity.

## Semantics And Counterexamples

| Existing rule area | Required handling | A naive implementation fails when… |
| --- | --- | --- |
| [Value/reference semantics](../../spec/lang/13-values-and-references.md) | Copy boundary values; no cross-instance references. | A Response stores a pointer into a discarded request heap. |
| [Entry points](../../spec/lang/10-modules.md#entry-point) | Add one explicit entry kind. | A checked public `handle!` is assumed to be an exported request handler. |
| [Program instances](../../spec/lang/10-modules.md#instances-and-threads) | Separate mutable globals and drivers. | A second request overwrites the first instance's single root. |
| [Initialization](../../spec/lang/10-modules.md#initialization-order) | Initialize each instance once under existing requirement restrictions. | A top-level cache is advertised as persistent but is rebuilt for every request. |
| [Boundary-safe values](../../spec/lang/10-modules.md#boundary-safe-values) | Use the existing Request/Response value encoding. | A platform Request object or mutable reference is passed as if it were an hd value. |
| [Requirement rows](../../spec/lang/10-modules.md#entry-point) | Explicit row; validate every imported capability against the profile. | Local Process access succeeds although a deployment cannot bind it. |
| [Http query semantics](../../spec/std/http.md#request-builder) | Preserve the full URL and leave incoming query pairs empty. | Forwarding appends the original query twice. |
| [Cancellation](../../spec/std/task.md) | Cancel the request root; bound cleanup; abandon only after the bound. | Disconnect drops the instance before `defer` or dispose runs. |
| [Panics](../../spec/lang/06-control-flow.md) | Fail one request and abort its operations. | A poisoned shared instance serves another request with partially mutated state. |
| [Header pairs](../../spec/std/http.md#requests-and-responses) | Preserve exposed multiplicity and host framing. | Combining Set-Cookie values changes cookie meaning. |

Generics remain ordinary body-level generics; the registered boundary itself is monomorphic.
Closures and their captures remain inside a request instance.
Suspension does not extend a request beyond its host lifetime, and copied `&mut` references are not exclusive locks.
Trait providers chosen through `$.with` retain normal lexical/dynamic behavior within that request.

The concrete counterexamples are also executable controls where today's surface permits them.
The gap package demonstrates registration; the query and invalid-UTF-8 tests demonstrate mapping; the blocking route demonstrates event-loop waiting.
Shared-reference and cross-root behavior cannot be demonstrated through a nonexistent server ABI and is not claimed as implemented.
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
Fresh instances reserve memories and hold copied bodies while active.
At eight requests and one MiB bodies, multiple input/output/exchange copies can dominate the initial one MiB of aggregate memory.
The implementation needs an aggregate byte budget, not merely a per-body check.

The proposed server profile caps guest heap memory at four MiB and exchange memory at two MiB per instance.
Reserve another four MiB per slot for retained input/output and provider-side copies, giving ten MiB per slot and eighty MiB for eight.
Enforce these maxima in the handler module's memory declarations and bounded host readers, including outgoing Http responses.
A guest allocation failure fails that request; exceeding an isolate-wide platform limit can still fail sibling requests.

The remaining platform memory headroom must cover JavaScript objects, compiled code, engine overhead, and transient read chunks.
These are proposed limits, not a measured proof of Cloudflare's peak memory use.
Profile maximum-sized bodies with eight requests before acceptance; lower admission or body limits if peak isolate usage exceeds 100 MiB.
A runtime that cannot enforce or measure the required quota must not advertise the portable server profile.

### Bottom-Up Implementation Budget

Exact source sizes below are `wc -l` physical lines at base `adaeaea81b15d6ec6c94ba436cab33f913b4a122`.
They identify existing machinery; they are not claimed deletions.
No existing host module is replaced wholesale.
Added lines are planning budgets, split into independently reviewable pieces.

| Component | Existing code and exact file lines | Replaced lines | Proposed added lines |
| --- | --- | --- | --- |
| Selection/validation | `hd_project/src/manifest.rs`: 867; driver `lib.rs`: 10,626 | 0 | 35 manifest + 65 typed entry/row validation. |
| Entry lowering | backend `lib.rs`: 603; `task.rs`: 2,121 | 0 | 25 entry metadata + 120 start/result wrappers + 10 memory-limit emission; reuse root poll/cancel. |
| Typed boundary | `host/wire.mjs`: 211; generated `abi.mjs`: 531 | 0 | 65 Rust ABI metadata + 65 generated JS codec/descriptor lines. |
| Portable request driver | `host/core.mjs`: 1,307 | 0 | 100 lease/admission/cleanup + 45 ready-only block/optional async call hooks. |
| Fetch provider | `host/http.mjs`: 231, currently Node-oriented | 0 | 150 fetch/redirect/grant/error/body adaptation. |
| Node listener | `host/node.mjs`: 1,046, retained for commands | 0 | 100 HTTP mapping + 80 supervision/shutdown. |
| CLI/artifacts | CLI `main.rs`: 1,327; `engine.rs`: 957; `node.rs`: 850 | 0 | 85 selection/output + 120 platform packaging. |
| Platform shims | Browser `host/web.mjs`: 133, unchanged | 0 | 25 Workers + 35 Vercel Node + 35 Vercel Edge. |
| Focused tests/docs | New cases for this boundary | 0 | 200 fixtures/driver tests + 100 adapter tests; prose counted separately. |

Budget totals are 525 implementation Rust/metadata lines and 635 implementation JavaScript lines, plus 300 test lines.
These are estimates grounded in the current bridge and existing machinery, not a measured reference implementation of the missing ABI.
No new core task scheduler or root table is budgeted.
If entry lowering requires broader ownership/runtime changes, stop and rescope rather than charging them to a small adapter row.

Each component stops for review above 1.5 times its allocated added-line budget.
Count nonblank code separately in implementation reports, with generated files distinguished from hand-written source.
Also stop if fresh-instance initialization exceeds one millisecond median for this control, or the representative artifact exceeds 512 KiB gzip.
Those are proposed review thresholds, not claims of general service latency.

### Compile Time, Runtime, And Compiler Footprint

The prototype reuses a frozen compiler with zero source difference across crates, std, hosts, and Cargo inputs from this branch's base.
It does not measure a new compiler path's compile-time overhead because that path does not exist.
An implementation must compare interleaved baseline/candidate builds for hello and this handler, keeping all trials and source hashes.
Report parse/check/mono/emit time, total time, Wasm functions/code bytes, and request initialization separately.

Command hello must pay zero handler exports, helper functions, or imports when no handler entry is selected.
This proposal changes no executable compiler bytes and makes no new hello size claim.
The implementation's web compiler gzip gate remains a hard two MB cap; added handler support must be measured against it.
The much smaller deployable application bundle is a different artifact and a different limit.

## Stress Test

This applies the [stress-test method](../../.agents/skills/stress-test/SKILL.md) to the proposed boundary.
The surface under test is the monomorphic handler, fresh-instance lifetime, whole-body limits, explicit grants, and existing task cleanup.
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
| S13: shared cache / counter | Top-level mutable reference increments across requests. | Deliberately no persistence; friction compared with a reusable service; document external state. |
| S14: CPU loop and panic | Non-yielding loop or trap during handling. | Trap isolation and Node supervision required; edge cooperative timer alone cannot bound CPU loops. |
| S15: redirect to denied service | Allowed upstream redirects to metadata/local address. | Required manual-redirect grant test; platform auto-follow would bypass the intended check. |
| S16: wake after disposal | Fetch completes after timeout/abandon and after its slot is reused. | Required generation test; late wake must not touch another instance. |

The prototype's exact translations live in the linked hd program and adapter scripts.
They approximate JSON handlers, context cancellation, timeout middleware, and proxying using existing hd APIs.
The remaining cases exercise missing transport code, so presenting them as checked hd examples would hide the actual gap.
Each maps to a proposed implementation fixture below.

The highest-risk failures are unbounded buffering, delayed cancellation, and capability bypass on redirects.
They can silently affect resources or authority despite correct handler typing.
Fresh instances remove cross-request hd aliases and single-root conflicts, but do not solve those host problems.
A shared-root implementation adds more failure modes before addressing these three.

## Phasing

Each slice targets roughly one hour and leaves existing command behavior green.
A slice that needs protected compiler/runtime files first receives explicit scope approval on its implementation issue.
The design commit itself changes only documentation and runnable research controls.

| Slice | Deliverable | Focused acceptance evidence |
| --- | --- | --- |
| 1 | Handler manifest selection and signature/row validation, descriptor. | Unknown/missing/private/wrong/generic entry diagnostics; existing command selection unchanged; fmt/clippy. |
| 2 | Typed single-request start/finish wrappers. | Node/native codec round-trip, malformed lengths, one-shot start/finish, existing command hello byte identity. |
| 3 | Portable driver and lease cleanup. | Pending/wake/cancel/abandon, late completion, trap, no operation leaks; no-JSPI ready and pending block distinction. |
| 4 | Fetch provider over platform APIs. | Existing Http cases for redirects, grants, methods, errors and body preservation on Node/workerd/Edge VM. |
| 5 | Bounded request/response adapter and admission. | Chunked limit, header limit, cookies, HEAD/204/205/304, overload and byte accounting, eight concurrent requests. |
| 6 | Node development listener and supervision. | Local U3/U4, disconnect, process shutdown, CPU-loop termination; Windows/macOS/Linux targeted coverage. |
| 7 | Cloudflare packaging. | Static Wasm dry-run bundle, local workerd U1–U4, request-signal compatibility, measured size/startup/memory. |
| 8 | Vercel Node packaging. | Build Output validation, local function adapter U1–U4, payload cap and deadline/error behavior. |
| 9 | Vercel Edge packaging. | Actual platform build validation plus Edge VM U1–U4 without JSPI; sanitized pending-sync-wait error. |
| 10 | Optional JSPI hd driver. | workerd pending `block_on` fetch success, cancellation during suspension, ready calls, traps, fallback host without JSPI. |
| 11 | Final spec and examples with the responsible implementation slices. | Numbered new IDs per changed meaning, related conformance cases, rewrite summary, web compiler under hard gzip cap. |

Spec rules and fixtures ship with the implementing code, not in a detached final semantics-only pass.
Slice 11 audits their combined coverage and removes obsolete examples; it does not postpone that requirement.
No full local test suite is required by this proposal; each implementing issue names its focused gates under the current Muse guide.
CI and the owner verify integration on current main.

## Recommendation And Approval

**Recommendation:** implement the typed handler entry with a fresh Wasm instance per admitted request and one shared compiled module.
Use whole bodies with explicit admission and byte budgets, common capability grants, and the existing single-root cleanup model.
Make the normal async path independent of JSPI; add optional JSPI support as a separately tested host feature.
Keep shared-root, warm hd state, streaming, and platform-specific services in later designs.

The next best option is a shared-root service when measurements show repeated initialization or external state is the limiting cost.
That option needs #806's supervision and shared-state answers before it can preserve predictable request isolation.
The queued single-instance option is smaller but fails the intended concurrency requirement under slow upstream calls.
The current controls make fresh instances a concrete alternative to the notes' assumed root table.

Approval is needed for the handler convention/manifest extension, fresh-state lifetime, explicit limits and overload policy, server-profile grants, and pending-sync-wait behavior.
The owner also needs to approve the versioned request ABI and platform artifact workflows before their implementation issues are filed.
These choices are recommendations here, not silently applied changes to language or CLI behavior.
The local VM/workerd results do not substitute for platform build validation or deployed acceptance tests.

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
| One handler, existing Request/Response, limited registration | A Handler Entry; Architecture And Core-Wasm ABI. |
| Node local server; Workers; Vercel Node and Edge | CLI And Deployment Workflows; Platform Limits And JSPI. |
| Bundle limits and static Wasm | Platform Limits And JSPI; Cost And Measurements; packaging acceptance slices. |
| Request concurrency, cancellation, state, minimal #806 | Concurrency, Cancellation, And State; Options On The Same Use Cases. |
| Whole bodies; streaming requirement | Bodies And HTTP Mapping; streaming explicitly deferred for the fixed cases. |
| Capability rows and platform grants | Capabilities And Grants. |
| Core-Wasm stays, no Component Model reversal | Architecture And Core-Wasm ABI; Prior Art distinguishes incompatible host models. |
| Library/glue before language changes | Options; no new syntax or std combinator; one compiler-known entry convention. |
| Four fixed end-to-end scenarios | Problem And Fixed Use Cases; workflows; controls and missing packaging gates. |
| Spin, wasmCloud, Fastly, Rust worker prior art | Four dedicated Prior Art sections with design/interface/coverage/limits. |
| workerd / Vercel Edge JSPI | Platform Limits And JSPI; actual local runtime probes and production uncertainty separated. |
| Data, algorithm, cache, tests | Architecture And Core-Wasm ABI; driver pseudocode, cache keys, lease fields, Phasing. |
| Stdlib API shape, rows, error types, receivers, bounds | A Handler Entry; value parameter/return, monomorphic boundary, explicit row, response errors; existing atoms reused. |
| Stdlib performance/security/prior art | Cost And Measurements; Capabilities And Grants; Prior Art and Stress Test. |
| CLI commands/output/schema/status/config/scope/platforms | CLI And Deployment Workflows; explicit unsupported native listener and platform build gate. |
| Runtime memory/RC/ABI/cleanup/parity | Architecture; Concurrency; Cost; Node/native wrapper tests in slice 2; production JS-only scope explicit. |
| Runtime failure, observability, grants, footprint | Cache/Failure; Output; Capabilities; measurements and future hello identity check. |
| Stress cases from real libraries and adversarial inputs | Stress Test; executed controls distinguished from required future tests. |
| Bottom-up cost and stop condition | Cost And Measurements; exact existing line counts, estimated added lines, 1.5× component stop. |
| Owner approval before spec/implementation; one-hour jobs | Recommendation And Approval; Phasing. |
| Extension points | Cache, Extensions, And Failure Isolation; typed capabilities, streaming ABI, stateful service deferred. |
