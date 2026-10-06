# Http

Status: standard library specification draft.

This chapter defines `std.http`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Http`, and the helpers `send!` and `get!`;
- the data types `Request` and `Response`, and the enums `Method` and
  `HttpError`;
- `ScriptedHttp`, the deterministic provider of `Http`;
- what the playground binds for `Http`.

Which provider a command binds, and which hosts its grant covers, is CLI
tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).

## Requests And Responses

A request and a response are plain data:

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

pub data Request:
    pub method: Method = Method.Get
    pub url: string
    pub headers: List[(string, string)] = []
    pub body: List[u8] = []
    pub timeout: Duration? = .None

pub data Response:
    pub status: u16
    pub headers: List[(string, string)]
    pub body: List[u8]
```

1. r[std-http.types.decl] `std.http` declares `Method`, `Request`, and `Response` as above. Code imports them, as in `use std.http.{Request, Response}`.
2. r[std-http.types.traits] `Method`, `Request`, and `Response` implement `Eq` and `Debug`.
3. r[std-http.request.defaults] A `Request` that gives only `url` is a `Get` with no headers, an empty body, and the provider's default timeout.
4. r[std-http.headers.pairs] Headers are a list of name and value pairs, so a repeated header such as `Set-Cookie` keeps every value, in order.
5. r[std-http.response.header] `Response` has the method `header(self, name: string) -> string?`. It returns the first value whose name equals `name`, ignoring ASCII case.
6. r[std-http.response.text] `Response` has the method `text(self) -> string`. It decodes the body as UTF-8, with U+FFFD for each byte sequence that is not valid UTF-8.

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

3. r[std-http.error.not-granted.text] The `Display` text of `NotGranted(host)` is `http access to HOST is not granted; run with --cap Http=HOST`, with the host for `HOST`.

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

1. r[std-http.trait.decl] `std.http` declares the host capability trait `Http`, with the one method `fn send!(mut self, request: Request) -> Result[Response, HttpError]`. Code imports it, as in `use std.http.Http`.
2. r[std-http.send] `send!(request)` sends `request`, and completes with the whole response.
3. r[std-http.send.status] Every status, 404 and 500 included, is an `.Ok` response, and the caller reads it from `status`.
4. r[std-http.send.redirect] `send!` follows up to 10 redirects.
5. r[std-http.send.timeout] A `timeout` of `.None` means the provider's default. A request that takes longer than its timeout completes with `.Err(HttpError.Timeout)`.
6. r[std-http.send.cancel] Cancelling the suspension of `send!` aborts the request, by [`req.cancel.external-abort`](../lang/11-requirements-and-suspension.md#r-req.cancel.external-abort).
7. r[std-http.send.mut] `send!` takes `mut self`, so `Http` is a mutable requirement trait, and a provider may record what it sends.
8. r[std-http.bodies] A body is a whole list of bytes. `std.http` has no streaming body.
9. r[std-http.helper.send] `std.http` declares `pub fn send!(request: Request) -> Result[Response, HttpError] $ Http`, which calls `send!(request)` on the `Http` provider that covers the call.
10. r[std-http.helper.get] `std.http` declares `pub fn get!(url: string) -> Result[Response, HttpError] $ Http`, which sends `Request { url: url }` the same way.

> **Why.** A status is data about a request that worked, as with `fetch`
> and Go, and as a non-zero exit status is for `Process`. Header pairs
> keep a repeated header, which a map would lose.

## Scripted Http

`ScriptedHttp` is the deterministic `Http` provider. It answers each URL
from a table, and records every request:

```text
use std.http.{Http, Response, ScriptedHttp, get}
use std.testing.assert_equal

fn status!(url: string) -> u16 $ Http:
    match get!(url):
        .Ok(response) => response.status
        .Err(_) => 0

tests:
    it("reads the scripted status"):
        let mut http = ScriptedHttp::new({"https://example.com/": Response { status: 204, headers: [], body: [] }})
        $.with(Http=http):
            assert_equal(status!("https://example.com/"), 204, reason="scripted")
        assert_equal(http.sent().len(), 1, reason="one request")
```

1. r[std-http.scripted.decl] `std.http` declares `ScriptedHttp`, which implements `Http` and `Debug`, with private fields. Code imports it, as in `use std.http.ScriptedHttp`.
2. r[std-http.scripted.new] `ScriptedHttp::new(responses: Map[string, Response]) -> mut ScriptedHttp` returns a provider that answers each URL in `responses`.
3. r[std-http.scripted.send] `send!(request)` returns `.Ok` of the response that `responses` holds for `request.url`, matched by its exact text, whatever the method.
4. r[std-http.scripted.unknown] For a URL that `responses` lacks, `send!` returns `.Err(HttpError.Connect(message))`, and `message` is `ScriptedHttp has no response for URL`, with the URL.
5. r[std-http.scripted.sent] `sent(self) -> List[Request]` returns every request that the provider received, in order, the unanswered ones included.
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

See also: [Capability Grants](../cli/command-line.md#capability-grants),
[Process](process.md), [Time](time.md#duration).
