# Http

Status: standard library specification draft.

This chapter defines `std.http`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Http`, and the free helper `get!`;
- the struct values `Request` and `Response`, and the enums `Method` and
  `HttpError`;
- status checking, retained error responses, and the JSON calls;
- which request URLs are valid;
- `ScriptedHttp`, the deterministic provider of `Http`;
- what the playground binds for `Http`, and how a `block_on` call waits
  there.

Which provider a command binds, and which hosts its grant covers, is CLI
tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).

## Requests And Responses

A request and a response are plain struct values:

```text
pub enum Method:
    Get
    Head
    Post
    Put
    Patch
    Delete
    Options
    Other(name: string)

pub struct Request:
    pub method: Method = Method.Get
    pub url: string
    pub query: List[(string, string)] = []
    pub headers: List[(string, string)] = []
    pub body: List[u8] = []
    pub redirects: usize = 10

pub struct Response:
    pub status: u16
    pub headers: List[(string, string)] = []
    pub body: List[u8] = []
    pub url: string = ""
```

1. r[std-http.types.builder-response-defaults] `std.http` declares `Method`, `Request`, and `Response` as above.
2. r[std-http.types.traits] `Method`, `Request`, and `Response` implement `Eq` and `Debug`.
3. r[std-http.request.builder-defaults] A `Request` that gives only `url` defaults to `Get`, empty query and header lists, an empty body, and ten redirects.
4. r[std-http.request.default-timeout] A provider that sends requests to a host has a default timeout of 30 seconds.
5. r[std-http.headers.pairs] Headers are a list of name and value pairs, so a repeated header such as `Set-Cookie` keeps every value, in order.
6. r[std-http.headers.case] Header names compare ignoring ASCII case, so a provider may change the case of a response header's name.
7. r[std-http.headers.order] In a response, only the values of one name keep their order. Headers of different names may come in any order.
8. r[std-http.response.header] `Response` has the method `header(self, name: string) -> string?`. It returns the first value whose name equals `name`, ignoring ASCII case.
9. r[std-http.response.text] `Response` has the method `text(self) -> string`. It decodes the body as UTF-8, with U+FFFD for each byte sequence that is not valid UTF-8.
10. r[std-http.response.url] `Response.url` is the URL of the last request, after every redirect. `ScriptedHttp` sets it to the request's URL, and a response built by hand has `""` unless it names one.
11. r[std-http.method.eq.variant-payload] Two `Method` values compare equal only when their variants and corresponding payloads are equal.
12. r[std-http.types.value-copy] A `Request` or `Response` copy is independent of the value it was copied from.

13. r[std-http.response.omitted-lists] A response's omitted `headers` and `body` fields default to empty lists.

## Request Builder

A request builder accumulates fields before its terminal `send!` call.
For example, `Request::get(url).query("page", 2).header("Accept", "text/plain").send!()` sends a GET with an encoded page number.

1. r[std-http.request.new] `Request::new(method: Method, url: string) -> *mut Request` returns a new request with those fields and the other defaults.
2. r[std-http.request.get] `Request::get(url: string) -> *mut Request` returns `Request::new(.Get, url)`.
3. r[std-http.request.post] `Request::post(url: string) -> *mut Request` returns `Request::new(.Post, url)`.
4. r[std-http.request.header] `header(*mut self, name: string, value: string) -> *mut Request` appends the header pair and returns the same builder.
5. r[std-http.request.query] `query[T < Display](&mut self, name: string, value: T) -> &mut Request` appends `(name, "$value")` and returns the same builder.
6. r[std-http.request.body] `body(*mut self, bytes: List[u8]) -> *mut Request` replaces the body and returns the same builder.
7. r[std-http.request.redirects] `redirects(*mut self, count: usize) -> *mut Request` sets the redirect limit and returns the same builder.
8. r[std-http.request.send] `send!(self) -> Result[Response, HttpError] $ Http` sends a copy with its encoded URL through the covering `Http` provider.
9. r[std-http.request.query-encoding] Query names and values encode their UTF-8 bytes with uppercase percent escapes, keeping only RFC 3986 unreserved bytes.
10. r[std-http.request.query-order] Encoding preserves query pair order and repeated names, joining each encoded name and value with `=` and pairs with `&`.
11. r[std-http.request.query-url] Sending appends the encoded query before any fragment, using `?` without an existing query and `&` within one.
12. r[std-http.request.query-separator] Within an existing query, no extra separator is inserted after a trailing `?` or `&`.
13. r[std-http.request.send-copy] Sending leaves the builder unchanged, so sending it again encodes its query once again from the original fields.
14. r[std-http.send.zero-redirects-supported] With `redirects` zero, Node and Wasmtime providers return the first response without following or validating its `Location`.


15. r[std-http.request.json-serialize] `json[T < Serialize](&mut self, value: T) -> &mut Request` replaces the body with the UTF-8 bytes of `std.json.encode(value)`.
16. r[std-http.request.json-once] `json` encodes its value once.
17. r[std-http.request.json-content-type] `json` appends `Content-Type: application/json` only when no existing header names `Content-Type`, ignoring ASCII case.
18. r[std-http.request.form-serialize] `form[T < Serialize](&mut self, value: T) -> &mut Request` replaces the body with a flat form encoding of the serialized value.
19. r[std-http.request.form-root] The form root is a record or a `Map[string, string]`.
20. r[std-http.request.form-scalars] A record member may serialize as a boolean, number, string, or payload-free enum variant.
21. r[std-http.request.form-enum] A payload-free enum member encodes as its variant name.
22. r[std-http.request.form-optional] An optional member encodes its present value or contributes no field when absent.
23. r[std-http.request.form-list] A list of scalar values repeats its member name once per item, in list order.
24. r[std-http.request.form-escaping] Form names and values encode UTF-8 bytes, keeping only ASCII letters, digits, `*`, `-`, `.`, and `_` unchanged.
25. r[std-http.request.form-space] Form encoding writes a space as `+` and each remaining escaped byte as `%` followed by two uppercase hexadecimal digits.
26. r[std-http.request.form-pairs] Form encoding joins each name and value with `=` and each pair with `&`.
27. r[std-http.request.form-content-type] `form` appends `Content-Type: application/x-www-form-urlencoded` only when no existing header names `Content-Type`, ignoring ASCII case.
28. r[std-http.request.typed-body-builder] Both typed body setters return the same builder.
29. r[std-http.request.form-unsupported] A root scalar, list, or enum, or a nested record, map, list, or payload-bearing enum panics. Panic: `explicit-panic`.
30. r[std-http.request.form-member-message] The unsupported-form panic message names the offending member, or `$` for the root.
31. r[std-http.request.form-wire-name] A form record member uses its serde wire name, including a `Rename` fact, as its field name.


> **Why.** Typed values share their serialization implementation across
> JSON and forms. Flat form fields match ordinary HTML submissions; nested
> shapes need an application-specific encoding instead of an implicit convention.

The provider's default timeout still applies; `std.task.timeout!` can impose a shorter bound around a send.

## Client

A client shares a base URL, default headers and host-only cookies across requests. A caller
adds a deadline by composing `std.task.timeout!` around `client.send!`.

1. r[std-http.client.http-only] `std.http` exports the struct `Client` with private fields.
2. r[std-http.client.new-empty-jar] `Client::new() -> *mut Client` returns a new client without a base URL, default headers or cookies.
3. r[std-http.client.base-url] `base_url(*mut self, url: string) -> *mut Client` replaces the base URL and returns the same client.
4. r[std-http.client.default-header] `header(*mut self, name: string, value: string) -> *mut Client` appends a default header pair and returns the same client.
5. r[std-http.client.send-http-only] `send!(&mut self, request: Request) -> Result[Response, HttpError] $ Http` sends through the covering `Http` provider.
6. r[std-http.client.resolve-root-url] A request URL starting with `/` resolves against the configured base URL.
7. r[std-http.client.absolute-url] An absolute request URL is used unchanged.
8. r[std-http.client.merge-defaults] Sending adds every default header whose name the request does not name, ignoring ASCII case.
9. r[std-http.client.default-initial-hosts] Default headers apply to the initial host of every client send.
10. r[std-http.client.request-unchanged] Sending leaves the supplied request unchanged.
11. r[std-http.client.unchecked-final-response] Sending returns the final response without checking its status, or propagates a provider error unchanged.
12. r[std-http.client.external-timeout] `Client` has no timeout setter; a deadline uses `std.task.timeout!` around the send.

13. r[std-http.client.cookie] `cookie(self, url: string, name: string) -> string?` returns the stored cookie eligible for that URL, resolved against `base_url`.
14. r[std-http.client.cookies] Each response updates the jar from its `Set-Cookie` header pairs before the client follows another redirect.
15. r[std-http.client.cookies.replace] A cookie replaces the previous value with the same host and name.
16. r[std-http.client.cookies.host-only] Cookies belong only to the response request's hostname, ignoring ASCII case and ports; a `Domain` attribute does not widen their scope.
17. r[std-http.client.cookies.secure] A cookie with the `Secure` attribute is eligible only for an `https` URL.
18. r[std-http.client.cookies.delete] A `Max-Age=0` attribute removes the cookie with that host and name.
19. r[std-http.client.cookies.attribute-case] Cookie attribute names and the `Set-Cookie` header name compare without ASCII case.
20. r[std-http.client.cookies.send] Without an explicit `Cookie` header, each request sends its eligible stored cookies as `name=value` pairs joined by `; `.
21. r[std-http.client.cookies.explicit] An explicit `Cookie` header takes precedence over the jar for that request without replacing stored cookies.
22. r[std-http.client.redirects.manual] The client sends each provider request with `redirects: 0` and follows redirects itself.
23. r[std-http.client.redirects.status] Responses with status 301, 302, 303, 307 or 308 and a `Location` header are redirects.
24. r[std-http.client.redirects.zero] A zero request redirect limit returns the first response without following its `Location`.
25. r[std-http.client.redirects.method] A 303 changes the method to `Get`; a 301 or 302 changes `Post` to `Get` and preserves other methods.
26. r[std-http.client.redirects.body] Changing a method to `Get` removes its body and its `Content-Type`, `Content-Length` and `Transfer-Encoding` headers.
27. r[std-http.client.redirects.resolve] A redirect's `Location` resolves against the current request URL as an RFC 3986 reference.
28. r[std-http.client.redirects.cross-host] A redirect to another hostname removes the previous request's `Authorization` and `Cookie` headers before applying the target host's jar.
29. r[std-http.client.redirects.limit] Exceeding a positive request redirect limit returns `HttpError.TooManyRedirects(url)`, naming the next target without sending to it.
30. r[std-http.client.redirects.no-location] A response without `Location` is returned without following another request.
31. r[std-http.client.redirects.response-url] The returned response's `url` is the final request URL.

> **Why.** Reading every response lets the client retain cookies set during
> redirects; delegating redirects to the provider would hide those responses.
> Host-only storage avoids relying on a public-suffix list.


## URLs

A request URL is an RFC 3986 URL, so its host is plain ASCII:

```hd
use std.http.{Http, HttpError, Response, get}

fn shop!() -> Result[Response, HttpError] $ Http:
    _ := get!("https://bücher.example/")         # .Err(HttpError.InvalidUrl(url))
    get!("https://xn--bcher-kva.example/")       # the same host in punycode
```

1. r[std-http.url.valid] A request URL is valid when it is an absolute `http` or `https` URL by RFC 3986, with an ASCII host.
2. r[std-http.url.non-ascii] A URL whose host holds a non-ASCII character is invalid, so `send!` returns `.Err(HttpError.InvalidUrl(url))` for it.
3. r[std-http.url.grant] A grant matches a request by its URL host, in lowercase ASCII.
4. r[std-http.url.no-normalize] A provider maps the host no other way: no IDNA mapping and no lookalike normalization.

> **Why.** An ASCII-only host leaves one spelling for each host, so a
> grant and the request it covers always compare the same text.

## Http Errors

Every failure of a request is one error enum:

```text
pub enum HttpError:
    NotGranted(host: string)
    InvalidUrl(url: string)
    Dns(host: string)
    Connect(message: string)
    Tls(message: string)
    Timeout
    TooManyRedirects(url: string)
    Other(message: string)
    Status(response: Response)
```

1. r[std-http.error.decl-retained-response] `std.http` declares `HttpError` as above. Code imports it, as in `use std.http.HttpError`.
2. r[std-http.error.traits] `HttpError` implements `Eq`, `Debug`, `Display`, and `std.error.Error`.

| Rule | Variant | Means |
| --- | --- | --- |
| r[std-http.error.not-granted] Not granted | `NotGranted(host)` | the program's capability grant does not cover the request's host, by [Partial Deny](../cli/command-line.md#partial-deny) |
| r[std-http.error.invalid-url] Invalid URL | `InvalidUrl(url)` | `url` is not an absolute `http` or `https` URL |
| r[std-http.error.dns] DNS | `Dns(host)` | the host name did not resolve |
| r[std-http.error.connect] Connect | `Connect(message)` | no connection was made, or it failed before the whole response arrived |
| r[std-http.error.tls] TLS | `Tls(message)` | the TLS handshake or a certificate check failed |
| r[std-http.error.timeout] Timeout | `Timeout` | the request took longer than its timeout |
| r[std-http.error.redirect-limit] Redirects | `TooManyRedirects(url)` | the response exceeded a positive `Request.redirects` limit; `url` is the last target |
| r[std-http.error.other] Other | `Other(message)` | any other failure, with the provider's message |
| r[std-http.error.status-response-value] Status | `Status(response)` | `Response.ok` found a status from 400 to 599, retaining the whole response |

3. r[std-http.error.not-granted.plain] The `Display` text of `NotGranted(host)` is `http access to HOST is not granted`, with the host for `HOST`.
4. r[std-http.error.status.safe-response-text] `Status(response)` displays `HTTP status STATUS for URL`, with the response status and its URL before any query or fragment.
5. r[std-http.error.status.no-response-secrets] The status error's `Display` text contains no response body or header values.
6. r[std-http.error.transient-kinds] `HttpError.is_transient(self) -> bool` returns true for `Timeout` and `Connect` errors.
7. r[std-http.error.transient-status-codes] A `Status` error is transient for 408, 429, 500, 502, 503, and 504.
8. r[std-http.error.permanent-kinds] `is_transient` returns false for every other error.

## Sending

`Http` sends one request and completes with the whole response, as Deno's
`fetch` and Go's `Client.Do` do:

```text
use std.http.{Http, HttpError, get}

fn latest_release!(repo: string) -> Result[string, HttpError] $ Http:
    response := get!("https://api.github.com/repos/${repo}/releases/latest")?
    if response.status != 200:
        return .Err(.Other("status ${response.status}"))
    .Ok(response.text())
```

1. r[std-http.trait.decl-ref] `std.http` declares the host capability trait `Http`, with the one method `fn send!(&mut self, request: Request) -> Result[Response, HttpError]`. Code imports it, as in `use std.http.Http`.
2. r[std-http.send] `send!(request)` sends `request`, and completes with the whole response.
3. r[std-http.send.status] Every status, 404 and 500 included, is an `.Ok` response, and the caller reads it from `status`.
4. r[std-http.send.redirect-limit] `send!` follows at most `Request.redirects` redirects.
5. r[std-http.send.provider-timeout] A host request exceeding the provider's default timeout completes with `.Err(HttpError.Timeout)`.
6. r[std-http.send.timeout.whole] A timeout covers the whole request, every redirect included.
7. r[std-http.send.cancel] Cancelling the suspension of `send!` aborts the request, by [`req.cancel.external-abort`](../lang/11-requirements-and-suspension.md#r-req.cancel.external-abort).
8. r[std-http.send.cancel.close] Aborting a request closes its connection.
9. r[std-http.send.mut-ref.reference-values] `send!` takes `&mut self`, so `Http` is a mutable requirement trait, `$.use(Http)` is a `&mut dyn Http` by [`req.ref.use.reference-values`](../lang/11-requirements-and-suspension.md#r-req.ref.use.reference-values), and a provider may record what it sends.
10. r[std-http.bodies] A body is a whole list of bytes. `std.http` has no streaming body.
11. r[std-http.helper.get.checked-response] `get!(url: string) -> Result[Response, HttpError] $ Http` returns `Request::get(url).send!()?.ok()`.
12. r[std-http.helper.only-get] `get!` is the only free HTTP request helper; `send!`, `get_text!`, and `post_json!` are not exported.

> **Why.** A status is data about a request that worked, as with `fetch`
> and Go, and as a non-zero exit status is for `Process`. Header pairs
> keep a repeated header, which a map would lose.

## Status And JSON

`Response.ok()` checks 400–599 while preserving the whole response in the
error; the free `get!` calls it automatically:

```text
use std.error.Error
use std.http.{Http, HttpError, Request, get}
use std.serde.{Deserialize, Serialize}

@derive(Serialize)
struct NewUser:
    name: string

@derive(Deserialize)
struct Created:
    id: i64

fn notes!() -> Result[string, HttpError] $ Http:
    .Ok(get!("https://example.com/notes.txt")?.text())   # a 404 is .Err(HttpError.Status(response))

fn create!(name: string) -> Result[Created, *dyn Error] $ Http:
    request := Request::post("https://example.com/users").json(NewUser { name: name })
    .Ok(request.send!()?.ok()?.json::[Created]()?)              # a bad body is a JsonError
```

1. r[std-http.response.ok.retained-error] `Response.ok(self) -> Result[Response, HttpError]` returns `.Err(HttpError.Status(self))` for status 400–599 and `.Ok(self)` otherwise.
2. r[std-http.response.json-decode] `Response` has the method `json::[T < Deserialize](self) -> Result[T, JsonError]`. It decodes `self.text()` with `std.json.decode::[T]` and returns its result: a body that is not valid JSON for `T` is the decoder's `JsonError`, not an `HttpError`. It does not look at `status`.
3. r[std-http.response.json-propagate] A function that sends and decodes propagates both errors with `?` into an error type covering both, such as `&dyn Error` ([`expr.try.convert`](../lang/05-expressions.md#r-expr.try.convert)).

> **Why.** Builder and provider `send!` calls keep every status available
> ([`std-http.send.status`](#r-std-http.send.status)). `Response.ok()` checks
> explicitly, as reqwest's `error_for_status` does; the free `get!` checks by default.
> Retaining the response lets a caller decode an API's error body.

## Scripted Http

`ScriptedHttp` is the deterministic `Http` provider. It consumes replies queued
for each URL, and records every request:

```text
use std.http.{Http, Response, ScriptedHttp, get}
fn status!(url: string) -> u16 $ Http:
    match get!(url):
        .Ok(response) => response.status
        .Err(_) => 0

tests:
    use std.testing.assert_equal

    it("reads the scripted status"):
        http := ScriptedHttp::new()
        http.reply("https://example.com/", .Ok(Response { status: 204, headers: [], body: [] }))
        $.with(Http=http):
            assert_equal(status!("https://example.com/"), 204, reason="scripted")
        assert_equal(http.received().len(), 1, reason="one request")
```

1. r[std-http.scripted.decl] `std.http` declares `ScriptedHttp`, which implements `Http` and `Debug`, with private fields. Code imports it, as in `use std.http.ScriptedHttp`.
2. r[std-http.scripted.new-empty] `ScriptedHttp::new() -> *mut ScriptedHttp` returns a new provider with no replies and no received requests.
3. r[std-http.scripted.reply] `reply(&mut self, url: string, result: Result[Response, HttpError]) -> void` appends a result for that URL.
4. r[std-http.scripted.key-text] Replies match the complete request URL exactly, after query encoding, whatever the request method.
5. r[std-http.scripted.queue-order] Sends consume queued results in insertion order, independently for each URL.
6. r[std-http.scripted.repeat-last] After a URL's queued results have been consumed, further sends repeat its last result.
7. r[std-http.scripted.transport-errors] A queued `.Err` is returned unchanged.
8. r[std-http.scripted.raw-status] Every status of a queued successful response is returned as `.Ok`, without a status check.
9. r[std-http.scripted.missing-reply] A URL without a reply returns `.Err(HttpError.Connect("ScriptedHttp has no response for URL"))`, substituting its full URL.
10. r[std-http.scripted.received-ref] `received(&self) -> List[Request]` returns all received requests in order, including those answered with errors or lacking a reply.
11. r[std-http.scripted.no-host] A `ScriptedHttp` opens no connection.

> **Why.** An unknown URL is an error that names it, not a 404. So a
> route that a test forgot never hides behind a valid status.

## Playground

The playground runs a program in a browser. Its `Http` provider calls the
browser's HTTP facilities:

```text
use std.http.{Http, HttpError, get}

fn ping!(url: string) -> string $ Http:
    match get!(url):
        .Ok(response) => "status ${response.status}"
        .Err(.NotGranted(host)) => "the playground grants no ${host}"  # another origin
        .Err(.Connect(_)) => "the browser refused the request"         # a network failure or CORS
        .Err(error) => "${error}"
```

1. r[std-http.playground.origin] The playground binds `Http` with a grant that covers only its own origin, as the browser's same-origin rule does.
2. r[std-http.playground.other-origin] A request to another origin returns `.Err(HttpError.NotGranted(host))`.
3. r[std-http.playground.refused] A request that the browser refuses, for a network failure or by CORS, returns `.Err(HttpError.Connect(message))`.
4. r[std-http.playground.unbound] The playground binds no `Process`, `Sys`, or `Net` provider.
5. r[std-http.playground.zero-redirects-error] A granted browser request with `redirects: 0` returns `.Err(HttpError.Other("the browser host cannot stop redirects"))` before sending.

> **Why.** A browser can't tell a CORS refusal from a network failure, so
> neither can the provider. The playground needs no grant table: the
> browser's own rule decides.

### Waiting In The Playground

A [`block_on`](../lang/11-requirements-and-suspension.md#r-req.drive.block-on.signature)
call in the playground may have to wait for the host, as for a response
to `get!`. How it waits depends on the browser:

| Browser | A `block_on` that waits on the host |
| --- | --- |
| has JavaScript Promise Integration | waits for the host operation, as on any other host |
| lacks it | completes an HTTP request as a synchronous same-origin request, and panics for any other host operation |

1. r[std-http.playground.block-on.jspi] In a browser with JavaScript Promise Integration, a `block_on` call in the playground waits for each host operation it needs.
2. r[std-http.playground.block-on.sync] In a browser without it, an `Http` request that a `block_on` call waits on is made as a synchronous same-origin request.
3. r[std-http.playground.block-on.other] In such a browser, a `block_on` call that must wait on any other host operation panics, with a message that names the operation. Panic: `host-contract`.

```hd
use std.http.{Http, get}
use std.task.block_on

fn status_of(url: string) -> string $ Http:
    match block_on(get(url)):   # without Promise Integration: a synchronous request
        .Completed(.Ok(response)) => "status ${response.status}"
        .Completed(.Err(error)) => "${error}"
        .Cancelled => "cancelled"
        .Abandoned => "cut short"
```

> **Why.** A browser thread cannot block, so only Promise Integration
> lets Wasm wait for a Promise. Without it, a synchronous request is the
> one wait a browser still offers, and only for HTTP.

See also: [Capability Grants](../cli/command-line.md#capability-grants),
[Process](process.md), [Time](time.md#duration).


## Timeout Conversion

1. r[std-http.error.from-timed-out] `HttpError` implements `std.convert.From[std.task.TimedOut]`, converting every deadline error to `.Timeout`.
