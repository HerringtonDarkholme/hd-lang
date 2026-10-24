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

> **Why.** The capability identifies request lifetimes without adding an
> HTTP entry form or an HTTP name known to the compiler. A test can provide
> it through an ordinary requirement binding.

## Serving

1. r[std-server-sdk.serve-decl] The SDK declares `serve![S, $R](workers: usize, state: &mut S, handler: fn!(Request, &mut S) -> Response $ R) -> Result[void, ServerError] $ R + HttpServer`.
2. r[std-server-sdk.serve-width] The worker count is `workers`, or one when `workers` is zero.
3. r[std-server-sdk.serve-loop] Each worker repeatedly accepts one request, invokes the handler, and responds with its result and original request ID.
4. r[std-server-sdk.serve-state] Every handler receives the same explicit state reference.
5. r[std-server-sdk.serve-join] `serve!` joins every worker before returning.
6. r[std-server-sdk.serve-error] It returns the first worker error in worker order, or a successful void value when every worker succeeds.
7. r[std-server-sdk.serve-row] Calls to the handler retain the handler's requirement row.

> **Why.** Worker concurrency uses the existing task scheduler. State
> mutations must leave the state valid before a suspending call; a mutable
> reference does not provide exclusion across suspension.
