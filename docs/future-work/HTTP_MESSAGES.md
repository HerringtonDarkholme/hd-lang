# HTTP Messages In Both Directions

Status: design proposal, 2026-10-25; domain: standard library API and host boundary; not accepted.

This note addresses [#847](https://github.com/HerringtonDarkholme/hd-lang/issues/847).
It follows the owner's [message direction](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403469822)
[consuming-reader clarification](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403519156),
[message-level API and private-wire direction](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403655215),
[ordinary header conversions](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403828161),
[HeaderName](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403739107),
[deferred header validation](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403751104),
[HttpStatus](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403775715),
and the final From-only direction above, which supersedes earlier automatic typed-header mapping and decoding proposals.
The [final form scope](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6404135821) keeps outgoing set_form and excludes a typed form reader.
Those decisions constrain the proposal; the remaining API and migration details still need approval before implementation.
The current contract remains [std.http](../../spec/std/http.md).

## Contents

- [Problem, users, use cases and today](#problem-users-use-cases-and-today)
- [State of the art](#state-of-the-art)
- [Semantics](#semantics)
- [Options](#options)
- [Architecture](#architecture)
- [Extension points](#extension-points)
- [Cost](#cost)
- [Stress test](#stress-test)
- [Caller migration](#caller-migration)
- [Phasing](#phasing)
- [Recommendation and approvals](#recommendation-and-approvals)
- [Sources and parse log](#sources-and-parse-log)

## Problem, users, use cases and today

An HTTP client builds requests and reads responses. A server reads requests and builds responses.
Today, hd offers convenient methods for the first pair only.
The server SDK therefore inspects URL strings and assembles response byte fields by hand.

The fixed use cases are a JSON client, a JSON server, a form receiver, a proxy, repeated headers, and retrying an outgoing operation.
Each needs the same message vocabulary, without endpoint declarations or routing machinery.
Typing belongs at the body decode: the owner rejected a message type parameter such as `Request[Order]`.

Inspection pins main `6ad81d02e5002f86b5f97ea2d1508fa11555ffbb`.
`Request` owns a string URL, an extra outgoing query list, header pairs, body bytes, and a redirect count.
Its builders return a counted mutable request.
`Response` owns status, header pairs, body bytes, and the final URL string.

`Response.header` finds the first case-insensitive match; `text` decodes lossily, and `json[T]` uses `Deserialize`.
There are no equivalent request readers or response writers.
Both body fields are ordinary lists, so reading or copying a message does not consume a body.
A current-build probe reads one response twice and prints the same text on Node and Wasmtime.

The proposal deliberately changes that last property.
The owner selected single-use bodies now so callers need no second consumption-model migration when streaming arrives.
This does not implement streaming, backpressure, or a transport handle.

| Use case | User's goal | Today | Proposed message layer |
| --- | --- | --- | --- |
| JSON client | Build a POST, send it, decode a reply into `Order` | Request builder and Response reader have different surfaces | The same `Body` writers and typed readers on both messages |
| JSON server | Read an incoming order and return a typed receipt | Manual request byte conversion and response construction | `request.json[Order]`, then `Response::status(.Created).set_json(reply)` |
| Form receiver | Read an incoming URL-encoded body | No Request body reader | Read text/bytes; application parsing stays explicit; no typed std form reader |
| Proxy | Change destination and headers while forwarding a body | Value-copy lists are reusable | Head copies are independent; body aliases share consumption |
| Repeated headers | Preserve two `Set-Cookie` values and look up a field | Pairs plus Response-only lookup | Owned `Headers` with first/all lookup and ordered duplicates |
| Retry | Repeat an operation without a silently empty body | Copies retain body bytes | Construct fresh work and a fresh body for each application attempt |

## State of the art

The comparison distinguishes all four directions.
Using one message name does not imply one body ownership protocol, and an incoming message need not be a live response writer.

| System | Outgoing request | Incoming request | Outgoing response | Incoming response |
| --- | --- | --- | --- | --- |
| Fetch | Constructed `Request` | Service Worker `FetchEvent.request` | Constructed `Response`, `respondWith` | `fetch` resolves `Response` |
| Rust `http` | `Request<T>` | `Request<T>` | `Response<T>` | `Response<T>` |
| Go `net/http` | `*Request` | `*Request` | `ResponseWriter` | `*Response` |
| Node `http` | `ClientRequest` | `IncomingMessage` | `ServerResponse` | `IncomingMessage` |
| Dart `dart:io` | `HttpClientRequest` sink | `HttpRequest` stream | `HttpResponse` sink | `HttpClientResponse` stream |
| Ktor | `HttpRequestBuilder` | `ApplicationRequest` / `receive<T>` | `ApplicationResponse` / `respond` | `HttpResponse` / `body<T>` |
| swift-http-types | `HTTPRequest` head | `HTTPRequest` head | `HTTPResponse` head | `HTTPResponse` head |

### Fetch

Request and Response share body readers, including JSON, text, bytes and form data.
JSON returns a JavaScript value rather than a statically chosen domain type.
A disturbed or locked stream cannot be consumed again; conversion failure does not rewind it.
`clone()` tees a body stream rather than sharing one consumption cursor.
[Fetch Body](https://fetch.spec.whatwg.org/#body-mixin), [body cloning](https://fetch.spec.whatwg.org/#concept-body-clone).

Metadata attributes are largely read-only, while Headers mutation follows guards.
Service Workers receive a Request and answer with a Response, demonstrating both server-facing directions.
hd borrows the message symmetry and explicit consumption; it does not adopt JavaScript typing or browser header guards.
[Fetch Request](https://fetch.spec.whatwg.org/#request-class), [Response](https://fetch.spec.whatwg.org/#response-class),
[Service Worker fetch handling](https://w3c.github.io/ServiceWorker/#handle-fetch).

### Rust http and Hyper

`http::Request<T>` and `Response<T>` own a head and arbitrary body type.
They expose immutable/mutable access, decomposition and body mapping; cloning depends on the body's `Clone` implementation.
The container does not itself prescribe JSON decoding or single-use consumption.
Hyper supplies receive-stream bodies for server requests and client responses.
[http Request](https://docs.rs/http/1.5.0/http/request/struct.Request.html),
[Response](https://docs.rs/http/1.5.0/http/response/struct.Response.html),
[Hyper bodies](https://docs.rs/hyper/1.12.0/hyper/body/index.html).

`HeaderMap` keeps multiple values, but does not promise iteration order.
`Uri.query()` exposes a raw query rather than decoded application values.
hd can separate the message head from body interpretation without adopting the rejected generic envelope or Rust's exclusive borrowing.
[HeaderMap](https://docs.rs/http/1.5.0/http/header/struct.HeaderMap.html),
[Uri](https://docs.rs/http/1.5.0/http/uri/struct.Uri.html).

### Go net/http

Go reuses Request for sending and receiving, with parsed URL and Header fields.
Servers write through ResponseWriter; clients receive a Response whose body the caller closes.
`Request.Clone` copies the head but shares Body, making it a useful comparison for hd's proposed copy behavior.
`GetBody` exists separately to obtain another body for replay, including preserved-body redirects.
[Request](https://pkg.go.dev/net/http#Request), [Clone](https://pkg.go.dev/net/http#Request.Clone),
[ResponseWriter](https://pkg.go.dev/net/http#ResponseWriter), [Client](https://pkg.go.dev/net/http#Client).

Go's map and pointer behavior is not hd's value-copy guarantee.
Nor does a buffered hd body acquire Go's connection cleanup obligations merely because both APIs say Body.
Transport lifetime remains the adapter's responsibility.

### Node http

Node separates writable ClientRequest/ServerResponse from readable IncomingMessage, which serves both incoming roles.
Its HTTP layer does not provide typed JSON decoding.
The server's URL is the raw request target; normalized headers and `rawHeaders` offer different preservation guarantees.
hd's message values need not become live connection writers to offer these useful operations.
[Node HTTP](https://nodejs.org/api/http.html#class-httpclientrequest),
[IncomingMessage](https://nodejs.org/api/http.html#class-httpincomingmessage),
[ServerResponse](https://nodejs.org/api/http.html#class-httpserverresponse).

### Dart dart:io

Dart exposes four transport roles: two incoming byte streams and two outgoing sinks.
Incoming requests own a parsed Uri; outgoing bodies are written before closing the request or response.
Incoming headers are immutable, and outgoing headers become immutable after body writing begins.
These are transport-state rules rather than hd's owned-message mutation permissions.
[HttpClientRequest](https://api.dart.dev/dart-io/HttpClientRequest-class.html),
[HttpRequest](https://api.dart.dev/dart-io/HttpRequest-class.html),
[HttpResponse](https://api.dart.dev/dart-io/HttpResponse-class.html),
[HttpClientResponse](https://api.dart.dev/dart-io/HttpClientResponse-class.html),
[HttpHeaders](https://api.dart.dev/dart-io/HttpHeaders-class.html).

Uri's `queryParametersAll` preserves repeated values; single-value header lookup instead requires at most one value.
JSON decoding is a separate, dynamically typed operation.
Automatic redirects do not forward the body, illustrating why replay needs its own contract.
[Query values](https://api.dart.dev/dart-core/Uri/queryParametersAll.html),
[header value](https://api.dart.dev/dart-io/HttpHeaders/value.html),
[JSON](https://api.dart.dev/dart-convert/jsonDecode.html),
[redirects](https://api.dart.dev/dart-io/HttpClientRequest/followRedirects.html).

### Ktor

Ktor's outgoing request builder owns URL/header builders and a body converted by installed serializers.
Servers read through ApplicationCall and answer through a separate response interface.
Typed `receive<T>` and `body<T>` show that typed conversion does not require a generic message envelope.
[Client requests](https://ktor.io/docs/client-requests.html),
[server requests](https://ktor.io/docs/server-requests.html),
[server responses](https://ktor.io/docs/server-responses.html).

Immutable Url and read-oriented Headers are separate from their builders; headers support first/all lookup.
Normal client responses cache their bodies for repeated access, while streaming avoids that cache.
Server repeated receiving requires the separate DoubleReceive plugin, whose supported combinations have restrictions.
Thus Ktor has no single consumption rule to import into hd.
[Headers](https://api.ktor.io/ktor-http/io.ktor.http/-headers/index.html),
[Url](https://api.ktor.io/ktor-http/io.ktor.http/-url/index.html),
[client responses](https://ktor.io/docs/client-responses.html),
[DoubleReceive](https://ktor.io/docs/server-double-receive.html).

### swift-http-types

Swift reuses HTTPRequest and HTTPResponse value heads across client and server integrations.
Bodies are intentionally separate, supplied by the transport library.
HTTPRequest contains method, scheme, authority, path and fields; HTTPResponse contains status and fields.
That supports symmetric owned metadata without prescribing how a body is decoded or consumed.
[Introduction](https://www.swift.org/blog/introducing-swift-http-types/),
[HTTPRequest](https://github.com/apple/swift-http-types/blob/main/Sources/HTTPTypes/HTTPRequest.swift),
[HTTPResponse](https://github.com/apple/swift-http-types/blob/main/Sources/HTTPTypes/HTTPResponse.swift).

HTTPFields is an ordered value collection with case-insensitive names.
Individual repeated values remain accessible even when a convenience lookup combines them.
This is useful metadata precedent, but not a solution for typed body reads or shared consumption.
[HTTPFields](https://github.com/apple/swift-http-types/blob/main/Sources/HTTPTypes/HTTPFields.swift),
[field names](https://github.com/apple/swift-http-types/blob/main/Sources/HTTPTypes/HTTPFieldName.swift).

Across these systems, message construction, typed decoding, repeated metadata and consumption are separate concerns.
The selected hd API combines them deliberately rather than copying any system's entire client/server framework.

## Semantics

### Values, references and direction

The message owns its `Url` and `Headers` as ordinary values.
Its `body` field is a counted mutable reference, `*mut Body`.
Response.status is the owner's HttpStatus enum with a u16 code, not shared enum data or a language change.
A message copy therefore copies its head independently while sharing the body object.
This follows existing [value parameters](../../spec/lang/07-functions.md#r-fn.param.value-copy)
and [reference identity](../../spec/lang/13-values-and-references.md#r-ref.identity.copies), rather than a new copying rule.

A handler takes `&Request`.
It cannot replace that request's URL, headers or body reference, but it can consume the body referent.
Read-only access is explicitly [shallow](../../spec/lang/13-values-and-references.md#r-ref.read.shallow).
It also permits mutations through that same body reference; this is not a deeply immutable security boundary.

Consuming Body readers take `&mut self`, as the owner clarified.
URL, header and message inspection methods take &self.
Message writes take mut self and return Self; owned metadata mutation and internal Body consumption take &mut self.
There is no facade with a second internal state allocation.
Counted-to-local receiver lending uses the existing [reference conversions](../../spec/lang/13-values-and-references.md#conversions).

### Body state and typed decoding

The proposed buffered implementation stores bytes and an unread/used state in one Body object.
A read claims the body before decoding; a second read returns a typed `Used` error.
Empty and malformed bodies also consume their one read in this proposal.
This last detail needs approval: it is not implied by copying Fetch's name.

`json[T < Deserialize]` returns `Result[T, BodyError]`; it never erases the target type.
Serialization uses `T < Serialize`, and preserves existing serde rename/default behavior.
The relevant contracts are [serde](../../spec/std/serde.md), [JSON](../../spec/std/json.md),
and [derivation](../../spec/lang/14-annotations.md#serialization).
Consuming reads are plain functions while the whole body is already buffered, with no capability row or bang.

Reading text should preserve today's lossy UTF-8 behavior.
Body and format errors remain typed; a malformed client payload should not become a panic or erased Error.
Incoming form interpretation, multipart files and validation are application concerns here; there is no new typed form reader.
The validation design in #846 must not be implemented incidentally here.

### URL and header invariants

A parsed URL must preserve the grant-visible spelling of the authority.
The existing [ASCII host and no-normalization rules](../../spec/std/http.md#urls) remain the starting constraint.
Do not use WHATWG host canonicalization to turn a spelling into a different grant identity.
An encoded slash in a path remains encoded; path access must not accidentally create routing semantics.

Header names are a HeaderName enum, compared and hashed by ASCII-folded wire name.
Known variants and Other spellings of the same name are equal; Eq and Hash are handwritten, not derived.
HttpStatus equality and hashing similarly compare code(), so Other(404) equals NotFound.
Incoming names parse into the matching known variant.
Keep repeated values, especially separate `Set-Cookie` fields, and distinguish first-value lookup from all-values lookup.
Do not comma-join cookie fields or overwrite them in a map.
These constraints follow [the current header rules](../../spec/std/http.md#requests-and-responses) and
[RFC 9110 field ordering](https://www.rfc-editor.org/rfc/rfc9110.html#section-5.3).

### Control flow, cancellation and boundaries

A body read completes synchronously before another task can observe a partially advanced cursor.
A failed decode leaves a used body; `?` propagates the error without restoring it.
No new `defer`, `using`, suspension or cancellation rule is needed for buffered bytes.
A closure capturing a message copies its shared body reference under [capture copying](../../spec/lang/07-functions.md#r-fn.capture.copy.value).
A retry closure must therefore construct fresh work rather than repeatedly send that captured message.
The SDK still owns leases, cancellation, response commitment and cleanup.

A host boundary has [tree semantics](../../spec/lang/10-modules.md#boundary-encoding), not shared guest identity.
Private representations need the direction's [Serialize/Deserialize opt-in](../../spec/lang/10-modules.md#private-fields-at-a-boundary).
Neither a guest pointer nor a future live stream handle may be copied as a host descriptor accidentally.
The transport plan must show where body consumption occurs and how returned bytes create a fresh unread Body.

## Options

The owner has already selected nongeneric messages, owned heads, a shared body, message-level methods, HeaderName and HttpStatus enums.
Message readers use &self; the stored Body's consuming reader uses &mut self.
The alternatives below explain mechanism and cost; they do not reopen that decision.

| Mechanism | What it buys | What it costs or misses |
| --- | --- | --- |
| Selected: one counted Body with a state flag | Shared consumption, shallow handler reads, ordinary hd | New used-body errors and explicit replay work |
| Facade plus private mutable state cell | Consuming APIs could spell `&self` | Second allocation and indirection; rejected by owner |
| Smallest change: keep repeatable body lists, add missing methods | Minimal migration and no consumption flag | Fails the chosen single-use semantics and postpones another migration |
| Generic `Request[B]`/`Response[B]` | Transport-specific body representations | Rejected envelope shape and pervasive body type parameters |

The message and enum operations are library mechanisms in the repository's design-cost order.
The chosen public shape needs no new syntax or intrinsic.
Internal host registration uses the existing pub rule; automatic adapter installation is separate integration work that must be priced explicitly.
Any host-schema change is separate engineering work and must be priced rather than called free.

## Architecture

### Data structures and complete signatures

The proposed field lists are:

| Value | Fields | Representation constraint |
| --- | --- | --- |
| Request | `method: Method`, `url: Url`, `headers: Headers`, `body: *mut Body` | Public owned fields; no query or redirect field |
| Response | `status: HttpStatus`, `headers: Headers`, `body: *mut Body`, `url: Url?` | An absent origin for a locally built response |
| Url | Written URI-reference plus parsed component ranges | Private invariant; preserve the grant-visible authority spelling |
| Headers | Ordered `List[(HeaderName, string)]` | Owned duplicate-preserving collection; validate before transport effects |
| Body | `payload: List[u8]`, `used: bool` | Private state in one counted object; no media hint or enclosing-message pointer |

`Url?` replaces the existing empty-string Response URL sentinel in this proposal.
Request method and Response status remain ordinary fields; no status-specific Response subclasses or typed endpoint objects appear.
Body owns no socket, provider, closure or live host handle.

The owner selected **message-level** read/write pairs, identically on Request and Response.
Writers take `mut self` and return Self, so they chain on temporaries without keeping an outer message reference.
Readers take `&self` and consume the stored `*mut Body` through its writable referent.
The lean Body's internal consuming reader takes `&mut self`.

| Surface on both messages | Proposed complete signature | Effect |
| --- | --- | --- |
| JSON read | `json[T < Deserialize](&self) -> Result[T, BodyError]` | Claim once and decode the requested type |
| JSON write | `set_json[T < Serialize](mut self, value: T) -> Self` | New Body, JSON bytes, Content-Type on the returned message |
| Text read | `text(&self) -> Result[string, BodyError]` | Claim once, preserving lossy UTF-8 decoding |
| Text write | `set_text(mut self, value: string) -> Self` | New Body and text/plain Content-Type |
| Bytes read | `bytes(&self) -> Result[List[u8], BodyError]` | Claim once without text conversion |
| Bytes write | `set_bytes(mut self, value: List[u8]) -> Self` | New Body; no invented content encoding |
| Form write | `set_form[T < Serialize](mut self, value: T) -> Self` | Preserve existing flat form serialization under the message writer name |
| Header read | `header(&self, name: HeaderName) -> string?` | First case-insensitive match |
| Header write | `set_header(mut self, name: HeaderName, value: string) -> Self` | Replace that name on the returned owned Headers value |

The proposed constructor signatures are `Request::new(method: Method, url: Url) -> Request`, `get(url: Url) -> Request`, and `post(url: Url) -> Request`.
Response has `status(code: HttpStatus) -> Response`, coexisting with its status field as the owner checked.
HttpStatus.code(&self) returns u16; from_code(u16) selects a known variant or Other(code).
`Response.ok(&self) -> Result[Response, HttpError]` checks status without consuming the body.
`Request.send!(&self, redirects: usize = 10) -> Result[Response, HttpError] $ Http` is the terminal.
Client uses the same redirect argument and retains its base/default-header/cookie state.

An `api` in `Request::post(api).set_json(order)` is an already parsed Url in this proposal.
The free `get!(url: string)` remains a convenience that parses and sends with defaults.
Constructor parsing therefore has an explicit typed error rather than hidden panics or invalid intermediate messages.
Client resolves relative references before sending; direct send rejects an unresolved reference.

The owned metadata atoms retain the 20% operations:

| Atom | Inspection / construction | Mutation |
| --- | --- | --- |
| Headers | `new() -> Headers`; `get(&self, name: HeaderName) -> string?`; `get_all(&self, name: HeaderName) -> List[string]`; `validate(&self) -> Result[void, HeaderError]` | `append(&mut self, name: HeaderName, value: string) -> void`; `set(&mut self, name: HeaderName, value: string) -> void`; `remove(&mut self, name: HeaderName) -> void` |
| Url | `parse(text: string) -> Result[Url, UrlError]`; `path(&self) -> string`; `query(&self, name: string) -> string?`; `query_all(&self, name: string) -> List[string]`; `to_string(&self) -> string` | `add_query(&mut self, name: string, value: string) -> void`; `set_path(&mut self, encoded: string) -> Result[void, UrlError]` |
| Body | `from_bytes(bytes: List[u8]) -> *mut Body`; consuming `bytes(&mut self) -> Result[List[u8], BodyError]` | Optional atom `replace_bytes(&mut self, bytes: List[u8]) -> Result[void, BodyError]`, rejecting Used rather than resetting aliases |

`Url.resolve(&self, reference: string) -> Result[Url, UrlError]` returns a new value; it does not mutate its base.
Header validation runs at the wire boundary, with an optional explicit early validate call.
Headers::from, Headers.set and message.set_header remain infallible and hold plain data, with no hidden error state.
Http sending returns HttpError.InvalidHeader(name); the server responds with 500 and logs the reason instead of emitting bad bytes.
UrlError details and optional early URL validation remain part of the approval review.
The complete checked catalog identifies which signatures are implemented in the local model and which have explicit unimplemented stubs.

The body error proposal is BodyError.Used and BodyError.Json(JsonError).
Only JSON decoding selects a body-read target type; text and bytes return their ordinary concrete types.
HttpError retains transport/status cases and gains a typed body-preparation cause.
Format failures are not silently converted into transport strings.

A message setter replaces its returned value's Body reference, detaching it from the source message's body aliases.
Thus `let mut out = *incoming; out = out.set_json(value)` changes only out's head/body reference.
An explicit atom that mutates the existing Body instead changes the object shared by all aliases.
This distinction also avoids using a header-changing message method to mutate a body owned by another head.

The proposed `set_json`, `set_text` and `set_form` set Content-Type immediately; there is no deferred media hint.
A later `set_header(.ContentType, custom)` overrides that selection in the returned message.
The reverse order gives the content setter's type, a proposed last-writer rule that needs approval.
A discarded returned message has no lasting setter effect on its source value, so migration must retain or chain the result.

### Ordinary header conversions and HeaderName

Headers::from is the ordinary From trait call, with std implementations for Map[string, string] and List[(string, string)].
There is no Serialize-based field mapping, typed header decoder, special header-source trait or automatic rename machinery.
Applications implement From[MyHeaders] for Headers to define their own typed writing convention.
Reading a typed header value is likewise the application's explicit conversion from Headers.

The public signatures are the ordinary `From[Map[string, string]] for Headers` and `From[List[(string, string)]] for Headers` implementations.
Each converts names through HeaderName.parse and retains ordinary string values.
The pair-list conversion preserves repeated values in order; a map contributes its own entries without inventing repeated-value semantics.
A user conversion can use Headers.get/get_all/set/append with the same HeaderName vocabulary.

The infallible conversions store data without validation errors or hidden error state.
They neither discard malformed fields nor inject an error sentinel.
Headers.validate gives an optional typed early check; the mandatory wire check remains authoritative after any subsequent mutation.
No invalid CR/LF or token bytes reach the client or server wire.

HeaderName equality is not Method-style derived equality.
Other("Cache-Control") must equal CacheControl, and equal values must write identical Hash input.
The short known-name inventory covers std's own reads/writes plus common application headers; unknown names remain Other(string).
Header-name validation checks ASCII token bytes, while value validation rejects CR/LF and the existing wire-disallowed controls.

The validator runs before body consumption or network effects in the proposed send preparation order.
The server checks before committing an outgoing response; invalid output becomes a logged 500, never an injected field line.
Incoming adapters validate host-supplied pairs before they become a request or response message.
An early validate call does not turn a later mutation into a permanently trusted object.

The proposed known HeaderName inventory is deliberately twenty names:

| Variant | Wire name | Variant | Wire name |
| --- | --- | --- | --- |
| Accept | accept | AcceptEncoding | accept-encoding |
| Authorization | authorization | CacheControl | cache-control |
| Connection | connection | ContentEncoding | content-encoding |
| ContentLength | content-length | ContentType | content-type |
| Cookie | cookie | Host | host |
| Location | location | Origin | origin |
| ProxyAuthorization | proxy-authorization | RetryAfter | retry-after |
| SetCookie | set-cookie | TransferEncoding | transfer-encoding |
| UserAgent | user-agent | Vary | vary |
| XForwardedFor | x-forwarded-for | XForwardedProto | x-forwarded-proto |

This covers every header std.http itself reads or writes.
Transport-specific names such as TE, Trailer and Content-Language remain valid Other values.
An incoming spelling selects a known variant after ASCII folding only.
Unicode lowercasing is wrong here: the Kelvin sign in CooKie must not normalize into Cookie and evade token validation.

The proposed twenty known HttpStatus variants are:

| Variant | Code | Variant | Code |
| --- | ---: | --- | ---: |
| Ok | 200 | Created | 201 |
| NoContent | 204 | MovedPermanently | 301 |
| Found | 302 | SeeOther | 303 |
| NotModified | 304 | TemporaryRedirect | 307 |
| PermanentRedirect | 308 | BadRequest | 400 |
| Unauthorized | 401 | Forbidden | 403 |
| NotFound | 404 | RequestTimeout | 408 |
| PayloadTooLarge | 413 | TooManyRequests | 429 |
| InternalServerError | 500 | BadGateway | 502 |
| ServiceUnavailable | 503 | GatewayTimeout | 504 |

Other(code: u16) preserves every unlisted code, including SDK cases 205 and 431.
This inventory includes all explicit std.http redirect/transient codes; it does not define a new Ord implementation.
Status-range checks continue to compare code() with the existing numeric bounds.
No shared enum data or language change is involved.

### Checked before and after programs

The [before program](http-messages/before/src/main.hd) uses current public std APIs.
It manually decodes the incoming body and constructs response bytes, then delivers that response through ScriptedHttp.
Both engines print 42.

```text
# Current public std.http API, matching one typed buffered client/server transaction.
use std.http.{Http, Request, Response, ScriptedHttp}
use std.json.{decode, encode}
use std.serde.{Deserialize, Serialize}

@derive(Serialize, Deserialize)
struct Order:
    quantity: usize

@derive(Serialize, Deserialize)
struct Reply:
    accepted: usize

fn handle!(request: Request) -> Response:
    text := string::from_utf8(request.body).expect("request UTF-8")
    order := decode::[Order](text).expect("typed request JSON")
    Response { status: 201, headers: [("Content-Type", "application/json")], body: encode(Reply { accepted: order.quantity }).to_utf8() }

fn transaction!() -> usize:
    request := Request::post("https://example.test/orders?mode=fast").header("x-trace", "demo").json(Order { quantity: 42 })
    response := handle!(*request)
    http := ScriptedHttp::new()
    http.reply("https://example.test/orders?mode=fast", .Ok(response))
    $.with(Http=http):
        result := request.redirects(3).send!().expect("request")
        result.json::[Reply]().expect("typed response JSON").accepted

pub fn main!() -> void $ Console:
    println(transaction!())
```

The [local model](http-messages/model/src/main.hd) defines the proposed message types in ordinary hd.
Its read-only server handler uses message methods; the model's ReadError is a deliberately simplified error type.
The excerpt below is checked inside that complete model, not as a standalone file defining std APIs.

```text
fn handle!(request: &Request) -> Result[Response, ReadError]:
    if request.method != .Post || request.url.path() != "/orders" || request.url.query("mode") != .Some("fast"):
        return .Err(.Fixture)
    if request.header(.Other("X-Trace")) != .Some("demo"):
        return .Err(.Fixture)
    order := request.json::[Order]()?
    .Ok(Response::status(.Created).set_json(Reply { accepted: order.quantity }))
```

The modeled client constructs a fresh outgoing value and decodes a Reply.
Its sample_url is a fixture-built Url, not evidence of a complete public URL parser.
HttpAdapter and Loopback exercise the guest conversion seam without pretending to install a real host provider.

```text
fn outgoing(quantity: usize) -> Request:
    Request::post(sample_url()).set_header(.Other("x-trace"), "demo").set_json(Order { quantity: quantity })

fn transaction!() -> Result[usize, ReadError]:
    transport := &mut Loopback { calls: 0, redirects: 0 }
    adapter := &mut HttpAdapter { wire: transport }
    $.with(Http=adapter):
        response := outgoing(42).send!(redirects=3)?
        reply := response.json::[Reply]()?
        .Ok(reply.accepted)
```

The [signature catalog](http-messages/api-catalog.hd) checks the proposed signatures, enum implementations and user-owned header conversions.
Unimplemented methods contain explicit shape-only panics; the catalog must not be run as an implementation.
The functional model instead executes its implemented operations and thirteen focused cases on each engine.
Outgoing form encoding, arbitrary URL parsing, full typed causes, lossy text and automatic provider installation remain outside that executable model.

### URL and form algorithms

The URL proposal accepts URI references for Client/base resolution and absolute http/https URLs for direct sends.
Parse components without decoding the authority or path; an escaped slash remains part of a segment.
Store outgoing query changes inside Url, replacing the old separate Request.query list.
A query read decodes each name/value once and keeps repeated names in order; mutation percent-encodes each new pair once.

The exact plus/space policy and malformed-percent handling need approval alongside this API.
Form bodies use URL-encoded form rules, where plus represents space, rather than assuming every URL query has that meaning.
There is no std form[T] reader in the final scope.
A server reads the body through text()/bytes() and performs any application-specific form conversion explicitly.
The optional plain querystring-to-pairs helper is not added speculatively here.

Outgoing set_form preserves today's flat FormWriter, including scalar/optional/list fields and serde wire names.
It remains a value-returning message setter, setting the form Content-Type immediately.
Unsupported outgoing shapes retain today's panic behavior, with their existing exact-message tests migrated to the new spelling.
Multipart uploads and a general typed form decoder are outside this job.

### Equality and debug output

Request, Response and HttpError currently implement Eq and Debug; changing the body field cannot silently remove those traits.
References can participate in value derivation when their referents do, as [reference-field derivation](../../spec/lang/09-traits.md#r-trait.derive.reference-fields) states.
The proposed comparison uses body content plus used state, without consuming or resetting either body.
Debug must likewise observe state without consuming it; body/header redaction policy needs explicit review rather than accidental derived secret output.

A message containing a mutable shared body is unsuitable as a stable map key across mutation.
That follows existing reference-by-value equality, not a newly introduced pointer-identity equality.
Tests should inspect selected metadata/content intentionally instead of assuming alias identity implies equal message snapshots.

### Data flow and core algorithm

The intended buffered flow is:

1. An adapter receives validated metadata and bounded bytes.
2. Guest decoding constructs owned URL/Headers values and one fresh unread Body.
3. A handler inspects the head and consumes or forwards the shared body.
4. Outgoing preparation validates metadata and claims the body once.
5. The provider transports a byte snapshot; its redirects reuse that snapshot within this one logical send.
6. The reply decodes into another message with a fresh unread Body.

An application retry is another logical send.
It must create another body or explicitly rebuild one from retained bytes.
A message copy alone does not rewind consumption.
This must be reconciled with Client, ScriptedHttp and retained status errors during migration.

### Private wire boundary and public adapters

The owner selected internal, unreexported `@host pub` wire traits and public ordinary message-level adapters.
Wire payloads contain URL text, ordered header pairs and bytes, never a guest Body reference.
The public Http and SDK HttpServer interfaces use the approved Request/Response values.
Transport-only metadata remains in Incoming, and redirect options are parameters to sending.

On send, the public adapter validates the head, claims the shared Body once, and calls the private wire provider.
On receive, it parses metadata and wraps bytes in one fresh unread Body.
Client redirects replay the prepared byte snapshot within one logical send; an application retry constructs a fresh message/body.
The SDK's accept/respond adapter follows the same conversion while retaining its request lease and cancellation protocol.
An entry provider factory captures the wire provider once per entry lifetime; public rows continue to name Http/HttpServer only.
SDK request-scoped Http clients wrap their own lease-scoped wire handle; they must not reuse a process-default transport.
Grant checks retain the public capability identity, including every redirect destination.
Private transport bindings must not become an extra capability users must grant.

A current-build package-host probe demonstrates why the distinction matters.
A public Message with `body: *mut Body` checks cleanly but fails linking under the current structural boundary codec.
Its exact diagnostic is `stage Link: \`#23[mut #2737]\` at the host boundary in the linear backend is not supported`.
The probe's internal type IDs are not part of the API.

Two compile-only controls build: a private custom-serde envelope and an explicitly prepared byte snapshot.
Only the latter matches the chosen transport route, without hiding the public fields.
The controls have not executed a real provider.
A separate captured-adapter proof runs on both engines using an HD fake, verifying conversion, sharing and fresh reply bodies.
It does not prove actual host I/O, automatic provider installation or grant propagation.
Boundary serialization still has ordinary tree semantics and does not carry guest identity.

General reference lowering is a costed alternative, not a prerequisite or an authorized compiler task.
Its RefCell encoder/decoder must preserve view/reference-count semantics, and built-in Http's fixed codec still cannot infer arbitrary private serde.
The note therefore does not budget a magical compiler fix as a free library operation.
[Owner route](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403655215),
[exact probe](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403610119).

The core preparation algorithm is pseudocode, not a new compiler pass or checked Rust implementation:

```rust
fn prepare(message: &Request, options: SendOptions) -> Result<PreparedRequest, HttpError> {
    let url = resolve_and_validate(&message.url)?;
    validate_method(&message.method)?;
    validate_headers(&message.headers).map_err(HttpError::InvalidHeader)?;
    let headers = message.headers.to_pairs();
    let bytes = message.body.bytes().map_err(HttpError::Body)?;
    Ok(PreparedRequest { method: message.method, url, headers, bytes, options })
}
fn send_prepared(mut request: PreparedRequest) -> Result<Response, HttpError> {
    let mut remaining = request.options.redirects;
    loop {
        check_destination_grant(&request.url)?;
        let reply = provider_exchange(&request)?;
        remember_response_cookies(&request.url, &reply.headers);
        if !is_redirect_with_location(&reply) || request.options.redirects == 0 {
            return decode_reply_with_fresh_unread_body(reply);
        }
        let next = resolve_location(&request.url, &reply)?;
        if remaining == 0 {
            return Err(HttpError::TooManyRedirects(next.written_text()));
        }
        strip_cross_host_credentials(&mut request, &next);
        if redirect_changes_method_to_get(&request, &reply) {
            request.method = Method::Get;
            request.bytes.clear();
            strip_body_headers(&mut request.headers);
        }
        request.url = next;
        remaining -= 1;
    }
}
fn reply_to_host(id: RequestId, response: &Response) -> Result<(), ServerError> {
    if let Err(reason) = response.headers.validate() {
        wire.log_safe_response_error(id, escaped_reason(reason));
        return wire.respond(id, fixed_internal_server_error());
    }
    let headers = response.headers.to_pairs();
    let bytes = match response.body.bytes() {
        Ok(bytes) => bytes,
        Err(reason) => {
            wire.log_safe_response_error(id, escaped_reason(reason));
            return wire.respond(id, fixed_internal_server_error());
        }
    };
    wire.respond(id, PreparedResponse { status: response.status.code(), headers, bytes })
}
```

Metadata validation precedes body consumption and effects in this proposal.
The source body is claimed once before the provider call; cancellation or transport failure does not revive it.
Redirects replay the private prepared snapshot within that send, without another public body read.
This requires explicit provider/transport integration and is not established by the local model alone.

### Installing the public provider without exposing the wire capability

The physical wire traits are ordinary @host pub traits in internal modules such as std.http.wire and the SDK counterpart.
They are not re-exported from the public facade, as the owner decided in [6403712625](https://github.com/HerringtonDarkholme/hd-lang/issues/847#issuecomment-6403712625).
This is an internal-module convention, not package-private visibility or an inaccessible namespace.
No @host visibility rule changes.

The current profile contains trait paths, not ordinary provider factories.
Existing entry emission builds host-call vtables directly for those paths.
An explicit $.with adapter proof does not install the adapter automatically for a package main.
The implementation needs a narrow internal binding descriptor, not a user-facing provider configuration DSL.

The proposed descriptor is Direct(surface_key) or Adapted(surface_key, factory_def, wire_trait).
All entries are resolved definitions/types rather than stringly runtime dispatch.
An adapted entry roots the private HD factory, its returned implementation vtable and the wire methods' codecs.
Only the physical wire methods enter boundary classification; public message methods remain ordinary guest calls.

The first root construction obtains the primitive wire provider, invokes the selected factory, and retains its returned public trait object.
Pending polls reuse that root frame rather than reconstructing adapters.
SDK request-scoped client factories similarly capture that lease's wire handle, not the process-default client.
Clock/Console handles retain their existing lease behavior; this work does not redesign those capabilities.

Literal private @host was a rejected alternative; the current package-owned pub requirement is retained.
Public requirement rows name Http/HttpServer, not the physical internal wire trait.
The registration controls demonstrate why private @host cannot be silently substituted for the selected ordinary pub declaration.
Automatic adapter/bootstrap integration still needs implementation and measurement, without changing generic Body-reference boundary encoding.

Future implementation scope is driver profile/host declaration construction, mono provider roots, backend entry provider construction and host ABI metadata.
SDK physical interface identity, generated schema, packaged runtime and nominal-source preflight change together.
This is genuine integration work beyond the small ordinary-HD adapter and needs an explicit approved budget before implementation.

### Client, ScriptedHttp and retained error bodies

Client keeps its existing mutable configuration/cookie object; message value setters do not turn the jar into a copied value builder.
The catalog checks `new() -> *mut Client`, `base_url(*mut self, url: Url) -> *mut Client`, and `header(*mut self, name: HeaderName, value: string) -> *mut Client`.
It also checks `cookie(&self, url: &Url, name: string) -> string?` and `send!(&mut self, request: Request, redirects: usize = 10) -> Result[Response, HttpError] $ Http`.
The legacy header configuration appends defaults; message set_header replaces one name, so those operations must be documented distinctly.

Client resolves the URL, merges owned defaults/cookies, validates the final head and claims the shared Body once.
Its redirect loop reuses that one prepared byte snapshot, preserves method rewriting and strips cross-host credentials as today.
Zero redirects returns the first response; exhausting a positive limit returns TooManyRedirects before another send.
Timeout/cancellation or a failed send does not reset the source Body.

ScriptedHttp must store immutable byte/head snapshots rather than public bodies sharing used flags.
The proposed registration is `reply(*mut self, url: Url, result: Result[Response, HttpError]) -> Result[void, BodyError]`.
It claims a successful reply body once, including a retained Status(Response) body, before mutating the queue.
This fallible registration is an explicit approval item; today's reply returns void.

`received(&self) -> List[Request]` and each queued/repeat-last delivery materialize fresh unread bodies from stored snapshots.
Reading one returned history value must not poison stored history or later deliveries.
This does not revive the caller's original body: each delivery/history materialization is a new snapshot view.
The catalog checks these type shapes but does not implement or runtime-test the migrated fake.

Within one response and its ok()/Status copies, the body remains shared.
Reading a retained error body makes its original response alias used, while status checking itself consumes nothing.
New preparation errors are not transient by default; existing transport/status classifications retain their meanings.
Safe status Display must continue to omit sensitive query/fragment data and never consume body bytes.

SDK HttpServer's public accept/respond/client/cancellation/finish surface remains an ordinary SDK interface.
Its private wire accept returns scalar request identity plus URL/method/header/byte values; public accept wraps them into Incoming.
Public respond validates before committing and sends a fixed500 plus an escaped log reason on invalid headers.
Public client wraps the raw client for that same lease; returning a process-default Http provider would bypass request-scoped authority.

### Caching, incrementality and tests

No global parsed-URL, header or body cache is proposed.
Each value owns its parse result; normal module/std source fingerprints invalidate compiled callers.
SDK runtime/schema archives must be regenerated from the selected compiler when their inputs change.
There is no reason to weaken provenance checks or bless new baselines automatically.

Focused tests must cover both engines and checked compilation.
They must assert actual consumed state and exact wire bytes, rather than merely count builder method calls.
An implementation-neutral conformance case owns each new stdlib rule.
No exception is added to KNOWN_FAILURES.tsv for a promised message operation.

## Extension points

Applications supply ordinary Serialize/Deserialize implementations and typed error conversions.
They can build helpers around Request/Response without registering endpoints or adding a routing DSL.
Custom Http providers remain the capability extension point; HttpServer stays in the SDK.
No global registry or format-plugin loading mechanism is needed.

Streaming can later change the private Body representation while preserving single-use reads.
Its asynchronous readers, handle cleanup, limits and backpressure need their own approved design.
This note makes no claim that the existing streaming proposal is already implemented.

## Cost

Cost evidence is being collected from the pinned compiler and a local model.
The fresh baseline browser compiler is 5,893,314 raw bytes and 1,970,381 gzip bytes, using the repository build script.
This is a measured baseline, not a prediction for a future complete implementation.
It must separate the model's overhead from a complete std/host implementation estimate.
The existing http module is 769 physical lines, 587 nonblank noncomment code lines, and 32,333 bytes.
The SDK server module is 100 physical lines, 84 code lines, and 3,794 bytes.

The current std module partitions include Request storage/builders/send45 code lines, Response storage/read23, and ScriptedHttp30.
Those are existing implementation footprints, not credits subtracted blindly from a future replacement.
The model has 676 physical lines and 582 nonblank noncomment code lines.
Its modeled core subset is 332 code lines; imports, fixture URL, application conversions, loopback and tests are counted separately.

| Matched prototype component | Code lines | 1.5× review threshold | What is excluded |
| --- | ---: | ---: | --- |
| Body and model read error | 33 | 49 | Real typed causes and reused lossy text |
| HeaderName, ASCII folding, Eq/Hash | 80 | 120 | Future changes to the known inventory |
| Headers, validation and ordinary From | 60 | 90 | Hostile incoming host pair integration |
| HttpStatus, Eq/Hash | 74 | 111 | No generic enum mechanism |
| Message fields and constructors | 16 | 24 | Parser implementation |
| Message setters/readers | 42 | 63 | Existing outgoing form-writer reuse |
| Public Http/send forwarding | 5 | 7 | Default binding and real grants |
| Primitive wire/internal adapter | 22 | 33 | SDK lease wrappers and host I/O |

Thresholds use floor(1.5×reference code lines), consistently excluding blank and whole-line comment lines.
They apply only to each matched component, not a 332-line budget for the complete feature.
The URL fixture contributes22 code lines, app-owned header conversions13, loopback/example47, tests163 and imports5.
The model's simplified UTF-8 conversion cannot be used to claim a smaller correct text reader.

| Existing reusable/reference dependency | Source span | Physical lines | Code lines | Addition accounting |
| --- | --- | ---: | ---: | --- |
| Query append/shared encoder | lib/std/http.hd119–151 | 33 | 32 | Shared encoder counted once |
| Existing flat FormWriter | lib/std/http.hd156–278 | 123 | 104 | Reuse for body form writing only; no smart header writer |
| Client URL/redirect resolution | lib/std/http.hd413–531 | 119 | 113 | Reference/reuse, not a full public parser |
| Lossy UTF-8 conversion | lib/std/http.hd707–765 | 59 | 58 | Preserve existing code rather than invent strict decoding |
| Existing ASCII header comparison | lib/std/http.hd687–703 | 17 | 16 | Reuse/check against new canonical name operations |

No numerical implementation estimate is claimed for unbuilt URL algorithms, automatic adapter installation, or SDK schema/lease migration.
Each needs its own measured reference or bounded proof before assigning an implementation share.
The ordinary captured-adapter seam is9 code lines,13 including its two traits, but does not price default-profile integration.
Existing compiler reference chunks are29 code lines for host_vtable,9 for host_providers,7 for first-root construction, and6 for the profile list.

The rejected general-reference alternative touches existing codec/model/collection chunks:18/182/288/21/61 code lines respectively.
Those are code under review, not an additive implementation estimate or an authorization to edit it.
It still leaves built-in private-serde compatibility unanswered, which is why it is not the chosen route.

The pinned before/model application builds have these measured sizes:

| Program | Wasm bytes | Defined functions | Code-section payload bytes |
| --- | ---: | ---: | ---: |
| Current std client/server example | 218,465 | 510 | 183,726 |
| Selected local message model | 248,807 | 574 | 209,420 |
| Current hello probe | 7,140 | 62 | 4,262 |

The model is 30,342 bytes larger, with64 more defined functions and25,694 more code-section bytes.
It also includes local enum/message implementations and a different fake transport, so these are program footprints, not isolated library overhead.
This is not a claim that a complete replacement std/SDK implementation will have that delta.
The source hashes and compiler identity are recorded with the [prototype materials](http-messages/README.md).

The browser compiler baseline leaves29,619 gzip bytes below the2,000,000-byte cap.
No new std or compiler implementation is embedded by this docs-only job, so it establishes no future gzip delta.
The future implementation must measure it directly and stop before crossing that cap.
Programs without HTTP should acquire no new provider roots; byte-identity controls are part of that future proof.

The fresh native compiler build took14.49seconds and the browser build20.79seconds on this machine.
Those are single observed development build times, not benchmark estimates for future code.
No isolated compile-time, latency, throughput or peak-memory comparison was run for the proposed complete API.
The algorithms and constructor counts below bound mechanisms; they do not substitute for such measurements.

The selected Body needs one counted object per body, plus its byte storage.
That is an object-count statement, not a claim that a whole builder chain performs only one allocation.
A constructor can create an empty Body that a following set_json replaces, so the simple chain creates two logical Body objects.
The emitted runtime must be measured before claiming an optimization removes the first allocation.
A message copy retains that reference; it does not duplicate body bytes or reset the state.
Other(u16) remains lossless as a value, but platform/outgoing response status validation is still required independently.
The model's Other(777) enum test does not prove transport acceptance of777.
The rejected facade would add an internal counted object.
Exact runtime allocation behavior still depends on the emitted implementation and must be measured.

URL scanning is linear in input bytes; header first/all lookup is linear in the number of pairs.
An ordered list avoids a map's hidden duplicate-loss problem for the small common header set.
Large-body buffering remains bounded by the existing client/server limits; the API does not remove those limits.

Each implementation slice stops for review above 1.5 times its measured component budget.
Unknown adapter/ABI work cannot be hidden in a library-only estimate.
The web compiler's compressed Wasm must stay below the repository's hard 2 MB limit.

## Stress test

The fixed surface is the owner's message and receiver direction.
The examples are approximations of the named library workflows, not vendored implementations.
Works below means the specified observation is exercised in the local model; future transport or codec claims remain explicitly unproved.
Provisional decode-error, URL and wire details are tested as proposals, not recorded decisions.
The cases must exercise actual shared references and both client/server directions.

| Case | Real-world analogue | Required observation | Evidence / verdict |
| --- | --- | --- | --- |
| S1 typed JSON exchange | Fetch client and server | Same body vocabulary, distinct statically typed read results | Works in the loopback model; real host binding remains future. |
| S2 proxy head rewrite | Go ReverseProxy | URL/header edits leave the incoming head unchanged | Works for owned header changes; complete URL parsing is future. |
| S3 proxy body alias | Go Request.Clone | Reading either copy makes both observe used state | Works: the second alias returns Used. |
| S4 proxy replacement | Go GetBody / explicit new body | Replacing only the outgoing reference leaves the original body alone | Works: message setter detaches a new Body. |
| S5 malformed JSON | Fetch consume-body conversion | Failed decode still consumes; diagnostic retains typed cause | Works with the model Decode marker; preserving JsonError remains future. |
| S6 empty body | Fetch null-body edge | Explicitly test the proposed empty-body policy | Works under the proposed consume-empty policy; approval remains explicit. |
| S7 repeated cookies | Rust HeaderMap / HTTP semantics | Two Set-Cookie values survive adapters in order | Works in local Headers; real platform delivery is a future gate. |
| S8 encoded path and repeated query | Rust Uri / Go URL | No double decoding, no lost repeated values | Repeated values work in fixture Url; encoded/percent edge cases are unimplemented. |
| S9 preserved-body redirects | Go GetBody | One logical send can replay its prepared bytes | Unimplemented transport check; the ordinary fake does not follow redirects. |
| S10 retry after a send | Policy retry and HTTP clients | Fresh work builds a fresh Body; aliases cannot stand in for replay | Friction: alias resend fails before transport; replacing/building fresh work succeeds. |
| S11 incoming/outgoing forms | Ktor receiveText / existing hd FormWriter | Incoming text/bytes; outgoing typed serialization | Scoped simplification: incoming raw read works; outgoing set_form shape checks and reuses existing writer, with no new typed reader. |
| S12 cancellation and limits | Server SDK | No unbounded buffering, stale lease response or leaked pending operation | Local invalid-response500/log reason works; actual cancellation/lease/limits remain future. |
| S13 non-ASCII header normalization | RFC token validation / Unicode case mapping | Invalid CooKie stays invalid; ASCII Other("Cookie") equals/hashes as Cookie | Works after ASCII fix: rejected before transport; canonical Eq/Hash controls pass. |

## Caller migration

The first-pass inventory finds 39 conformance HD files, seven SDK HD files, twenty SDK JS files, and five stdlib spec chapters.
These are search matches, not a claim that every match requires an edit.
Historical prototypes and measured workloads must be distinguished from live examples before migration.
The guide also describes Request/Client as mutable builders without importing std.http in that paragraph.

The migration must update Request/Response literals, body readers, query construction, redirect settings, and handler signatures.
The SDK's HttpServer boundary, schema asset and Node/Fetch adapters need an explicit wire-preservation review.
Physical source identities in std-server-sdk.host/application.source/cloudflare.nominal-source/vercel.nominal-source change alongside their schema/preflight checks.
Public requirement rows continue to name HttpServer, while the physical host interface becomes the private wire contract.
Tests for deadlines, grants, backpressure and cleanup must retain their original purposes.
Do not weaken them merely to make new message constructors convenient.

Changing repeatable reads to single-use reads retires affected rule IDs rather than reusing their meanings.
New ordinary APIs belong in spec/std/http.md; existing language reference rules do not change.
Preserve Method's variant/payload equality; HeaderName/HttpStatus have their own handwritten stdlib Eq/Hash semantics.
The physical SDK source pins need corresponding rule replacements, not a claim that public HttpServer is still the raw host interface.
Any compiler-known boundary name or diagnostic belongs in the language tier only if genuinely needed.
Conformance fixtures ship with each approved implementation slice.

## Phasing

Each phase targets about one hour of agent work and starts only after this note's approval.
No phase is an excuse to ship an approved operation behind a known-failure exception.

| Phase | Independently green result | Focused proof |
| --- | --- | --- |
| 0 | Internal @host pub wire trait plus public adapter/default-profile binding is demonstrated | Actual URL/header/byte wire exchange, public rows, grants and fresh bodies on both engines |
| 1 | Owned URL/Headers atoms beside existing messages | Round trips, duplicate headers, ASCII grants, encoded paths |
| 2 | Single-use buffered Body and typed readers/writers | Shared-use, replacement, failed/empty JSON decode and outgoing form cases |
| 3 | Request/Response, send/SDK boundaries and every compile-dependent caller migrate atomically | Both-engine wire, redirects, grants, maintained SDK tests and generated schema |
| 4 | Public documentation and remaining nondependent historical examples are reconciled | Guide programs, policy cost reference compatibility and exact outputs |

The cut between phases 2 and 3 must avoid half-migrated public transport types.
Retain the existing message representation until the body, provider, SDK boundary and their active callers can change atomically.
A phase cannot defer a broken SDK literal or handler signature to the next phase.
If that atomic slice exceeds its budget, return for a smaller agreed cut before pushing.

## Recommendation and approvals

Recommendation: implement the owner's selected nongeneric message layer with one counted Body and ordinary owned head values.
Use one message-level read/write vocabulary in all directions, and make consumption visible through typed errors.
Use internal, unreexported @host pub wire traits and public ordinary adapters as the owner selected.
Keep redirects on the send operation and request leases in Incoming.
Do not add routing, generic envelopes, a facade allocation, or generic reference boundary lowering.
Price the narrow profile/adapter binding integration explicitly rather than calling it an existing library facility.

The approval review must settle the remaining exact signatures, error/empty-body policy, setter replacement/last-writer behavior, URL/query policy, and replay details.
The private wire route and message-level read/write naming are already decided; they are not reopened.
Checked examples and component evidence are included, with the unimplemented codecs/profile work called out rather than priced as zero.
Form scope and registration are settled: outgoing set_form only, and ordinary pub host traits in unreexported internal modules.
No @host rule change or typed form reader is proposed.
This draft is pushed for the owner's review before any implementation, as requested in6404135821.
Implementation is not authorized by a recommendation alone.

## Sources and parse log

Source inspection and current-build probes pin `6ad81d02e5002f86b5f97ea2d1508fa11555ffbb`.
The native dev compiler SHA-256 is `58721916b85652ad9801b45f95b462d5be8e9e97f24e7b263c1b2caf02aa3294`.
Its features include wasmtime; every HD run uses scratch HD_CACHE inside the isolated worktree.
The initial current-API probe checks with zero errors/warnings and runs identically on Node and Wasmtime.

The reference APIs above are primary-source observations, not hd semantic authority.
The parse log distinguishes displayed excerpts, complete executable models and signature-only stubs.
The publication checks use these files directly, not a substitute source with the same public names.
No proposed API is claimed to exist in std today.

| Material | Parse | Type/format check | Execution |
| --- | --- | --- | --- |
| Displayed block1: current API before program | Pass | Complete before package,0errors/warnings; format pass | Both engines:42, empty stderr |
| Displayed block2: modeled server handler | Pass | Included verbatim in complete model | Exercised by typed loopback cases |
| Displayed block3: modeled client helpers | Pass | Included verbatim in complete model | Both engines:42, empty stderr |
| Full model source | Compiler parse during check |0errors/warnings; format pass |13tests on Node and13on Wasmtime pass |
| API catalog | Compiler parse during check |0errors/warnings; format pass | Not run: explicit shape-only stubs |

The copied publication packages produce the exact recorded Wasm hashes and sizes, not merely matching stdout.
The prototype README gives reproduction commands; every run uses a scratch HD_CACHE.
The three Markdown examples were separately parsed with scripts/hd-conformance.sh.
A Markdown-it walk checks all30local links and anchors, avoiding false links inside generic method signatures.

Current docs gates pass: spec style7616rule IDs, phase audit2324diagnostic rows, spec check, and website83pages.
The rewrite inventory against6ad81d02e has0added/retired/reused/changed IDs,0lost codes and0lost examples.
This is a design-only branch: no accepted spec/std/host/compiler implementation or full local test suite.
The audit log records18actual writing/command mistakes once, including the rejected header-writer prototype's escaped-control mistake.
Expected negative boundary/registration probes are evidence, not programming mistakes.
