# Http

Status: standard library specification draft.

This chapter defines `std.http`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Http`, and the helpers `send!`, `get!`,
  `get_text!`, and `post_json!`;
- the struct values `Request` and `Response`, and the enums `Method` and
  `HttpError`;
- a status outside 200 to 299 as an error, and the JSON calls;
- which request URLs are valid;
- `ScriptedHttp`, the deterministic provider of `Http`;
- what the playground binds for `Http`, and how a `block_on` call waits
  there.

Which provider a command binds, and which hosts its grant covers, is CLI
tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).

## Requests And Responses

A request and a response are plain struct values:

```text
use std.time.Duration

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
    pub headers: List[(string, string)] = []
    pub body: List[u8] = []
    pub timeout: Duration? = .None

pub struct Response:
    pub status: u16
    pub headers: List[(string, string)]
    pub body: List[u8]
    pub url: string = ""
```

1. r[std-http.types.decl-struct] `std.http` declares `Method`, `Request`, and `Response` as above. `Request` and `Response` are struct values: a copy is independent of the value it was copied from. Code imports them, as in `use std.http.{Request, Response}`.
2. r[std-http.types.traits] `Method`, `Request`, and `Response` implement `Eq` and `Debug`.
3. r[std-http.request.defaults] A `Request` that gives only `url` is a `Get` with no headers, an empty body, and the provider's default timeout.
4. r[std-http.request.default-timeout] A provider that sends requests to a host has a default timeout of 30 seconds.
5. r[std-http.headers.pairs] Headers are a list of name and value pairs, so a repeated header such as `Set-Cookie` keeps every value, in order.
6. r[std-http.headers.case] Header names compare ignoring ASCII case, so a provider may change the case of a response header's name.
7. r[std-http.headers.order] In a response, only the values of one name keep their order. Headers of different names may come in any order.
8. r[std-http.response.header] `Response` has the method `header(self, name: string) -> string?`. It returns the first value whose name equals `name`, ignoring ASCII case.
9. r[std-http.response.text] `Response` has the method `text(self) -> string`. It decodes the body as UTF-8, with U+FFFD for each byte sequence that is not valid UTF-8.
10. r[std-http.response.url] `Response.url` is the URL of the last request, after every redirect. `ScriptedHttp` sets it to the request's URL, and a response built by hand has `""` unless it names one.
11. r[std-http.method.eq.variant-payload] Two `Method` values compare equal only when their variants and corresponding payloads are equal.

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
    Status(status: u16, url: string)
    Json(error: JsonError)
```

1. r[std-http.error.decl] `std.http` declares `HttpError` as above. Code imports it, as in `use std.http.HttpError`.
2. r[std-http.error.traits] `HttpError` implements `Eq`, `Debug`, `Display`, and `std.error.Error`.

| Rule | Variant | Means |
| --- | --- | --- |
| r[std-http.error.not-granted] Not granted | `NotGranted(host)` | the program's capability grant does not cover the request's host, by [Partial Deny](../cli/command-line.md#partial-deny) |
| r[std-http.error.invalid-url] Invalid URL | `InvalidUrl(url)` | `url` is not an absolute `http` or `https` URL |
| r[std-http.error.dns] DNS | `Dns(host)` | the host name did not resolve |
| r[std-http.error.connect] Connect | `Connect(message)` | no connection was made, or it failed before the whole response arrived |
| r[std-http.error.tls] TLS | `Tls(message)` | the TLS handshake or a certificate check failed |
| r[std-http.error.timeout] Timeout | `Timeout` | the request took longer than its timeout |
| r[std-http.error.redirects] Redirects | `TooManyRedirects(url)` | the response redirected more than 10 times; `url` is the last target |
| r[std-http.error.other] Other | `Other(message)` | any other failure, with the provider's message |
| r[std-http.error.status] Status | `Status(status, url)` | `Response.ok` found a status outside 200 to 299; `url` is the response's `url` |
| r[std-http.error.json] JSON | `Json(error)` | `Response.json` could not decode the body; `error` is the `std.json.JsonError` |

3. r[std-http.error.not-granted.text] The `Display` text of `NotGranted(host)` is `http access to HOST is not granted; run with --cap Http=HOST`, with the host for `HOST`.
4. r[std-http.error.status.text] The `Display` text of `Status(status, url)` is `HTTP status STATUS for URL`, with the status and the URL, for every URL, an empty one included.
5. r[std-http.error.json.text] The `Display` text of `Json(error)` is `invalid JSON in the response: ERROR`, with the `Display` text of `error`.

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
4. r[std-http.send.redirect] `send!` follows up to 10 redirects.
5. r[std-http.send.timeout] A `timeout` of `.None` means the provider's default. A request that takes longer than its timeout completes with `.Err(HttpError.Timeout)`.
6. r[std-http.send.timeout.whole] A timeout covers the whole request, every redirect included.
7. r[std-http.send.cancel] Cancelling the suspension of `send!` aborts the request, by [`req.cancel.external-abort`](../lang/11-requirements-and-suspension.md#r-req.cancel.external-abort).
8. r[std-http.send.cancel.close] Aborting a request closes its connection.
9. r[std-http.send.mut-ref.reference-values] `send!` takes `&mut self`, so `Http` is a mutable requirement trait, `$.use(Http)` is a `&mut dyn Http` by [`req.ref.use.reference-values`](../lang/11-requirements-and-suspension.md#r-req.ref.use.reference-values), and a provider may record what it sends.
10. r[std-http.bodies] A body is a whole list of bytes. `std.http` has no streaming body.
11. r[std-http.helper.send] `std.http` declares `pub fn send!(request: Request) -> Result[Response, HttpError] $ Http`, which calls `send!(request)` on the `Http` provider that covers the call.
12. r[std-http.helper.get] `std.http` declares `pub fn get!(url: string) -> Result[Response, HttpError] $ Http`, which sends `Request { url: url }` the same way.

> **Why.** A status is data about a request that worked, as with `fetch`
> and Go, and as a non-zero exit status is for `Process`. Header pairs
> keep a repeated header, which a map would lose.

## Status And JSON

A status outside 200 to 299 is an error only when the code asks for it.
`ok` asks, and `get_text!` asks for the common one-line fetch:

```text
use std.http.{Http, HttpError, get_text, post_json}
use std.json.JsonError
use std.serde.{Deserialize, Serialize}

@derive(Serialize)
struct NewUser:
    name: string

@derive(Deserialize)
struct Created:
    id: i64

fn notes!() -> Result[string, HttpError] $ Http:
    get_text!("https://example.com/notes.txt")    # a 404 is .Err(HttpError.Status(404, url))

fn create!(name: string) -> Result[Created, HttpError] $ Http:
    response := post_json!("https://example.com/users", NewUser { name: name })?
    response.ok()?.json::[Created]()              # a bad body is .Err(HttpError.Json(error))
```

1. r[std-http.response.ok] `Response` has the method `ok(self) -> Result[Response, HttpError]`. It returns `.Ok(self)` for a `status` from 200 to 299, and `.Err(HttpError.Status(status, url))` for any other, with the response's `status` and `url`.
2. r[std-http.response.json] `Response` has the method `json::[T < Deserialize](self) -> Result[T, HttpError]`. It decodes `self.text()` with `std.json.decode::[T]`, and returns `.Err(HttpError.Json(error))` for the `JsonError` it fails with. It does not look at `status`.
3. r[std-http.helper.get-text] `std.http` declares `pub fn get_text!(url: string) -> Result[string, HttpError] $ Http`. It sends `get!(url)`, applies `ok`, and returns `.Ok` of the response's `text()`, or the error of either step.
4. r[std-http.helper.post-json] `std.http` declares `pub fn post_json![T < Serialize](url: string, value: T) -> Result[Response, HttpError] $ Http`. It sends a `Post` of `url` with the header `("Content-Type", "application/json")` and the body `std.json.encode(value)` as UTF-8 bytes, and returns the response whatever its status.

> **Why.** Reading a page's text must be one call, and it must not return
> a 404 page as the text. `send!` stays status-neutral
> ([`std-http.send.status`](#r-std-http.send.status)); `ok` is the opt-in,
> as reqwest's `error_for_status` is.

## Scripted Http

`ScriptedHttp` is the deterministic `Http` provider. It answers each URL
from a table, and records every request:

```text
use std.http.{Http, Response, ScriptedHttp, get}
fn status!(url: string) -> u16 $ Http:
    match get!(url):
        .Ok(response) => response.status
        .Err(_) => 0

tests:
    use std.testing.assert_equal

    it("reads the scripted status"):
        http := ScriptedHttp::new({"https://example.com/": Response { status: 204, headers: [], body: [] }})
        $.with(Http=http):
            assert_equal(status!("https://example.com/"), 204, reason="scripted")
        assert_equal(http.sent().len(), 1, reason="one request")
```

1. r[std-http.scripted.decl] `std.http` declares `ScriptedHttp`, which implements `Http` and `Debug`, with private fields. Code imports it, as in `use std.http.ScriptedHttp`.
2. r[std-http.scripted.new-ref] `ScriptedHttp::new(responses: Map[string, Response]) -> &mut ScriptedHttp` returns a new object holding a provider that answers each URL in `responses`. Installing it as `Http=http` shares the object, so the test reads back what was sent.
3. r[std-http.scripted.send] `send!(request)` returns `.Ok` of the response that `responses` holds for `request.url`, matched by its exact text, whatever the method, with `url` set to `request.url`.
4. r[std-http.scripted.unknown] For a URL that `responses` lacks, `send!` returns `.Err(HttpError.Connect(message))`, and `message` is `ScriptedHttp has no response for URL`, with the URL.
5. r[std-http.scripted.sent-ref] `sent(&self) -> List[Request]` returns every request that the provider received, in order, the unanswered ones included.
6. r[std-http.scripted.no-host] A `ScriptedHttp` opens no connection.

> **Why.** An unknown URL is an error that names it, not a 404. So a
> route that a test forgot never hides behind a valid status.

## Playground

The playground runs a program in a browser. Its `Http` provider calls the
browser's `fetch`:

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
