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
14. r[std-server-sdk.console-decl] Its `console(&mut self, id: RequestId)` method returns a lease-owned `*mut dyn Console`.
15. r[std-server-sdk.console-expired] A source console rejects writes after its lease is finished. Boundary failure: `host-contract`.

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
2. r[std-server-sdk.lease-handler-timeout] The handler has a twenty-second fallback timeout.
3. r[std-server-sdk.lease-timeout-response] A handler timeout produces status 504 with body `request timed out`.
4. r[std-server-sdk.lease-reply] A completed handler's response returns with its original request ID.
5. r[std-server-sdk.lease-cancel] A source cancellation cancels that handler without cancelling the worker group.
6. r[std-server-sdk.lease-external-cancel] Cancelling the worker externally retains the same cleanup bound.
7. r[std-server-sdk.lease-finish] Every accepted lease is finished once after response delivery or cancellation cleanup.
8. r[std-server-sdk.lease-expired-response] An expired response lease does not end its worker's admission loop.
9. r[std-server-sdk.lease-client] The worker binds the source's request client around the handler and its cleanup bound.
10. r[std-server-sdk.lease-cleanup-budget] The SDK bounds cooperative cleanup to one hundred milliseconds from when cleanup begins.
11. r[std-server-sdk.lease-cleanup-backstop] The host's cancellation watchdog expires two hundred milliseconds after cancellation is sent.
12. r[std-server-sdk.lease-cleanup-escalation] If cleanup remains unacknowledged at that expiry, the host poisons or terminates the affected instance.
13. r[std-server-sdk.lease-cleanup-acknowledged] Acknowledged cleanup cancels its host watchdog.
14. r[std-server-sdk.lease-cleanup-platform] Platform CPU limits apply independently of the cleanup watchdog.
15. r[std-server-sdk.lease-console] The worker binds the source's request console around the handler and its cleanup.

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
12. r[std-server-sdk.dispatch.effect] `effect(task)` queues a synchronous callback for execution in the lease's owning coroutine.
13. r[std-server-sdk.dispatch.effect-release] Already captured effects drain before the lease owner's `run()` completes.

> **Why.** A platform may cancel I/O started in another completed request.
> Only readiness and copied data cross shared guest turns; native I/O stays
> with the request that awaits it.

## JavaScript Application Adapter

`adapters/application.mjs` connects the dispatcher to an ordinary compiled
SDK application through public runtime and generated binding modules.

1. r[std-server-sdk.application.inputs-with-logs] `createApplication` takes a Wasm module, generated bindings, `createCoreHost`, explicit grants, and an output sink or private log observer.
2. r[std-server-sdk.application.source] The adapter requires exactly one root package capability named `server.HttpServer` with an explicitly granted selector.
3. r[std-server-sdk.application.driver] The adapter drives its retained instance through serialized synchronous `createDriver().step()` turns.
4. r[std-server-sdk.application.values] `dispatch(request, signal)` accepts an HD request value and returns response, cancellation, and cleanup-abandonment values.
5. r[std-server-sdk.application.signal-reason] Aborting the supplied signal preserves `Deadline` and `Stopping` reasons and maps every other reason to lease-local `Disconnected` cancellation.
6. r[std-server-sdk.application.clock] The source clock queues timer creation and disposal in its request's owning coroutine.
7. r[std-server-sdk.application.stop] `stop()` closes admission and ends idle workers while admitted requests finish.
8. r[std-server-sdk.application.fail] `fail(error)` poisons the instance and closes its core host.
9. r[std-server-sdk.application.client-policy] Source clients use the owning core runtime's Http policy service.
10. r[std-server-sdk.application.http-guard] An ambient Http send fails instead of substituting a client without a request lease. Boundary failure: `host-contract`.
11. r[std-server-sdk.application.start] `start()` eagerly drives the retained application's startup.
12. r[std-server-sdk.application.ready] The optional `onReady` callback runs once after a completed driver turn parks on request acceptance.
13. r[std-server-sdk.application.stopped] The optional `onStopped` callback runs after stopped main completion closes the core host.

> **Note.** This adapter exchanges plain values. Platform HTTP mapping,
> admission limits, supervision and deployment
> packaging are separate layers.

## Application Logs

The SDK keeps application output separate from protocol and lifecycle events.
The embedding caller chooses the private log destination.

1. r[std-server-sdk.log.console-policy] SDK request consoles use their owning core runtime's Console authority.
2. r[std-server-sdk.log.denied] A denied write returns `ConsoleError.NotGranted` for `stdout` or `stderr` without capturing a record.
3. r[std-server-sdk.log.capture] A permitted Console call completes when it captures its immutable text record.
4. r[std-server-sdk.log.owner] A captured request record is delivered in that request's owning coroutine.
5. r[std-server-sdk.log.record] An application record has `version: 1`, `stream` of `out` or `err`, `text`, and a `lease` identity.
6. r[std-server-sdk.log.identity] A request record's lease contains decimal-string `generation` and `sequence` fields captured from its request ID.
7. r[std-server-sdk.log.unassociated] Application records without a request console, including startup and runtime diagnostics, have `lease: null`.
8. r[std-server-sdk.log.fallback] Without a `log(record)` observer, Application forwards text to its output sink's corresponding stream.
9. r[std-server-sdk.log.observer] Log observers run synchronously and their return values are not awaited.
10. r[std-server-sdk.log.observer-failure] A throwing observer does not prevent lease completion or later record delivery.
11. r[std-server-sdk.log.private] The SDK never copies application log text into lifecycle events or generic HTTP error responses.
12. r[std-server-sdk.log.metadata] The SDK adds no request headers, bodies, environment values, or credentials to log metadata.
13. r[std-server-sdk.log.supervisor] The supervisor delivers application records through its separate `log(record)` observer.
14. r[std-server-sdk.log.supervisor-unassociated] An unassociated supervised record has `request: null`.
15. r[std-server-sdk.log.supervisor-stale] The supervisor ignores log messages from stale worker generations or mismatched active request identities.
16. r[std-server-sdk.log.node] The development Node provider emits each captured record as one JSON line on its corresponding standard stream.
17. r[std-server-sdk.log.worker] A supervised record adds `worker: { slot, generation }` identifying its worker.
18. r[std-server-sdk.log.physical-request] A lease-associated supervised record adds `request` containing its physical request ID.

> **Why.** Another request may cause a shared guest turn, so the current
> JavaScript turn cannot identify the owner of application output.
> Lease-bound consoles retain identity through handler cleanup.

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


## Fetch Request Adapter

`adapters/fetch.mjs` maps standard Fetch requests to the ordinary application.
Its optional timer service supports deterministic hosts and tests without
changing the profile's limits.

1. r[std-server-sdk.fetch.factory] `createFetchAdapter(options)` constructs one retained application and exposes its Fetch handler.
2. r[std-server-sdk.fetch.handler] `createFetchHandler(application, options)` adapts an existing application or supervisor.
3. r[std-server-sdk.fetch.reservation] Admission reserves one of eight slots before reading request headers or body bytes.
4. r[std-server-sdk.fetch.overload] A ninth active request receives status 503 with `Retry-After: 1`, without waiting for a slot.
5. r[std-server-sdk.fetch.body-limit] Incoming and outgoing whole bodies are limited to one mebibyte each.
6. r[std-server-sdk.fetch.declared-body] A declared incoming body exceeding that limit receives status 413 before body reading.
7. r[std-server-sdk.fetch.actual-body] The adapter counts incoming bytes independently of the declared length.
8. r[std-server-sdk.fetch.header-limit] Incoming and outgoing headers are limited to 65,536 UTF-8 bytes, including four framing bytes per field.
9. r[std-server-sdk.fetch.headers-rejected] Oversized incoming headers receive status 431.
10. r[std-server-sdk.fetch.malformed] Invalid incoming method tokens, lengths, or body streams receive status 400.
11. r[std-server-sdk.fetch.url] The incoming request retains the platform's absolute URL, with an empty separate query list.
12. r[std-server-sdk.fetch.method] Standard methods map to their standard variants, and other valid tokens map to `Method.Other`.
13. r[std-server-sdk.fetch.framing] The adapter removes hop-by-hop fields, Connection-nominated fields, and length or transfer framing on both sides.
14. r[std-server-sdk.fetch.cookies] Response Set-Cookie values remain separate platform header values.
15. r[std-server-sdk.fetch.status] Response statuses must be between 200 and 599.
16. r[std-server-sdk.fetch.bodyless] HEAD responses and statuses 204, 205, and 304 have no platform response body.
17. r[std-server-sdk.fetch.response-invalid] Invalid or oversized guest responses become generic status 500 before platform response construction.
18. r[std-server-sdk.fetch.deadline] The twenty-second request deadline starts at admission and includes incoming body reading.
19. r[std-server-sdk.fetch.deadline-response] Admission deadline cancellation produces status 504.
20. r[std-server-sdk.fetch.disconnect] A platform disconnect cancels only its admitted request.
21. r[std-server-sdk.fetch.reader-owner] Native reader cancellation runs in the reader's owning continuation.
22. r[std-server-sdk.fetch.backstop] The adapter applies the two-hundred-millisecond cancellation watchdog to unacknowledged work and native reader disposal.
23. r[std-server-sdk.fetch.poison] Watchdog escalation closes admission and poisons the retained application.
24. r[std-server-sdk.fetch.failure] Application failure exposes a generic status 500 and cancels other active requests.
25. r[std-server-sdk.fetch.private-log] Failure logs contain event categories without guest details, bodies, or header values.
26. r[std-server-sdk.fetch.stop] The first `stop()` closes admission and drains already admitted work.
27. r[std-server-sdk.fetch.stop-again] A second `stop()` cancels admitted work with `Stopping`.
28. r[std-server-sdk.fetch.stale] Late completions after poisoning cannot reopen admission.

> **Note.** Eight reservations bound the platform layer; an application
> with fewer accepting workers can still wait internally. A blocking
> isolate still needs an external supervisor or its platform CPU limit.

## Node Worker Supervisor

`adapters/supervisor.mjs` runs each retained application in a separate
Node worker. Runtime, generated-binding, and application module URLs are
explicit public inputs; packaging supplies them independently.

1. r[std-server-sdk.supervisor.factory] `createSupervisor` takes a compiled module, module URLs, grants, explicit lifecycle budgets, and observer callbacks.
2. r[std-server-sdk.supervisor.worker] Each worker retains one application instance and its state across sequential requests.
3. r[std-server-sdk.supervisor.capacity] At most one request is active per ready worker, with no waiting request queue.
4. r[std-server-sdk.supervisor.overload] Unavailable capacity returns status 503 with the configured Retry-After value.
5. r[std-server-sdk.supervisor.admission-time] Request deadlines include time elapsed since the supplied admission timestamp.
6. r[std-server-sdk.supervisor.budgets] Startup, request, drain, outer-stop, restart-count, restart-window, and Retry-After values must be supplied explicitly.
7. r[std-server-sdk.supervisor.outer-floor] The outer-stop budget cannot precede the two-hundred-millisecond cleanup watchdog.
8. r[std-server-sdk.supervisor.watchdog] The supervisor runs the fixed cleanup watchdog outside the guest's worker thread.
9. r[std-server-sdk.supervisor.non-yielding] Watchdog expiry terminates an unacknowledged worker even when its guest never yields.
10. r[std-server-sdk.supervisor.failure] Worker failure returns generic status 500 without private guest diagnostics.
11. r[std-server-sdk.supervisor.sibling] Retiring one worker does not poison another worker's retained instance.
12. r[std-server-sdk.supervisor.restart] Replacement respects the supplied restart count and window, reporting exhaustion to the observer.
13. r[std-server-sdk.supervisor.release] A response result requires a matching worker generation, request ID, and release acknowledgement.
14. r[std-server-sdk.supervisor.stop] First stop drains active work, while second stop or drain expiry starts cancellation.
15. r[std-server-sdk.supervisor.join] Stop and close completion join worker termination.
16. r[std-server-sdk.supervisor.confirmation] Forced termination reports cleanup as unconfirmed.

> **Note.** The component supplies no deployment task, lifecycle budget
> defaults, or aggregate memory guarantee.
