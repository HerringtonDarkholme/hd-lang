# HTTP Response Decompression

**Status:** proposal, 2026-10-28; domain: `std.http` and a new
`std.compression`; nothing here is accepted behavior.

## Proposal

`Client.send!` advertises gzip and deflate and returns a response whose body
readers decode them. `Client.send_encoded!` is the explicit wire-body path.
The decoder lives below HTTP in `std.compression`.

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
2. **Decoded body limit:** fixed 64 MiB or unbounded like today's buffered
   body? **Recommendation:** 64 MiB, reported as `InflateError.OutputTooLarge`.
   Larger bodies should use the streaming design rather than one allocation.
3. **Brotli in the first release:** yes or no? **Recommendation:** no. Gzip and
   deflate share one DEFLATE core; Brotli adds a second decoder whose Wasm cost
   has not been justified.

## Problems Solved

| Program | Today | Proposal |
| --- | --- | --- |
| Read a compressed JSON API | `response.json()` sees gzip bytes and fails | `Client.send!` advertises and decodes before the body reader |
| Save the exact server payload | Client code has no explicit raw mode | `send_encoded!` preserves bytes and encoding headers |
| Reuse inflate outside HTTP | no std inflater | `std.compression` exposes bounded gzip and zlib decoders |
| Receive corrupt compressed data | often appears as lossy text | the body reader returns a typed inflate error |

## Public Shape

| Item | Purpose |
| --- | --- |
| `Client.send!` | decoded buffered response, the default |
| `Client.send_encoded!` | encoded buffered response with wire headers intact |
| `gzip_decode(bytes, max_output)` | gzip members, including checksum checks |
| `zlib_decode(bytes, max_output)` | RFC 1950 zlib wrapper around DEFLATE |
| `InflateError` | malformed input or a decoded-size limit |
| `BodyError.Decompression` | maps a body coding and its `InflateError` into HTTP |

`std.compression` depends only on byte collections. `std.http` depends on it;
the compression module never imports HTTP.

## Client Rules

Before every request, decoded `send!` adds `Accept-Encoding: gzip, deflate`
only when the request has neither `Accept-Encoding` nor `Range`.
`send_encoded!` adds nothing. An explicit request header is preserved.

On a final response, decoded `send!` parses every `Content-Encoding` token.
It decodes supported stacked codings in reverse order. `identity` is a no-op.
If every coding is supported, it removes `Content-Encoding` and
`Content-Length`; those fields described the encoded representation.

An unsupported coding leaves the entire body and both headers unchanged,
matching reqwest rather than partially decoding a stack. `send_encoded!`
always leaves them unchanged. Cookies and redirects are processed before the
final response is wrapped, so redirect bodies are not inflated.

A `Range` request, a 206 response, a HEAD response, and 1xx, 204, or 304
responses are not wrapped. Their representation headers remain intact. HTTP
`deflate` means the RFC 1950 zlib wrapper; a raw DEFLATE stream is invalid.

Decoding is lazy at the read-once `Body`: `bytes`, `text`, `json`, and `form`
all see decoded bytes and can return `BodyError.Decompression`. This retains
the status and headers when compressed data is corrupt and fits the planned
streaming decoder. The first read still consumes the body on either success or
failure.

`Request.send!` remains the low-level provider terminal and returns encoded
bytes. Only `Client.send!` has the new default. This keeps the provider ABI and
scripted transports independent of compression policy.

## Differences From Today's API

- `Client.send!` changes from encoded to decoded bodies for recognized
  content codings.
- `send_encoded!` is new and preserves today's client result.
- `BodyError` gains `Decompression(encoding, error)`.
- `Request.send!`, `Http.send!`, `Response`, and `ScriptedHttp` keep their
  existing wire behavior.
- No request body is compressed.

## Cost

| Component | Cost |
| --- | --- |
| std code | one DEFLATE core, gzip/zlib framing, HTTP header glue |
| Compile time | decoder bodies compile only when reachable |
| Runtime | O(encoded + decoded bytes); one 32 KiB history window |
| Memory | decoded body plus the history window, capped at 64 MiB |
| Program Wasm | budget +70 KB raw / +40 KB gzip for the full decoder |
| Programs using only encoded sends | no decoder functions are reachable, so no inflater is linked |
| Web compiler | std source increases its packed input; implementation must remain below the 2 MB gzip gate |

Every proposed item is stdlib tier. The implementation needs no syntax,
semantic exception, intrinsic, or host ABI change.

The program-Wasm budget is a measured proxy. A scratch Rust `cdylib` linked
flate2 1.1.10's multi-member gzip and zlib readers for
`wasm32-unknown-unknown`, with `opt-level=s`, LTO, one codegen unit, aborting
panics, stripping, and `gzip -9 -n`. The empty export was 75 bytes raw /
91 bytes gzip; the inflater was 68,869 / 40,042. The delta was
**68,794 raw / 39,951 gzip**.
The hd implementation must report its own before/after numbers.

## Appendix A: Stress Test

| Case | Result under this proposal |
| --- | --- |
| GitHub-style gzip JSON | `send!` adds the header and `response.json()` reads decoded bytes |
| Explicit `Accept-Encoding: gzip` | preserved; recognized response gzip is still decoded |
| `Range` request | no automatic encoding header; a 206 body and its representation headers stay encoded |
| `Content-Encoding: gzip, deflate` | zlib is decoded first, then gzip |
| `Content-Encoding: br, gzip` | no partial transform; body and headers remain encoded |
| Concatenated gzip members | all members concatenate, and each trailer is checked |
| Bad gzip CRC or truncated deflate | first body read returns `BodyError.Decompression` |
| 1 KiB expands past 64 MiB | first body read returns `OutputTooLarge(67108864)` |
| HEAD, 204, or 304 with an empty body | no decoder runs for an empty body |
| Redirect with a compressed body | body is skipped; only the final response is wrapped |
| Scripted response | the client applies the same policy after `ScriptedHttp.send!` |
| Save a signed encoded artifact | `send_encoded!` preserves bytes, `Content-Encoding`, and `Content-Length` |

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

### Inflater

```rust
let mut decoder = flate2::read::MultiGzDecoder::new(input);
decoder.take(limit).read_to_end(&mut output)?;
```

[`miniz_oxide`](https://docs.rs/miniz_oxide/latest/miniz_oxide/inflate/index.html)
provides raw-DEFLATE and zlib decoding, including output-limited entry points.
Hd takes the bounded decoder shape and adds gzip framing and trailer checks in
`std.compression`.

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

### Decode In The Host

Node Fetch may decode already, while native transports can expose wire bytes.
Depending on host behavior would violate engine parity and make raw access
unreliable. The shared std layer owns the policy.

### Brotli Now

Brotli is common in browsers and optional in reqwest, but it shares no DEFLATE
core. Without a measured Wasm budget it is excluded from the first release.

## Appendix D: Today In hd

`Client.send!` in [`lib/std/http.hd`](../../lib/std/http.hd) manages cookies
and redirects, then returns the provider's response bytes. `Body.bytes()` is a
read-once copy with JSON, text, and form readers layered above it.

The header enum already includes `AcceptEncoding` and `ContentEncoding`.
[`spec/std/http.md`](../../spec/std/http.md#client) specifies no decoding, and
the native host test `client_keeps_status_headers_bytes_and_does_not_decode_gzip`
records raw gzip behavior. There is no inflater in `lib/std`.

## Appendix E: Details And Phases

- **Errors:** malformed supported data is `BodyError.Decompression`; unknown
  codings remain raw and discoverable in headers.
- **Ownership:** decoding replaces a read-once body's payload without changing
  copies' shared consumption state.
- **Cancellation:** buffered inflate is synchronous; the enclosing send's
  cancellation rules do not change.
- **Parity:** Node and native providers both return wire bytes; std alone
  applies decoding.
- **Streaming:** the future reader wraps its source in the same decoder and
  enforces the same output limit incrementally.
- **Phase 1:** `std.compression` gzip/zlib functions, vectors, corrupt inputs,
  bomb limit, and linked-size measurement.
- **Phase 2:** encoded `Body` state, reverse coding lists, header normalization,
  and read-once errors.
- **Phase 3:** `Client.send!` advertisement plus `send_encoded!`, redirects,
  scripted-provider tests, and Node/native parity.
- **Rollback:** remove the client wrapper and new terminal; the lower
  compression module remains independently useful.

## Parse Log

The one `text` block parsed successfully with `scripts/hd-conformance.sh parse`
on 2026-10-28. Parsing checks syntax only. The checked package provides the
stronger signature and usage check; names remain unavailable in std until the
owner accepts and the implementation lands.
