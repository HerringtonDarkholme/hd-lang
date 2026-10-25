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

## Deployment Grants

1. r[std-server-sdk.deploy.root-manifest] The SDK's shared deployment-grant reader reads `hd.toml` in the selected root package directory.
2. r[std-server-sdk.deploy.production-only] It takes deployment grants only from that manifest's `[capabilities]` table.
3. r[std-server-sdk.deploy.explicit-grants] It preserves every explicit unrestricted grant, denial and scope list.
4. r[std-server-sdk.deploy.package-consent] It preserves explicit package-capability consent and denial under the written capability path.
5. r[std-server-sdk.deploy.no-inheritance] Dependency manifests, development grants, command-line grants and ambient environment values do not add deployment grants.
6. r[std-server-sdk.deploy.no-default-grants] An absent production table or entry contributes no grant.
7. r[std-server-sdk.deploy.fresh-manifest] A later deployment read uses the manifest's current contents.
8. r[std-server-sdk.deploy.invalid-manifest] An unreadable, non-TOML or non-UTF-8 root manifest fails deployment preparation.
9. r[std-server-sdk.deploy.invalid-grant] A production entry with an invalid capability key or grant value fails deployment preparation.
10. r[std-server-sdk.deploy.no-value-diagnostics] Manifest-reader errors do not include source excerpts or grant values.

> **Why.** Deploy-time selection keeps the build policy-free and prevents
> development permissions from becoming production authority. The shared
> reader gives every platform adapter the same production entries.

The SDK JavaScript conformance inputs are in
[deployment-grants.json](../../packages/server-sdk/test/fixtures/deployment-grants.json);
its maintained [test runner](../../packages/server-sdk/test/grants.test.mjs)
also checks root selection, fresh reads, invalid UTF-8 and the SDK task bridge.

## Portable Runtime Packaging

1. r[std-server-sdk.runtime.snapshot] The SDK ships a platform-neutral host graph and its host ABI model as a portable runtime snapshot.
2. r[std-server-sdk.runtime.identity] The snapshot fingerprint covers its file names, file contents, package-host ABI version and source base revision.
3. r[std-server-sdk.runtime.validate] The emitter rejects invalid snapshot metadata, an unexpected file list, a checksum mismatch or a fingerprint mismatch before creating output.
4. r[std-server-sdk.runtime.bytes] Emitted runtime files preserve their snapshot contents byte for byte.
5. r[std-server-sdk.runtime.fresh-output] The emitter creates only a fresh task-owned directory and refuses an existing destination.
6. r[std-server-sdk.runtime.installed] Emitting the installed SDK's runtime requires neither a compiler checkout nor compiler-cache access.

> **Why.** SDK-owned distribution gives deploy tasks the host modules they
> need while the compiler's build output stays platform-neutral and policy-free.
> Each platform task owns its final bundle and build validation.

The maintained [runtime conformance controls](../../packages/server-sdk/test/runtime.test.mjs)
check invalid snapshots, source fidelity, output ownership and a compiled
application running from a relocated SDK after source and provider removal.

## Cloudflare Deployment

The SDK's `deployment` module prepares a Cloudflare Worker from built
application artifacts. The sample's ordinary
[deployment task](../../examples/server-api/tasks/deploy-cloudflare.hd)
uses this API through its package-declared provider.

### Deployment API

1. r[std-server-sdk.cloudflare.artifacts] `Artifacts` has public string fields `wasm`, `bindings`, and `output`.
2. r[std-server-sdk.cloudflare.options] `CloudflareOptions` has public fields `name: string`, `artifacts: Artifacts`, `compatibility_date: string`, and `env_bindings: List[string] = []`.
3. r[std-server-sdk.cloudflare.receipt] `DeploymentReceipt` has public string fields `directory`, `runtime_fingerprint`, `wasm_sha256`, and `bindings_sha256`.
4. r[std-server-sdk.cloudflare.error] `DeploymentError` has the variant `Failed(message: string)`.
5. r[std-server-sdk.cloudflare.error-display] That variant's display text is its supplied message.
6. r[std-server-sdk.cloudflare.host] `Deployment` is an SDK-owned package host capability trait.
7. r[std-server-sdk.cloudflare.method] Its `cloudflare!(&mut self, options: CloudflareOptions)` method returns `Result[DeploymentReceipt, DeploymentError]`.
8. r[std-server-sdk.cloudflare.function] `cloudflare!(options: CloudflareOptions)` requires `Deployment` and forwards to that capability's method with the same result type.
9. r[std-server-sdk.cloudflare.task-root] The deployment provider selects the invoking task's root package directory.
10. r[std-server-sdk.cloudflare.paths] Relative artifact and output paths resolve from that root directory.
11. r[std-server-sdk.cloudflare.name] The Worker name must contain lowercase ASCII letters, digits, or hyphens, with alphanumeric endpoints and at most 63 characters.
12. r[std-server-sdk.cloudflare.date] `compatibility_date` must be a valid calendar date written as `YYYY-MM-DD`.
13. r[std-server-sdk.cloudflare.output-root] The output directory must not equal or contain the selected root package directory.
14. r[std-server-sdk.cloudflare.prepare-only] Preparation writes local deployment files without uploading or requesting deployment credentials.
15. r[std-server-sdk.cloudflare.reexports] The package root reexports `Artifacts`, `CloudflareOptions`, `DeploymentReceipt`, `DeploymentError`, `Deployment`, and `cloudflare!`.

### Artifact And Policy Validation

1. r[std-server-sdk.cloudflare.production] Preparation uses the shared [deployment-grant reader](#deployment-grants) for the selected root package.
2. r[std-server-sdk.cloudflare.nominal-source] The contract must have exactly one source interface: `server.HttpServer` from the Deployment provider's validated nominal SDK package and version.
3. r[std-server-sdk.cloudflare.source-consent] At least one selector naming that source must have explicit production consent `true`.
4. r[std-server-sdk.cloudflare.source-veto] Any selector naming that source with production value `false` rejects preparation.
5. r[std-server-sdk.cloudflare.capture] Preparation captures the input Wasm and binding companion once and uses those captured bytes throughout validation and emission.
6. r[std-server-sdk.cloudflare.driver] The module must export functions `hd.init`, `hd.poll`, `hd.wake`, and memory `hd.x`.
7. r[std-server-sdk.cloudflare.contract-abi] The module must contain exactly one `hd.capabilities` section whose package-host ABI matches the installed SDK runtime and expected schema.
8. r[std-server-sdk.cloudflare.schema] Preparation validates every used package interface and method, including returned handles, against the installed SDK's expected server ABI schema.
9. r[std-server-sdk.cloudflare.module-identity] Compiler-generated module identifiers do not affect nominal identity or value-shape validation.
10. r[std-server-sdk.cloudflare.companion] The binding companion's generated data header must contain the same ABI version and contract bytes as the captured Wasm.
11. r[std-server-sdk.cloudflare.binding-code] Preparation neither evaluates nor ships the supplied companion's JavaScript body.
12. r[std-server-sdk.cloudflare.rebuilt-bindings] Deployed bindings are rebuilt by SDK-owned code from the validated contract data.
13. r[std-server-sdk.cloudflare.import-members] Every module import must name a supported function member of the portable runtime or the validated SDK package contract.
14. r[std-server-sdk.cloudflare.preflight-effects] Import validation must not instantiate the application or invoke application providers and outgoing I/O.
15. r[std-server-sdk.cloudflare.clock-random-denial] A required primitive Clock or Random import with an explicit production denial rejects preparation.
16. r[std-server-sdk.cloudflare.env-names] `env_bindings` must contain distinct names matching `[A-Za-z_][A-Za-z0-9_]*` and covered by the production Env grant.
17. r[std-server-sdk.cloudflare.env-values] The Worker exposes only selected string-valued platform bindings through Env, checking the production grant on access.
18. r[std-server-sdk.cloudflare.env-capture] Worker startup captures selected binding values from its first request's platform environment.
19. r[std-server-sdk.cloudflare.env-private] Preparation records binding names without reading or embedding their runtime values.
20. r[std-server-sdk.cloudflare.receipt-values] The receipt identifies the output directory, portable runtime fingerprint, captured Wasm SHA-256, and emitted binding-module SHA-256.

| Portable primitive namespace | Availability |
| --- | --- |
| `hd:rt`, `hd:prim` | Runtime support. |
| `hd:Console` | Production Console policy. |
| `hd:Clock`, `hd:Random` | Core policy, with explicit denials enforced. |
| `hd:Env` | Selected string bindings and production Env policy. |
| `hd:Http` | Platform Fetch transport and production destination policy. |

> **Note.** The expected schema and fingerprints record ABI compatibility
> and provenance. They do not authenticate a compiler or recover nominal
> hd value types erased by the boundary ABI.

### Output Publication

1. r[std-server-sdk.cloudflare.payload] Prepared output includes a static Wasm import, SDK-built bindings, the portable runtime, Fetch adapters, production grants, provenance, and Wrangler configuration.
2. r[std-server-sdk.cloudflare.output-owned] Preparation refuses an existing output directory without the SDK's Cloudflare ownership record.
3. r[std-server-sdk.cloudflare.output-unchanged] Preparation refuses modified SDK generations or an externally changed Wrangler selector.
4. r[std-server-sdk.cloudflare.output-generation] Each complete payload occupies an immutable generation directory.
5. r[std-server-sdk.cloudflare.output-selector] Publication atomically replaces the regular `wrangler.json` selector with one naming a complete generation.
6. r[std-server-sdk.cloudflare.output-retain] Publication retains earlier generations so readers of an earlier selector can finish using their original payload.
7. r[std-server-sdk.cloudflare.output-extras] Preparation preserves unrelated output-root files and platform caches without traversing them.
8. r[std-server-sdk.cloudflare.output-writers] SDK writers must acquire the output lock before publication.
9. r[std-server-sdk.cloudflare.output-reserved] Other writers must not concurrently modify the SDK ownership record, generation store, or Wrangler selector.
10. r[std-server-sdk.cloudflare.output-retry] An interrupted SDK-owned publication without a selector may be retried after validating its completed generations.

> **Why.** A small selector commit keeps the active configuration and its
> immutable payload together while allowing Wrangler to retain its cache.
> The SDK owns reserved output paths, rather than replacing a nonempty
> directory containing files managed by other tools.

The maintained [deployment controls](../../packages/server-sdk/test/deployment.test.mjs)
exercise ordinary tasks, artifact validation, production policy and relocated
output. [Output controls](../../packages/server-sdk/test/output.test.mjs)
cover ownership and publication; [binding controls](../../packages/server-sdk/test/contract-bindings.test.mjs)
and [schema controls](../../packages/server-sdk/test/server-schema.test.mjs)
cover companion pairing, value codecs and the SDK's expected ABI data.
