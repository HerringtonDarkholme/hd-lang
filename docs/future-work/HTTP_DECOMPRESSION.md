# HTTP Response Decompression

**Status:** implemented by [the `std.http` specification](../../spec/std/http.md);
design accepted 2026-10-28.

## Proposal

The Node and native Wasmtime transports advertise gzip and deflate. Every host
returns a decoded response before `Response` reaches Wasm, so HTTP decompression
adds no inflater to a program.

```text
use std.http.{Client, Http, HttpError, Request, Url}
use std.time.Clock

fn release_notes!(url: string) -> Result[string, HttpError] $ Http + Clock:
    let mut client = Client::new()
    request := Request::get(Url::parse(url)?)
    response := client.send!(&request)?
    .Ok(response.text()?)
```

`Request.send!` requires only `Http`. `Client.send!` retains its existing
`Http + Clock` requirements because its cookie jar evaluates expiry and
`Max-Age`; decompression adds no authority. Timeout policy is outside this
decompression design.

## Problems Solved

| Program | Today | Proposal |
| --- | --- | --- |
| Read a compressed JSON API | `response.json()` sees gzip bytes and fails | the raw transport advertises and the host decodes before the body reader |
| Read through every supported host | native hosts preserve encoded bytes today | every host exposes the same decoded response contract |
| Avoid charging each HTTP program for inflate | a Wasm decoder would add about 69 KB | the host decodes with its platform library; Wasm adds zero decoder bytes |
| Receive corrupt compressed data | often appears as lossy text | `send!` returns a typed content-decoding error |
| Bound a decompression bomb | decoded size has no common limit | the host stops at 64 MiB with `OutputTooLarge` |

## Public Shape

| Item | Purpose |
| --- | --- |
| `Client.send!`, `Request.send!` | decoded buffered response on every host |
| `HttpError.InvalidContentEncoding(encoding)` | a coding was unsupported or malformed |
| `HttpError.OutputTooLarge` | the decoded response exceeded 64 MiB |

This proposal does not add `std.compression`. A separate proposal may add
general gzip/zlib functions for non-HTTP work such as reading a `.gz` file,
but that API must justify its own linked size and is not reachable from HTTP.

## Client Rules

Before every raw Node or native Wasmtime request, the transport adds
`Accept-Encoding: gzip, deflate` only when the request has neither
`Accept-Encoding` nor `Range`. Fetch and XHR control that header themselves, so
their adapters add nothing. An `Http` provider sees exactly the caller's
headers, and an explicit request header is preserved without changing the
decoded-response contract.

On every response, the host parses every `Content-Encoding` token. It
decodes supported stacked codings in reverse order. `identity` is a no-op. If
every coding is supported, it removes `Content-Encoding` and `Content-Length`;
those fields described the encoded representation.

Because the universal contract permits no encoded `Response` to escape, a
raw-transport host returns `InvalidContentEncoding` for an unsupported coding
or a partially supported stack. Fetch-based
hosts may already have decoded a coding before the hd adapter can inspect it;
the adapter treats that platform-decoded representation as the decoded result
and removes stale representation headers.

A `Range` request asks for `identity`; if the server still returns a supported
coding, the host decodes it. A HEAD response and 1xx, 204, or 304 responses have
no body to decode. HTTP `deflate` means the RFC 1950 zlib wrapper; a raw DEFLATE
stream is invalid.

Decoding is eager in the host while it buffers the response. The host stops
and returns `HttpError.OutputTooLarge` once decoded output would exceed 64 MiB.
`bytes`, `text`, `json`, and `form` therefore all see the same bounded decoded
bytes. A corrupt supported stream makes `send!` return
`HttpError.InvalidContentEncoding(encoding)`; no partially decoded `Response`
is constructed.

The provider always returns the decoded representation. `ScriptedHttp` treats
its queued response as already decoded and does no compression work. This
keeps deterministic tests independent of a decoder while making the real
host, not Wasm, own decoding policy.

### Exact Host Behavior

| Host | Every send |
| --- | --- |
| Node CLI (`host/http.mjs`) | advertises on the wire; Node `zlib` decodes gzip/zlib stacks, enforces the cap, and normalizes headers |
| Wasmtime | advertises on the wire; native `flate2` decodes gzip/zlib stacks, enforces the cap, and normalizes headers |
| Browser playground (`XMLHttpRequest`) | the browser-decoded body is capped; the adapter removes stale encoding/length headers |
| Cloudflare/Fetch server host | the Fetch-decoded body is capped; the adapter removes stale encoding/length headers |
| Vercel Fetch adapter | the Fetch-decoded body is capped; the adapter removes stale encoding/length headers |
| `ScriptedHttp` | the queued response is interpreted as decoded and remains deterministic |

## Differences From Today's API

- `Client.send!` changes from encoded to decoded bodies for recognized
  content codings.
- `Request.send!` follows the same decoded-body contract as `Client.send!`.
- `HttpError` gains `InvalidContentEncoding(encoding)` and
  `OutputTooLarge`.
- `Response`, `BodyError`, and body readers do not change.
- No request body is compressed.

## Cost

| Component | Cost |
| --- | --- |
| std code | error and decoded-size-cap plumbing only |
| Host code | Node built-in `zlib`; native `flate2`; Fetch/XHR use platform decoding |
| Runtime | host buffering plus host decoding; no second Wasm-side pass |
| Memory | the host's encoded input and decoded response buffer |
| Program Wasm | **zero decoder bytes**; only ordinary HTTP call glue |
| Web compiler | no inflater source or decoder bodies added |

The public items are stdlib tier. The implementation needs no syntax,
semantic exception, or intrinsic. It does change the ordinary `Http` provider
and wire-host response path to carry decoding failures and the size cap.

The program-Wasm budget is a measured proxy. A scratch Rust `cdylib` linked
flate2 1.1.10's multi-member gzip and zlib readers for
`wasm32-unknown-unknown`, with `opt-level=s`, LTO, one codegen unit, aborting
panics, stripping, and `gzip -9 -n`. The empty export was 75 bytes raw /
91 bytes gzip; the inflater was 68,869 / 40,042. The delta was
**68,794 raw / 39,951 gzip**.
Those numbers reject in-Wasm HTTP decoding: its decoder alone was almost the
size of the 71 KB JSON program used for comparison. A future non-HTTP
`std.compression` proposal must report its own hd before/after numbers.

## Appendix A: Stress Test

| Case | Result under this proposal |
| --- | --- |
| GitHub-style gzip JSON | the raw transport adds the header and `response.json()` reads decoded bytes |
| Explicit `Accept-Encoding: gzip` | preserved; recognized response gzip is still decoded |
| `Range` request | a raw transport requests `identity`; any returned supported coding is still decoded |
| `Content-Encoding: gzip, deflate` | zlib is decoded first, then gzip |
| `Content-Encoding: br, gzip` | no partial transform; the host returns `InvalidContentEncoding` |
| Concatenated gzip members | all members concatenate, and each trailer is checked |
| Bad gzip CRC or truncated deflate | `send!` returns `InvalidContentEncoding` |
| HEAD, 204, or 304 with an empty body | no decoder runs for an empty body |
| Redirect with a compressed body | each hop is decoded and bounded before the client continues |
| Scripted response | its queued value is already decoded |
| Decoded output reaches 64 MiB | the final byte at the limit succeeds |
| Decoded output exceeds 64 MiB | the host stops with `OutputTooLarge` and constructs no `Response` |
| Unsupported coding on a raw host | the host returns `InvalidContentEncoding` instead of exposing encoded bytes |

The adversarial finding is stacked codings. Decoding only a known suffix would
make the headers lie about the transformed bytes. The design therefore decodes
the whole supported stack or returns an error without a `Response`.

## Appendix B: Prior Art

### Go `net/http`

```go
transport := &http.Transport{DisableCompression: true}
client := &http.Client{Transport: transport}
response, err := client.Get(url)
```

Go's [`Transport.DisableCompression`](https://pkg.go.dev/net/http#Transport)
documents default gzip advertisement and transparent decoding. It does not
decode when the caller supplied `Accept-Encoding`. Hd follows the decoded
default, but an explicit header does not change the returned representation.

### reqwest

```rust
let client = reqwest::Client::builder()
    .gzip(true)
    .deflate(true)
    .build()?;
let text = client.get(url).send().await?.text().await?;
```

reqwest's [`ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html)
adds an encoding header only without `Accept-Encoding` or `Range`, decodes by
`Content-Encoding`, and removes that field plus `Content-Length`. Hd adopts
those header rules, fixes the portable set at gzip and deflate, and exposes no
per-client decoding toggle.

### Fetch

```javascript
const response = await fetch(url);
const text = await response.text();
```

The [Fetch Standard](https://fetch.spec.whatwg.org/#http-network-or-cache-fetch)
handles content codings before exposing body bytes and requests `identity` for
ranges. Browsers also control `Accept-Encoding`. Hd adopts decoding before
body readers and makes that decoded representation the only portable response.

## Appendix C: Alternatives

### Client Boolean

`client.decompression(false)` is close to reqwest. It makes identical
`send!` code return different body representations based on earlier mutable
state. That violates the consistent-behavior rule and is rejected.

### Header As The Opt-Out

Go treats an explicit `Accept-Encoding` as ownership of decoding. This adds no
API, but a header-setting helper far from the body read changes its meaning.
Hd keeps explicit headers and still returns decoded supported codings under
every send.

### Decode In Wasm

A shared Wasm decoder could make the same raw transport policy available to
Node and native hosts. It cannot run on Fetch/XHR hosts because those platforms
have already decoded the response, and it costs about 69 KB raw / 40 KB gzip
when linked. Host-side decoding gives the default path the same decoded-body
contract at zero decoder bytes.

### Brotli Now

Brotli is common in browsers and optional in reqwest. The accepted portable
set is gzip and deflate only; a later Brotli addition needs its own host-parity
and size case.

## Appendix D: Today In hd

`Client.send!` in [`lib/std/http.hd`](../../lib/std/http.hd) manages cookies
and redirects, then returns the provider's response bytes. `Body.bytes()` is a
read-once copy with JSON, text, and form readers layered above it.

The header enum already includes `AcceptEncoding` and `ContentEncoding`.
[`spec/std/http.md`](../../spec/std/http.md#client) specifies no decoding, and
the native host test `client_keeps_status_headers_bytes_and_does_not_decode_gzip`
records raw gzip behavior. The Node and native hosts currently preserve those
bytes; XHR and Fetch already expose decoded bytes. There is no inflater in
`lib/std`.

## Appendix E: Details And Phases

- **Errors:** malformed or unsupported data is
  `HttpError.InvalidContentEncoding`; decoded output above 64 MiB is
  `HttpError.OutputTooLarge`.
- **Ownership:** the host constructs one final body representation before
  Wasm receives the response; read-once body ownership is unchanged.
- **Cancellation:** decoding belongs to the bounded host request and completes
  or fails before the host posts its result.
- **Parity:** every host returns decoded gzip/deflate through `Client.send!`;
  `Request.send!` has the same contract.
- **Streaming:** a future host streaming reader applies the same decoding and
  header normalization before yielding bytes to Wasm.
- **Phase 1:** add errors to the provider contract; implement Node/native
  decoding, the 64 MiB cap, and stacked, corrupt, and oversized fixtures.
- **Phase 2:** normalize browser/Fetch headers, enforce the cap, and add
  cross-host parity cases.
- **Phase 3:** add raw-transport advertisement, redirects, and
  provider-header tests.
- **Rollback:** remove host decoding and its two error cases; no lower
  compression module remains coupled to HTTP.

## Parse Log

The one `text` block parsed successfully with `scripts/hd-conformance.sh parse`
on 2026-10-28. Parsing checks syntax only. The implementation and conformance
fixtures provide the stronger signature and usage checks.
