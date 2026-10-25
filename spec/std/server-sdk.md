# Server SDK

Status: library specification draft.

The `server_sdk` package supplies an ordinary library over package-declared
host capabilities. Its `server` module owns the request-source interface;
an application retains an ordinary `main!` and chooses its explicit state.
These names are also re-exported from the package's root module.

## Request Values

1. r[std-server-sdk.request-id] `RequestId` has public `generation: u64` and `sequence: u64` fields.
2. r[std-server-sdk.incoming] `Incoming` has public `id: RequestId` and `request: std.http.Request` fields.
3. r[std-server-sdk.request-id-identity] Both fields identify a request lease within its source instance.
4. r[std-server-sdk.response-value] Replies use the existing `std.http.Response` value.

## Errors And Cancellation

1. r[std-server-sdk.error] `ServerError` has variants `Closed`, `Expired`, and `Other(reason: string)`.
2. r[std-server-sdk.error-display] Their display texts are `server closed`, `request lease expired`, and the supplied reason, respectively.
3. r[std-server-sdk.cancel-reason] `CancelReason` has variants `Disconnected`, `Deadline`, and `Stopping`.

## Request Source

1. r[std-server-sdk.host] The SDK declares the package-owned host capability trait `HttpServer`.
2. r[std-server-sdk.accept-decl] Its `accept!(&mut self)` method returns `Result[Incoming?, ServerError]`.
3. r[std-server-sdk.accept-end] A successful absent value ends that worker's admission loop.
4. r[std-server-sdk.respond-decl] Its `respond!(&mut self, id: RequestId, response: Response)` method returns `Result[void, ServerError]`.
5. r[std-server-sdk.cancelled-decl] Its `cancelled!(&mut self, id: RequestId)` method returns `CancelReason`.
6. r[std-server-sdk.cancelled-wait] `cancelled!` waits until that request's source reports a cancellation reason.
7. r[std-server-sdk.clock-decl] Its `clock(&mut self, id: RequestId)` method returns a lease-owned `*mut dyn std.time.Clock`.
8. r[std-server-sdk.finish-decl] Its `finish(&mut self, id: RequestId, abandoned: bool)` method returns void.
9. r[std-server-sdk.finish-lifetime] `finish` acknowledges that the worker has finished or abandoned cleanup and no longer needs the lease.
10. r[std-server-sdk.finish-abandoned] Its `abandoned` argument reports whether cleanup was cut short.
11. r[std-server-sdk.clock-expired] A source clock rejects operations after its lease is finished. Boundary failure: `host-contract`.
12. r[std-server-sdk.client-decl] Its `client(&mut self, id: RequestId)` method returns a lease-owned `*mut dyn std.http.Http`.
13. r[std-server-sdk.client-expired] A source client rejects operations after its lease is finished. Boundary failure: `host-contract`.

> **Why.** The capability identifies request lifetimes without adding an
> HTTP entry form or an HTTP name known to the compiler. A test can provide
> it through an ordinary requirement binding.

## Serving

1. r[std-server-sdk.serve-decl] The SDK declares `serve![S, $R](workers: usize, state: *mut S, handler: fn!(Request, &mut S) -> Response $ R) -> Result[void, ServerError] $ R + HttpServer`.
2. r[std-server-sdk.serve-width] The worker count is `workers`, or one when `workers` is zero.
3. r[std-server-sdk.serve-lease-loop] Each worker repeatedly accepts one request and runs a job-local race between its handler and the source's cancellation signal.
4. r[std-server-sdk.serve-state] Every handler receives the same explicit state reference.
5. r[std-server-sdk.serve-join] `serve!` joins every worker before returning.
6. r[std-server-sdk.serve-error] It returns the first worker error in worker order, or a successful void value when every worker succeeds.
7. r[std-server-sdk.serve-row] Calls to the handler retain the handler's requirement row.

> **Why.** Worker concurrency uses the existing task scheduler. State
> mutations must leave the state valid before a suspending call; a mutable
> reference does not provide exclusion across suspension.

## Request Cleanup

1. r[std-server-sdk.lease-clock] The worker binds the source's request clock around the handler and its cleanup bound.
2. r[std-server-sdk.lease-timeout] The handler has a twenty-second timeout with a hundred-millisecond cooperative cleanup bound.
3. r[std-server-sdk.lease-timeout-response] A handler timeout produces status 504 with body `request timed out`.
4. r[std-server-sdk.lease-reply] A completed handler's response returns with its original request ID.
5. r[std-server-sdk.lease-cancel] A source cancellation cancels that handler without cancelling the worker group.
6. r[std-server-sdk.lease-external-cancel] Cancelling the worker externally retains the same cleanup bound.
7. r[std-server-sdk.lease-finish] Every accepted lease is finished once after response delivery or cancellation cleanup.
8. r[std-server-sdk.lease-expired-response] An expired response lease does not end its worker's admission loop.
9. r[std-server-sdk.lease-client] The worker binds the source's request client around the handler and its cleanup bound.

> **Why.** The source's admission deadline can cancel earlier than the
> handler's fallback timeout. The existing timeout combinator bounds
> cleanup when the local cancellation race or an outer cancellation wins.
> A CPU-bound handler or cleanup still needs platform interruption.

## JavaScript Lease Dispatcher

The SDK adapter keeps native I/O in each request's owning coroutine while
the retained guest shares synchronous driver turns.

1. r[std-server-sdk.dispatch.open] `createLeaseDispatcher` creates generation-tagged leases with monotonically increasing request sequences.
2. r[std-server-sdk.dispatch.owner] A lease's `run()` coroutine owns native operation starts and disposal.
3. r[std-server-sdk.dispatch.poll] Polling another request's guest work queues its native operations for their owning lease.
4. r[std-server-sdk.dispatch.pending] `operation(context, start)` returns readiness through the package provider's existing `context.pending` interface.
5. r[std-server-sdk.dispatch.abort] Aborting before an operation starts prevents its native I/O.
6. r[std-server-sdk.dispatch.late] Completions after disposal or lease finish must not wake a later request.
7. r[std-server-sdk.dispatch.copy] `respond` copies response bytes and header pairs before guest lease release.
8. r[std-server-sdk.dispatch.ack] `run()` publishes the response only after the worker acknowledges cleanup through `finish`.
9. r[std-server-sdk.dispatch.failure] Instance failure rejects active requests after their queued native disposal runs in their respective owners.
10. r[std-server-sdk.dispatch.poison] A failed dispatcher rejects new requests.
11. r[std-server-sdk.dispatch.cancel] Cancellation records the first reason for its lease without cancelling other leases.

> **Why.** A platform may cancel I/O started in another completed request.
> Only readiness and copied data cross shared guest turns; native I/O stays
> with the request that awaits it.

## JavaScript Application Adapter

`adapters/application.mjs` connects the dispatcher to an ordinary compiled
SDK application through public runtime and generated binding modules.

1. r[std-server-sdk.application.inputs] `createApplication` takes a Wasm module, generated bindings, `createCoreHost`, explicit grants, and an output sink.
2. r[std-server-sdk.application.source] The adapter requires exactly one root package capability named `server.HttpServer` with an explicitly granted selector.
3. r[std-server-sdk.application.driver] The adapter drives its retained instance through serialized synchronous `createDriver().step()` turns.
4. r[std-server-sdk.application.values] `dispatch(request, signal)` accepts an HD request value and returns response, cancellation, and cleanup-abandonment values.
5. r[std-server-sdk.application.signal-reason] Aborting the supplied signal preserves `Deadline` and `Stopping` reasons and maps every other reason to lease-local `Disconnected` cancellation.
6. r[std-server-sdk.application.clock] The source clock queues timer creation and disposal in its request's owning coroutine.
7. r[std-server-sdk.application.stop] `stop()` closes admission and ends idle workers while admitted requests finish.
8. r[std-server-sdk.application.fail] `fail(error)` poisons the instance and closes its core host.
9. r[std-server-sdk.application.client-policy] Source clients use the owning core runtime's Http policy service.
10. r[std-server-sdk.application.http-guard] An ambient Http send fails instead of substituting a client without a request lease. Boundary failure: `host-contract`.

> **Note.** This adapter exchanges plain values. Platform HTTP mapping,
> admission limits, supervision and deployment
> packaging are separate layers.

## JavaScript Fetch Transport

`adapters/http.mjs` supplies a native-fetch transport for a lease-owned
`std.http.Http` handle.

1. r[std-server-sdk.http.factory] `createHttpClient(lease, options)` returns a JavaScript implementation of `Http.send` for that lease.
2. r[std-server-sdk.http.inputs] Its caller supplies resolved application grants and the public core URL-validation helpers.
3. r[std-server-sdk.http.owned] Native fetch, body reading, timeout timers and abort controllers belong to the lease's operation callback.
4. r[std-server-sdk.http.manual] Every fetch uses manual redirects.
5. r[std-server-sdk.http.grants] The transport checks the destination's written host and port against the Http grant before every fetch.
6. r[std-server-sdk.http.invalid] Invalid, non-ASCII and percent-encoded hosts are rejected before fetching.
7. r[std-server-sdk.http.backslash] Backslashes in request URLs or redirect locations are rejected before fetching their targets.
8. r[std-server-sdk.http.redirects] A zero redirect limit returns the first response without following or validating its Location.
9. r[std-server-sdk.http.redirect-limit] Exceeding a positive redirect limit returns `TooManyRedirects` without fetching the next target.
10. r[std-server-sdk.http.credentials] A cross-origin redirect removes Authorization, Proxy-Authorization, Cookie and Host request headers.
11. r[std-server-sdk.http.rewrite] Redirects use Fetch's method and body rewriting rules.
12. r[std-server-sdk.http.responses] Every final status completes with the whole body and final request URL.
13. r[std-server-sdk.http.timeout] The default thirty-second provider timeout covers every redirect and final body reading.
14. r[std-server-sdk.http.abort] Disposing the lease operation aborts its native request and clears its timer.
15. r[std-server-sdk.http.headers] Response headers follow the standard HTTP field-combination and separate-cookie rules.

> **Why.** Manual redirects enforce grants on every destination and avoid
> forwarding credentials to another origin. The transport's own timeout
> requires no ambient HD Clock capability.
