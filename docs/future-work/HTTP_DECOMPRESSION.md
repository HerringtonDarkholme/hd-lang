# HTTP Response Decompression

**Status:** proposal, 2026-10-28; domain: `std.http` and its host providers;
nothing here is accepted behavior.

## Proposal

`Client.send!` advertises gzip and deflate and asks the host for a decoded
response. `Client.send_encoded!` is the explicit wire-body path on hosts that
can expose one. Decoding happens in the host before `Response` reaches Wasm;
HTTP decompression adds no inflater to a program.

```text
use std.http.{Client, Http, HttpError, Request, Url}
use std.time.Clock

fn release_notes!(url: string) -> Result[string, HttpError] $ Http + Clock:
    let mut client = Client::new()
    request := Request::get(Url::parse(url)?)
    response := client.send!(&request)?
    .Ok(response.text()?)
```

The checked signatures and examples are in
[`http-decompression/`](http-decompression/). Run `hd fmt --check`,
`hd lint --all`, and `hd check --all` there.

## Questions For The Owner

1. **Opt-out:** separate `send_encoded!` or a client flag?
   **Recommendation:** a separate terminal. Its name makes the body
   representation stable; a boolean would change every body reader's meaning.
2. **Brotli in the portable guarantee:** yes or no? **Recommendation:** no.
   Every host guarantees gzip and deflate. Fetch-based hosts may decode more
   codings before hd receives the response, but portable code cannot depend on
   that larger set.

## Problems Solved

| Program | Today | Proposal |
| --- | --- | --- |
| Read a compressed JSON API | `response.json()` sees gzip bytes and fails | `Client.send!` advertises and decodes before the body reader |
| Save the exact server payload | Client code has no explicit raw mode | `send_encoded!` preserves bytes and encoding headers |
| Avoid charging each HTTP program for inflate | a Wasm decoder would add about 69 KB | the host decodes with its platform library; Wasm adds zero decoder bytes |
| Receive corrupt compressed data | often appears as lossy text | `send!` returns a typed content-decoding error |

## Public Shape

| Item | Purpose |
| --- | --- |
| `Client.send!` | decoded buffered response, the default |
| `Client.send_encoded!` | encoded buffered response with wire headers intact, or `EncodedResponseUnavailable` |
| `HttpError.InvalidContentEncoding(encoding)` | a supported coding was malformed |
| `HttpError.EncodedResponseUnavailable` | this host cannot expose bytes before platform decoding |

This proposal does not add `std.compression`. A separate proposal may add
general gzip/zlib functions for non-HTTP work such as reading a `.gz` file,
but that API must justify its own linked size and is not reachable from HTTP.

## Client Rules

Before every request, decoded `send!` adds `Accept-Encoding: gzip, deflate`
only when the request has neither `Accept-Encoding` nor `Range`.
`send_encoded!` adds nothing. An explicit request header is preserved.

On a final response, the host parses every `Content-Encoding` token. It
decodes supported stacked codings in reverse order. `identity` is a no-op. If
every coding is supported, it removes `Content-Encoding` and `Content-Length`;
those fields described the encoded representation.

On raw-transport hosts, an unsupported coding leaves the entire body and both
headers unchanged, matching reqwest rather than partially decoding a stack.
Fetch-based hosts may already have decoded a coding before the hd adapter can
inspect it; the adapter treats that platform-decoded representation as the
decoded result and removes stale representation headers. Cookies and redirects
are processed before the final response is returned, so redirect bodies are
not decoded for Wasm.

A `Range` request, a 206 response, a HEAD response, and 1xx, 204, or 304
responses are not wrapped. Their representation headers remain intact. HTTP
`deflate` means the RFC 1950 zlib wrapper; a raw DEFLATE stream is invalid.

Decoding is eager in the host while it buffers the response. `bytes`, `text`,
`json`, and `form` therefore all see the same decoded bytes. A corrupt
supported stream makes `send!` return
`HttpError.InvalidContentEncoding(encoding)`; no partially decoded `Response`
is constructed.

The provider request gains a response-representation mode. `Client.send!`
uses `Decoded`; `Client.send_encoded!` uses `Encoded`; the existing
`Request.send!` uses `Native` and keeps its low-level host representation.
`ScriptedHttp` treats its queued response as the requested representation and
does no compression work. This keeps deterministic tests independent of a
decoder while making the real host, not Wasm, own decoding policy.

### Exact Host Behavior

| Host | `send!` | `send_encoded!` |
| --- | --- | --- |
| Node CLI (`host/http.mjs`) | Node `zlib` decodes gzip/zlib stacks and normalizes headers | raw `http.request` bytes and original representation headers |
| Wasmtime | native `flate2` decoding (the ureq `gzip` feature supplies the gzip path) and normalized headers | ureq decoding disabled; original bytes and representation headers |
| Browser playground (`XMLHttpRequest`) | the browser-decoded body; the adapter removes stale encoding/length headers | `HttpError.EncodedResponseUnavailable` before starting the request |
| Cloudflare/Fetch server host | the Fetch-decoded body; the adapter removes stale encoding/length headers | `HttpError.EncodedResponseUnavailable` before starting the request |
| Vercel Fetch adapter | the Fetch-decoded body; the adapter removes stale encoding/length headers | `HttpError.EncodedResponseUnavailable` before starting the request |
| `ScriptedHttp` | the queued response, interpreted as decoded | the queued response, interpreted as encoded |

There is no approximate encoded result. A method named `send_encoded!` either
returns the encoded representation or reports that the host cannot provide it.

## Differences From Today's API

- `Client.send!` changes from encoded to decoded bodies for recognized
  content codings.
- `send_encoded!` is new. It preserves today's native-client result and is
  explicitly unavailable on Fetch/XHR hosts.
- `HttpError` gains `InvalidContentEncoding(encoding)` and
  `EncodedResponseUnavailable`.
- The host-provider request gains `Native`, `Decoded`, and `Encoded` response
  modes; `Request.send!` keeps `Native`.
- `Response`, `BodyError`, and body readers do not change.
- No request body is compressed.

## Cost

| Component | Cost |
| --- | --- |
| std code | request-mode and error plumbing only |
| Host code | Node built-in `zlib`; native `flate2`; Fetch/XHR use platform decoding |
| Runtime | host buffering plus host decoding; no second Wasm-side pass |
| Memory | the host's encoded input and decoded response buffer |
| Program Wasm | **zero decoder bytes**; only ordinary HTTP call glue |
| Web compiler | no inflater source or decoder bodies added |

The public items are stdlib tier. The implementation needs no syntax,
semantic exception, or intrinsic. It does change the ordinary `Http` provider
and wire-host request to carry the representation mode.

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
| GitHub-style gzip JSON | `send!` adds the header and `response.json()` reads decoded bytes |
| Explicit `Accept-Encoding: gzip` | preserved; recognized response gzip is still decoded |
| `Range` request | no automatic encoding header; a 206 body and its representation headers stay encoded |
| `Content-Encoding: gzip, deflate` | zlib is decoded first, then gzip |
| `Content-Encoding: br, gzip` | no partial transform; body and headers remain encoded |
| Concatenated gzip members | all members concatenate, and each trailer is checked |
| Bad gzip CRC or truncated deflate | `send!` returns `InvalidContentEncoding` |
| HEAD, 204, or 304 with an empty body | no decoder runs for an empty body |
| Redirect with a compressed body | body is skipped; only the final response is wrapped |
| Scripted response | its queued value is already the representation requested by the test |
| Save a signed encoded artifact on Node/native | `send_encoded!` preserves bytes, `Content-Encoding`, and `Content-Length` |
| Save a signed encoded artifact in a browser | `send_encoded!` returns `EncodedResponseUnavailable` without a request |

The adversarial finding is stacked codings. Decoding only a known suffix would
make the headers lie about the transformed bytes. The design therefore either
decodes the whole supported stack or none of it.

## Appendix B: Prior Art

### Go `net/http`

```go
transport := &http.Transport{DisableCompression: true}
client := &http.Client{Transport: transport}
response, err := client.Get(url)
```

Go's [`Transport.DisableCompression`](https://pkg.go.dev/net/http#Transport)
documents default gzip advertisement and transparent decoding. It does not
decode when the caller supplied `Accept-Encoding`. Hd follows the default but
uses a separate terminal, so an explicit header does not silently switch the
returned representation.

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
those header rules, but replaces its boolean toggles with `send_encoded!`.

### Fetch

```javascript
const response = await fetch(url);
const text = await response.text();
```

The [Fetch Standard](https://fetch.spec.whatwg.org/#http-network-or-cache-fetch)
handles content codings before exposing body bytes and requests `identity` for
ranges. Browsers also control `Accept-Encoding`. Hd adopts decoding before
body readers, while retaining an explicit encoded-body API for tools that need
wire bytes.

## Appendix C: Alternatives

### Client Boolean

`client.decompression(false)` is close to reqwest. It makes identical
`send!` code return different body representations based on earlier mutable
state. That violates the consistent-behavior rule and is rejected.

### Header As The Opt-Out

Go treats an explicit `Accept-Encoding` as ownership of decoding. This adds no
API, but a header-setting helper far from the body read changes its meaning.
Hd keeps explicit headers and still decodes supported codings under `send!`;
`send_encoded!` is the local opt-out.

### Decode In Wasm

A shared Wasm decoder could make the same raw transport policy available to
Node and native hosts. It cannot run on Fetch/XHR hosts because those platforms
have already decoded the response, and it costs about 69 KB raw / 40 KB gzip
when linked. Host-side decoding gives the default path the same decoded-body
contract at zero decoder bytes; strict raw access is an explicit host
capability instead.

### Brotli Now

Brotli is common in browsers and optional in reqwest. Without a portable host
guarantee it is excluded from the first release.

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

- **Errors:** malformed supported data is `HttpError.InvalidContentEncoding`;
  raw hosts leave an unsupported stack untouched.
- **Ownership:** the host constructs one final body representation before
  Wasm receives the response; read-once body ownership is unchanged.
- **Cancellation:** decoding belongs to the bounded host request and completes
  or fails before the host posts its result.
- **Parity:** every host returns decoded gzip/deflate through `Client.send!`;
  strict encoded access is capability-dependent and fails explicitly.
- **Streaming:** a future host streaming reader applies the same decoding and
  header normalization before yielding bytes to Wasm.
- **Phase 1:** add representation mode and errors to the provider contract;
  implement Node/native decoding with stacked and corrupt fixtures.
- **Phase 2:** normalize browser/Fetch headers and reject encoded mode before
  network I/O; add cross-host parity cases.
- **Phase 3:** add `Client.send!` advertisement and `send_encoded!`, redirects,
  and scripted-provider mode tests.
- **Rollback:** remove the new terminal and provider mode; no lower compression
  module remains coupled to HTTP.

## Parse Log

The one `text` block parsed successfully with `scripts/hd-conformance.sh parse`
on 2026-10-28. Parsing checks syntax only. The checked package provides the
stronger signature and usage check; names remain unavailable in std until the
owner accepts and the implementation lands.
