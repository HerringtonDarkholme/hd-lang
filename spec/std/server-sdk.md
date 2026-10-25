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
7. r[std-server-sdk.clock-decl] Its `clock(&mut self, id: RequestId)` method returns a lease-owned `&mut dyn std.time.Clock`.
8. r[std-server-sdk.finish-decl] Its `finish(&mut self, id: RequestId, abandoned: bool)` method returns void.
9. r[std-server-sdk.finish-lifetime] `finish` acknowledges that the worker has finished or abandoned cleanup and no longer needs the lease.
10. r[std-server-sdk.finish-abandoned] Its `abandoned` argument reports whether cleanup was cut short.
11. r[std-server-sdk.clock-expired] A source clock rejects operations after its lease is finished. Boundary failure: `host-contract`.

> **Why.** The capability identifies request lifetimes without adding an
> HTTP entry form or an HTTP name known to the compiler. A test can provide
> it through an ordinary requirement binding.

## Serving

1. r[std-server-sdk.serve-decl] The SDK declares `serve![S, $R](workers: usize, state: &mut S, handler: fn!(Request, &mut S) -> Response $ R) -> Result[void, ServerError] $ R + HttpServer`.
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

> **Why.** The source's admission deadline can cancel earlier than the
> handler's fallback timeout. The existing timeout combinator bounds
> cleanup when the local cancellation race or an outer cancellation wins.
> A CPU-bound handler or cleanup still needs platform interruption.
