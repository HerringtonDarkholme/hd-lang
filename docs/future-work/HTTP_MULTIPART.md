# HTTP Multipart Forms

Status: design accepted 2026-10-28; not implemented; domain: standard library HTTP messages.

This proposal addresses [#948](https://github.com/HerringtonDarkholme/hd-lang/issues/948).
It adds buffered `multipart/form-data` building and reading to `std.http`.
The [checked package](http-multipart/) contains every proposed signature and realistic usage.

## Proposal

`Multipart` is an ordered list of `MultipartPart` values.
Each part reuses `Body` for read-once bytes and `Headers` for metadata.
Encoding attaches a generated boundary, body, and `Content-Type` to an existing `Request`.
Decoding reads those headers, claims the request body, and returns fresh unread part bodies.
Both entry points are inherent `Request` methods: `set_multipart` for clients and `multipart` for servers.

```text
form := Multipart::new()
    .text("caption", "launch day")
    .file("photo", "launch.png", "image/png", photo)
request := Request::post(url).set_multipart(form)?
response := request.send!()?.ok()?
```

## Problems it solves

| Today | With this proposal |
| --- | --- |
| `set_form` can send only URL-encoded scalar data | `Multipart.text` and `Multipart.file` build one buffered upload |
| A server can read only the whole request body | `request.multipart()` returns ordered named parts |
| File metadata must be parsed by application code | Each part exposes `name`, optional `filename`, `Headers`, and `Body` |
| Custom parts need a second body/header abstraction | A `MultipartPart` literal uses the existing `Body` and `Headers` |

## How it works

### Public surface

| Item | Purpose |
| --- | --- |
| `Multipart` | ordered buffered parts; `new`, `add`, `text`, and `file` builders |
| `MultipartPart` | name, optional filename, ordinary `Headers`, and a read-once `Body` |
| `Request.set_multipart` | validate and encode once, then replace the request body and Content-Type |
| `Request.multipart` | validate Content-Type, claim the request body, and parse one multipart level |
| `MultipartError` | typed body, header, boundary, disposition, and framing failures |

The API is ordinary stdlib code. It adds no syntax, intrinsic, lang item, capability, or host call.
The full signatures are in [`http-multipart/src/multipart.hd`](http-multipart/src/multipart.hd).
That package uses a `MultipartRequest` adapter only because it cannot add inherent methods to `std.http.Request`.
The proposed std API has inherent `Request` methods and no public adapter trait.

### Client encoding

Parts remain in insertion order, including repeated names.
`text` writes UTF-8 with `text/plain; charset=utf-8`; `file` requires the caller's media type and never guesses from the filename.
`add` preserves custom headers and bytes.

`Request.set_multipart` claims every part body once, validates names, filenames, and headers, then buffers the wire body.
It rejects carriage return, line feed, and NUL in names and filenames.
It quotes `"` and `\` in `Content-Disposition` parameters and otherwise writes UTF-8.

The encoder starts with an ASCII boundary and appends a decimal counter until no part body contains a delimiter line with it.
This needs no randomness or capability and guarantees that generated framing does not collide with buffered content.
The method replaces `Content-Type` with `multipart/form-data; boundary=...` and leaves other request headers unchanged.

### Server decoding

`Request.multipart` requires a `multipart/form-data` Content-Type and a valid boundary parameter.
It belongs on the request, rather than `Body`, because the boundary is a request header.
The boundary follows RFC 2046's one-to-seventy-character limit and permitted ASCII set, with no trailing space.
It claims `request.body` before parsing, preserving the existing read-once rule.
A failure leaves the request body used, just like JSON and URL-encoded readers.

Every part must have `Content-Disposition: form-data` with one nonempty `name` parameter.
After UTF-8 and header validation, the optional `filename` is returned exactly as sent.
It is untrusted display metadata, never a path; the decoder does not strip path components.
Other part headers use `Headers`, and a caller reads Content-Type with `part.headers.get(.ContentType)`.

Each parsed payload becomes a fresh `Body`, so the caller chooses `bytes`, `text`, `json`, or another existing typed reader.
Parsing stops at the closing boundary and rejects trailing malformed framing.
Nested multipart content remains an opaque part body in this proposal.
[RFC 7578](https://www.rfc-editor.org/rfc/rfc7578) supplies the form-data framing and disposition rules.

### Errors and limits

`MultipartError.Body` retains a `BodyError`, including `Used`.
Other variants identify missing or wrong content type, bad boundaries, unsafe disposition parameters,
invalid headers, malformed framing with a byte position, or a previously consumed part.

The design buffers because `Request.body` is already buffered.
It adds no streaming handle and no second size limit.
Streaming and incremental limits remain in [Streaming Design](STREAMING_DESIGN.md).

## Cost

| Component | Cost |
| --- | --- |
| Data | one list plus one `MultipartPart` per field; part bodies retain existing shared read state |
| Encode | linear in headers and payload bytes, plus boundary collision scans |
| Decode | linear in body bytes; one buffered slice copy per returned part |
| Unused program | 0 Wasm bytes expected; ordinary std functions remain unreachable |
| Compile time | one ordinary std codec; no compiler or host change |
| Wasm size | unmeasured until implementation; report total, function count, and code section before merge |
| Web compiler | source-only std addition; measure gzip against the 2 MB cap before merge |

The checked package contains `panic("todo")` bodies for the wire codec, so it cannot measure implementation size honestly.

## Appendix A: Stress test

The checked client example translates reqwest's common text-plus-file upload and adds a custom byte part.
The server example translates Go's `NextPart` loop into an ordered list while preserving Body's typed readers.

Adversarial cases shape these rules:

- Two `photo` files retain order; no map silently drops one.
- Empty text and zero-byte files remain present parts.
- Payload text equal to an early boundary candidate forces the encoder to choose the next candidate.
- A quoted name and filename round trip after `"` and `\` escaping.
- CR, LF, or NUL in disposition parameters fails before bytes are attached to a request.
- Missing Content-Type, a missing boundary, an overlong boundary, and an unfinished closing delimiter are distinct failures.
- Invalid UTF-8 in a payload is fine for `bytes`; invalid UTF-8 in a header is not.
- A filename such as `../../report.csv` is returned unchanged and must never be opened as a path without application policy.
- A nested `multipart/mixed` part stays opaque; recursive or streaming parsing is not implied.
- Copying a parsed part shares its `Body` state, so reading either copy makes the other return `BodyError.Used`.

The design loses streaming and file-path convenience.
Those require resource lifetime, filesystem authority, cancellation, and size-limit decisions outside this issue.

## Appendix B: Prior art

### Go `mime/multipart`

```go
var body bytes.Buffer
w := multipart.NewWriter(&body)
_ = w.WriteField("caption", "launch day")
p, _ := w.CreateFormFile("photo", "launch.png")
_, _ = p.Write(photo)
_ = w.Close()
```

Go exposes sequential `Writer` and `Reader.NextPart`; each `Part` exposes `FormName`, `FileName`, and headers.
hd takes the same ordered part model but buffers it behind existing `Body` because streaming is deferred.
[Go package documentation](https://pkg.go.dev/mime/multipart).

### reqwest `multipart::Form`

```rust
let form = reqwest::multipart::Form::new()
    .text("caption", "launch day")
    .part("photo", reqwest::multipart::Part::bytes(photo)
        .file_name("launch.png")
        .mime_str("image/png")?);
let response = client.post(url).multipart(form).send().await?;
```

reqwest offers value-style `Form` and `Part` builders, plus text, bytes, filename, media type, and custom headers.
hd takes the value builder and explicit media type, while rejecting its file-path shortcut until streaming resources exist.
[Form](https://docs.rs/reqwest/latest/reqwest/multipart/struct.Form.html),
[Part](https://docs.rs/reqwest/latest/reqwest/multipart/struct.Part.html).

### Browser `FormData`

```javascript
const form = new FormData();
form.append("caption", "launch day");
form.append("photo", new Blob([photo], {type: "image/png"}), "launch.png");
await fetch(url, {method: "POST", body: form});
```

`append` preserves repeated names and accepts strings or binary blobs with a filename.
hd keeps that common path typed as `text` and `file`, and exposes arbitrary parts through `add`.
[FormData.append](https://developer.mozilla.org/en-US/docs/Web/API/FormData/append).

## Appendix C: Alternatives considered

**Put conversion methods on `Multipart`.** `form.into_request(request)` keeps the codec's two directions together.
It reads backwards at the call site and differs from `Request.set_form` and reqwest's request-builder method.

**Represent fields and files as separate enums.** This simplifies the common server match.
It duplicates `Body` and `Headers` behavior and cannot represent an ordinary custom part without another variant.

**Use a map from name to part.** This is convenient for one value per field.
It loses repeated names and wire order, both used by browser forms and RFC 7578.

**Generate a random boundary.** This usually avoids collisions but needs an authority source and remains probabilistic.
Buffered bytes let a deterministic collision scan provide a guarantee instead.

**Read files from `Path`.** A path shortcut hides filesystem authority and forces eager versus streaming policy here.
Callers may read bytes explicitly today; streaming file parts wait for the streaming design.

## Appendix D: Today in hd

[`Body`](../../lib/std/http.hd) is a shared, read-once byte value with `bytes`, `text`, `json`, and URL-form readers.
[`Headers`](../../lib/std/http.hd) is the ordered typed header collection used by both messages.
[`Request`](../../spec/std/http.md#requests-and-responses) owns those values and has value-style setters.

`Request.set_form` writes only `application/x-www-form-urlencoded` through the serde form codec.
There is no multipart boundary parser, part metadata type, file upload builder, or server part reader.
The proposal changes no accepted behavior and drops no existing API.

## Appendix E: Implementation phases

1. Add `MultipartPart`, `Multipart`, and validation helpers; test repeated names and unsafe metadata.
2. Add deterministic buffered encoding and `Request.set_multipart`; test collision selection and exact CRLF framing.
3. Add Content-Type and Content-Disposition parsing plus `Request.multipart`; test every typed error.
4. Add focused client/server conformance on Node and Wasmtime using raw wire bodies.
5. Measure optimized Wasm and web-compiler gzip; stop for owner review if the ordinary HTTP surface grows materially.

No grammar, inference, pattern, ownership, compiler-cache, host ABI, suspension, or cancellation change is needed.
If accepted, behavior belongs in `spec/std/http.md` with stdlib-tier fixtures.

## Parse log

`scripts/hd-conformance.sh parse` parses the proposal's `text` block.
This validates syntax only; `hd check` and `hd fmt --check` validate the package.
